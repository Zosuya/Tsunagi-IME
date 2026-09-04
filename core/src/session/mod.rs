//! 一次輸入的完整狀態：按鍵串、選了哪種切法、選字選到哪一格。
//!
//! # 為什麼放在 core 而不是 TSF 層
//!
//! 這些狀態轉換（切法選單怎麼翻、選字怎麼移動、選了字之後後面怎麼改）
//! 跟平台無關，寫在 `platform/windows` 裡的話沒辦法測——TSF 的東西要
//! 真的裝上輸入法才跑得起來。
//!
//! 放在 core 就能用一般的單元測試驗證，TSF 層只負責「把按鍵翻譯成
//! 呼叫哪個方法」與「把結果畫出來」。
//!
//! # 兩層選擇
//!
//! ```text
//! 按鍵串  su3cl3
//!   ↓ 切法（TAB 選）
//! 注:su3cl3          ← 第一名，或使用者從選單挑的
//!   ↓ 選字（方向鍵選）
//! [你][好]           ← 每格可以各自換字
//! ```
//!
//! 切法換了，選字要重來——那是不同的分段，格子數都可能不一樣。

mod cutting;
mod select;
#[cfg(test)]
mod tests;

use crate::compose::{self, Slot};
// **切法引擎的東西這裡都不用了**——那些在 `crate::input` 裡面。
// 這一層只認 `Segment`（輸入層的產出）與選字相關的東西。
use crate::cutpoint::Segment;

/// 切法選單一次展開幾個。
///
/// 使用者定的：TAB 展開前 10，快速按兩下展開前 50。
pub const CUTTING_PAGE: usize = 10;
pub const CUTTING_PAGE_ALL: usize = 50;

/// 選字候選一般狀態顯示幾個（一直排）。
///
/// **九個，對齊數字鍵**（使用者定，2026-09-01）。選字時 `1`～`9` 可以
/// 直接挑，但沒有第十個鍵——列十個的話最後一個只能用方向鍵移過去，
/// 清單上卻跟其他九個長得一樣，看不出差別。
pub const CHAR_PAGE: usize = 9;

/// 展開全部時，每一欄放幾個。
///
/// **每欄獨立 1-9、向下數**——使用者定的，跟日文 IME 的排法一致。
/// 數字鍵只對應目前選中的那一欄，所以跟 `CHAR_PAGE` 一樣是九個。
pub const CHAR_COLUMN: usize = 9;

/// 展開全部時，畫面上最多同時顯示幾欄。
///
/// **候選數沒有上限**——ㄧˋ 有 340 個字，全部攤開是 38 欄，
/// 橫向長度遠超過任何螢幕，右邊會直接被切掉看不到。
/// 所以只畫十欄，反白移出可見範圍時整片跟著捲（見 `scroll_cand_into_view`）。
pub const MAX_COLUMNS: usize = 10;

/// `bool`，但**預設是 `true`**。
///
/// # 為什麼需要一個型別
///
/// `derive(Default)` 給 `bool` 的是 `false`。開關的產品預設是「開」的
/// 時候，那個落差會變成很難查的 bug——**TSF 那層的 `State` 是
/// `derive(Default)` 建的**，所以正式環境真的走這條路（見測試
/// 「default 建的 session 也能打字」）。
///
/// 在 `new()` 裡補寫預設值治不了本：`Default` 那條路還是錯的。
/// 讓型別自己帶對的預設，`derive` 就不會再騙人。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefaultOn(pub bool);

impl Default for DefaultOn {
    fn default() -> Self {
        Self(true)
    }
}

