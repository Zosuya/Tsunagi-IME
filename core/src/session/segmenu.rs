//! 段選單：反白引擎切出來的**一段**，選它要當成什麼。
//!
//! # 跟切法選單的差別
//!
//! ```text
//! 切法選單（cutting.rs）   一列是一整種分段        整句挑
//! 段選單（這裡）            反白一段、列出它的可能   逐段挑
//! ```
//!
//! 長句 20 段，使用者只想改中間某一段，在整句選單裡要從幾十種排列
//! 組合裡大海撈針。段選單把粒度降到單段。
//!
//! # 「選了就凍」
//!
//! 使用者挑定某一段之後，**那一段以前全部定案**（交給輸入層的凍結區），
//! 後區在剩下的按鍵上重算。
//!
//! 這是這個選單真正的價值所在，不是省按鍵。整句切法要活到最後得
//! 「每一段都好**而且整句排名夠前**」——長句的正解常常在
//! `ALIVE_LIMIT` 就被砍了。前區定案讓後區變短，正解就活得下來。
//!
//! **跟 `cutpoint::incremental` 的分區凍結是同一招**，差別只在觸發者：
//! 那個是引擎判斷離游標夠遠，這個是使用者說了算——而使用者的意圖比
//! 任何啟發式都可靠。
//!
//! # 候選從哪來
//!
//! 從「這個起點開始、通過合法性引擎」的 `(長度, 語言)` 組合來，
//! **不限於既有的整句切法**。限制在既有清單裡的話就繼承了整句選單的
//! 限制，等於白做。
//!
//! 但也**不是什麼都能選**——每一段仍然要通過該語言的合法性判斷。

use super::*;
use crate::language::Language;

/// 一段最多幾個按鍵。
///
/// 太長的段沒有意義（沒有 30 個按鍵的注音詞），而候選要窮舉長度，
/// 上限也讓那個迴圈不至於失控。
const MAX_SEG_LEN: usize = 24;

/// 段選單一次列幾個候選。跟選字的 `CHAR_PAGE` 一樣對齊數字鍵。
pub const SEG_PAGE: usize = 9;

/// 台語斷詞往前看幾個字。
///
/// 台語包裡最長的華語詞是 11 字（「栓牛的小木樁」那種解釋性條目），
/// 但那些查不到也無所謂——日常詞很少超過 4 字。設 6 是折衷：夠涵蓋
/// 「便利商店」「腳踏車」這類，又不必每個位置都往後看十幾格。
const TW_MAX_WORD: usize = 6;

/// 台語斷詞切出來的一個詞。
///
/// **位置是「第幾格」不是「第幾個按鍵」**——斷詞在國字上做，而一格
/// 恰好是一個字（`Slot`）。要換回按鍵範圍時再去查那幾格的 `keys`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TwWord {
    /// 從第幾格開始
    pub at: usize,
    /// 幾格（＝幾個字）
    pub len: usize,
    /// 這個意思的**所有說法**，華語排第一（沙發／膨椅／胖椅）。
    ///
    /// 不分「原本的」跟「換過的」——它們地位相等，就像選字裡同一個音
    /// 的幾個字。詞表是雙向的（`pack::build_index`），從哪一個查都拿到
    /// 同一組。
    pub says: Vec<String>,
}

/// 段選單的一個候選：這一段可以是什麼。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegCand {
    /// 這一段的按鍵
    pub keys: String,
    /// 當成哪個語言
    pub lang: Language,
    /// 選了它之後這一段會顯示成什麼（給選單畫用）
    pub text: String,
    /// **這是台語的說法**（`tw` 那一層來的），不是這串注音的華語正解。
    ///
    /// # 為什麼要獨立一欄
    ///
    /// `keys` 與 `lang` 跟華語那筆**一模一樣**（都是同一串注音、都標
    /// `Bopomofo`），光靠它們分不出來。而下游要分得出來：
    ///
    /// - **學習層不可以記它**。`learn::record` 拿 `(keys, text)` 去記，
    ///   記進去就變成「`vu,4vu,4` → 感恩」，選幾次之後打「謝謝」的注音
    ///   永遠出「感恩」，**關掉台語包也救不回來**（污染寫進學習檔了）。
    ///   實測見開發文件 §2.64.13。
    /// - 選單上可以標示這是另一種語言的說法。
    pub taigi: bool,
}

impl Session {
    /// 段選單現在看到的分段。
    ///
    /// # 前區只有一個主人
    ///
    /// 使用者定案的段**存在輸入層的凍結區裡**（`Incremental::frozen`），
    /// 這裡不另存一份。`cuttings()` 給的每一種切法本來就把凍結的前區
    /// 接在前面，所以直接用，沒有組裝。
    ///
    /// 早期版本讓 `Session` 也存一份前區，同一段程式碼要應付「輸入層
    /// 帶不帶前區」兩種情況，只能比長度去猜——猜錯就少一截或多一截。
    /// 實測正確率從 71 句掉到 38 句，**兩個主人就是那個 bug 的根**。
    pub fn seg_segments(&self) -> Vec<Segment> {
        let mut out = if let Some(c) = self
            .cut_at(self.cutting_idx)
            .and_then(|i| self.cuttings.get(i))
            .or_else(|| self.cuttings.first())
        {
            c.clone()
        } else {
            // **整串都定案了**（最後一段也選過）——那時後區是空的，
            // `cuttings` 也空，但凍結區裡有完整的分段。
            self.input.frozen_segments()
        };
        // **空段不留**：全部定案之後尾巴會多一個空的（後區沒東西了），
        // 反白走過去就是「0 個候選」，看起來像壞掉。
        out.retain(|s| !s.keys.is_empty());
        out
    }

    /// 整串按鍵。輸入層的 `keys()` 對外語意就是完整按鍵串（含凍結區）。
    fn seg_all_keys(&self) -> String {
        self.input.keys().to_string()
    }

    /// 段選單有幾段。
    pub fn seg_count(&self) -> usize {
        self.seg_segments().len()
    }

    // ══ 台語：反白單位是「詞」 ══════════════════════════════
    //
    // 鎖定注音時整串就是一段，`seg_idx` 永遠是 0——所以台語這條路
    // **不動 `seg_idx`**，另外用 `tw_at` 記反白落在第幾格、`tw_len`
    // 記幾格寬。兩者的單位都是**格**（一格一個字），不是按鍵。

