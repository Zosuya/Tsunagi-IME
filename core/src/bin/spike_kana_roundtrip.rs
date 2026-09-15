//! **spike：驗證 VTuber 包的假名讀音打不打得出來**。
//!
//! 包裡 `ja` 條目的鍵是平假名讀音，但**不是每串假名都打得出來**——
//! 引擎的羅馬字表有它的邊界（`ゔぃ`、`すぅ`、外來語的小假名組合）。
//! 寫進包裡打不出來的話那條就是死的，而且**不會有任何錯誤訊息**。
//!
//! 這支把每個讀音走一趟來回：假名 →（`reverse::keys_for`）→ 按鍵
//! →（`kana::to_kana`）→ 假名，比對頭尾一不一樣。
//!
//! 用法：
//! - `… --bin spike_kana_roundtrip -- <讀音檔>`：每行第一欄是假名讀音
//! - `… --bin spike_kana_roundtrip -- --pack <資料夾> <包名>`：把真的包
//!   載進來，逐條確認查得到（走的是 `pack::load` 本尊，不是我的推論）
//!
//! `--pack` 那條會回報每種條目的筆數，並抽查 `ja_get`——**包裡寫了
//! 但查不到的條目是靜默失效的**，只有實際查一次才知道。

use ime_core::{pack, reverse, romaji::kana};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--pack") {
        let (Some(dir), Some(name)) = (args.get(1), args.get(2)) else {
            eprintln!("用法: spike_kana_roundtrip --pack <資料夾> <包名>");
            std::process::exit(2);
        };
        check_pack(dir, name);
        return;
    }
    let path = match args.into_iter().next() {
        Some(p) => p,
        None => {
            eprintln!("用法: spike_kana_roundtrip <檔案>（每行第一欄是假名讀音）");
            eprintln!("      spike_kana_roundtrip --pack <資料夾> <包名>");
            std::process::exit(2);
        }
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("讀不到 {path}: {e}");
            std::process::exit(2);
        }
    };

    let (mut total, mut ok) = (0usize, 0usize);
    let mut bad: Vec<(String, String, String)> = Vec::new();

    for line in text.lines() {
        let line = line.trim_end_matches(['\r', '\n']);
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let reading = line.split('\t').next().unwrap_or("").trim();
        if reading.is_empty() {
            continue;
        }
        total += 1;

        // 假名 → 按鍵
        let Some(keys) = reverse::keys_for(reading) else {
            bad.push((reading.into(), "—".into(), "反查不出按鍵".into()));
            continue;
        };
        // 按鍵 → 假名，比對是否還原
        match kana::to_kana(&keys) {
            Some(back) if back == reading => ok += 1,
            Some(back) => bad.push((reading.into(), keys, format!("轉回來變成 {back}"))),
            None => bad.push((reading.into(), keys, "按鍵拼不出假名".into())),
        }
    }

    println!("讀音 {total} 筆，來回還原 {ok} 筆");
    if bad.is_empty() {
        println!("全部打得出來");
        return;
    }
    println!("\n打不出來的 {} 筆：", bad.len());
    println!("{:<28} {:<24} 問題", "讀音", "按鍵");
    for (r, k, why) in &bad {
        println!("{r:<28} {k:<24} {why}");
    }
    std::process::exit(1);
}

/// 把真的包載進來，逐條確認**查得到**。
///
/// 解析成功不等於查得到——`zh` 的注音轉不出按鍵、音節切不開都會讓
/// 那條在建索引時被丟掉，而且**不會有任何訊息**。這裡拿原始檔的每
/// 一行去問索引，對不上的列出來。
fn check_pack(dir: &str, name: &str) {
    let n = pack::load(dir, &[name.to_string()]);
    println!("載入 {name}：索引 {n} 條");
    let idx = pack::index();
    println!(
        "  en {} 條、ja {} 條、zh {} 條",
        idx.en_len(),
        idx.ja_len(),
        idx.zh_len()
    );

    let path = std::path::Path::new(dir).join(format!("{name}.txt"));
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("讀不到 {}", path.display());
        std::process::exit(2);
    };

    let (mut ja_total, mut ja_ok) = (0usize, 0usize);
    let mut miss: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim_start_matches('\u{feff}');
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let mut f = line.split('\t');
        let (Some(lang), Some(input)) = (f.next(), f.next()) else {
            continue;
        };
        let output = f.next().map(str::trim).unwrap_or("");
        if lang.trim() != "ja" {
            continue;
        }
        ja_total += 1;
        match idx.ja_get(input.trim()) {
            Some(got) if got == output => ja_ok += 1,
            Some(got) => miss.push(format!("{input} → 查到 {got}，包裡寫的是 {output}")),
            None => miss.push(format!("{input} → 查不到（包裡寫 {output}）")),
        }
    }

    println!("\nja 條目 {ja_total} 筆，查得到且內容相符 {ja_ok} 筆");
    if miss.is_empty() {
        println!("全部查得到");
        return;
    }
    println!("\n對不上的 {} 筆：", miss.len());
    for m in &miss {
        println!("  {m}");
    }
    std::process::exit(1);
}
