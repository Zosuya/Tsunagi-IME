//! 按鍵綁定：把「按了什麼鍵」翻譯成「要做什麼」。
//!
//! # 為什麼要抽出來
//!
//! 原本按鍵是寫死在 `OnKeyDown` 的一長串 `if-else`，而且 `wants_key`
//! 又把同一批鍵列了一次——**改一個鍵要動兩個地方**。加上切法選單
//! （TAB、Shift+空白）與選字（方向鍵）之後會更難維護。
//!
//! 抽成「按鍵 → 動作」的表之後：
//!
//! - 加新鍵只要改表，不必動 `OnKeyDown` 的邏輯
//! - 之後可以從設定檔讀，使用者能自己改鍵位
//! - **按鍵序列（「秘笈」式）的偵測有地方放**——那需要一個看得到
//!   每一個按鍵的中心點，散在 `if-else` 裡做不到
//!
//! # 模式決定同一個鍵做什麼
//!
//! 空白鍵在「打字中」是注音的一聲，在「切法選單開著」是往下選。
//! 所以查表要看模式，不能只看鍵碼。

pub use ime_core::command::Dir;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_BACK, VK_CONTROL, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_RETURN, VK_RIGHT, VK_SHIFT,
    VK_SPACE, VK_TAB, VK_UP,
};

/// 輸入法目前在做什麼。同一個鍵在不同模式下的意義不同。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// 沒有組字——按鍵一律放行給宿主
    #[default]
    Idle,
    /// 正在打字，預覽列顯示第一名
    Typing,
    /// 切法選單開著（按過 TAB）
    CuttingMenu,
    /// 選字中（反白某一格）
    Selecting,
    /// 選字中且候選字已展開全部（多欄）。
    ///
    /// 跟 `Selecting` 分開是因為方向鍵的意義不同：一般狀態左右是換格，
    /// 展開後左右是換欄。同一個鍵在兩種狀態下做不同的事，正是要分模式。
    SelectingExpanded,
}

/// 使用者要做的事。動作的實作跟按哪個鍵無關。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// 輸入一個字元
    Input(char),
    /// 刪一個字元
    Backspace,
    /// 取消組字
    Cancel,
    /// 送出目前的文字
    Commit,

    // ── 切法選單 ──
    /// 展開切法選單（TAB）；已經開著時展開更多（雙擊 TAB）
    OpenCuttingMenu,
    /// 切法選單：下一個
    NextCutting,
    /// 切法選單：上一個
    PrevCutting,
    /// 選中反白的切法，關閉選單但留在組字狀態（Enter）
    ConfirmCutting,
    /// 單純關閉選單，切法維持原本選中的那個（TAB）
    CloseCuttingMenu,

    // ── 選字 ──
    /// 進入選字模式，反白第一格
    EnterSelect,
    /// 進入選字模式，反白最後一格（按左鍵進入時）
    EnterSelectLast,
    /// 日文詞界往右推一個假名（`Shift+→`）
    WidenWord,
    /// 日文詞界往左收一個假名（`Shift+←`）
    NarrowWord,
    /// 反白位置左移
    SelectLeft,
    /// 反白位置右移
    SelectRight,
    /// 候選字反白往下一個
    NextCand,
    /// 候選字反白往上一個
    PrevCand,
    /// 選中目前反白的候選字（Enter）
    ConfirmCand,
    /// 展開全部候選字（右鍵）
    ExpandAllChars,
    /// 收回展開狀態，回到一般的一直排
    CollapseChars,
    /// 展開狀態：反白往右一欄
    NextColumn,
    /// 展開狀態：反白往左一欄
    PrevColumn,
    /// 選第 N 個候選字（0-based）
    PickChar(usize),
    /// 數字鍵盤打的字元（數字與 `+ - * / .`）。
    ///
    /// **跟 `Input` 分開**：主鍵盤那排的 `5` 是注音的ㄓ，數字鍵盤的
    /// `5` 就該是 `5`——那正是它存在的意義。走 `Input` 的話會被切點
    /// 引擎當成注音鍵。
    NumpadInput(char),
    /// 方向鍵：餵給手勢偵測器，順便當作「進選字」的候補
    Gesture(Dir),
    /// 切換全半形（Shift+空白）
    ToggleWidth,

    /// 吃掉這個鍵，什麼都不做。
    ///
    /// 組字中的按鍵一律歸輸入法管——沒綁定的鍵放行給宿主會把游標
    /// 移出組字區，組字就散了。見 `lookup`。
    Swallow,
}

/// 一個按鍵組合。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Combo {
    pub vk: u32,
    pub shift: bool,
    pub ctrl: bool,
}

impl Combo {
    pub const fn plain(vk: u32) -> Self {
        Self {
            vk,
            shift: false,
            ctrl: false,
        }
    }
    pub const fn shift(vk: u32) -> Self {
        Self {
            vk,
            shift: true,
            ctrl: false,
        }
    }
}

