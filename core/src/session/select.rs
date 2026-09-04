//! 逐格選字：反白框移到哪一格、那一格有哪些字、選了之後怎麼辦。
//!
//! # 框與清單是兩件事
//!
//! **框是「現在在改哪一格」的游標，清單是「這一格有哪些字」**。
//! 左右鍵只移動框（`arrow_*`），上下鍵才叫出清單（`open_cands`）。
//! 混在一起的話，只是想把框移過去看看，候選視窗就一直彈出來擋著。
//!
//! 離開選字（`exit_select`）不等於忘記位置——位置記在 `last_select`，
//! 框會留在畫面上，下次方向鍵從那裡接續。

use super::*;

impl Session {
    /// 進入選字模式，落在**第一個**能選的格子。
    ///
    /// **從上次選過的那一格接續**，不是從頭開始——使用者改完第五個字、
    /// 按 Enter 離開之後又想改回來，反白跳回第一格等於要重跑一遍。
    /// 沒選過（或那一格已經不能選了）才用第一格。
    ///
    /// **英文段跳過**——`check` 沒有同音字的問題。
    ///
    /// 生產環境的方向鍵走的是 `enter_select_last`，那邊有說明為什麼。
    pub fn enter_select_first(&mut self) {
        self.select_idx = self.resume_or(self.next_selectable(None));
        self.cand_idx = 0;
        self.cand_expanded = false;
        self.cand_col_first = 0;
        // 先只出框，候選清單等使用者按下鍵才叫出來
        self.cands_open = false;
    }

    /// 上次停在哪一格？那一格還能選就用它，否則用 `fallback`。
    fn resume_or(&self, fallback: Option<usize>) -> Option<usize> {
        match self.last_select {
            Some(i) if self.slots.get(i).is_some_and(|s| s.selectable) => Some(i),
            _ => fallback,
        }
    }

    /// 該把哪一格標起來？
    ///
    /// 正在選字就是那一格；**選完離開之後仍然是剛才那一格**——使用者
    /// 要看得到自己剛改的是哪個字，反白整個消失的話會不確定改到沒有。
    pub fn marked_index(&self) -> Option<usize> {
        self.select_idx.or(self.last_select)
    }

    /// 進入選字模式，反白**最後一個**能選的格子。
    ///
    /// **方向鍵進選字一律走這個**（使用者定的）：剛打完字，插入點就在
    /// 尾端，最可能要改的是最後一個字；落在最前面等於要一路按回來。
    ///
    /// 按左鍵時更明顯——使用者的直覺是「從右邊選過來」，反白卻跑到
    /// 最左邊、再按左鍵沒反應，看起來就像跳過了最後一個字。
    ///
    /// 一樣以「上次停在哪一格」優先，見 `resume_or`。
    pub fn enter_select_last(&mut self) {
        let last = (0..self.slots.len())
            .rev()
            .find(|&i| self.slots[i].selectable);
        self.select_idx = self.resume_or(last);
        self.cand_idx = 0;
        self.cand_expanded = false;
        self.cand_col_first = 0;
        self.cands_open = false;
    }

    /// 目前反白哪一格。
    pub fn select_index(&self) -> Option<usize> {
        self.select_idx
    }

    /// 候選清單開著嗎？見 `cands_open` 欄位。
    pub fn cands_open(&self) -> bool {
        self.cands_open
    }

    /// 把候選清單叫出來（按下鍵）。
    pub fn open_cands(&mut self) {
        self.cands_open = true;
    }

    /// 按了右方向鍵。**這是方向鍵唯一該呼叫的入口。**
    ///
    /// 三種情況一次處理，因為對使用者來說它們是同一件事——「往右」：
    ///
    /// | 現在的狀態 | 行為 |
    /// |---|---|
    /// | 正在選字 | 移到下一格 |
    /// | 框留著（剛按完 Enter） | **接續那一格，然後真的往右移** |
    /// | 什麼都沒有 | 進選字，落在最後一格 |
    ///
    /// 中間那個是重點：框就在畫面上，使用者按右鍵的意思是「移到下一
    /// 格」。內部雖然要先「重新進入選字」，但那是實作細節——不吸收掉
    /// 的話會變成按兩下才動一格，很莫名其妙。
    pub fn arrow_right(&mut self) {
        match (self.select_idx, self.last_select) {
            (Some(_), _) => self.select_right(),
            (None, Some(_)) => {
                self.enter_select_last();
                self.select_right();
            }
            (None, None) => self.enter_select_last(),
        }
    }

