//! 切法選單：這串按鍵有哪幾種分段方式、現在選的是哪一種。
//!
//! **切法換了，選字就要重來**——那是不同的分段，格子數都可能不一樣。
//! 所以這裡的每個「換切法」動作都會回頭呼叫 `rebuild_slots`。

use super::*;

impl Session {
    /// 目前會送出的文字。
    pub fn text(&self) -> String {
        compose::text_of(&self.slots)
    }

    /// 目前的選字格。
    pub fn slots(&self) -> &[Slot] {
        &self.slots
    }

    /// 選單有幾列。
    ///
    /// 不是「有幾種切法」——尾巴可能多一列「注音符號直出」。
    pub fn cutting_count(&self) -> usize {
        self.cuttings.len() + usize::from(self.symbol_pos.is_some())
    }

    /// 選了選單的第幾列。
    pub fn cutting_index(&self) -> usize {
        self.cutting_idx
    }

    /// 切法選單要顯示的文字，最多 `n` 個。
    pub fn cutting_menu(&self, n: usize) -> Vec<String> {
        let symbols = self.symbol_row();
        (0..self.cutting_count().min(n))
            .map(|i| match self.cut_at(i) {
                // 「注音符號直出」那一列：顯示符號本身，不經過選詞層
                None => format!("（ㄅ）{}", symbols.clone().unwrap_or_default()),
                Some(k) => {
                    let c = &self.cuttings[k];
                    let text = compose::text_of(&compose::compose_with(c, self.width));
                    // 語言代表前面加個記號，一眼看出「這是日文的最佳解」
                    match self.rep_of.get(k) {
                        Some(langs) if !langs.is_empty() => {
                            let m: String = langs.iter().map(|l| Self::rep_mark(*l)).collect();
                            format!("（{m}）{text}")
                        }
                        _ => text,
                    }
                }
            })
            .collect()
    }

    /// 語言代表在選單上的記號。
    ///
    /// 用「中」不用「注」——選單顯示的是**輸出的文字**，使用者看到的
    /// 是中文，不是注音符號。（鎖定狀態那邊顯示「注」是對的，那講的
    /// 是輸入方式。）
    fn rep_mark(lang: crate::language::Language) -> &'static str {
        use crate::language::Language;
        match lang {
            Language::Bopomofo => "中",
            Language::Romaji => "日",
            Language::English => "英",
        }
    }

    /// 切法選單：下一個。到底就繞回開頭。
    pub fn next_cutting(&mut self) {
        let n = self.cutting_count();
        if n == 0 {
            return;
        }
        self.cutting_idx = (self.cutting_idx + 1) % n;
        self.remember_cut();
        self.rebuild_slots();
    }

    /// 直接選第 `i` 種切法——**滑鼠點的是哪一列就是哪一種**，
    /// 不像鍵盤只能一格一格移。超出範圍就當沒發生。
    pub fn set_cutting_index(&mut self, i: usize) {
        if i >= self.cutting_count() {
            return;
        }
        self.cutting_idx = i;
        self.remember_cut();
        self.rebuild_slots();
    }

    /// 切法選單：上一個。
    pub fn prev_cutting(&mut self) {
        let n = self.cutting_count();
        if n == 0 {
            return;
        }
        self.cutting_idx = (self.cutting_idx + n - 1) % n;
        self.remember_cut();
        self.rebuild_slots();
    }
}
