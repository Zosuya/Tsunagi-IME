//! **spike 工具（2026-09-10）**：中文詞層貪心搶字，該怎麼修？
//!
//! 完整經過見開發文件 §2.74。**結論已經實作進 `compose::recut_tail`**，
//! 這支留著是為了**重跑量測**——改動詞層或排序之後，用它確認
//! 「正確句數」與「遠處改寫」這兩個數字有沒有一起退步。
//!
//! # 為什麼需要它（漏斗量不到的東西）
//!
//! 漏斗只印改寫的**總數**，不印是哪一句、往哪個方向改。而這件事的
//! 取捨正在那裡：多對 2 句、卻讓使用者看到前文閃爍，是不划算的。
//! 這支**逐鍵餵入**（不是一次餵完整句），所以量得到打字過程的性質。
//!
//! # 三種模式
//!
//! ```text
//! spike_word_lattice                 全測資：對的句數 vs 遠處改寫
//! spike_word_lattice <按鍵>          單句：貪心與維特比的結果並排
//! DETAIL=1  spike_word_lattice       每一次遠處改寫是變對還是變錯
//! SETTLED=1 spike_word_lattice       「音節收尾」這個判準準不準
//! ```
//!
//! 參數用環境變數掃：`SINGLE`（單字邊代價）、`WRANK`（名次權重）、
//! `WIN`（凍結窗口）、`GATE=1`（開詞完整性閘門）。
//!
//! # 量到的結論（不要再重走一次）
//!
//! | 做法 | 對的句數 | 遠處改寫 |
//! |---|---|---|
//! | 貪心（實作前的基準） | 907 | 238 |
//! | 全面詞格維特比 | 909 | 259 |
//! | ＋收尾偵測＋詞完整性閘門 | 909 | 247 |
//!
//! **`WRANK` 是窄峰**：3.0 是唯一的 +2，1.0 → −1、5.0 → −3。
//! `SINGLE` 掃 2～12 完全不敏感。
//!
//! 真正讓改寫回到基準的是**防抖**（只改最後一個詞），那一步在
//! 產品碼裡，這支量不到——它跑的是自己那份離線實作。

use ime_core::compose::{self, Slot};
use ime_core::cutpoint::incremental::Incremental;
use ime_core::cutpoint::{normalize, rank};
use ime_core::language::Language;

#[path = "common/testdata.rs"]
mod testdata;

/// 單字邊的代價。§2.60.2 實測到的：不付代價的話拆開反而總分高
/// （`下午`→`下五`、`伺服器`→`四氟氣`）。那一輪掃到 6.0～8.0 才有分數
/// 收益，這裡沿用 6.0 當起點，再由 `--single` 覆寫。
fn single_cost() -> f32 {
    std::env::var("SINGLE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(6.0)
}
fn w_rank() -> f32 {
    std::env::var("WRANK")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3.0)
}

/// 查不到的罰分，跟 `compose::LM_MISS` 同值。
const MISS: f32 = -1.0;

/// 字頻表——判斷「落單的那個字自不自然」用。
fn char_freq(c: char) -> u32 {
    use std::sync::OnceLock;
    static F: OnceLock<std::collections::HashMap<char, u32>> = OnceLock::new();
    F.get_or_init(|| {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data");
        ime_core::dict::char_freq_map(&data)
    })
    .get(&c)
    .copied()
    .unwrap_or(0)
}

fn is_han(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
}

