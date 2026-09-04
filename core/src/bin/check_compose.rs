//! 文字正確率（錯字率）計分。**量的是使用者實際看到的字**。
//!
//! `check_incremental` 量的是切點——「哪一段是什麼語言」對不對。
//! 但切對了不代表顯示的字對：`u10` 切成注音之後，「一班」還是「一般」
//! 是選詞層（`compose`）的事，那一層此前完全沒有被量過。
//!
//! # 量什麼
//!
//! 對每句測資：取切法第一名 → `compose` → `text_of`，跟測資第一欄的
//! 文字期望比。這正是輸入法打完不動選字鍵時送出的東西。
//!
//! 失分拆成三類，修法完全不同：
//!
//! | 類別 | 性質 | 怎麼解 |
//! |---|---|---|
//! | 切法錯 | 第一名切法就不是正解 | 改切點排序（`rank`） |
//! | 空白差 | 字全對，只差空白的全半形 | `width` 的空白規則 vs 測資期望，要人裁決 |
//! | 選字錯 | 切法對，但同音字／詞挑錯 | 改選詞層（`compose`／詞頻） |
//!
//! # 期望標記（見 testdata/期望基準審核.md §0）
//!
//! - 無標記：接起來的文字要完全一樣（`_` 代表空白）
//! - `~` 開頭：忽略空白再比（寬容-段——段間空白歸屬不影響輸出）
//! - ` || ` 分隔：**寬容-列舉**——這幾個答案都算對，其他都錯。
//!   每個選項各自可以加 `~`。
//!
//!   用在「同一串按鍵有兩種都正確的輸出」：`sushi` 判成英文原樣送出、
//!   或判成日文寫作 `すし`，兩者都對，錯的是別的東西。沒有這個標記的話
//!   計分器會把引擎的正確行為記成錯，修引擎去迎合只會愈修愈壞。
//!
//! 另外報一個**錯字率**：期望與實際的字元編輯距離總和 ÷ 期望字元總數
//! （雙方先去掉空白）。「差一個字」跟「整句全錯」對使用者的痛感不同，
//! 句級命中率看不出這個差別。
//!
//! # 依語言拆的錯字率
//!
//! 「注音部分錯多少」跟「日文部分錯多少」是兩個不同的問題，修法也不同。
//! 最後一節把每一段按語言歸戶分開算。
//!
//! **只統計切法正確的句子**——切錯的時候段落對不起來（引擎給的段跟
//! 期望的段連數量都不同），硬要對應只會產生假數字。被排除的句數會
//! 一併報出來，看的時候要記得這是「切對之後」的錯字率。
//!
//! 用法：cargo run --release -p ime-core --bin check_compose

use ime_core::compose;
use ime_core::cutpoint::incremental::Incremental;
use ime_core::cutpoint::{normalize, rank};
use std::collections::BTreeMap;

const TAB: char = '\u{9}';

/// 寬容-列舉的分隔符。**前後都要有空白**——`|` 本身是段的分隔符，
/// 沒有空白的話 `A|B||C|D` 分不出是三段還是兩個選項。
const ALT: &str = " || ";

/// `check| |u vu84` → 各段的按鍵
fn expected_segs(key_expect: &str) -> Vec<String> {
    key_expect
        .split('|')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// 文字期望欄 → 期望字串：`|` 只是分隔、`_` 代表空白
fn expected_text(want: &str) -> String {
    want.replace('|', "").replace('_', " ")
}

/// 去掉全半形空白——寬容-段與錯字率都不把空白當字算
fn strip_spaces(s: &str) -> String {
    s.chars()
        .filter(|c| *c != ' ' && *c != '\u{3000}')
        .collect()
}

/// 字元編輯距離（插入／刪除／替換各算 1）
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != cb);
            cur[j + 1] = sub.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// 依語言的字元統計。
