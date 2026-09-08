//! spike：切法選單改成「反白一段、選那段是什麼語言」可不可行？
//!
//! # 要驗證什麼
//!
//! 新互動的粒度是**單段**：使用者反白引擎切出來的某一段，往下拉出
//! 「這一段要當成哪個語言」的候選。它取代現在那個「一列一整句」的
//! 切法選單。
//!
//! 動手改產品之前只有一個問題非量不可：**指定了語言之後，既有的切法
//! 清單裡找得到符合的嗎？**
//!
//! 找不到的話就得在「那個選項乾脆不列出來」與「強制改寫那一段」之間
//! 二選一，而那是使用者要決定的事——這支就是給那個決定用的數據。
//!
//! # 怎麼模擬「使用者想要什麼」
//!
//! 測資有兩欄拼得出正解的分段：
//!
//! - **第二欄**（`check| |u vu84`）給**切點位置**——每段的按鍵
//! - **第一欄**（`check|_|一下`）給**每段的輸出**，字型就透露了語言：
//!   漢字＝注音、假名＝日文、拉丁字母＝英文
//!
//! 兩欄的段是一一對應的（`want_segs` 與 `expected_segs` 濾掉分隔符
//! 之後對齊）。拿拼出來的正解跟引擎第一名的切法比：第一個對不上的
//! 位置，就是使用者實際會去反白的段；「他想指定的語言」就是正解在那
//! 個位置用的語言。
//!
//! **漢字有歧義**：日文的漢字（`仕事`）跟中文的漢字長得一樣。所以
//! 「全是漢字」的段用**引擎自己判的語言**當正解——那一段本來就沒有
//! 語言爭議，使用者不會去改它。有爭議的（假名 vs 漢字、字母 vs 漢字）
//! 才判得出來，而那些正是這個互動要處理的情況。
//!
//! 然後在既有清單裡找「那一段的按鍵範圍與語言都符合」的切法——這正是
//! 產品裡按下那個候選之後要做的事（方案 A：當成過濾器）。
//!
//! # 量三件事
//!
//! 1. **找得到嗎**——過濾器有沒有東西可選
//! 2. **一次就對了嗎**——換過去之後整句的分段就是正解了嗎
//! 3. **要指定幾次**——不對的話再反白下一個錯段，最多試 `MAX_ROUNDS` 次
//!
//! # 用法
//!
//! ```text
//! cargo run --release -p ime-core --bin spike_seg_lang
//! cargo run --release -p ime-core --bin spike_seg_lang -- --verbose
//! ```

use ime_core::cutpoint::Segment;
use ime_core::input::Input;
use ime_core::language::Language;
use std::collections::BTreeMap;

#[path = "common/testdata.rs"]
mod testdata;

/// 使用者最多願意指定幾段。超過就當「這句救不回來」——真的要點五次
/// 才對的話，那個互動已經不比重打一次划算。
const MAX_ROUNDS: usize = 5;

/// 一節的模擬結果。
#[derive(Default)]
struct Tally {
    /// 這一節有幾句
    total: usize,
    /// 引擎第一名本來就對，不必動選單
    already: usize,
    /// 指定 n 次之後對了（`fixed[n-1]`）
    fixed: [usize; MAX_ROUNDS],
    /// 試到底還是不對
    failed: usize,
    /// **想指定的語言在清單裡找不到**的次數（不是句數，是點擊次數）
    missing: usize,
    /// 指定成功的次數
    hit: usize,
    /// **找到了、換過去卻是同一種切法**的次數——原地打轉，
    /// 代表這個過濾器對那一段無能為力
    stuck: usize,
}

