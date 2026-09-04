//! 切點引擎的計分：拿測資的第二欄（按鍵層級的切點期望）當基準。
//!
//! 第二欄不需要任何推導，逐字元比對即可：
//!
//! ```text
//! check|_|一下      check| |u vu84      check u vu84
//!                   ↑ 這一欄
//! ```
//!
//! 判定方式：**切點集合必須完全相同**。
//!
//! 一開始用 `is_subset`（只要求「期望的每一刀都有切」，允許多切），
//! 想法是「期望切到語言段、實作可能切到音節，多切不算錯」。
//! 但那讓分數虛高 47 個百分點——`check` 被切成 `che|ck` 時，
//! 期望的那一刀確實存在，所以算過，可是英文單字已經被拆爛了。
//!
//! 多切在切點引擎裡就是錯的：`check` 是一個英文段，不該被切開。
//!
//! 用法：cargo run -p ime-core --bin check_cutpoint

use ime_core::cutpoint;
use std::collections::BTreeMap;

/// `check| |u vu84` → 切點位置集合（切在第 i 個字元之前）
fn cuts_of(key_expect: &str) -> std::collections::BTreeSet<usize> {
    let mut pos = std::collections::BTreeSet::new();
    let mut n = 0usize;
    let parts: Vec<&str> = key_expect.split('|').collect();
    for seg in &parts[..parts.len().saturating_sub(1)] {
        n += seg.chars().count();
        pos.insert(n);
    }
    pos
}

fn main() {
    // 規則三要查英文詞典（見 cutpoint::merge）。沒載入時那層補救會跳過。
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data");
    ime_core::english::load(&data);

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let files = [
        "mixed_daily",
        "mixed_otaku",
        "mixed_holdout",
        "mixed_trilingual",
        "mixed_japanese_verbs",
        "mixed_ja_en",
        "mixed_cutpoint",
        "mixed_en_bopomofo",
        "mixed_en_split",
        "mixed_en_vowel",
    ];

    let mut per_file: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let mut failures: Vec<(String, String, String)> = Vec::new();

    for f in files {
        let path = dir.join(format!("{f}.txt"));
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        for line in content.lines() {
            let line = line.trim_end_matches(['\r', '\n']);
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let cols: Vec<&str> = line.split('\t').collect();
            if cols.len() < 3 {
                continue;
            }
            let (want, key_expect, keys) = (cols[0], cols[1], cols[2]);

            let expected = cuts_of(key_expect);
            let actual = cutpoint::cut(keys);
            let actual_cuts: std::collections::BTreeSet<usize> = {
                let mut pos = std::collections::BTreeSet::new();
                let mut n = 0usize;
                for seg in &actual[..actual.len().saturating_sub(1)] {
                    n += seg.keys.chars().count();
                    pos.insert(n);
                }
                pos
            };

            let e = per_file.entry(f).or_insert((0, 0));
            e.1 += 1;
            if expected == actual_cuts {
                e.0 += 1;
            } else if failures.len() < 25 {
                let got: Vec<String> = actual
                    .iter()
                    .map(|s| format!("{}:{}", s.lang.short(), s.keys.replace(' ', "␣")))
                    .collect();
                failures.push((
                    want.to_string(),
                    key_expect.replace(' ', "␣"),
                    got.join(" | "),
                ));
            }
        }
    }

    println!("=== 切點引擎 ===\n");
    let mut total = (0usize, 0usize);
    for (f, (o, n)) in &per_file {
        total.0 += o;
        total.1 += n;
        println!(
            "  {f:24} {o:4}/{n:4}  {:5.1}%",
            *o as f64 / *n as f64 * 100.0
        );
    }
    println!(
        "\n  {:24} {:4}/{:4}  {:5.1}%",
        "總計",
        total.0,
        total.1,
        total.0 as f64 / total.1 as f64 * 100.0
    );

    if !failures.is_empty() {
        println!("\n=== 失分例子（前 {}）===", failures.len());
        for (w, e, g) in &failures {
            println!("  {w}");
            println!("     期望 {e}");
            println!("     實際 {g}");
        }
    }
}