/// 詞格維特比：在一段連續注音格上找總分最高的「詞序列」。
///
/// 跟現行貪心的差別只有一個：**每條邊的接續分數同時看左右**，因為
/// 整段一起解，右邊那個詞在回溯時已經定案。貪心是由左往右、命中就跳，
/// 右邊還沒算到。
fn lattice_pick(slots: &[Slot], lo: usize, hi: usize) -> Option<Vec<String>> {
    let lm = ime_core::lm::get()?;
    let n = hi - lo;
    // best[i] = 走到第 i 格為止的最佳總分，以及那條路最後一個詞
    let mut best: Vec<Option<(f32, usize, String)>> = vec![None; n + 1];
    best[0] = Some((0.0, 0, String::new()));

    for end in 1..=n {
        for start in 0..end {
            let Some((prev_score, _, ref prev_word)) = best[start].clone() else {
                continue;
            };
            let keys: String = slots[lo + start..lo + end]
                .iter()
                .map(|s| s.keys.as_str())
                .collect();
            let len = end - start;
            // 候選詞：長度 1 的話用單字候選，否則查詞庫
            let cands: Vec<String> = if len == 1 {
                compose::candidates_for(&slots[lo + start])
                    .into_iter()
                    .filter(|w| w.chars().count() == 1 && w.chars().all(is_han))
                    .take(5)
                    .collect()
            } else {
                ime_core::dict::words_for(&keys)
                    .into_iter()
                    .filter(|w| w.chars().count() == len)
                    .map(|w| w.to_string())
                    .collect()
            };
            if cands.is_empty() {
                continue;
            }
            // 手動選過的字不能被覆蓋
            let fits = |w: &str| {
                let cs: Vec<char> = w.chars().collect();
                (start..end).all(|j| {
                    !slots[lo + j].picked
                        || slots[lo + j]
                            .text
                            .chars()
                            .eq(std::iter::once(cs[j - start]))
                })
            };
            for (rank_i, w) in cands.iter().enumerate() {
                if !fits(w) {
                    continue;
                }
                let cs: Vec<char> = w.chars().collect();
                let mut s = prev_score - w_rank() * rank_i as f32;
                // 單字邊付代價（§2.60.2）
                if len == 1 {
                    s -= single_cost();
                }
                // 詞內部的接續
                for pair in cs.windows(2) {
                    if is_han(pair[0]) && is_han(pair[1]) {
                        s += lm.score(pair[0], pair[1]).unwrap_or(MISS);
                    }
                }
                // 跟左鄰那個詞的接續
                if let (Some(l), Some(&f)) = (prev_word.chars().last(), cs.first()) {
                    if is_han(l) && is_han(f) {
                        s += lm.score(l, f).unwrap_or(MISS);
                    }
                }
                let better = match &best[end] {
                    Some((bs, _, _)) => s > *bs,
                    None => true,
                };
                if better {
                    best[end] = Some((s, start, w.clone()));
                }
            }
        }
    }
    best[n].as_ref()?;
    // 回溯
    let mut out = Vec::new();
    let mut at = n;
    while at > 0 {
        let (_, prev, ref w) = best[at].clone()?;
        out.push(w.clone());
        at = prev;
    }
    out.reverse();
    Some(out)
}

#[allow(dead_code)]
/// 把一句的 slots 用詞格維特比重算，回傳文字。
fn lattice_text(slots: &[Slot]) -> String {
    let mut out = String::new();
    let n = slots.len();
    let mut i = 0;
    while i < n {
        if !slots[i].selectable || slots[i].lang != Language::Bopomofo {
            out.push_str(&slots[i].text);
            i += 1;
            continue;
        }
        let mut end = i;
        while end < n && slots[end].selectable && slots[end].lang == Language::Bopomofo {
            end += 1;
        }
        match lattice_pick(slots, i, end) {
            Some(words) => {
                for w in words {
                    out.push_str(&w);
                }
            }
            // 算不出來就照抄現行結果——這一段本來就不該因為 spike 而變差
            None => {
                for s in &slots[i..end] {
                    out.push_str(&s.text);
                }
            }
        }
        i = end;
    }
    out
}

