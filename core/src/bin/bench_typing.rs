//! 打字的每鍵耗時：接上 TSF 之前必須確認的事。
//!
//! TSF 的按鍵處理是**同步**的——`ITfKeyEventSink::OnKeyDown` 沒回來
//! 之前，那個鍵不會出現在畫面上。所以每一鍵的處理時間直接等於使用者
//! 感受到的延遲。
//!
//! 一幀是 16ms。超過那個數字使用者就感覺得到卡頓。
//!
//! # 量的是最壞情況，不是平均
//!
//! 使用者感覺得到的是卡頓那一下，不是平均值。所以每個案例都逐鍵餵，
//! 記錄**最慢的那一鍵**。
//!
//! 用法：cargo run --release -p ime-core --bin bench_typing

use ime_core::cutpoint::incremental::Incremental;
use ime_core::cutpoint::{normalize, rank};
use std::time::{Duration, Instant};

/// 常駐記憶體上限（使用者定的目標）。
const MEM_BUDGET_MB: u64 = 50;

/// 目前行程的常駐記憶體（working set），MB。
///
/// 用 Windows 的 `GetProcessMemoryInfo`。拿不到就回 `None`——
/// 這只是報告數字，量不到不該讓測試失敗。
#[cfg(windows)]
fn resident_mb() -> Option<u64> {
    // 不想為了量記憶體引入 windows crate 到 core，用 PowerShell 問。
    let out = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!("(Get-Process -Id {}).WorkingSet64", std::process::id()),
        ])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<u64>()
        .ok()
        .map(|b| b / 1024 / 1024)
}

/// Linux 版：讀 `/proc/self/status`。
///
/// **`RssFile` 要分開報**——mmap 進來的詞庫算在那一欄，而那些頁是
/// **多個宿主行程共用的**，不是每個各背一份。只看總數會低估 mmap 的
/// 價值（單行程看起來省不多，但第二個行程之後幾乎免費）。
#[cfg(not(windows))]
fn resident_mb() -> Option<u64> {
    let st = std::fs::read_to_string("/proc/self/status").ok()?;
    let field = |name: &str| -> Option<u64> {
        st.lines()
            .find(|l| l.starts_with(name))?
            .split_whitespace()
            .nth(1)?
            .parse::<u64>()
            .ok()
            .map(|kb| kb / 1024)
    };
    if let (Some(anon), Some(file)) = (field("RssAnon"), field("RssFile")) {
        println!("    （私有 {anon}MB ＋ 檔案頁 {file}MB，檔案頁多行程共用）");
    }
    field("VmRSS")
}

/// 取百分位數。`p` 是 0.0~1.0。
///
/// **p99 而不是最大值**——使用者感覺得到的是「幾乎每次都順」，
/// 偶爾一次超標可以忍受。但最大值也要看，那是最壞的卡頓。
fn percentile(sorted: &[Duration], p: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx]
}

/// 一幀的預算。超過這個數字使用者就感覺得到卡頓。
const FRAME_BUDGET: Duration = Duration::from_millis(16);

/// 各種長度與語言組合。長句與純日文是已知的最壞情況——
/// 日文沒有空白也沒有聲調鍵，沒有東西擋著切法爆炸。
const CASES: &[(&str, &str)] = &[
    ("短句（注音）", "su3cl3"),
    ("短句（英文）", "check"),
    ("中句（中英混）", "rup wu0 5p 2k7 meeting"),
    ("中句（英＋注音）", "middlewared9 "),
    ("長句（純英文 43 鍵）", "the quick brown fox jumps over the lazy dog"),
    (
        "長句（純日文 47 鍵）",
        "maikaikareanishigotowooshitsukerareteirukigasuru",
    ),
    ("長句（三語混合）", "ji3e; e; fm4supermarketa93xk7gyuunyuu"),
    (
        "極長（日文 71 鍵）",
        "maikaikareanishigotowooshitsukerareteirukigasurushikatanaiiitsumokoudesu",
    ),
    (
        "超長（日文 110 鍵）",
        "maikaikareanishigotowooshitsukerareteirukigasurushikatanaiiitsumokoudesukaishanishucchousaseraretemattakuyaruki",
    ),
    (
        "極長（三語 70 鍵）",
        "rup wu0 2k7meeting5p2k7fu4dj4boringji3e; e; fm4supermarketa93xk7gyuunyuu",
    ),
];