/// 一次輸入的狀態。
#[derive(Debug, Default)]
pub struct Session {
    /// **輸入層**：按鍵怎麼變成語言段落。
    ///
    /// 自動模式與鎖定注音是**兩套完全不同的輸入邏輯**，各自封裝在
    /// `input::Cascade` 與 `input::BopomofoInput` 裡。這裡只拿它們
    /// 的產出（切法清單），不管內部怎麼累積按鍵。見 `crate::input`。
    input: crate::input::Input,
    /// 啟用哪些語言引擎。停用的連自動辨識都跳過，見
    /// [`config::Engines`](crate::config::Engines)。
    engines: crate::config::Engines,
    /// 鎖定注音時標點鍵怎麼處理。見 `config::LockPunct`。
    lock_punct: crate::config::LockPunct,
    /// 使用者手動調整過的日文詞界。見 `compose::JpBounds`。
    jp_bounds: Option<crate::compose::JpBounds>,
    /// 鎖定時倒退鍵刪整格。見 `delete_marked_slot`。
    ///
    /// 用 `DefaultOn` 而不是 `bool`——這個開關預設是開的，見那個型別
    /// 的說明。
    backspace_whole_cell: DefaultOn,
    /// 每一種切法是不是某個語言的「代表」，跟 `cuttings` 一一對應。
    ///
    /// 用途只有一個：在選單上標出來（`（日）すし`），讓使用者一眼看出
    /// 「這一項是日文的最佳解」。見 `promote_language_reps`。
    ///
    /// **一種切法可能同時是兩個語言的代表**（`ちぇ喝一下` 的日文與注音
    /// 涵蓋都最多），所以是清單不是單一值——那時標成「（中日）」。
    rep_of: Vec<Vec<crate::language::Language>>,
    /// 輸入層算出來的切法（快取，每次按鍵重取）
    cuttings: Vec<Vec<Segment>>,
    /// 選單上選了第幾列。
    ///
    /// **是「選單列」不是「第幾種切法」**——選單尾巴可能多一列
    /// 「注音符號直出」，那一列不在 `cuttings` 裡。轉換用 `cut_at`。
    cutting_idx: usize,
    /// 「注音符號直出」那一列排在選單的第幾位。`None` 代表這次沒有。
    ///
    /// # 為什麼不放進 `cuttings`
    ///
    /// 它不是一種分段方式——不經過選詞層、沒有候選可選、輸出的也不是
    /// 文字而是符號本身。塞進 `cuttings` 就得讓 `compose` 認得一種
    /// 「不要選字」的假語言，那是為了省一個欄位去弄髒整條管線。
    symbol_pos: Option<usize>,
    /// 使用者**主動選過**符號那一列嗎？
    ///
    /// 跟 `chosen_cut` 同一個道理：每打一鍵選單都會重排，不記著的話
    /// 選了符號再多打一個字就跳回中文。
    symbol_chosen: bool,
    /// 使用者手動挑過的切法「長什麼樣」：每段的 `(按鍵, 語言)`。
    ///
    /// # 為什麼要記
    ///
    /// 每打一鍵切法都會重排，`cutting_idx` 歸零就跳回第一名。使用者
    /// 明明挑過一種切法，多打一個字就被換掉——手動選的字還在，但
    /// 它們屬於的那個分段沒了，看起來就像整串被重算。
    ///
    /// # 為什麼記「長什麼樣」而不是索引
    ///
    /// 索引會漂移：重排之後第 3 名可能變成第 5 名。記分段本身，
    /// 重算後再去新清單裡找**前綴相符**的那一種。
    chosen_cut: Option<Vec<(String, crate::language::Language)>>,
    /// 目前切法的選字格
    slots: Vec<Slot>,
    /// 選字選到第幾格；`None` 代表沒在選字
    select_idx: Option<usize>,
    /// 候選清單打開了嗎？
    ///
    /// **反白框與候選清單是兩件事**：框是「現在在改哪一格」的游標，
    /// 左右鍵移動它；候選清單是「這一格有哪些字可選」，按下鍵才叫出來。
    ///
    /// 混在一起的話，只是想把框移過去看看，候選視窗就一直彈出來擋著。
    cands_open: bool,
    /// **離開選字之後仍然記著的那一格**。
    ///
    /// 離開選字只代表「不再吃候選字的上下鍵」，不代表忘記使用者在看
    /// 哪裡。用途有兩個：下次按方向鍵從這裡接續、以及讓那一格的標記
    /// 留在畫面上。見 `exit_select` 與 `marked_index`。
    last_select: Option<usize>,
    /// 候選字清單裡反白第幾個。
    ///
    /// 跟 `select_idx` 是兩層：`select_idx` 是「在選哪一格」，
    /// 這個是「那一格的候選字裡反白哪一個」。
    cand_idx: usize,
    /// 候選字展開全部了嗎？
    ///
    /// 一般狀態只列前 `CHAR_PAGE` 個（一直排），展開後全部列出來、
    /// 分成多欄。按右鍵展開，Esc 收回。
    cand_expanded: bool,
    /// 展開時，可見的第一欄是第幾欄。
    ///
    /// 候選多到十欄裝不下時就靠它捲動——畫面永遠只畫
    /// `cand_col_first` 起算的 `MAX_COLUMNS` 欄。
    cand_col_first: usize,
    /// 全半形模式。Shift+Space 切換，見 `crate::width`。
    width: crate::width::Width,
    /// 鎖定成單一語言；`None` 是自動辨識（預設）。
    ///
    /// # 鎖定之後就是一般的輸入法
    ///
    /// 這個專案的特色是自動辨識，但使用者有時就是知道自己接下來要打
    /// 什麼。鎖定注音之後行為跟微軟新注音一樣——打 `hello` 就是照
    /// 注音鍵解讀（ㄘㄍ…），不會被當成英文；要打英文就切到英文模式，
    /// 而不是靠引擎猜。
    ///
    /// | 鎖定 | 行為等同 |
    /// |---|---|
    /// | 注音 | 微軟新注音 |
    /// | 日文 | Google 日本語入力（羅馬字） |
    /// | 英文 | 關掉輸入法直接打字 |
    ///
    /// **鎖定時不切段**——整串就是一段，切法選單自然也沒有東西可選。
    lock: Option<crate::language::Language>,
    /// 使用者手動選過的字：`(這一格的按鍵, 選了什麼字)`。
    ///
    /// # 為什麼要另外存
    ///
    /// 每次打新的鍵、換切法，`slots` 都是整個重建的——手動選的字
    /// 會被引擎重算的結果蓋掉。使用者選了「妳」，多打一個字就變回
    /// 「你」，那是最惱人的行為。
    ///
    /// # 為什麼用按鍵當 key 而不是格子位置
    ///
    /// 位置會漂移。切法一換，格數和順序都可能不同，第 2 格不再是
    /// 原本那個字。按鍵（`su3`）是跟著那個音節走的，切法怎麼換，
    /// 「使用者在 `su3` 這個音節選了『妳』」都成立。
    picks: Vec<(String, String)>,
}

