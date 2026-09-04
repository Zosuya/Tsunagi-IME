//! 測資稽核：文字期望與按鍵期望對得上嗎？
//!
//! 測資是三欄——文字期望、按鍵期望、按鍵序列。前兩欄是**人工寫的**，
//! 兩邊各自獨立，所以可能互相矛盾：按鍵寫 `e93`（ㄍㄞˇ）而文字寫
//! 「修」（ㄒㄧㄡ）。這種列引擎永遠不可能過，而且會把計分器的
//! 數字帶偏——修引擎去迎合錯的期望，只會愈修愈壞。
//!
//! 這支把每個注音段的按鍵切成音節，逐音節問詞庫「這個音有哪些字」，
//! 對不上就報出來。`期望基準審核.md` §0 的第四種標記（**測資錯**）
//! 就是這一類。
//!
//! # 三種語言各查得動的部分
//!
//! | 語言 | 查什麼 | 為什麼只能查這些 |
//! |---|---|---|
//! | 注音 | 逐音節問詞庫「這個音有哪些字」 | 一音節一字，對得起來 |
//! | 日文 | 按鍵是不是合法羅馬字；期望裡的**假名片段**有沒有依序出現在轉出來的假名裡；整段都是假名時要完全相同 | 漢字讀音是多對多，而且 mozc 只收辭書形，活用形查不到——嚴格比對會製造大量假警報 |
//! | 英文 | 期望文字要跟按鍵一模一樣 | 英文是 passthrough，打什麼出什麼 |
//!
//! 用法：cargo run --release -p ime-core --bin check_testdata

use std::collections::{BTreeMap, HashMap};

const TAB: char = '\u{9}';

/// 寬容-列舉的分隔符（見 `期望基準審核.md` §0）。**前後都要有空白**
/// ——`|` 本身是段的分隔符，沒有空白的話分不出是段還是選項。
const ALT: &str = " || ";

/// 平假名或片假名？
fn is_kana(c: char) -> bool {
    ('\u{3041}'..='\u{309F}').contains(&c) || ('\u{30A1}'..='\u{30FF}').contains(&c)
}

/// 片假名轉平假名——稽核比的是**讀音**，不是選了哪種寫法。
fn to_hira(s: &str) -> String {
    s.chars()
        .map(|c| {
            if ('\u{30A1}'..='\u{30F6}').contains(&c) {
                char::from_u32(c as u32 - 0x60).unwrap_or(c)
            } else {
                c
            }
        })
        .collect()
}

/// 表記 → 這個寫法有哪些讀音。直接讀 mozc 的辭典檔建反查表。
///
/// `dict` 模組存的是「讀音 → 表記」（打字要的方向），稽核要反過來問
/// 「這個漢字詞該讀什麼」。只有稽核用得到，不進產品端。
fn build_reading_map(data_dir: &std::path::Path) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for i in 0..10 {
        let path = data_dir
            .join("japanese")
            .join(format!("dictionary{i:02}.txt"));
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        for line in content.lines() {
            // 讀音<TAB>左id<TAB>右id<TAB>詞頻<TAB>表記
            let f: Vec<&str> = line.split(TAB).collect();
            let (Some(reading), Some(surface)) = (f.first(), f.get(4)) else {
                continue;
            };
            if surface
                .chars()
                .any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c))
            {
                let e = out.entry((*surface).to_string()).or_default();
                let r = (*reading).to_string();
                if !e.contains(&r) {
                    e.push(r);
                }
            }
        }
    }
    out
}

/// 期望文字裡的**漢字詞**，讀音有出現在按鍵轉出的假名裡嗎？
///
/// 由左而右取最長的詞條。一個表記可能有好幾種讀音（明日＝あした／あす／
/// みょうにち），**全部都對不上才算錯**——不然會把合理的異讀誤判成錯。
fn check_kanji_readings(
    text: &str,
    kana: &str,
    map: &HashMap<String, Vec<String>>,
    bad: &mut Vec<String>,
) {
    const MAX_WORD: usize = 8;
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let mut hit = 0usize;
        for len in (1..=MAX_WORD.min(chars.len() - i)).rev() {
            let word: String = chars[i..i + len].iter().collect();
            if !word.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c)) {
                continue;
            }
            let Some(readings) = map.get(&word) else {
                continue;
            };
            if !readings.iter().any(|r| kana.contains(&to_hira(r))) {
                bad.push(format!(
                    "「{word}」的讀音（{}）都沒出現在「{kana}」裡",
                    readings
                        .iter()
                        .take(4)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("／")
                ));
            }
            hit = len;
            break;
        }
        i += if hit > 0 { hit } else { 1 };
    }
}

