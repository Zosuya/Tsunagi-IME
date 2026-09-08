//! spike：新的段選單，選切法真的比舊的輕鬆嗎？
//!
//! # 要驗證什麼
//!
//! §2.70 提的段選單有沒有比現在的整句選單好用，不能靠感覺——量**按鍵
//! 次數**。同一批句子、同一個目標（選到正解的分段），兩種選單各要按
//! 幾下？
//!
//! # 兩種選單怎麼算
//!
//! **舊的（整句選單，現況）**：TAB 開 → ↓ 一列一列走 → Enter。
//! 每列是一整種切法，正解排第 n 名就要按 n-1 次 ↓。
//! 超過 `CUTTING_PAGE`（10）列要先展開（空白），走到底自動展開也算一次。
//!
//! **新的（段選單）**：TAB 開 → → 走到要改的段 → ↓ 走到那一項 → Enter。
//! 可能要改好幾段，每段各算一次來回。附帶量 `Shift+→` 推邊界那條捷徑
//! 有沒有更省。
//!
//! # 判準
//!
//! 只算**到達正解**要按幾下，不含最後確認的 Enter（兩邊都要按，抵銷）。
//! 正解在第一名就是 0 下（兩邊都不必開選單）。
//!
//! **選不到的算特別的分數**：舊選單正解不在清單裡就是選不到；新選單
//! 是每一段都選不到想要的解釋。分開統計，不混進平均。
//!
//! # 用法
//!
//! ```text
//! cargo run --release -p ime-core --bin spike_seg_cost
//! cargo run --release -p ime-core --bin spike_seg_cost -- --verbose
//! ```

use ime_core::cutpoint::Segment;
use ime_core::input::Input;
use ime_core::language::Language;
use std::collections::BTreeMap;

#[path = "common/testdata.rs"]
mod testdata;

/// 整句選單一次列幾列（`session::CUTTING_PAGE`）。
const CUTTING_PAGE: usize = 10;
/// 展開後列幾列（`session::CUTTING_PAGE_ALL`）。
const CUTTING_PAGE_ALL: usize = 50;
/// 段選單裡一段最多試幾次（超過就當使用者放棄）。
const MAX_ROUNDS: usize = 5;

#[derive(Default)]
struct Tally {
    /// 這一節有幾句
    total: usize,
    /// 正解本來就是第一名，兩種選單都不必開
    already: usize,
    /// 舊選單選得到的句數 / 總按鍵
    old_ok: usize,
    old_keys: usize,
    /// 新選單選得到的句數 / 總按鍵
    new_ok: usize,
    new_keys: usize,
    /// 兩種都選得到的句數（**只有這些拿來比平均**，否則母體不同）
    both: usize,
    both_old: usize,
    both_new: usize,
    /// 只有新的選得到 / 只有舊的選得到
    only_new: usize,
    only_old: usize,
    /// 兩種都選不到
    neither: usize,
}

