//! 切法模組的三條特殊規則。
//!
//! 依據 `語言辨識演算法(新).canvas` 的「切法模組」那格：
//!
//! > 僅有一種正解，可實現組合皆須於候選中
//! >
//! > 特殊規則
//! > - 單一字母英文強制併入前一段
//! > - 結構為【英+日+英】時合併為【英】
//! > - 切法頭與尾段與相鄰語言不同時，查詢英文辭典作為補候選
//!
//! # 這三條在救什麼
//!
//! 英文單字幾乎都含有合法的日文 mora——`check` 的 `che`（ちぇ）、
//! `meeting` 的 `meeti`（めえてぃ）、`review` 的 `revie`（れゔぃえ）。
//! 日文引擎會貪婪吃掉它們，剩下的字母落到英文，於是英文單字被拆爛：
//!
//! ```text
//! check    →  日:che | 英:ck
//! meeting  →  日:meeti | 英:ng
//! ```
//!
//! 前兩條規則就是把這些碎片黏回去。

use super::{Segment, SEPARATOR};
use crate::language::Language;

/// 套用三條特殊規則。
pub fn apply(segs: Vec<Segment>) -> Vec<Segment> {
    let segs = rule1_single_letter(segs);
    let segs = rule2_en_ja_en(segs);
    let segs = rule3_dictionary(segs);
    merge_adjacent(segs)
}

/// 規則三：切法頭與尾段與相鄰語言不同時，查詢英文辭典。
///
/// 規則二只救得了「英+日+英」的夾心，頭尾的日文段夾不住：
///
/// ```text
/// check    →  日:che | 英:ck      日文在頭
/// meeting  →  日:meeti | 英:ng    日文在頭
/// update   →  英:up | 日:date     日文在尾
/// ```
///
/// 這裡把**相鄰且語言不同的段**合起來查詞典，查到就整段當英文。
/// 只查詞典有的——沒查到就維持原樣，不亂猜。
fn rule3_dictionary(segs: Vec<Segment>) -> Vec<Segment> {
    if !crate::english::is_loaded() {
        return segs; // 詞庫沒載入時跳過這層補救
    }
    let mut out: Vec<Segment> = Vec::new();
    let mut i = 0usize;
    while i < segs.len() {
        // 從 i 開始，試著把連續的「非分隔符、非注音」段合起來查詞典。
        // 注音不參與——它跟英文的按鍵集合不相交，黏在一起沒有意義。
        let mut best: Option<usize> = None;
        let mut merged = String::new();
        for (offset, s) in segs[i..].iter().enumerate() {
            if s.is_mark || s.lang == Language::Bopomofo {
                break;
            }
            merged.push_str(&s.keys);
            // 至少要兩段才算「合併」，單段本來就是那樣
            if offset > 0 && crate::english::is_word(&merged) {
                best = Some(i + offset);
            }
        }
        match best {
            Some(j) => {
                let keys: String = segs[i..=j].iter().map(|s| s.keys.as_str()).collect();
                out.push(Segment {
                    keys,
                    lang: Language::English,
                    is_mark: false,
                });
                i = j + 1;
            }
            None => {
                out.push(segs[i].clone());
                i += 1;
            }
        }
    }
    out
}

/// 規則一：單一字母英文強制併入前一段。
///
/// **但不能吃注音**——`deadline␣前␣確認` 的 `deadline` 尾巴若把
/// 注音的「前」也拖下水，那一段就毀了。所以前一段必須是英文或日文。
///
/// # 併入是暫時的，後面成詞就還回去
///
/// 這條規則往前黏，**在詞的內部是對的，跨過詞的邊界就錯**：
///
/// ```text
/// 日:che | 英:c | 英:k        c 是 check 的第 4 個字母 → 往回黏，對
/// 日:dennwa | 英:c | 日:o…    c 是 config 的第 1 個字母 → 往回黏，錯
/// ```
///
/// 逐字打到 `dennwac` 的那一刻，資訊不足以分辨這兩種——要等 `config`
/// 打完才知道 `c` 屬於後面。而每打一鍵都會重跑整串，所以只要在黏之前
/// 先**往後看**一眼：這個字母跟後面接起來會不會剛好是個英文詞？
///
/// ```text
/// c + onfig = config  ✓ 成詞 → 不黏，讓它跟後面走
/// c + k     = ck      ✗ 不成詞 → 黏回 che
/// ```
///
/// 沒有這一眼的話，`dennwa` 會被 `c` 拖成英文，之後同語言合併把整串
/// 黏成 `英:dennwaconfig`——連規則三都救不了，因為那一整串查不到。
fn rule1_single_letter(segs: Vec<Segment>) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    for (i, s) in segs.iter().enumerate() {
        let single_en = s.lang == Language::English
            && s.keys.chars().count() == 1
            && s.keys != SEPARATOR
            && s.keys.chars().all(|c| c.is_ascii_alphabetic());
        let can_merge = out
            .last()
            .map(|p| matches!(p.lang, Language::English | Language::Romaji) && !p.is_mark)
            .unwrap_or(false);
        if single_en && can_merge && !starts_english_word(&segs[i..]) {
            let last = out.last_mut().unwrap();
            last.keys.push_str(&s.keys);
            last.lang = Language::English;
        } else {
            out.push(s.clone());
        }
    }
    out
}