    /// 把組字區的國字用台語詞庫切成詞。**最大匹配**：從左往右掃，
    /// 每個位置試最長的詞，切過的不重複用。
    ///
    /// 「沙發」佔掉「沙」「發」之後，掃到「謝」時不會回頭試「發謝」
    /// ——這就是跨詞誤配消失的原因。
    ///
    /// **只回傳有台語說法的詞**（而且說法要跟華語不同）。切得到但
    /// 台語講法一樣的（「好→好」）對使用者是雜訊，他要的是「有別的
    /// 講法」的地方。
    pub fn tw_words(&self) -> Vec<TwWord> {
        if !crate::pack::any_tw() {
            return Vec::new();
        }
        let idx = crate::pack::index();
        let slots = self.slots();
        // 一格一個字才切得動——多字格（英文段、符號）跳過
        let chars: Vec<Option<char>> = slots
            .iter()
            .map(|s| {
                let mut it = s.text.chars();
                match (it.next(), it.next()) {
                    (Some(c), None) => Some(c),
                    _ => None,
                }
            })
            .collect();

        let mut out = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            // **一格裝著一整個詞**——定案成台語之後 `apply_taigi` 是整段
            // 換掉的（「沙發」兩格併成「膨椅」一格，台語詞跟華語詞 35.5%
            // 字數不同，逐格填不進去）。那一格本身就是一個詞，直接查。
            if chars[i].is_none() {
                // **後面被清空的格也算進來**：`apply_taigi` 只改文字，
                // 按鍵留在原本的格子裡（「沙發」兩格的按鍵沒有合併）。
                // 不涵蓋它們的話定案時只拿到半截按鍵，換不回去。
                let mut len = 1;
                while i + len < slots.len() && slots[i + len].text.is_empty() {
                    len += 1;
                }
                let word = &slots[i].text;
                if let Some(group) = idx.tw.get(word) {
                    if group.len() >= 2 {
                        out.push(TwWord {
                            at: i,
                            len,
                            says: group.clone(),
                        });
                    }
                }
                i += len;
                continue;
            }

            let mut matched = false;
            // 從長到短試——最大匹配
            for len in (1..=TW_MAX_WORD.min(chars.len() - i)).rev() {
                if chars[i..i + len].iter().any(Option::is_none) {
                    continue;
                }
                let word: String = chars[i..i + len].iter().flatten().collect();
                // **詞表是雙向的**（`pack::build_index`），所以畫面上
                // 顯示華語或台語都查得到同一組——定案成「膨椅」之後
                // 這裡照樣切得出來，不必另外記「原本是什麼」
                let Some(group) = idx.tw.get(&word) else {
                    continue;
                };
                // 只有一個說法＝沒有別的講法可選，不算一個詞
                if group.len() < 2 {
                    continue;
                }
                out.push(TwWord {
                    at: i,
                    len,
                    says: group.clone(),
                });
                i += len;
                matched = true;
                break;
            }
            if !matched {
                i += 1;
            }
        }
        out
    }

    /// 反白現在落在哪個詞上？回傳 `(起點格, 幾格)`。
    ///
    /// `tw_len` 是 0 代表「還沒開過選單」——這時取斷詞的第一個詞當
    /// 預設。使用者用 `Shift+←→` 調過寬度之後 `tw_len` 就有值了。
    fn tw_span(&self) -> Option<(usize, usize)> {
        let words = self.tw_words();
        if self.seg_tw_len > 0 {
            // 調過寬度：起點固定（使用者裁定丙案），只有長度會變
            return Some((self.seg_tw_at, self.seg_tw_len));
        }
        // 反白落在 `tw_at` 或它之後的第一個詞
        words
            .iter()
            .find(|w| w.at >= self.seg_tw_at)
            .or_else(|| words.first())
            .map(|w| (w.at, w.len))
    }

    /// 反白那個範圍的台語說法。
    /// 反白那個範圍的**所有說法**（華語排第一）。
    ///
    /// 直接拿畫面上的字去查就夠了——詞表是雙向的，顯示「沙發」或
    /// 「膨椅」都拿到同一組。
    fn tw_at_cursor(&self) -> Vec<String> {
        let Some((at, len)) = self.tw_span() else {
            return Vec::new();
        };
        let slots = self.slots();
        if at + len > slots.len() {
            return Vec::new();
        }
        let word: String = slots[at..at + len]
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        let idx = crate::pack::index();
        idx.tw.get(&word).cloned().unwrap_or_default()
    }

    /// 反白那個範圍對應的按鍵——定案時要用它換掉那一截。
    fn tw_cursor_keys(&self) -> String {
        let Some((at, len)) = self.tw_span() else {
            return String::new();
        };
        let slots = self.slots();
        if at + len > slots.len() {
            return String::new();
        }
        slots[at..at + len]
            .iter()
            .map(|s| s.keys.as_str())
            .collect()
    }

    /// 台語模式：反白的格範圍。繪製要用（組字區把那幾個字框起來）。
    pub fn tw_cursor_range(&self) -> Option<(usize, usize)> {
        self.tw_span()
    }

    /// 台語模式：跳到下一個詞。
    ///
    /// **跳過沒有台語說法的地方**——鎖定注音開段選單的目的就是找台語，
    /// 停在沒東西的位置只是浪費按鍵。
    pub fn tw_next(&mut self) {
        let words = self.tw_words();
        let cur = self.tw_span().map(|(a, _)| a).unwrap_or(0);
        if let Some(w) = words.iter().find(|w| w.at > cur) {
            self.seg_tw_at = w.at;
        } else if let Some(w) = words.first() {
            // 到底繞回，跟選字的 `next_cand` 一致
            self.seg_tw_at = w.at;
        }
        // 換詞就放掉手動調過的寬度——新的詞用它自己的長度
        self.seg_tw_len = 0;
        self.seg_cand = 0;
        self.seg_first = 0;
    }

    /// 台語模式：跳到上一個詞。
    pub fn tw_prev(&mut self) {
        let words = self.tw_words();
        let cur = self.tw_span().map(|(a, _)| a).unwrap_or(0);
        if let Some(w) = words.iter().rev().find(|w| w.at < cur) {
            self.seg_tw_at = w.at;
        } else if let Some(w) = words.last() {
            self.seg_tw_at = w.at;
        }
        self.seg_tw_len = 0;
        self.seg_cand = 0;
        self.seg_first = 0;
    }

    /// 台語模式：反白往右加一格（`Shift+→`）。
    ///
    /// **起點固定、只調長度**（使用者裁定丙案）：「我們」縮成「我」用
    /// `Shift+←`，要選「們」用 `←→` 跳過去，不必再加一組推左邊界的鍵。
    pub fn tw_widen(&mut self) {
        let Some((at, len)) = self.tw_span() else {
            return;
        };
        if at + len < self.slots().len() && len < TW_MAX_WORD {
            self.seg_tw_at = at;
            self.seg_tw_len = len + 1;
            self.seg_cand = 0;
            self.seg_first = 0;
        }
    }

    /// 台語模式：反白往左縮一格（`Shift+←`）。
    pub fn tw_narrow(&mut self) {
        let Some((at, len)) = self.tw_span() else {
            return;
        };
        if len > 1 {
            self.seg_tw_at = at;
            self.seg_tw_len = len - 1;
            self.seg_cand = 0;
            self.seg_first = 0;
        }
    }

    /// 現在走的是台語那條路嗎？（鎖定注音＋有台語包）
    ///
    /// 平台層要靠它決定 `←→` 是「換段」還是「換詞」。
    pub fn tw_mode(&self) -> bool {
        self.lock == Some(Language::Bopomofo) && crate::pack::any_tw()
    }

    /// 開段選單時叫一次——**台語模式的反白回到第一個詞**。
    ///
    /// **不放在 `exit_select` 裡**：那是選字的事，不該管段選單。而
    /// 不重設的話重開 TAB 會停在上次定案後的位置（實測：選完「膨椅」
    /// 關掉再開，反白停在「謝謝」，使用者以為回不去了）。
    ///
    /// **`seg_idx` 不動**——非台語模式關掉再開會停在原處，那是刻意的。
    pub fn seg_open(&mut self) {
        // **「走完了」的旗標兩條路都要清**——不清的話上一輪按 Enter
        // 退出之後，重開 TAB 會立刻又被判定成走完，選單開不起來。
        self.seg_done_flag = false;
        if self.tw_mode() {
            self.seg_tw_at = 0;
            self.seg_tw_len = 0;
            self.seg_cand = 0;
            self.seg_first = 0;
        }
    }

    /// 反白第幾段。
    pub fn seg_index(&self) -> usize {
        self.seg_idx
    }

    /// 那一段的候選裡反白第幾個（絕對索引）。
    pub fn seg_cand_index(&self) -> usize {
        self.seg_cand
    }

    /// 移動反白，**並把它捲進畫面**。
    ///
    /// 所有改變 `seg_cand` 的路徑都走這裡——漏掉任何一條，反白就會
    /// 跑到畫面外消失（實測回報「反白條不會動」就是這個）。
    fn set_seg_cand(&mut self, i: usize) {
        self.seg_cand = i;
        // 捲到看得見：往前捲對齊反白，往後捲讓它落在最後一個
        if self.seg_cand < self.seg_first {
            self.seg_first = self.seg_cand;
        } else if self.seg_cand >= self.seg_first + SEG_PAGE {
            self.seg_first = self.seg_cand + 1 - SEG_PAGE;
        }
    }

    /// 候選清單畫得出來的那一段（絕對索引的範圍）。
    ///
    /// 一段常有二三十個候選，一次只列 `SEG_PAGE` 個。
    pub fn seg_visible_range(&self) -> std::ops::Range<usize> {
        let n = self.seg_cands().len();
        let start = self.seg_first.min(n);
        start..(start + SEG_PAGE).min(n)
    }

    /// 反白的是**畫面上**第幾個（相對索引）。捲到看不見時是 `None`。
    ///
    /// 繪製那層只認得自己拿到的那幾個候選，所以要換算過再交出去
    /// ——跟選字的 `cand_index_in_view` 同一個道理。
    pub fn seg_cand_in_view(&self) -> Option<usize> {
        let view = self.seg_visible_range();
        view.contains(&self.seg_cand)
            .then(|| self.seg_cand - view.start)
    }

    /// 按數字鍵 `n`（0 起算）選的是哪一個候選（絕對索引）。
    ///
    /// **數字鍵對應的是畫面上的位置**，捲動之後跟絕對索引對不起來。
    pub fn seg_number_index(&self, n: usize) -> Option<usize> {
        let view = self.seg_visible_range();
        let i = view.start + n;
        (i < view.end).then_some(i)
    }

    /// 反白往右一段。到底停住——**不繞回**。
    ///
    /// 選字的反白框也是到底停住（`select_right`）。繞回去在「一直排」
    /// 的東西上很怪：使用者以為自己往右走，反白卻跳到最左邊。
    pub fn seg_right(&mut self) {
        let n = self.seg_count();
        if n > 0 && self.seg_idx + 1 < n {
            self.seg_idx += 1;
            // **換段要從頭看**——沿用上一段的捲動位置會讓新的一段
            // 開場就停在中間
            self.seg_first = 0;
            // 推出來的額外候選只屬於上一段
            self.seg_extra.clear();
            self.set_seg_cand(self.seg_current_index());
        }
    }

    /// 反白往左一段。到底停住。
    ///
    /// **定案過的段也走得回去**——使用者改變心意是他的自由，不該因為
    /// 「已經選過」就鎖住。走回去重選會讓那一段以後全部解凍重算
    /// （見 `seg_confirm`），代價是後面的選擇要重做，但那正是他要的。
    pub fn seg_left(&mut self) {
        if self.seg_idx > 0 {
            self.seg_idx -= 1;
            self.seg_first = 0;
            // 推出來的額外候選只屬於上一段
            self.seg_extra.clear();
            self.set_seg_cand(self.seg_current_index());
        }
    }

    /// 這一段現在的樣子排在候選清單的第幾個。
    ///
    /// 開選單或換段時反白要落在「現況」上——使用者才看得出來自己
    /// 從哪裡出發。找不到就落在第一個。
    fn seg_current_index(&self) -> usize {
        let segs = self.seg_segments();
        let Some(cur) = segs.get(self.seg_idx) else {
            return 0;
        };
        self.seg_cands()
            .iter()
            .position(|c| c.keys == cur.keys && c.lang == cur.lang)
            .unwrap_or(0)
    }

    /// 反白那一段的候選清單。
    ///
    /// # 排序
    ///
    /// 1. **現在這個樣子排第一**——使用者的出發點
    /// 2. 同樣長度、換語言（「解釋不同」）
    /// 3. 不同長度（「切錯了」）：先長後短，長的更常見
    ///    （`log`→`logger` 這種詞尾被切掉的情況佔多數）
    pub fn seg_cands(&self) -> Vec<SegCand> {
        let segs = self.seg_segments();
        let Some(cur) = segs.get(self.seg_idx) else {
            return Vec::new();
        };
        // 這一段從整串按鍵的哪裡開始
        let start: usize = segs[..self.seg_idx]
            .iter()
            .map(|s| s.keys.chars().count())
            .sum();
        let all: Vec<char> = self.seg_all_keys().chars().collect();
        if start >= all.len() {
            return Vec::new();
        }
        let cur_len = cur.keys.chars().count();

        let mut out: Vec<SegCand> = Vec::new();

        // ── 鎖定語言：只列現況＋台語 ──
        //
        // **切法的候選在這裡全是雜訊**（使用者裁定 2026-09-08）：
        //
        // - 鎖定注音時**切法只有一種**（整串就是一段），「切短一點」
        //   那些要改長度該用倒退鍵，不是在選單裡挑
        // - 換語言更不用說——鎖定注音的人不會想要 `ゔ` 或整串英文
        //
        // 實測「謝謝不客氣」原本列 12 個，只有 2 個是台語，其餘 10 個
        // 都沒有意義。
        if self.lock.is_some() {
            // ── 台語：反白一個詞，列出它的所有說法 ──
            //
            // 舊版是「從每個音節邊界往後試 1~24 個按鍵，對得上就列」
            // ——按鍵有歧義，`ㄈㄚㄒㄧㄝˋ` 同時是「發謝」與「發洩」，
            // 「沙發謝謝」就冒出「發洩／出氣／消解」六個誤配，那兩個字
            // 在畫面上根本沒相鄰過。現在拿組字區已經選好的**國字**查
            // （歧義在選字階段解掉了），反白單位是**詞**不是整段。
            //
            // **華語跟台語不分主從**：詞表是雙向的，這一組就是「同一個
            // 意思的幾種說法」，第一個是華語。使用者在裡面來回選，跟
            // 選字換同音字是同一件事。
            let says = if self.tw_mode() {
                self.tw_at_cursor()
            } else {
                Vec::new()
            };
            if !says.is_empty() {
                let keys = self.tw_cursor_keys();
                let zh = says.first().cloned().unwrap_or_default();
                for w in says {
                    out.push(SegCand {
                        // 華語那筆不算台語——它不進 `seg_taigi`，
                        // 定案時走一般的重算（見 `seg_confirm_with`）
                        taigi: w != zh,
                        keys: keys.clone(),
                        lang: Language::Bopomofo,
                        text: w,
                    });
                }
            } else {
                // 沒有台語可選（沒裝包、鎖定日文英文、或這個詞查不到）
                // ——只列現況
                out.push(SegCand {
                    keys: cur.keys.clone(),
                    lang: cur.lang,
                    text: self.seg_preview(&cur.keys, cur.lang),
                    taigi: false,
                });
            }
            // 使用者推邊界推出來的也算（`Shift+←→`）
            for e in &self.seg_extra {
                if !out.iter().any(|c| c.keys == e.keys && c.lang == e.lang) {
                    out.push(e.clone());
                }
            }
            return out;
        }

        // 窮舉「從 start 起、長度 1..=MAX_SEG_LEN」的每一種切法，
        // 問三個引擎收不收
        for len in 1..=MAX_SEG_LEN.min(all.len() - start) {
            let keys: String = all[start..start + len].iter().collect();
            for lang in [Language::Bopomofo, Language::Romaji, Language::English] {
                if !self.engines.enabled(lang) || !seg_valid(&keys, lang) {
                    continue;
                }
                // **英文是 passthrough，什麼都收**——不加限制的話每個
                // 長度都會多一個英文候選，清單被灌爆。
                //
                // 但判準**不能是「在不在詞典裡」**：`logger`／`logout`／
                // `footer` 這種技術詞不在 en_50k，而它們正是使用者最需要
                // 段選單的場合（CLAUDE.md 記的 `en_vowel` 弱項）。用詞典
                // 當門檻等於把段選單最該解決的問題排除在外。
                //
                // 判準改成**看起來像英文**：字母為主，可以夾標點
                // （`a@b.com`、`hello,`、`don't` 都是使用者真的會打的），
                // 但不能含數字——主鍵盤的數字全是注音聲調鍵，含數字的
                // 段當英文列出來只會是雜訊。
                if lang == Language::English && len != cur_len && !looks_english(&keys) {
                    continue;
                }
                let text = self.seg_preview(&keys, lang);
                out.push(SegCand {
                    keys: keys.clone(),
                    lang,
                    text,
                    taigi: false,
                });
                // **同一個長度的不同語言都要列**——那正是「這一段要
                // 當成什麼」這個問題的核心。`ul4` 可以是注音的「要」
                // 也可以是英文，兩個都得看得到。
            }
        }
        // **使用者用 `Shift+←→` 推出來的也算數**——那些繞過了上面的
        // 過濾（他明確表達了意圖），但仍然通過合法性判斷
        for e in &self.seg_extra {
            if !out.iter().any(|c| c.keys == e.keys && c.lang == e.lang) {
                out.push(e.clone());
            }
        }
        // 現況一定要在清單裡——引擎可能不收自己切出來的段（英文
        // passthrough 的情況），但使用者總得看得到自己的出發點
        if !out.iter().any(|c| c.keys == cur.keys && c.lang == cur.lang) {
            out.insert(
                0,
                SegCand {
                    keys: cur.keys.clone(),
                    lang: cur.lang,
                    text: self.seg_preview(&cur.keys, cur.lang),
                    taigi: false,
                },
            );
        }
        out.sort_by_key(|c| {
            // **台語那筆不算「現況」**——它的 `keys` 與 `lang` 跟華語
            // 那筆一模一樣，不排除的話會跟現況搶第一名。
            let is_cur = !c.taigi && c.keys == cur.keys && c.lang == cur.lang;
            let len = c.keys.chars().count();
            (
                // 現況第一
                !is_cur,
                // **台語緊接在現況之後**——它是「另一種說法」，不是
                // 「另一種切法」。
                //
                // 排在 `len != cur_len` 後面的話會被歸進「切法不同」那組：
                // 台語詞的按鍵是詞的長度（`vu,4vu,4`），而現況可能是整串
                // （`vu,4vu,41j6dk4fu4`），於是台語掉到所有較短切法後面
                // ——實測「謝謝不客氣」的台語排第 11、12，**擠出第一頁**。
                !c.taigi,
                // 同長度的排前面（換語言）——那是「這一段要當成什麼」，
                // 比「切法不同」更接近使用者的問題
                len != cur_len,
                // 長的排前面（詞尾被切掉是最常見的錯）
                std::cmp::Reverse(len),
            )
        });
        out
    }

    /// 這一段按鍵當成 `lang` 會顯示成什麼。
    fn seg_preview(&self, keys: &str, lang: Language) -> String {
        let seg = Segment {
            keys: keys.to_string(),
            is_mark: keys.trim().is_empty(),
            lang,
        };
        let slots = compose::compose_all(&[seg], self.width, self.jp_bounds.as_ref(), self.lock());
        compose::text_of(&slots)
    }

    /// 段選單：候選往下一個。到底繞回。
    pub fn seg_next_cand(&mut self) {
        let n = self.seg_cands().len();
        if n > 0 {
            self.set_seg_cand((self.seg_cand + 1) % n);
        }
    }

    /// 段選單：候選往上一個。到頂繞回。
    pub fn seg_prev_cand(&mut self) {
        let n = self.seg_cands().len();
        if n > 0 {
            self.set_seg_cand((self.seg_cand + n - 1) % n);
        }
    }

    /// 直接挑第 `i` 個候選（數字鍵、滑鼠）。超出範圍當沒發生。
    pub fn seg_set_cand(&mut self, i: usize) {
        if i < self.seg_cands().len() {
            self.set_seg_cand(i);
        }
    }

    /// **邊界往右推一格**（`Shift+→`）：換成下一個更長的候選。
    ///
    /// 跟從清單裡挑是同一件事，只是跳過清單——給知道自己要什麼的人用。
    /// 手勢跟選字的日文詞界調整（`widen_word`）一致。
    pub fn seg_widen(&mut self) {
        self.seg_resize(true);
    }

    /// **邊界往左收一格**（`Shift+←`）。
    pub fn seg_narrow(&mut self) {
        self.seg_resize(false);
    }

    /// 換成長度比現在多／少一格的候選。沒有就不動。
    fn seg_resize(&mut self, longer: bool) {
        let cands = self.seg_cands();
        let Some(now) = cands.get(self.seg_cand) else {
            return;
        };
        let now_len = now.keys.chars().count();
        let now_lang = now.lang;

        // 先在清單裡找同方向最接近的長度
        let next = cands
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                let l = c.keys.chars().count();
                if longer {
                    l > now_len
                } else {
                    l < now_len
                }
            })
            .min_by_key(|(_, c)| {
                let l = c.keys.chars().count();
                if longer {
                    l - now_len
                } else {
                    now_len - l
                }
            });
        if let Some((i, _)) = next {
            self.set_seg_cand(i);
            return;
        }

        // **清單裡沒有就自己算一個**。
        //
        // 候選清單為了不被灌爆有過濾（`looks_english` 排除含數字的段），
        // 但 `Shift+←→` 是**使用者明確的意圖**——他就是要「這一段再長
        // 一個字」，不必先問那看起來像不像英文。
        //
        // 這救回 `usb3`／`sha256`／`2fa`／`step3` 這類**技術詞含數字**
        // 的情況：清單只給得出 `usb`（`usb3` 含數字被濾掉），推一格
        // 就到了。
        self.seg_extend_beyond_list(longer, now_len, now_lang);
    }

    /// 推邊界到清單以外的長度：直接算一個候選插進去。
    ///
    /// 只在 `seg_resize` 找不到時走這條——**它繞過候選清單的過濾，
    /// 但不繞過合法性**：算出來的段仍然要通過該語言的引擎（英文是
    /// passthrough，永遠收）。
    fn seg_extend_beyond_list(&mut self, longer: bool, now_len: usize, lang: Language) {
        let segs = self.seg_segments();
        let start: usize = segs[..self.seg_idx.min(segs.len())]
            .iter()
            .map(|s| s.keys.chars().count())
            .sum();
        let all: Vec<char> = self.seg_all_keys().chars().collect();
        let max = all.len().saturating_sub(start).min(MAX_SEG_LEN);

        let want = if longer {
            if now_len + 1 > max {
                return;
            }
            now_len + 1
        } else {
            if now_len <= 1 {
                return;
            }
            now_len - 1
        };
        let keys: String = all[start..start + want].iter().collect();
        // **語言先照舊**：推邊界改的是範圍不是解釋。原本那個語言不收
        // 的話退回英文（passthrough，永遠收得下）。
        let lang = if seg_valid(&keys, lang) {
            lang
        } else {
            Language::English
        };
        let text = self.seg_preview(&keys, lang);
        // 推邊界推出來的是切法的調整，不是台語
        self.seg_extra.push(SegCand {
            keys,
            lang,
            text,
            taigi: false,
        });
        // 重算之後它會出現在清單裡，把反白移過去
        let target = self
            .seg_cands()
            .iter()
            .position(|c| c.keys.chars().count() == want && c.lang == lang);
        if let Some(i) = target {
            self.set_seg_cand(i);
        }
    }

    /// **選定反白的候選：那一段以前定案，後區重算。**
    ///
    /// 這是整個段選單的核心動作。做三件事：
    ///
    /// 1. 前區 = 已定案的 ＋ 使用者剛選的這一段
    /// 2. 後區 = 剩下的按鍵，交給輸入層**從頭重算**
    /// 3. 反白移到後區的第一段——使用者多半接著要改那裡
    ///
    /// 選到最後一段（沒有後區）時就只做第 1 件。
    pub fn seg_confirm(&mut self) {
        self.seg_confirm_with(true)
    }

    /// `advance` 為 `false` 時定案完就**留在原地**，不跳下一段。
    ///
    /// 對應設定 `behavior.enter_in_select`（跟選字共用一個開關，
    /// 使用者裁定）：`Next` 是新注音式的「選完往下一個」，`Exit` 是
    /// 微軟注音式的「選完就停住」。兩層的粒度不同但心智模型一樣。
    pub fn seg_confirm_with(&mut self, advance: bool) {
        let cands = self.seg_cands();
        let Some(pick) = cands.get(self.seg_cand).cloned() else {
            return;
        };
        // 反白現在框住哪裡——定案會改寫那幾格，事後就問不到了
        let span = self.tw_span();
        let segs = self.seg_segments();
        let at = self.seg_idx.min(segs.len());
        // 這一段從整串按鍵的哪裡開始
        let start: usize = segs[..at].iter().map(|s| s.keys.chars().count()).sum();

        // **定案的前區** = 反白那一段以前的 ＋ 他挑的那一段。
        //
        // 前面那些是引擎算的、使用者沒改——但他既然要改後面，前面就
        // 等於他認可了。不一起定案的話後區重算會把它們換掉。
        let mut prefix: Vec<Segment> = segs[..at].to_vec();
        // **台語詞要另外記**——`Segment` 只有 `(keys, lang)`，`compose`
        // 會拿 `keys` 去查詞庫重算，台語那個 `text` 就丟了。見
        // `Session::seg_taigi`。
        self.seg_taigi.retain(|(k, _)| *k != pick.keys);
        if pick.taigi {
            self.seg_taigi.push((pick.keys.clone(), pick.text.clone()));
        }
        prefix.push(Segment {
            keys: pick.keys.clone(),
            is_mark: pick.keys.trim().is_empty(),
            lang: pick.lang,
        });
        let n = start + pick.keys.chars().count();
        // 有幾段是使用者定案的——**只用來擋反白往左走**，不參與組裝
        self.seg_locked = prefix.len();

        // **鎖定語言時不必凍結**——切法只有一種（整串就是一段），沒有
        // 後區要重算。
        //
        // 而且**不能凍**：鎖定注音走的是 `BopomofoInput`（四格音節緩衝），
        // `freeze_user_prefix` 只支援 `Cascade`，會退回慢路重建成
        // `Cascade`——跟鎖定模式不相容，整串按鍵就沒了（實測：定案之後
        // 段變成空的、送出空字串）。
        //
        // 台語正是在這條路上（只在鎖定注音時出現），所以這個分支必要。
        if self.lock.is_some() {
            // **定案會改變斷詞結果**（「沙發」變「膨椅」之後就不是原本
            // 那個詞了），所以往下跳要用**定案前**的位置當基準——事後
            // 再問 `tw_span` 會拿到重算過的答案，反白跳回開頭。
            let was = span.map(|(a, l)| a + l);
            let pick_len = span.map(|(_, l)| l);
            // 只有一段，反白哪裡都不必動；`rebuild_slots` 會把
            // `seg_taigi` 套回去（見 `apply_taigi`）
            self.rebuild_slots();
            // 台語模式：`advance` 決定定案完往哪走，跟選字共用同一個
            // 設定（使用者裁定）。
            if self.tw_mode() {
                self.seg_tw_len = 0;
                if advance {
                    // 「選完移到下一個」：定案那個詞的**後面**第一個
                    // 有台語說法的詞
                    let end = was.unwrap_or(0);
                    let words = self.tw_words();
                    match words.iter().find(|w| w.at >= end) {
                        Some(w) => self.seg_tw_at = w.at,
                        None => {
                            // **後面沒有詞了＝走完整句**。選單該關掉，
                            // 不是繞回開頭（實測回報「最後選完沒跳出去
                            // 又回到第一個」）
                            self.seg_done_flag = true;
                            self.seg_tw_at = words.first().map(|w| w.at).unwrap_or(0);
                        }
                    }
                } else {
                    // 「選完離開選字」：**選完就退出**，跟選字那層同一個
                    // 語意（`confirm_cand_with(false)` 直接 `exit_select`）。
                    //
                    // 兩層共用一個設定，行為就要一致——只設反白位置而不
                    // 標「走完了」的話，選單永遠關不掉（實測回報「退不出去」）。
                    self.seg_done_flag = true;
                    // 反白留在剛定案的範圍——選單這一幀還要畫，別讓它
                    // 漂到別的詞去。重開 TAB 時 `seg_open` 會重設。
                    if let (Some(end), Some(len)) = (was, pick_len) {
                        self.seg_tw_at = end.saturating_sub(len);
                        self.seg_tw_len = len;
                    }
                }
                self.seg_cand = 0;
                self.seg_first = 0;
            } else {
                self.set_seg_cand(self.seg_current_index());
            }
            return;
        }

        // **快路**：把定案交給輸入層當凍結區，後區已經算好的分支不重建。
        // 成本跟後區長度無關（實測 93 鍵長句 47ms → 0.46ms）。
        if !self.input.freeze_user_prefix(prefix.clone(), n) {
            // **慢路**：使用者的切點跟引擎的凍結衝突得太厲害，只能拿
            // 剩下的按鍵重開一個引擎。那正是使用者要的——他就是不同意
            // 引擎的切法。
            let rest: String = self.input.keys().chars().skip(n).collect();
            let mut input = crate::input::Input::from_keys_with(&rest, self.lock, self.engines);
            // 新引擎只認得後區，把前區補回它的凍結區——`keys()` 與
            // `cuttings()` 才會涵蓋整串，送出的文字才不會缺開頭
            input.adopt_frozen(prefix);
            self.input = input;
        }
        self.refresh();

        // 反白移到後區第一段——使用者多半接著要改那裡。
        //
        // **最後一段定案之後沒有後區了**，那時 `seg_locked` 會指到
        // 段數之外，`seg_cands()` 回空清單、反白框畫不出來，看起來
        // 就是卡住（實測回報「TAB 選到最後不會跳離切法選擇」）。
        // 夾回最後一段，並讓 `seg_done()` 告訴 TSF 層該關掉選單了。
        let n = self.seg_count();
        // `advance == false`（微軟注音式）就留在原地——但**最後一段
        // 例外**：那時原地已經沒有東西可選，一律夾回去。
        self.seg_idx = if advance || self.seg_locked >= n {
            self.seg_locked.min(n.saturating_sub(1))
        } else {
            self.seg_idx.min(n.saturating_sub(1))
        };
        // **「選完就退出」要真的退出**。
        //
        // 只把反白留在原地是不夠的——選單照樣開著、`seg_done()` 照樣
        // false，使用者按幾次 Enter 都一樣，**永遠出不去**（實測回報
        // 「TAB 選單沒辦法 Enter 確定選法」，log 顯示動作有執行但狀態
        // 一動也不動）。
        //
        // 語意要跟選字層一致：那邊的 `confirm_cand_with(false)` 直接
        // `exit_select()`，選完就離開。台語那條路早就這樣做了
        // （見上面的 `seg_done_flag = true`），非台語這條漏了。
        if !advance {
            self.seg_done_flag = true;
        }
        self.set_seg_cand(self.seg_current_index());
    }

    /// **每一段都定案了嗎？**
    ///
    /// 到這一步使用者已經逐段確認完整句，選單沒有東西可挑了——TSF 層
    /// 該把它關掉，回到打字狀態（再按 Enter 才是送出）。
    ///
    /// 不關的話反白會停在最後一段、候選是空的，看起來像卡住。
    pub fn seg_done(&self) -> bool {
        // **台語模式算的是「詞」不是「段」**。
        //
        // 鎖定注音時整串就是一段，定案任何一個詞都會讓 `seg_locked`
        // 追上 `seg_count()`（都是 1）——選單就在第一次 Enter 之後
        // 被關掉，使用者根本沒機會往下選（實測回報「選字設定在台語
        // TAB 中沒生效」）。
        //
        // 台語走完了沒有，**在定案時就決定**（`seg_done_flag`）。
        //
        // 事後推論都不可靠：「還有沒有詞可挑」永遠是有（定案過的詞
        // 也算在斷詞裡，才回得去改），而「反白在不在最後一個」也不行
        // ——`tw_next` 走到底會繞回開頭，反白早就跑掉了（實測回報
        // 「最後選完沒跳出去又回到第一個」）。
        if self.tw_mode() {
            return self.seg_done_flag || self.tw_words().is_empty();
        }
        // 旗標優先：「選完就退出」的設定按一次 Enter 就該出去，
        // 不必等到每一段都定案。
        self.seg_done_flag || (self.seg_locked > 0 && self.seg_locked >= self.seg_count())
    }

    /// 使用者在段選單裡定案過嗎？
    pub fn seg_touched(&self) -> bool {
        self.seg_locked > 0
    }

    /// 段選單開著時，反白那一段在整串按鍵裡的範圍。
    ///
    /// 繪製要用——組字區要把那一段框起來。
    pub fn seg_key_range(&self) -> Option<(usize, usize)> {
        let segs = self.seg_segments();
        let s = segs.get(self.seg_idx)?;
        let start: usize = segs[..self.seg_idx]
            .iter()
            .map(|x| x.keys.chars().count())
            .sum();
        Some((start, start + s.keys.chars().count()))
    }

    /// 取消段選單，回到引擎自己算的分段。
    ///
    /// 前區定案全部丟掉、整串按鍵重新交給引擎——**這是唯一的後悔藥**。
    /// 逐段撤銷會讓凍結變成可撤銷的，狀態機複雜一倍而收益不明。
    pub fn seg_reset(&mut self) {
        self.seg_idx = 0;
        self.set_seg_cand(0);
        if self.seg_locked == 0 {
            return;
        }
        self.seg_locked = 0;
        // 整串按鍵重新交給引擎——凍結區一併丟掉，回到它自己的判斷
        let all = self.seg_all_keys();
        self.input = crate::input::Input::from_keys_with(&all, self.lock, self.engines);
        self.refresh();
    }

    /// 段選單開著時，反白那一段在整串按鍵裡的範圍。
    ///
    /// 繪製要用——組字區要把那一段框起來。
    pub fn seg_range(&self) -> Option<(usize, usize)> {
        let segs = self.seg_segments();
        let s = segs.get(self.seg_idx)?;
        let start: usize = segs[..self.seg_idx]
            .iter()
            .map(|x| x.keys.chars().count())
            .sum();
        Some((start, start + s.keys.chars().count()))
    }
}