#[derive(Default, Clone, Copy)]
struct LangStat {
    /// 期望字元數
    chars: usize,
    /// 錯字數（編輯距離）
    errs: usize,
    /// 這個語言的段數
    segs: usize,
    /// 整段完全正確的段數
    ok_segs: usize,
}

/// 一份測資的統計。
#[derive(Default, Clone, Copy)]
struct Stat {
    /// 文字完全命中
    hit: usize,
    /// 沒中，而且切法第一名就錯了——切點的鍋
    cut_wrong: usize,
    /// 字全對，只差空白的全半形——規則與期望的矛盾，另計
    space_diff: usize,
    /// 沒中，切法對——選詞層的鍋
    pick_wrong: usize,
    /// 期望字元數（去空白）
    chars: usize,
    /// 錯字數（編輯距離，去空白）
    errs: usize,
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
    // `--pack <包名>`：帶著領域包量一次。**包會影響切點**（`en` 那半
    // 餵 `is_top_word`、`zh` 那半餵 `claimed`），所以「學到的詞會不會
    // 讓分數退步」只能這樣量。不給就是不載任何包。
    let packs: Vec<String> = std::env::args()
        .skip(1)
        .scan(false, |want, a| {
            let take = *want;
            *want = a == "--pack";
            Some(take.then_some(a))
        })
        .flatten()
        .collect();
    if !packs.is_empty() {
        let cfg = ime_core::config::Config::load(None);
        let n = ime_core::pack::load(&cfg.behavior.packs_dir, &packs);
        println!("  （載入領域包 {packs:?}，共 {n} 條）");
    }

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

    let mut per_file: BTreeMap<&str, Stat> = BTreeMap::new();
    // (期望, 實際, 是不是切法錯)
    let mut fails: Vec<(String, String, bool)> = Vec::new();
    // 語言名 -> 字元統計。只累計切法正確的句子
    let mut per_lang: BTreeMap<&str, LangStat> = BTreeMap::new();
    let mut lang_rows = 0usize;
    let mut lang_skipped = 0usize;

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
            let (want_col, key_expect, keys) = (cols[0], cols[1], cols[2]);

            let cands = rank::sort(Incremental::from_keys(keys).cuttings());
            let top1 = cands.first().map(|c| normalize(c)).unwrap_or_default();
            let slots = compose::compose(&top1);
            let got = compose::text_of(&slots);

            // 寬容-列舉：命中任一個選項就算對
            let alts: Vec<&str> = want_col.split(ALT).collect();
            let hit = alts.iter().find(|alt| {
                let tol = alt.starts_with('~');
                let e = expected_text(alt.trim_start_matches('~'));
                if tol {
                    strip_spaces(&e) == strip_spaces(&got)
                } else {
                    e == got
                }
            });
            let ok = hit.is_some();
            // 沒中的話拿第一個選項當代表——報表與逐段統計都要有個基準
            let want = hit
                .copied()
                .unwrap_or_else(|| alts[0])
                .trim_start_matches('~');
            let expect = expected_text(want);
            let cut_ok = top1.iter().map(|s| s.keys.clone()).collect::<Vec<_>>()
                == expected_segs(key_expect);

            // 依語言歸戶：切法正確時，期望的段與引擎的段一一對應
            let texts: Vec<&str> = want.split('|').collect();
            if cut_ok && texts.len() == top1.len() {
                lang_rows += 1;
                let mut si = 0usize;
                for (seg, want_seg) in top1.iter().zip(texts.iter()) {
                    // 一個段可能被切成好幾格（注音是一音節一格），
                    // 按鍵接回原段長度為止就是這一段的格
                    let mut acc = String::new();
                    let mut got_seg = String::new();
                    while si < slots.len() && acc.len() < seg.keys.len() {
                        acc.push_str(&slots[si].keys);
                        got_seg.push_str(&slots[si].text);
                        si += 1;
                    }
                    let name = if seg.is_mark {
                        "標點／空白"
                    } else {
                        match seg.lang {
                            ime_core::language::Language::Bopomofo => "注音",
                            ime_core::language::Language::Romaji => "日文",
                            ime_core::language::Language::English => "英文",
                        }
                    };
                    let we = strip_spaces(&want_seg.replace('_', " "));
                    let ge = strip_spaces(&got_seg);
                    let e = per_lang.entry(name).or_default();
                    e.chars += we.chars().count();
                    e.errs += edit_distance(&we, &ge);
                    e.segs += 1;
                    if we == ge {
                        e.ok_segs += 1;
                    }
                }
            } else {
                lang_skipped += 1;
            }