/// Ctrl 現在按著嗎？
///
/// 一般的 Ctrl 組合在 `OnKeyDown` 就擋掉了（留給宿主），這個是用來
/// 判斷「單按 Ctrl」——見 `text_service` 的 `mod_used`。
pub fn ctrl_down() -> bool {
    unsafe { GetKeyState(VK_CONTROL.0 as i32) < 0 }
}

/// Shift 現在按著嗎？
pub fn shift_down() -> bool {
    unsafe { GetKeyState(VK_SHIFT.0 as i32) < 0 }
}

/// 這一下 key-down 是系統的**自動重複**嗎？
///
/// `lparam` 的第 30 位元是「前一次的鍵狀態」——1 代表這個鍵**本來就
/// 按著**，也就是按住不放時系統重送的那些。
///
/// 修飾鍵也會重複，而那正是 `CtrlTap` 踩過的坑：重複的 Ctrl key-down
/// 會把「這一輪用過了」洗掉，複製貼上就變成切換語言。
fn is_repeat_bits(lparam: isize) -> bool {
    lparam & 0x4000_0000 != 0
}

/// 同上，收 TSF 傳來的 `LPARAM`。
pub fn is_repeat(lparam: windows::Win32::Foundation::LPARAM) -> bool {
    is_repeat_bits(lparam.0)
}

/// 「單按 Ctrl」的偵測。
///
/// # 為什麼需要一個狀態機
///
/// 單按 `Ctrl` 是語言輪替，但 `Ctrl+C`／`Ctrl+V` 這種組合**不能**觸發
/// ——那是使用者在複製貼上，模式被切走會莫名其妙。判斷方式是：Ctrl
/// 按著時只要看到別的鍵，這一輪就不算單按。
///
/// **這條規則錯過兩次**，所以抽出來測：
///
/// 1. 一開始把「按下 Ctrl 本身」也算成用過，結果單按永遠沒反應。
/// 2. 標記只做在 `OnKeyDown`，但那個方法在我們對修飾鍵回「不要」之後
///    就不會被呼叫——`Ctrl+C` 的那個 C 根本看不到，放開就誤判成單按。
///    複製貼上是最常按的組合，這個漏洞天天踩。
#[derive(Debug, Default, Clone, Copy)]
pub struct CtrlTap {
    used: bool,
}

impl CtrlTap {
    /// 有鍵按下去了。
    ///
    /// `ctrl_held` 是「這一刻 Ctrl 按著嗎」，`repeat` 是「這一下是系統
    /// 的自動重複嗎」（`lparam` 的第 30 位元＝前一次的鍵狀態）。
    pub fn key_down(&mut self, vk: u32, ctrl_held: bool, repeat: bool) {
        if vk == VK_CONTROL.0 as u32 {
            // **按下 Ctrl 是新一輪的開始**。
            //
            // 不能算成「用過」——按下 Ctrl 的那一刻它自己已經是按著的，
            // 算進去的話單按永遠成立不了（這個 bug 犯過）。
            //
            // 順手重置，把上一輪沒收乾淨的狀態清掉——中途換視窗、
            // 放開事件沒收到的話，殘留會讓下一次單按失效。
            //
            // **但自動重複的那些不算新一輪**（這是第三次踩到這條規則）。
            // 按著 Ctrl 不放時系統會持續送 key-down，重置寫在這裡的話：
            //
            // ```text
            // 按下 Ctrl  → used = false
            // 按下 C     → used = true
            // Ctrl 重複  → used = false   ← 洗掉了
            // 放開 Ctrl  → 誤判成單按 → 語言被切走
            // ```
            //
            // 症狀是「**有時候**複製貼上會意外切換語言」——要按著 Ctrl
            // 超過自動重複的延遲（預設約 0.5 秒）才會發生，所以時有時無。
            //
            // 拿不到 `repeat` 訊號時（一律傳 false）行為跟修之前一樣，
            // 不會比現況更糟。
            if !repeat {
                self.used = false;
            }
        } else if ctrl_held {
            self.used = true;
        }
    }

    /// Ctrl 放開了。回傳這一輪**是不是單按**，並重置狀態。
    pub fn ctrl_released(&mut self) -> bool {
        !std::mem::take(&mut self.used)
    }
}