impl Session {
    pub fn new() -> Self {
        Self {
            ..Default::default()
        }
    }

    /// 打一個鍵。
    pub fn push(&mut self, ch: char) {
        // 輸入層自己知道該怎麼累積——自動模式是字串追加，鎖定注音
        // 是四格音節緩衝。這裡只看它說「段落變了沒」。
        match self.input.push(ch, self.lock) {
            crate::input::Changed::Segments => self.refresh(),
            // 只有正在打的音節變了（還沒收尾），段落沒動，
            // 但畫面要更新——組字區顯示的是符號
            crate::input::Changed::PendingOnly => self.rebuild_slots(),
            crate::input::Changed::Nothing => {}
        }
    }

    /// 刪一個鍵。
    ///
    /// 累加式沒有退格——分支是一路累積的，沒有反向的走法。
    /// 整串重建，成本可接受（按鍵串通常十幾個字元）。
    ///
    /// **鎖定語言＋有反白框時例外**：那時倒退鍵刪掉整格，見
    /// `delete_marked_slot`。
    pub fn backspace(&mut self) {
        // 鎖定模式下有框的話，倒退鍵刪的是整個那一格（可在設定關掉）
        if self.backspace_whole_cell.0 && self.lock.is_some() {
            if let Some(i) = self.marked_index() {
                if self.delete_marked_slot(i) {
                    return;
                }
            }
        }
        self.backspace_one();
    }

    /// 把反白那一格整個刪掉。刪不了回 `false`（交回一般的退格）。
    ///
    /// # 為什麼鎖定模式要有這條路
    ///
    /// 鎖定注音時一格就是一個字（新酷音式）。使用者看到框停在某個字上，
    /// 按倒退鍵的直覺是「把這個字刪掉」——而不是「刪掉組成它的最後一個
    /// 注音符號」。後者要連按三四下才刪得掉一個字，中間還會經過幾個
    /// 半成品音節。
    ///
    /// # 刪完之後框往前挪一格
    ///
    /// 像文字游標那樣。停在原地的話，連按倒退鍵會一路吃掉後面的字；
    /// 直接收掉框的話，下一下倒退鍵又變成刪單鍵，行為忽然變了。
    fn delete_marked_slot(&mut self, i: usize) -> bool {
        let Some(slot) = self.slots.get(i) else {
            return false;
        };
        // 格子的按鍵接起來就是完整的按鍵串，所以位移用長度累加就行
        let start: usize = self.slots[..i].iter().map(|s| s.keys.len()).sum();
        let len = slot.keys.len();
        if len == 0 {
            return false;
        }
        let keys = self.input.drain_keys();
        if start + len > keys.len() {
            // 對不起來就別亂刪——交回一般的退格比較安全
            self.input = crate::input::Input::from_keys_with(&keys, self.lock, self.engines);
            return false;
        }
        let mut left = String::with_capacity(keys.len() - len);
        left.push_str(&keys[..start]);
        left.push_str(&keys[start + len..]);

        let was_selecting = self.select_idx.is_some();
        self.input = crate::input::Input::from_keys_with(&left, self.lock, self.engines);
        // 那一格沒了，使用者挑過的字也跟著作廢
        self.picks.retain(|(k, _)| *k != slot.keys);
        self.chosen_cut = None;
        self.refresh();

        // 框往前挪一格（`refresh` 會把位置清掉，這裡補回來）
        if !self.slots.is_empty() {
            let at = i.saturating_sub(1).min(self.slots.len() - 1);
            if was_selecting {
                self.select_idx = Some(at);
            } else {
                self.last_select = Some(at);
            }
        }
        true
    }

    /// 原本的退格：刪一個鍵。
    fn backspace_one(&mut self) {
        match self.input.backspace(self.lock) {
            crate::input::Changed::Segments => self.refresh(),
            crate::input::Changed::PendingOnly => self.rebuild_slots(),
            crate::input::Changed::Nothing => {}
        }
    }

