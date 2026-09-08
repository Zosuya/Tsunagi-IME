//! 段選單計分器：真的用 `Session` 跑「選了就凍」，量正確率與效能。
//!
//! # 為什麼要有這支
//!
//! `spike_seg_freeze` 是**模擬**——自己拼前區、自己叫 `Incremental`，
//! 而那個模擬始終跑不對（`long` 節 0/57，跟理論上限 57/57 差太多）。
//! 段選單實作出來之後不必再模擬：直接開 `Session`、按方向鍵、按 Enter，
//! 量的就是使用者真的會遇到的東西。
//!
//! # 量兩件事
//!
//! 1. **正確率**：模擬使用者從左到右逐段修，最多 `MAX_PICKS` 段，
//!    問「能不能修到正解」。對照組是引擎第一名（不動選單）。
//! 2. **效能**：每按一次 Enter（前區定案、後區重算）要多久。
//!    **判準是一幀 16ms**，跟 `bench_typing` 一致——超過使用者感覺得到。
//!
//! # 使用者怎麼選
//!
//! 從左到右找第一個跟正解對不上的段，在那一段的候選裡挑正解要的那個。
//! 挑不到就放棄（現實中使用者也只能放棄，或退回去重打）。
//!
//! # 用法
//!
//! ```text
//! cargo run --release -p ime-core --bin check_segmenu
//! cargo run --release -p ime-core --bin check_segmenu -- --verbose
//! ```

use ime_core::cutpoint::Segment;
use ime_core::language::Language;
use ime_core::session::Session;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

#[path = "common/testdata.rs"]
mod testdata;

/// 使用者最多願意選幾段。長句 20 段，給足空間。
const MAX_PICKS: usize = 30;

/// 一幀。超過使用者感覺得到頓（跟 `bench_typing` 同一個判準）。
const FRAME_BUDGET: Duration = Duration::from_millis(16);

#[derive(Default)]
struct Tally {
    total: usize,
    /// 引擎第一名就對，不必開選單
    already: usize,
    /// 用段選單修到正解
    fixed: usize,
    /// 修不到
    failed: usize,
    /// 修好的那些，總共選了幾段
    picks: usize,
    /// 每次 Enter（定案＋重算）的耗時
    times: Vec<Duration>,
}

