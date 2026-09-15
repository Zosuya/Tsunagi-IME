//! 逐層量 `preload` 每一層的私有記憶體成本。
//!
//! # 為什麼需要這支
//!
//! 2026-09-15 的相容性測試 M1 量到記事本私有增量 36MB，而 2026-09-02
//! 首測是 6.6MB。但 `bench_dict --hold` 的獨立行程仍然只有 6.1MB
//! ——跟當年一模一樣。
//!
//! 差別在**兩者載的東西不同**：`bench_dict` 只載三本詞庫，而宿主走
//! 的是 `preload`，多載 bigram 語言模型與字頻表。字頻表是
//! `HashMap<char, u32>`、讀音別字頻是 `HashMap<String, u32>`，
//! **都不是 mmap，每個行程各背一份**。
//!
//! 這支把 `preload` 拆開，一層一層印私有記憶體，讓那 26MB 現形。
//! 判準看「私有工作集」——mmap 的頁不算私有，堆配置才算。
//!
//!     cargo run --release -p ime-core --bin spike_mem_layers

use std::path::Path;

/// 目前行程的私有記憶體（bytes）。
///
/// Windows 用 `PROCESS_MEMORY_COUNTERS_EX.PrivateUsage`（＝工作管理員
/// 的「認可大小」），它涵蓋堆配置但不含 mmap 的唯讀檔案頁——正是我們
/// 要區分的兩件事。非 Windows 回 0（這支是為 Windows 的量測寫的）。
#[cfg(windows)]
fn private_bytes() -> u64 {
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
        private_usage: usize,
    }
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(h: isize, c: *mut Counters, cb: u32) -> i32;
    }
    let mut c = Counters {
        cb: std::mem::size_of::<Counters>() as u32,
        ..Default::default()
    };
    unsafe {
        if K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) == 0 {
            return 0;
        }
    }
    c.private_usage as u64
}

#[cfg(not(windows))]
fn private_bytes() -> u64 {
    0
}

fn mb(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0
}

/// 量一段程式跑完之後私有記憶體漲了多少。
fn step(name: &str, prev: &mut u64, f: impl FnOnce()) {
    let t = std::time::Instant::now();
    f();
    let now = private_bytes();
    let delta = now.saturating_sub(*prev);
    println!(
        "{name:<28} +{:>7.1} MB   （累計 {:>7.1} MB，{}ms）",
        mb(delta),
        mb(now),
        t.elapsed().as_millis()
    );
    *prev = now;
}

fn main() {
    let data = Path::new("data");
    if !data.exists() {
        eprintln!("找不到 data/，請在專案根目錄執行");
        std::process::exit(1);
    }

    let mut prev = private_bytes();
    println!("起點  {:.1} MB\n", mb(prev));

    // 順序照 `preload`（core/src/lib.rs），兩個引擎都開。
    step("english::load", &mut prev, || {
        ime_core::english::load(data);
    });
    step("dict::load_bopomofo", &mut prev, || {
        ime_core::dict::load_bopomofo(data);
    });

    // bigram 拆成兩半量：字頻表是 HashMap（私有），.gram 本體是 mmap（共用）。
    let mut freq = None;
    step("dict::char_freq_map", &mut prev, || {
        freq = Some(ime_core::dict::char_freq_map(data));
    });
    step("lm::load（.gram 本體）", &mut prev, || {
        ime_core::lm::load(data, freq.take().unwrap_or_default());
    });

    step("dict::load_japanese", &mut prev, || {
        ime_core::dict::load_japanese(data);
    });
    step("dict::load_connection", &mut prev, || {
        ime_core::dict::load_connection(data);
    });

    // 真的去查一次，把延遲初始化的東西（讀音別字頻等）逼出來——
    // 宿主裡使用者一打字就會碰到，不能只量載入不量第一次查詢。
    step("第一次查詢（注音）", &mut prev, || {
        let _ = ime_core::dict::word_for("su3cl3");
    });
    step("第一次查詢（日文）", &mut prev, || {
        let _ = ime_core::dict::best_kana_word("すし");
    });

    // 宿主還會載這兩層（見 `text_service::background::ensure_dict_loaded`），
    // 而 `bench_dict` 與 `preload` 都不含它們——這正是獨立行程量不到的部分。
    let user_dir = ime_core::config::user_dir();
    step("learn::load（學習層）", &mut prev, || {
        let n = ime_core::learn::load(user_dir.as_deref());
        print!("[{n} 條] ");
    });

    // 擴充包：一包一包分開量，找出是哪一包吃記憶體。
    //
    // `pack::load` 是整批覆寫式的（每次都重建全域索引），所以這裡
    // **累加**地載：先只載第一包、再載前兩包……每一步的增量就是
    // 那一包自己的成本。
    let cfg = ime_core::config::Config::load(user_dir.as_deref());
    let all = cfg.behavior.packs.clone();
    for i in 0..all.len() {
        let subset: Vec<String> = all[..=i].to_vec();
        let name = all[i].clone();
        let dir = cfg.behavior.packs_dir.clone();
        step(&format!("  + 包「{name}」"), &mut prev, || {
            ime_core::pack::load(&dir, &subset);
        });
    }

    // **重建的代價**：`pack::load` 每次都整個重建索引，不是增量的。
    // 使用者在設定頁每勾一次／取消一次包就觸發一次。這裡用**完全
    // 相同的包清單**重載五次——內容沒變，理論上不該多用任何記憶體，
    // 漲的部分就是重建留下的碎片。
    println!("\n--- 用相同清單重載五次（模擬在設定頁勾選）---");
    for i in 1..=5 {
        let dir = cfg.behavior.packs_dir.clone();
        let names = all.clone();
        step(&format!("  重載第 {i} 次"), &mut prev, || {
            ime_core::pack::load(&dir, &names);
        });
    }

    println!("\n合計私有 {:.1} MB", mb(prev));
    println!("PID {} — 按 Enter 結束（可在外面量）", std::process::id());
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
}