    /// 清空這一次輸入，**但保留跨輸入的模式設定**。
    ///
    /// # 什麼該留、什麼該清
    ///
    /// | 留 | 清 |
    /// |---|---|
    /// | 鎖定的語言、全半形模式 | 按鍵串、切法、選字狀態、手動選過的字 |
    ///
    /// 鎖定與全半形是**使用者對輸入法的設定**，不是這一次輸入的一部分——
    /// 打完一句話送出去，下一句還在同一個模式裡。原本這裡是
    /// `*self = Self::new()`，把設定一起沖掉，結果每送出一次就跳回
    /// 自動模式。
    /// 把反白那一格的**日文詞界**往右推一個假名（`Shift+→`）。
    ///
    /// # 為什麼需要這件事
    ///
    /// Viterbi 只能給「詞典查得到的」分法。遇到詞典沒收的專有名詞
    /// （`うさだぺこら`）時，它切出來的格子再怎麼選字都拼不出正確
    /// 答案——**使用者得能自己把詞界拉開**，逐段選字組出來。
    ///
    /// 那也是「第一次輸入一個引擎不認識的詞」的唯一途徑：拼一次、
    /// 學起來，第二次就自動對了。
    pub fn widen_word(&mut self) -> bool {
        self.adjust_word(1)
    }

    /// 把反白那一格的日文詞界往左收一個假名（`Shift+←`）。
    pub fn narrow_word(&mut self) -> bool {
        self.adjust_word(-1)
    }

    /// 詞界調整的本體。`delta` 是這一格要多吃（＋）或吐回（－）幾個假名。
    fn adjust_word(&mut self, delta: i32) -> bool {
        let Some(i) = self.select_idx else {
            return false;
        };
        if self.slots.get(i).map(|s| s.lang) != Some(crate::language::Language::Romaji) {
            return false;
        }
        // 這一格屬於哪一段連續的日文
        let mut start = i;
        while start > 0 && self.slots[start - 1].lang == crate::language::Language::Romaji {
            start -= 1;
        }
        let mut end = i + 1;
        while end < self.slots.len() && self.slots[end].lang == crate::language::Language::Romaji {
            end += 1;
        }
        // 目前的詞界＝每格的假名長度
        let kana_len = |keys: &str| {
            crate::romaji::kana::to_kana(keys)
                .map(|k| k.chars().count())
                .unwrap_or_else(|| keys.chars().count())
        };
        let mut lens: Vec<usize> = self.slots[start..end]
            .iter()
            .map(|s| kana_len(&s.keys))
            .collect();
        let k = i - start;
        let total: usize = lens.iter().sum();

        if delta > 0 {
            // 往右吃：後面要有東西可以吃
            if k + 1 >= lens.len() || lens[k + 1] == 0 {
                return false;
            }
            lens[k] += 1;
            lens[k + 1] -= 1;
            if lens[k + 1] == 0 {
                lens.remove(k + 1);
            }
        } else {
            if lens[k] <= 1 {
                return false;
            }
            lens[k] -= 1;
            match lens.get_mut(k + 1) {
                Some(next) => *next += 1,
                None => lens.push(1),
            }
        }
        debug_assert_eq!(lens.iter().sum::<usize>(), total, "假名總數不該變");

        let keys: String = self.slots[start..end]
            .iter()
            .map(|s| s.keys.as_str())
            .collect();
        self.jp_bounds = Some(crate::compose::JpBounds { keys, lens });
        self.rebuild_slots();
        // 框留在同一個詞上；詞界收掉時可能少一格，夾住範圍
        self.select_idx = Some(i.min(self.slots.len().saturating_sub(1)));
        true
    }

    /// 送出了——把這一次的選擇學起來。
    ///
    /// **呼叫端要先確定這裡可以學**：密碼欄與 `IS_PRIVATE` 的欄位不能學
    /// （見開發文件 §2.12.4——帳號、身分證、信用卡都在那裡）。核心
    /// 看不到那些訊號，所以守門在平台層。
    ///
    /// 回傳記了幾條。沒有手動改過字就是 0。
    pub fn learn_on_commit(&self) -> usize {
        let mut n = crate::learn::record(&self.slots);
        // **按過 Tab 換切法才有切詞訊號**——沒換就代表引擎給對了，
        // 記下來只是在強化現狀。跟選字那條的 `picked` 同一個道理。
        if self.chosen_cut.is_some() && !self.symbol_chosen {
            if let (Some(chosen), Some(default)) = (
                self.cut_at(self.cutting_idx)
                    .and_then(|i| self.cuttings.get(i)),
                self.cuttings.first(),
            ) {
                n += crate::learn::record_cutting(self.input.keys(), chosen, default);
            }
        }
        n
    }

    /// **使用者明講這是標點**（Ctrl+`,` 之類）。
    ///
    /// 只有鎖定注音時有意義——其他模式的標點鍵本來就打得出標點。
    /// 回傳 `false` 代表現在的模式不需要它，呼叫端該照一般按鍵處理。
    pub fn push_punct(&mut self, ch: char) -> bool {
        if !self.input.push_punct(ch) {
            return false;
        }
        self.refresh();
        true
    }