/// 鍵位表。**這是唯一的一份，不開放使用者自訂**。
///
/// 要調鍵位就改這裡——集中成一張表正是為了「只動一個地方」。
///
/// # 為什麼不做自訂（2026-08-31 決定）
///
/// 這張表已經很擠：十個數字全是注音鍵、方向鍵在選字、Tab 開切法
/// 選單、Ctrl 輪替語言鎖定、Shift+空白切全半形、↑↑↓↓ 是指令手勢。
/// 能空出來給使用者綁的本來就沒幾個。
///
/// 而**綁錯的代價很高**：不小心佔用注音鍵，打字本身就壞了；輸入法
/// 壞掉的時候使用者往往連改回來的介面都叫不出來。要把衝突偵測做到
/// 可靠（每個模式下誰已被佔用、哪些會被宿主或系統吃掉），工程量比
/// 功能本身還大。
///
/// # 為什麼數字鍵不在這裡
///
/// **十個數字全都是注音鍵**（`3467` 是聲調，其餘在大千鍵盤上也都有
/// 對應符號）。一般輸入法用數字選候選，這個輸入法不能——那會讓
/// `r4`（ㄐㄋ）打不出來。選字改用方向鍵。
#[rustfmt::skip]
const DEFAULT_BINDINGS: &[(Mode, Combo, Action)] = &[
    // ── 打字中 ──
    (Mode::Typing, Combo::plain(VK_BACK.0 as u32),   Action::Backspace),
    (Mode::Typing, Combo::plain(VK_ESCAPE.0 as u32), Action::Cancel),
    (Mode::Typing, Combo::plain(VK_RETURN.0 as u32), Action::Commit),
    (Mode::Typing, Combo::plain(VK_TAB.0 as u32),    Action::OpenCuttingMenu),
    // 上下鍵走手勢偵測：湊滿「上上下下」且組字內容是指令就執行，
    // 否則退回原本的「進選字」。見 `ime_core::command::Gesture`。
    (Mode::Typing, Combo::plain(VK_DOWN.0 as u32),   Action::Gesture(Dir::Down)),
    (Mode::Typing, Combo::plain(VK_UP.0 as u32),     Action::Gesture(Dir::Up)),
    // 左右鍵在打字中也要吃掉——組字時**所有按鍵都歸輸入法管**，
    // 放行的話游標會跑到組字區外面，組字就散了。
    // 左鍵進選字要從**最後一格**開始——使用者按左鍵的直覺是
    // 「從右邊選過來」，從第一格進來會看起來像跳過了最後一個字
    (Mode::Typing, Combo::plain(VK_LEFT.0 as u32),   Action::EnterSelectLast),
    (Mode::Typing, Combo::plain(VK_RIGHT.0 as u32),  Action::EnterSelect),
    // **空白鍵在打字中是注音的一聲**，要當成輸入而不是控制鍵。
    // 沒綁的話會放行給宿主，組字就被打斷了。
    //
    // 但沒組字時（`Mode::Idle`）不綁——那時候空白就是空白。
    // 一聲必須前面已經有構成合法注音的鍵，不會憑空從空白開始。
    (Mode::Typing, Combo::plain(VK_SPACE.0 as u32),  Action::Input(' ')),
    // Shift+空白切換全半形。三態輪流：自動 → 半形 → 全形。
    // 沒組字時也要能切——使用者可能先設好模式再開始打。
    (Mode::Typing, Combo::shift(VK_SPACE.0 as u32),  Action::ToggleWidth),
    (Mode::Idle,   Combo::shift(VK_SPACE.0 as u32),  Action::ToggleWidth),
    (Mode::Selecting, Combo::shift(VK_SPACE.0 as u32), Action::ToggleWidth),
    // **日文詞界調整**（文節伸縮）。日文 IME 的通用慣例，而且我們的
    // Shift+方向鍵本來就是空的。只在選字時有意義——那時框停在某一格上，
    // 「把這一格拉長／縮短」才有指涉對象。見 `Session::widen_word`。
    (Mode::Selecting, Combo::shift(VK_RIGHT.0 as u32), Action::WidenWord),
    (Mode::Selecting, Combo::shift(VK_LEFT.0 as u32),  Action::NarrowWord),

    // ── 切法選單 ──
    //
    // 空白往下、Shift+空白往上——使用者指定的。
    // TAB 再按一次展開更多（`OpenCuttingMenu` 自己判斷）。
    (Mode::CuttingMenu, Combo::plain(VK_SPACE.0 as u32),  Action::NextCutting),
    (Mode::CuttingMenu, Combo::shift(VK_SPACE.0 as u32),  Action::PrevCutting),
    // TAB 在選單裡是「單純關掉選單」，不選也不送出。
    // 快速按兩下展開全部是靠 `OpenCuttingMenu` 自己判斷時間差，
    // 那個判斷在 `text_service` 裡（見 `DOUBLE_TAB`）。
    (Mode::CuttingMenu, Combo::plain(VK_TAB.0 as u32),    Action::CloseCuttingMenu),
    (Mode::CuttingMenu, Combo::plain(VK_DOWN.0 as u32),   Action::NextCutting),
    (Mode::CuttingMenu, Combo::plain(VK_UP.0 as u32),     Action::PrevCutting),
    (Mode::CuttingMenu, Combo::plain(VK_RIGHT.0 as u32),  Action::NextCutting),
    (Mode::CuttingMenu, Combo::plain(VK_LEFT.0 as u32),   Action::PrevCutting),
    // Enter 是「就選反白這個切法」——關掉選單但**留在組字狀態**，
    // 不是送出。送出要再按一次 Enter（那時已經是 Typing 模式）。
    (Mode::CuttingMenu, Combo::plain(VK_RETURN.0 as u32), Action::ConfirmCutting),
    // esc 跟 TAB 一樣是關掉選單，不是取消組字——
    // 使用者只是不想選了，字還在打
    (Mode::CuttingMenu, Combo::plain(VK_ESCAPE.0 as u32), Action::CloseCuttingMenu),
    (Mode::CuttingMenu, Combo::plain(VK_BACK.0 as u32),   Action::Backspace),

    // ── 選字中（未展開，一次列 10 個候選）──
    //
    // **兩個層次的方向鍵語意**（使用者定的）：
    //
    // | | 未展開 | 展開後 |
    // |---|---|---|
    // | ↑↓ | 換候選字 | 同欄上下 |
    // | ←→ | **在字與字之間移動** | 換欄 |
    // | 空白 | **展開全部** | **收合**（同一個鍵開關） |
    //
    // 未展開時左右是「換格」——選完這個字換下一個字繼續選。
    // 原本右鍵綁的是展開全部，結果選完一格沒辦法移到別格。
    (Mode::Selecting, Combo::plain(VK_LEFT.0 as u32),   Action::SelectLeft),
    (Mode::Selecting, Combo::plain(VK_RIGHT.0 as u32),  Action::SelectRight),
    // 上下鍵在候選字清單裡移動反白
    (Mode::Selecting, Combo::plain(VK_DOWN.0 as u32),   Action::NextCand),
    (Mode::Selecting, Combo::plain(VK_UP.0 as u32),     Action::PrevCand),
    (Mode::Selecting, Combo::plain(VK_TAB.0 as u32),    Action::OpenCuttingMenu),
    (Mode::Selecting, Combo::plain(VK_BACK.0 as u32),   Action::Backspace),
    // **空白鍵展開全部候選**——10 個不夠時攤開來找
    (Mode::Selecting, Combo::plain(VK_SPACE.0 as u32),  Action::ExpandAllChars),
    // Enter 是「選中反白的候選字」，不是送出——
    // 送出要先 esc 退出選字選單，回到 Typing 再按 Enter
    (Mode::Selecting, Combo::plain(VK_RETURN.0 as u32), Action::ConfirmCand),
    (Mode::Selecting, Combo::plain(VK_ESCAPE.0 as u32), Action::Cancel),

    // ── 選字中且已展開全部（多欄）──
    //
    // 上下同欄移動、左右換欄——使用者定的。
    (Mode::SelectingExpanded, Combo::plain(VK_DOWN.0 as u32),   Action::NextCand),
    (Mode::SelectingExpanded, Combo::plain(VK_UP.0 as u32),     Action::PrevCand),
    (Mode::SelectingExpanded, Combo::plain(VK_RIGHT.0 as u32),  Action::NextColumn),
    (Mode::SelectingExpanded, Combo::plain(VK_LEFT.0 as u32),   Action::PrevColumn),
    // **空白鍵是展開／收合的開關**——展開後再按一次就收回來。
    // 翻欄交給左右鍵，空白只管這一件事，語意才單純。
    (Mode::SelectingExpanded, Combo::plain(VK_SPACE.0 as u32),  Action::CollapseChars),
    (Mode::SelectingExpanded, Combo::plain(VK_RETURN.0 as u32), Action::ConfirmCand),
    // Esc 先收回展開，再按一次才離開選字。
    //
    // 跟空白鍵通到同一個動作，但語意不同——空白是「開關」，
    // Esc 是「退回上一層」。兩條路殊途同歸在輸入法裡很常見。
    (Mode::SelectingExpanded, Combo::plain(VK_ESCAPE.0 as u32), Action::CollapseChars),
    (Mode::SelectingExpanded, Combo::plain(VK_BACK.0 as u32),   Action::Backspace),
];