    /// 按了左方向鍵。規則同 `arrow_right`，方向相反。
    pub fn arrow_left(&mut self) {
        match (self.select_idx, self.last_select) {
            (Some(_), _) => self.select_left(),
            (None, Some(_)) => {
                self.enter_select_last();
                self.select_left();
            }
            (None, None) => self.enter_select_last(),
        }
    }

    /// 反白往右移到下一個能選的格子。
    pub fn select_right(&mut self) {
        if let Some(next) = self.next_selectable(self.select_idx) {
            self.select_idx = Some(next);
            // **換格就要從頭反白**——上一格選到第 5 個，
            // 不代表這一格也要停在第 5 個
            self.cand_idx = 0;
            self.cand_expanded = false;
            self.cand_col_first = 0;
        }
    }

    /// 反白往左移。
    pub fn select_left(&mut self) {
        let Some(cur) = self.select_idx else { return };
        if let Some(prev) = (0..cur).rev().find(|&i| self.slots[i].selectable) {
            self.select_idx = Some(prev);
            self.cand_idx = 0;
            self.cand_expanded = false;
            self.cand_col_first = 0;
        }
    }

    /// 從 `from` 之後找下一個能選的格子（`None` 代表從頭找）。
    fn next_selectable(&self, from: Option<usize>) -> Option<usize> {
        let start = from.map(|i| i + 1).unwrap_or(0);
        (start..self.slots.len()).find(|&i| self.slots[i].selectable)
    }

    /// 反白那一格的候選字。
    pub fn char_candidates(&self) -> Vec<String> {
        self.select_idx
            .and_then(|i| self.slots.get(i))
            .map(compose::candidates_for)
            .unwrap_or_default()
    }

    /// 候選字清單裡反白第幾個。
    pub fn cand_index(&self) -> usize {
        self.cand_idx
    }

    /// 候選字反白往下一個。到底繞回開頭。
    ///
    /// **展開狀態下只在同一欄內上下跑**（使用者定的：上下同欄移動、
    /// 左右換欄）。跑到欄底繞回該欄的頂端，不會溢到隔壁欄。
    pub fn next_cand(&mut self) {
        // **關著的時候第一下只負責打開**，不順便換字——使用者按下鍵
        // 的意思是「讓我看看有哪些」，不是「換成第二個」
        if !self.cands_open {
            self.cands_open = true;
            return;
        }
        let n = self.char_candidates().len();
        if n == 0 {
            return;
        }
        if self.cand_expanded {
            let (lo, hi) = self.column_range(n);
            self.cand_idx = if self.cand_idx + 1 >= hi {
                lo
            } else {
                self.cand_idx + 1
            };
        } else {
            let visible = n.min(CHAR_PAGE);
            // **打到底再按一次就自動展開**（使用者定的）：九個裡沒有
            // 想要的字，繼續往下的意思就是「還要看更多」，不該讓使用者
            // 先繞回第一個、再想起有個右鍵可以展開。
            // 跳到第二欄的第一個——第一欄就是剛剛看過的那九個。
            if self.cand_idx + 1 >= visible && n > visible {
                self.cand_expanded = true;
                self.cand_col_first = 0;
                self.cand_idx = CHAR_COLUMN;
                return;
            }
            self.cand_idx = (self.cand_idx + 1) % visible;
        }
    }

    /// 直接反白第 `i` 個候選字——滑鼠點選用。
    ///
    /// **`i` 是畫面上的第幾個，不是絕對索引**：沒展開時視窗只畫前
    /// `CHAR_PAGE` 個，展開又捲動過的話畫面第一個也不是第 0 個。
    /// 點不到的就不該選得到，所以超出可見範圍一律忽略。
    pub fn set_cand_index(&mut self, i: usize) {
        let view = self.cand_visible_range();
        if i < view.len() {
            self.cand_idx = view.start + i;
        }
    }

    /// 候選字反白往上一個。
    pub fn prev_cand(&mut self) {
        if !self.cands_open {
            self.cands_open = true;
            return;
        }
        let n = self.char_candidates().len();
        if n == 0 {
            return;
        }
        if self.cand_expanded {
            let (lo, hi) = self.column_range(n);
            self.cand_idx = if self.cand_idx <= lo {
                hi - 1
            } else {
                self.cand_idx - 1
            };
        } else {
            let visible = n.min(CHAR_PAGE);
            self.cand_idx = (self.cand_idx + visible - 1) % visible;
        }
    }