/// 這一段按鍵當成 `lang` 合法嗎？
///
/// **這是段選單的守門員**——候選不限於既有切法，但也不是什麼都能選。
/// 每一段仍然要通過該語言的合法性判斷，差別只在**不受整句剪枝的
/// 牽連**（`prune`、`ALIVE_LIMIT` 那些是為了控制組合爆炸，跟這一段
/// 本身合不合法無關）。
fn seg_valid(keys: &str, lang: Language) -> bool {
    // 分隔符空白自成一段，永遠合法
    if keys.trim().is_empty() {
        return true;
    }
    match lang {
        Language::Bopomofo => crate::bopomofo::validity(keys) == crate::bopomofo::Validity::Valid,
        Language::Romaji => crate::romaji::validity(keys) == crate::romaji::Validity::Valid,
        // 英文是 passthrough——瀑布的最後一站，永遠接得住
        Language::English => true,
    }
}

impl Session {
    /// 段選單每一段顯示成什麼（給預覽列用），**含反白那一項的預覽**。
    ///
    /// 回傳 `(每一段的文字, 反白那一段是第幾個)`。
    ///
    /// # 為什麼要預覽而不是顯示現況
    ///
    /// 使用者按 ↑↓ 換解釋、按 `Shift+←→` 推邊界時，期待**立刻看到
    /// 結果**。只顯示現況的話組字區一動也不動，看起來就像那顆鍵沒作用
    /// ——實測回報「Shift 方向鍵是不是沒實作」就是這個。
    ///
    /// 所以這裡把反白那一段換成**候選清單裡選中的那一項**，後面的段
    /// 照舊（它們要等定案才會重算）。使用者看到的就是「按下 Enter 會
    /// 變成什麼樣」。
    pub fn seg_preview_texts(&self) -> (Vec<String>, usize) {
        // ── 台語模式：反白單位是**詞**，不是段 ──
        //
        // 鎖定注音時整串是一段，走下面那條路的話反白會蓋住整句。這裡
        // 一格一個字地畫，把反白那幾格換成台語說法。
        if self.tw_mode() {
            if let Some((at, len)) = self.tw_cursor_range() {
                let slots = self.slots();
                if at + len <= slots.len() {
                    let mut out: Vec<String> = Vec::new();
                    // 反白前面的字，逐格照舊
                    for s in &slots[..at] {
                        out.push(s.text.clone());
                    }
                    let mark = out.len();
                    // 反白那幾格：換成選中的台語說法，沒選就顯示原文
                    let cands = self.seg_cands();
                    match cands.get(self.seg_cand).filter(|c| c.taigi) {
                        Some(c) => out.push(c.text.clone()),
                        None => out.push(
                            slots[at..at + len]
                                .iter()
                                .map(|s| s.text.as_str())
                                .collect(),
                        ),
                    }
                    for s in &slots[at + len..] {
                        out.push(s.text.clone());
                    }
                    return (out, mark);
                }
            }
        }

        let segs = self.seg_segments();
        let cands = self.seg_cands();
        let pick = cands.get(self.seg_cand);
        let mut out: Vec<String> = Vec::new();
        let mut at = 0usize;
        for (i, s) in segs.iter().enumerate() {
            if i == self.seg_idx {
                at = out.len();
                match pick {
                    // 反白那一段用候選的預覽
                    Some(c) => {
                        out.push(c.text.clone());
                        // **候選可能比現況長**，多吃掉的按鍵要從後面的段
                        // 扣掉——否則預覽會重複顯示那幾個字
                        let mut eaten = c.keys.chars().count();
                        let mut skip = 0usize;
                        for later in &segs[i..] {
                            let n = later.keys.chars().count();
                            if eaten < n {
                                break;
                            }
                            eaten -= n;
                            skip += 1;
                        }
                        // 被吃掉的段不畫；剩下半截的那一段畫尾巴
                        let rest: Vec<&Segment> = segs[i + skip..].iter().collect();
                        for (k, later) in rest.iter().enumerate() {
                            let keys: String = if k == 0 && eaten > 0 {
                                later.keys.chars().skip(eaten).collect()
                            } else {
                                later.keys.clone()
                            };
                            if !keys.is_empty() {
                                out.push(self.seg_preview(&keys, later.lang));
                            }
                        }
                        return (out, at);
                    }
                    None => out.push(self.seg_preview(&s.keys, s.lang)),
                }
            } else {
                out.push(self.seg_preview(&s.keys, s.lang));
            }
        }
        (out, at)
    }

