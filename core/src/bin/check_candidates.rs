//! 候選涵蓋率與排名：正解在候選裡嗎？排第幾？
//!
//! 這支回答的是切點引擎最關鍵的問題——**剩下的失分是生成問題還是
//! 排序問題**：
//!
//! | 現象 | 性質 | 怎麼解 |
//! |---|---|---|
//! | 正解不在候選裡 | 生成問題 | 改切法的生成規則 |
//! | 正解在候選但排後面 | 排序問題 | 改挑第一名的規則 |
//!
//! 兩者的修法完全不同，不先分清楚會白做工。
//!
//! 用法：cargo run -p ime-core --bin check_candidates

use ime_core::cutpoint::{self, candidates};
use std::collections::BTreeMap;

/// `check| |u vu84` → 各段的按鍵
fn expected_segs(key_expect: &str) -> Vec<String> {
    key_expect
        .split('|')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// 一種切法的各段按鍵
fn cutting_segs(c: &[cutpoint::Segment]) -> Vec<String> {
    c.iter().map(|s| s.keys.clone()).collect()
}

fn main() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data");
    ime_core::english::load(&data);

    let limit: usize = std::env::var("CAND_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);

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

    // (在候選裡, 排第一, 總數)
    let mut per_file: BTreeMap<&str, (usize, usize, usize)> = BTreeMap::new();
    let mut misses: Vec<(String, String, usize)> = Vec::new();
    let mut ranks: Vec<usize> = Vec::new();

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

            let expected = expected_segs(key_expect);
            let cands = candidates::all_cuttings(keys, limit);
            let rank = cands.iter().position(|c| cutting_segs(c) == expected);

            let e = per_file.entry(f).or_insert((0, 0, 0));
            e.2 += 1;
            match rank {
                Some(r) => {
                    e.0 += 1;
                    ranks.push(r + 1);
                    // 第一名是否正確，用 cut() 的結果判斷（那才是產品行為）
                    let first = cutting_segs(&cutpoint::cut(keys));
                    if first == expected {
                        e.1 += 1;
                    }
                }
                None if misses.len() < 20 => {
                    misses.push((want.to_string(), key_expect.replace(' ', "␣"), cands.len()));
                }
                None => {}
            }
        }
    }

    println!("=== 候選涵蓋率與第一名正確率（上限 {limit} 個候選）===\n");
    println!("  {:24} {:>12} {:>12}", "", "在候選裡", "排第一");
    let mut total = (0usize, 0usize, 0usize);
    for (f, (in_c, first, n)) in &per_file {
        total.0 += in_c;
        total.1 += first;
        total.2 += n;
        println!(
            "  {f:24} {:>5}/{:<3} {:>4.0}% {:>5}/{:<3} {:>4.0}%",
            in_c,
            n,
            *in_c as f64 / *n as f64 * 100.0,
            first,
            n,
            *first as f64 / *n as f64 * 100.0
        );
    }
    println!(
        "\n  {:24} {:>5}/{:<3} {:>4.1}% {:>5}/{:<3} {:>4.1}%",
        "總計",
        total.0,
        total.2,
        total.0 as f64 / total.2 as f64 * 100.0,
        total.1,
        total.2,
        total.1 as f64 / total.2 as f64 * 100.0
    );

    if !ranks.is_empty() {
        ranks.sort_unstable();
        let mid = ranks[ranks.len() / 2];
        let top3 = ranks.iter().filter(|r| **r <= 3).count();
        let top10 = ranks.iter().filter(|r| **r <= 10).count();
        println!(
            "\n  正解的名次：中位 {mid}，前 3 名 {top3}/{}，前 10 名 {top10}/{}",
            ranks.len(),
            ranks.len()
        );
    }

    if !misses.is_empty() {
        println!("\n=== 正解不在候選裡（生成問題）前 {} ===", misses.len());
        for (w, e, n) in &misses {
            println!("  {w}");
            println!("     期望 {e}   （候選 {n} 種）");
        }
    }
}
