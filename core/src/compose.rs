//! 選詞模組：把切法變成文字。
//!
//! 切點引擎的職責到「哪一段是什麼語言」為止（`cutpoint`），這一層
//! 負責「那一段該顯示什麼字」。
//!
//! # 以字為單位，用詞去修正
//!
//! 使用者定的規則：
//!
//! > 以字為單位。如果使用者選第一個字之後，詞庫有符合的詞，可以改變
//! > 第二個字——例如打「擬郝」，使用者選了「你」，「郝」就自己變成「好」。
//!
//! 所以每個注音音節各自是一個選字位置，但**選過之後會回頭查詞**：
//!
//! ```text
//! su3cl3  →  [擬][郝]        ← 各自的字頻第一名
//!            使用者選「你」
//!         →  [你][好]        ← 詞庫有「你好」，第二個字跟著改
//! ```
//!
//! 這跟新注音的行為一致——選字會帶動後面，不必逐字選完。
//!
//! # 英文段幾乎不參與選字
//!
//! 英文段就是它自己（`check` 顯示 `check`），沒有同音字的問題，
//! 選字時直接跳過。**唯一的例外是日文詞典也收的那些**（`ii`→いい），
//! 理由見 `compose_with_bounds` 裡英文那一支的註解。

use crate::cutpoint::Segment;
use crate::language::Language;

/// 一個可以選字的位置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    /// 這一格的按鍵（一個注音音節，或整段英文／日文）。
    pub keys: String,
    /// 這一格目前顯示的文字。
    pub text: String,
    /// 這一格是哪個語言。
    pub lang: Language,
    /// 這一格能不能選字？英文段原則上不能（日文詞典也收的除外）。
    pub selectable: bool,
    /// 這個字是**使用者手動選的**嗎？
    ///
    /// 手動選過的字不可以被詞庫或重算覆蓋掉——使用者已經表態了，
    /// 引擎再自作聰明改回去是最惱人的行為。見 `apply_word_context`。
    pub picked: bool,
}

/// 使用者手動調整過的**日文詞界**。
///
/// Viterbi 只能給「詞典查得到的」分法，遇到詞典沒收的專有名詞時
/// 再怎麼選字都拼不出來——所以要能自己把詞界拉開。
/// 見 `romaji::convert::convert_with`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JpBounds {
    /// 這是哪一段日文（用它的**按鍵**認）。
    ///
    /// 按鍵變了就代表使用者又打了字，那時這份調整已經過期——
    /// 跟 `Session::chosen_cut` 同一個道理。
    pub keys: String,
    /// 每個詞佔幾個假名
    pub lens: Vec<usize>,
}

/// 把一種切法變成可選字的格子。
///
/// 注音段會**再切成音節**——選字是以字為單位的。日文與英文段維持整段。
pub fn compose(segs: &[Segment]) -> Vec<Slot> {
    compose_with(segs, crate::width::Width::default())
}

/// 同上，但指定全半形模式。
///
/// 標點段會依模式轉換——`Auto` 看**前面那一段**的語言決定：
/// 中日文旁邊用全形（中文排版習慣），英文旁邊用半形（打程式碼時
/// 不能冒出全形符號）。
pub fn compose_with(segs: &[Segment], width: crate::width::Width) -> Vec<Slot> {
    compose_with_bounds(segs, width, None)
}

