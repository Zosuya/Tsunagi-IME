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
    /// 切法選單開著（按過 TAB）。
    ///
    /// **舊的整句選單**，目前沒有入口——TAB 進的是 `SegMenu`。
    /// 段選單實測沒問題之後這個模式連同 `session::cutting` 一起刪掉。
    CuttingMenu,
    /// **段選單**開著：反白引擎切出來的一段，選它要當成什麼。
    ///
    /// 跟 `CuttingMenu` 分開而不是加參數——方向鍵的意義完全不同：
    /// 整句選單只有上下（每列一整句），段選單左右換段、上下換解釋。
    SegMenu,
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
    /// 打開切法選單（TAB）
    OpenCuttingMenu,
    /// 選單開著時列出更多切法（空白鍵）
    ExpandCuttingMenu,
    /// 切法選單：下一個
    NextCutting,
    /// 切法選單：上一個
    PrevCutting,
    /// 選中反白的切法，關閉選單但留在組字狀態（Enter）
    ConfirmCutting,
    /// 單純關閉選單，切法維持原本選中的那個（TAB）
    CloseCuttingMenu,

    // ── 段選單（新的，取代切法選單；舊的先留著沒入口）──
    /// 打開段選單（TAB）
    OpenSegMenu,
    /// 關閉段選單，保留已定案的段
    CloseSegMenu,
    /// 反白往右一段
    SegRight,
    /// 反白往左一段
    SegLeft,
    /// 這一段的候選往下一個
    SegNextCand,
    /// 這一段的候選往上一個
    SegPrevCand,
    /// **選定這一段：前面定案、後面重算**（Enter）
    SegConfirm,
    /// 邊界往右推到下一個合法長度（`Shift+→`）
    SegWiden,
    /// 邊界往左收到上一個合法長度（`Shift+←`）
    SegNarrow,
    /// 直接挑第 N 個候選（數字鍵，0-based）
    SegPick(usize),
    /// 取消整個段選單，回到引擎自己算的分段
    SegReset,

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
    /// 切換全半形（`Ctrl+Shift+空白`）
    ToggleWidth,
    /// 輪替語言鎖定（`Shift+空白`）：自動 → 注音 → 日文 → 英文 → 自動。
    ///
    /// **原本是「單按 Ctrl」**（2026-08-30～2026-09-09）。換掉的理由是
    /// 誤觸與軟體衝突：單按的判斷要靠一整套旗標去分辨「真的只按了
    /// Ctrl」與「Ctrl+C 的 Ctrl」，那套邏輯踩過兩次坑，而且某些宿主
    /// 自己需要單按 Ctrl 時會被輸入法吃掉。
    ///
    /// 換成明確的組合鍵就沒有這層猜測。鍵位是實測選的（見
    /// `keyprobe`）——`Ctrl+Shift`、`Ctrl+空白`、`Alt+Shift` 都被系統
    /// 攔走，`Shift+空白` 兩邊平台都到得了。
    CycleLock,

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
    /// `Ctrl+Shift+鍵`。
    ///
    /// **目前只有全半形用它**。Ctrl 系組合原則上留給宿主（那些是應用
    /// 程式的快捷鍵），這是刻意開的例外——`Shift+空白` 讓給語言鎖定
    /// 之後，全半形要一個兩邊平台都到得了的位子，實測只有這個組合
    /// 過關。代價是宿主原本綁在這個組合上的功能會被吃掉（VSCode 的
    /// 參數提示），使用者已確認可接受。
    pub const fn ctrl_shift(vk: u32) -> Self {
        Self {
            vk,
            shift: true,
            ctrl: true,
        }
    }
}

