//! 拿 `core/testdata/` 的真實測資驗證注音引擎的合法性判斷。
//!
//! 這支只驗證一件事：**測資裡標成注音的段落，注音引擎判不判它合法**。
//! 不驗證切點（那是 Phase 2）、不驗證選字（那是選詞模組）。
//!
//! 用法：cargo run -p ime-core --bin check_bopomofo

use ime_core::bopomofo;
use std::collections::BTreeMap;

/// 測資的第二欄（按鍵期望）用 `|` 標出切點。取出各段。
fn segments(key_expect: &str) -> Vec<&str> {
    key_expect.split('|').filter(|s| !s.is_empty()).collect()
}

/// 這一段該由注音引擎負責嗎？
///
/// **不能只看期望文字是不是漢字**——中日共用漢字，`生誕`/`明日`/`寿司`
/// 都是純漢字但用羅馬字打的日文。要一起看按鍵：注音的按鍵一定以聲調鍵
/// 收尾（`3467` 或空白），日文羅馬字永遠不會有聲調鍵。
fn is_bopomofo_segment(text: &str, keys: &str) -> bool {
    let all_han = !text.is_empty() && text.chars().all(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
    let ends_with_tone = keys
        .chars()
        .last()
        .is_some_and(|c| matches!(c, ' ' | '3' | '4' | '6' | '7'));
    all_han && ends_with_tone
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
    ];

    let mut total = 0usize;
    let mut ok = 0usize;
    let mut per_file: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let mut failures: Vec<(String, String, String)> = Vec::new();

    for f in files {
        let path = dir.join(format!("{f}.txt"));
        let Ok(content) = std::fs::read_to_string(&path) else {
            eprintln!("找不到 {}", path.display());
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
            let (want, key_expect) = (cols[0], cols[1]);

            // 期望的各段（濾掉分隔符空白段 `_`）
            let want_segs: Vec<&str> = want
                .trim_start_matches('~')
                .split('|')
                .map(str::trim)
                .filter(|s| *s != "_")
                .collect();
            let key_segs = segments(key_expect);
            // 兩邊的段數要對得上才比得了（`_` 在按鍵欄是空字串，已濾掉）
            if want_segs.len() != key_segs.len() {
                continue;
            }

            for (w, k) in want_segs.iter().zip(key_segs.iter()) {
                if !is_bopomofo_segment(w, k) {
                    continue;
                }
                total += 1;
                let e = per_file.entry(f).or_insert((0, 0));
                e.1 += 1;
                if bopomofo::validity(k) == bopomofo::Validity::Valid {
                    ok += 1;
                    e.0 += 1;
                } else if failures.len() < 30 {
                    failures.push((f.to_string(), w.to_string(), k.to_string()));
                }
            }
        }
    }

    println!("=== 注音引擎：中文段的合法性判斷 ===\n");
    for (f, (o, n)) in &per_file {
        println!(
            "  {f:24} {o:4}/{n:4}  {:5.1}%",
            *o as f64 / *n as f64 * 100.0
        );
    }
    println!(
        "\n  {:24} {ok:4}/{total:4}  {:5.1}%",
        "總計",
        ok as f64 / total as f64 * 100.0
    );

    if !failures.is_empty() {
        println!("\n=== 判為非法的中文段（前 {}）===", failures.len());
        for (f, w, k) in &failures {
            let vis = k.replace(' ', "␣");
            println!("  [{f}] {w}  按鍵 {vis}");
        }
    }
}
