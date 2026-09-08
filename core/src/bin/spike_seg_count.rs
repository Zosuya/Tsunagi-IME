//! spike：段選單能不能省掉要算的候選？
//!
//! # 要驗證什麼
//!
//! 現在引擎每按一鍵要維護**整句所有切法的排列組合**（`Incremental`
//! 的 `alive`，`ALIVE_LIMIT` 800 還砍不完）。段選單只問「這一段是
//! 什麼」，那是不是就不必維護整句組合，只要各段的可能性？
//!
//! 組合爆炸的來源是**乘法**：三段各有 3 種解讀，整句就有 27 種。
//! 如果選單的粒度是段，理論上只要記 3＋3＋3 = 9 種。
//!
//! # 量什麼
//!
//! 對每一句：
//!
//! | | 現在要算的 | 段選單要算的 |
//! |---|---|---|
//! | 單位 | 整句切法（`cuttings`） | 每個切點位置的段候選 |
//! | 數量 | `cuttings.len()` | Σ（每個起點有幾種段） |
//!
//! **兩者不等價**，這正是要量的重點：整句切法帶著「哪些段能共存」的
//! 資訊，拆成各段之後那個資訊就沒了。所以還要量**能不能還原**——
//! 從各段的候選能不能拼回合法的整句。
//!
//! # 用法
//!
//! ```text
//! cargo run --release -p ime-core --bin spike_seg_count
//! cargo run --release -p ime-core --bin spike_seg_count -- --verbose
//! ```

use ime_core::cutpoint::Segment;
use ime_core::input::Input;
use ime_core::language::Language;
use std::collections::BTreeMap;

#[path = "common/testdata.rs"]
mod testdata;

#[derive(Default)]
struct Tally {
    rows: usize,
    /// 整句切法的總數（現在要維護的）
    whole: usize,
    /// 各段候選的總數（段選單理論上要維護的）
    seg: usize,
    /// 最大的一句：整句切法數 / 段候選數
    max_whole: usize,
    max_seg: usize,
    /// 段候選拼回去能得到幾種整句組合（＝資訊遺失的程度）
    /// 只在數字不爆炸時算得動，爆掉就記一筆
    recomb_blown: usize,
}

