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
                    let text = compose::text_of(&self.preview_slots(k));
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

    /// 切法選單一次看得到幾列。
    ///
    /// 沒展開是 `CUTTING_PAGE`，展開後是 `CUTTING_PAGE_ALL`，
    /// 兩者都不會超過實際有幾列。
    pub fn cutting_shown(&self) -> usize {
        let page = if self.cutting_expanded {
            CUTTING_PAGE_ALL
        } else {
            CUTTING_PAGE
        };
        self.cutting_count().min(page)
    }

    /// 切法選單展開全部了嗎？
    pub fn cutting_expanded(&self) -> bool {
        self.cutting_expanded
    }

    /// 展開切法選單，列出 `CUTTING_PAGE_ALL` 列（空白鍵）。
    ///
    /// **反白不動**——展開只是「讓我多看幾列」，不是「換一個切法」。
    /// 已經展開了就什麼也不做（沒有更多可展開，`CUTTING_PAGE_ALL`
    /// 是上限）。
    pub fn expand_cutting(&mut self) {
        self.cutting_expanded = true;
    }

    /// 收回展開狀態，回到只列 `CUTTING_PAGE` 列。關掉選單時呼叫。
    ///
    /// **選中的切法不動**——使用者展開後挑了第 30 列，關掉選單是
    /// 「不看清單了」，不是「放棄剛才的選擇」。夾回第 10 列等於把
    /// 他選的字換掉，那是最惱人的那種 bug。
    ///
    /// 所以選中的列還在展開區時就維持展開：下次再開選單看到的仍是
    /// 那一列反白著。真正的重設在 `refresh`（又打字了）。
    pub fn collapse_cutting(&mut self) {
        if self.cutting_idx < CUTTING_PAGE {
            self.cutting_expanded = false;
        }
    }

    /// 切法選單：下一個。
    ///
    /// **走到看得到的最後一列再往下就自動展開**（跟選字的 `next_cand`
    /// 同一種手勢）：列到底了還要往下，意思就是「還要看更多」，不該
    /// 讓使用者先繞回第一列、再想起有另一個手勢可以展開。
    /// 展開之後仍然到底就繞回開頭——`CUTTING_PAGE_ALL` 是上限。
    ///
    /// **繞回的是「看得到的範圍」而不是全部**：只列 10 列時繞到第 11
    /// 列的話反白會整個消失在畫面外，使用者只看到反白不見了。
    pub fn next_cutting(&mut self) {
        let n = self.cutting_count();
        if n == 0 {
            return;
        }
        let visible = self.cutting_shown();
        if self.cutting_idx + 1 >= visible && !self.cutting_expanded && n > visible {
            // 展開，並直接落在原本看不到的第一列——那正是使用者要看的
            self.cutting_expanded = true;
            self.cutting_idx = visible;
        } else {
            self.cutting_idx = (self.cutting_idx + 1) % visible.max(1);
        }
        self.remember_cut();
        self.rebuild_slots();
    }

    /// 直接選第 `i` 種切法——**滑鼠點的是哪一列就是哪一種**，
    /// 不像鍵盤只能一格一格移。
    ///
    /// 超出**看得到的範圍**就當沒發生：畫面上沒畫出來的列點不到，
    /// 也就不該選得到（跟 `set_cand_index` 同一個道理）。
    pub fn set_cutting_index(&mut self, i: usize) {
        if i >= self.cutting_shown() {
            return;
        }
        self.cutting_idx = i;
        self.remember_cut();
        self.rebuild_slots();
    }

    /// 切法選單：上一個。到頂繞回**看得到的**最後一列。
    ///
    /// 跟 `next_cutting` 一樣只在可見範圍內繞——繞到畫面外的話
    /// 反白會消失。往上不會觸發展開（那是「往下要看更多」的動作）。
    pub fn prev_cutting(&mut self) {
        if self.cutting_count() == 0 {
            return;
        }
        let visible = self.cutting_shown().max(1);
        self.cutting_idx = (self.cutting_idx.min(visible - 1) + visible - 1) % visible;
        self.remember_cut();
        self.rebuild_slots();
    }
}
