//! 累加式切法的計分。**這支是產品行為的計分器**。
//!
//! `check_candidates` 量的是一次性窮舉，那不是輸入法實際會做的事——
//! 沒辦法知道使用者什麼時候要把字打完，所以候選只能一路累加。
//!
//! # 使用者定的兩個標準
//!
//! | 標準 | 要求 |
//! |---|---|
//! | **切點涵蓋** | 100%——正解一定要在候選裡，這是硬指標 |
//! | **前 3 名** | 正解要排進前三，越前面越好 |
//!
//! 涵蓋率沒到 100% 的話排序分數沒有意義：排序只能從候選裡挑，
//! 候選沒有的東西永遠選不到。
//!
//! # 為什麼比對要「輸出等價」
//!
//! 切點引擎的職責是切語言，同一個語言內部切不切**不影響輸出**：
//!
//! ```text
//! 注:ru04au04cl3t␣         見面好吃
//! 注:ru04au04 | 注:cl3t␣   切了一刀，但送出的字一模一樣
//! ```
//!
//! 所以候選要先用 `normalize` 正規化再去重，名次算的是**相異輸出**
//! 的名次。用嚴格比對的話 440 句裡有 30 句會被誤判成錯。
//!
//! 用法：cargo run --release -p ime-core --bin check_incremental

use ime_core::cutpoint::incremental::Incremental;
use ime_core::cutpoint::{normalize, rank, Segment};
use std::collections::{BTreeMap, HashSet};

const TAB: char = '\u{9}';

/// `check| |u vu84` → 各段的按鍵
fn expected_segs(key_expect: &str) -> Vec<String> {
    key_expect
        .split('|')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

fn segs_of(c: &[Segment]) -> Vec<String> {
    c.iter().map(|s| s.keys.clone()).collect()
}

fn show(segs: &[Segment]) -> String {
    segs.iter()
        .map(|s| format!("{}:{}", s.lang.short(), s.keys.replace(' ', "␣")))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// 一份測資的統計。
#[derive(Default, Clone, Copy)]
struct Stat {
    /// 正解在候選裡（硬指標，要 100%）
    covered: usize,
    /// 正解排前 3 名（使用者的標準）
    top3: usize,
    /// 正解排第 1
    first: usize,
    total: usize,
}

fn main() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data");
    ime_core::english::load(&data);
    ime_core::dict::load_bopomofo(&data);
    ime_core::dict::load_japanese(&data);

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

    let mut per_file: BTreeMap<&str, Stat> = BTreeMap::new();
    let mut ranks: Vec<usize> = Vec::new();
    let mut sizes: Vec<usize> = Vec::new();
    // 沒進前 3 名的（含完全不在候選的）
    let mut fails: Vec<(String, Option<usize>, String, String)> = Vec::new();

    for f in files {
        let Ok(content) = std::fs::read_to_string(dir.join(format!("{f}.txt"))) else {
            continue;
        };
        for line in content.lines() {
            // 只剝換行——**不能剝空白**，注音一聲的結尾空白是資料的一部分
            let line = line.trim_end_matches(['\u{d}', '\u{a}']);
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let cols: Vec<&str> = line.split(TAB).collect();
            if cols.len() < 3 {
                continue;
            }
            let (want, key_expect, keys) = (cols[0], cols[1], cols[2]);
            let expected = expected_segs(key_expect);

            let cands = rank::sort(Incremental::from_keys(keys).cuttings());
            sizes.push(cands.len());

            // 正規化後去重——名次算的是相異輸出的名次
            let mut seen = HashSet::new();
            let uniq: Vec<Vec<Segment>> = cands
                .iter()
                .map(|c| normalize(c))
                .filter(|c| seen.insert(show(c)))
                .collect();
            let r = uniq.iter().position(|c| segs_of(c) == expected);

            let e = per_file.entry(f).or_default();
            e.total += 1;
            if let Some(i) = r {
                e.covered += 1;
                ranks.push(i + 1);
                if i == 0 {
                    e.first += 1;
                }
                if i < 3 {
                    e.top3 += 1;
                }
            }
            if r.is_none() || r.is_some_and(|i| i >= 3) {
                let first = uniq.first().map(|c| show(c)).unwrap_or_default();
                fails.push((
                    want.to_string(),
                    r.map(|i| i + 1),
                    key_expect.replace(' ', "␣"),
                    first,
                ));
            }
        }
    }

    println!("=== 累加式切法 ===\n");
    println!(
        "  {:22} {:>12} {:>12} {:>12}",
        "", "切點涵蓋", "前 3 名", "第 1 名"
    );
    let mut t = Stat::default();
    for (f, s) in &per_file {
        t.covered += s.covered;
        t.top3 += s.top3;
        t.first += s.first;
        t.total += s.total;
        println!(
            "  {:22} {:>5}/{:<3} {:>3.0}% {:>5}/{:<3} {:>3.0}% {:>5}/{:<3} {:>3.0}%",
            f.replace("mixed_", ""),
            s.covered,
            s.total,
            s.covered as f64 / s.total as f64 * 100.0,
            s.top3,
            s.total,
            s.top3 as f64 / s.total as f64 * 100.0,
            s.first,
            s.total,
            s.first as f64 / s.total as f64 * 100.0
        );
    }
    println!(
        "\n  {:22} {:>5}/{:<3} {:>3.0}% {:>5}/{:<3} {:>3.0}% {:>5}/{:<3} {:>3.0}%",
        "總計",
        t.covered,
        t.total,
        t.covered as f64 / t.total as f64 * 100.0,
        t.top3,
        t.total,
        t.top3 as f64 / t.total as f64 * 100.0,
        t.first,
        t.total,
        t.first as f64 / t.total as f64 * 100.0
    );

    // 對照使用者定的標準
    println!();
    if t.covered == t.total {
        println!("  ✓ 切點涵蓋 100%——正解一定在候選裡");
    } else {
        println!(
            "  ⚠ 切點涵蓋只有 {:.1}%，有 {} 句連正解都生不出來——\
             排序分數在這之前沒有意義",
            t.covered as f64 / t.total as f64 * 100.0,
            t.total - t.covered
        );
    }
    println!(
        "  前 3 名 {:.1}%（差 {} 句）",
        t.top3 as f64 / t.total as f64 * 100.0,
        t.total - t.top3
    );

    if !ranks.is_empty() {
        ranks.sort_unstable();
        println!(
            "\n  正解名次：中位 {}，最差 {}",
            ranks[ranks.len() / 2],
            ranks.last().unwrap()
        );
    }
    if !sizes.is_empty() {
        sizes.sort_unstable();
        println!(
            "  候選數：中位 {}，最大 {}",
            sizes[sizes.len() / 2],
            sizes.last().unwrap()
        );
    }

    if !fails.is_empty() {
        println!("\n=== 沒進前 3 名的 {} 句 ===", fails.len());
        for (w, r, expect, got) in &fails {
            let pos = match r {
                Some(i) => format!("第 {i} 名"),
                None => "不在候選".to_string(),
            };
            println!("  [{pos}] {w}");
            println!("     期 {expect}");
            println!("     實 {got}");
        }
    }
}
