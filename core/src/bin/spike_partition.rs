//! spike：切法改成「按高信心邊界分段、每段各自算」會怎樣？
//!
//! # 要驗證什麼
//!
//! §2.51 的設計是**分區**——引擎用高信心邊界把按鍵串切成幾段，每段各自
//! 算切法，Tab 的選單按段組織。動手改產品之前要先確認兩件事：
//!
//! 1. **機制可行嗎**——對子串重開 `Incremental` 算出來的切法，能不能
//!    拼回一份完整的分段，讓 `compose` 照樣吃得下？
//! 2. **正確率會怎麼動**——這是重點。§2.51.7 手動模擬顯示混語言長句會
//!    大幅改善，但那只有兩句；要跑完整測資才知道有沒有賠掉別的。
//!
//! # 怎麼分段
//!
//! 只用兩種**不必先有切法就判斷得出來**的邊界（跟 `check_freeze
//! --segments` 同一套）：
//!
//! - **標點**（`punct::is_punct`）——自成一段，`is_mark`
//! - **真分隔符空白**——注音的一聲也是空白（`rup␣wu0␣` 今天），所以要先問
//!   `space::tone_suffix_start`：收得了前面音節的尾就不是邊界，把空白吃
//!   進段裡。這條沒做對的話「今天」會被切成「今」「天」，正好是舊架構
//!   那個「金天」（§4.37）。
//!
//! 語言切換點也是高信心邊界，但**這裡不用**——它要先有切法才知道，是
//! 雞生蛋。所以這支量到的是**保守下界**：真做進產品時段會更短、效果只會
//! 更好。
//!
//! # 光有「高信心邊界」不夠，還要離游標夠遠
//!
//! 第一版把**每一個**邊界都拿來分區，結果比現況差 37 句（85.1% → 80.4%）。
//! 失敗的形狀全都一樣：
//!
//! ```text
//! bugu␣vu84    現況 bug一下   分區 ぶぐ␣下
//! tomodachivm␣ul4  現況 友達需要   分區 友達vm␣要
//! ```
//!
//! `bugu␣` 這一段**獨立算**會判成日文 `ぶぐ`；在整串脈絡下才會切成
//! `英:bug | 注:u␣`。**切點位置一樣，但段內排序需要後文**——後面接著
//! 注音的話，`英:bug | 注:u␣vu84` 兩段中文能湊成詞，分數才贏得了。
//!
//! 所以「高信心邊界」只保證**這裡是一個切點**，不保證**切開之後兩邊
//! 各自算會得到同樣的結果**。分區點還必須滿足 §2.51.5 量出來的條件：
//! **離游標 16 個按鍵以外**。近處的邊界不能分——那裡的排序還在跟後文
//! 互動。
//!
//! `SAFE_DIST` 就是這件事。短句（不足 16 鍵）完全不分區，行為不變。
//!
//! # 用法
//!
//! ```text
//! cargo run --release -p ime-core --bin spike_partition
//! cargo run --release -p ime-core --bin spike_partition -- <額外的測資檔>
//! ```

use ime_core::compose;
use ime_core::cutpoint::{incremental::Incremental, normalize, punct, rank, space, Segment};
use ime_core::language::Language;

const TAB: char = '\u{9}';
const ALT: &str = "||";

/// 一塊：要算切法的區塊，或自成一段的邊界符號。
enum Piece {
    Block(String),
    Mark(String),
}

/// 分區點必須離游標這麼遠。
///
/// §2.51.5 量**切點**穩定距離得到 16，但這支量的是**文字**——
/// `compose::apply_word_context` 會跨段修字，所以文字層需要的距離更長。
/// 掃描 790 句：16 還會弄壞 1 句，**20 起完全無害**。
const SAFE_DIST: usize = 20;