/// 同上，但可以指定**使用者手動調整過的日文詞界**。
pub fn compose_with_bounds(
    segs: &[Segment],
    width: crate::width::Width,
    bounds: Option<&JpBounds>,
) -> Vec<Slot> {
    let mut out = Vec::new();
    // 前一段是什麼語言？標點自己不算——連續兩個標點時要看更前面
    let mut prev_lang: Option<Language> = None;
    for s in segs {
        if s.is_mark {
            let text: String = s
                .keys
                .chars()
                .map(|c| crate::width::convert(c, width, prev_lang))
                .collect();
            out.push(Slot {
                keys: s.keys.clone(),
                text,
                lang: s.lang,
                selectable: false,
                picked: false,
            });
            continue;
        }
        prev_lang = Some(s.lang);
        match s.lang {
            Language::Bopomofo => {
                // 注音再切成音節，每個音節一格
                match crate::bopomofo::split_syllables(&s.keys) {
                    Some(syllables) => {
                        for syl in syllables {
                            let text = best_char(&syl);
                            out.push(Slot {
                                keys: syl,
                                text,
                                lang: Language::Bopomofo,
                                selectable: true,
                                picked: false,
                            });
                        }
                    }
                    // 切不出音節（還在打）——整段當一格，顯示原始按鍵
                    None => out.push(Slot {
                        keys: s.keys.clone(),
                        text: s.keys.clone(),
                        lang: Language::Bopomofo,
                        selectable: false,
                        picked: false,
                    }),
                }
            }
            Language::Romaji => {
                let kana = crate::romaji::kana::to_kana(&s.keys).unwrap_or_else(|| s.keys.clone());
                // **一段日文再切成詞**——跟注音段切成音節同一個道理。
                //
                // 語言邊界是切點引擎的事（這一段是日文），詞的邊界是
                // 日文引擎的事（這段裡有幾個詞）。兩個維度分開，見
                // 開發文件 §2.23。
                // 使用者調整過這一段的詞界就照他的來，否則交給 Viterbi
                let words = match bounds {
                    Some(b) if b.keys == s.keys => {
                        crate::romaji::convert::convert_with(&kana, &b.lens)
                    }
                    _ => crate::romaji::convert::convert(&kana),
                };
                if words.len() <= 1 {
                    // 只有一個詞（或轉不出來）就維持原本的一格
                    out.push(Slot {
                        keys: s.keys.clone(),
                        text: best_japanese(&kana),
                        lang: Language::Romaji,
                        selectable: true,
                        picked: false,
                    });
                } else {
                    // **按鍵怎麼分配**：格子的 `keys` 要接得回原字串
                    // （`check_rewrite`、`delete_marked_slot`、學習都靠
                    // 這個性質）。分詞是照假名做的，而羅馬字跟假名
                    // **不是等比**——所以用 mora 的真實對應換算，
                    // 見 `kana::mora_spans`。
                    let spans = crate::romaji::kana::mora_spans(&s.keys).unwrap_or_default();
                    let keys: Vec<char> = s.keys.chars().collect();
                    let mut used = 0usize; // 用掉幾個按鍵
                    let mut si = 0usize; // 走到第幾個 mora
                    for (i, w) in words.iter().enumerate() {
                        let want = w.kana.chars().count();
                        let take = if i + 1 == words.len() || spans.is_empty() {
                            // 最後一格吃掉剩下的——保證接得回去
                            keys.len().saturating_sub(used)
                        } else {
                            // 吃 mora 直到假名數湊滿。詞界如果落在 mora
                            // 中間（`きゃ` 是一個 mora 兩個假名）就多吃
                            // 那一個——邊界差一點好過按鍵對不回去。
                            let mut k = 0usize;
                            let mut kana = 0usize;
                            while si < spans.len() && kana < want {
                                k += spans[si].0;
                                kana += spans[si].1;
                                si += 1;
                            }
                            k.min(keys.len().saturating_sub(used))
                        };
                        let part: String = keys[used..used + take].iter().collect();
                        used += take;
                        out.push(Slot {
                            keys: part,
                            text: w.surface.clone(),
                            lang: Language::Romaji,
                            selectable: true,
                            picked: false,
                        });
                    }
                }
            }
            // 英文就是它自己，沒有同音字的問題。
            //
            // **一個例外：日文詞典也收的英文段**（`ii`→いい、`mou`→もう）。
            // 語言判斷是一票定生死的（`lang_of`：夠常用的英文詞不讓給
            // 日文，否則 `you`／`the`／`time` 全會變假名），使用者沒有
            // 第二意見可表達——切法選單裡也不會有把它當日文的那一種。
            // 所以這一格開放選字，把日文候選補進清單。
            // **預設仍然是英文**，只有使用者表態過（學習到 `LEARNED`
            // 次）才會換掉，三支計分器量的第一名因此不動。
            Language::English => {
                let jp = crate::dict::is_japanese_word(&s.keys);
                let text = jp
                    .then(|| {
                        crate::learn::any()
                            .then(|| crate::learn::index().best(&s.keys).map(str::to_string))
                            .flatten()
                    })
                    .flatten()
                    .unwrap_or_else(|| s.keys.clone());
                out.push(Slot {
                    keys: s.keys.clone(),
                    text,
                    lang: Language::English,
                    selectable: jp,
                    picked: false,
                });
            }
        }
    }
    // 組好之後用詞庫修一次——單看每個字的字頻常常是錯的
    apply_word_context(&mut out);
    out
}