/// 這個按鍵在這個模式下要做什麼？沒綁定就回 `None`（放行給宿主）。
pub fn lookup(mode: Mode, vk: u32) -> Option<Action> {
    // **選字模式下數字是選字，不是輸入**。
    //
    // 打字時十個數字全都是注音鍵，不能拿來選候選（那會讓 r4 打不出來）。
    // 但選字模式已經在挑字了，不會再輸入注音，數字就空出來了——
    // 這跟新注音的行為一致。
    if matches!(mode, Mode::Selecting | Mode::SelectingExpanded) {
        if let Some(d) = (0x31..=0x39u32).contains(&vk).then(|| (vk - 0x31) as usize) {
            return Some(Action::PickChar(d));
        }
        // 數字鍵盤也能選——**它本來就不是注音鍵**，沒有衝突。
        // 這是主鍵盤那排做不到的事（見 `DEFAULT_BINDINGS` 的說明）。
        if let Some(d) = (0x61..=0x69u32).contains(&vk).then(|| (vk - 0x61) as usize) {
            return Some(Action::PickChar(d));
        }
    }
    // **數字鍵盤：打什麼就是什麼**。要放在 `typed_char` 之前，
    // 不然會被當成一般字元送進組字區、被切點引擎當注音鍵解讀。
    //
    // 只有 NumLock 開著時 Windows 才送 `VK_NUMPAD*`；關著送的是
    // Home/End/方向鍵那些，本來就該走別的路。
    if let Some(ch) = numpad_char(vk) {
        return Some(Action::NumpadInput(ch));
    }
    // 字元鍵優先——它跟模式無關，隨時可以繼續打字。
    // `typed_char` 自己會看 Shift（Shift+1 是 `!`）。
    if let Some(ch) = typed_char(vk) {
        return Some(Action::Input(ch));
    }
    // Idle 只有空白鍵有綁定（注音的一聲），其餘放行給宿主
    let combo = Combo {
        vk,
        shift: shift_down(),
        ctrl: false,
    };
    DEFAULT_BINDINGS
        .iter()
        .find(|(m, c, _)| *m == mode && *c == combo)
        .map(|(_, _, a)| *a)
        // Shift 版本沒綁的話退回無 Shift 版本——
        // 使用者按 Shift+Enter 應該還是送出
        .or_else(|| {
            DEFAULT_BINDINGS
                .iter()
                .find(|(m, c, _)| *m == mode && c.vk == vk && !c.shift)
                .map(|(_, _, a)| *a)
        })
        // **組字中沒綁定的鍵一律吃掉**（使用者要求）。
        //
        // 放行給宿主的話，Home／End／PageUp 這些鍵會把游標移出組字區，
        // 而輸入法還以為自己在組字——接下來打的字就跑到別的地方去了。
        // 寧可讓那顆鍵沒反應，也不能讓組字散掉。
        //
        // Idle 不在此列：沒組字時輸入法不該干擾宿主。
        .or(if mode == Mode::Idle {
            None
        } else {
            Some(Action::Swallow)
        })
}