    /// 目前反白所在那一欄的索引範圍 `[lo, hi)`。
    ///
    /// 最後一欄可能不滿，所以 `hi` 要夾在總數內。
    fn column_range(&self, n: usize) -> (usize, usize) {
        let col = self.cand_idx / CHAR_COLUMN;
        let lo = col * CHAR_COLUMN;
        (lo, (lo + CHAR_COLUMN).min(n))
    }

    /// 選中目前反白的候選字，然後往右移到下一格。
    ///
    /// 這是使用者按 Enter 的行為——選中反白，不是送出。
    ///
    /// **選完最後一格就自動離開選字狀態**：後面沒有格子可以選了，
    /// 卡在原地會讓使用者按 Enter 沒反應，以為當掉。回傳 `true`
    /// 代表已經離開選字模式。
    pub fn confirm_cand(&mut self) -> bool {
        self.confirm_cand_with(true)
    }

    /// `advance` 為 `false` 時選完就離開選字，不移到下一格。
    ///
    /// 對應設定 `behavior.enter_in_select`：`Next`（新注音式，選下一個字）
    /// 或 `Exit`（微軟注音式，選完就退出）。見開發文件 §1.6。
    pub fn confirm_cand_with(&mut self, advance: bool) -> bool {
        let Some(choice) = self.char_candidates().get(self.cand_idx).cloned() else {
            return false;
        };
        self.pick_char(&choice);
        // 還有下一格能選就移過去；沒有的話這輪選字就結束了
        if advance && self.next_selectable(self.select_idx).is_some() {
            self.select_right();
            false
        } else {
            self.exit_select();
            true
        }
    }

    /// 在反白那一格選了 `choice`。
    ///
    /// 選完之後**後面的格子會跟著改**——使用者定的規則：
    /// 「選了『你』，『郝』就自己變成『好』」。
    pub fn pick_char(&mut self, choice: &str) {
        let Some(i) = self.select_idx else { return };
        // 在這個切法底下選了字，等於認可了這個分段——
        // 之後重算要找回它，不能跳回第一名
        self.remember_cut();
        // 記下來，之後重建格子時要套回去（見 `reapply_picks`）
        let keys = self.slots[i].keys.clone();
        self.picks.retain(|(k, _)| *k != keys);
        self.picks.push((keys, choice.to_string()));
        compose::pick(&mut self.slots, i, choice);
    }

    /// 選完字之後離開選字模式。
    ///
    /// **記住停在哪一格**：離開只代表「不再吃候選字的上下鍵」，不代表
    /// 忘記使用者在看哪裡。下次按方向鍵要從這裡接續（見 `enter_select`），
    /// 而且那一格的標記要留著（見 `marked_index`）。
    pub fn exit_select(&mut self) {
        self.last_select = self.select_idx.or(self.last_select);
        self.select_idx = None;
        self.cands_open = false;
        self.cand_idx = 0;
        self.cand_expanded = false;
        self.cand_col_first = 0;
    }

    /// 候選字展開全部了嗎？
    pub fn cand_expanded(&self) -> bool {
        self.cand_expanded
    }

    /// 展開全部候選（右鍵）。
    ///
    /// 一般狀態只列前 `CHAR_PAGE` 個，展開後全部列出、分成多欄。
    /// 右鍵在選字模式本來是「換到下一格」，但那跟展開衝突——
    /// 使用者決定右鍵拿來展開，換格改用其他方式。
    pub fn expand_cands(&mut self) {
        if self.select_idx.is_some() {
            self.cand_expanded = true;
        }
    }

    /// 收回展開狀態，回到一般的一直排。
    ///
    /// 反白位置要跟著夾回可見範圍——展開時選到第 25 個，
    /// 收回後只剩 10 個，指著第 25 個會超出範圍。
    pub fn collapse_cands(&mut self) {
        self.cand_expanded = false;
        self.cand_col_first = 0;
        if self.cand_idx >= CHAR_PAGE {
            self.cand_idx = CHAR_PAGE.saturating_sub(1);
        }
    }

    /// 展開狀態下要顯示幾欄。**最多 `MAX_COLUMNS` 欄**，見那個常數。
    pub fn cand_columns(&self) -> usize {
        if !self.cand_expanded {
            return 1;
        }
        let visible = self.cand_visible_range().len();
        visible.div_ceil(CHAR_COLUMN).max(1)
    }