/// 這個字母是不是**後面某個英文詞的開頭**？
///
/// 從 `segs[0]`（那個單一字母）開始往後接，只要接出來的東西是個英文詞
/// 就算——`c` ＋ `o` ＋ `n` ＋ `fi` ＋ `g` = `config` ✓。
///
/// 遇到分隔符、標點或注音就停：那是詞的邊界，跨過去接沒有意義。
///
/// 詞庫沒載入時一律回 false，維持舊的無條件併入行為——切點引擎要能在
/// 沒有詞庫的情況下運作。
fn starts_english_word(segs: &[Segment]) -> bool {
    if !crate::english::is_loaded() {
        return false;
    }
    let mut acc = String::new();
    for s in segs {
        if s.is_mark || s.lang == Language::Bopomofo {
            break;
        }
        acc.push_str(&s.keys);
        // 至少要接到後面的東西才算「往後成詞」，單獨一個字母不算
        if acc.chars().count() > 1 && crate::english::is_word(&acc) {
            return true;
        }
    }
    false
}

/// 規則二：結構為【英 + 日 + 英】時合併為【英】。
///
/// `keyboard` = `key`(英) + `bo`(日) + `ard`(英) → 三段併成一段英文。
/// 被英文夾住的日文段，多半是英文單字裡剛好像日文的部分。
fn rule2_en_ja_en(segs: Vec<Segment>) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    let mut i = 0usize;
    while i < segs.len() {
        let is_sandwich = i + 2 < segs.len()
            && segs[i].lang == Language::English
            && !segs[i].is_mark
            && segs[i + 1].lang == Language::Romaji
            && segs[i + 2].lang == Language::English
            && !segs[i + 2].is_mark;
        if is_sandwich {
            let merged = format!("{}{}{}", segs[i].keys, segs[i + 1].keys, segs[i + 2].keys);
            out.push(Segment {
                keys: merged,
                lang: Language::English,
                is_mark: false,
            });
            i += 3;
        } else {
            out.push(segs[i].clone());
            i += 1;
        }
    }
    out
}

/// 相鄰同語言合併（詞列組成的收尾）。
///
/// 前兩條規則可能產生相鄰的同語言段，這裡收攏。
fn merge_adjacent(segs: Vec<Segment>) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    for s in segs {
        match out.last_mut() {
            // 標點與分隔符（`is_mark`）不參與合併——它們一律自成一段。
            // 用 `keys != SEPARATOR` 判斷不夠：標點段的語言標成 English，
            // 會跟前面的英文段合併，`hello,` 的逗號就黏回去了。
            Some(last) if last.lang == s.lang && !last.is_mark && !s.is_mark => {
                last.keys.push_str(&s.keys);
            }
            _ => out.push(s),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(keys: &str, lang: Language) -> Segment {
        Segment {
            keys: keys.to_string(),
            lang,
            is_mark: keys == SEPARATOR,
        }
    }

    fn show(segs: &[Segment]) -> String {
        segs.iter()
            .map(|s| format!("{}:{}", s.lang.short(), s.keys))
            .collect::<Vec<_>>()
            .join(" | ")
    }

    #[test]
    fn 規則一_單字母英文併入前面() {
        // check → 日:che | 英:ck ... 先變成 日:che | 英:c | 英:k？
        // 實際上 ck 是兩個字元，這裡測單一字母的情況
        let input = vec![
            seg("che", Language::Romaji),
            seg("c", Language::English),
            seg("k", Language::English),
        ];
        let out = apply(input);
        assert_eq!(
            show(&out),
            "英:chec | 英:k".replace("英:chec | 英:k", "英:check")
        );
    }

    #[test]
    fn 規則一_不能吃注音() {
        // deadline 的尾巴不該把注音的「前」拖下水
        let input = vec![
            seg("deadline", Language::English),
            seg(SEPARATOR, Language::English),
            seg("fu06", Language::Bopomofo),
        ];
        let out = apply(input);
        assert_eq!(out[2].lang, Language::Bopomofo, "注音要保住");
    }

    #[test]
    fn 規則二_英日英夾心() {
        // keyboard = key + bo + ard
        let input = vec![
            seg("key", Language::English),
            seg("bo", Language::Romaji),
            seg("ard", Language::English),
        ];
        let out = apply(input);
        assert_eq!(show(&out), "英:keyboard");
    }

    #[test]
    fn 分隔符不參與合併() {
        let input = vec![
            seg("abc", Language::English),
            seg(SEPARATOR, Language::English),
            seg("def", Language::English),
        ];
        let out = apply(input);
        assert_eq!(out.len(), 3, "分隔符要留著：{}", show(&out));
    }

    #[test]
    fn 相鄰同語言合併() {
        let input = vec![
            seg("su3", Language::Bopomofo),
            seg("cl3", Language::Bopomofo),
        ];
        let out = apply(input);
        assert_eq!(show(&out), "注:su3cl3");
    }
}