/// 這個虛擬鍵碼對應哪個輸入字元？不是輸入用的鍵回 `None`。
///
/// **要看 Shift**：Shift+1 是 `!` 而不是 `1`。原本沒看，所以
/// `!@#$%` 這些符號全打不出來——按下去只拿到數字，還被當成注音吃掉。
///
/// 空白鍵**不在這裡**——它在打字中是注音的一聲，在切法選單裡是
/// 往下選，要看模式決定。見 `lookup` 與 `Mode`。
///
/// # 符號為什麼也算「輸入字元」
///
/// 符號會進組字區，切點引擎把純標點當**硬切點**——`su3cl3!wu0an`
/// 會切成「你好 │ ! │ 世界」，兩邊各自切不會合併。所以一口氣打完
/// 帶標點的句子是可行的，不必為了打一個驚嘆號先送出前半段。
/// 見 `ime_core::cutpoint::punct`。
/// 數字鍵盤上的鍵打出什麼字元。不是數字鍵盤的鍵回 `None`。
///
/// **NumLock 關著時 Windows 送的是別的 VK**（Home、End、方向鍵……），
/// 所以這裡只會在 NumLock 開著時命中——剛好就是使用者想打數字的時候。
pub fn numpad_char(vk: u32) -> Option<char> {
    match vk {
        0x60..=0x69 => char::from_digit(vk - 0x60, 10),
        0x6A => Some('*'),
        0x6B => Some('+'),
        0x6D => Some('-'),
        0x6E => Some('.'),
        0x6F => Some('/'),
        _ => None,
    }
}

pub fn typed_char(vk: u32) -> Option<char> {
    let shift = shift_down();
    match vk {
        // 字母：Shift 給大寫。**大寫要進組字**——不然打不出
        // Hello、GitHub 這種大寫開頭的英文詞。
        0x41..=0x5A => {
            let c = vk as u8 as char;
            Some(if shift { c } else { c.to_ascii_lowercase() })
        }
        // 數字列：Shift 給上排符號
        0x30..=0x39 => {
            let digit = (vk as u8) as char;
            Some(if shift { shifted_digit(digit) } else { digit })
        }
        // 注音鍵盤也會用到的符號鍵，各有 Shift 版本
        0xBC => Some(if shift { '<' } else { ',' }),
        0xBE => Some(if shift { '>' } else { '.' }),
        0xBA => Some(if shift { ':' } else { ';' }),
        0xBF => Some(if shift { '?' } else { '/' }),
        0xBD => Some(if shift { '_' } else { '-' }),
        // 其餘符號鍵。這些在注音鍵盤上沒有對應音符，
        // 純粹是標點——切點引擎會把它們當硬切點。
        0xBB => Some(if shift { '+' } else { '=' }),
        0xC0 => Some(if shift { '~' } else { '`' }),
        0xDB => Some(if shift { '{' } else { '[' }),
        0xDD => Some(if shift { '}' } else { ']' }),
        0xDC => Some(if shift { '|' } else { '\\' }),
        0xDE => Some(if shift { '"' } else { '\'' }),
        _ => None,
    }
}