fn main() {
    let verbose = std::env::args().any(|a| a == "--verbose");
    testdata::load_engine();
    let rows = testdata::load("spike_seg_count");
    if rows.is_empty() {
        eprintln!("沒有測資");
        return;
    }

    let mut by_tag: BTreeMap<String, Tally> = BTreeMap::new();
    let mut all = Tally::default();
    let mut worst: Vec<(usize, String)> = Vec::new();

    for row in &rows {
        let input = Input::from_keys(&row.keys, None);
        let cuttings: Vec<Vec<Segment>> = input.cuttings().to_vec();
        if cuttings.is_empty() {
            continue;
        }

        // 段候選：每個「起點」有哪幾種 (長度, 語言)
        //
        // 這就是段選單真正需要的東西——使用者反白某個起點的段時，
        // 清單上要列的就是這些。
        let mut per_start: BTreeMap<usize, Vec<(usize, Language)>> = BTreeMap::new();
        for c in &cuttings {
            let mut pos = 0usize;
            for s in c {
                let n = s.keys.chars().count();
                let e = per_start.entry(pos).or_default();
                // `Language` 沒有 `Ord`，用 `Vec` 去重（每個起點的候選
                // 頂多幾種，線性掃就夠）
                if !e.contains(&(n, s.lang)) {
                    e.push((n, s.lang));
                }
                pos += n;
            }
        }
        let seg_total: usize = per_start.values().map(Vec::len).sum();

        let t = by_tag.entry(row.tag.clone()).or_default();
        t.rows += 1;
        t.whole += cuttings.len();
        t.seg += seg_total;
        t.max_whole = t.max_whole.max(cuttings.len());
        t.max_seg = t.max_seg.max(seg_total);
        all.rows += 1;
        all.whole += cuttings.len();
        all.seg += seg_total;
        all.max_whole = all.max_whole.max(cuttings.len());
        all.max_seg = all.max_seg.max(seg_total);

        // **資訊遺失有多嚴重**：從各段候選自由組合，能拼出幾種走法？
        //
        // 走法 = 從位置 0 一路跳到結尾，每一步挑一個 (起點,長度)。
        // 這個數字比 `cuttings.len()` 大很多的話，代表拆成段之後
        // 多出了一堆**引擎根本不承認**的組合——那些組合要是被使用者
        // 選到，輸出就是引擎沒驗證過的東西。
        let n = row.keys.chars().count();
        let paths = count_paths(&per_start, n);
        match paths {
            Some(p) if p > cuttings.len() => {
                if verbose && worst.len() < 40 {
                    worst.push((
                        p.saturating_sub(cuttings.len()),
                        format!(
                            "  {:<28} 整句 {:>4} 種 → 段拼回 {:>6} 種（多 {}）",
                            row.keys,
                            cuttings.len(),
                            p,
                            p - cuttings.len()
                        ),
                    ));
                }
            }
            None => {
                t.recomb_blown += 1;
                all.recomb_blown += 1;
            }
            _ => {}
        }
    }

    println!("段選單能不能省掉要算的候選？\n");
    println!("整句切法 = 現在 `Incremental` 維護的；段候選 = 段選單理論上需要的\n");
    println!(
        "{:<16} {:>5} │ {:>9} {:>8} │ {:>9} {:>8} │ {:>6}",
        "節", "句數", "整句·平均", "最多", "段候選·平均", "最多", "省"
    );
    println!("{}", "─".repeat(74));
    for (tag, t) in &by_tag {
        print_row(tag, t);
    }
    println!("{}", "─".repeat(74));
    print_row("總計", &all);

    if all.rows > 0 {
        let w = all.whole as f64 / all.rows as f64;
        let s = all.seg as f64 / all.rows as f64;
        println!(
            "\n平均一句：整句切法 {:.1} 種 → 段候選 {:.1} 種（**省 {:.0}%**）",
            w,
            s,
            100.0 * (w - s) / w
        );
        println!(
            "最壞的一句：整句 {} 種 → 段候選 {} 種",
            all.max_whole, all.max_seg
        );
    }
    println!("\n拼回去會爆炸（>10^9 種走法）的句數：{}", all.recomb_blown);

    if verbose && !worst.is_empty() {
        worst.sort_by(|a, b| b.0.cmp(&a.0));
        println!("\n拆成段之後多出最多「引擎沒承認過」組合的句子：");
        for (_, s) in worst.iter().take(15) {
            println!("{s}");
        }
    }
}

fn print_row(name: &str, t: &Tally) {
    let avg = |sum: usize| {
        if t.rows == 0 {
            0.0
        } else {
            sum as f64 / t.rows as f64
        }
    };
    let w = avg(t.whole);
    let s = avg(t.seg);
    let save = if w > 0.0 {
        format!("{:.0}%", 100.0 * (w - s) / w)
    } else {
        "─".into()
    };
    println!(
        "{:<16} {:>5} │ {:>9.1} {:>8} │ {:>9.1} {:>8} │ {:>6}",
        name, t.rows, w, t.max_whole, s, t.max_seg, save
    );
}

/// 從各段候選自由組合，有幾種走完整串的方式？
///
/// 動態規劃：`ways[i]` = 走到位置 `i` 的方法數。爆掉（超過 10^9）
/// 就回 `None`——那代表拆成段之後組合數比原本還誇張。
fn count_paths(per_start: &BTreeMap<usize, Vec<(usize, Language)>>, n: usize) -> Option<usize> {
    const CAP: usize = 1_000_000_000;
    let mut ways = vec![0usize; n + 1];
    ways[0] = 1;
    for i in 0..n {
        if ways[i] == 0 {
            continue;
        }
        let Some(cands) = per_start.get(&i) else {
            continue;
        };
        for (len, _) in cands {
            let j = i + len;
            if j <= n {
                ways[j] = ways[j].saturating_add(ways[i]);
                if ways[j] > CAP {
                    return None;
                }
            }
        }
    }
    Some(ways[n])
}
