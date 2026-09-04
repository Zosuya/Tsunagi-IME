//! 空白鍵的角色判斷：聲調，還是切點？
//!
//! 依據 `通用語言輸入法 篩選規則.canvas` 的「空白鍵切點邏輯」那格。
//!
//! # 為什麼這是最難的一段
//!
//! 空白鍵有兩種身分：
//!
//! ```text
//! check␣u␣vu84       （check 一下）
//!       ↑   ↑
//!   分隔符  「一」的一聲
//! ```
//!
//! 判斷它是哪一種，需要知道左邊那段是不是注音；但判斷左邊那段是不是
//! 注音，又需要先切開——**先有雞還是先有蛋**。
//!
//! # 兩階段解法
//!
//! ```text
//! 階段1：所有空白都先當切點，切開左邊區塊
//!        → 語言邊界因此明確，循環依賴消失
//! 階段2：左邊區塊最後一段是注音嗎？
//!          否 → 空白是切點（自成一段）
//!          是 → 那一段＋空白是合法注音嗎？
//!                 否 → 空白是切點
//!                 是 → 空白是聲調（併入該段）
//! ```
//!
//! **第一問「本來就是注音嗎」是關鍵**——它擋掉英文/日文的詞尾。
//! `notebook` 的 `k` 雖然 `k␣` 剛好是 ㄜˉ，但它是英文尾巴。

use crate::bopomofo;

/// 這個空白該不該併入左邊那段當聲調？
///
/// `left` 是空白左邊的**注音部分**，不含前面別的語言。
///
/// # 呼叫端要先剝掉非注音的前綴
///
/// `hirup␣wu0␣`（hi今天）的第一個空白是「今」的一聲，但直接傳
/// `hirup` 會判成 false——因為 `hi` 不是合法注音音節（ㄘㄛ 缺聲調），
/// 整個區塊切不出注音序列。
///
/// 呼叫端（分段邏輯）要先讓英文吃掉 `hi`，再拿 `rup` 來問。
pub fn is_tone(left: &str) -> bool {
    if left.is_empty() {
        return false;
    }

    // 階段2 第一問：左邊區塊的最後一段是注音嗎？
    //
    // 拿整個區塊去切音節，看最後一段。切不出來就代表這個區塊不是
    // 純注音——那它的尾巴不該被當成注音聲母。
    let Some(syllables) = split_prefix_syllables(left) else {
        return false;
    };
    let Some(last) = syllables.last() else {
        return false;
    };

    // 階段2 第二問：那一段＋空白是合法注音嗎？
    //
    // `ru/` 是 ㄐㄧㄥ（缺聲調），加上空白變成 ㄐㄧㄥˉ =「經」，
    // 所以這個空白是聲調。
    let with_space = format!("{last} ");
    bopomofo::syllable::check(&with_space) == bopomofo::Validity::Valid
}

/// 區塊**尾端**的注音音節在等這個空白收尾嗎？回傳該注音尾巴的起點。
///
/// # 跟 `is_tone` 的差別：這個會自己剝掉前綴
///
/// `is_tone` 要求整串都是注音，所以呼叫端得先把別的語言剝乾淨。
/// 但切區塊的時候還沒切段，剝不了——`configg6ru0␣`（config時間）
/// 拿整塊去問 `is_tone` 一定是 false，於是空白被當成分隔符切走，
/// 「時間」的 `ru0␣` 少了聲調就判非法，整段注音再也湊不回來。
///
/// 這裡改成從左往右找**最長的注音尾巴**：`configg6ru0` 找到
/// `g6ru0` 在等收尾，回傳 6。
///
/// 擋掉英文詞尾的機制沒變——靠的是 `is_tone` 要求尾巴能切成
/// 完整音節序列。`notebook` 的尾巴 `k` 雖然 `k␣` 是 ㄜˉ，但
/// `is_tone("k")` 為真，會誤判……所以還要求**尾巴不是整塊的最後一個字元**
/// 之外的保護：見下方的長度門檻。
pub fn tone_suffix_start(block: &str) -> Option<usize> {
    let chars: Vec<char> = block.chars().collect();
    // 從左往右試，取最長的注音尾巴
    for start in 0..chars.len() {
        let tail: String = chars[start..].iter().collect();
        if !is_tone(&tail) {
            continue;
        }
        // **整塊都是注音**時直接成立，不必多問。
        if start == 0 {
            return Some(0);
        }
        // 前面還有別的語言時，要求這個尾巴至少切得出一個完整音節
        // ——也就是「不只是最後那個待收尾的碎片」。
        //
        // 沒有這道門檻的話 `notebook` 會被拆成 `noteboo` ＋ 尾巴 `k`
        // （`k␣` 剛好是 ㄜˉ），英文詞尾又被當成聲調了。
        let syllables = split_prefix_syllables(&tail)?;
        if syllables.len() >= 2 {
            return Some(start);
        }
    }
    None
}