fn main() {
    let verbose = std::env::args().any(|a| a == "--verbose");
    testdata::load_engine();
    let rows = testdata::load("spike_seg_cost");
    if rows.is_empty() {
        eprintln!("沒有測資");
        return;
    }

    let mut by_tag: BTreeMap<String, Tally> = BTreeMap::new();
    let mut all = Tally::default();
    let mut wins: Vec<String> = Vec::new();
    let mut losses: Vec<String> = Vec::new();
    // 新選單選不到的原因
    let mut blocked: Vec<String> = Vec::new();

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

        let old = old_cost(&cuttings, &want);
        // 追蹤「新選單為什麼選不到」——只在 --verbose 且還沒收集夠時記
        let mut trace: Vec<String> = Vec::new();
        let want_trace = verbose && blocked.len() < 10;
        let new = new_cost(&cuttings, &want, want_trace.then_some(&mut trace));
        if new.is_none() && want_trace && !trace.is_empty() {
            blocked.push(format!("  {}｜{}\n{}", row.tag, row.keys, trace.join("\n")));
        }

        if let Some(k) = old {
            t.old_ok += 1;
            t.old_keys += k;
            all.old_ok += 1;
            all.old_keys += k;
        }
        if let Some(k) = new {
            t.new_ok += 1;
            t.new_keys += k;
            all.new_ok += 1;
            all.new_keys += k;
        }
        match (old, new) {
            (Some(o), Some(n)) => {
                t.both += 1;
                t.both_old += o;
                t.both_new += n;
                all.both += 1;
                all.both_old += o;
                all.both_new += n;
                if verbose && n + 3 <= o && wins.len() < 12 {
                    wins.push(format!("  {:<26} 舊 {o:>2} 下 → 新 {n:>2} 下", row.keys));
                }
                if verbose && o + 3 <= n && losses.len() < 12 {
                    losses.push(format!("  {:<26} 舊 {o:>2} 下 → 新 {n:>2} 下", row.keys));
                }
            }
            (None, Some(_)) => {
                t.only_new += 1;
                all.only_new += 1;
            }
            (Some(_), None) => {
                t.only_old += 1;
                all.only_old += 1;
            }
            (None, None) => {
                t.neither += 1;
                all.neither += 1;
            }
        }
    }

    println!("選到正解要按幾下：整句選單（現況）vs 段選單（§2.70）\n");
    println!(
        "{:<16} {:>5} {:>5} │ {:>7} {:>7} │ {:>6} {:>6} {:>6}",
        "節", "句數", "免選", "舊·平均", "新·平均", "只有新", "只有舊", "都不行"
    );
    println!("{}", "─".repeat(74));
    for (tag, t) in &by_tag {
        print_row(tag, t);
    }
    println!("{}", "─".repeat(74));
    print_row("總計", &all);

    println!("\n※ 平均只取**兩種都選得到**的句子（母體相同才比得出來）");
    if all.both > 0 {
        let o = all.both_old as f64 / all.both as f64;
        let n = all.both_new as f64 / all.both as f64;
        println!(
            "  可比較的 {} 句：舊 {:.1} 下 → 新 {:.1} 下（{}{:.1} 下，{:+.0}%）",
            all.both,
            o,
            n,
            if n < o { "省 " } else { "多 " },
            (o - n).abs(),
            100.0 * (n - o) / o,
        );
    }
    println!(
        "\n涵蓋率：舊選單選得到 {} 句，新選單 {} 句（需要動選單的共 {} 句）",
        all.old_ok,
        all.new_ok,
        all.total - all.already,
    );

    if verbose && !wins.is_empty() {
        println!("\n新的省很多的例子：");
        for w in &wins {
            println!("{w}");
        }
    }
    if verbose && !blocked.is_empty() {
        println!(
            "
新選單選不到的原因（最多 10 筆）："
        );
        for b in &blocked {
            println!("{b}");
        }
    }
    if verbose && !losses.is_empty() {
        println!("\n新的反而更費事的例子：");
        for l in &losses {
            println!("{l}");
        }
    }
}

fn print_row(name: &str, t: &Tally) {
    let avg = |sum: usize, n: usize| {
        if n == 0 {
            "─".to_string()
        } else {
            format!("{:.1}", sum as f64 / n as f64)
        }
    };
    println!(
        "{:<16} {:>5} {:>5} │ {:>7} {:>7} │ {:>6} {:>6} {:>6}",
        name,
        t.total,
        t.already,
        avg(t.both_old, t.both),
        avg(t.both_new, t.both),
        t.only_new,
        t.only_old,
        t.neither,
    );
}

/// 舊選單（整句）：正解排第幾名，就要按幾下。
///
/// 正解不在清單裡（或在展開後仍看不到）就回 `None`——選不到。
fn old_cost(cuttings: &[Vec<Segment>], want: &[(String, Language)]) -> Option<usize> {
    let idx = cuttings.iter().position(|c| same(c, want))?;
    // 看得到的範圍：沒展開 10 列，展開後 50 列
    if idx >= CUTTING_PAGE_ALL {
        return None;
    }
    // TAB 開選單算 1 下，之後每按一次 ↓ 走一列
    //
    // **走到第 10 列再往下會自動展開**（`next_cutting`），不必另外按
    // 空白——所以 idx 就是 ↓ 的次數，不加展開的成本
    Some(1 + idx)
}

