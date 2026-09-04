//! 產生「讀音別字頻」對照表 `data/bopomofo/char_freq_by_reading.txt`。
//!
//! # 為什麼需要這張表
//!
//! `char_freq.txt` 記的是「這個字在語料裡出現幾次」，**沒有讀音這個
//! 維度**。於是常用字只要有個冷僻的破音讀法，就會霸佔那個讀音的第一名
//! ——「吃」的 1329 次幾乎全來自 ㄔ（吃飯），卻讓它在 ㄐㄧˊ（口吃）
//! 底下排到「及」「集」前面。詞庫裡有 3580 個多音字，而字頻最高的那批
//! （的、不、一、了、來、會、行、長）全是多音字，所以這不是零星個案。
//!
//! # 怎麼算
//!
//! `BPMFMappings.txt` 的每個詞條都附了**逐字對齊**的注音，把 `word_freq`
//! 依這個對齊分攤下去，就得到「這個字念這個音時貢獻了多少詞頻」。
//! 除以該字的總量得到**佔比**，例如 `吃 ㄔ 99%`、`吃 ㄐㄧˊ 0%`。
//!
//! # 為什麼輸出佔比而不是絕對值
//!
//! 選字排序是在**同一個讀音底下**比大小，而不是所有讀音一起比。若直接
//! 輸出絕對詞頻，同一格裡「有詞條覆蓋的字」（動輒數十萬）會壓死「沒被
//! 覆蓋的罕見字」（退回字頻，只有幾百），兩種尺度混在一起反而製造新的
//! 排序錯誤。輸出佔比則是「在原本的字頻上打折」——沒被覆蓋的字維持
//! 原樣（視為 1000‰），行為不變。
//!
//! # 用法
//!
//! ```text
//! cargo run --release -p ime-core --bin gen_reading_freq
//! ```
//!
//! 輸出檔要進版控。詞庫或詞頻表更新後才需要重跑。

use std::collections::{HashMap, HashSet};
use std::path::Path;

fn main() {
    let data = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data")
        .join("bopomofo");

    // ── 每個字有哪些讀音（來源：單字表）──
    let mut readings: HashMap<char, HashSet<String>> = HashMap::new();
    let base = std::fs::read_to_string(data.join("BPMFBase.txt")).expect("讀不到 BPMFBase.txt");
    for line in base.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        let (Some(text), Some(bopomofo)) = (f.first(), f.get(1)) else {
            continue;
        };
        if let Some(c) = text.chars().next() {
            readings
                .entry(c)
                .or_default()
                .insert((*bopomofo).to_string());
        }
    }

    // ── 詞頻表 ──
    let mut word_freq: HashMap<String, u64> = HashMap::new();
    let wf = std::fs::read_to_string(data.join("word_freq.txt")).expect("讀不到 word_freq.txt");
    for line in wf.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if let (Some(w), Some(n)) = (f.first(), f.get(1)) {
            if let Ok(n) = n.parse::<u64>() {
                *word_freq.entry((*w).to_string()).or_default() += n;
            }
        }
    }

    // ── 把詞頻依逐字對齊分攤到 (字, 讀音) ──
    let mut per: HashMap<(char, String), u64> = HashMap::new();
    let mut total: HashMap<char, u64> = HashMap::new();
    let mut misaligned = 0usize;
    let map =
        std::fs::read_to_string(data.join("BPMFMappings.txt")).expect("讀不到 BPMFMappings.txt");
    for line in map.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 2 {
            continue;
        }
        let word = f[0];
        let sylls = &f[1..];
        // 字數與音節數對不上就跳過——對不齊的分攤是錯的，寧可少算
        if word.chars().count() != sylls.len() {
            misaligned += 1;
            continue;
        }
        let Some(freq) = word_freq.get(word).copied() else {
            continue;
        };
        for (c, s) in word.chars().zip(sylls.iter()) {
            *per.entry((c, (*s).to_string())).or_default() += freq;
            *total.entry(c).or_default() += freq;
        }
    }

    // ── 輸出：字 讀音 千分比 ──
    //
    // 只輸出「有詞條覆蓋」的字。沒覆蓋的字不寫進檔案，查表時退回原本的
    // 字頻（等同 1000‰），行為與現在完全一致。
    let mut lines: Vec<String> = Vec::new();
    let mut covered = 0usize;
    for (c, tot) in &total {
        if *tot == 0 {
            continue;
        }
        covered += 1;
        let Some(rs) = readings.get(c) else { continue };
        let mut rs: Vec<&String> = rs.iter().collect();
        rs.sort();
        for r in rs {
            let hit = per.get(&(*c, r.clone())).copied().unwrap_or(0);
            // 下限 1‰：讓「這個讀音沒有任何詞條」的字沉底但不歸零，
            // 同格內仍依原本的字頻分先後，不會退化成隨機
            let permille = std::cmp::max(1, hit * 1000 / tot);
            lines.push(format!("{c} {r} {permille}"));
        }
    }
    lines.sort();

    let out = data.join("char_freq_by_reading.txt");
    let mut content = String::new();
    content.push_str("# 讀音別字頻（佔比，千分比）——由 gen_reading_freq 產生，不要手改\n");
    content.push_str("# 格式：字 注音 千分比\n");
    content.push_str("# 意思：這個字的總詞頻裡，念這個音的佔多少。選字排序時用來對字頻打折。\n");
    content.push_str("# 沒列出的字＝詞條沒覆蓋到，查表時視為 1000（不打折）。\n");
    for l in &lines {
        content.push_str(l);
        content.push('\n');
    }
    std::fs::write(&out, content).expect("寫不出輸出檔");

    println!("有詞條覆蓋的字：{covered}");
    println!("輸出 (字, 讀音) 條目：{}", lines.len());
    println!("字數與音節數對不上而略過的詞條：{misaligned}");
    println!("寫入：{}", out.display());
}