    /// 鎖定注音時標點鍵怎麼處理。設定改了要呼叫。
    pub fn set_lock_punct(&mut self, mode: crate::config::LockPunct) {
        self.lock_punct = mode;
        self.input.set_punct_mode(mode);
    }

    pub fn clear(&mut self) {
        let lock = self.lock;
        let width = self.width;
        let engines = self.engines;
        let punct = self.lock_punct;
        *self = Self::new();
        self.lock = lock;
        self.width = width;
        self.engines = engines;
        // 輸入層要配合鎖定的語言重建——注音鎖定用的是另一套邏輯，
        // 預設的 `Cascade` 不對
        self.input = crate::input::Input::with_engines(lock, engines);
        // `clear` 會重建輸入層，設定要跟著套回去——不然清空一次就失效
        self.set_lock_punct(punct);
    }

    pub fn is_empty(&self) -> bool {
        self.input.is_empty()
    }

    pub fn keys(&self) -> &str {
        self.input.keys()
    }

    /// **組字區該顯示什麼**。
    ///
    /// 兩種模式顯示的東西不一樣：
    ///
    /// | 模式 | 顯示 | 為什麼 |
    /// |---|---|---|
    /// | 自動 | 原始按鍵 `su3cl3` | 還不知道是注音還是英文，顯示按鍵才誠實 |
    /// | 鎖定注音 | `你好ㄋㄧ` | 已經確定是注音了，跟新酷音一致 |
    /// | 鎖定日文 | `すし` | 同上，顯示假名 |
    ///
    /// 鎖定注音時是**已完成的字＋正在打的注音符號**——前半來自
    /// `slots`（可以選字、查詞庫修正），後半是還沒收尾的那個音節。
    pub fn composition_text(&self) -> String {
        match self.lock {
            Some(crate::language::Language::Bopomofo) => {
                let mut t = self.text();
                t.push_str(&self.input.pending());
                t
            }
            Some(crate::language::Language::Romaji) => {
                // 已轉的部分是假名（`text()` 會查詞庫轉），
                // 加上還沒湊成 mora 的殘留字母
                let mut t = self.text();
                t.push_str(&self.input.pending());
                t
            }
            // 自動模式與鎖定英文都顯示原始按鍵
            _ => self.input.keys().to_string(),
        }
    }

    /// 正在打、還沒收尾的那個注音音節（鎖定注音時才會有東西）。
    pub fn pending_symbols(&self) -> String {
        self.input.pending()
    }

    /// 打字或退格之後重算切法與選字格。
    ///
    /// **切法變了選字就要重來**——不同的分段，格子數都可能不一樣，
    /// 沿用舊的位置會指到錯的地方。
    /// 換一組啟用的引擎（設定改過時呼叫）。
    ///
    /// 會重建輸入層並重算——不然已經打的字還是照舊的規則切的。
    pub fn set_engines(&mut self, engines: crate::config::Engines) {
        if self.engines == engines {
            return;
        }
        self.engines = engines;
        // 鎖定的語言被關掉了就退回自動
        if let Some(l) = self.lock {
            if !engines.enabled(l) {
                self.lock = None;
            }
        }
        let keys = self.input.drain_keys();
        self.input = crate::input::Input::from_keys_with(&keys, self.lock, engines);
        // 切換鎖定也會重建輸入層，設定同樣要套回去
        self.input.set_punct_mode(self.lock_punct);
        self.chosen_cut = None;
        self.refresh();
    }

    /// 換掉「倒退鍵刪整格」的開關（設定改過時呼叫）。
    pub fn set_backspace_whole_cell(&mut self, on: bool) {
        self.backspace_whole_cell = DefaultOn(on);
    }

    /// 目前啟用哪些引擎。
    pub fn engines(&self) -> crate::config::Engines {
        self.engines
    }

    /// 目前鎖定哪個語言；`None` 是自動。
    pub fn lock(&self) -> Option<crate::language::Language> {
        self.lock
    }

    /// 鎖定成某個語言，或傳 `None` 解鎖回自動。
    pub fn set_lock(&mut self, lang: Option<crate::language::Language>) {
        if self.lock == lang {
            return;
        }
        self.lock = lang;
        // **換模式＝換一套輸入邏輯**，所以整個輸入層重建。
        //
        // 已經打的按鍵要帶過去（含還沒收尾的音節，`drain_keys` 會
        // 一併結算），不然那些字會憑空消失。
        let keys = self.input.drain_keys();
        self.input = crate::input::Input::from_keys_with(&keys, lang, self.engines);
        // 切法的前提變了，使用者挑過的那一種不再適用
        self.chosen_cut = None;
        self.refresh();
    }

