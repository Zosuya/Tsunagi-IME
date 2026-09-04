//! 日文羅馬字的按鍵分類。
//!
//! 依據 `字元規則/日文按鍵合法表.md`（使用者整理並核對過）。
//! 表外皆為非法字元——**`q` 是唯一完全用不到的字母**。

/// 一個按鍵在日文羅馬字裡扮演的角色。
///
/// 同一個字母可能有多重身分（`n` 既是清音也是撥音、`y` 既是清音也是
/// 拗音中介），所以這裡用一組判斷函式而不是單一的 enum 對照。
pub const VOWELS: &str = "aiueo";

/// 清音的子音。
pub const SEION: &str = "hkmnrstwyc";

/// 濁音的子音。
pub const DAKUON: &str = "gdzbj";

/// 半濁音。
pub const HANDAKUON: &str = "p";

/// 外來語音 `v` `f`。
///
/// `v` → ゔ（`va`=ゔぁ、`vu`=ゔ）
/// `f` → ふ 行（`fa`=ふぁ、`fu`=ふ、`fyu`=ふゅ）——`fujisan`、`fairu` 會用到
pub const GAIRAI: &str = "vf";

/// 小寫打法（`xa`=ぁ、`xya`=ゃ、`xtu`=っ、`xwa`=ゎ、`xka`=ゕ）。
pub const KOGAKI: &str = "xl";

/// 長音符號。**不能出現在開頭**——它的語意是「延長前一個假名」。
pub const CHOUON: char = '-';

pub fn is_vowel(c: char) -> bool {
    VOWELS.contains(c)
}

pub fn is_seion(c: char) -> bool {
    SEION.contains(c)
}

pub fn is_dakuon(c: char) -> bool {
    DAKUON.contains(c)
}

pub fn is_handakuon(c: char) -> bool {
    HANDAKUON.contains(c)
}

pub fn is_gairai(c: char) -> bool {
    GAIRAI.contains(c)
}

pub fn is_kogaki(c: char) -> bool {
    KOGAKI.contains(c)
}

pub fn is_chouon(c: char) -> bool {
    c == CHOUON
}

/// 能當一個 mora 開頭的子音（清音／濁音／半濁音／外來語）。
///
/// 促音的條件「1=清音、濁音、半濁音、外來語 and 1≠n and 2=1」用到這個。
pub fn is_consonant(c: char) -> bool {
    is_seion(c) || is_dakuon(c) || is_handakuon(c) || is_gairai(c)
}

/// 這個字元在日文羅馬字裡有沒有可能出現。
///
/// 表外皆為非法——包含 `q`（日文羅馬字用不到）。
pub fn is_romaji_key(c: char) -> bool {
    is_vowel(c) || is_consonant(c) || is_kogaki(c) || is_chouon(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn q_是唯一用不到的字母() {
        // 日文羅馬字用不到 `q`——標準羅馬字系統（訓令式、平文式）
        // 都不需要它，`k` 已經涵蓋か行。
        let unused: Vec<char> = ('a'..='z').filter(|c| !is_romaji_key(*c)).collect();
        assert_eq!(unused, vec!['q'], "只有 q 用不到");
    }

    #[test]
    fn 分類不重疊() {
        // 一個字母可以同時是清音與拗音中介（`y`），但不能同時是清音與濁音。
        for c in 'a'..='z' {
            let n = [
                is_vowel(c),
                is_seion(c),
                is_dakuon(c),
                is_handakuon(c),
                is_gairai(c),
                is_kogaki(c),
            ]
            .iter()
            .filter(|x| **x)
            .count();
            assert!(n <= 1, "{c:?} 落在 {n} 個分類裡");
        }
    }

    #[test]
    fn 五個母音() {
        assert_eq!(VOWELS.len(), 5);
        for c in "aiueo".chars() {
            assert!(is_vowel(c));
        }
    }

    #[test]
    fn 撥音的_n_也是清音() {
        // `n` 有雙重身分：`na`/`ni` 的清音，以及單獨的撥音 ん。
        // 這裡只確認它在清音表裡；撥音的處理在 mora.rs。
        assert!(is_seion('n'));
    }

    #[test]
    fn 長音不算一般按鍵分類() {
        assert!(is_chouon('-'));
        assert!(!is_vowel('-'));
        assert!(!is_consonant('-'));
    }
}