fn main() {
    let verbose = std::env::args().any(|a| a == "--verbose");
    // 加上「往右吃一個字」的邊界操作。量它值不值得做。
    let bounds = std::env::args().any(|a| a == "--bounds");
    // **詞庫要先載**——不載的話合法性引擎什麼都認不得，切法全是亂的
    testdata::load_engine();
    let rows = testdata::load("spike_seg_lang");
    if rows.is_empty() {
        eprintln!("沒有測資");
        return;
    }

    let mut by_tag: BTreeMap<String, Tally> = BTreeMap::new();
    let mut all = Tally::default();
    // 找不到的案例，印給人看
    let mut misses: Vec<String> = Vec::new();
    // 試到底還是不對的案例
    let mut fails: Vec<String> = Vec::new();

    for row in &rows {
        let input = Input::from_keys(&row.keys, None);
        let cuttings: Vec<Vec<Segment>> = input.cuttings().to_vec();
        if cuttings.is_empty() {
            continue;
        }
        // 兩欄拼出正解的分段。拼不出來（寬容-段、段數對不上）就跳過——
        // 那種列沒有明確的「使用者意圖」可以模擬
        let Some(want) = parse_expect(row, &cuttings[0]) else {
            continue;
        };

        let t = by_tag.entry(row.tag.clone()).or_default();
        t.total += 1;
        all.total += 1;

        let mut cur = cuttings[0].clone();
        if same(&cur, &want) {
            t.already += 1;
            all.already += 1;
            continue;
        }

        let mut done = false;
        for round in 0..MAX_ROUNDS {
            // 使用者看著目前的分段，找第一個跟他想要的對不上的段
            let Some((idx, want_lang)) = first_wrong(&cur, &want) else {
                break;
            };
            // 那一段從按鍵串的哪裡開始
            let start: usize = cur[..idx].iter().map(|s| s.keys.chars().count()).sum();

            // 過濾器：清單裡有沒有「從這個位置起的那一段是他要的語言」
            // 的切法？有多個就取排名最前的（清單本來就是排好的）。
            //
            // **只鎖起點不鎖長度**——使用者反白的是引擎切的那一段，但他
            // 心裡想要的段可能更長或更短（`file` 被切成 `fi`＋`le`，他
            // 反白 `fi` 說「這是英文」，要的是整個 `file`）。鎖長度的話
            // 找到的永遠是同一種切法，原地打轉。
            //
            // `--bounds`：**加上邊界操作**。語言相同時，改找「這一段要
            // 比現在長」的切法——那是「往右吃一個字」的手勢。量它值不
            // 值得做進產品。
            let cur_len = cur[idx].keys.chars().count();
            let want_longer = bounds && cur[idx].lang == want_lang;
            let found = cuttings.iter().find(|c| {
                has_seg(c, start, want_lang)
                    && (!want_longer || seg_len_at(c, start).is_some_and(|n| n > cur_len))
            });
            match found {
                Some(c) => {
                    t.hit += 1;
                    all.hit += 1;
                    // **換過去卻沒有真的改變**就是原地打轉：過濾器找到的
                    // 是同一種切法。再試下去只是重複同一步，直接判失敗。
                    if *c == cur {
                        t.stuck += 1;
                        all.stuck += 1;
                        break;
                    }
                    cur = c.clone();
                }
                None => {
                    t.missing += 1;
                    all.missing += 1;
                    if verbose && misses.len() < 40 {
                        misses.push(format!(
                            "  {:<28} 第{}段 {:?} 想改成 {:?}（{}）",
                            row.keys,
                            idx + 1,
                            cur[idx].keys,
                            want_lang,
                            row.tag
                        ));
                    }
                    break;
                }
            }

            if same(&cur, &want) {
                t.fixed[round] += 1;
                all.fixed[round] += 1;
                done = true;
                break;
            }
        }
        if !done && !same(&cur, &want) {
            t.failed += 1;
            all.failed += 1;
            if verbose && fails.len() < 30 {
                fails.push(format!(
                    "  {}｜{}\n    正解 {}\n    卡在 {}",
                    row.tag,
                    row.keys,
                    show(&want),
                    show_segs(&cur),
                ));
            }
        }
    }

    println!("模擬：反白一段 → 指定語言 → 在既有切法清單裡找符合的\n");
    println!(
        "{:<18} {:>5} │ {:>6} {:>6} {:>6} {:>6} │ {:>7} {:>7} {:>7}",
        "節", "句數", "本來對", "指定1次", "指定2+", "救不回", "找得到", "找不到", "沒動靜"
    );
    for (tag, t) in &by_tag {
        print_row(tag, t);
    }
    println!();
    print_row("總計", &all);

    let clicks = all.hit + all.missing;
    if clicks > 0 {
        println!(
            "\n指定總次數 {clicks}：找得到 {} （{:.1}%），找不到 {} （{:.1}%）",
            all.hit,
            100.0 * all.hit as f64 / clicks as f64,
            all.missing,
            100.0 * all.missing as f64 / clicks as f64,
        );
    }
    let need_menu = all.total - all.already;
    if need_menu > 0 {
        let fixed: usize = all.fixed.iter().sum();
        println!(
            "需要動選單的 {need_menu} 句：{fixed} 句救得回來（{:.1}%），其中 {} 句只要指定一次",
            100.0 * fixed as f64 / need_menu as f64,
            all.fixed[0],
        );
    }

    if verbose && !misses.is_empty() {
        println!("\n找不到的例子（最多 40 筆）：");
        for m in &misses {
            println!("{m}");
        }
    }
    if verbose && !fails.is_empty() {
        println!("\n試到底還是不對的例子（最多 30 筆）：");
        for f in &fails {
            println!("{f}");
        }
    }
}