    /// 段選單每一段顯示成什麼（不含預覽，內部與測試用）。
    pub fn seg_texts(&self) -> Vec<String> {
        self.seg_segments()
            .iter()
            .map(|s| self.seg_preview(&s.keys, s.lang))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 測試要有詞庫才有意義——沒載的話合法性引擎什麼都不認得。
    /// 跟 `input.rs` 的測試同一個做法。
    fn load() -> bool {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data");
        crate::preload(&data, crate::config::Engines::default());
        crate::dict::bopomofo_loaded()
    }

    fn sess(keys: &str) -> Session {
        let mut s = Session::new();
        for c in keys.chars() {
            s.push(c);
        }
        s
    }

    #[test]
    fn 段選單一開始反白第一段() {
        let s = sess("su3cl3");
        assert_eq!(s.seg_index(), 0);
        assert!(!s.seg_segments().is_empty());
    }

    /// **「選完就退出」要真的退出**（實測回報 2026-09-09：
    /// 「TAB 選單沒辦法 Enter 確定選法」）。
    ///
    /// 病灶是 `advance == false` 只做了「反白留在原地」，沒有標記走完
    /// ——選單照樣開著、`seg_done()` 照樣 false，按幾次 Enter 都一樣。
    /// log 顯示動作**有執行**但狀態一動也不動，看起來就像「關掉之後又
    /// 立刻打開」。
    ///
    /// 台語那條路早就標了旗標，非台語這條漏了——共用同一個旗標之後
    /// 就不會再有一邊修了一邊沒修的情況。
    #[test]
    fn 選完就退出的設定按一次enter就該走完() {
        if !load() {
            return;
        }
        let mut s = sess("su3cl3hello");
        assert!(!s.seg_done(), "一開始不該是走完的");
        // advance = false 就是「選完就退出」（EnterInSelect::Exit）
        s.seg_confirm_with(false);
        assert!(s.seg_done(), "選完就退出：按一次 Enter 就該關掉選單");
    }

    /// 相對的，「選完往下一段」不該提早結束——還有段可挑就繼續。
    #[test]
    fn 選完往下一段的設定不會提早走完() {
        if !load() {
            return;
        }
        let mut s = sess("su3cl3hello");
        if s.seg_count() < 2 {
            return; // 只有一段的話「往下」本來就等於走完
        }
        s.seg_confirm_with(true);
        assert!(!s.seg_done(), "還有段可挑就不該關掉選單");
    }

    /// 退出之後重開 TAB 要能再選——旗標沒清的話選單開不起來。
    #[test]
    fn 退出之後重開tab還能再選() {
        if !load() {
            return;
        }
        let mut s = sess("su3cl3hello");
        s.seg_confirm_with(false);
        assert!(s.seg_done(), "先確認真的走完了");
        s.seg_open();
        assert!(!s.seg_done(), "重開 TAB 要能再選，不能一開就被判定走完");
    }

    #[test]
    fn 反白右移到底停住不繞回() {
        let mut s = sess("su3cl3hello");
        let n = s.seg_count();
        for _ in 0..n + 5 {
            s.seg_right();
        }
        assert_eq!(s.seg_index(), n - 1, "到底要停住，不能繞回第一段");
    }

    /// **反白走得回定案過的段**（使用者裁定：不要限制使用者行為）。
    ///
    /// 早期版本擋著不讓走回去，理由是「凍結變成可撤銷會讓狀態機複雜
    /// 一倍」。實際接上去之後發現**根本不必特別處理**——`seg_confirm`
    /// 本來就是「重建整個前區」，選比較前面的段自然會把後面解凍。
    #[test]
    fn 反白走得回定案過的段() {
        if !load() {
            return;
        }
        let mut s = sess("loggerul4");
        s.seg_confirm();
        assert!(s.seg_locked > 0, "選定之後前區該有東西");
        // 一路往左走，該回得到第一段
        for _ in 0..10 {
            s.seg_left();
        }
        assert_eq!(s.seg_index(), 0, "該回得到第一段");
    }

    #[test]
    fn 選定之後前區定案後區重算() {
        if !load() {
            return;
        }
        let mut s = sess("su3cl3hello");
        let before = s.seg_segments().len();
        s.seg_confirm();
        assert!(s.seg_locked > 0, "選定之後要有前區");
        // 前區＋後區加起來的按鍵要跟原本一樣長——重算不能弄丟按鍵
        let total: usize = s
            .seg_segments()
            .iter()
            .map(|x| x.keys.chars().count())
            .sum();
        assert_eq!(total, "su3cl3hello".chars().count(), "按鍵不能弄丟");
        assert!(before > 0);
    }

    #[test]
    fn 候選裡現況排第一() {
        if !load() {
            return;
        }
        let s = sess("su3cl3");
        let segs = s.seg_segments();
        let cands = s.seg_cands();
        assert!(!cands.is_empty());
        assert_eq!(
            (cands[0].keys.as_str(), cands[0].lang),
            (segs[0].keys.as_str(), segs[0].lang),
            "第一項要是使用者的出發點"
        );
    }

    #[test]
    fn 候選每一段都合法() {
        if !load() {
            return;
        }
        for keys in ["su3cl3", "loggerul4", "sushi", "hello"] {
            let s = sess(keys);
            for c in s.seg_cands() {
                assert!(
                    seg_valid(&c.keys, c.lang),
                    "{keys}：候選 {:?}({:?}) 不合法",
                    c.keys,
                    c.lang
                );
            }
        }
    }

    #[test]
    fn 推邊界會換到不同長度() {
        if !load() {
            return;
        }
        let mut s = sess("loggerul4");
        let before = s.seg_cands()[s.seg_cand_index()].keys.chars().count();
        s.seg_widen();
        let after = s.seg_cands()[s.seg_cand_index()].keys.chars().count();
        // 有更長的就該變長；沒有就維持（不能亂跳）
        assert!(after >= before, "推右邊界只能變長或不動");
    }

    #[test]
    fn 取消之後回到引擎原本的分段() {
        if !load() {
            return;
        }
        let keys = "su3cl3hello";
        let original = sess(keys).seg_segments();
        let mut s = sess(keys);
        s.seg_confirm();
        s.seg_right();
        s.seg_confirm();
        s.seg_reset();
        assert_eq!(s.seg_locked, 0, "取消要把前區清空");
        assert_eq!(s.seg_index(), 0, "取消要回到第一段");
        assert_eq!(
            s.seg_segments().len(),
            original.len(),
            "取消之後分段要跟一開始一樣"
        );
    }

    /// **選了就凍要能組出引擎算不出來的東西**——這是整個段選單存在的
    /// 理由。`loggerul4` 的正解是 `英:logger｜注:ul4`，而引擎第一名
    /// 把 `logger` 切開了。
    #[test]
    fn 能組出引擎第一名以外的分段() {
        if !load() {
            return;
        }
        let mut s = sess("loggerul4");
        let first = s.seg_segments()[0].keys.clone();
        // 找「整個 logger 是一段」那個候選
        let cands = s.seg_cands();
        let Some(i) = cands.iter().position(|c| c.keys == "logger") else {
            // 引擎連這個候選都給不出來的話這個測試沒意義，但那本身
            // 就是個問題——留個訊息
            panic!("段選單該列得出 `logger` 這個候選，實際有：{cands:?}");
        };
        s.seg_set_cand(i);
        s.seg_confirm();
        assert_eq!(s.seg_segments()[0].keys, "logger", "選了就要定案");
        assert_ne!(first, "logger", "前提：引擎第一名本來沒把 logger 當一段");
    }
}

/// 這串按鍵看起來像英文段嗎？（段選單列不列它當英文候選）
///
/// **不是合法性判斷**——英文是 passthrough，什麼都收。這是**要不要
/// 列出來**的判斷：不設限的話每個長度都會多一個英文候選，清單被灌爆。
///
/// 判準：
/// - **至少要有一個字母**（`,,,` 不是英文段）
/// - **不能含數字**——主鍵盤的數字全是注音聲調鍵，含數字的段列成英文
///   多半是雜訊（`ul4` 是「要」不是英文）
/// - **標點可以夾在裡面**：`a@b.com`、`hello,`、`don't`、`test.meeting`
///   都是使用者真的會打的東西。實測 `long` 節有 56 句敗在這裡——
///   純字母的判準把含標點的英文段全擋掉了
fn looks_english(keys: &str) -> bool {
    let mut has_alpha = false;
    for c in keys.chars() {
        if c.is_ascii_digit() {
            return false;
        }
        if c.is_ascii_alphabetic() {
            has_alpha = true;
        }
    }
    has_alpha
}

#[cfg(test)]
mod widen_tests {
    use super::*;