/// Ctrl 現在按著嗎？
///
/// `lookup` 用它組出要查表的 `Combo`。Ctrl 系組合原則上留給宿主，
/// 綁定表裡有的例外（目前只有 `Ctrl+Shift+空白`），見 `defer_to_host`。
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
/// **用途在 2026-09-09 換了，判準本身一字不動**（那條規則已經錯過
/// 三次，見開發文件 §2.44）。原本是給 `CtrlTap` 用的——重複的 Ctrl
/// key-down 會把「這一輪用過了」洗掉，複製貼上就變成切換語言。
///
/// 現在是給**輪替型的動作**擋自動重複：`Shift+空白`（語言鎖定）與
/// `Ctrl+Shift+空白`（全半形）按住不放，系統會連續重送 key-down，
/// 不擋的話語言會瘋狂輪替。原本的單按 Ctrl 沒這個問題——它在放開時
/// 才觸發，而放開只會發生一次。
fn is_repeat_bits(lparam: isize) -> bool {
    lparam & 0x4000_0000 != 0
}

/// 同上，收 TSF 傳來的 `LPARAM`。
pub fn is_repeat(lparam: windows::Win32::Foundation::LPARAM) -> bool {
    is_repeat_bits(lparam.0)
}

/// 鍵位表。**這是唯一的一份，不開放使用者自訂**。
///
/// 要調鍵位就改這裡——集中成一張表正是為了「只動一個地方」。
///
/// # 為什麼不做自訂（2026-08-31 決定）
///
/// 這張表已經很擠：十個數字全是注音鍵、方向鍵在選字、Tab 開切法
/// 選單、Shift+空白輪替語言鎖定、Ctrl+Shift+空白切全半形、↑↑↓↓ 是
/// 指令手勢。
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
    // TAB 開**段選單**（新的）。舊的整句選單留在程式碼裡但沒有入口，
    // 段選單實測沒問題之後一起刪。
    (Mode::Typing, Combo::plain(VK_TAB.0 as u32),    Action::OpenSegMenu),
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
    // **Shift+空白輪替語言鎖定**：自動 → 注音 → 日文 → 英文 → 自動。
    // 沒組字時也要能切——使用者常是先設好模式再開始打。
    //
    // 這個位子原本是全半形的，2026-09-09 讓給語言鎖定（換掉單按 Ctrl，
    // 見 `Action::CycleLock`），全半形搬到 `Ctrl+Shift+空白`。
    (Mode::Typing, Combo::shift(VK_SPACE.0 as u32),  Action::CycleLock),
    (Mode::Idle,   Combo::shift(VK_SPACE.0 as u32),  Action::CycleLock),
    (Mode::Selecting, Combo::shift(VK_SPACE.0 as u32), Action::CycleLock),
    // Ctrl+Shift+空白切換全半形。三態輪流：自動 → 半形 → 全形。
    (Mode::Typing, Combo::ctrl_shift(VK_SPACE.0 as u32),  Action::ToggleWidth),
    (Mode::Idle,   Combo::ctrl_shift(VK_SPACE.0 as u32),  Action::ToggleWidth),
    (Mode::Selecting, Combo::ctrl_shift(VK_SPACE.0 as u32), Action::ToggleWidth),
    // **日文詞界調整**（文節伸縮）。日文 IME 的通用慣例，而且我們的
    // Shift+方向鍵本來就是空的。只在選字時有意義——那時框停在某一格上，
    // 「把這一格拉長／縮短」才有指涉對象。見 `Session::widen_word`。
    (Mode::Selecting, Combo::shift(VK_RIGHT.0 as u32), Action::WidenWord),
    (Mode::Selecting, Combo::shift(VK_LEFT.0 as u32),  Action::NarrowWord),

    // ── 切法選單 ──
    //
    // **空白鍵是展開**（使用者指定的）：選單裡最想要的動作是「還有
    // 別的嗎」，那顆最大的鍵就給它。移動交給方向鍵。
    // Shift+空白在選單裡不綁——它在別的模式是切全半形，不該在這裡
    // 變成第三種意思。
    (Mode::CuttingMenu, Combo::plain(VK_SPACE.0 as u32),  Action::ExpandCuttingMenu),
    // TAB 在選單裡是「單純關掉選單」，不選也不送出。
    (Mode::CuttingMenu, Combo::plain(VK_TAB.0 as u32),    Action::CloseCuttingMenu),
    // **上下移動，不用左右**——選單是一直排的（切法每列是一整句），
    // 左右在這裡沒有意義。走到最後一列再往下仍會自動展開，
    // 見 `Session::next_cutting`。
    (Mode::CuttingMenu, Combo::plain(VK_DOWN.0 as u32),   Action::NextCutting),
    (Mode::CuttingMenu, Combo::plain(VK_UP.0 as u32),     Action::PrevCutting),
    // Enter 是「就選反白這個切法」——關掉選單但**留在組字狀態**，
    // 不是送出。送出要再按一次 Enter（那時已經是 Typing 模式）。
    (Mode::CuttingMenu, Combo::plain(VK_RETURN.0 as u32), Action::ConfirmCutting),
    // esc 跟 TAB 一樣是關掉選單，不是取消組字——
    // 使用者只是不想選了，字還在打
    (Mode::CuttingMenu, Combo::plain(VK_ESCAPE.0 as u32), Action::CloseCuttingMenu),
    (Mode::CuttingMenu, Combo::plain(VK_BACK.0 as u32),   Action::Backspace),

    // ── 段選單 ──
    //
    // **跟選字同一組手勢**，只是反白的是「段」不是「格」：
    //
    // | 鍵 | 段選單 | 選字 |
    // |---|---|---|
    // | ←→ | 換一段 | 換一格 |
    // | ↑↓ | 換這段的解釋 | 換這格的字 |
    // | Shift+←→ | 推這段的邊界 | 推日文詞界 |
    // | 1-9 | 直接挑 | 直接挑 |
    //
    // 使用者不必學新東西——同一個心智模型，換一個粒度。
    (Mode::SegMenu, Combo::plain(VK_RIGHT.0 as u32),  Action::SegRight),
    (Mode::SegMenu, Combo::plain(VK_LEFT.0 as u32),   Action::SegLeft),
    (Mode::SegMenu, Combo::plain(VK_DOWN.0 as u32),   Action::SegNextCand),
    (Mode::SegMenu, Combo::plain(VK_UP.0 as u32),     Action::SegPrevCand),
    // **Shift+←→ 推邊界**：跳過清單直接改長度。跟選字的日文詞界調整
    // （`WidenWord`／`NarrowWord`）是同一個手勢、同一個概念。
    (Mode::SegMenu, Combo::shift(VK_RIGHT.0 as u32),  Action::SegWiden),
    (Mode::SegMenu, Combo::shift(VK_LEFT.0 as u32),   Action::SegNarrow),
    // Enter 是「選定這一段」——前面定案、後面重算，反白自動移到下一段。
    // **不是送出**：送出要先關掉選單回到 Typing。
    (Mode::SegMenu, Combo::plain(VK_RETURN.0 as u32), Action::SegConfirm),
    // TAB 關掉選單，已經定案的段留著
    (Mode::SegMenu, Combo::plain(VK_TAB.0 as u32),    Action::CloseSegMenu),
    // **Esc 是「全部重來」**，不是關選單——關選單有 TAB 了，而定案
    // 之後沒有別的後悔藥（逐段撤銷會讓狀態機複雜一倍）。
    (Mode::SegMenu, Combo::plain(VK_ESCAPE.0 as u32), Action::SegReset),
    (Mode::SegMenu, Combo::plain(VK_BACK.0 as u32),   Action::Backspace),

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
    // 選字中按 TAB 也是開**段選單**（跟打字中一致）。舊的整句選單
    // 留在程式碼裡但沒有入口。
    (Mode::Selecting, Combo::plain(VK_TAB.0 as u32),    Action::OpenSegMenu),
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
    // **段選單也一樣**：那時在挑「這一段是什麼」，不會再輸入注音，
    // 數字就空出來了。跟選字同一個道理。
    if mode == Mode::SegMenu {
        if let Some(d) = (0x31..=0x39u32).contains(&vk).then(|| (vk - 0x31) as usize) {
            return Some(Action::SegPick(d));
        }
        if let Some(d) = (0x61..=0x69u32).contains(&vk).then(|| (vk - 0x61) as usize) {
            return Some(Action::SegPick(d));
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
    let ctrl = ctrl_down();
    // ★ 按著 Ctrl 時字元鍵不是輸入 ★
    //
    // **這一段一定要在 `typed_char` 之前。** `typed_char` 只看 Shift，
    // 它不知道 Ctrl 按著——`Ctrl+C` 問它會老實回答 `'c'`，於是 `lookup`
    // 回 `Some(Input('c'))`，而 `defer_to_host` 看的正是「`lookup` 是不是
    // `None`」。結果整組 `Ctrl+…` 都被判定成「綁定表裡有」，不讓回宿主，
    // 複製貼上全部失效。
    //
    // 這是 2026-09-09 換鍵位的連帶損害：以前 Ctrl 走 `CtrlTap` 的專門
    // 路徑，退休之後就掉進一般字元鍵的順序裡了。
    //
    // 例外走下面的綁定表（目前只有 `Ctrl+Shift+空白` 全半形）——那是
    // 刻意收編的組合，見 `Combo::ctrl_shift`。
    if !ctrl {
        // 字元鍵優先——它跟模式無關，隨時可以繼續打字。
        // `typed_char` 自己會看 Shift（Shift+1 是 `!`）。
        if let Some(ch) = typed_char(vk) {
            return Some(Action::Input(ch));
        }
    }
    // Idle 只有空白鍵有綁定（注音的一聲），其餘放行給宿主
    let combo = Combo {
        vk,
        shift: shift_down(),
        ctrl,
    };
    DEFAULT_BINDINGS
        .iter()
        .find(|(m, c, _)| *m == mode && *c == combo)
        .map(|(_, _, a)| *a)
        // Shift 版本沒綁的話退回無 Shift 版本——
        // 使用者按 Shift+Enter 應該還是送出。
        //
        // **按著 Ctrl 時不走這條退路**：`Ctrl+Shift+X` 沒綁的話應該
        // 放行給宿主（那是應用程式的快捷鍵），退回去會誤中 `X` 的
        // 綁定，把一堆宿主快捷鍵吃掉。
        .or_else(|| {
            if ctrl {
                return None;
            }
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
        //
        // **按著 Ctrl 的組合也不在此列**：那些是宿主的快捷鍵，組字中
        // 按 `Ctrl+C` 應該複製，不該被吃掉。組字散不散由宿主自己負責
        // ——它本來就知道自己那個快捷鍵會做什麼。
        .or(if mode == Mode::Idle || ctrl {
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
    use super::*;

    /// 自動重複的判準。**這條規則已經錯過三次**（見開發文件 §2.44），
    /// 用測試釘住——現在擋的是「按著 Shift+空白 不放會連續輪替語言」。
    #[test]
    fn 自動重複的位元判得出來() {
        assert!(!is_repeat_bits(0x0000_0001), "第一次按下");
        assert!(is_repeat_bits(0x4000_0001), "按著不放的重複");
    }

    /// 找出綁定表裡某個組合對應的動作。
    ///
    /// `lookup` 讀的是**真實鍵盤狀態**（`shift_down()`／`ctrl_down()`），
    /// 單元測試按不出 Shift，所以這裡直接查表——這次改的規則正是
    /// 綁定表本身，查表就測得到。
    fn 查表(mode: Mode, combo: Combo) -> Option<Action> {
        DEFAULT_BINDINGS
            .iter()
            .find(|(m, c, _)| *m == mode && *c == combo)
            .map(|(_, _, a)| *a)
    }

    /// 2026-09-09 換鍵位：`Shift+空白` 從全半形改成語言鎖定輪替，
    /// 全半形搬到 `Ctrl+Shift+空白`。單按 Ctrl 整個退休。
    ///
    /// 鍵位是實測選的——`Ctrl+Shift`、`Ctrl+空白`、`Alt+Shift` 都被
    /// 系統攔走（見 `keyprobe` 與開發文件 §4.17）。
    mod 語言鎖定換鍵位 {
        use super::*;
        const 空白: u32 = 0x20;

        #[test]
        fn shift空白是語言輪替() {
            for mode in [Mode::Typing, Mode::Idle, Mode::Selecting] {
                assert_eq!(
                    查表(mode, Combo::shift(空白)),
                    Some(Action::CycleLock),
                    "{mode:?} 的 Shift+空白 應該輪替語言鎖定"
                );
            }
        }

        #[test]
        fn ctrl_shift空白是全半形() {
            for mode in [Mode::Typing, Mode::Idle, Mode::Selecting] {
                assert_eq!(
                    查表(mode, Combo::ctrl_shift(空白)),
                    Some(Action::ToggleWidth),
                    "{mode:?} 的 Ctrl+Shift+空白 應該切全半形"
                );
            }
        }

        /// **兩個動作不能綁在同一個組合上**——這正是這次換鍵位的重點，
        /// 綁重了會有一個永遠觸發不到。
        #[test]
        fn 兩個動作沒有綁在同一個組合() {
            for mode in [Mode::Typing, Mode::Idle, Mode::Selecting] {
                let 輪替 = 查表(mode, Combo::shift(空白));
                let 全半形 = 查表(mode, Combo::ctrl_shift(空白));
                assert_ne!(輪替, 全半形, "{mode:?} 兩個組合不該對到同一個動作");
            }
        }

        /// 打字中的裸空白仍然是注音的一聲——換鍵位不該動到它。
        #[test]
        fn 裸空白還是注音一聲() {
            assert_eq!(
                查表(Mode::Typing, Combo::plain(空白)),
                Some(Action::Input(' ')),
                "空白鍵在打字中是一聲，不是控制鍵"
            );
        }

        /// 全半形只剩 `Ctrl+Shift+空白` 一個入口，舊的 `Shift+空白`
        /// 不該還留著（留著就會蓋掉語言輪替）。
        #[test]
        fn 全半形沒有第二個入口() {
            let 全半形入口: Vec<_> = DEFAULT_BINDINGS
                .iter()
                .filter(|(_, _, a)| *a == Action::ToggleWidth)
                .map(|(m, c, _)| (*m, *c))
                .collect();
            assert!(
                全半形入口.iter().all(|(_, c)| c.ctrl && c.shift),
                "全半形只該綁在 Ctrl+Shift 組合上，實際：{全半形入口:?}"
            );
        }
    }

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
        // TAB 開**段選單**（舊的整句選單留在程式碼裡但沒有入口）
        let tab = VK_TAB.0 as u32;
        assert_eq!(lookup(Mode::Typing, tab), Some(Action::OpenSegMenu));
        let down = VK_DOWN.0 as u32;
        assert_eq!(lookup(Mode::Typing, down), Some(Action::Gesture(Dir::Down)));
        assert_eq!(lookup(Mode::CuttingMenu, down), Some(Action::NextCutting));
        assert_eq!(lookup(Mode::SegMenu, down), Some(Action::SegNextCand));
        assert_eq!(lookup(Mode::Selecting, down), Some(Action::NextCand));
    }

    /// 段選單跟選字**同一組手勢**，只是粒度不同（段 vs 格）。
    #[test]
    fn 段選單的方向鍵跟選字同一套() {
        let (left, right) = (VK_LEFT.0 as u32, VK_RIGHT.0 as u32);
        // 左右換段
        assert_eq!(lookup(Mode::SegMenu, left), Some(Action::SegLeft));
        assert_eq!(lookup(Mode::SegMenu, right), Some(Action::SegRight));
        // 上下換這一段的解釋
        assert_eq!(
            lookup(Mode::SegMenu, VK_UP.0 as u32),
            Some(Action::SegPrevCand)
        );
        // Enter 是「選定這一段」，不是送出
        assert_eq!(
            lookup(Mode::SegMenu, VK_RETURN.0 as u32),
            Some(Action::SegConfirm)
        );
        // Esc 是全部重來（關選單有 TAB）
        assert_eq!(
            lookup(Mode::SegMenu, VK_ESCAPE.0 as u32),
            Some(Action::SegReset)
        );
        assert_eq!(
            lookup(Mode::SegMenu, VK_TAB.0 as u32),
            Some(Action::CloseSegMenu)
        );
    }

    /// 數字鍵在段選單是直接挑候選——跟選字同一個道理（那時不會再
    /// 輸入注音，十個數字就空出來了）。
    #[test]
    fn 段選單的數字鍵是挑候選() {
        assert_eq!(lookup(Mode::SegMenu, 0x31), Some(Action::SegPick(0)));
        assert_eq!(lookup(Mode::SegMenu, 0x39), Some(Action::SegPick(8)));
        // 數字鍵盤也一樣
        assert_eq!(lookup(Mode::SegMenu, 0x61), Some(Action::SegPick(0)));
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
    fn 切法選單的空白是展開不是往下() {
        // 使用者定的：選單裡最想要的動作是「還有別的嗎」，
        // 那顆最大的鍵就給展開。移動交給上下鍵。
        assert_eq!(
            lookup(Mode::CuttingMenu, VK_SPACE.0 as u32),
            Some(Action::ExpandCuttingMenu)
        );
        assert_eq!(
            lookup(Mode::CuttingMenu, VK_DOWN.0 as u32),
            Some(Action::NextCutting)
        );
        assert_eq!(
            lookup(Mode::CuttingMenu, VK_UP.0 as u32),
            Some(Action::PrevCutting)
        );
    }

    #[test]
    fn 切法選單不用左右鍵() {
        // 選單是一直排的（每列是一整句），左右在這裡沒有意義。
        // 但**仍然要吃掉**——放行給宿主會把游標移出組字區。
        for vk in [VK_LEFT.0 as u32, VK_RIGHT.0 as u32] {
            assert_eq!(
                lookup(Mode::CuttingMenu, vk),
                Some(Action::Swallow),
                "左右鍵在切法選單裡該吃掉不做事"
            );
        }
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
        // 但切法選單開著時空白是展開更多，見 `切法選單的空白是展開不是往下`
        assert_eq!(
            lookup(Mode::CuttingMenu, VK_SPACE.0 as u32),
            Some(Action::ExpandCuttingMenu)
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

    /// 2026-09-09 實測回報：**複製貼上全部失效**。
    ///
    /// 根因是 `lookup` 的「字元鍵優先」排在 Ctrl 判斷之前，而
    /// `typed_char` 只看 Shift——`Ctrl+C` 問它會回 `'c'`，`lookup` 就
    /// 回 `Some(Input('c'))`，於是 `defer_to_host`（它看的是 `lookup`
    /// 是不是 `None`）判定「這個組合我們有綁」，不讓回宿主。
    ///
    /// 換鍵位（`ca016b1`）之前 Ctrl 走 `CtrlTap` 的專門路徑，不會掉進
    /// 字元鍵的順序裡；退休之後才踩到。
    mod ctrl系組合要讓回宿主 {
        use super::*;

        /// 測試按不出真的 Ctrl（`lookup` 讀的是實體鍵盤），所以這裡
        /// 驗的是**修好之後仍然成立的那一半**：沒按 Ctrl 時字元鍵照樣
        /// 是輸入。Ctrl 那半靠 `defer_to_host` 的邏輯與實機驗收。
        #[test]
        fn 沒按ctrl時字母照樣進組字() {
            for (vk, ch) in [(0x43u32, 'c'), (0x56, 'v'), (0x41, 'a')] {
                assert_eq!(
                    lookup(Mode::Typing, vk),
                    Some(Action::Input(ch)),
                    "沒按 Ctrl 時 {ch} 該進組字"
                );
            }
        }

        /// `Ctrl+Shift+空白`（全半形）是**刻意收編**的例外，必須留在
        /// 綁定表裡——修 Ctrl 放行時別把它一起放掉了。
        #[test]
        fn 全半形那個例外還在表裡() {
            let combo = Combo::ctrl_shift(0x20);
            assert!(
                DEFAULT_BINDINGS
                    .iter()
                    .any(|(_, c, a)| *c == combo && matches!(a, Action::ToggleWidth)),
                "Ctrl+Shift+空白 該還綁著全半形"
            );
        }
    }
}
