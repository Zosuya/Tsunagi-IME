//! 切詞學習有沒有用？門檻要設幾？**這是調 `LEARNED_CUT` 的那把尺。**
//!
//! # 怎麼模擬
//!
//! 一輪 ＝ 使用者把整批測資打過一次：
//!
//! 1. 逐鍵打完，比對引擎給的文字與期望
//! 2. 不一樣就**在切法選單裡找正解**——找得到就模擬使用者按 Tab 挑它，
//!    並記進學習（`learn_on_commit`）
//! 3. 下一輪再打一次同一批——學到門檻之後引擎自己就該切對
//!
//! 找不到正解的那些是**生成階段**就沒給出來，不歸切詞學習管。
//!
//! # 為什麼要掃門檻
//!
//! 選字學習的門檻是 2 次，但切錯詞的代價高得多（整句重新斷句、前面的
//! 字跟著變），所以門檻另計、而且要量出來。見開發文件 §2.26.2。
//!
//! 用法：`cargo run --release -p ime-core --bin bench_cut_learn`

use ime_core::session::Session;

const ROUNDS: usize = 4;
const TAB: char = '\u{9}';
const ALT: &str = " || ";

/// 測資的每一列：期望文字（可能有寬容標記）、按鍵串。
fn rows() -> Vec<(String, String)> {
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
        "bopomofo_words",
        "bopomofo_sentences",
    ];
    let mut out = Vec::new();
    for f in files {
        let Ok(content) = std::fs::read_to_string(dir.join(format!("{f}.txt"))) else {
            continue;
        };
        for line in content.lines() {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let mut it = line.split(TAB);
            let (Some(want), Some(_), Some(keys)) = (it.next(), it.next(), it.next()) else {
                continue;
            };
            if keys.trim().is_empty() || want.is_empty() {
                continue;
            }
            out.push((want.to_string(), keys.to_string()));
        }
    }
    out
}

fn strip_spaces(s: &str) -> String {
    s.chars()
        .filter(|c| *c != ' ' && *c != '\u{3000}')
        .collect()
}

/// 引擎給的文字算不算中？照 `check_compose` 的寬容標記規則。
fn matches(want_field: &str, got: &str) -> bool {
    want_field.split(ALT).any(|opt| {
        let loose = opt.starts_with('~');
        let want = opt
            .trim_start_matches('~')
            .replace('|', "")
            .replace('_', " ");
        if loose {
            strip_spaces(&want) == strip_spaces(got)
        } else {
            want == got
        }
    })
}

fn typed(keys: &str) -> Session {
    let mut s = Session::new();
    for c in keys.chars() {
        s.push(c);
    }
    s
}

/// 一輪：回傳（命中數、這輪模擬使用者按了幾次 Tab、記了幾條）。
fn round(rows: &[(String, String)]) -> (usize, usize, usize) {
    let (mut hit, mut fixed, mut learned) = (0, 0, 0);
    for (want, keys) in rows {
        let mut s = typed(keys);
        if matches(want, &s.text()) {
            hit += 1;
            continue;
        }
        // 選單裡有正解嗎？有就模擬使用者挑它
        let n = s.cutting_count();
        let mut found = None;
        for i in 0..n {
            s.set_cutting_index(i);
            if matches(want, &s.text()) {
                found = Some(i);
                break;
            }
        }
        match found {
            Some(i) => {
                s.set_cutting_index(i);
                fixed += 1;
                learned += s.learn_on_commit();
            }
            // 選單裡根本沒有——生成階段的事，不歸切詞學習管
            None => s.set_cutting_index(0),
        }
    }
    (hit, fixed, learned)
}

fn main() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data");
    ime_core::preload(&data, ime_core::config::Engines::default());
    let rows = rows();
    println!("=== 切詞學習：門檻掃描 ===\n");
    println!("  句數 {}\n", rows.len());

    // 先看兩種粒度各自的貢獻（門檻固定 2，看終局）
    println!("  ── 兩種粒度各自的貢獻（門檻 2、打四輪）──");
    for (name, lang, whole) in [
        ("只有段落層級", true, false),
        ("只有整串層級", false, true),
        ("兩種都開", true, true),
    ] {
        ime_core::learn::clear();
        ime_core::learn::set_cut_kinds(lang, whole);
        ime_core::learn::set_learned_cut(2);
        let mut last = 0usize;
        for _ in 1..=ROUNDS {
            last = round(&rows).0;
        }
        println!(
            "    {name}  命中 {last}/{} ({:.1}%)",
            rows.len(),
            100.0 * last as f64 / rows.len().max(1) as f64
        );
    }
    ime_core::learn::set_cut_kinds(true, true);
    println!();

    for need in [2u32, 3, 4] {
        ime_core::learn::clear();
        ime_core::learn::set_learned_cut(need);
        println!("  門檻 {need} 次");
        for r in 1..=ROUNDS {
            let (hit, fixed, learned) = round(&rows);
            println!(
                "    第 {r} 輪  命中 {hit}/{} ({:.1}%)   選單裡有正解 {fixed}   記了 {learned} 條",
                rows.len(),
                100.0 * hit as f64 / rows.len().max(1) as f64
            );
        }
        println!("    學習庫 {} 條切詞\n", ime_core::learn::cutting().len());
    }
    ime_core::learn::clear();
}
