//! 拿 `core/testdata/` 的真實測資驗證日文引擎的合法性判斷。
//!
//! 跟 `check_bopomofo` 同樣的做法：只驗證「測資裡標成日文的段落，
//! 日文引擎判不判它合法」，不驗證切點或選字。
//!
//! 用法：cargo run -p ime-core --bin check_romaji

use ime_core::romaji;
use std::collections::BTreeMap;

/// 這一段該由日文引擎負責嗎？
///
/// 判準：期望文字含假名，或者「全是漢字但按鍵是純字母」。
/// 後者是為了抓 `生誕`/`明日`/`寿司` 這種日文漢字——中日共用漢字，
/// 光看文字分不出來，要一起看按鍵：注音一定以聲調鍵收尾（`3467`
/// 或空白），日文羅馬字永遠沒有。
fn is_japanese_segment(text: &str, keys: &str) -> bool {
    let has_kana = text.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c));
    if has_kana {
        return true;
    }
    let all_han = !text.is_empty() && text.chars().all(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
    let pure_alpha = !keys.is_empty() && keys.chars().all(|c| c.is_ascii_alphabetic() || c == '-');
    all_han && pure_alpha
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

            let want_segs: Vec<&str> = want
                .trim_start_matches('~')
                .split('|')
                .map(str::trim)
                .filter(|s| *s != "_")
                .collect();
            let key_segs: Vec<&str> = key_expect.split('|').filter(|s| !s.is_empty()).collect();
            if want_segs.len() != key_segs.len() {
                continue;
            }

            for (w, k) in want_segs.iter().zip(key_segs.iter()) {
                if !is_japanese_segment(w, k) {
                    continue;
                }
                total += 1;
                let e = per_file.entry(f).or_insert((0, 0));
                e.1 += 1;
                if romaji::validity(k) == romaji::Validity::Valid {
                    ok += 1;
                    e.0 += 1;
                } else if failures.len() < 40 {
                    failures.push((f.to_string(), w.to_string(), k.to_string()));
                }
            }
        }
    }

    println!("=== 日文引擎：日文段的合法性判斷 ===\n");
    for (f, (o, n)) in &per_file {
        println!(
            "  {f:24} {o:4}/{n:4}  {:5.1}%",
            *o as f64 / *n as f64 * 100.0
        );
    }
    if total > 0 {
        println!(
            "\n  {:24} {ok:4}/{total:4}  {:5.1}%",
            "總計",
            ok as f64 / total as f64 * 100.0
        );
    }

    if !failures.is_empty() {
        println!("\n=== 判為非法的日文段（{}）===", failures.len());
        for (f, w, k) in &failures {
            println!("  [{f}] {w}  按鍵 {k}");
        }
    }
}