    fn load() -> bool {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data");
        crate::preload(&data, crate::config::Engines::default());
        crate::dict::bopomofo_loaded()
    }

    fn sess(keys: &str) -> Session {
        let mut s = Session::new();
        for c in keys.chars() {
            s.push(c);
        }
        s
    }

    /// **Shift+→ 要真的把這一段變長**（實測回報「好像沒實作」）。
    #[test]
    fn 推右邊界真的變長() {
        if !load() {
            return;
        }
        let mut s = sess("loggerul4");
        let before = s.seg_cands()[s.seg_cand_index()].keys.clone();
        s.seg_widen();
        let after = s.seg_cands()[s.seg_cand_index()].keys.clone();
        assert!(
            after.chars().count() > before.chars().count(),
            "Shift+→ 該讓 {before:?} 變長，實際變成 {after:?}"
        );
    }

    /// Shift+← 要變短。
    #[test]
    fn 推左邊界真的變短() {
        if !load() {
            return;
        }
        let mut s = sess("loggerul4");
        // 先推長兩次再收回來
        s.seg_widen();
        s.seg_widen();
        let before = s.seg_cands()[s.seg_cand_index()].keys.clone();
        s.seg_narrow();
        let after = s.seg_cands()[s.seg_cand_index()].keys.clone();
        assert!(
            after.chars().count() < before.chars().count(),
            "Shift+← 該讓 {before:?} 變短，實際變成 {after:?}"
        );
    }
}

#[cfg(test)]
mod scroll_tests {
    use super::*;

    fn load() -> bool {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data");
        crate::preload(&data, crate::config::Engines::default());
        crate::dict::bopomofo_loaded()
    }

    fn sess(keys: &str) -> Session {
        let mut s = Session::new();
        for c in keys.chars() {
            s.push(c);
        }
        s
    }