/// 期望的分段印成 `注:u3ru/ ｜英: ｜注:j06t/6`
fn show(want: &[(String, Language)]) -> String {
    want.iter()
        .map(|(k, l)| format!("{}:{k}", mark(*l)))
        .collect::<Vec<_>>()
        .join("｜")
}

fn show_segs(c: &[Segment]) -> String {
    c.iter()
        .map(|s| format!("{}:{}", mark(s.lang), s.keys))
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

fn print_row(name: &str, t: &Tally) {
    let two_plus: usize = t.fixed[1..].iter().sum();
    println!(
        "{:<18} {:>5} │ {:>6} {:>6} {:>6} {:>6} │ {:>7} {:>7} {:>7}",
        name, t.total, t.already, t.fixed[0], two_plus, t.failed, t.hit, t.missing, t.stuck
    );
}

/// 兩欄拼出正解的分段：按鍵欄給切點，文字欄給語言。
///
/// 拼不出來就回 `None`——寬容-段（`~`，不指定切法）、兩欄段數對不上、
/// 或按鍵接起來跟實際按的不一樣（測資本身有問題）都跳過。
///
/// `first` 是引擎第一名的切法，只用來**替純漢字段補語言**：日文與中文
/// 的漢字長得一樣，看輸出分不出來。那種段本來就沒有語言爭議（使用者
/// 不會去改一個已經是他要的漢字的段），拿引擎自己判的就好。
fn parse_expect(row: &testdata::Row, first: &[Segment]) -> Option<Vec<(String, Language)>> {
    // 寬容-段不指定切法，沒有「正解的分段」可言
    if row.want.starts_with('~') || row.want.contains(testdata::ALT) {
        return None;
    }
    // **分隔符空白也是一段**，不能濾掉——引擎的切法裡它自成一段
    // （`SEPARATOR`），濾掉的話後面每一段的按鍵位置都會偏掉，`lang_at`
    // 拿到的語言全錯。
    let keys = row.expected_segs();
    let texts: Vec<&str> = row
        .alts()
        .first()
        .copied()
        .unwrap_or(&row.want)
        .split('|')
        .map(str::trim)
        .collect();
    if keys.len() != texts.len() || keys.is_empty() {
        return None;
    }
    // 按鍵接起來要跟實際按的一樣，否則兩邊的位置對不上
    if keys.concat() != row.keys {
        return None;
    }

    let mut out: Vec<(String, Language)> = Vec::new();
    let mut pos = 0usize;
    for (k, t) in keys.iter().zip(&texts) {
        // 分隔符空白：引擎一律標成 English（`SEPARATOR` 那段），跟著寫
        let sep = *t == "_";
        let lang = if sep {
            Language::English
        } else {
            lang_of_text(t).or_else(|| lang_from_keys(k, first, pos))?
        };
        pos += k.chars().count();
        // **相鄰同語言的段要黏回去**——引擎給的切法是 `normalize` 過的
        // （同語言內部切不切不影響輸出，見 `cutpoint::normalize`），
        // 測資的切點卻是逐詞標的。不黏的話純中文長句永遠對不上：
        // 引擎給一段，測資寫五段。分隔符是 `is_mark`，不參與合併。
        let prev_sep = out.last().is_some_and(|(k, _)| k.trim().is_empty());
        match out.last_mut() {
            Some((prev, l)) if *l == lang && !sep && !prev_sep => prev.push_str(k),
            _ => out.push((k.clone(), lang)),
        }
    }
    Some(out)
}

/// 一段輸出的文字是哪個語言？漢字有歧義（中日共用）所以回 `None`，
/// 交給呼叫端拿引擎判的補。
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
    let mut kana = false;
    let mut latin = false;
    let mut han = false;
    for c in t.chars() {
        match c {
            // 平假名與片假名——只有日文有
            '\u{3040}'..='\u{30ff}' => kana = true,
            'A'..='Z' | 'a'..='z' => latin = true,
            '\u{4e00}'..='\u{9fff}' => han = true,
            _ => {}
        }
    }
    match (kana, latin, han) {
        (true, _, _) => Some(Language::Romaji),
        (false, true, false) => Some(Language::English),
        // 純漢字：中日不分，交給引擎
        _ => None,
    }
}