/// 這個注音音節最可能是哪個字？
///
/// 同音字依**字頻**排序，取第一名。`su3` 有 29 個同音字，
/// 字頻讓「你」排在「儗」「旎」前面。
fn best_char(syllable: &str) -> String {
    crate::dict::best_char_for(syllable)
        .map(str::to_string)
        .unwrap_or_else(|| syllable.to_string())
}

/// 用詞庫修正相鄰的注音格。
///
/// 單看每個字的字頻常常選錯——`su3cl3` 逐字選是「擬郝」，但那兩個字
/// 連在一起是「你好」。這裡從長到短掃過去，找得到詞就把整組字換掉。
///
/// **從長到短**是因為長詞的資訊量大：`5k4ek7` 是「這個」而不是
/// 「這」＋「個」各自的第一名。
fn apply_word_context(slots: &mut [Slot]) {
    let n = slots.len();
    let mut i = 0;
    while i < n {
        if !slots[i].selectable || slots[i].lang != Language::Bopomofo {
            i += 1;
            continue;
        }
        // 從最長的連續注音段開始試
        let mut end = i;
        while end < n && slots[end].selectable && slots[end].lang == Language::Bopomofo {
            end += 1;
        }
        let mut matched = false;
        for stop in (i + 2..=end).rev() {
            let keys: String = slots[i..stop].iter().map(|s| s.keys.as_str()).collect();
            // **同讀音的詞不只一個**，挑跟「使用者已經選過的字」相容的
            // 那一個。「城市」與「程式」讀音相同，選了「程」就該挑到
            // 「程式」，「市」才會跟著變「式」——挑不到相容的就用第一個
            // （預設值），行為跟只有一個詞的時候一樣。
            let chosen = crate::dict::words_for(&keys).into_iter().find(|w| {
                let cs: Vec<char> = w.chars().collect();
                // 詞的字數要跟格數對得上才能一格一格填回去
                cs.len() == stop - i
                    && (i..stop).all(|j| {
                        !slots[j].picked || slots[j].text.chars().eq(std::iter::once(cs[j - i]))
                    })
            });
            if let Some(word) = chosen {
                for (k, c) in word.chars().enumerate() {
                    // **手動選過的字不覆蓋**——使用者已經表態了
                    if !slots[i + k].picked {
                        slots[i + k].text = c.to_string();
                    }
                }
                i = stop;
                matched = true;
                break;
            }
        }
        if !matched {
            i += 1;
        }
    }
}

/// 使用者在第 `idx` 格選了 `choice` 這個字之後，重算後面的格子。
///
/// 這是使用者定的規則——「選了『你』，『郝』就自己變成『好』」。
/// 只往後修，不動前面已經選過的。
pub fn pick(slots: &mut [Slot], idx: usize, choice: &str) {
    if idx >= slots.len() {
        return;
    }
    slots[idx].text = choice.to_string();
    slots[idx].picked = true;
    // 從選中的那格開始往後找詞。`picked` 的格子不會被覆蓋，
    // 所以這裡不必再把使用者選的字寫回去一次。
    apply_word_context(&mut slots[idx..]);
}