    /// 輪到下一個模式：自動 → 注音 → 日文 → 英文 → 自動。
    ///
    /// **一個鍵輪替，不是三個獨立的鎖定鍵**——跟 `Shift+空白` 切全半形
    /// 同一套互動，使用者只要記一個鍵。自動排第一（那是預設），
    /// 其餘照[語言辨識瀑布](crate::language::detect)的順序。
    pub fn cycle_lock(&mut self) {
        use crate::language::Language::*;
        // **停用的語言要跳過**——關掉日文之後輪替就是三態
        // （自動→注音→英文），不會停在一個沒有引擎的模式上。
        let mut next = self.lock;
        for _ in 0..4 {
            next = match next {
                None => Some(Bopomofo),
                Some(Bopomofo) => Some(Romaji),
                Some(Romaji) => Some(English),
                Some(English) => None,
            };
            match next {
                None => break,
                Some(l) if self.engines.enabled(l) => break,
                _ => {}
            }
        }
        self.set_lock(next);
    }

    fn refresh(&mut self) {
        // 切法由輸入層算好了，這裡只負責「拿來、選一種、重建選字格」。
        //
        // 怎麼算是輸入層的事——自動模式是累加式切法加排序，鎖定注音
        // 是單一切法。這一層不必知道差別。
        self.cuttings = self.input.cuttings().to_vec();
        self.promote_language_reps();
        self.symbol_pos = self.symbol_row().map(|_| self.symbol_slot_pos());
        // **找回使用者挑過的切法**，而不是無條件跳回第一名。
        // 使用者多打一個字，前面已經確定的分段不該被重排掉。
        self.cutting_idx = match (self.symbol_chosen, self.symbol_pos) {
            // 選過符號那一列就留在符號那一列
            (true, Some(p)) => p,
            // **旗標不要在這裡清掉**：打到一半音節還不完整時（`su3c`）
            // 這一列會暫時消失，清掉的話打完下一個音節就回不去了。
            // 只有使用者自己選了別列才算改變心意，那是 `remember_cut` 的事。
            _ => self.menu_index_of_cut(self.find_chosen().unwrap_or(0)),
        };
        self.select_idx = None;
        // **又打字了就把標記收掉**：那個標記的意思是「你剛改了這一格」，
        // 繼續打字之後它就過期了，留著會在組字區留下一個沒人管的框。
        self.last_select = None;
        self.cand_idx = 0;
        self.cand_expanded = false;
        self.cand_col_first = 0;
        self.cands_open = false;
        self.rebuild_slots();
    }