/// 數字列按著 Shift 是哪個符號。
///
/// 這是 US 鍵盤的排列。之後要支援別種鍵盤配置的話，這張表要跟著換。
fn shifted_digit(d: char) -> char {
    match d {
        '1' => '!',
        '2' => '@',
        '3' => '#',
        '4' => '$',
        '5' => '%',
        '6' => '^',
        '7' => '&',
        '8' => '*',
        '9' => '(',
        '0' => ')',
        other => other,
    }
}

#[cfg(test)]
mod tests {
    /// 單按 Ctrl 的偵測。這條規則錯過兩次，用測試釘住。
    mod 單按ctrl {
        use super::super::CtrlTap;
        const CTRL: u32 = 0x11;
        const C: u32 = 0x43;

        #[test]
        fn 自動重複的位元判得出來() {
            assert!(!super::super::is_repeat_bits(0x0000_0001), "第一次按下");
            assert!(super::super::is_repeat_bits(0x4000_0001), "按著不放的重複");
        }

        #[test]
        fn 單獨按放算單按() {
            let mut t = CtrlTap::default();
            t.key_down(CTRL, true, false); // 按下 Ctrl 時它自己已經是按著的
            assert!(t.ctrl_released(), "單按 Ctrl 應該算數");
        }

        #[test]
        fn 複製貼上不算單按() {
            let mut t = CtrlTap::default();
            t.key_down(CTRL, true, false);
            t.key_down(C, true, false); // Ctrl+C
            assert!(!t.ctrl_released(), "Ctrl+C 不該切換語言");
        }

        /// **假設驗證**：按著 Ctrl 不放時 Windows 會持續送 key-down，
        /// 那個重複會不會把「用過了」洗掉？
        #[test]
        fn 按著ctrl的自動重複不該洗掉用過的標記() {
            let mut t = CtrlTap::default();
            t.key_down(CTRL, true, false); // 按下 Ctrl
            t.key_down(C, true, false); // Ctrl+C
            t.key_down(CTRL, true, true); // ← 按著不放，系統重送 Ctrl 的 key-down
            assert!(!t.ctrl_released(), "重複的 Ctrl key-down 不該讓它變成單按");
        }

        #[test]
        fn 用過之後會重置() {
            let mut t = CtrlTap::default();
            t.key_down(CTRL, true, false);
            t.key_down(C, true, false);
            assert!(!t.ctrl_released());
            // 下一輪才是真的單按——狀態沒重置的話這裡會失敗
            t.key_down(CTRL, true, false);
            assert!(t.ctrl_released(), "上一輪的組合不該影響這一輪");
        }

        #[test]
        fn 沒按著ctrl時的按鍵不影響() {
            let mut t = CtrlTap::default();
            t.key_down(C, false, false); // 單純打字
            t.key_down(CTRL, true, false);
            assert!(t.ctrl_released(), "打字之後單按 Ctrl 仍然算數");
        }

        #[test]
        fn 按下ctrl會清掉殘留() {
            let mut t = CtrlTap::default();
            t.key_down(CTRL, true, false);
            t.key_down(C, true, false);
            // 這一輪沒收到「放開」就換了視窗，殘留著「用過」
            t.key_down(CTRL, true, false);
            assert!(t.ctrl_released(), "新一輪不該被上一輪的殘留拖累");
        }

        #[test]
        fn 連按兩次都算() {
            let mut t = CtrlTap::default();
            for _ in 0..2 {
                t.key_down(CTRL, true, false);
                assert!(t.ctrl_released());
            }
        }
    }

    use super::*;

    #[test]
    fn 字元鍵不分模式() {
        assert_eq!(
            lookup(Mode::Typing, 0x41),
            Some(Action::Input('a')),
            "A-Z 隨時可以打字"
        );
        assert_eq!(lookup(Mode::CuttingMenu, 0x34), Some(Action::Input('4')));
    }

    #[test]
    fn shift_數字給符號() {
        // 使用者回報：!@#$% 這些打不出來。原因是 typed_char 沒看 Shift，
        // 按 Shift+1 只拿到 '1'，還被當成注音吃掉。
        //
        // 注意：這個測試讀的是真實鍵盤狀態，沒按著 Shift 時
        // `typed_char` 回的是數字——所以這裡直接驗換算表。
        assert_eq!(shifted_digit('1'), '!');
        assert_eq!(shifted_digit('2'), '@');
        assert_eq!(shifted_digit('3'), '#');
        assert_eq!(shifted_digit('4'), '$');
        assert_eq!(shifted_digit('5'), '%');
        assert_eq!(shifted_digit('0'), ')');
    }

    #[test]
    fn 符號鍵有被接手() {
        // 這些鍵原本完全沒綁，按下去輸入法不管，符號進不了組字區
        for vk in [0xBBu32, 0xC0, 0xDB, 0xDD, 0xDC, 0xDE] {
            assert!(
                typed_char(vk).is_some(),
                "VK {vk:#x} 該是輸入字元（標點會當硬切點）"
            );
        }
    }