/// 日文的候選：**三種假名 + 詞庫的漢字轉換**。
///
/// 假名那三種不必查詞庫——它們是純粹的字形轉換，任何一段假名都
/// 一定有對應。日本的輸入法都提供這三個選項：
///
/// | | 例（`sushi`） | 什麼時候用 |
/// |---|---|---|
/// | 平假名 | すし | 和語、助詞 |
/// | 全形片假名 | スシ | 外來語、強調 |
/// | 半形片假名 | ｽｼ | 舊系統相容、表格 |
///
/// 排在最前面是因為它們**一定正確**——漢字轉換是猜的，假名不是。
/// 詞庫查到的漢字接在後面，重複的（詞庫剛好收了假名寫法）去掉。
fn romaji_candidates(kana: &str) -> Vec<String> {
    use crate::romaji::kana::{to_halfwidth_katakana, to_katakana};
    if kana.is_empty() {
        return Vec::new();
    }
    let dict = crate::dict::words_for_kana(kana);
    let mut out = Vec::new();
    // 詞典最佳排第一——那是預設顯示的字，清單第一個要跟它一致
    if let Some(best) = dict.first() {
        out.push(best.clone());
    }
    // 三種假名寫法緊接在後。它們**一定正確**（漢字轉換是猜的，假名不是），
    // 所以要讓使用者永遠一兩下就回得到假名
    for k in [
        kana.to_string(),
        to_katakana(kana),
        to_halfwidth_katakana(&to_katakana(kana)),
    ] {
        if !out.contains(&k) {
            out.push(k);
        }
    }
    for w in dict.iter().skip(1) {
        if !out.contains(w) {
            out.push(w.clone());
        }
    }
    out
}

/// 這串假名預設顯示什麼？
///
/// # 漢字優先（使用者 2026-08-31 裁決，見期望基準審核.md A6）
///
/// 查詞典取總成本最低的表記。**總成本含接續成本**，這是關鍵——只看
/// 詞成本的話「すし」會給「酸し」(4451) 而不是「寿司」(4520)，因為
/// 「酸し」是文語形容詞，便宜在詞本身、貴在句首接不上去。
///
/// 好處是**平假名該贏的時候會自己贏**：`ありがとう`、`おはよう` 查出來
/// 的第一名就是平假名，不必為它們另外維護例外表。
///
/// 詞典查不到就用假名——活用形句子（mozc 只收辭書形）都走這條。
fn best_japanese(kana: &str) -> String {
    crate::dict::best_kana_word(kana)
        .map(|w| w.into_owned())
        .unwrap_or_else(|| kana.to_string())
}

/// 某一格的候選字，依字頻排序。
pub fn candidates_for(slot: &Slot) -> Vec<String> {
    if !slot.selectable {
        return Vec::new();
    }
    let mut out = candidates_raw(slot);
    // **清單的第一個永遠是這一格現在顯示的字。**
    //
    // 候選本身是依字頻排的，但那一格顯示的未必是字頻第一名——手動選過
    // （`picked`）或被詞層修正過都會不一樣：
    //
    // ```text
    // 選了「程」之後   #1 text=式   候選 是 事 市 世 士 示 式 …
    //                                              ↑ 排第 7
    // ```
    //
    // 反白進選字時停在第 0 個（`select.rs` 的 `cand_idx = 0`），清單第一個
    // 不是現在的字的話，**方向鍵一移就把剛修好的字弄丟**。
    //
    // 這條規則跟 §2.24 給英文段定的「第一個放英文原文，讓使用者永遠
    // 回得來」是同一條，只是推廣到所有語言。
    //
    // 純顯示層的調整——預設輸出走的是 `best_char` 與 `apply_word_context`，
    // 不經過這裡，所以三支計分器不會動。
    if let Some(i) = out.iter().position(|c| *c == slot.text) {
        let cur = out.remove(i);
        out.insert(0, cur);
    } else if !slot.text.is_empty() {
        // 詞層填進來的字未必在同音字清單裡（偏好表注入的詞就可能）
        out.insert(0, slot.text.clone());
    }
    out
}