    /// **反白往下走絕不能消失在畫面外**（實測回報「反白條不會動」）。
    ///
    /// 一段常有二三十個候選，而畫面一次只列 `SEG_PAGE` 個。沒有捲動的
    /// 話走到第 10 個就看不見了。
    #[test]
    fn 反白往下走一直看得見() {
        if !load() {
            return;
        }
        let mut s = sess("loggerul4");
        let n = s.seg_cands().len();
        assert!(n > SEG_PAGE, "這個測試要候選多於一頁才有意義（實際 {n}）");
        for step in 0..n {
            assert!(
                s.seg_cand_in_view().is_some(),
                "走到第 {} 個時反白跑出畫面了",
                step + 1
            );
            s.seg_next_cand();
        }
    }

    /// 往上走繞回最後一個時也要看得見。
    #[test]
    fn 往上繞回也看得見() {
        if !load() {
            return;
        }
        let mut s = sess("loggerul4");
        s.seg_prev_cand(); // 從第一個往上 → 繞到最後一個
        assert!(
            s.seg_cand_in_view().is_some(),
            "繞回最後一個時反白該在畫面內"
        );
    }

    /// 數字鍵挑的是**畫面上**那一個，不是絕對索引。
    #[test]
    fn 數字鍵對應畫面位置() {
        if !load() {
            return;
        }
        let mut s = sess("loggerul4");
        // 捲到後面去
        // 走到最後一個（不繞回）——那一定會捲動
        let n = s.seg_cands().len();
        s.seg_set_cand(n - 1);
        let view = s.seg_visible_range();
        assert!(view.start > 0, "該捲動了（實際 {view:?}）");
        // 按「1」要選畫面上第一個，也就是 `view.start`
        assert_eq!(s.seg_number_index(0), Some(view.start));
        // 超出畫面的數字不該選到東西
        assert_eq!(s.seg_number_index(SEG_PAGE), None);
    }

    /// 換段時捲動要歸零——新的一段從頭看。
    #[test]
    fn 換段之後從頭看() {
        if !load() {
            return;
        }
        // 要候選夠多（才捲得動）而且有第二段可以走過去
        let mut s = sess("loggerul4 hello");
        let n = s.seg_cands().len();
        assert!(n > SEG_PAGE, "這個測試要候選多於一頁（實際 {n}）");
        assert!(s.seg_count() > 1, "要有第二段可以走過去");
        s.seg_set_cand(n - 1);
        assert!(s.seg_visible_range().start > 0, "先捲動");
        s.seg_right();
        assert_eq!(s.seg_visible_range().start, 0, "換段之後該回到清單開頭");
    }
}

#[cfg(test)]
mod extend_tests {
    use super::*;

    fn load() -> bool {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data");
        crate::preload(&data, crate::config::Engines::default());
        crate::dict::bopomofo_loaded()
    }

    fn sess(keys: &str) -> Session {
        let mut s = Session::new();
        for c in keys.chars() {
            s.push(c);
        }
        s
    }

    /// **含數字的技術詞要推得到**（`usb3`、`sha256`、`2fa`、`step3`）。
    ///
    /// 候選清單為了不被灌爆排除含數字的英文段，但 `Shift+→` 是使用者
    /// 明確的意圖，該繞過那個過濾。實測那 9 句 `number` 節就是敗在這。
    #[test]
    fn 推邊界救得回含數字的英文詞() {
        if !load() {
            return;
        }
        for (keys, want) in [("usb3", "usb3"), ("step3", "step3"), ("sha256", "sha256")] {
            let mut s = sess(keys);
            // 清單裡本來沒有（含數字被濾掉）
            let before = s.seg_cands().iter().any(|c| c.keys == want);
            // 一路往右推到底
            for _ in 0..want.chars().count() {
                s.seg_widen();
            }
            let now = &s.seg_cands()[s.seg_cand_index()];
            assert_eq!(
                now.keys, want,
                "{keys}：推邊界該到得了 {want:?}（清單原本有嗎：{before}）"
            );
        }
    }

    /// 推出來的候選只屬於當下那一段——換段要清掉。
    #[test]
    fn 換段之後推出來的候選要清掉() {
        if !load() {
            return;
        }
        let mut s = sess("usb3 hello");
        s.seg_widen();
        let n_before = s.seg_cands().len();
        s.seg_right();
        s.seg_left();
        assert!(
            s.seg_cands().len() <= n_before,
            "換段來回之後不該累積額外候選"
        );
    }
}

#[cfg(test)]
mod done_tests {
    use super::*;

    fn load() -> bool {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data");
        crate::preload(&data, crate::config::Engines::default());
        crate::dict::bopomofo_loaded()
    }

    fn sess(keys: &str) -> Session {
        let mut s = Session::new();
        for c in keys.chars() {
            s.push(c);
        }
        s
    }

    /// **選到最後一段之後不能卡住**（實測回報）。
    ///
    /// 原本 `seg_confirm` 無條件把反白移到 `seg_locked`，最後一段定案
    /// 之後那個位置在段數之外——候選 0 個、反白框畫不出來。
    #[test]
    fn 選到最後不會卡住() {
        if !load() {
            return;
        }
        let mut s = sess("su3cl3hello");
        let n = s.seg_count();
        for _ in 0..n {
            s.seg_confirm();
        }
        assert!(s.seg_done(), "每一段都定案了，`seg_done` 該回 true");
        assert!(
            s.seg_index() < s.seg_count(),
            "反白不能跑到段數之外（實際 {} / 共 {} 段）",
            s.seg_index(),
            s.seg_count()
        );
        // 再按也不該壞掉
        s.seg_confirm();
        assert!(s.seg_index() < s.seg_count(), "多按幾次也不能跑出去");
    }

    /// 還沒選完時 `seg_done` 要是 false——不然選單會提早關掉。
    #[test]
    fn 還沒選完不算完成() {
        if !load() {
            return;
        }
        let mut s = sess("su3cl3hello");
        assert!(!s.seg_done(), "一段都還沒選");
        s.seg_confirm();
        assert!(!s.seg_done(), "只選了第一段，後面還有");
    }
}

#[cfg(test)]
mod learn_tests {
    use super::*;

    fn load() -> bool {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data");
        crate::preload(&data, crate::config::Engines::default());
        crate::dict::bopomofo_loaded()
    }

    fn sess(keys: &str) -> Session {
        let mut s = Session::new();
        for c in keys.chars() {
            s.push(c);
        }
        s
    }

    /// **段選單定案過的要記進切詞學習層**——不然同一串按鍵每次都要
    /// 重選一遍。
    #[test]
    fn 定案過的分段會被學起來() {
        if !load() {
            return;
        }
        let mut s = sess("loggerul4");
        // 挑「整個 logger 是一段」
        let cands = s.seg_cands();
        let Some(i) = cands.iter().position(|c| c.keys == "logger") else {
            panic!("該列得出 logger");
        };
        s.seg_set_cand(i);
        s.seg_confirm();
        assert!(s.learn_on_commit() > 0, "定案過就該記下東西");
    }

    /// **沒動過段選單就不記**——照引擎給的送出，記了只是強化現狀。
    #[test]
    fn 沒動過選單不記切詞() {
        if !load() {
            return;
        }
        let s = sess("su3cl3");
        // `record`（詞層）可能記東西，但切詞那條該是 0
        assert_eq!(s.learn_seg_choice(), 0, "沒定案過任何段就不該記切詞");
    }

    /// **記的是切詞不是選字**。
    ///
    /// 兩層共用同一個 `Index`，靠 key 的前綴分（`語:`／`切:`，見
    /// `learn::LANG_PREFIX`）。段選單記的一律要帶前綴——不帶前綴的
    /// 是詞層（「這個注音選哪個字」），那是另一回事。
    #[test]
    fn 切詞跟詞層分開() {
        if !load() {
            return;
        }
        crate::learn::clear();

        let mut s = sess("loggerul4");
        let cands = s.seg_cands();
        if let Some(i) = cands.iter().position(|c| c.keys == "logger") {
            s.seg_set_cand(i);
            s.seg_confirm();
        }
        let mut s = sess("loggerul4");
        let cands = s.seg_cands();
        if let Some(i) = cands.iter().position(|c| c.keys == "logger") {
            s.seg_set_cand(i);
            s.seg_confirm();
        }
        assert!(s.learn_seg_choice() > 0, "該記下東西");

        // **詞層一條都不該有**：`Index::best` 查的是不帶前綴的 key，
        // 而段選單記的一律帶前綴（`record_cutting` 的 `LANG_PREFIX`／
        // `CUT_PREFIX`）。使用者說「logger 是一個英文段」是**切法**的
        // 知識，不該影響「這個注音該選哪個字」。
        assert_eq!(
            crate::learn::index().best("loggerul4"),
            None,
            "段選單不該動到詞層"
        );
        assert_eq!(
            crate::learn::index().best("ul4"),
            None,
            "更不該影響單一注音段選什麼字"
        );
    }
}

#[cfg(test)]
mod revisit_tests {
    use super::*;

    fn load() -> bool {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data");
        crate::preload(&data, crate::config::Engines::default());
        crate::dict::bopomofo_loaded()
    }

    fn sess(keys: &str) -> Session {
        let mut s = Session::new();
        for c in keys.chars() {
            s.push(c);
        }
        s
    }

    /// **定案過的段要能走回去重選**——使用者改變心意是他的自由
    /// （實測回報「選過就不能從選這要拔掉，不要限制使用者行為」）。
    #[test]
    fn 定案過的段走得回去() {
        if !load() {
            return;
        }
        let mut s = sess("su3cl3hello");
        s.seg_confirm(); // 定案第一段
        assert!(s.seg_index() > 0, "定案之後反白該往後移");
        let before = s.seg_index();
        s.seg_left();
        assert_eq!(s.seg_index(), before - 1, "該走得回去");
        s.seg_left();
        // 一路走到第一段
        while s.seg_index() > 0 {
            s.seg_left();
        }
        assert_eq!(s.seg_index(), 0, "該回得到第一段");
    }

    /// 走回去重選之後，**那一段以後要跟著重算**。
    #[test]
    fn 回頭改會讓後面重算() {
        if !load() {
            return;
        }
        let mut s = sess("loggerul4");
        // 先定案「log」（引擎的預設）
        s.seg_confirm();
        let after_first = s.text();
        // 走回第一段，改選整個 logger
        while s.seg_index() > 0 {
            s.seg_left();
        }
        let cands = s.seg_cands();
        let Some(i) = cands.iter().position(|c| c.keys == "logger") else {
            panic!("該列得出 logger，實際 {cands:?}");
        };
        s.seg_set_cand(i);
        s.seg_confirm();
        assert_ne!(s.text(), after_first, "改了之後輸出該不一樣");
        assert!(
            s.seg_segments()[0].keys == "logger",
            "第一段該變成 logger，實際 {:?}",
            s.seg_segments()[0].keys
        );
    }
}

#[cfg(test)]
mod advance_tests {
    use super::*;

    fn load() -> bool {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data");
        crate::preload(&data, crate::config::Engines::default());
        crate::dict::bopomofo_loaded()
    }

    fn sess(keys: &str) -> Session {
        let mut s = Session::new();
        for c in keys.chars() {
            s.push(c);
        }
        s
    }

    /// **跟選字共用同一個開關**：`Next`（新注音式）定案完跳下一段。
    #[test]
    fn advance_跳下一段() {
        if !load() {
            return;
        }
        let mut s = sess("su3cl3hello");
        assert_eq!(s.seg_index(), 0);
        s.seg_confirm_with(true);
        assert_eq!(s.seg_index(), 1, "該跳到下一段");
    }

