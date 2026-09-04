//! 注音音節的累積緩衝——**打字當下**的狀態。
//!
//! 跟 `syllable.rs` 的分工：那邊回答「這串按鍵合不合法」（事後驗證），
//! 這邊回答「使用者現在打到哪、螢幕上該顯示什麼」（打字過程）。
//!
//! # 四個格子，同格覆寫
//!
//! 注音音節有固定的四個位置，一個鍵按下去是**寫進它所屬的那一格**，
//! 不是往字串後面接：
//!
//! ```text
//! 打 ㄅ  →  [ㄅ][  ][  ][  ]
//! 打 ㄆ  →  [ㄆ][  ][  ][  ]   ← 同一格，覆寫掉 ㄅ
//! 打 ㄧ  →  [ㄆ][ㄧ][  ][  ]   ← 不同格，往下填
//! 打 ˇ   →  完成，轉成字
//! ```
//!
//! 這是新酷音（libchewing）的行為：`syllable.update(bopomofo)` 按角色
//! 寫格子。好處是**打錯聲母直接打正確的就換掉**，不必先按 Backspace。
//!
//! # 為什麼日文不需要這個
//!
//! 羅馬字是線性序列，`ka` 的 `k` 跟 `a` 沒有「格」的概念，打錯只能刪。
//! mozc 的 `CharChunk` 是三層緩衝（raw／conversion／ambiguous），
//! 那是為了處理 `n` 可能是「ん」也可能是「な」的歧義——本專案規定
//! 撥音一律打 `nn`，歧義不存在，所以不需要那一層。

use super::keymap::{self, Role};

/// 一個正在打的注音音節。
///
/// 四個格子各存**一個注音符號**（不是按鍵）。空的格子是 `None`。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Syllable {
    initial: Option<char>,
    medial: Option<char>,
    final_: Option<char>,
    tone: Option<char>,
    /// 每一格對應的**原始按鍵**。
    ///
    /// 覆寫時要跟著換——打 `1`（ㄅ）再打 `q`（ㄆ），按鍵該是 `q`
    /// 不是 `1q`。所以按鍵不能另外用字串追加，得跟符號存在同一格。
    keys: [Option<char>; 4],
}

/// 按下一個鍵之後，這個音節怎麼了。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyResult {
    /// 收下了，繼續累積（畫面要更新）。
    Absorbed,
    /// 音節完成了——按到聲調鍵。呼叫端該把它轉成字、清空緩衝。
    Committed,
    /// 不是注音鍵，這裡不處理。
    Rejected,
}

impl Syllable {
    pub fn new() -> Self {
        Self::default()
    }

    /// 空的嗎？（一個符號都還沒打）
    pub fn is_empty(&self) -> bool {
        self.initial.is_none()
            && self.medial.is_none()
            && self.final_.is_none()
            && self.tone.is_none()
    }

    /// 打一個鍵。
    ///
    /// **同一格再打就是覆寫**——那是這個模組存在的理由。
    ///
    /// 聲調鍵是收尾：音節非空時按聲調就 `Committed`；音節是空的時候
    /// 按聲調則不收（那多半是誤按，或者在別的語言裡是空白鍵）。
    pub fn key_press(&mut self, key: char) -> KeyResult {
        let Some((symbol, role)) = keymap::lookup(key) else {
            return KeyResult::Rejected;
        };
        // **非法組合按下去就擋掉**（新酷音的行為）。
        //
        // ㄈ 之後按 ㄩ 直接不收，緩衝維持 ㄈ——使用者可能只是手滑，
        // 或接著要打的是 ㄈㄥ。讓非法音節累積起來的話，收尾時查不到
        // 字，顯示會退回原始按鍵（`zm ` 而不是注音符號），看起來
        // 像整個輸入法壞掉。
        //
        // 作法是先試寫，用 `viable` 驗，不通過就還原。
        let backup = self.clone();
        let result = self.write(key, symbol, role);
        if result != KeyResult::Rejected && !super::syllable::viable(&self.keys()) {
            *self = backup;
            return KeyResult::Rejected;
        }
        result
    }

