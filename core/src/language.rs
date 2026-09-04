//! 語言辨識：一段按鍵屬於哪個語言。
//!
//! 依據 `語言辨識演算法(新).canvas` 的三層瀑布：
//!
//! ```text
//! 注音範圍? ─是→ 注音合法 ─合法→ 注音例外 ─合法→ Valid(注音)
//!    │否           │非法          │非法
//!    └─────────────┴──────────────┴→ Invalid → 日文範圍? → ... → Invalid → Passthrough(英文)
//! ```
//!
//! # 為什麼順序是安全的
//!
//! 注音與日文的按鍵集合**天然不相交**——注音必須以聲調鍵收尾
//! （`3` `4` `6` `7` 或空白），日文只用字母和 `-`。所以「注音優先」
//! 不會搶走任何日文，順序換過來結果也一樣。
//!
//! 這件事實測驗證過：兩者都判合法的按鍵組合是 **0 個**。

use crate::{bopomofo, romaji};

/// 一段按鍵屬於哪個語言。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Bopomofo,
    Romaji,
    /// 英文——瀑布的最後一站。前兩條都不成立就是它，
    /// 所以它不做合法性判斷，原樣輸出（passthrough）。
    English,
}

/// 這段按鍵屬於哪個語言。
///
/// 依 canvas 的瀑布順序問，第一個判合法的就是答案。
/// 都不合法時回 `English`——那是 passthrough，永遠有答案。
pub fn detect(keys: &str) -> Language {
    if bopomofo::validity(keys) == bopomofo::Validity::Valid {
        return Language::Bopomofo;
    }
    if romaji::validity(keys) == romaji::Validity::Valid {
        return Language::Romaji;
    }
    Language::English
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 注音() {
        assert_eq!(detect("su3"), Language::Bopomofo, "你");
        assert_eq!(detect("su3cl3"), Language::Bopomofo, "你好");
        assert_eq!(detect("rup wu0 "), Language::Bopomofo, "今天");
    }

    #[test]
    fn 日文() {
        assert_eq!(detect("a"), Language::Romaji, "あ");
        assert_eq!(detect("sushi"), Language::Romaji, "すし");
        assert_eq!(detect("arigatou"), Language::Romaji, "ありがとう");
        assert_eq!(detect("gannbatte"), Language::Romaji, "がんばって");
    }

    #[test]
    fn 英文是最後一站() {
        assert_eq!(detect("javascript"), Language::English);
        assert_eq!(detect("keyboard"), Language::English);
        assert_eq!(detect("password"), Language::English);
    }

    #[test]
    fn 空字串當英文() {
        // passthrough 永遠有答案。
        assert_eq!(detect(""), Language::English);
    }

    #[test]
    fn 注音與日文的按鍵集合不相交() {
        // 這是「注音優先」這個順序安全的根據：
        // 注音必須以聲調鍵收尾，日文永遠沒有聲調鍵。
        //
        // 窮舉 1~3 個字元，確認沒有任何組合被兩個引擎都判合法。
        let keys: Vec<char> = "abcdefghijklmnopqrstuvwxyz0123456789,./;- "
            .chars()
            .collect();
        let mut both = Vec::new();
        for &a in &keys {
            let s1 = a.to_string();
            check_disjoint(&s1, &mut both);
            for &b in &keys {
                let s2 = format!("{a}{b}");
                check_disjoint(&s2, &mut both);
                for &c in &keys {
                    let s3 = format!("{a}{b}{c}");
                    check_disjoint(&s3, &mut both);
                }
            }
        }
        assert!(both.is_empty(), "這些組合兩個引擎都判合法：{both:?}");
    }

    fn check_disjoint(s: &str, both: &mut Vec<String>) {
        if both.len() > 5 {
            return;
        }
        let bp = bopomofo::validity(s) == bopomofo::Validity::Valid;
        let ja = romaji::validity(s) == romaji::Validity::Valid;
        if bp && ja {
            both.push(s.to_string());
        }
    }
}
