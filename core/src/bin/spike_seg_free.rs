//! spike：段選單**自由組合**（不限於既有切法清單）救得回多少？
//!
//! # 為什麼要量這個
//!
//! `spike_seg_lang` 與 `spike_seg_cost` 都假設段選單是個**過濾器**——
//! 使用者指定某一段之後，在既有的 `cuttings` 裡找符合的。那個假設下
//! 有 77 句救不回來，我當時判斷是「引擎層的限制，選單改不了」。
//!
//! 使用者指出那個判斷錯了：**引擎算出來的那幾十種裡根本沒有他要的**，
//! 而逐段挑的話組得出來。也就是說段選單不該只當過濾器，它應該能組出
//! **引擎沒生成過的組合**。
//!
//! 證據就在 `spike_seg_cost` 自己印的診斷裡：
//!
//! ```text
//! en_vowel｜userel3   第2段想要「日文長3」，清單只有 [(3, 注音)]
//! ```
//!
//! 長度對、語言不對——那個組合在 `prune` 就死了，過濾器永遠找不到。
//! 但使用者在段選單上明講「這一段是日文」的話，引擎其實**問得出**
//! 那一段合不合法（三個合法性引擎本來就獨立）。
//!
//! # 量什麼
//!
//! 三種強度，逐級放寬：
//!
//! | 模式 | 段候選從哪來 | 對應的產品行為 |
//! |---|---|---|
//! | **過濾器** | 既有 `cuttings` 拆出來的 | `spike_seg_lang` 量的那套 |
//! | **自由** | 任何**通過合法性引擎**的 (長度,語言) | 直接問引擎，繞過 `prune` 與 `ALIVE_LIMIT` |
//!
//! 「自由」模式仍然**不是無法無天**——每一段都要通過該語言的合法性
//! 判斷，只是不受整句剪枝的牽連。這正是段選單相對於整句選單的結構性
//! 優勢：**段的合法性是局部的，整句的存活是全域的**。
//!
//! # 用法
//!
//! ```text
//! cargo run --release -p ime-core --bin spike_seg_free
//! cargo run --release -p ime-core --bin spike_seg_free -- --verbose
//! ```

use ime_core::cutpoint::Segment;
use ime_core::input::Input;
use ime_core::language::Language;
use std::collections::BTreeMap;

#[path = "common/testdata.rs"]
mod testdata;

/// 一段最多幾個按鍵。太長的段沒有意義，也讓窮舉不至於爆炸。
const MAX_SEG: usize = 24;

#[derive(Default)]
struct Tally {
    total: usize,
    /// 第一名就對，不必開選單
    already: usize,
    /// 過濾器模式救得回（＝既有清單裡找得到）
    filter_ok: usize,
    /// 自由模式救得回
    free_ok: usize,
    /// **只有自由模式救得回**——這就是「舊的算不出我要的」那批
    free_only: usize,
    /// 兩種都救不回
    neither: usize,
}