    /// 把符號寫進它所屬的格子。合法性由 `key_press` 把關。
    fn write(&mut self, key: char, symbol: char, role: Role) -> KeyResult {
        match role {
            Role::Initial => {
                self.initial = Some(symbol);
                self.keys[0] = Some(key);
            }
            Role::Medial => {
                self.medial = Some(symbol);
                self.keys[1] = Some(key);
            }
            Role::Final => {
                self.final_ = Some(symbol);
                self.keys[2] = Some(key);
            }
            Role::Tone => {
                if self.is_empty() {
                    // 什麼都還沒打就按聲調——不收。空白鍵在這種情況下
                    // 該讓呼叫端當成一般的空白處理。
                    return KeyResult::Rejected;
                }
                self.tone = Some(symbol);
                self.keys[3] = Some(key);
                return KeyResult::Committed;
            }
        }
        KeyResult::Absorbed
    }

    /// 刪掉最後打的那個符號。
    ///
    /// **按格子的順序倒著刪**（聲調→韻母→介音→聲母），不是照打字
    /// 順序——那需要另外記歷程，而倒著刪的結果在絕大多數情況下相同：
    /// 使用者本來就是照聲介韻調的順序打的。
    pub fn backspace(&mut self) -> bool {
        for i in (0..4).rev() {
            let slot = match i {
                0 => &mut self.initial,
                1 => &mut self.medial,
                2 => &mut self.final_,
                _ => &mut self.tone,
            };
            if slot.is_some() {
                *slot = None;
                self.keys[i] = None;
                return true;
            }
        }
        false
    }

    /// 目前累積的注音符號，照聲介韻調的順序。
    ///
    /// 這就是組字區要顯示的東西——新酷音打 `su` 時顯示「ㄋㄧ」，
    /// 不是顯示按鍵本身。
    pub fn symbols(&self) -> String {
        [self.initial, self.medial, self.final_, self.tone]
            .into_iter()
            .flatten()
            .collect()
    }

    /// 目前累積的**原始按鍵**，照聲介韻調的順序。
    ///
    /// 覆寫過的舊按鍵不會留下——打 `1` 再打 `q` 回傳 `q`。
    /// 音節結算時要把這串接到 session 的按鍵串後面。
    pub fn keys(&self) -> String {
        self.keys.iter().flatten().collect()
    }

    /// 清空。
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 打一串鍵，回傳最後一個鍵的結果。
    fn press(s: &mut Syllable, keys: &str) -> KeyResult {
        let mut last = KeyResult::Rejected;
        for c in keys.chars() {
            last = s.key_press(c);
        }
        last
    }

    #[test]
    fn 同一格再打就覆寫() {
        // 這是整個模組存在的理由：打錯聲母不必刪，直接打正確的
        let mut s = Syllable::new();
        s.key_press('1'); // ㄅ
        assert_eq!(s.symbols(), "ㄅ");
        s.key_press('q'); // ㄆ
        assert_eq!(s.symbols(), "ㄆ", "同是聲母，該覆寫而不是變成 ㄅㄆ");
    }

    #[test]
    fn 不同格往下填() {
        let mut s = Syllable::new();
        press(&mut s, "su"); // ㄋ ㄧ
        assert_eq!(s.symbols(), "ㄋㄧ");
    }

    #[test]
    fn 介音也會覆寫() {
        let mut s = Syllable::new();
        press(&mut s, "su"); // ㄋㄧ
        s.key_press('j'); // ㄨ
        assert_eq!(s.symbols(), "ㄋㄨ", "介音同格覆寫");
    }