    /// `Exit`（微軟注音式）定案完留在原地。
    #[test]
    fn 不_advance_留在原地() {
        if !load() {
            return;
        }
        let mut s = sess("su3cl3hello");
        assert_eq!(s.seg_index(), 0);
        s.seg_confirm_with(false);
        assert_eq!(s.seg_index(), 0, "該留在原地");
    }

    /// **最後一段是例外**：不 advance 也要夾回去，否則反白會停在
    /// 段數之外（候選 0 個、框畫不出來）。
    #[test]
    fn 最後一段不_advance_也不會跑出去() {
        if !load() {
            return;
        }
        let mut s = sess("su3cl3hello");
        let n = s.seg_count();
        for _ in 0..n {
            s.seg_confirm_with(false);
            // 不 advance 會留在第一段，所以要自己走過去
            s.seg_right();
        }
        // 再定案幾次都不該跑出去
        for _ in 0..3 {
            s.seg_confirm_with(false);
            assert!(
                s.seg_index() < s.seg_count(),
                "反白不能跑到段數之外（{} / {} 段）",
                s.seg_index(),
                s.seg_count()
            );
        }
    }
}

#[cfg(test)]
mod taigi_tests {
    use super::*;

    /// 寫一份臨時的台語包並載入。
    ///
    /// **不用產品的包**——測試要能獨立跑，而且不受使用者裝了什麼影響。
    ///
    /// **所有台語測試共用這一份**：`pack::load` 寫的是全域狀態，兩份
    /// 不同內容的測試包會互相覆蓋。實測分成兩份時單獨跑都過、一起跑
    /// 掛三個（後載入的那份贏，反白落在別的詞上）。
    ///
    /// 鍵是**華語國字**不是注音，見 `pack::Index::tw`。
    fn write_test_pack() -> bool {
        let dir = std::env::temp_dir().join("tsunagi-taigi-test");
        let _ = std::fs::create_dir_all(&dir);
        let content = "# 測試用\n\
            tw\t謝謝\t感恩\n\
            tw\t謝謝\tseh-seh\n\
            tw\t謝謝\t多謝\n\
            tw\t我\t阮\n\
            tw\t我們\t阮\n\
            tw\t我們\t咱\n\
            tw\t沙發\t膨椅\n";
        if std::fs::write(dir.join("測試台語.txt"), content).is_err() {
            return false;
        }
        crate::pack::load(dir.to_str().unwrap_or(""), &["測試台語".to_string()]);
        crate::pack::any_tw()
    }

    /// 載入引擎與那份臨時台語包。**整組測試只做一次**。
    ///
    /// # 為什麼要 `OnceLock`
    ///
    /// 內容共用一份還不夠。`cargo test` 預設**並行**跑，17 個測試各自
    /// 呼叫一次 `pack::load`，而 `load` 是「建好新索引再整個換掉」——
    /// 換的那一瞬間別的測試正在查，就查到半套。症狀是**隨機掛一個**
    /// （每次不同那個），單執行緒跑則全過。
    ///
    /// `OnceLock` 讓載入真的只發生一次：第一個到的做，其餘的等它做完
    /// 再一起往下走。之後沒有人再動那個索引，查詢自然穩定。
    ///
    /// **不用 `serial_test` 那類序列化**：那會讓 17 個測試排隊跑，而
    /// 它們其實只是共用同一份唯讀資料，並行本身沒有問題。
    fn load_with_taigi() -> bool {
        static READY: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *READY.get_or_init(|| {
            let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join("data");
            crate::preload(&data, crate::config::Engines::default());
            if !crate::dict::bopomofo_loaded() {
                return false;
            }
            write_test_pack()
        })
    }

    /// **鎖定注音**的 session——台語只在那時出現（使用者裁定）。
    fn sess(keys: &str) -> Session {
        let mut s = Session::new();
        s.set_lock(Some(Language::Bopomofo));
        for c in keys.chars() {
            s.push(c);
        }
        s
    }

    /// **鎖定注音時只列現況＋台語**（使用者裁定 2026-09-08）。
    ///
    /// 切法的候選在那裡全是雜訊：鎖定注音時切法只有一種，「切短一點」
    /// 要用倒退鍵；換語言更不用說（鎖定注音的人不要 `ゔ` 或整串英文）。
    /// 實測「謝謝不客氣」原本列 12 個，只有 2 個有意義。
    #[test]
    fn 鎖定時只列現況跟台語() {
        if !load_with_taigi() {
            return;
        }
        let s = sess("vu,4vu,4");
        let cands = s.seg_cands();
        assert!(!cands[0].taigi, "第一個該是現況");
        let others: Vec<String> = cands[1..]
            .iter()
            .filter(|c| !c.taigi)
            .map(|c| c.text.clone())
            .collect();
        assert!(others.is_empty(), "現況以外只該有台語，實際多了 {others:?}");
    }

    /// **自動模式不出台語**（使用者裁定 2026-09-08）。
    ///
    /// 一個注音最多對 16 個台語詞（`ㄌㄢˋㄋㄧˊㄅㄚ`），混進自動模式
    /// 會**整頁塞滿台語**，切法選項被擠到看不見——而自動模式的 TAB
    /// 本來就是在解切法問題。
    #[test]
    fn 自動模式不出台語() {
        if !load_with_taigi() {
            return;
        }
        // **不鎖定**（`sess` 會鎖定注音，這裡要自動模式）
        let mut s = Session::new();
        for c in "vu,4vu,4".chars() {
            s.push(c);
        }
        let tw: Vec<String> = s
            .seg_cands()
            .iter()
            .filter(|c| c.taigi)
            .map(|c| c.text.clone())
            .collect();
        assert!(tw.is_empty(), "自動模式的段選單不該有台語，實際 {tw:?}");
    }

    /// **台語是「多一個選擇」不是「取代」**：華語仍然排第一。
    #[test]
    fn 台語補在華語後面不取代() {
        if !load_with_taigi() {
            return;
        }
        let s = sess("vu,4vu,4");
        let cands = s.seg_cands();
        assert_eq!(cands[0].text, "謝謝", "第一名該still是華語");
        assert!(!cands[0].taigi);
        assert!(
            cands.iter().any(|c| c.taigi && c.text == "感恩"),
            "台語該列得出來，實際 {:?}",
            cands.iter().map(|c| c.text.clone()).collect::<Vec<_>>()
        );
    }

    /// 選了台語詞**真的送得出去**。
    ///
    /// `Segment` 只有 `(keys, lang)`，`compose` 會拿 `keys` 重算——
    /// 不另外記的話選了「感恩」送出的還是「謝謝」。
    #[test]
    fn 選了台語詞真的送得出去() {
        if !load_with_taigi() {
            return;
        }
        let mut s = sess("vu,4vu,4");
        assert_eq!(s.text(), "謝謝", "預設是華語");
        let cands = s.seg_cands();
        let Some(i) = cands.iter().position(|c| c.text == "感恩") else {
            panic!("該列得出「感恩」");
        };
        s.seg_set_cand(i);
        s.seg_confirm();
        assert_eq!(s.text(), "感恩", "選了台語就該送出台語");
    }

    /// **台語詞絕對不能進學習層**（§2.64.13 量出來的洞）。
    ///
    /// 記進去就變成「`vu,4vu,4` → 感恩」，選幾次之後打「謝謝」的注音
    /// 永遠出「感恩」——**關掉台語包也救不回來**，污染寫進學習檔了。
    #[test]
    fn 台語詞不進學習層() {
        if !load_with_taigi() {
            return;
        }
        crate::learn::clear();
        let mut s = sess("vu,4vu,4");
        let cands = s.seg_cands();
        if let Some(i) = cands.iter().position(|c| c.text == "感恩") {
            s.seg_set_cand(i);
            s.seg_confirm();
        }
        s.learn_on_commit();
        assert_eq!(
            crate::learn::index().best("vu,4vu,4"),
            None,
            "台語詞進了學習層——打「謝謝」會永遠出「感恩」，關包也救不回來"
        );
    }

    // ══ 反白單位是「詞」（2026-09-08 改）══════════════════
    //
    // 舊版拿**按鍵**去查台語，而按鍵有歧義：`z8 vu,4` 同時是「發謝」
    // 與「發洩」，所以「沙發謝謝」會冒出「發洩／出氣／消解」六個誤配
    // ——那兩個字在畫面上根本沒相鄰過。現在拿組字區選好的**國字**查。

    /// **最大匹配**：「我們」佔掉兩格之後不會再從「們」回頭試。
    #[test]
    fn 斷詞取最長的詞() {
        if !load_with_taigi() {
            return;
        }
        // 我們這個詞
        let s = sess("ji3ap65k4ek7h6");
        if s.text() != "我們這個詞" {
            return; // 引擎沒打出預期的字，這條測不到重點
        }
        let words = s.tw_words();
        assert!(
            words
                .iter()
                .any(|w| w.says[0] == "我們" && w.at == 0 && w.len == 2),
            "該切出「我們」（最長匹配），實際 {:?}",
            words.iter().map(|w| &w.says[0]).collect::<Vec<_>>()
        );
        // 「我」被「我們」吃掉了，不該再單獨出現
        assert!(
            !words.iter().any(|w| w.says[0] == "我"),
            "「我」該被「我們」吃掉——切過的不重複用"
        );
    }

    /// **跨詞邊界的誤配不該出現**——這是改用國字的主要理由。
    #[test]
    fn 不會撈到跨詞邊界的組合() {
        if !load_with_taigi() {
            return;
        }
        // 沙發謝謝：舊版會從「發」往後撈到「發謝」
        let s = sess("g8 z8 vu,4vu,4");
        if s.text() != "沙發謝謝" {
            return;
        }
        let words = s.tw_words();
        let names: Vec<&String> = words.iter().map(|w| &w.says[0]).collect();
        assert!(
            names.contains(&&"沙發".to_string()),
            "該有「沙發」，實際 {names:?}"
        );
        assert!(
            names.contains(&&"謝謝".to_string()),
            "該有「謝謝」，實際 {names:?}"
        );
        // 跨在中間的組合一個都不該有
        for w in &words {
            assert!(
                w.says[0] == "沙發" || w.says[0] == "謝謝",
                "冒出跨詞邊界的「{}」——國字斷詞不該切出畫面上沒相鄰的字",
                w.says[0]
            );
        }
    }

    /// `Shift+←` 縮寬度：「我們」→「我」，兩種都拿得到（使用者裁定丙案）。
    #[test]
    fn shift左右調反白寬度() {
        if !load_with_taigi() {
            return;
        }
        let mut s = sess("ji3ap65k4ek7h6");
        if s.text() != "我們這個詞" {
            return;
        }
        assert_eq!(
            s.tw_cursor_range(),
            Some((0, 2)),
            "預設反白斷詞算的「我們」"
        );
        assert!(
            s.seg_cands().iter().any(|c| c.text == "咱"),
            "「我們」該有「咱」"
        );

        s.tw_narrow();
        assert_eq!(s.tw_cursor_range(), Some((0, 1)), "縮成一格＝「我」");
        let cands = s.seg_cands();
        assert!(cands.iter().any(|c| c.text == "阮"), "「我」該有「阮」");
        assert!(
            !cands.iter().any(|c| c.text == "咱"),
            "縮成「我」之後不該還有「我們」的說法"
        );

        // **起點固定**——推寬只往右長，不會跑到「們」去
        s.tw_widen();
        assert_eq!(s.tw_cursor_range(), Some((0, 2)), "推回兩格");
    }