/// 用高信心邊界把按鍵串切塊。
///
/// 兩個條件都要滿足才切：
///
/// 1. **是高信心邊界**——標點，或收不了前面音節尾的空白
/// 2. **離結尾 `SAFE_DIST` 鍵以外**——近處的排序還在跟後文互動，
///    切開會讓 `bugu␣` 這種段失去「後面接注音」的資訊
///
/// 不滿足第 2 條的邊界照樣留在字串裡，只是**不當分區點**——那一段交給
/// `Incremental` 自己去切，跟現況一模一樣。
fn pieces(keys: &str, safe: usize) -> Vec<Piece> {
    let chars: Vec<char> = keys.chars().collect();
    let n = chars.len();
    let mut out = Vec::new();
    let mut cur = String::new();
    for (i, &c) in chars.iter().enumerate() {
        // 離結尾夠遠嗎？不夠的話這個位置不當分區點
        let far = n.saturating_sub(i) > safe;
        if c == ' ' {
            if !cur.is_empty() && space::tone_suffix_start(&cur).is_some() {
                cur.push(' '); // 這個空白是聲調，不是邊界
                continue;
            }
            if !far {
                cur.push(' ');
                continue;
            }
            if !cur.is_empty() {
                out.push(Piece::Block(std::mem::take(&mut cur)));
            }
            out.push(Piece::Mark(" ".to_string()));
        } else if punct::is_punct(keys, i) {
            if !far {
                cur.push(c);
                continue;
            }
            if !cur.is_empty() {
                out.push(Piece::Block(std::mem::take(&mut cur)));
            }
            out.push(Piece::Mark(c.to_string()));
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        out.push(Piece::Block(cur));
    }
    out
}

/// 分區版的第一名切法：每個區塊各自算，再串接。
fn top1_partitioned(keys: &str, safe: usize) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    for p in pieces(keys, safe) {
        match p {
            Piece::Block(b) => {
                let cands = rank::sort(Incremental::from_keys(&b).cuttings());
                match cands.first() {
                    Some(c) => out.extend(c.iter().cloned()),
                    // 整塊都被丟棄規則殺光（理論上不該發生），保底當英文
                    None => out.push(Segment {
                        keys: b,
                        lang: Language::English,
                        is_mark: false,
                    }),
                }
            }
            Piece::Mark(m) => out.push(Segment {
                keys: m,
                lang: Language::English,
                is_mark: true,
            }),
        }
    }
    out
}

/// 現況的第一名切法：整串一次算。
fn top1_whole(keys: &str) -> Vec<Segment> {
    rank::sort(Incremental::from_keys(keys).cuttings())
        .first()
        .cloned()
        .unwrap_or_default()
}

/// 文字期望欄 → 期望字串：`|` 只是分隔、`_` 代表空白
fn expected_text(want: &str) -> String {
    want.replace('|', "").replace('_', " ")
}

fn strip_spaces(s: &str) -> String {
    s.chars()
        .filter(|c| *c != ' ' && *c != '\u{3000}')
        .collect()
}

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

/// 這一句的判定結果。
struct Verdict {
    hit: bool,
    /// 錯字數（跟最接近的那個期望比）
    errs: usize,
    /// 期望字元數（去空白）
    want_chars: usize,
    got: String,
}

/// 拿一份切法去組字、跟期望比。判定規則照 `check_compose`。
fn judge(segs: &[Segment], want_col: &str) -> Verdict {
    let top1 = normalize(segs);
    let got = compose::text_of(&compose::compose(&top1));

    let alts: Vec<&str> = want_col.split(ALT).collect();
    let hit = alts.iter().any(|alt| {
        let tol = alt.starts_with('~');
        let e = expected_text(alt.trim_start_matches('~'));
        if tol {
            strip_spaces(&e) == strip_spaces(&got)
        } else {
            e == got
        }
    });

    // 錯字率跟最接近的那個期望比
    let (errs, want_chars) = alts
        .iter()
        .map(|alt| {
            let e = strip_spaces(&expected_text(alt.trim_start_matches('~')));
            (edit_distance(&e, &strip_spaces(&got)), e.chars().count())
        })
        .min_by_key(|(d, _)| *d)
        .unwrap_or((0, 0));

    Verdict {
        hit,
        errs,
        want_chars,
        got,
    }
}

fn pct(c: usize, total: usize) -> f64 {
    if total == 0 {
        return 0.0;
    }
    100.0 * c as f64 / total as f64
}