fn main() {
    let verbose = std::env::args().any(|a| a == "--verbose");
    testdata::load_engine();
    let rows = testdata::load("spike_seg_free");
    if rows.is_empty() {
        eprintln!("沒有測資");
        return;
    }

    let mut by_tag: BTreeMap<String, Tally> = BTreeMap::new();
    let mut all = Tally::default();
    let mut gained: Vec<String> = Vec::new();
    let mut lost: Vec<String> = Vec::new();

    for row in &rows {
        let input = Input::from_keys(&row.keys, None);
        let cuttings: Vec<Vec<Segment>> = input.cuttings().to_vec();
        if cuttings.is_empty() {
            continue;
        }
        let Some(want) = parse_expect(row, &cuttings[0]) else {
            continue;
        };

        let t = by_tag.entry(row.tag.clone()).or_default();
        t.total += 1;
        all.total += 1;

        if same(&cuttings[0], &want) {
            t.already += 1;
            all.already += 1;
            continue;
        }

        // ── 過濾器模式：正解的每一段，都要在既有清單裡找得到 ──
        let filter = want_reachable_by_filter(&cuttings, &want);
        // ── 自由模式：正解的每一段，各自通過合法性引擎就行 ──
        let free = want_reachable_free(&want);

        if filter {
            t.filter_ok += 1;
            all.filter_ok += 1;
        }
        if free {
            t.free_ok += 1;
            all.free_ok += 1;
        }
        match (filter, free) {
            (false, true) => {
                t.free_only += 1;
                all.free_only += 1;
                if verbose && gained.len() < 20 {
                    gained.push(format!("  {:<26} {}", row.keys, show(&want)));
                }
            }
            (false, false) => {
                t.neither += 1;
                all.neither += 1;
                if verbose && lost.len() < 15 {
                    lost.push(format!("  {:<26} {}", row.keys, show(&want)));
                }
            }
            _ => {}
        }
    }

    println!("段選單：過濾器（限既有清單）vs 自由組合（只問合法性）\n");
    println!(
        "{:<16} {:>5} {:>5} │ {:>7} {:>7} │ {:>8} {:>7}",
        "節", "句數", "免選", "過濾器", "自由", "只有自由", "都不行"
    );
    println!("{}", "─".repeat(66));
    for (tag, t) in &by_tag {
        print_row(tag, t);
    }
    println!("{}", "─".repeat(66));
    print_row("總計", &all);

    let need = all.total - all.already;
    if need > 0 {
        println!(
            "\n需要動選單的 {need} 句：\n  過濾器救得回 {}（{:.1}%）\n  自由組合救得回 {}（{:.1}%）\n  **只有自由組合救得回 {}**",
            all.filter_ok,
            100.0 * all.filter_ok as f64 / need as f64,
            all.free_ok,
            100.0 * all.free_ok as f64 / need as f64,
            all.free_only,
        );
    }

    if verbose && !gained.is_empty() {
        println!("\n舊選單算不出來、逐段挑才組得出來的（最多 20 筆）：");
        for g in &gained {
            println!("{g}");
        }
    }
    if verbose && !lost.is_empty() {
        println!("\n連自由組合都救不回的（最多 15 筆）：");
        for l in &lost {
            println!("{l}");
        }
    }
}

fn print_row(name: &str, t: &Tally) {
    println!(
        "{:<16} {:>5} {:>5} │ {:>7} {:>7} │ {:>8} {:>7}",
        name, t.total, t.already, t.filter_ok, t.free_ok, t.free_only, t.neither
    );
}

/// 過濾器模式：正解的每一段，都要在既有 `cuttings` 裡當過「從那個
/// 起點開始的一段」。
///
/// 這就是 `spike_seg_lang` 的做法——使用者只能從引擎生出來的東西裡挑。
fn want_reachable_by_filter(cuttings: &[Vec<Segment>], want: &[(String, Language)]) -> bool {
    let mut pos = 0usize;
    for (k, lang) in want {
        let len = k.chars().count();
        let found = cuttings.iter().any(|c| {
            let mut at = 0usize;
            for s in c {
                let n = s.keys.chars().count();
                if at == pos {
                    return n == len && s.lang == *lang;
                }
                if at > pos {
                    return false;
                }
                at += n;
            }
            false
        });
        if !found {
            return false;
        }
        pos += len;
    }
    true
}

/// 自由模式：正解的每一段各自通過合法性引擎就行，不管整句組合有沒有
/// 被生成過。
///
/// **這不是「什麼都能選」**——每一段仍然要是合法的注音／日文，或是
/// 英文（passthrough，永遠合法）。差別在於**不受整句剪枝的牽連**。
fn want_reachable_free(want: &[(String, Language)]) -> bool {
    want.iter().all(|(k, lang)| seg_valid(k, *lang))
}

/// 這一段按鍵，當成 `lang` 合法嗎？
fn seg_valid(keys: &str, lang: Language) -> bool {
    if keys.chars().count() > MAX_SEG {
        return false;
    }
    // 分隔符空白自成一段，永遠合法
    if keys.trim().is_empty() {
        return true;
    }
    match lang {
        Language::Bopomofo => {
            ime_core::bopomofo::validity(keys) == ime_core::bopomofo::Validity::Valid
        }
        Language::Romaji => ime_core::romaji::validity(keys) == ime_core::romaji::Validity::Valid,
        // 英文是 passthrough——瀑布的最後一站，永遠接得住
        Language::English => true,
    }
}