    /// `←→` 跳詞：**跳過沒有台語說法的地方**（使用者裁定）。
    #[test]
    fn 跳詞會略過沒台語的位置() {
        if !load_with_taigi() {
            return;
        }
        let mut s = sess("g8 z8 vu,4vu,4");
        if s.text() != "沙發謝謝" {
            return;
        }
        assert_eq!(s.tw_cursor_range(), Some((0, 2)), "從「沙發」開始");
        s.tw_next();
        assert_eq!(s.tw_cursor_range(), Some((2, 2)), "跳到「謝謝」");
        s.tw_prev();
        assert_eq!(s.tw_cursor_range(), Some((0, 2)), "跳回「沙發」");
    }

    /// **「現況」那筆要跟反白範圍一致**——反白只框住一個詞時，
    /// 第一個候選不該是整句。
    #[test]
    fn 現況候選跟反白範圍一致() {
        if !load_with_taigi() {
            return;
        }
        let s = sess("g8 z8 vu,4vu,4");
        if s.text() != "沙發謝謝" {
            return;
        }
        let cands = s.seg_cands();
        assert_eq!(cands[0].text, "沙發", "現況該是反白那個詞，不是整句");
        assert!(!cands[0].taigi);
    }

    /// 定案之後反白要**往下走**，不是跳回開頭。
    ///
    /// 定案會改變斷詞結果（「沙發」變「膨椅」就不是原本那個詞了），
    /// 事後才問位置會拿到重算過的答案。
    #[test]
    fn 定案後跳到下一個詞() {
        if !load_with_taigi() {
            return;
        }
        let mut s = sess("g8 z8 vu,4vu,4");
        if s.text() != "沙發謝謝" {
            return;
        }
        let cands = s.seg_cands();
        let Some(i) = cands.iter().position(|c| c.text == "膨椅") else {
            return;
        };
        s.seg_set_cand(i);
        s.seg_confirm_with(true);
        assert_eq!(s.text(), "膨椅謝謝", "定案要換掉那幾格");
        let cands = s.seg_cands();
        assert_eq!(
            cands[0].text, "謝謝",
            "定案後該反白下一個詞，實際 {cands:?}"
        );
    }

    /// **「選完停住」要真的停住**（設定頁的第二個選項）。
    ///
    /// 定案過的詞會從斷詞結果裡消失（「沙發」變「膨椅」就不是華語詞
    /// 了），只釘起點的話 `tw_span` 會往後找到下一個詞，反白照樣跑掉
    /// ——所以連寬度一起釘。實測回報「這功能在台語 TAB 中沒生效」。
    #[test]
    fn 選完停住不會跳到下一個詞() {
        if !load_with_taigi() {
            return;
        }
        let mut s = sess("g8 z8 vu,4vu,4");
        if s.text() != "沙發謝謝" {
            return;
        }
        let cands = s.seg_cands();
        let Some(i) = cands.iter().position(|c| c.text == "膨椅") else {
            return;
        };
        s.seg_set_cand(i);
        s.seg_confirm_with(false); // 選完離開
        assert_eq!(s.text(), "膨椅謝謝");
        assert_eq!(
            s.tw_cursor_range(),
            Some((0, 2)),
            "反白該留在剛定案的「膨椅」上，不是跳到「謝謝」"
        );
        // **而且選單要關掉**——「選完離開選字」的語意就是選完就退出，
        // 跟選字那層一致（`confirm_cand_with(false)` 直接 `exit_select`）。
        // 少了這個就退不出去（實測回報）。
        assert!(s.seg_done(), "「選完離開選字」該關掉選單");
    }

    /// **`seg_done` 在台語模式要算「詞」不是「段」**。
    ///
    /// 鎖定注音時整串是一段，用段數判斷的話第一次 Enter 就會讓
    /// `seg_locked` 追上 `seg_count()`（都是 1），TSF 層立刻關掉選單
    /// ——使用者根本沒機會往下選。
    #[test]
    fn 定案一個詞不算走完() {
        if !load_with_taigi() {
            return;
        }
        let mut s = sess("g8 z8 vu,4vu,4");
        if s.text() != "沙發謝謝" {
            return;
        }
        let cands = s.seg_cands();
        let Some(i) = cands.iter().position(|c| c.text == "膨椅") else {
            return;
        };
        s.seg_set_cand(i);
        s.seg_confirm_with(true);
        assert!(
            !s.seg_done(),
            "還有「謝謝」可以選，不該關掉選單（用段數算會誤判成走完）"
        );
    }

    /// **選了台語之後要選得回中文**（實測回報「台語選不回中文」）。
    ///
    /// 候選是拿畫面上的字去查台語庫，而定案後那幾格顯示的是「膨椅」
    /// ——它不是華語詞，查不到任何東西，候選只剩它自己。修法是定案過
    /// 的位置拿 `keys` 重算華語（見 `tw_cursor_zh`）。
    #[test]
    fn 選了台語還能選回中文() {
        if !load_with_taigi() {
            return;
        }
        let mut s = sess("g8 z8 vu,4vu,4");
        if s.text() != "沙發謝謝" {
            return;
        }
        let cands = s.seg_cands();
        let Some(i) = cands.iter().position(|c| c.text == "膨椅") else {
            return;
        };
        s.seg_set_cand(i);
        s.seg_confirm_with(false); // 停在原地
        assert_eq!(s.text(), "膨椅謝謝");

        // 候選要還在——原本的華語＋所有台語說法
        let cands = s.seg_cands();
        assert_eq!(cands[0].text, "沙發", "第一個該是原本的華語，選得回去");
        assert!(
            cands.iter().any(|c| c.text == "膨椅"),
            "台語說法也要留著，實際 {cands:?}"
        );

        // 真的選回中文
        let i = cands.iter().position(|c| c.text == "沙發").unwrap();
        s.seg_set_cand(i);
        s.seg_confirm_with(false);
        assert_eq!(s.text(), "沙發謝謝", "選回中文要真的變回去");
    }

    /// **關掉選單再重開，定案過的台語還找得到**
    /// （實測回報「選成台語字之後按 TAB 會顯示中文但選不回去」）。
    ///
    /// 兩件事一起壞：定案過那幾格顯示「膨椅」不是華語詞，斷詞找不到
    /// 它，反白根本到不了那個位置；而且反白還停在上次定案後的地方。
    #[test]
    fn 關掉重開還選得回中文() {
        if !load_with_taigi() {
            return;
        }
        let mut s = sess("g8 z8 vu,4vu,4");
        if s.text() != "沙發謝謝" {
            return;
        }
        s.seg_open();
        let cands = s.seg_cands();
        let Some(i) = cands.iter().position(|c| c.text == "膨椅") else {
            return;
        };
        s.seg_set_cand(i);
        s.seg_confirm_with(true); // 選完往下一個（反白跑到「謝謝」）
        assert_eq!(s.text(), "膨椅謝謝");

        // TAB 關掉再開——TSF 走 exit_select() ＋ seg_open()
        s.exit_select();
        s.seg_open();

        // 斷詞要認得定案過的位置，否則反白回不去
        let words = s.tw_words();
        assert!(
            words.iter().any(|w| w.at == 0 && w.says[0] == "沙發"),
            "斷詞該認出定案過的「膨椅」原本是「沙發」，實際 {:?}",
            words.iter().map(|w| &w.says[0]).collect::<Vec<_>>()
        );

        // 反白要回到第一個詞
        assert_eq!(s.tw_cursor_range(), Some((0, 2)), "重開該從第一個詞開始");
        let cands = s.seg_cands();
        assert_eq!(cands[0].text, "沙發", "選得回中文");

        let i = cands.iter().position(|c| c.text == "沙發").unwrap();
        s.seg_set_cand(i);
        s.seg_confirm_with(false);
        assert_eq!(s.text(), "沙發謝謝");
    }

    /// **最後一個詞選完要跳出去，不是繞回第一個**
    /// （實測回報「最後選完沒跳出去又回到第一個」）。
    ///
    /// 判準必須在定案的當下記下來：「還有沒有詞可挑」永遠是有（定案
    /// 過的詞也留在斷詞裡，才回得去改），「反白在不在最後一個」也不行
    /// （`tw_next` 走到底會繞回開頭）。
    #[test]
    fn 最後一個詞選完就結束() {
        if !load_with_taigi() {
            return;
        }
        let mut s = sess("g8 z8 vu,4vu,4");
        if s.text() != "沙發謝謝" {
            return;
        }
        s.seg_open();

        // 第一個詞：還沒完
        let cands = s.seg_cands();
        let Some(i) = cands.iter().position(|c| c.text == "膨椅") else {
            return;
        };
        s.seg_set_cand(i);
        s.seg_confirm_with(true);
        assert!(!s.seg_done(), "還有「謝謝」可以選");

        // 最後一個詞：走完了
        let cands = s.seg_cands();
        let Some(i) = cands.iter().position(|c| c.text == "感恩") else {
            return;
        };
        s.seg_set_cand(i);
        s.seg_confirm_with(true);
        assert_eq!(s.text(), "膨椅感恩");
        assert!(s.seg_done(), "最後一個選完該關掉選單，不是繞回第一個");

        // 重開要能再選
        s.seg_open();
        assert!(!s.seg_done(), "重開之後又有得選了");
        assert_eq!(s.tw_cursor_range(), Some((0, 2)), "從第一個詞開始");
    }

    /// **選回中文一樣要往下跳**（實測回報「選回中文時沒觸發自動跳下一個」）。
    ///
    /// 定案時 `seg_taigi` 的 `retain` 會刪掉這個位置的台語紀錄，而
    /// `tw_span` 靠那筆紀錄才認得出「膨椅」原本是一個詞——刪掉之後
    /// 再問位置就整個錯掉，還會誤判成走完整句。所以反白範圍要在
    /// 動 `seg_taigi` 之前先問。
    #[test]
    fn 選回中文也要跳下一個() {
        if !load_with_taigi() {
            return;
        }
        let mut s = sess("g8 z8 vu,4vu,4");
        if s.text() != "沙發謝謝" {
            return;
        }
        s.seg_open();
        let cands = s.seg_cands();
        let Some(i) = cands.iter().position(|c| c.text == "膨椅") else {
            return;
        };
        s.seg_set_cand(i);
        s.seg_confirm_with(true);
        assert_eq!(s.text(), "膨椅謝謝");

        // 關掉再開，回頭把「膨椅」選回「沙發」
        s.exit_select();
        s.seg_open();
        let cands = s.seg_cands();
        let i = cands
            .iter()
            .position(|c| c.text == "沙發")
            .expect("選得回中文");
        s.seg_set_cand(i);
        s.seg_confirm_with(true);
        assert_eq!(s.text(), "沙發謝謝");
        assert_eq!(
            s.tw_cursor_range(),
            Some((2, 2)),
            "選回中文之後一樣要跳到「謝謝」"
        );
        assert!(!s.seg_done(), "還有「謝謝」可選，不該誤判成走完整句");
    }
}