    /// 讓切法選單的 **4～6 名固定是三種語言各自的代表**。
    ///
    /// # 解決什麼
    ///
    /// 使用者心裡想的常常是「我這句其實想打的是純日文」或「這串就是
    /// 英文，別轉」。但那種切法在排序裡不見得靠前——尤其純英文，
    /// passthrough 沒有任何詞典依據，通常掉到很後面，要翻很久。
    ///
    /// | 語言 | 代表 |
    /// |---|---|
    /// | 注音 | 注音涵蓋字數最多的那一種切法 |
    /// | 日文 | 日文涵蓋字數最多的那一種 |
    /// | 英文 | **整串 passthrough**——英文收任何字元，所以直接造一個， |
    /// |  | 不從清單裡找（它常常根本進不了前段） |
    ///
    /// # 前三名一動也不動
    ///
    /// 那是引擎算出來的，第一名就是預設會送出的東西。代表一律排在
    /// 它們**後面**，而且已經出現在前三名的就不再重複列一次。
    ///
    /// 這是跟「整體偏好某語言」的關鍵差別：偏好會把正解擠掉（實測
    /// `su3cl3` 偏日文之後「你好」掉到第三名），這個不會動到任何
    /// 已經排在前面的東西。
    fn promote_language_reps(&mut self) {
        use crate::language::Language;
        /// 前面幾名不動。
        const KEEP: usize = 3;

        // **鎖定語言時不補代表**：那個模式的整個意義就是「只有一種
        // 切法、不要給我選」，補進來等於把鎖定破壞掉。
        // （既有測試「鎖定時只有一種切法」抓到過這件事。）
        if self.lock.is_some() {
            self.rep_of.clear();
            return;
        }
        let old = std::mem::take(&mut self.cuttings);
        if old.is_empty() {
            return;
        }

        // **看引擎認可了幾個字，不是段落裡有幾個按鍵**。
        //
        // 看按鍵數的話會挑到「按鍵多但都不是詞」的垃圾切法——實測
        // `check u vu84` 的中文代表會變成「ちぇ喝一下」而不是
        // 「check 一下」。見 `rank::covered_by`。
        let covered = |c: &Vec<Segment>, want: Language| -> usize {
            crate::cutpoint::rank::covered_by(c, want)
        };

        let mut reps: Vec<(Language, Vec<Segment>)> = Vec::new();
        for want in [Language::Bopomofo, Language::Romaji] {
            // **先找「整串都是這個語言」的那一種**。
            //
            // 「（中）」這個記號答應使用者的就是「整句當中文唸」——
            // 只要那種讀法存在，它就是答案，不必再跟涵蓋分數比。
            //
            // 原本這條只是 `covered` 全是 0 時的墊底（單音節那個情境），
            // 結果是：整串明明可以是純中文，卻因為切得碎的那一種涵蓋
            // 分數比較高而被選走，（中）那一列混著日文段。
            let best = old
                .iter()
                .find(|c| !c.is_empty() && c.iter().all(|s| s.is_mark || s.lang == want));
            // 沒有純粹的讀法時（`check` 那種字元注音根本吃不下的），
            // 退而求其次：涵蓋最多的那一種。**同樣多就取排序較前的**
            // ——那是引擎認為比較好的那一個，沒理由換掉。
            let best = best.or_else(|| {
                old.iter()
                    .filter(|c| covered(c, want) > 0)
                    .fold(None::<&Vec<Segment>>, |acc, c| match acc {
                        Some(a) if covered(a, want) >= covered(c, want) => Some(a),
                        _ => Some(c),
                    })
            });
            if let Some(b) = best {
                reps.push((want, b.clone()));
            }
        }
        let keys = self.input.keys().to_string();
        // **日文代表也可以是造出來的**——整串當日文。
        //
        // 原本只從既有切法裡挑，所以生成階段沒給出日文段就沒得選。
        // **單母音正是這種**：`a` 在日文引擎是合法的（あ），但它同時是
        // 英文最常用的詞，`lang_of` 那條「很常用的英文詞不讓給日文」
        // （本來是為了擋 `you`→よう）把它判給英文，於是五個單母音的
        // 切法裡一個日文段都沒有。
        //
        // 造一個代表比改 `lang_of` 安全得多——後者會動到切點排序，
        // 而 `a␣banana`、`I␣am` 這些英文正解都靠那條規則。
        if self.engines.romaji
            && !keys.is_empty()
            && !reps.iter().any(|(l, _)| *l == Language::Romaji)
            && crate::romaji::validity(&keys) == crate::romaji::Validity::Valid
        {
            reps.push((
                Language::Romaji,
                vec![Segment {
                    keys: keys.clone(),
                    is_mark: false,
                    lang: Language::Romaji,
                }],
            ));
        }
        // 英文代表是造出來的，不是找出來的
        if !keys.is_empty() {
            reps.push((
                Language::English,
                vec![Segment {
                    keys,
                    is_mark: false,
                    lang: Language::English,
                }],
            ));
        }

        let keep = KEEP.min(old.len());
        let mut out: Vec<Vec<Segment>> = old[..keep].to_vec();
        // **代表在哪一位就標在哪一位**——已經排在前三名的也要標，
        // 不然「すし」明明是日文的最佳解卻沒有記號，反而更難懂
        let mut marks: Vec<Vec<Language>> = vec![Vec::new(); out.len()];
        for (lang, r) in reps {
            match out.iter().position(|c| *c == r) {
                // **用 push 不是覆蓋**：同一種切法可能同時是兩個語言的
                // 代表，覆蓋的話後面那個會把前面的吃掉，畫面上就少一個
                Some(i) => marks[i].push(lang),
                None => {
                    out.push(r);
                    marks.push(vec![lang]);
                }
            }
        }
        for c in old {
            if !out.contains(&c) {
                out.push(c);
                marks.push(Vec::new());
            }
        }
        self.cuttings = out;
        self.rep_of = marks;
    }

    /// 在新的切法清單裡找回使用者挑過的那一種。
    ///
    /// （鎖定模式不會走到這裡——那時切法只有一種。）
    ///
    /// 比對**前綴**而不是全等——使用者挑完之後又多打了幾個字，
    /// 新的切法會比當初長，但前面那幾段應該一樣。
    fn find_chosen(&self) -> Option<usize> {
        let want = self.chosen_cut.as_ref()?;
        self.cuttings.iter().position(|c| {
            c.len() >= want.len()
                && c.iter()
                    .zip(want)
                    .all(|(seg, (keys, lang))| seg.keys == *keys && seg.lang == *lang)
        })
    }

    /// 把目前選中的切法記下來，之後重算要找回它。
    fn remember_cut(&mut self) {
        self.symbol_chosen = self.on_symbol_row();
        // 符號那一列不是切法，沒有東西好記
        if self.symbol_chosen {
            return;
        }
        self.chosen_cut = self
            .cut_at(self.cutting_idx)
            .and_then(|i| self.cuttings.get(i))
            .map(|c| c.iter().map(|s| (s.keys.clone(), s.lang)).collect());
    }

    fn rebuild_slots(&mut self) {
        // 符號那一列：整串一格、不給選字（符號沒有候選可挑）
        if let (true, Some(text)) = (self.on_symbol_row(), self.symbol_row()) {
            self.slots = vec![Slot {
                keys: self.input.keys().to_string(),
                text,
                lang: crate::language::Language::Bopomofo,
                selectable: false,
                picked: false,
            }];
            return;
        }
        self.slots = match self
            .cut_at(self.cutting_idx)
            .and_then(|i| self.cuttings.get(i))
        {
            Some(c) => compose::compose_with_bounds(c, self.width, self.jp_bounds.as_ref()),
            None => Vec::new(),
        };
        self.reapply_picks();
    }