/// 凍結邊界：距離**尾端**這麼多格以內才准重算。
///
/// §2.60 擋下來的是改寫——lattice 整段求全域最佳解，會翻案前面早就
/// 定案的字。凍結就是「只重算尾巴」，前面照抄上一次的結果。
fn freeze_window() -> usize {
    std::env::var("WIN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(6)
}

/// **這一段被切成幾個「真詞」、幾個落單字？**
///
/// 使用者的判準：重算之後才准套用，而且要**變得更完整**才算改善。
/// 「想見立」是「想見」＋落單的「立」，「想建立」是「想」＋「建立」
/// ——後者的落單字比較少、詞比較長，那是明確的改善。
///
/// 回傳（詞數, 落單的單字數, 最長的詞有幾個字）。
fn word_shape(words: &[String]) -> (usize, usize, usize) {
    let mut singles = 0usize;
    let mut longest = 0usize;
    for w in words {
        let n = w.chars().count();
        if n == 1 {
            singles += 1;
        }
        longest = longest.max(n);
    }
    (words.len(), singles, longest)
}

/// 現行貪心在這一段切出來的詞形——拿來當比較基準。
///
/// 直接重跑 `apply_word_context` 拿不到「切成哪些詞」（它只回報
/// 每一格屬於幾字詞），所以這裡用同一套規則重走一遍：由左往右、
/// 最長先、命中就跳。
fn greedy_words(slots: &[Slot], lo: usize, hi: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = lo;
    while i < hi {
        let mut matched = false;
        for stop in ((i + 2)..=hi).rev() {
            let keys: String = slots[i..stop].iter().map(|s| s.keys.as_str()).collect();
            let all = ime_core::dict::words_for(&keys);
            if let Some(w) = all.into_iter().find(|w| w.chars().count() == stop - i) {
                out.push(w.to_string());
                i = stop;
                matched = true;
                break;
            }
        }
        if !matched {
            out.push(slots[i].text.clone());
            i += 1;
        }
    }
    out
}

/// 帶凍結的詞層維特比：`frozen` 是上一次的結果，前面照抄。
fn lattice_text_frozen(slots: &[Slot], frozen: &[String]) -> Vec<String> {
    let n = slots.len();
    let win = freeze_window();
    // 凍結線：這一格以前的照抄上次
    let line = n.saturating_sub(win);
    let mut out: Vec<String> = Vec::with_capacity(n);
    let mut i = 0;
    while i < n {
        if !slots[i].selectable || slots[i].lang != Language::Bopomofo {
            out.push(slots[i].text.clone());
            i += 1;
            continue;
        }
        let mut end = i;
        while end < n && slots[end].selectable && slots[end].lang == Language::Bopomofo {
            end += 1;
        }
        // 這一段整段都在凍結線之前 → 照抄上次的結果
        if end <= line && frozen.len() >= end {
            out.extend_from_slice(&frozen[i..end]);
            i = end;
            continue;
        }
        match lattice_pick(slots, i, end) {
            Some(words) => {
                // **詞完整性閘門**：只有在「切得更完整」時才套用。
                //
                // 兩邊都是詞的時候（`適合` vs `是合`）純粹是分數在打架，
                // 那種來回跳沒有收益、只有視覺干擾。真正該修的是
                // 「殘詞併成真詞」——`想見`＋`立` → `想`＋`建立`。
                let gate = std::env::var("GATE").is_ok();
                let apply = if gate {
                    let g = greedy_words(slots, i, end);
                    // **比的是「被詞涵蓋的字數」**，不是詞數或落單數。
                    //
                    // 第一版比落單數與最長詞長度，結果把 `想見立`→`想建立`
                    // 也擋掉了：兩邊都是「一個雙字詞＋一個落單字」，形狀
                    // 完全一樣。差別在**哪一個**落單——貪心讓「立」落單，
                    // 維特比讓「想」落單，而「建立」比「想見」更像使用者
                    // 要的詞。形狀分不出來，涵蓋率也分不出來。
                    //
                    // 真正分得出來的是**落單的那個字合不合理**：一個字
                    // 單獨成詞（「想」「我」「的」）很常見，但「立」單獨
                    // 出現在句尾很罕見。用字頻當代理——落單字的總字頻
                    // 越高，那種切法越自然。
                    let freq_of = |ws: &[String]| -> u64 {
                        ws.iter()
                            .filter(|w| w.chars().count() == 1)
                            .filter_map(|w| w.chars().next())
                            .map(|c| char_freq(c) as u64)
                            .sum()
                    };
                    let (_, gs, gl) = word_shape(&g);
                    let (_, ls, ll) = word_shape(&words);
                    // 明顯更完整（落單變少或詞變長）→ 直接採用
                    // 形狀一樣時 → 比落單字的自然度
                    ls < gs || ll > gl || (ls == gs && ll == gl && freq_of(&words) > freq_of(&g))
                } else {
                    true
                };
                if apply {
                    for w in words {
                        for c in w.chars() {
                            out.push(c.to_string());
                        }
                    }
                } else {
                    for s in &slots[i..end] {
                        out.push(s.text.clone());
                    }
                }
            }
            None => {
                for s in &slots[i..end] {
                    out.push(s.text.clone());
                }
            }
        }
        i = end;
    }
    out
}

/// **剛剛收完一個字了嗎？**
///
/// 使用者的觀察：`今天wu` 一看就知道還沒打完，`今天天` 就有明確的
/// 收尾訊號。引擎其實已經知道——打一半的按鍵走英文 passthrough
/// 原樣顯示，成字了才變成可選字的中文／日文格。
///
/// 判準因此是「最後一格是不是可選字的中文／日文格」。這一刻畫面本來
/// 就要從 `wu84` 變成 `天`，順便重算前文，視覺上是連貫的；其餘時間
/// 完全不動，前文不會閃。
fn settled_now(slots: &[Slot]) -> bool {
    let Some(last) = slots.last() else {
        return false;
    };
    if !last.selectable {
        return false;
    }
    match std::env::var("LANG_MODE").as_deref() {
        // 只在中文收尾時重算
        Ok("zh") => last.lang == Language::Bopomofo,
        // 只在日文收尾時
        Ok("ja") => last.lang == Language::Romaji,
        // 預設：中日都算（英文沒有「音節打完」的概念，判不出來）
        _ => last.lang != Language::English,
    }
}

/// 沒收尾時：新算出來的結果，但前面沿用上一次的（已經修正過的）。
///
/// 不這樣做的話，維特比在收尾那一鍵修好的字，會被下一鍵的貪心結果
/// 洗回去——修正等於沒發生。
fn keep_prefix(plain: &[String], prev: &[String], slots: &[Slot]) -> Vec<String> {
    let mut out = plain.to_vec();
    // 只沿用「格數對得上」的前綴——新的一鍵可能改變分段
    let keep = prev.len().min(out.len().saturating_sub(1));
    for i in 0..keep {
        // **只沿用中文格**。日文與英文的正確性由別的機制決定
        // （日文有自己的 Viterbi），沿用會把 `ございます` 洗成 `go`
        // ——那是段邊界移動造成的錯位，不是選字問題。
        if slots
            .get(i)
            .map(|s| s.lang == Language::Bopomofo)
            .unwrap_or(false)
        {
            out[i] = prev[i].clone();
        }
    }
    out
}

/// 一次遠處改寫的樣子：第幾鍵、哪一格、從什麼變成什麼。
#[derive(Clone, Debug)]
struct Rewrite {
    at_key: usize,
    idx: usize,
    from: String,
    to: String,
}

/// 逐鍵打完一句，回傳（最終文字，遠處改寫的明細）。
fn type_out_detail(keys: &str, use_lattice: bool) -> (String, Vec<Rewrite>) {
    let mut hits: Vec<Rewrite> = Vec::new();
    let mut prev: Vec<String> = Vec::new();
    let mut last_text = String::new();
    let mut acc = String::new();
    for (ki, c) in keys.chars().enumerate() {
        acc.push(c);
        let cands = rank::sort(Incremental::from_keys(&acc).cuttings());
        let Some(top1) = cands.first() else { continue };
        let norm = normalize(top1);
        let slots = compose::compose(&norm);
        let plain: Vec<String> = slots.iter().map(|s| s.text.clone()).collect();
        let now: Vec<String> = if use_lattice && settled_now(&slots) {
            lattice_text_frozen(&slots, &prev)
        } else if use_lattice {
            keep_prefix(&plain, &prev, &slots)
        } else {
            plain
        };
        if prev.len() <= now.len() {
            for (i, (a, b)) in prev.iter().zip(now.iter()).enumerate() {
                if a != b && now.len() - i >= 3 {
                    hits.push(Rewrite {
                        at_key: ki + 1,
                        idx: i,
                        from: a.clone(),
                        to: b.clone(),
                    });
                }
            }
        }
        last_text = now.concat();
        prev = now;
    }
    (last_text, hits)
}

/// 逐鍵打完一句，回傳（最終文字，改寫伸到游標 3 格外的次數）。
fn type_out(keys: &str, use_lattice: bool) -> (String, usize) {
    let mut far = 0usize;
    let mut prev: Vec<String> = Vec::new();
    let mut last_text = String::new();
    let mut acc = String::new();
    for c in keys.chars() {
        acc.push(c);
        let cands = rank::sort(Incremental::from_keys(&acc).cuttings());
        let Some(top1) = cands.first() else { continue };
        let norm = normalize(top1);
        let slots = compose::compose(&norm);
        let plain: Vec<String> = slots.iter().map(|s| s.text.clone()).collect();
        let now: Vec<String> = if use_lattice && settled_now(&slots) {
            lattice_text_frozen(&slots, &prev)
        } else if use_lattice {
            // 沒收尾 → 不重算，但要沿用上一次已經修正過的前綴，
            // 否則維特比修好的字會被下一鍵的貪心結果洗掉
            keep_prefix(&plain, &prev, &slots)
        } else {
            plain
        };
        // 改寫距離：逐格比對，只算「格子還在、但字變了」
        if prev.len() <= now.len() {
            for (i, (a, b)) in prev.iter().zip(now.iter()).enumerate() {
                if a != b && now.len() - i >= 3 {
                    far += 1;
                }
            }
        }
        last_text = now.concat();
        prev = now;
    }
    (last_text, far)
}

fn main() {
    testdata::load_engine();
    testdata::load_packs_from_args();

    // 收尾偵測模式：逐鍵打，看「尾巴是不是打完的音節」這個判準準不準
    if std::env::var("SETTLED").is_ok() {
        let rows = testdata::load("spike_word_lattice");
        // 只看純中文的節，其餘語言的收尾判準不同
        let (mut n_key, mut n_settled, mut n_final_settled) = (0usize, 0usize, 0usize);
        let mut sample: Vec<String> = Vec::new();
        let mut n_rows = 0usize;
        for row in rows.iter() {
            // 只看含中文的句子——中文詞層的重算只對它們有意義
            {
                let cands = rank::sort(Incremental::from_keys(&row.keys).cuttings());
                let Some(t) = cands.first() else { continue };
                let sl = compose::compose(&normalize(t));
                if !sl
                    .iter()
                    .any(|x| x.selectable && x.lang == Language::Bopomofo)
                {
                    continue;
                }
            }
            n_rows += 1;
            let mut acc = String::new();
            let total = row.keys.chars().count();
            for (ki, c) in row.keys.chars().enumerate() {
                acc.push(c);
                let cands = rank::sort(Incremental::from_keys(&acc).cuttings());
                let Some(top1) = cands.first() else { continue };
                let norm = normalize(top1);
                let slots = compose::compose(&norm);
                // 「收尾了」＝最後一格是可選字的中文／日文格，
                // 而不是英文 passthrough 的殘渣
                let settled = slots
                    .last()
                    .map(|s| s.selectable && s.lang != Language::English)
                    .unwrap_or(false);
                n_key += 1;
                if settled {
                    n_settled += 1;
                }
                if ki + 1 == total {
                    if settled {
                        n_final_settled += 1;
                    } else if sample.len() < 10 {
                        sample.push(format!(
                            "  打完了卻判為未收尾：{} → 「{}」",
                            row.keys,
                            compose::text_of(&slots)
                        ));
                    }
                }
            }
        }
        println!(
            "
=== 收尾偵測的準確度（前 200 句）===
"
        );
        println!("  總按鍵次數：{n_key}");
        println!(
            "  判為「收尾」的次數：{n_settled}（{:.1}%）",
            100.0 * n_settled as f64 / n_key as f64
        );
        println!("  含中文的句子：{n_rows}");
        println!(
            "  真正打完時判為收尾：{n_final_settled} / {n_rows}（{:.1}%）",
            100.0 * n_final_settled as f64 / n_rows as f64
        );
        println!();
        for x in &sample {
            println!("{x}");
        }
        return;
    }

    // 單句模式：直接看某一串按鍵的結果
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .collect();
    if let Some(k) = args.first() {
        let (base, bf) = type_out(k, false);
        let (lat, lf) = type_out(k, true);
        println!("按鍵：{k}");
        println!("  貪心　：「{base}」（遠處改寫 {bf}）");
        println!("  維特比：「{lat}」（遠處改寫 {lf}）");
        return;
    }

    let rows = testdata::load("spike_word_lattice");

    // 改寫明細模式：把每一次遠處改寫的方向查出來，判斷是「跳成對的」
    // 還是「跳成錯的」——判準是那一格最後的期望字
    if std::env::var("DETAIL").is_ok() {
        let (mut to_right, mut to_wrong, mut wrong_to_wrong) = (0usize, 0usize, 0usize);
        let mut samples: Vec<String> = Vec::new();
        for row in &rows {
            let want = testdata::strip_spaces(&testdata::expected_text(
                row.alts().first().unwrap_or(&row.want.as_str()),
            ));
            let wchars: Vec<char> = want.chars().collect();
            let (fin, hits) = type_out_detail(&row.keys, true);
            if hits.is_empty() {
                continue;
            }
            // 只在「最終結果跟期望等長」時判定得了每一格
            let fchars: Vec<char> = testdata::strip_spaces(&fin).chars().collect();
            if fchars.len() != wchars.len() {
                continue;
            }
            for h in &hits {
                if h.idx >= wchars.len() {
                    continue;
                }
                let target = wchars[h.idx].to_string();
                let was_ok = h.from == target;
                let now_ok = h.to == target;
                match (was_ok, now_ok) {
                    (false, true) => {
                        to_right += 1;
                        if samples.len() < 30 {
                            samples.push(format!(
                                "  ✓變對　第{}鍵 格{}「{}」→「{}」（期望「{}」）  {}",
                                h.at_key, h.idx, h.from, h.to, target, want
                            ));
                        }
                    }
                    (true, false) => {
                        to_wrong += 1;
                        if samples.len() < 30 {
                            samples.push(format!(
                                "  ✗變錯　第{}鍵 格{}「{}」→「{}」（期望「{}」）  {}",
                                h.at_key, h.idx, h.from, h.to, target, want
                            ));
                        }
                    }
                    _ => wrong_to_wrong += 1,
                }
            }
        }
        println!(
            "
=== 遠處改寫的方向（維特比＋凍結，WIN={}）===
",
            freeze_window()
        );
        println!("  錯 → 對　：{to_right}");
        println!("  對 → 錯　：{to_wrong}");
        println!("  錯 → 還是錯：{wrong_to_wrong}");
        println!();
        for s in &samples {
            println!("{s}");
        }
        return;
    }

    let mut n_zh = 0usize;
    let (mut base_ok, mut lat_ok) = (0usize, 0usize);
    let (mut base_far, mut lat_far) = (0usize, 0usize);
    let mut fixed: Vec<(String, String, String)> = Vec::new();
    let mut broke: Vec<(String, String, String)> = Vec::new();

    for row in &rows {
        let cands = rank::sort(Incremental::from_keys(&row.keys).cuttings());
        let Some(top1) = cands.first() else { continue };
        let norm = normalize(top1);
        let slots = compose::compose(&norm);
        if !slots
            .iter()
            .any(|s| s.selectable && s.lang == Language::Bopomofo)
        {
            continue;
        }
        n_zh += 1;
        let (base, bf) = type_out(&row.keys, false);
        let (lat, lf) = type_out(&row.keys, true);
        base_far += bf;
        lat_far += lf;
        let b_ok = testdata::match_text(row, &base).is_some();
        let l_ok = testdata::match_text(row, &lat).is_some();
        if b_ok {
            base_ok += 1;
        }
        if l_ok {
            lat_ok += 1;
        }
        let want = testdata::expected_text(row.alts().first().unwrap_or(&row.want.as_str()));
        if !b_ok && l_ok {
            fixed.push((want, base, lat));
        } else if b_ok && !l_ok {
            broke.push((want, base, lat));
        }
    }

    println!(
        "
=== 詞層維特比＋凍結（SINGLE={} WRANK={} WIN={}）===
",
        single_cost(),
        w_rank(),
        freeze_window()
    );
    println!("  含中文的句子：{n_zh}");
    println!("  現行（貪心）　：{base_ok} 對，遠處改寫 {base_far}");
    println!("  維特比＋凍結　：{lat_ok} 對，遠處改寫 {lat_far}");
    println!(
        "  淨變化：{:+}（修好 {}，弄壞 {}）
",
        lat_ok as i64 - base_ok as i64,
        fixed.len(),
        broke.len()
    );

    println!("── 修好的（前 25）──");
    for (w, b, l) in fixed.iter().take(25) {
        println!("  期望「{w}」　貪心「{b}」→ 維特比「{l}」");
    }
    println!(
        "
── 弄壞的（前 25）──"
    );
    for (w, b, l) in broke.iter().take(25) {
        println!("  期望「{w}」　貪心「{b}」→ 維特比「{l}」");
    }
}