    /// 畫面上看得到的候選字是哪一段（絕對索引）。
    ///
    /// 沒展開時是前 `CHAR_PAGE` 個；展開時是從 `cand_col_first` 那一欄
    /// 起算的 `MAX_COLUMNS` 欄。**畫面與滑鼠命中都要以這一段為準**
    /// ——多出來的候選不在畫面上，不該選得到。
    pub fn cand_visible_range(&self) -> std::ops::Range<usize> {
        let n = self.char_candidates().len();
        if !self.cand_expanded {
            return 0..n.min(CHAR_PAGE);
        }
        let start = (self.cand_col_first * CHAR_COLUMN).min(n);
        let end = (start + MAX_COLUMNS * CHAR_COLUMN).min(n);
        start..end
    }

    /// 反白的是**畫面上**第幾個（相對索引）。捲到看不見它時是 `None`。
    ///
    /// 捲動之後絕對索引跟畫面位置就對不起來了——繪製那層只認得到
    /// 自己拿到的那幾個候選，所以要換算過再交出去。
    ///
    /// **拖捲軸看別欄時不該有反白**：那時反白還留在原本那一欄，
    /// 硬夾到畫面內會在不相干的字上亮一條，看起來像選中了它。
    pub fn cand_index_in_view(&self) -> Option<usize> {
        let view = self.cand_visible_range();
        view.contains(&self.cand_idx)
            .then(|| self.cand_idx - view.start)
    }

    /// 直接把可見範圍捲到第 `first` 欄——滑鼠拖捲軸用。
    ///
    /// **反白不跟著跑**：拖捲軸的意思是「我看看別欄有什麼」，
    /// 不是「我選了那裡」。超出可捲範圍就夾住。
    pub fn set_cand_col_first(&mut self, first: usize) {
        if !self.cand_expanded {
            return;
        }
        let total = self.char_candidates().len().div_ceil(CHAR_COLUMN).max(1);
        self.cand_col_first = first.min(total.saturating_sub(MAX_COLUMNS));
    }

    /// 按數字鍵 `n`（0 起算）要選的是哪一個候選（絕對索引）。
    ///
    /// **數字鍵只對應目前反白那一欄**（見 `CHAR_COLUMN` 的說明）——
    /// 展開後畫面上每一欄都各自標著 1-9，用絕對索引會選到別欄去。
    pub fn cand_number_index(&self, n: usize) -> Option<usize> {
        let total = self.char_candidates().len();
        let i = if self.cand_expanded {
            self.column_range(total).0 + n
        } else {
            n
        };
        (i < total && self.cand_visible_range().contains(&i)).then_some(i)
    }

    /// 展開時的橫向捲動狀態：`(可見的第一欄, 總欄數)`。
    ///
    /// **十欄以內回 `None`**——全部看得到就不該畫捲軸，那只是雜訊。
    /// 畫面那層靠這兩個數字算滑塊該多長、停在哪。
    pub fn cand_scroll(&self) -> Option<(usize, usize)> {
        if !self.cand_expanded {
            return None;
        }
        let total = self.char_candidates().len().div_ceil(CHAR_COLUMN).max(1);
        (total > MAX_COLUMNS).then_some((self.cand_col_first, total))
    }

    /// 讓反白所在那一欄回到可見範圍內。
    ///
    /// 只捲**剛好一欄**——反白往右頂到邊就整片推一欄過去，
    /// 使用者的視線不必重新找位置。
    fn scroll_cand_into_view(&mut self) {
        let col = self.cand_idx / CHAR_COLUMN;
        if col < self.cand_col_first {
            self.cand_col_first = col;
        } else if col >= self.cand_col_first + MAX_COLUMNS {
            self.cand_col_first = col + 1 - MAX_COLUMNS;
        }
    }

    /// 反白往右一欄。**上下同欄移動、左右換欄**（使用者定的）。
    ///
    /// 換欄時保持在同一列——視覺上反白是水平移動過去的。
    pub fn cand_right_column(&mut self) {
        if !self.cand_expanded {
            return;
        }
        let n = self.char_candidates().len();
        let next = self.cand_idx + CHAR_COLUMN;
        if next < n {
            self.cand_idx = next;
            self.scroll_cand_into_view();
        }
    }

    /// 反白往左一欄。
    pub fn cand_left_column(&mut self) {
        if !self.cand_expanded {
            return;
        }
        self.cand_idx = self.cand_idx.saturating_sub(CHAR_COLUMN);
        self.scroll_cand_into_view();
    }
}