fn main() {
    let verbose = std::env::args().any(|a| a == "--verbose");
    testdata::load_engine();
    let rows = testdata::load("check_segmenu");
    if rows.is_empty() {
        eprintln!("沒有測資");
        return;
    }

    let mut by_tag: BTreeMap<String, Tally> = BTreeMap::new();
    let mut all = Tally::default();
    let mut gained: Vec<String> = Vec::new();
    let mut lost: Vec<String> = Vec::new();

    for row in &rows {
        let mut s = Session::new();
        for c in row.keys.chars() {
            s.push(c);
        }
        let first = s.seg_segments();
        if first.is_empty() {
            continue;
        }
        let Some(want) = parse_expect(row, &first) else {
            continue;
        };

        let t = by_tag.entry(row.tag.clone()).or_default();
        t.total += 1;
        all.total += 1;

        if same(&first, &want) {
            t.already += 1;
            all.already += 1;
            continue;
        }

        // ── 模擬使用者逐段修 ──
        let mut picks = 0usize;
        let mut ok = false;
        for _ in 0..MAX_PICKS {
            let cur = s.seg_segments();
            if same(&cur, &want) {
                ok = true;
                break;
            }
            // 第一個對不上的段（只看還沒定案的部分）
            let Some(idx) = first_wrong(&cur, &want, s.seg_index()) else {
                break;
            };
            // 反白移過去
            while s.seg_index() < idx {
                let before = s.seg_index();
                s.seg_right();
                if s.seg_index() == before {
                    break;
                }
            }
            if s.seg_index() != idx {
                break;
            }
            // 這一段的候選裡，有正解要的那個嗎？
            let start: usize = cur[..idx].iter().map(|x| x.keys.chars().count()).sum();
            let Some((wkeys, wlang)) = seg_of_want(&want, start) else {
                break;
            };
            // **清單裡找不到就試著用 `Shift+←→` 推過去**。
            //
            // 候選清單為了不被灌爆有過濾（含數字的英文段），而推邊界是
            // 使用者明確的意圖、繞得過那個過濾——`usb3`／`sha256`／
            // `2fa` 這類技術詞就是這樣救回來的。
            //
            // 使用者真的會這樣做：他看到清單裡沒有 `usb3` 只有 `usb`，
            // 直覺就是「再推一格」。
            if !s
                .seg_cands()
                .iter()
                .any(|c| c.keys.trim() == wkeys.trim() && c.lang == wlang)
            {
                let want_len = wkeys.chars().count();
                // 最多推 `MAX_SEG_LEN` 次（推不動就會停在原地）
                for _ in 0..24 {
                    let now_keys = s.seg_cands()[s.seg_cand_index()].keys.clone();
                    let now = now_keys.chars().count();
                    // **只比長度**：推邊界的過程中語言可能變（`sha` 是
                    // 合法日文 mora、`sha2` 只能是英文），那是引擎照
                    // 合法性給的，不算「推錯了」
                    if now == want_len {
                        break;
                    }
                    if now < want_len {
                        s.seg_widen();
                    } else {
                        s.seg_narrow();
                    }
                    // 推不動了（長度沒變）
                    if s.seg_cands()[s.seg_cand_index()].keys.chars().count() == now {
                        break;
                    }
                }
            }
            let cands = s.seg_cands();
            let Some(pick) = cands
                .iter()
                .position(|c| c.keys.trim() == wkeys.trim() && c.lang == wlang)
            else {
                if verbose && lost.len() < 40 {
                    lost.push(format!(
                        "  {}｜{}\n    第{}段想要 {:?}({})，候選只有 {:?}",
                        row.tag,
                        row.keys,
                        idx + 1,
                        wkeys,
                        mark(wlang),
                        cands.iter().map(|c| c.keys.clone()).collect::<Vec<_>>(),
                    ));
                }
                break;
            };
            s.seg_set_cand(pick);
            // **這裡就是要量的**：定案＋後區重算
            let t0 = Instant::now();
            s.seg_confirm();
            let dt = t0.elapsed();
            t.times.push(dt);
            all.times.push(dt);
            picks += 1;
        }
        if !ok && same(&s.seg_segments(), &want) {
            ok = true;
        }

        if ok {
            t.fixed += 1;
            t.picks += picks;
            all.fixed += 1;
            all.picks += picks;
            if verbose && gained.len() < 15 && picks > 0 {
                gained.push(format!("  {:<30} 選 {picks} 段", row.keys));
            }
        } else {
            t.failed += 1;
            all.failed += 1;
        }
    }

    println!("段選單（選了就凍）：能修到正解嗎？重算會不會卡？\n");
    println!(
        "{:<16} {:>5} {:>5} │ {:>5} {:>5} {:>7} │ {:>9} {:>9}",
        "節", "句數", "免選", "修好", "修不到", "平均選幾段", "重算·p99", "重算·最慢"
    );
    println!("{}", "─".repeat(78));
    for (tag, t) in &by_tag {
        print_row(tag, t);
    }
    println!("{}", "─".repeat(78));
    print_row("總計", &all);

    let need = all.total - all.already;
    if need > 0 {
        println!(
            "\n需要動選單的 {need} 句：**修好 {}（{:.1}%）**，修不到 {}",
            all.fixed,
            100.0 * all.fixed as f64 / need as f64,
            all.failed,
        );
        if all.fixed > 0 {
            println!(
                "  修好的平均選 {:.1} 段",
                all.picks as f64 / all.fixed as f64
            );
        }
    }
    println!(
        "\n整體通過率：{}/{}（{:.1}%）＝ 免選 {} ＋ 用選單修好 {}",
        all.already + all.fixed,
        all.total,
        100.0 * (all.already + all.fixed) as f64 / all.total.max(1) as f64,
        all.already,
        all.fixed,
    );

    // ── 效能 ──
    let mut times = all.times.clone();
    let p99 = percentile(&mut times, 0.99);
    let worst = all.times.iter().max().copied().unwrap_or_default();
    let over = all.times.iter().filter(|d| **d > FRAME_BUDGET).count();
    println!(
        "\n重算 {} 次：中位 {:.2?}、p99 {:.2?}、最慢 {:.2?}",
        all.times.len(),
        percentile(&mut all.times.clone(), 0.5),
        p99,
        worst,
    );
    println!(
        "超過一幀（{FRAME_BUDGET:?}）的次數：{over}（{:.2}%）",
        100.0 * over as f64 / all.times.len().max(1) as f64
    );
    println!(
        "{}",
        if p99 < FRAME_BUDGET {
            "**p99 在預算內**——重算不會造成感覺得到的頓。"
        } else {
            "**p99 超出預算**——重算會頓，要想辦法。"
        }
    );

    if verbose && !gained.is_empty() {
        println!("\n修好的例子：");
        for g in &gained {
            println!("{g}");
        }
    }
    if verbose && !lost.is_empty() {
        println!("\n候選裡找不到正解的例子：");
        for l in &lost {
            println!("{l}");
        }
    }
}

