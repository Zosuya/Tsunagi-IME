//! 測資裡的英文詞，哪些不在 `en_50k`？
//!
//! # 為什麼要有這支
//!
//! `en_vowel` 那一節的失分有一半是「英文詞尾被日文吃走」
//! （`logger要` → `logゲル奧`），而根因之一是**技術詞不在 en_50k**：
//! `rank` 回 `None`、`is_word` 回 false，切法排不上去。
//!
//! §2.55.5 試過改排序（`covered` 對單音節注音算分），實測否決
//! （漏斗 -102）。剩下的路是**補詞**——但要先知道缺哪些、缺多少。
//!
//! 這支從測資的文字期望欄抽出所有英文詞，逐一問 `english::is_word`。
//!
//! # 用法
//!
//! ```text
//! cargo run --release -p ime-core --bin check_en_missing
//! cargo run --release -p ime-core --bin check_en_missing -- --pack
//! ```
//!
//! `--pack` 直接吐成領域包的格式（`en<TAB>詞`），可以貼進包檔。

#[path = "common/testdata.rs"]
mod testdata;

use std::collections::BTreeMap;

fn main() {
    testdata::load_engine();
    let as_pack = std::env::args().any(|a| a == "--pack");
    let rows = testdata::load("check_en_missing");
    if rows.is_empty() {
        eprintln!("沒有測資");
        return;
    }

    // 詞 → 出現在哪幾節（幾次）
    let mut missing: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut total_words = 0usize;
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for row in &rows {
        // 文字期望欄裡的英文詞——用 `|` 與 `_` 切開之後，純字母的那些
        for part in row.want.split(['|', '_']) {
            for w in part.split(|c: char| !c.is_ascii_alphabetic()) {
                // 一個字母的不算（`a`、`I` 那些本來就在詞典裡，而切碎的
                // 殘渣也不該當成「缺的詞」）
                if w.len() < 3 {
                    continue;
                }
                let lower = w.to_ascii_lowercase();
                if seen.insert(lower.clone()) {
                    total_words += 1;
                }
                if !ime_core::english::is_word(&lower) {
                    *missing
                        .entry(lower)
                        .or_default()
                        .entry(row.tag.clone())
                        .or_insert(0) += 1;
                }
            }
        }
    }

    if as_pack {
        println!("# 測資裡不在 en_50k 的英文詞（`check_en_missing --pack` 產生）");
        for w in missing.keys() {
            println!("en\t{w}");
        }
        return;
    }

    println!(
        "測資裡的英文詞 {} 個，**{} 個不在 en_50k**\n",
        total_words,
        missing.len()
    );
    println!("{:<18} {:>4}  出現在", "詞", "次數");
    println!("{}", "─".repeat(56));
    // 按出現次數排（多的先修）
    let mut v: Vec<(&String, usize, String)> = missing
        .iter()
        .map(|(w, tags)| {
            let n: usize = tags.values().sum();
            let where_: Vec<String> = tags
                .iter()
                .map(|(t, c)| {
                    if *c > 1 {
                        format!("{t}×{c}")
                    } else {
                        t.clone()
                    }
                })
                .collect();
            (w, n, where_.join("、"))
        })
        .collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    for (w, n, where_) in &v {
        println!("{:<18} {:>4}  {}", w, n, where_);
    }
}