    #[test]
    fn 聲調收尾() {
        let mut s = Syllable::new();
        assert_eq!(press(&mut s, "su"), KeyResult::Absorbed);
        assert_eq!(s.key_press('3'), KeyResult::Committed, "ㄋㄧˇ 你");
        assert_eq!(s.symbols(), "ㄋㄧˇ");
    }

    #[test]
    fn 空音節按聲調不收() {
        // 什麼都沒打就按空白／數字鍵，該讓呼叫端當一般按鍵處理
        let mut s = Syllable::new();
        assert_eq!(s.key_press('3'), KeyResult::Rejected);
        assert!(s.is_empty());
    }

    #[test]
    fn 非注音鍵不收() {
        let mut s = Syllable::new();
        // `[` 不在注音鍵盤上
        assert_eq!(s.key_press('['), KeyResult::Rejected);
        assert!(s.is_empty());
    }

    #[test]
    fn 大小寫視為同一個鍵() {
        // `lookup` 會轉小寫——注音鍵盤上 A 跟 a 是同一個位置（ㄇ）。
        //
        // **這代表 Shift+字母 在這一層看不出差別**。鎖定注音時
        // Shift+A 該打出大寫英文（新酷音的暫時英數模式）還是 ㄇ，
        // 得由呼叫端決定，不是這裡。
        let mut upper = Syllable::new();
        let mut lower = Syllable::new();
        upper.key_press('A');
        lower.key_press('a');
        assert_eq!(upper.symbols(), lower.symbols());
        assert_eq!(upper.symbols(), "ㄇ");
    }

    #[test]
    fn backspace_倒著刪() {
        let mut s = Syllable::new();
        press(&mut s, "su"); // ㄋㄧ
        assert!(s.backspace());
        assert_eq!(s.symbols(), "ㄋ");
        assert!(s.backspace());
        assert!(s.is_empty());
        assert!(!s.backspace(), "空的時候該回 false，讓呼叫端去刪別的");
    }

    #[test]
    fn 完整音節的四格() {
        // ㄋㄧㄠˇ 鳥
        let mut s = Syllable::new();
        assert_eq!(press(&mut s, "sul3"), KeyResult::Committed);
        assert_eq!(s.symbols(), "ㄋㄧㄠˇ");
    }

    #[test]
    fn 一聲是空白鍵() {
        // ㄇㄚ 媽——一聲不標符號，但輸入時要按空白
        let mut s = Syllable::new();
        assert_eq!(press(&mut s, "a8 "), KeyResult::Committed);
        assert!(s.symbols().starts_with("ㄇㄚ"));
    }

    #[test]
    fn 覆寫時原始按鍵也跟著換() {
        // 這是 `keys` 要跟符號存同一格的理由：打 1 再打 q，
        // 按鍵該是 q 不是 1q，否則結算出去的按鍵串是錯的
        let mut s = Syllable::new();
        s.key_press('1'); // ㄅ
        assert_eq!(s.keys(), "1");
        s.key_press('q'); // ㄆ
        assert_eq!(s.keys(), "q", "覆寫後舊按鍵不該留下");
    }

    #[test]
    fn 按鍵串照聲介韻調順序() {
        let mut s = Syllable::new();
        press(&mut s, "sul3"); // ㄋㄧㄠˇ
        assert_eq!(s.keys(), "sul3");
    }

    #[test]
    fn 亂序打也照結構排() {
        // 先打韻母再補聲母——結算出去的按鍵串仍是引擎認得的順序
        let mut s = Syllable::new();
        s.key_press('l'); // ㄠ 韻母
        s.key_press('s'); // ㄋ 聲母
        assert_eq!(s.symbols(), "ㄋㄠ");
        assert_eq!(s.keys(), "sl", "按鍵要照聲介韻調排，不是打字順序");
    }

    #[test]
    fn 清空之後可以重來() {
        let mut s = Syllable::new();
        press(&mut s, "su3");
        s.clear();
        assert!(s.is_empty());
        assert_eq!(s.symbols(), "");
    }
}