/// 依字頻排好的候選，還沒把「現在顯示的字」提到前面。
fn candidates_raw(slot: &Slot) -> Vec<String> {
    match slot.lang {
        Language::Bopomofo => crate::dict::chars_for(&slot.keys),
        // **拿讀音去查，不是拿已經轉換過的文字**。
        //
        // 這裡踩過坑：原本傳的是 `slot.text`，那一格是漢字時
        // （`ご飯`）就查不到任何候選，只剩三種假名寫法；剛好停在
        // 假名時才查得到。整句轉換之前每格常常是假名，所以不明顯，
        // 分詞之後每格都是漢字，問題就整個浮出來。
        // **鏡像 §2.24**：日文段若英文詞典也收得到這串按鍵，把英文
        // 原文補在最後（`youtube`→ようつべ、`sushi`→すし）。
        //
        // 理由跟 `ii`→いい 那次一模一樣，只是方向相反。`lang_of` 的
        // 「夠常用的英文詞不讓給日文」門檻是排名 5000，而 `youtube`
        // 排 10160，於是整段被判成日文，切法選單裡也不會有把它當英文
        // 的那一種——使用者沒有第二意見可表達。
        //
        // **拉高那個門檻的路是死的**：`sushi` 排 7210、`karaoke` 排
        // 9015，都比 `youtube` 前面。門檻只要高到能救 `youtube`，
        // `sushi` 就會變成英文原樣而不是「すし」。英文詞頻排名分不開
        // 「英文詞」與「用羅馬字打的日文外來語」，這兩群完全交錯。
        //
        // **補在最後、不動前面**：預設輸出不變（第一名仍是日文），
        // 三支計分器因此一個數字都不動。選過 `LEARNED` 次之後
        // `best_kana_word` 才會換掉——日文格的學習鍵是假名，那條路
        // 本來就通，不必另外接。
        //
        // **用 `is_common_word` 不是 `is_word`**：`en_50k` 收了大量冷僻
        // 的兩字母詞，助詞格會因此多出雜訊（`wo`→を 那一格冒出「wo」）。
        // `is_common_word` 對兩字母有頻率門檻（≥10 萬），正好濾掉這批，
        // 三字母以上一律放行，所以 `youtube`／`sushi` 不受影響。
        Language::Romaji => {
            let kana =
                crate::romaji::kana::to_kana(&slot.keys).unwrap_or_else(|| slot.text.clone());
            let mut out = romaji_candidates(&kana);
            if crate::english::is_common_word(&slot.keys) && !out.contains(&slot.keys) {
                out.push(slot.keys.clone());
            }
            out
        }
        // 只有「日文詞典也收」的英文段走得到這裡（別的英文段
        // `selectable` 是 false，上面就回去了）。第一個放英文原文，
        // 讓使用者永遠回得來。
        Language::English => {
            let mut out = vec![slot.keys.clone()];
            if let Some(kana) = crate::romaji::kana::to_kana(&slot.keys) {
                for c in romaji_candidates(&kana) {
                    if !out.contains(&c) {
                        out.push(c);
                    }
                }
            }
            out
        }
    }
}