    /// 數字鍵盤跟主鍵盤那排數字**語意完全不同**，這組測試守住這件事。
    mod 數字鍵盤 {
        use super::super::{lookup, Action, Mode};

        #[test]
        fn 打字時打出的是數字不是注音() {
            // 主鍵盤的 5（0x35）在這個輸入法裡是ㄓ，走 Input；
            // 數字鍵盤的 5（0x65）就是 5，走 NumpadInput
            assert_eq!(lookup(Mode::Typing, 0x35), Some(Action::Input('5')));
            assert_eq!(lookup(Mode::Typing, 0x65), Some(Action::NumpadInput('5')));
        }

        #[test]
        fn 十個數字都認得() {
            for d in 0..=9u32 {
                let ch = char::from_digit(d, 10).unwrap();
                assert_eq!(
                    lookup(Mode::Typing, 0x60 + d),
                    Some(Action::NumpadInput(ch)),
                    "數字鍵盤的 {ch}"
                );
            }
        }

        #[test]
        fn 運算符號也認得() {
            for (vk, ch) in [
                (0x6A, '*'),
                (0x6B, '+'),
                (0x6D, '-'),
                (0x6E, '.'),
                (0x6F, '/'),
            ] {
                assert_eq!(lookup(Mode::Typing, vk), Some(Action::NumpadInput(ch)));
            }
        }

        #[test]
        fn 選字時可以拿來選候選() {
            // 這是主鍵盤那排做不到的——它們全是注音鍵
            assert_eq!(lookup(Mode::Selecting, 0x61), Some(Action::PickChar(0)));
            assert_eq!(lookup(Mode::Selecting, 0x69), Some(Action::PickChar(8)));
        }

        #[test]
        fn 選字時的零不拿來選() {
            // 候選只編到 1~9，0 沒有對應的那一列
            assert_ne!(lookup(Mode::Selecting, 0x60), Some(Action::PickChar(9)));
        }
    }

    #[test]
    fn 數字鍵是輸入不是選候選() {
        // 十個數字全都是注音鍵，不能拿來選候選
        for vk in 0x30..=0x39u32 {
            assert!(
                matches!(lookup(Mode::Typing, vk), Some(Action::Input(_))),
                "VK {vk:#x} 該是輸入"
            );
        }
    }

    #[test]
    fn 同一個鍵不同模式做不同的事() {
        let tab = VK_TAB.0 as u32;
        assert_eq!(lookup(Mode::Typing, tab), Some(Action::OpenCuttingMenu));
        let down = VK_DOWN.0 as u32;
        assert_eq!(lookup(Mode::Typing, down), Some(Action::Gesture(Dir::Down)));
        assert_eq!(lookup(Mode::CuttingMenu, down), Some(Action::NextCutting));
        assert_eq!(lookup(Mode::Selecting, down), Some(Action::NextCand));
    }

    #[test]
    fn 沒組字時不攔按鍵() {
        assert_eq!(lookup(Mode::Idle, VK_LEFT.0 as u32), None);
        assert_eq!(lookup(Mode::Idle, VK_RETURN.0 as u32), None);
        // 但字元鍵要接手——那是開始組字
        assert!(lookup(Mode::Idle, 0x41).is_some());
    }

    #[test]
    fn 沒組字時空白就是空白() {
        // 一聲要前面已經有合法注音，不會憑空從空白開始。
        // 注意：`shift_down` 讀的是真實鍵盤狀態，測試時沒按著 Shift。
        assert_eq!(lookup(Mode::Idle, VK_SPACE.0 as u32), None);
    }

    #[test]
    fn 選字選單的上下鍵移動反白() {
        // 圖上的「上下方向鍵：改變選中選項」
        assert_eq!(
            lookup(Mode::Selecting, VK_DOWN.0 as u32),
            Some(Action::NextCand)
        );
        assert_eq!(
            lookup(Mode::Selecting, VK_UP.0 as u32),
            Some(Action::PrevCand)
        );
        // Enter 是選中反白，不是送出
        assert_eq!(
            lookup(Mode::Selecting, VK_RETURN.0 as u32),
            Some(Action::ConfirmCand)
        );
        // 左右鍵是換格，不是換候選字
        assert_eq!(
            lookup(Mode::Selecting, VK_LEFT.0 as u32),
            Some(Action::SelectLeft)
        );
    }

    #[test]
    fn 未展開時左右是換格() {
        // **選完一個字要能移到下一個字繼續選**。
        //
        // 原本右鍵綁的是「展開全部」，結果左右不成對——選完一格
        // 沒辦法往右移，只能靠左鍵繞一圈。
        assert_eq!(
            lookup(Mode::Selecting, VK_RIGHT.0 as u32),
            Some(Action::SelectRight)
        );
        assert_eq!(
            lookup(Mode::Selecting, VK_LEFT.0 as u32),
            Some(Action::SelectLeft)
        );
    }