/// 新選單（段）：走到要改的段、挑那一項，可能要改好幾段。
///
/// 一段的成本 = 右鍵走過去的次數 ＋ 下鍵走到那一項的次數。
/// 每改一段之後分段會重算，反白留在原處（§2.70.6），所以下一段的
/// 右鍵次數從目前位置起算。
fn new_cost(
    cuttings: &[Vec<Segment>],
    want: &[(String, Language)],
    trace: Option<&mut Vec<String>>,
) -> Option<usize> {
    let mut cur = cuttings[0].clone();
    // TAB 開選單
    let mut keys = 1usize;
    // 反白現在停在第幾段（預設第一段，§2.70.6 待決事項 1）
    let mut at = 0usize;

    for _ in 0..MAX_ROUNDS {
        if same(&cur, want) {
            return Some(keys);
        }
        let (idx, want_lang) = first_wrong(&cur, want)?;
        // 右鍵走過去（反白留在原處，所以從 `at` 走到 `idx`）
        keys += idx.abs_diff(at);
        at = idx;

        // 這一段的候選清單：從既有切法篩出「這個起點是一段」的，
        // 按切法排名排序、同一種只留一次
        let start: usize = cur[..idx].iter().map(|s| s.keys.chars().count()).sum();
        let cur_len = cur[idx].keys.chars().count();
        //
        // **清單要走完再挑**，不能一找到就 break——挑中的那一項排在
        // 第幾位，要知道完整的清單才算得出來。
        let mut seen: Vec<(usize, Language)> = Vec::new();
        // 每一項對應的完整切法（挑了它之後整句長什麼樣）
        let mut cuts: Vec<&Vec<Segment>> = Vec::new();
        for c in cuttings {
            let Some((len, lang)) = seg_at(c, start) else {
                continue;
            };
            if seen.contains(&(len, lang)) {
                continue;
            }
            seen.push((len, lang));
            cuts.push(c);
        }
        // 使用者要的那一項：**這一段**跟正解一致就好。整句對不對是
        // 下一輪的事——段選單的重點就是逐段修，不是一次選對整句。
        let target = want_seg_len(want, start);
        let Some(pick) = seen
            .iter()
            .position(|(len, lang)| *lang == want_lang && target == Some(*len))
        else {
            if let Some(log) = trace {
                log.push(format!(
                    "    第{}段（起點{start}）想要 {:?}長{:?}，清單只有 {:?}",
                    idx + 1,
                    want_lang,
                    target,
                    seen
                ));
            }
            return None;
        };
        // 現況那一項排在第幾位？從它走到目標
        let now = seen
            .iter()
            .position(|(l, g)| *l == cur_len && *g == cur[idx].lang)
            .unwrap_or(0);
        keys += pick.abs_diff(now);
        cur = cuts[pick].clone();
    }
    same(&cur, want).then_some(keys)
}

/// 正解裡從 `start` 起的那一段有多長？不是段的開頭就回 `None`。
fn want_seg_len(want: &[(String, Language)], start: usize) -> Option<usize> {
    let mut pos = 0usize;
    for (k, _) in want {
        let n = k.chars().count();
        if pos == start {
            return Some(n);
        }
        if pos > start {
            return None;
        }
        pos += n;
    }
    None
}

/// 這種切法裡，從 `start` 起的那一段的 `(長度, 語言)`。
fn seg_at(c: &[Segment], start: usize) -> Option<(usize, Language)> {
    let mut pos = 0usize;
    for s in c {
        let n = s.keys.chars().count();
        if pos == start {
            return Some((n, s.lang));
        }
        if pos > start {
            return None;
        }
        pos += n;
    }
    None
}

// ── 以下三個函式跟 spike_seg_lang 同一套判準 ──

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

fn first_wrong(cur: &[Segment], want: &[(String, Language)]) -> Option<(usize, Language)> {
    let mut pos = 0usize;
    for (i, s) in cur.iter().enumerate() {
        let len = s.keys.chars().count();
        let mut matched = None;
        let mut wpos = 0usize;
        for (k, l) in want {
            let wlen = k.chars().count();
            if wpos == pos {
                matched = Some((wlen, *l));
                break;
            }
            wpos += wlen;
        }
        match matched {
            Some((wlen, l)) if wlen == len && l == s.lang => {}
            Some((_, l)) => return Some((i, l)),
            None => {
                let mut wpos = 0usize;
                for (k, l) in want {
                    let wlen = k.chars().count();
                    if pos < wpos + wlen {
                        return Some((i, *l));
                    }
                    wpos += wlen;
                }
                return Some((i, s.lang));
            }
        }
        pos += len;
    }
    None
}