fn show(want: &[(String, Language)]) -> String {
    want.iter()
        .map(|(k, l)| format!("{}:{k}", mark(*l)))
        .collect::<Vec<_>>()
        .join("｜")
}

fn mark(l: Language) -> &'static str {
    match l {
        Language::Bopomofo => "注",
        Language::Romaji => "日",
        Language::English => "英",
    }
}

// ── 以下跟 spike_seg_lang 同一套判準 ──

fn parse_expect(row: &testdata::Row, first: &[Segment]) -> Option<Vec<(String, Language)>> {
    if row.want.starts_with('~') || row.want.contains(testdata::ALT) {
        return None;
    }
    let keys = row.expected_segs();
    let texts: Vec<&str> = row
        .alts()
        .first()
        .copied()
        .unwrap_or(&row.want)
        .split('|')
        .map(str::trim)
        .collect();
    if keys.len() != texts.len() || keys.is_empty() || keys.concat() != row.keys {
        return None;
    }
    let mut out: Vec<(String, Language)> = Vec::new();
    let mut pos = 0usize;
    for (k, t) in keys.iter().zip(&texts) {
        let sep = *t == "_";
        let lang = if sep {
            Language::English
        } else {
            lang_of_text(t).or_else(|| lang_from_keys(k, first, pos))?
        };
        pos += k.chars().count();
        let prev_sep = out.last().is_some_and(|(k, _)| k.trim().is_empty());
        match out.last_mut() {
            Some((prev, l)) if *l == lang && !sep && !prev_sep => prev.push_str(k),
            _ => out.push((k.clone(), lang)),
        }
    }
    Some(out)
}

/// 純漢字段的語言：中日的漢字長得一樣，看輸出分不出來。
///
/// **先問合法性引擎，再退回引擎第一名的判斷**。直接用第一名是錯的
/// ——那正是引擎搞錯的地方（`loggerul4` 的 `ul4` 被判成日文，測資
/// 寫的卻是注音的「要」）。把引擎的錯誤當成正解，量出來的東西就
/// 全歪了。
fn lang_from_keys(keys: &str, first: &[Segment], pos: usize) -> Option<Language> {
    let zh = ime_core::bopomofo::validity(keys) == ime_core::bopomofo::Validity::Valid;
    let ja = ime_core::romaji::validity(keys) == ime_core::romaji::Validity::Valid;
    match (zh, ja) {
        // 只有一邊收，就是它
        (true, false) => Some(Language::Bopomofo),
        (false, true) => Some(Language::Romaji),
        // 兩邊都收或都不收，才退回引擎的判斷
        _ => lang_at(first, pos),
    }
}

fn lang_of_text(t: &str) -> Option<Language> {
    let (mut kana, mut latin, mut han) = (false, false, false);
    for c in t.chars() {
        match c {
            '\u{3040}'..='\u{30ff}' => kana = true,
            'A'..='Z' | 'a'..='z' => latin = true,
            '\u{4e00}'..='\u{9fff}' => han = true,
            _ => {}
        }
    }
    match (kana, latin, han) {
        (true, _, _) => Some(Language::Romaji),
        (false, true, false) => Some(Language::English),
        _ => None,
    }
}

fn lang_at(c: &[Segment], pos: usize) -> Option<Language> {
    let mut at = 0usize;
    for s in c {
        let n = s.keys.chars().count();
        if pos < at + n {
            return Some(s.lang);
        }
        at += n;
    }
    None
}

fn same(cur: &[Segment], want: &[(String, Language)]) -> bool {
    cur.len() == want.len()
        && cur
            .iter()
            .zip(want)
            .all(|(s, (k, l))| s.keys.trim() == k.trim() && s.lang == *l)
}