/// 把格子接成一串顯示文字。
pub fn text_of(slots: &[Slot]) -> String {
    slots.iter().map(|s| s.text.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cutpoint::incremental::Incremental;
    use crate::cutpoint::{normalize, rank};

    fn load() -> bool {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data");
        crate::preload(&data, crate::config::Engines::default());
        crate::dict::bopomofo_loaded()
    }

    /// 取第一名切法的格子
    fn slots_of(keys: &str) -> Vec<Slot> {
        let cands = rank::sort(Incremental::from_keys(keys).cuttings());
        compose(&normalize(&cands[0]))
    }

    #[test]
    fn 注音切成單字() {
        if !load() {
            eprintln!("詞庫未下載，跳過（跑 data/download.ps1）");
            return;
        }
        let slots = slots_of("su3cl3");
        assert_eq!(slots.len(), 2, "你好是兩個字：{slots:?}");
        assert!(slots.iter().all(|s| s.selectable));
    }

    #[test]
    fn 詞庫修正逐字選錯的結果() {
        if !load() {
            return;
        }
        // su3 的字頻第一名不是「你」，但「你好」是詞
        let slots = slots_of("su3cl3");
        assert_eq!(text_of(&slots), "你好");
    }

    #[test]
    fn 英文段不參與選字() {
        if !load() {
            return;
        }
        let slots = slots_of("check");
        assert_eq!(slots.len(), 1);
        assert!(!slots[0].selectable);
        assert_eq!(slots[0].text, "check");
    }

    /// **候選清單的第一個永遠是這一格現在顯示的字**。
    ///
    /// 反白進選字時停在第 0 個，清單第一個不是現在的字的話，方向鍵
    /// 一移就把手動選過／被詞層修正過的字弄丟。
    #[test]
    fn 候選第一個是目前顯示的字() {
        if !load() {
            return;
        }
        // 詞層修正過的：選了「程」之後第 2 格是「式」（字頻排第 7）
        let mut slots = slots_of("t/6g4");
        pick(&mut slots, 0, "程");
        assert_eq!(text_of(&slots), "程式");
        assert_eq!(
            candidates_for(&slots[0]).first().map(String::as_str),
            Some("程"),
            "手動選過的那格"
        );
        assert_eq!(
            candidates_for(&slots[1]).first().map(String::as_str),
            Some("式"),
            "被詞層修正過的那格"
        );
        // 其餘仍照字頻——「式」被抽走之後第二個是原本的第一名
        assert_eq!(
            candidates_for(&slots[1]).get(1).map(String::as_str),
            Some("是")
        );
    }

    /// **同讀音的詞要全部查得到，而且預設不變**。
    #[test]
    fn 同讀音的詞不只一個() {
        if !load() {
            return;
        }
        let ws = crate::dict::words_for("t/6g4");
        assert_eq!(
            ws.first().map(|w| w.as_ref()),
            Some("城市"),
            "第一個仍是最常用的——預設輸出不能變：{ws:?}"
        );
        assert!(
            ws.iter().any(|w| w.as_ref() == "程式"),
            "「程式」讀音相同，也要在清單裡：{ws:?}"
        );
        // `word_for` 只回第一個，那是「直接送出」要的預設值
        assert_eq!(crate::dict::word_for("t/6g4").as_deref(), Some("城市"));
    }

    /// **輕聲詞用本調也要打得出來**。
    ///
    /// 語料標的是「這個字怎麼念」（輕聲），使用者打的是字典音。
    /// 沒有別名的話 `5k4ek4` 查不到「這個」，詞層一落空就整個詞逐字
    /// 重排——「各」在ㄍㄜˋ底下贏「個」五倍。
    #[test]
    fn 輕聲詞的本調別名() {
        if !load() {
            return;
        }
        // 使用者實際的打法：ㄓㄜˋㄍㄜˋ
        assert_eq!(text_of(&slots_of("5k4ek4")), "這個");
        assert_eq!(text_of(&slots_of("u6ek4")), "一個");
        assert_eq!(text_of(&slots_of("s84ek4")), "那個");
        // 輕聲那條路不能壞
        assert_eq!(text_of(&slots_of("5k4ek7")), "這個");
        // **別名只填空位**：「各位」的鍵上本來就有真詞，不該被蓋掉
        assert_eq!(text_of(&slots_of("ek4jo4")), "各位");
    }

    /// **壞掉的常常不是那個輕聲字，是被拖垮的鄰居**。
    ///
    /// 「子」在ㄗˇ底下本來就排第一，所以逐字重排時它自己是對的
    /// ——垮的是前面那格：ㄏㄞˊ 的第一名是「還」不是「孩」。
    /// 判準因此不能問「這個字排不排得到第一」，要問「整個詞對不對」。
    #[test]
    fn 輕聲詞的本調別名_壞的是鄰居() {
        if !load() {
            return;
        }
        // c96y3 = ㄏㄞˊㄗˇ，沒有別名的話是「還子」
        assert_eq!(text_of(&slots_of("c96y3")), "孩子");
        // gk6ai6 = ㄕㄜˊㄇㄛˊ（什麼的四種讀音之一，本調）
        assert_eq!(text_of(&slots_of("gk6ai6")), "什麼");
        // u 2j4y3 = ㄧ ㄉㄨˋㄗˇ，沒有別名的話是「一度子」
        assert_eq!(text_of(&slots_of("u 2j4y3")), "一肚子");
        // **別名不排擠真詞**：ㄕㄣˊㄇㄛˊ 上面本來就有「神魔」，
        // 「什麼」的別名不該蓋掉它
        assert_eq!(text_of(&slots_of("gp6ai6")), "神魔");
    }

    /// **一次只換一個音節**。
    ///
    /// 第一版寫成整串 `replace`，詞裡有兩個「個」而輕重不同時就生不出
    /// 逐位置的變體，實測漏掉 9 條。
    #[test]
    fn 輕聲詞的本調別名_同一個字輕重並存() {
        if !load() {
            return;
        }
        // 一個個 = ㄧ ㄍㄜ˙ ㄍㄜˋ，第二個「個」本來就是本調
        assert_eq!(text_of(&slots_of("u ek7ek4")), "一個個");
        assert_eq!(text_of(&slots_of("u6ek7ek4")), "一個個");
    }

    /// **英文詞典也收的日文段要補英文原文**（§2.24 的鏡像）。
    ///
    /// `youtube` 排名 10160，超過 `lang_of` 的「夠常用就不讓給日文」
    /// 門檻（5000），於是整段被判成日文（ようつべ 在 mozc 裡）。
    /// 拉高門檻救不了——`sushi` 排 7210、`karaoke` 排 9015，都比它前面。
    /// 這一格的候選是使用者打出英文原文的唯一出口。
    #[test]
    fn 日文段_英文詞典也收的補上英文原文() {
        if !load() || !crate::dict::japanese_loaded() {
            return;
        }
        let slots = slots_of("youtubeao6g4");
        let yt = slots
            .iter()
            .find(|s| s.keys == "youtube")
            .unwrap_or_else(|| panic!("該有一格 youtube：{slots:?}"));
        assert_eq!(yt.lang, Language::Romaji);
        let cands = candidates_for(yt);
        // **預設不變**——第一名仍是日文，英文只是補在後面
        assert_ne!(
            cands.first().map(String::as_str),
            Some("youtube"),
            "英文不該搶第一名：{cands:?}"
        );
        assert!(
            cands.iter().any(|c| c == "youtube"),
            "候選要有英文原文：{cands:?}"
        );
    }

    /// 冷僻的兩字母詞不算——助詞格不該冒出雜訊。
    ///
    /// `wo`（を）在 en_50k 裡，但頻率是雜訊等級。用 `is_word` 的話
    /// 每個助詞格都會多一個英文候選。
    #[test]
    fn 日文段_冷僻兩字母詞不補() {
        if !load() || !crate::dict::japanese_loaded() {
            return;
        }
        let slots = slots_of("sushiwotabemasu");
        let wo = slots
            .iter()
            .find(|s| s.keys == "wo")
            .unwrap_or_else(|| panic!("該有一格 wo：{slots:?}"));
        let cands = candidates_for(wo);
        assert!(
            !cands.iter().any(|c| c == "wo"),
            "wo 是雜訊，不該補進候選：{cands:?}"
        );
    }

    /// **日文詞典也收的英文段要能選字**。
    ///
    /// `ii` 是排名 3557 的英文詞，`lang_of` 的「夠常用就不讓給日文」
    /// 把它判成英文；語言是一票定生死的，切法選單裡也沒有把它當日文
    /// 的那一種。這一格的候選是使用者打出「いい」的唯一出口。
    #[test]
    fn 英文段_日文詞典也收的可以選字() {
        if !load() || !crate::dict::japanese_loaded() {
            return;
        }
        let slots = slots_of("rup wu0 g4ek7iiwu0 fu4");
        let ii = slots
            .iter()
            .find(|s| s.keys == "ii")
            .expect("該有一格 ii：{slots:?}");
        assert_eq!(ii.lang, Language::English);
        assert!(ii.selectable, "該可以選字");
        let cands = candidates_for(ii);
        assert_eq!(
            cands.first().map(String::as_str),
            Some("ii"),
            "第一個要是英文原文"
        );
        assert!(cands.iter().any(|c| c == "いい"), "候選要有いい：{cands:?}");
    }

    #[test]
    fn 日文段預設寫成漢字() {
        if !load() {
            return;
        }
        // 使用者裁決「預設漢字優先」（期望基準審核.md A6）。
        // 「寿司」贏過「酸し」靠的是接續成本——只看詞成本的話
        // 「酸し」(4451) 比「寿司」(4520) 便宜
        assert_eq!(text_of(&slots_of("sushi")), "寿司");
        // 外來語用片假名
        assert_eq!(text_of(&slots_of("anime")), "アニメ");
    }

    #[test]
    fn 沒把握的就維持假名() {
        if !load() {
            return;
        }
        // 「どうしよう」是「どう＋しよう」兩個詞，詞典裡沒有這個條目，
        // 但「同仕様」剛好有。那種假命中的總成本偏高，被門檻擋掉
        assert_eq!(text_of(&slots_of("doushiyou")), "どうしよう");
        // 平假名本來就該贏的也不會被硬轉
        assert_eq!(text_of(&slots_of("arigatou")), "ありがとう");
    }

    #[test]
    fn 選字會帶動後面() {
        if !load() {
            return;
        }
        let mut slots = slots_of("su3cl3");
        // 假設使用者把第一格改成別的字，後面應該跟著重算
        pick(&mut slots, 0, "妳");
        assert_eq!(slots[0].text, "妳", "使用者選的不能被改掉");
    }

    #[test]
    fn 空輸入() {
        assert!(compose(&[]).is_empty());
        assert_eq!(text_of(&[]), "");
    }
    #[test]
    fn 日文候選是漢字加三種假名() {
        if !load() {
            return;
        }
        let c = romaji_candidates("すし");
        // 第一個是預設顯示的字，要跟 compose 的輸出一致
        assert_eq!(c[0], "寿司", "詞典最佳排第一：{c:?}");
        // 三種假名緊接在後——它們一定正確，使用者要永遠一兩下回得到
        assert_eq!(&c[1..4], &["すし", "スシ", "ｽｼ"], "三種假名要接在後面");
    }

    #[test]
    fn 濁音的半形片假名() {
        if !load() {
            return;
        }
        let c = romaji_candidates("ありがとう");
        assert_eq!(c[1], "アリガトウ");
        assert_eq!(c[2], "ｱﾘｶﾞﾄｳ", "濁音要拆成清音＋濁點");
    }

    #[test]
    fn 假名候選不重複() {
        if !load() {
            return;
        }
        // 詞庫剛好收了假名寫法時不該出現兩次
        let c = romaji_candidates("こんにちは");
        let n = c.iter().filter(|x| *x == "こんにちは").count();
        assert_eq!(n, 1, "重複的要去掉：{c:?}");
    }
}