fn main() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data");
    ime_core::english::load(&data);
    ime_core::dict::load_bopomofo(&data);
    // 選字的中文 bigram——計分器一定要載，不然量到的是「關掉模型」的行為
    ime_core::lm::load(&data, ime_core::dict::char_freq_map(&data));
    ime_core::dict::load_japanese(&data);

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let files = [
        "mixed_cutpoint",
        "mixed_kana_fragment",
        "mixed_daily",
        "mixed_trilingual",
        "mixed_en_vowel",
        "mixed_en_split",
        "mixed_ja_en",
        "mixed_otaku",
        "mixed_holdout",
        "mixed_japanese_verbs",
        "mixed_en_bopomofo",
        "mixed_symbol",
        "mixed_long",
        "bopomofo_sentences",
        "bopomofo_words",
    ];
    let argv: Vec<String> = std::env::args().skip(1).collect();
    // `--show <檔>`：不評分，只印「現況 vs 分區」的輸出對照。
    // 壓力句沒有經過人審的期望欄（測資的期望一律要人審），所以那批只能
    // 用肉眼比對——這是質性驗證，跟測資那邊的量化驗證分開。
    if argv.iter().any(|a| a == "--bench") {
        bench_ja(12);
        return;
    }
    if argv.iter().any(|a| a == "--rank") {
        let extra: Vec<String> = argv
            .iter()
            .filter(|a| !a.starts_with("--"))
            .cloned()
            .collect();
        let dir2 = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
        let ps: Vec<std::path::PathBuf> = if extra.is_empty() {
            files
                .iter()
                .map(|f| dir2.join(format!("{f}.txt")))
                .collect()
        } else {
            extra.iter().map(std::path::PathBuf::from).collect()
        };
        rank_mode(&ps, SAFE_DIST);
        return;
    }
    if let Some(i) = argv.iter().position(|a| a == "--show") {
        let safe = SAFE_DIST;
        for f in &argv[i + 1..] {
            let Ok(content) = std::fs::read_to_string(f) else {
                eprintln!("讀不到 {f}");
                continue;
            };
            for line in content.lines() {
                let line = line.trim_end_matches(['\u{d}', '\u{a}']);
                if line.trim().is_empty() || line.starts_with('#') {
                    continue;
                }
                let cols: Vec<&str> = line.split(TAB).collect();
                if cols.len() < 3 || cols[2].trim().is_empty() {
                    continue;
                }
                let keys = cols[2];
                let a = compose::text_of(&compose::compose(&normalize(&top1_whole(keys))));
                let b =
                    compose::text_of(&compose::compose(&normalize(&top1_partitioned(keys, safe))));
                let mark = if a == b { "  " } else { "≠ " };
                println!("{mark}{keys}\n    現況 {a}\n    分區 {b}");
            }
        }
        return;
    }
    let extra: Vec<String> = argv;
    let paths: Vec<std::path::PathBuf> = if extra.is_empty() {
        files.iter().map(|f| dir.join(format!("{f}.txt"))).collect()
    } else {
        extra.iter().map(std::path::PathBuf::from).collect()
    };

    // 掃一遍安全距離：0 = 每個邊界都分（第一版，已知會變差）
    const SWEEP: [usize; 7] = [0, 4, 8, 12, 16, 20, 24];

    // 先把測資讀進來，免得每個 safe 值都重讀一次
    let mut rows: Vec<(String, String)> = Vec::new();
    for f in paths {
        let Ok(content) = std::fs::read_to_string(&f) else {
            eprintln!("讀不到 {}", f.display());
            continue;
        };
        for line in content.lines() {
            let line = line.trim_end_matches(['\u{d}', '\u{a}']);
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let cols: Vec<&str> = line.split(TAB).collect();
            if cols.len() < 3 || cols[2].trim().is_empty() {
                continue;
            }
            rows.push((cols[0].to_string(), cols[2].to_string()));
        }
    }

    // 一句都沒讀到就不要出數字——**這一支在測資合併之後就一直是空跑**。
    //
    // 底下那份 `files` 是 2026-09-06 量 §2.51 時的檔名，commit 49181d2
    // 把十幾個 `mixed_*.txt` 併成一份 `測資.txt` 並改用共用載入器
    // （`common/testdata.rs`），漏了這一支。它從那天起讀不到任何檔，
    // 卻照樣印出「句數 0」的完整掃描表、exit code 還是 0。
    //
    // 真正的修法是接 `testdata::load`，但 `rank_mode` 也自己讀檔、
    // 還要期望欄，改它等於動到 §2.51 已經記錄的量測口徑，要先提案。
    // 在那之前先讓它閉嘴，不要假裝有跑。
    if rows.is_empty() {
        eprintln!("✗ 一句測資都沒讀到，這一輪什麼都沒量到。");
        eprintln!();
        eprintln!("  預設讀的是測資合併前的舊檔名（mixed_*.txt），那些檔在");
        eprintln!("  commit 49181d2 就併進 core/testdata/測資.txt 了。");
        eprintln!("  暫時的用法：把要量的檔案路徑當參數帶進來。");
        std::process::exit(1);
    }

    // 現況基準
    let mut hit_w = 0usize;
    let mut err_w = 0usize;
    let mut want_total = 0usize;
    let mut base: Vec<bool> = Vec::new();
    for (want, keys) in &rows {
        let w = judge(&top1_whole(keys), want);
        hit_w += usize::from(w.hit);
        err_w += w.errs;
        want_total += w.want_chars;
        base.push(w.hit);
    }

    let n = rows.len();
    println!("=== 分區 vs 現況（整串一次算）===\n");
    println!("  句數  {n}    期望 {want_total} 字\n");
    println!(
        "  現況        整段正確 {hit_w}（{:.1}%）   錯字率 {:.1}%\n",
        pct(hit_w, n),
        pct(err_w, want_total)
    );
    println!("  安全距離   整段正確        差    錯字率   修好  弄壞");

    let mut best: (usize, usize) = (0, 0);
    for safe in SWEEP {
        let mut hit = 0usize;
        let mut err = 0usize;
        let (mut fixed, mut broke) = (0usize, 0usize);
        for (i, (want, keys)) in rows.iter().enumerate() {
            let v = judge(&simulate(keys, safe), want);
            hit += usize::from(v.hit);
            err += v.errs;
            match (base[i], v.hit) {
                (false, true) => fixed += 1,
                (true, false) => broke += 1,
                _ => {}
            }
        }
        println!(
            "  {safe:>6}      {hit:>6}（{:>4.1}%）  {:>+5}    {:>4.1}%   {fixed:>4}  {broke:>4}",
            pct(hit, n),
            hit as i64 - hit_w as i64,
            pct(err, want_total)
        );
        if hit > best.1 {
            best = (safe, hit);
        }
    }

    // 最好的那個安全距離，把差異案例印出來
    let safe = best.0;
    println!("\n=== 安全距離 {safe} 的差異案例 ===\n");
    let (mut fixed, mut broke) = (Vec::new(), Vec::new());
    for (i, (want, keys)) in rows.iter().enumerate() {
        let v = judge(&simulate(keys, safe), want);
        if !base[i] && v.hit {
            let w = judge(&top1_whole(keys), want);
            fixed.push((keys.clone(), w.got, v.got));
        } else if base[i] && !v.hit {
            let w = judge(&top1_whole(keys), want);
            broke.push((keys.clone(), w.got, v.got));
        }
    }
    println!("  修好 {} 句：", fixed.len());
    for (k, a, b) in fixed.iter().take(8) {
        let ks: String = k.chars().take(44).collect();
        println!("    {ks}\n      現況 {a}\n      分區 {b}");
    }
    println!("\n  弄壞 {} 句：", broke.len());
    for (k, a, b) in broke.iter().take(8) {
        let ks: String = k.chars().take(44).collect();
        println!("    {ks}\n      現況 {a}\n      分區 {b}");
    }
}

