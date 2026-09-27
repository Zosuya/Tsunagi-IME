//! 切法清單：這串按鍵有哪幾種分段方式、現在選的是哪一種。
//!
//! 原本是整句選單（TAB）的後端，選單在 2026-09-15 被段選單取代後刪掉，
//! 剩下的是測試、`show_ime`、設定頁除錯分頁用來**觀察**切法排序的查詢。
//!
//! **切法換了，選字就要重來**——那是不同的分段，格子數都可能不一樣。
//! 所以這裡的每個「換切法」動作都會回頭呼叫 `rebuild_slots`。

use super::*;

impl Session {
    /// 目前會送出的文字。
    pub fn text(&self) -> String {
        compose::text_of(&self.slots)
    }

    /// **真正送出（Enter／空白確認／切換語言／失焦……）時該送的文字**。
    ///
    /// 跟 `text()` 的差別只在鎖定日文、且還有半個 mora 卡在 `pending`
    /// 時：`sush` 按 Enter，`su` 已經湊成「す」進 `keys`（`text()` 就有），
    /// 但 `sh` 卡在 `pending`——`text()` 看不到它，導致使用者打過的字元
    /// 無聲消失（見開發文件「鎖定日文打半個 mora 直接送出」）。
    ///
    /// **裁決（2026-09-27）**：已成字的照常轉假名，沒成字的尾巴照原樣
    /// **字母**送出——`sush` + Enter → `すsh`，不是憑空消失，也不是硬湊
    /// 一個不完整的假名。
    ///
    /// 鎖定注音刻意不比照辦理：那條路已經有明確的相反規則（`input.rs`
    /// 的 `drain_keys` 註解、2026-09-20 的修法）——半個注音符號不成字，
    /// 送出時就是要丟掉，不是這個 bug 的範圍。自動模式與鎖定英文的
    /// `pending()` 本來就是空字串（見 `Input::pending`），呼叫這支等於
    /// `text()`，不受影響。
    pub fn commit_text(&self) -> String {
        let mut t = self.text();
        if self.lock == Some(crate::language::Language::Romaji) {
            t.push_str(&self.input.pending());
        }
        t
    }

    /// 目前的選字格。
    pub fn slots(&self) -> &[Slot] {
        &self.slots
    }

    /// 有幾種切法。
    pub fn cutting_count(&self) -> usize {
        self.cuttings.len()
    }

    /// 選的是第幾種切法。
    pub fn cutting_index(&self) -> usize {
        self.cutting_idx
    }

    /// 前 `n` 種切法各自會組出什麼文字（觀察排序用）。
    pub fn cutting_menu(&self, n: usize) -> Vec<String> {
        (0..self.cutting_count().min(n))
            .map(|k| compose::text_of(&self.preview_slots(k)))
            .collect()
    }

    /// 直接選第 `i` 種切法。
    ///
    /// 產品裡已經沒有入口（整句選單 2026-09-15 刪掉），留給
    /// `bench_cut_learn` 模擬「使用者挑了某種切法」。
    ///
    /// 超出範圍就當沒發生。
    pub fn set_cutting_index(&mut self, i: usize) {
        if i >= self.cutting_count() {
            return;
        }
        self.cutting_idx = i;
        self.remember_cut();
        self.rebuild_slots();
    }
}