/// 從區塊尾端往前切出注音音節，回傳切出來的序列。
///
/// 跟 `bopomofo::split_syllables` 的差別：**允許最後一段缺聲調**。
/// 那正是我們要找的東西——`ru/` 這種「待收尾的音節」。
///
/// 前面的部分必須切得乾淨（都是完整音節），否則回 `None`：
/// `notebook` 切不出完整的注音音節序列，它的尾巴 `k` 就不算注音。
pub fn split_prefix_syllables(keys: &str) -> Option<Vec<String>> {
    let chars: Vec<char> = keys.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let mut matched = None;
        // 先試完整音節（含聲調）
        for len in (1..=4.min(chars.len() - i)).rev() {
            let seg: String = chars[i..i + len].iter().collect();
            if bopomofo::syllable::check(&seg) == bopomofo::Validity::Valid {
                matched = Some((seg, len));
                break;
            }
        }
        // 完整音節切不出來時，若剩下的部分「加上空白就合法」，
        // 那它就是待收尾的最後一段。
        if matched.is_none() {
            let rest: String = chars[i..].iter().collect();
            let with_space = format!("{rest} ");
            if bopomofo::syllable::check(&with_space) == bopomofo::Validity::Valid {
                out.push(rest);
                return Some(out);
            }
            return None;
        }
        let (seg, len) = matched.unwrap();
        out.push(seg);
        i += len;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 待收尾的注音音節_空白是聲調() {
        // `ru/` = ㄐㄧㄥ 缺聲調，加空白變「經」
        assert!(is_tone("u3ru/"), "已經");
        // 單一個音節
        assert!(is_tone("wu0"), "ㄊㄧㄢ → 天");
        assert!(is_tone("rup"), "ㄐㄧㄣ → 今");
    }

    #[test]
    fn 英文詞尾_空白是切點() {
        // `check` 的 `k` 剛好是 ㄜ，`k␣` 是合法注音——但 check 是英文，
        // 前面的 `chec` 切不出完整音節，所以不算。
        assert!(!is_tone("check"), "check 後面的空白是分隔符");
        assert!(!is_tone("notebook"), "notebook 同理");
        assert!(!is_tone("hotel"), "hotel 的 l 是 ㄠ");
        assert!(!is_tone("meeting"), "meeting 的 g 是 ㄕ");
    }

    #[test]
    fn 完整音節後面的空白是切點() {
        // 已經收尾的音節不需要再一個聲調。
        assert!(!is_tone("su3"), "你 已經有三聲");
        assert!(!is_tone("su3cl3"), "你好 已經收尾");
        assert!(!is_tone("rup "), "今 已經有一聲");
    }

    #[test]
    fn 空區塊() {
        assert!(!is_tone(""));
    }

    #[test]
    fn 日文區塊的尾巴不算注音() {
        assert!(!is_tone("sushi"), "すし");
        assert!(!is_tone("arigatou"), "ありがとう");
    }

    #[test]
    fn 混合區塊要先剝掉非注音的前綴() {
        // `hirup` = hi（英文）+ rup（ㄐㄧㄣ）。
        // 直接問整個區塊會判 false，因為 `hi` 不是合法注音音節
        // （ㄘㄛ 缺聲調），整串切不出注音序列。
        assert!(!is_tone("hirup"), "整個區塊問不出來");

        // 但剝掉英文前綴之後就對了——測資裡 `hirup␣wu0␣` 期望
        // `hi|今天`，那個空白正是「今」的一聲。
        assert!(is_tone("rup"), "剝掉 hi 之後");

        // 所以分段邏輯要負責先切出語言邊界，再拿注音的部分來問。
    }
}