fn main() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data");

    // 領域包：帶參數才載，用來量「多一層查詢」的成本。
    // **要在詞庫之前**——en 的包會影響切點，得在建表前就位。
    // `--learn`：先塞一批切詞學習記錄，量「學了東西之後會不會變慢」。
    // 沒學過切詞時 `cut_any()` 一次原子讀就短路了，學了之後才會真的
    // 多查雜湊——那條路必須量過才敢說沒退步。
    let with_learn = std::env::args().any(|a| a == "--learn");
    let packs: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| a != "--learn")
        .collect();
    if with_learn {
        let n = ime_core::learn::seed_cuttings(500);
        println!(
            "切詞學習：塞了 {n} 條
"
        );
    }
    if !packs.is_empty() {
        let t = std::time::Instant::now();
        // 量測用的包放在專案的 data/packs，不必去動 %APPDATA%
        let dir = data.join("packs");
        let n = ime_core::pack::load(&dir.to_string_lossy(), &packs);
        println!(
            "領域包：{} 條，載入 {:?}
",
            n,
            t.elapsed()
        );
    }

    // ── 詞庫載入時間 ──
    //
    // 現在是 OnceLock 惰性載入，第一次呼叫才讀檔。在 TSF 裡那代表
    // **第一次打字會卡住**，所以要量它有多久、決定要不要預載。
    let t = Instant::now();
    ime_core::english::load(&data);
    let en = t.elapsed();
    let t = Instant::now();
    ime_core::dict::load_bopomofo(&data);
    let bo = t.elapsed();
    let t = Instant::now();
    ime_core::dict::load_japanese(&data);
    let ja = t.elapsed();

    println!("=== 詞庫載入（一次性）===\n");
    println!("  英文   {:>8.1?}", en);
    println!("  注音   {:>8.1?}", bo);
    println!("  日文   {:>8.1?}", ja);
    println!("  合計   {:>8.1?}", en + bo + ja);
    if en + bo + ja > Duration::from_millis(500) {
        println!("\n  ⚠ 超過 0.5 秒——TSF 裡要預載，不能等第一次打字");
    }

    println!("\n=== 每鍵耗時（最壞情況）===\n");
    println!(
        "  {:24} {:>7} {:>9} {:>8} {:>8}",
        "案例", "鍵數", "最慢一鍵", "總耗時", "候選數"
    );

    let mut worst_overall = Duration::ZERO;
    let mut over_budget = Vec::new();
    // 所有案例的每一鍵耗時，算整體 p99 用
    let mut all_keys: Vec<Duration> = Vec::new();

    for (name, keys) in CASES {
        print!("  {name:24} ");
        use std::io::Write;
        std::io::stdout().flush().ok();
        let chars: Vec<char> = keys.chars().collect();
        let mut inc = Incremental::new();
        let mut worst = Duration::ZERO;
        let mut total = Duration::ZERO;

        for &c in &chars {
            let t = Instant::now();
            inc.push(c);
            // **排序也要算進去**——TSF 每一鍵都要重畫候選視窗，
            // 所以使用者等的是「切法 + 排序 + 去重」的總和。
            let cands = rank::sort(inc.cuttings());
            let mut seen = std::collections::HashSet::new();
            let _uniq: Vec<_> = cands
                .iter()
                .map(|c| normalize(c))
                .filter(|c| seen.insert(format!("{c:?}")))
                .collect();
            let dt = t.elapsed();
            worst = worst.max(dt);
            total += dt;
            all_keys.push(dt);
        }

        let n = inc.len();
        let mark = if worst > FRAME_BUDGET { " ⚠" } else { "" };
        println!(
            "{:>7} {:>9.1?} {:>8.1?} {:>8}{mark}",
            chars.len(),
            worst,
            total,
            n
        );
        worst_overall = worst_overall.max(worst);
        if worst > FRAME_BUDGET {
            over_budget.push((*name, worst));
        }
    }

    println!("\n  一幀預算 {FRAME_BUDGET:?}，實測最慢 {worst_overall:.1?}");
    if over_budget.is_empty() {
        println!("  ✓ 全部在預算內");
    } else {
        println!("  ⚠ 有 {} 個案例超出預算：", over_budget.len());
        for (name, d) in &over_budget {
            println!("     {name}  {d:.1?}");
        }
    }

    // ── 驗收目標 ──
    //
    // 使用者定的：按鍵到候選更新 < 16ms（p99）、常駐記憶體 < 50MB。
    all_keys.sort();
    let p50 = percentile(&all_keys, 0.50);
    let p95 = percentile(&all_keys, 0.95);
    let p99 = percentile(&all_keys, 0.99);

    println!(
        "
=== 驗收目標 ===
"
    );
    println!("  按鍵延遲（{} 鍵樣本）", all_keys.len());
    println!("    p50   {p50:>9.1?}");
    println!("    p95   {p95:>9.1?}");
    println!("    p99   {p99:>9.1?}   目標 < {FRAME_BUDGET:?}");
    println!("    最大  {worst_overall:>9.1?}");
    println!(
        "    {}",
        if p99 < FRAME_BUDGET {
            "✓ p99 達標"
        } else {
            "✗ p99 未達標"
        }
    );

    match resident_mb() {
        Some(mb) => {
            println!(
                "
  常駐記憶體  {mb} MB   目標 < {MEM_BUDGET_MB} MB"
            );
            println!(
                "    {}",
                if mb < MEM_BUDGET_MB {
                    "✓ 達標"
                } else {
                    "✗ 未達標"
                }
            );
        }
        None => println!(
            "
  常駐記憶體  量不到"
        ),
    }
}