fn print_row(name: &str, t: &Tally) {
    let mut times = t.times.clone();
    let avg = if t.fixed == 0 {
        "─".to_string()
    } else {
        format!("{:.1}", t.picks as f64 / t.fixed as f64)
    };
    println!(
        "{:<16} {:>5} {:>5} │ {:>5} {:>5} {:>7} │ {:>9.2?} {:>9.2?}",
        name,
        t.total,
        t.already,
        t.fixed,
        t.failed,
        avg,
        percentile(&mut times, 0.99),
        t.times.iter().max().copied().unwrap_or_default(),
    );
}

fn percentile(v: &mut [Duration], p: f64) -> Duration {
    if v.is_empty() {
        return Duration::ZERO;
    }
    v.sort_unstable();
    v[((v.len() as f64 - 1.0) * p).round() as usize]
}

/// 第一個跟正解對不上的段，從 `from` 起算（前面的已經定案）。
fn first_wrong(cur: &[Segment], want: &[(String, Language)], from: usize) -> Option<usize> {
    let mut pos: usize = cur[..from.min(cur.len())]
        .iter()
        .map(|s| s.keys.chars().count())
        .sum();
    for (i, s) in cur.iter().enumerate().skip(from) {
        let len = s.keys.chars().count();
        match seg_of_want(want, pos) {
            Some((k, l)) if k.chars().count() == len && l == s.lang => {}
            // 對不上（內容不同，或正解在這裡根本不是段界）
            _ => return Some(i),
        }
        pos += len;
    }
    None
}

fn seg_of_want(want: &[(String, Language)], start: usize) -> Option<(String, Language)> {
    let mut pos = 0usize;
    for (k, l) in want {
        if pos == start {
            return Some((k.clone(), *l));
        }
        if pos > start {
            return None;
        }
        pos += k.chars().count();
    }
    None
}

fn mark(l: Language) -> &'static str {
    match l {
        Language::Bopomofo => "注",
        Language::Romaji => "日",
        Language::English => "英",
    }
}

// ── 正解的判準跟 spike_seg_* 那批一致 ──

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
        // **標點與分隔符不參與合併**——跟 `cutpoint::normalize` 同一條
        // 規則（`Segment::is_mark`：「標點與分隔符一律自成一段、不參與
        // 任何合併」）。
        //
        // 漏掉這個條件的話 `。` 跟 `，` 會被黏成一段（兩者都是 English），
        // 而引擎正確地給了兩段，比對就永遠失敗——實測回報「記事本裡
        // 標點各是一段」正是這個。
        let is_mark = |s: &str| s.trim().is_empty() || is_punct_seg(s);
        let prev_mark = out.last().is_some_and(|(k, _)| is_mark(k));
        let cur_mark = sep || is_mark(k);
        match out.last_mut() {
            Some((prev, l)) if *l == lang && !cur_mark && !prev_mark => prev.push_str(k),
            _ => out.push((k.clone(), lang)),
        }
    }
    Some(out)
}