            let e = per_file.entry(f).or_default();
            e.total += 1;
            let se = strip_spaces(&expect);
            let sg = strip_spaces(&got);
            e.chars += se.chars().count();
            e.errs += edit_distance(&se, &sg);
            if ok {
                e.hit += 1;
            } else if se == sg {
                // 去掉空白就一樣——差的只有空白的形制或位置
                e.space_diff += 1;
            } else if cut_ok {
                e.pick_wrong += 1;
                fails.push((expect, got, false));
            } else {
                e.cut_wrong += 1;
                fails.push((expect, got, true));
            }
        }
    }

    println!("=== 文字正確率（切法第一名 → compose）===\n");
    println!(
        "  {:22} {:>12} {:>10} {:>10} {:>10} {:>10}",
        "", "文字命中", "切法錯", "空白差", "選字錯", "錯字率"
    );
    let mut t = Stat::default();
    for (f, s) in &per_file {
        t.hit += s.hit;
        t.cut_wrong += s.cut_wrong;
        t.space_diff += s.space_diff;
        t.pick_wrong += s.pick_wrong;
        t.chars += s.chars;
        t.errs += s.errs;
        t.total += s.total;
        println!(
            "  {:22} {:>5}/{:<3} {:>3.0}% {:>8} {:>8} {:>8} {:>9.1}%",
            f.replace("mixed_", ""),
            s.hit,
            s.total,
            s.hit as f64 / s.total as f64 * 100.0,
            s.cut_wrong,
            s.space_diff,
            s.pick_wrong,
            s.errs as f64 / s.chars.max(1) as f64 * 100.0
        );
    }
    println!(
        "\n  {:22} {:>5}/{:<3} {:>3.0}% {:>8} {:>8} {:>8} {:>9.1}%",
        "總計",
        t.hit,
        t.total,
        t.hit as f64 / t.total as f64 * 100.0,
        t.cut_wrong,
        t.space_diff,
        t.pick_wrong,
        t.errs as f64 / t.chars.max(1) as f64 * 100.0
    );
    println!(
        "\n  沒中的 {} 句裡：切法錯 {}（切點的鍋）、空白差 {}（規則 vs 期望，要裁決）、\
         選字錯 {}（選詞層的鍋）",
        t.cut_wrong + t.space_diff + t.pick_wrong,
        t.cut_wrong,
        t.space_diff,
        t.pick_wrong
    );

    println!(
        "\n=== 依語言拆的錯字率（只算切法正確的 {lang_rows} 句，排除 {lang_skipped} 句）===\n"
    );
    println!(
        "  {:12} {:>10} {:>10} {:>12} {:>12}",
        "", "段數", "整段正確", "期望字元", "錯字率"
    );
    for (name, s) in &per_lang {
        println!(
            "  {:12} {:>10} {:>6}/{:<3} {:>3.0}% {:>8} {:>11.1}%",
            name,
            s.segs,
            s.ok_segs,
            s.segs,
            s.ok_segs as f64 / s.segs.max(1) as f64 * 100.0,
            s.chars,
            s.errs as f64 / s.chars.max(1) as f64 * 100.0
        );
    }

    if !fails.is_empty() {
        println!("\n=== 文字沒中的 {} 句 ===", fails.len());
        for (expect, got, cut_wrong) in &fails {
            let tag = if *cut_wrong { "切" } else { "選" };
            println!("  [{tag}] 期 {expect}");
            println!("      實 {got}");
        }
    }
}