    /// 現在選中的是「注音符號直出」那一列嗎？
    pub(super) fn on_symbol_row(&self) -> bool {
        self.symbol_pos == Some(self.cutting_idx)
    }

    /// 選單第 `i` 列對應 `cuttings` 的第幾個？符號那一列回 `None`。
    pub(super) fn cut_at(&self, i: usize) -> Option<usize> {
        match self.symbol_pos {
            Some(p) if i == p => None,
            Some(p) if i > p => Some(i - 1),
            _ => Some(i),
        }
    }

    /// `cuttings` 的第 `i` 個排在選單第幾列？（`cut_at` 的反向）
    fn menu_index_of_cut(&self, i: usize) -> usize {
        match self.symbol_pos {
            Some(p) if i >= p => i + 1,
            _ => i,
        }
    }

    /// 「注音符號直出」要排在選單第幾列——**三語代表的後面**。
    ///
    /// 代表最多三個，但已經排在前三名的不會另外附加，所以位置要看
    /// 實際標了記號的最後一列在哪，不能寫死。
    fn symbol_slot_pos(&self) -> usize {
        let last_rep = self
            .rep_of
            .iter()
            .rposition(|m| !m.is_empty())
            .map(|i| i + 1)
            .unwrap_or(0);
        last_rep.max(1).min(self.cuttings.len())
    }

    /// 「注音符號直出」那一列要顯示什麼？
    ///
    /// # 判準是「每個鍵都是注音鍵」，不是「切得出合法音節」
    ///
    /// 一開始要求整串能切成完整合法音節，結果是**單獨的符號打不出來**
    /// ——`ㄅ` 不能自成音節（注音裡沒有單獨念 ㄅ 的字），所以打 `1` 那一列
    /// 就消失。而這個功能的用途正是「把注音符號當文字打出來」（維護
    /// `priority.txt`、寫擴充包、寫文件），單獨的 `ㄅㄆㄇㄈ` 與聲調符號
    /// `ˊˇˋ˙` 都是要打的。
    ///
    /// 實測「符號＋空白」（空白是一聲鍵）只救得回 42 個符號裡的 23 個
    /// ——能自成音節的那些（`ㄗ`→「資」、`ㄚ`→「啊」），聲母與聲調救不回來。
    ///
    /// **代價是這一列幾乎永遠都在**：大千配置把 26 個字母全用掉了，
    /// 所以 `sushi` 也會給出 `ㄋㄨㄕㄏㄛ`。使用者裁決（2026-09-01）接受
    /// 這個代價——選單是按 Tab 才開的，而且位置固定反而比忽隱忽現好按。
    ///
    /// 音節之間**不加空格**——跟 `priority.txt`、擴充包裡的寫法一致，
    /// 複製貼上就能用。
    fn symbol_row(&self) -> Option<String> {
        // 鎖定注音時組字區本來就顯示符號了，不必再給一列
        if self.lock.is_some() {
            return None;
        }
        let mut out = String::new();
        for c in self.input.keys().chars() {
            // 一聲是空白鍵，沒有對應的符號。注音的一聲本來就不標，
            // 跳過才是正確的寫法
            if c == ' ' {
                continue;
            }
            out.push(crate::bopomofo::keymap::symbol_of(c)?);
        }
        (!out.is_empty()).then_some(out)
    }

    /// 把使用者手動選過的字套回重建後的格子上。
    ///
    /// 重建之後每一格都是引擎算的結果，這裡按「按鍵」對回去——
    /// 同一個音節出現多次的話（打「你你」）依序對應。
    fn reapply_picks(&mut self) {
        if self.picks.is_empty() {
            return;
        }
        // 同一個按鍵可能出現多次，用過的就不再用
        let mut used = vec![false; self.picks.len()];
        for i in 0..self.slots.len() {
            if !self.slots[i].selectable {
                continue;
            }
            let found = self
                .picks
                .iter()
                .enumerate()
                .position(|(k, (keys, _))| !used[k] && *keys == self.slots[i].keys);
            if let Some(k) = found {
                used[k] = true;
                compose::pick(&mut self.slots, i, &self.picks[k].1.clone());
            }
        }
    }

    /// 目前的全半形模式。
    pub fn width(&self) -> crate::width::Width {
        self.width
    }

    /// 直接指定全半形模式（設定檔載入、測試用）。
    pub fn set_width(&mut self, w: crate::width::Width) {
        self.width = w;
        self.rebuild_slots();
    }

    /// 切換全半形（Shift+Space）。三態輪流：自動 → 半形 → 全形。
    ///
    /// 切完要重畫已經打好的標點——使用者按下去就該看到效果，
    /// 不是等下一個符號才變。
    pub fn toggle_width(&mut self) {
        self.width = self.width.next();
        self.rebuild_slots();
    }

    // ── 選字 ──

    // ── 展開全部候選 ──
}
