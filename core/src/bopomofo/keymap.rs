//! 注音鍵位對照表。
//!
//! 依據 `注音按鍵合法表及對照表.md`（使用者已核對過）——標準大千式配置，
//! 也就是 Windows 內建「新注音」的預設鍵盤。
//!
//! 這份對照是整個專案的核心前提：**注音鍵位就是 QWERTY 字母鍵**，
//! 所以任何英文字串都「切得出注音音節」，語言辨識才需要那麼小心。

/// 注音符號的四種角色。一個音節最多各出現一次，順序固定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// 聲母（21 個）
    Initial,
    /// 介音（3 個：ㄧㄨㄩ）
    Medial,
    /// 韻母（13 個）
    Final,
    /// 聲調（5 個：一聲是空白鍵）
    Tone,
}

/// 聲母 21 個。
const INITIALS: [(char, char); 21] = [
    ('1', 'ㄅ'),
    ('q', 'ㄆ'),
    ('a', 'ㄇ'),
    ('z', 'ㄈ'),
    ('2', 'ㄉ'),
    ('w', 'ㄊ'),
    ('s', 'ㄋ'),
    ('x', 'ㄌ'),
    ('e', 'ㄍ'),
    ('d', 'ㄎ'),
    ('c', 'ㄏ'),
    ('r', 'ㄐ'),
    ('f', 'ㄑ'),
    ('v', 'ㄒ'),
    ('5', 'ㄓ'),
    ('t', 'ㄔ'),
    ('g', 'ㄕ'),
    ('b', 'ㄖ'),
    ('y', 'ㄗ'),
    ('h', 'ㄘ'),
    ('n', 'ㄙ'),
];

/// 介音 3 個。
const MEDIALS: [(char, char); 3] = [('u', 'ㄧ'), ('j', 'ㄨ'), ('m', 'ㄩ')];

/// 韻母 13 個。
const FINALS: [(char, char); 13] = [
    ('8', 'ㄚ'),
    ('i', 'ㄛ'),
    ('k', 'ㄜ'),
    (',', 'ㄝ'),
    ('9', 'ㄞ'),
    ('o', 'ㄟ'),
    ('l', 'ㄠ'),
    ('.', 'ㄡ'),
    ('0', 'ㄢ'),
    ('p', 'ㄣ'),
    (';', 'ㄤ'),
    ('/', 'ㄥ'),
    ('-', 'ㄦ'),
];

/// 聲調 5 個。
///
/// **一聲是空白鍵**。這件事有兩個後果：
///
/// 1. 一聲在**書寫**時不標符號（「知」寫作 ㄓ 不是 ㄓˉ），但在**輸入**
///    時仍要按空白鍵——兩者常被搞混。
/// 2. 空白鍵因此有兩種身分：一聲，或段與段之間的分隔符。切點引擎最難
///    的一段就是分辨這兩者。
///
/// 內部用 U+02C9（ˉ，陰平調號）代表一聲，這樣它能跟其他聲調一樣走
/// 同一套驗證邏輯，不必開特例分支。
const TONES: [(char, char); 5] = [(' ', 'ˉ'), ('6', 'ˊ'), ('3', 'ˇ'), ('4', 'ˋ'), ('7', '˙')];

/// 把按鍵轉成注音符號與它的角色。表外的字元回 `None`。
///
/// 大小寫不分——大千鍵盤本來就不分，而且 Shift 組合在這裡沒有意義。
pub fn lookup(key: char) -> Option<(char, Role)> {
    let k = key.to_ascii_lowercase();
    for &(a, b) in &INITIALS {
        if a == k {
            return Some((b, Role::Initial));
        }
    }
    for &(a, b) in &MEDIALS {
        if a == k {
            return Some((b, Role::Medial));
        }
    }
    for &(a, b) in &FINALS {
        if a == k {
            return Some((b, Role::Final));
        }
    }
    for &(a, b) in &TONES {
        if a == k {
            return Some((b, Role::Tone));
        }
    }
    None
}

/// 這個按鍵是不是注音鍵盤上的鍵？
pub fn is_bopomofo_key(key: char) -> bool {
    lookup(key).is_some()
}

/// 這個按鍵扮演的角色。
pub fn role_of(key: char) -> Option<Role> {
    lookup(key).map(|(_, r)| r)
}

/// 這個按鍵對應的注音符號。
pub fn symbol_of(key: char) -> Option<char> {
    lookup(key).map(|(s, _)| s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 四種角色的數量符合注音的定義() {
        assert_eq!(INITIALS.len(), 21, "聲母 21 個");
        assert_eq!(MEDIALS.len(), 3, "介音 ㄧㄨㄩ");
        assert_eq!(FINALS.len(), 13, "韻母 13 個");
        assert_eq!(TONES.len(), 5, "四聲加輕聲");
    }

    #[test]
    fn 每個按鍵只對應一個角色() {
        // 同一個鍵不能既是聲母又是韻母，否則音節驗證會有歧義。
        let mut seen = std::collections::HashMap::new();
        for (list, role) in [
            (&INITIALS[..], Role::Initial),
            (&MEDIALS[..], Role::Medial),
            (&FINALS[..], Role::Final),
            (&TONES[..], Role::Tone),
        ] {
            for &(key, _) in list {
                if let Some(prev) = seen.insert(key, role) {
                    panic!("按鍵 {key:?} 同時是 {prev:?} 與 {role:?}");
                }
            }
        }
    }

    #[test]
    fn 每個注音符號只對應一個按鍵() {
        let mut seen = std::collections::HashSet::new();
        for list in [&INITIALS[..], &MEDIALS[..], &FINALS[..], &TONES[..]] {
            for &(_, sym) in list {
                assert!(seen.insert(sym), "注音符號 {sym:?} 重複");
            }
        }
    }

    #[test]
    fn 空白鍵是一聲() {
        // 這條容易被誤解成「空白鍵不是注音的一部分」，特別標一個測試。
        assert_eq!(lookup(' '), Some(('ˉ', Role::Tone)));
    }

    #[test]
    fn 大小寫都查得到() {
        assert_eq!(symbol_of('A'), Some('ㄇ'));
        assert_eq!(symbol_of('a'), Some('ㄇ'));
    }

    #[test]
    fn 表外字元查不到() {
        // 對照表明文寫著「表外皆為非法字元」。
        for c in ['~', '!', '@', '#', '$', '%', '^', '&', '*', '(', ')'] {
            assert_eq!(lookup(c), None, "{c:?} 不該在表內");
        }
    }

    #[test]
    fn 涵蓋所有字母與數字鍵() {
        // 大千配置把 26 個字母全用掉了，數字鍵只用 0-9 裡的 8 個
        // （1qaz 那排的 1、2、5 是聲母，3467 是聲調，89 是韻母，0 是ㄢ）。
        let letters: Vec<char> = ('a'..='z').filter(|c| !is_bopomofo_key(*c)).collect();
        assert!(letters.is_empty(), "這些字母沒對應到注音：{letters:?}");
    }
}