/// 引擎第一名的切法裡，按鍵位置 `pos` 落在哪一段、那段是什麼語言。
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

/// 切法跟期望一樣嗎？（比每段的按鍵與語言）
fn same(cur: &[Segment], want: &[(String, Language)]) -> bool {
    cur.len() == want.len()
        && cur
            .iter()
            .zip(want)
            .all(|(s, (k, l))| s.keys.trim() == k.trim() && s.lang == *l)
}

/// 目前的分段裡，第一個跟期望對不上的段是第幾個、應該是什麼語言。
///
/// 「對不上」有兩種：語言不同，或按鍵範圍不同（切點位置就錯了）。
/// 兩種都用**同一個動作**修——反白那一段、指定期望的語言，讓過濾器
/// 去找按鍵範圍對的那一種切法。
fn first_wrong(cur: &[Segment], want: &[(String, Language)]) -> Option<(usize, Language)> {
    let mut pos = 0usize;
    for (i, s) in cur.iter().enumerate() {
        let len = s.keys.chars().count();
        // 期望裡起點相同的那一段
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
            // 起點對齊、長度與語言都一樣 → 這段沒問題
            Some((wlen, l)) if wlen == len && l == s.lang => {}
            // 起點對齊但長度或語言不對 → 就是它
            Some((_, l)) => return Some((i, l)),
            // 起點根本對不齊（前面的段長度就錯了）→ 拿期望裡涵蓋這個
            // 起點的那一段的語言
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

/// 這種切法裡，按鍵位置 `start` 正好是一段的開頭，而且那一段是 `lang` 嗎？
///
/// **不管那段多長**——長度是引擎決定的，使用者只指定語言。
/// 這種切法裡，從 `start` 起的那一段有多長？`start` 不是段的開頭就回 `None`。
fn seg_len_at(c: &[Segment], start: usize) -> Option<usize> {
    let mut pos = 0usize;
    for s in c {
        let n = s.keys.chars().count();
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

fn has_seg(c: &[Segment], start: usize, lang: Language) -> bool {
    let mut pos = 0usize;
    for s in c {
        if pos == start {
            return s.lang == lang;
        }
        if pos > start {
            return false;
        }
        pos += s.keys.chars().count();
    }
    false
}