/// 這一段是標點嗎？（一個字元、而且是 ASCII 標點）
///
/// 切點引擎的標點一律自成一段，所以只要看單一字元就夠。
fn is_punct_seg(s: &str) -> bool {
    let mut it = s.chars();
    matches!((it.next(), it.next()), (Some(c), None) if c.is_ascii_punctuation())
}

/// 純漢字段的語言：中日的漢字長得一樣，看輸出分不出來。
///
/// **先問合法性引擎，再退回引擎第一名的判斷**。直接用第一名是錯的
/// ——那正是引擎搞錯的地方（`loggerul4` 的 `ul4` 被判成日文，測資
/// 寫的卻是注音的「要」）。
fn lang_from_keys(keys: &str, first: &[Segment], pos: usize) -> Option<Language> {
    let zh = ime_core::bopomofo::validity(keys) == ime_core::bopomofo::Validity::Valid;
    let ja = ime_core::romaji::validity(keys) == ime_core::romaji::Validity::Valid;
    match (zh, ja) {
        (true, false) => Some(Language::Bopomofo),
        (false, true) => Some(Language::Romaji),
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

/// 分段跟正解一樣嗎？
///
/// # 空白算誰的不重要
///
/// `英:vpn｜英:␣｜注:需要` 與 `英:vpn␣｜注:需要` **送出的文字完全一樣**
/// ——空白自成一段或黏在前一段，輸出沒有差別。測資寫成獨立一段，引擎
/// 常常黏在前面，硬比段數會把正確的判成錯的（實測 4 句敗在這）。
///
/// 所以比對前先**濾掉純空白的段、並把每段的空白 trim 掉**。這跟
/// `cutpoint::normalize` 是同一種精神：不計較對輸出沒影響的差異。
fn same(cur: &[Segment], want: &[(String, Language)]) -> bool {
    let a: Vec<(String, Language)> = cur
        .iter()
        .filter(|s| !s.keys.trim().is_empty())
        .map(|s| (s.keys.trim().to_string(), s.lang))
        .collect();
    let b: Vec<(String, Language)> = want
        .iter()
        .filter(|(k, _)| !k.trim().is_empty())
        .map(|(k, l)| (k.trim().to_string(), *l))
        .collect();
    a == b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(keys: &str, lang: Language) -> Segment {
        Segment {
            keys: keys.into(),
            is_mark: keys.trim().is_empty(),
            lang,
        }
    }

    /// **空白算誰的不影響判定**——輸出一樣就算對。
    #[test]
    fn 空白自成一段或黏在前面都算對() {
        use Language::*;
        let want = vec![
            ("vpn".to_string(), English),
            (" ".to_string(), English),
            ("xu06".to_string(), Bopomofo),
        ];
        // 空白黏在前面
        assert!(same(&[seg("vpn ", English), seg("xu06", Bopomofo)], &want));
        // 空白自成一段
        assert!(same(
            &[
                seg("vpn", English),
                seg(" ", English),
                seg("xu06", Bopomofo)
            ],
            &want
        ));
    }

    /// **但語言不同就是不同**——判準只放寬空白，不放寬別的。
    #[test]
    fn 語言不同仍然算錯() {
        use Language::*;
        let want = vec![("sushi".to_string(), Romaji)];
        assert!(
            !same(&[seg("sushi", English)], &want),
            "日文 vs 英文不該算對"
        );
    }

    /// **切點不同也是不同**。
    #[test]
    fn 切點不同仍然算錯() {
        use Language::*;
        let want = vec![
            ("logger".to_string(), English),
            ("ul4".to_string(), Bopomofo),
        ];
        assert!(
            !same(&[seg("log", English), seg("gerul4", Bopomofo)], &want),
            "切在不同位置不該算對"
        );
    }

    /// 標點各自一段，不該被黏起來（`cutpoint::normalize` 的規則）。
    #[test]
    fn 標點不參與合併() {
        assert!(is_punct_seg("."), "單一標點");
        assert!(is_punct_seg(","), "單一標點");
        assert!(!is_punct_seg(".,"), "兩個標點不是「一個標點段」");
        assert!(!is_punct_seg("ab"), "字母不是標點");
    }
}