/// 日文段：按鍵轉得出假名嗎？期望裡的假名對得上嗎？
///
/// **不比漢字**——漢字讀音是多對多，而且 mozc 只收辭書形，活用形
/// （`teishutsushinakereba`）查不到，嚴格比對會製造大量假警報。
/// 能可靠查的是期望文字裡**本來就是假名**的部分：那些是使用者打出來的
/// 送假名與助詞，一定要出現在轉出來的假名裡，順序也要一樣。
fn check_japanese(
    text: &str,
    keys: &str,
    readings: &HashMap<String, Vec<String>>,
    bad: &mut Vec<String>,
) {
    let Some(kana) = ime_core::romaji::kana::to_kana(keys) else {
        bad.push(format!("「{keys}」不是合法羅馬字"));
        return;
    };
    let kana = to_hira(&kana);
    let want = to_hira(text);
    // 整段都是假名：要完全一樣
    if want.chars().all(is_kana) {
        if want != kana {
            bad.push(format!("假名對不上：期望「{want}」按鍵給「{kana}」"));
        }
        return;
    }
    // 混漢字：期望裡的假名片段要依序出現
    let mut rest = kana.as_str();
    for run in want.split(|c: char| !is_kana(c)).filter(|r| !r.is_empty()) {
        match rest.find(run) {
            Some(i) => rest = &rest[i + run.len()..],
            None => bad.push(format!("「{run}」沒出現在按鍵轉出的「{kana}」裡")),
        }
    }
    check_kanji_readings(&want, &kana, readings, bad);
}

fn main() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data");
    ime_core::dict::load_bopomofo(&data);
    ime_core::dict::load_japanese(&data);
    let readings = build_reading_map(&data);

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

    // 檔名 → (查過的注音段數, 對不上的列)
    let mut per_file: BTreeMap<&str, (usize, Vec<String>)> = BTreeMap::new();
    let mut rows_total = 0usize;
    let mut rows_bad = 0usize;

    for f in files {
        let Ok(content) = std::fs::read_to_string(dir.join(format!("{f}.txt"))) else {
            continue;
        };
        let e = per_file.entry(f).or_default();
        for line in content.lines() {
            let line = line.trim_end_matches(['\u{d}', '\u{a}']);
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let cols: Vec<&str> = line.split(TAB).collect();
            if cols.len() < 3 {
                continue;
            }
            let key_expect = cols[1];
            let keys: Vec<&str> = key_expect.split('|').collect();

            // 寬容-列舉的每個選項都要自洽——`sushi || すし` 兩邊各自
            // 都得跟按鍵對得上，其中一個寫錯就是測資有問題
            let alt_texts: Vec<Vec<&str>> = cols[0]
                .split(ALT)
                .map(|a| a.trim_start_matches('~').split('|').collect::<Vec<&str>>())
                .filter(|t| t.len() == keys.len())
                .collect();
            if alt_texts.is_empty() {
                continue;
            }
            rows_total += 1;

            let mut bad: Vec<String> = Vec::new();
            for (t, k) in alt_texts.iter().flatten().zip(keys.iter().cycle()) {
                // 分隔符段跳過
                if k.trim().is_empty() {
                    continue;
                }
                // ── 英文：passthrough，打什麼出什麼 ──
                if t.is_ascii() && !t.is_empty() {
                    e.0 += 1;
                    if t != k {
                        bad.push(format!("英文段「{t}」跟按鍵「{k}」不一樣"));
                    }
                    continue;
                }
                // ── 日文：期望裡有假名就查 ──
                if t.chars().any(is_kana) {
                    e.0 += 1;
                    check_japanese(t, k, &readings, &mut bad);
                    continue;
                }
                // ── 注音：切得出音節而且字數對得上 ──
                let Some(syls) = ime_core::bopomofo::split_syllables(k) else {
                    continue;
                };
                let chars: Vec<char> = t.chars().collect();
                if syls.is_empty() || chars.len() != syls.len() {
                    continue;
                }
                e.0 += 1;
                for (c, s) in chars.iter().zip(syls.iter()) {
                    let ok = ime_core::dict::chars_for(s)
                        .iter()
                        .any(|w| w.starts_with(*c));
                    if !ok {
                        bad.push(format!("{c} 讀不出 {s}"));
                    }
                }
            }
            if !bad.is_empty() {
                rows_bad += 1;
                e.1.push(format!("{}\t{key_expect}\t（{}）", cols[0], bad.join("、")));
            }
        }
    }

    println!("=== 測資稽核：文字期望 vs 按鍵期望 ===\n");
    println!("  {:22} {:>10} {:>10}", "", "查過的列", "對不上");
    for (f, (segs, bad)) in &per_file {
        println!(
            "  {:22} {:>10} {:>10}   （注音段 {segs} 個）",
            f.replace("mixed_", ""),
            "",
            bad.len()
        );
    }
    println!("\n  總計 {rows_total} 列可比對，其中 {rows_bad} 列對不上");

    for (f, (_, bad)) in &per_file {
        if bad.is_empty() {
            continue;
        }
        println!("\n=== {} 的 {} 列 ===", f.replace("mixed_", ""), bad.len());
        for b in bad {
            println!("  {b}");
        }
    }
}
