//! 正解切得出來嗎？兩道檢查，都必須 100%。
//!
//! **只要有一句切不出正解，排序分數再高都沒用**——排序只能從候選裡
//! 挑，候選沒有的東西永遠選不到。所以這支是排序工作的前置閘門。
//!
//! | 檢查 | 問的問題 | 強度 |
//! |---|---|---|
//! | 切點池 | 正解需要的每一刀，單獨看是可達的嗎 | 必要條件 |
//! | 完整路徑 | 正解整條是引擎走得出來的一條路嗎 | 較強 |
//!
//! 切點池只保證每一刀單獨可達，不保證那些刀能同時成立；完整路徑
//! 檢查正解的每一段都有引擎認領，且串起來正好是原按鍵串。
use ime_core::cutpoint::incremental::Incremental;
use ime_core::cutpoint::punct;
use ime_core::{bopomofo, romaji};
use std::collections::BTreeSet;
const TAB: char = '\u{9}';

fn legal(s: &str) -> bool {
    bopomofo::validity(s) == bopomofo::Validity::Valid
        || romaji::validity(s) == romaji::Validity::Valid
        || s.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ')
}

fn cuts_expect(ke: &str) -> BTreeSet<usize> {
    let mut p = BTreeSet::new();
    let mut n = 0;
    let parts: Vec<&str> = ke.split('|').collect();
    for s in &parts[..parts.len().saturating_sub(1)] {
        n += s.chars().count();
        p.insert(n);
    }
    p
}

/// 可達的切點位置：從 0 走得到、且從它走得到終點。
fn pool_of(keys: &str) -> BTreeSet<usize> {
    let cs: Vec<char> = keys.chars().collect();
    let n = cs.len();
    let is_pt = |i: usize| punct::is_punct(keys, i);
    // 標點自成一段，所以一段只有兩種：不含標點的合法段，或單一標點
    let seg_ok = |b: usize, e: usize| -> bool {
        if (b..e).any(is_pt) {
            return e == b + 1 && is_pt(b);
        }
        let s: String = cs[b..e].iter().collect();
        legal(&s)
    };
    let mut reach = vec![false; n + 1];
    reach[0] = true;
    for e in 1..=n {
        reach[e] = (0..e).any(|b| reach[b] && seg_ok(b, e));
    }
    let mut can_end = vec![false; n + 1];
    can_end[n] = true;
    for b in (0..n).rev() {
        can_end[b] = ((b + 1)..=n).any(|e| can_end[e] && seg_ok(b, e));
    }
    (1..n).filter(|&i| reach[i] && can_end[i]).collect()
}

/// 這一段有引擎認領嗎？（標點自成一段，另外判）
fn seg_claimed(seg: &str, keys: &str, off: usize) -> bool {
    if legal(seg) {
        return true;
    }
    seg.chars().count() == 1 && off < keys.chars().count() && punct::is_punct(keys, off)
}

fn main() {
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
    let (mut cov, mut tot) = (0, 0);
    let mut path_ok = 0usize;
    let mut miss: Vec<String> = vec![];
    let mut path_bad: Vec<String> = vec![];
    for f in files {
        let Ok(c) = std::fs::read_to_string(dir.join(format!("{f}.txt"))) else {
            continue;
        };
        for line in c.lines() {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let cols: Vec<&str> = line.split(TAB).collect();
            if cols.len() < 3 {
                continue;
            }
            tot += 1;
            // ── 檢查二：正解整條是走得出來的路徑嗎 ──
            let segs: Vec<&str> = cols[1].split('|').collect();
            let joined: String = segs.concat();
            if joined != cols[2] {
                path_bad.push(format!(
                    "{}  期望欄串起來 {:?} ≠ 按鍵欄 {:?}",
                    cols[0],
                    joined.replace(' ', "␣"),
                    cols[2].replace(' ', "␣")
                ));
            } else {
                let mut off = 0usize;
                let mut all = true;
                for seg in &segs {
                    if !seg_claimed(seg, cols[2], off) {
                        all = false;
                        if path_bad.len() < 15 {
                            path_bad.push(format!(
                                "{}  段 {:?} 無引擎認領",
                                cols[0],
                                seg.replace(' ', "␣")
                            ));
                        }
                        break;
                    }
                    off += seg.chars().count();
                }
                if all {
                    path_ok += 1;
                }
            }

            // ── 檢查一：切點池 ──
            let want = cuts_expect(cols[1]);
            // **用累加式引擎的切點**，不是理論上的可達性——
            // 產品實際會做的是累加，計分器要量的也是那個。
            let pool = Incremental::from_keys(cols[2]).cut_positions();
            let _unused = pool_of(cols[2]);
            if want.is_subset(&pool) {
                cov += 1;
            } else if miss.len() < 15 {
                miss.push(format!(
                    "{}  缺{:?}  期望 {}",
                    cols[0],
                    want.difference(&pool).collect::<Vec<_>>(),
                    cols[1].replace(' ', "␣")
                ));
            }
        }
    }
    println!(
        "  切點池涵蓋   {cov}/{tot}  {:.1}%",
        cov as f64 / tot as f64 * 100.0
    );
    for m in &miss {
        println!("     ✗ {m}");
    }
    println!(
        "  完整路徑     {path_ok}/{tot}  {:.1}%",
        path_ok as f64 / tot as f64 * 100.0
    );
    for b in &path_bad {
        println!("     ✗ {b}");
    }
    if cov == tot && path_ok == tot {
        println!(
            "
  兩道都滿分——每一句的正解都切得出來，可以談排序了。"
        );
    } else {
        println!(
            "
  ⚠ 有句子的正解切不出來，排序分數在這之前沒有意義。"
        );
    }
}