    #[test]
    fn 空白鍵展開全部候選() {
        // 10 個不夠時用空白攤開——方向鍵四個都留給移動
        assert_eq!(
            lookup(Mode::Selecting, VK_SPACE.0 as u32),
            Some(Action::ExpandAllChars)
        );
    }

    #[test]
    fn 空白鍵是展開收合的開關() {
        // 同一個鍵開關：展開後再按一次就收回來。
        // 翻欄交給左右鍵，空白只管這一件事。
        assert_eq!(
            lookup(Mode::Selecting, VK_SPACE.0 as u32),
            Some(Action::ExpandAllChars)
        );
        assert_eq!(
            lookup(Mode::SelectingExpanded, VK_SPACE.0 as u32),
            Some(Action::CollapseChars)
        );
    }

    #[test]
    fn 展開後左右換欄上下同欄() {
        // 上下同欄移動
        assert_eq!(
            lookup(Mode::SelectingExpanded, VK_DOWN.0 as u32),
            Some(Action::NextCand)
        );
        assert_eq!(
            lookup(Mode::SelectingExpanded, VK_UP.0 as u32),
            Some(Action::PrevCand)
        );
        // 左右換欄
        assert_eq!(
            lookup(Mode::SelectingExpanded, VK_RIGHT.0 as u32),
            Some(Action::NextColumn)
        );
        assert_eq!(
            lookup(Mode::SelectingExpanded, VK_LEFT.0 as u32),
            Some(Action::PrevColumn)
        );
        // Esc 先收回展開
        assert_eq!(
            lookup(Mode::SelectingExpanded, VK_ESCAPE.0 as u32),
            Some(Action::CollapseChars)
        );
        // 數字鍵在展開狀態也要能選
        assert_eq!(
            lookup(Mode::SelectingExpanded, 0x31),
            Some(Action::PickChar(0))
        );
    }

    #[test]
    fn 兩種退出切法選單的方式() {
        // Enter：選中反白的切法，關選單但留在組字狀態（不是送出）
        assert_eq!(
            lookup(Mode::CuttingMenu, VK_RETURN.0 as u32),
            Some(Action::ConfirmCutting)
        );
        // TAB 與 esc：單純關掉選單，不取消組字
        assert_eq!(
            lookup(Mode::CuttingMenu, VK_TAB.0 as u32),
            Some(Action::CloseCuttingMenu)
        );
        assert_eq!(
            lookup(Mode::CuttingMenu, VK_ESCAPE.0 as u32),
            Some(Action::CloseCuttingMenu)
        );
        // 關掉之後才是送出
        assert_eq!(
            lookup(Mode::Typing, VK_RETURN.0 as u32),
            Some(Action::Commit)
        );
    }

    #[test]
    fn 打字中的空白是一聲不是控制鍵() {
        // 沒綁的話會放行給宿主，組字就被打斷了
        assert_eq!(
            lookup(Mode::Typing, VK_SPACE.0 as u32),
            Some(Action::Input(' '))
        );
        // 但切法選單開著時空白是往下選
        assert_eq!(
            lookup(Mode::CuttingMenu, VK_SPACE.0 as u32),
            Some(Action::NextCutting)
        );
    }

    #[test]
    fn 選字模式下數字是選字() {
        // 打字時 1 是注音鍵，選字時是「選第 1 個」
        assert_eq!(lookup(Mode::Typing, 0x31), Some(Action::Input('1')));
        assert_eq!(lookup(Mode::Selecting, 0x31), Some(Action::PickChar(0)));
        assert_eq!(lookup(Mode::Selecting, 0x39), Some(Action::PickChar(8)));
        // 0 不是選字鍵（1~9 而已），維持輸入
        assert_eq!(lookup(Mode::Selecting, 0x30), Some(Action::Input('0')));
    }

    #[test]
    fn 組字中沒綁定的鍵也要吃掉() {
        // F1、Home、End 都沒綁定，但組字中一律吃掉——
        // 放行的話游標會被移出組字區，組字就散了
        for vk in [0x70u32, 0x24, 0x23, 0x21, 0x22] {
            assert_eq!(
                lookup(Mode::Typing, vk),
                Some(Action::Swallow),
                "VK {vk:#x} 組字中該吃掉"
            );
            assert_eq!(lookup(Mode::CuttingMenu, vk), Some(Action::Swallow));
            assert_eq!(lookup(Mode::Selecting, vk), Some(Action::Swallow));
        }
        // 但沒組字時不能干擾宿主
        assert_eq!(lookup(Mode::Idle, 0x70), None);
        assert_eq!(lookup(Mode::Idle, 0x24), None);
    }

    #[test]
    fn 打字中方向鍵不會漏出去() {
        // 使用者回報：方向鍵會中斷組字。四個方向都要被接手。
        for vk in [VK_LEFT, VK_RIGHT, VK_UP, VK_DOWN] {
            assert!(
                lookup(Mode::Typing, vk.0 as u32).is_some(),
                "{vk:?} 該被接手"
            );
        }
    }
}