/// `--rank` 模式：分區之後，正解在候選裡排第幾？
///
/// 現況的名次是「整串 N 種相異分段裡排第幾」；分區之後不是單一清單，而是
/// **每段各自一份**，使用者要在每段各選一次。所以可比的指標是
/// **「到達正解總共要按幾次選擇鍵」**：現況是 `名次 − 1`，分區是各段的
/// `名次 − 1` 相加。
///
/// 另外多一個現況沒有的失敗模式：**分區點切斷正解的段**。那時正解在任何
/// 一段裡都湊不出來，等於涵蓋失敗——這是分區獨有的風險，要單獨報。
fn rank_mode(paths: &[std::path::PathBuf], safe: usize) {
    /// 一段候選清單裡，正解排第幾（1-based）。
    fn rank_in(keys: &str, want: &[String]) -> Option<usize> {
        let cands = rank::sort(Incremental::from_keys(keys).cuttings());
        let mut seen = std::collections::HashSet::new();
        cands
            .iter()
            .map(|c| normalize(c))
            .filter(|c| seen.insert(format!("{c:?}")))
            .position(|c| c.iter().map(|s| s.keys.clone()).collect::<Vec<_>>() == want)
            .map(|i| i + 1)
    }

    /// 把正解依塊邊界分組。某段跨越邊界就回 `None`——分區切斷了正解。
    fn group(expected: &[String], blocks: &[(usize, usize, bool)]) -> Option<Vec<Vec<String>>> {
        let mut out = vec![Vec::new(); blocks.len()];
        let mut pos = 0usize;
        for seg in expected {
            let len = seg.chars().count();
            let (a, b) = (pos, pos + len);
            // 這一段整個落在哪一塊裡？跨界就失敗
            let idx = blocks.iter().position(|&(s, e, _)| a >= s && b <= e)?;
            out[idx].push(seg.clone());
            pos = b;
        }
        Some(out)
    }

    let mut n = 0usize;
    let (mut cov_w, mut cov_p) = (0usize, 0usize);
    let (mut first_w, mut first_p) = (0usize, 0usize);
    let (mut press_w, mut press_p) = (0usize, 0usize);
    let mut cut_thru = 0usize;
    // 真的被分成多於一塊的句子——沒有的話這整個比較就是 no-op
    let mut partitioned = 0usize;
    let mut worse: Vec<(String, usize, usize)> = Vec::new();
    let mut cut_samples: Vec<(String, String)> = Vec::new();

    for f in paths {
        let Ok(content) = std::fs::read_to_string(f) else {
            continue;
        };
        for line in content.lines() {
            let line = line.trim_end_matches(['\u{d}', '\u{a}']);
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let cols: Vec<&str> = line.split(TAB).collect();
            if cols.len() < 3 || cols[2].trim().is_empty() {
                continue;
            }
            let (key_expect, keys) = (cols[1], cols[2]);
            let expected: Vec<String> = key_expect
                .split('|')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            if expected.is_empty() {
                continue;
            }
            n += 1;

            // 現況
            let rw = rank_in(keys, &expected);
            if let Some(r) = rw {
                cov_w += 1;
                press_w += r - 1;
                if r == 1 {
                    first_w += 1;
                }
            }

            // 分區：先把塊的範圍算出來
            let mut blocks: Vec<(usize, usize, bool)> = Vec::new();
            let mut pos = 0usize;
            let ps = pieces(keys, safe);
            for p in &ps {
                let (text, is_block) = match p {
                    Piece::Block(b) => (b.as_str(), true),
                    Piece::Mark(m) => (m.as_str(), false),
                };
                let len = text.chars().count();
                blocks.push((pos, pos + len, is_block));
                pos += len;
            }

            if blocks.len() > 1 {
                partitioned += 1;
            }
            let Some(groups) = group(&expected, &blocks) else {
                cut_thru += 1;
                if cut_samples.len() < 6 {
                    let bs: Vec<String> = blocks
                        .iter()
                        .map(|&(a, b, is_blk)| {
                            let t: String = keys.chars().skip(a).take(b - a).collect();
                            if is_blk {
                                t
                            } else {
                                format!("[{t}]")
                            }
                        })
                        .collect();
                    cut_samples.push((expected.join("|"), bs.join(" / ")));
                }
                continue;
            };

            let mut total = 0usize;
            let mut ok = true;
            let mut all_first = true;
            for (i, p) in ps.iter().enumerate() {
                let Piece::Block(b) = p else { continue };
                match rank_in(b, &groups[i]) {
                    Some(r) => {
                        total += r - 1;
                        if r != 1 {
                            all_first = false;
                        }
                    }
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                cov_p += 1;
                press_p += total;
                if all_first {
                    first_p += 1;
                }
                if let Some(r) = rw {
                    if total > r - 1 && worse.len() < 10 {
                        worse.push((keys.to_string(), r - 1, total));
                    }
                }
            }
        }
    }

    println!("=== 到達正解要按幾次選擇鍵（安全距離 {safe}）===\n");
    println!("  句數  {n}\n");
    println!("                        現況        分區");
    println!("  正解在候選裡      {:>6}      {:>6}", cov_w, cov_p);
    println!(
        "  一次就對（第1名）  {:>6}      {:>6}   （{:+}）",
        first_w,
        first_p,
        first_p as i64 - first_w as i64
    );
    println!(
        "  總按鍵次數        {:>6}      {:>6}   （{:+}）",
        press_w,
        press_p,
        press_p as i64 - press_w as i64
    );
    println!("\n  分區點切斷正解  {cut_thru} 句  ← 分區獨有的失敗模式");
    println!(
        "  真的被分區的句子  {partitioned} 句（{:.1}%）  ← 這個是 0 的話上面整張表都是 no-op",
        pct(partitioned, n)
    );

    if !cut_samples.is_empty() {
        println!(
            "
=== 分區點切斷正解的例子（期望的段 vs 分出來的塊）===
"
        );
        for (e, b) in cut_samples.iter().take(4) {
            println!(
                "    期望 {e}
    分塊 {b}
"
            );
        }
    }
    if !worse.is_empty() {
        println!("\n=== 分區之後反而要按更多次的句子 ===\n");
        for (k, a, b) in worse.iter().take(8) {
            let ks: String = k.chars().take(46).collect();
            println!("    現況 {a} 次 → 分區 {b} 次   {ks}");
        }
    }
}

/// 切法數到這個量就凍一刀——接近 `ALIVE_LIMIT`（400）但還沒撞死。
///
/// 用切法數而不是長度當觸發，是因為**只有真的要爆的句子才需要凍**：
/// 純中文長句的切法數少（正解切點是 0），永遠碰不到這條線，行為完全不變。
const TRIGGER: usize = 300;

/// 這個位置可以當凍結點嗎？——**保守版**的字面邊界。
///
/// # 兩條路都試過，這是第三版
///
/// **第一版**用 `space::tone_suffix_start` 判空白。它要求注音尾巴至少
/// 切得出**兩個**音節（為了擋 `notebook` 的 `k` 被當成 ㄜˉ），所以
/// `…sushiu␣`（一）這種單音節過不了門檻，聲調空白被誤判成邊界，
/// `u␣vu84`（一下）被攔腰切開——舊架構「今天→金天」的同型錯誤（§4.37）。
/// 60 句長測資有 18 句踩到。
///
/// **第二版**改用引擎當下第一名切法的段邊界（`normalize` 後相鄰段語言
/// 必定不同，所以每個邊界都是語言切換點）。長測資改善很大，但純日文
/// 長句被弄壞：`mottohayakushuppatsushiteokeba` 中途的第一名是
/// `英:mott | 日:ohayaku…`，那個「語言切換點」本身就是誤判，凍下去
/// 就定死了。**純日文沒有任何真正的邊界**，不該有東西可凍。
///
/// **這一版**回到字面邊界，但把空白判準改成**寧可少凍**：只要尾端
/// 存在**任何**可能待收尾的注音音節，就不當邊界。`notebook␣` 的 `k`
/// 因此也會被當成聲調而不切——少一個凍結點不會壞事，凍錯才會。
fn can_freeze_here(keys: &str, i: usize, cur: &str) -> bool {
    let Some(c) = keys.chars().nth(i) else {
        return false;
    };
    if c == ' ' {
        // 尾端有任何可能的待收尾注音 → 這個空白可能是聲調 → 不凍
        let chars: Vec<char> = cur.chars().collect();
        for start in 0..chars.len() {
            let tail: String = chars[start..].iter().collect();
            if space::is_tone(&tail) {
                return false;
            }
        }
        return !cur.is_empty();
    }
    punct::is_punct(keys, i)
}

/// 日文分詞邊界的安全距離，單位是**假名**。
///
/// `check_freeze --ja` 量出來的：22 句純日文、617 步，距離 8 個假名以外
/// **凍錯 0 步 0 句**（4 假名還會錯 1 句、3 假名錯 5 句）。
const JA_SAFE_KANA: usize = 8;

/// 純日文段的凍結點：用 mozc 的分詞邊界。
///
/// # 為什麼日文要另外一套
///
/// 純日文長句**沒有任何字面邊界**——沒空白、沒標點、沒語言切換，所以
/// `can_freeze_here` 對它一無所獲，永遠不凍，§2.51.11.1 的效能問題就
///留著。
///
/// 但日文有另一種邊界：**mozc 的分詞**（§2.23 的整句轉換）。`ご飯|を|
/// 食べます` 那些縫是**日文內部**的詞邊界，不會出現第二版那種
/// `英:mott | 日:ohayaku` 的語言誤判。
///
/// # 假名位置怎麼換回按鍵位置
///
/// **不能照長度比例分**——羅馬字跟假名不等比（CLAUDE.md 記過的坑，
/// `kya` 三個字母是一個 mora 兩個假名字元）。用 `mora_spans` 拿到每個
/// mora 的按鍵範圍，再逐格問 `to_kana` 到那裡為止有幾個假名，建出對照。
fn ja_freeze_point(keys: &str, safe_kana: usize) -> Option<usize> {
    use ime_core::romaji::{convert, kana};

    // **一定要用 `to_kana_partial`**：`keys` 是打到一半的串，尾巴常常是
    // 不完整的 mora（`…kigasur`），整串問 `to_kana` 會回 `None`，這條路
    // 就永遠走不到。第一版就是這樣，實測「凍了 0 刀」。
    let (done_kana, rest) = kana::to_kana_partial(keys);
    if done_kana.is_empty() {
        return None;
    }
    let all: Vec<char> = keys.chars().collect();
    let done_len = all.len().checked_sub(rest.chars().count())?;
    let head: String = all[..done_len].iter().collect();
    let spans = kana::mora_spans(&head)?;

    // `mora_spans` 回的是每個 mora 的 **(按鍵字元數, 假名字元數)**
    // ——是長度不是位置，累加才得到對照。這正是 CLAUDE.md 記過那條
    // 「按鍵不能照假名長度比例分」的解法（`sushi` 五個字母兩個假名）。
    let mut map: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
    let (mut key_acc, mut kana_acc) = (0usize, 0usize);
    for (kl, nl) in spans {
        key_acc += kl;
        kana_acc += nl;
        map.push((kana_acc, key_acc));
    }

    let total = done_kana.chars().count();
    let words = convert::convert(&done_kana);
    if words.len() < 2 {
        return None; // 沒分出詞，沒有邊界可用
    }

    // 詞邊界（累積假名數），取離結尾 safe_kana 以外的最後一個
    let mut acc = 0usize;
    let mut best: Option<usize> = None;
    for w in &words[..words.len() - 1] {
        acc += w.kana.chars().count();
        if total - acc < safe_kana {
            break;
        }
        if let Some(&(_, kp)) = map.iter().find(|&&(kc, _)| kc == acc) {
            best = Some(kp);
        }
    }
    best
}

/// 分區模擬：逐鍵推進，切法數逼近上限時在最近的安全邊界凍一刀。
///
/// 邊界有兩種來源，依序試：
///
/// 1. **字面邊界**（`can_freeze_here`）——標點、以及保守判定的空白
/// 2. **日文分詞邊界**（`ja_freeze_point`）——整段是日文時才有，補的正是
///    純日文長句「一個字面邊界都沒有」的洞
fn simulate(keys: &str, safe: usize) -> Vec<Segment> {
    let mut frozen: Vec<Segment> = Vec::new();
    let mut inc = Incremental::new();
    let mut pending = String::new();

    for c in keys.chars() {
        inc.push(c);
        pending.push(c);
        if inc.len() < TRIGGER {
            continue;
        }
        let pc: Vec<char> = pending.chars().collect();
        let n = pc.len();

        // 一、往回找字面邊界：離游標 safe 鍵以外，取最靠近游標的那個
        let mut cut = 0usize;
        for i in (0..n.saturating_sub(safe)).rev() {
            let before: String = pc[..i].iter().collect();
            if can_freeze_here(&pending, i, &before) {
                cut = i + 1; // 邊界字元本身歸前區
                break;
            }
        }
        // 二、沒有字面邊界時，試日文分詞
        if cut == 0 {
            if let Some(p) = ja_freeze_point(&pending, JA_SAFE_KANA) {
                cut = p;
            }
        }
        if cut == 0 {
            continue; // 兩種都沒有——不凍
        }

        let head: String = pc[..cut].iter().collect();
        if let Some(best) = rank::sort(Incremental::from_keys(&head).cuttings())
            .into_iter()
            .next()
        {
            frozen.extend(normalize(&best));
        }
        pending = pc[cut..].iter().collect();
        inc = Incremental::from_keys(&pending);
    }

    if !pending.is_empty() {
        if let Some(best) = rank::sort(inc.cuttings()).into_iter().next() {
            frozen.extend(normalize(&best));
        }
    }
    frozen
}

/// `--bench` 模式：分區對純日文長句的每鍵耗時有沒有用？
///
/// 測資裡的純日文句最長才 35 鍵，切法數到不了 `TRIGGER`，所以上面那兩張
/// 正確率表根本沒觸發到日文分詞那條路。要驗證它得用真正的長句
/// （取自 `bench_typing`）。
///
/// 量的是**最慢一鍵**——使用者感覺得到的是卡頓那一下，不是平均。
fn bench_ja(safe: usize) {
    use std::time::Instant;

    const CASES: [(&str, &str); 3] = [
        (
            "日文 47 鍵",
            "maikaikareanishigotowooshitsukerareteirukigasuru",
        ),
        (
            "日文 71 鍵",
            "maikaikareanishigotowooshitsukerareteirukigasurushikatanaiiitsumokoudesu",
        ),
        (
            "日文 110 鍵",
            "maikaikareanishigotowooshitsukerareteirukigasurushikatanaiiitsumokoudesukaishanishucchousaseraretemattakuyaruki",
        ),
    ];

    /// 現況：整串一路累加。
    fn worst_whole(keys: &str) -> u128 {
        let mut inc = Incremental::new();
        let mut worst = 0u128;
        for c in keys.chars() {
            let t = Instant::now();
            inc.push(c);
            let _ = rank::sort(inc.cuttings());
            worst = worst.max(t.elapsed().as_micros());
        }
        worst
    }

    /// 分區：切法數逼近上限時在安全邊界凍一刀。
    fn worst_partitioned(keys: &str, safe: usize) -> (u128, usize) {
        let mut inc = Incremental::new();
        let mut pending = String::new();
        let mut worst = 0u128;
        let mut freezes = 0usize;
        for c in keys.chars() {
            let t = Instant::now();
            inc.push(c);
            pending.push(c);
            let _ = rank::sort(inc.cuttings());
            if inc.len() >= TRIGGER {
                let pc: Vec<char> = pending.chars().collect();
                let n = pc.len();
                let mut cut = 0usize;
                for i in (0..n.saturating_sub(safe)).rev() {
                    let before: String = pc[..i].iter().collect();
                    if can_freeze_here(&pending, i, &before) {
                        cut = i + 1;
                        break;
                    }
                }
                if cut == 0 {
                    if let Some(p) = ja_freeze_point(&pending, JA_SAFE_KANA) {
                        cut = p;
                    }
                }
                if cut > 0 {
                    let head: String = pc[..cut].iter().collect();
                    let _ = rank::sort(Incremental::from_keys(&head).cuttings());
                    pending = pc[cut..].iter().collect();
                    inc = Incremental::from_keys(&pending);
                    freezes += 1;
                }
            }
            worst = worst.max(t.elapsed().as_micros());
        }
        (worst, freezes)
    }

    println!("=== 純日文長句的每鍵耗時（最慢一鍵，安全距離 {safe}）===\n");
    println!("  案例              現況      分區    凍了幾刀");
    for (name, keys) in CASES {
        let a = worst_whole(keys);
        let (b, f) = worst_partitioned(keys, safe);
        println!(
            "  {name:<16} {:>6.1}ms   {:>6.1}ms   {f:>6}",
            a as f64 / 1000.0,
            b as f64 / 1000.0
        );
    }
    println!("\n  一幀預算 16ms。");
}
