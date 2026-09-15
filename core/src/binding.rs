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
//! - **按鍵序列（「秘笈」式）的偵測有地方放**——那需要一個看得到
//!   每一個按鍵的中心點，散在 `if-else` 裡做不到
//!
//! # 為什麼在 core（2026-09-13 從 `platform/windows/src/keymap.rs` 上搬）
//!
//! 原本整張表寫的是 Windows 的 `VK_*` 鍵碼，macOS 那邊只好照著表**人工
//! 抄一份** `match keyCode`，抄漏過三處（見開發文件 §2.52.32）。
//!
//! 現在表裡寫的是中性的 [`Key`]，各平台只負責一件事：把自己的鍵碼翻成
//! `Key`、讀好修飾鍵，填成 [`KeyEvent`] 來查。**跟「這個平台的鍵盤怎麼
//! 回報」有關的留在平台，跟「這顆鍵在這個模式要做什麼」有關的在這裡。**
//!
//! 附帶的好處：`lookup` 不再自己去讀實體鍵盤，測試終於按得出 Shift。
//!
//! # 模式決定同一個鍵做什麼
//!
//! 空白鍵在「打字中」是注音的一聲，在「切法選單開著」是往下選。
//! 所以查表要看模式，不能只看鍵。

pub use crate::command::Dir;

/// 一顆鍵的中性名稱。平台層把自己的鍵碼翻成這個再來查表。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// 印得出來的字元，**平台已經套過 Shift 與鍵盤配置**（`Shift+1` 就是 `'!'`）。
    ///
    /// 空白鍵**不在這裡**，走 [`Key::Space`]——它在打字中是注音的一聲、
    /// 在選字中是展開，要看模式決定，不能一律當輸入。
    Char(char),
    /// 數字鍵盤打出的字元（數字與 `+ - * / .`）。
    ///
    /// **跟 `Char` 分開**：主鍵盤那排的 `5` 是注音的ㄓ，數字鍵盤的 `5`
    /// 就該是 `5`——那正是它存在的意義。
    Numpad(char),
    Space,
    /// 主鍵盤的 Return 與數字鍵盤的 Enter 都是這個
    Enter,
    Tab,
    Esc,
    Backspace,
    Left,
    Right,
    Up,
    Down,
    /// 其他（Home、End、F1……）。沒有綁定，但**組字中一律吃掉**，
    /// 靠的就是它們也翻得成一個 `Key`。見 [`lookup`]。
    Other,
}

/// 一次按鍵：哪顆鍵，加上當下的修飾鍵。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    pub shift: bool,
    /// **主修飾鍵**：Windows 是 Ctrl，macOS 是 Cmd。
    ///
    /// macOS 的 `Ctrl+…` 根本到不了輸入法（開發文件 §2.52.46），所以
    /// 「Ctrl+Shift+空白切全半形」在那邊只能是 `Cmd+Shift+空白`。表裡
    /// 寫「主修飾鍵」，由平台決定它是哪一顆，表就不必分兩份。
    pub primary: bool,
}

impl KeyEvent {
    /// 沒按修飾鍵
    pub const fn plain(key: Key) -> Self {
        Self {
            key,
            shift: false,
            primary: false,
        }
    }
}

/// 輸入法目前在做什麼。同一個鍵在不同模式下的意義不同。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// 沒有組字——按鍵一律放行給宿主
    #[default]
    Idle,
    /// 正在打字，預覽列顯示第一名
    Typing,
    /// **段選單**開著（按過 TAB）：反白引擎切出來的一段，選它要當成什麼。
    ///
    /// 取代了舊的整句選單（每列一整句、只有上下），2026-09-15 兩平台
    /// 回歸實測通過後刪掉。
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
    /// **刪掉反白的那一格**（選字模式）。
    ///
    /// 跟 `DeleteSeg` 分開是因為兩者的設定與粒度都不同（格 vs 段），
    /// 見 `config::Behavior::delete_marked_cell`。
    DeleteCell,
    /// **刪掉反白的那一個單位**（段選單裡是一段）。
    ///
    /// 跟 `Backspace` 分開是因為兩者要能並存——設定
    /// `DeleteUnitKey::ShiftBackspace` 時倒退鍵維持刪單鍵，
    /// `Shift+`倒退鍵才刪整段。哪顆鍵送出這個動作由綁定表決定，
    /// **設定的三態在動作內部判斷**（見平台層的 `DeleteSeg` 分派）。
    DeleteSeg,
    /// 取消組字
    Cancel,
    /// 送出目前的文字
    Commit,

    // ── 段選單 ──
    /// 打開段選單（TAB）
    OpenSegMenu,
    /// 關閉段選單，保留已定案的段（TAB 與 Esc）
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
    /// 切換全半形（`主修飾鍵+Shift+空白`）
    ToggleWidth,
    /// 輪替語言鎖定（`Shift+空白`）：自動 → 注音 → 日文 → 英文 → 自動。
    ///
    /// **原本是「單按 Ctrl」**（2026-08-30～2026-09-09）。換掉的理由是
    /// 誤觸與軟體衝突：單按的判斷要靠一整套旗標去分辨「真的只按了
    /// Ctrl」與「Ctrl+C 的 Ctrl」，那套邏輯踩過兩次坑，而且某些宿主
    /// 自己需要單按 Ctrl 時會被輸入法吃掉。
    ///
    /// 換成明確的組合鍵就沒有這層猜測。鍵位是實測選的（見 Windows 的
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
    pub key: Key,
    pub shift: bool,
    pub primary: bool,
}

impl Combo {
    pub const fn plain(key: Key) -> Self {
        Self {
            key,
            shift: false,
            primary: false,
        }
    }
    pub const fn shift(key: Key) -> Self {
        Self {
            key,
            shift: true,
            primary: false,
        }
    }
    /// `主修飾鍵+Shift+鍵`（Windows 的 `Ctrl+Shift`、macOS 的 `Cmd+Shift`）。
    ///
    /// **目前只有全半形用它**。主修飾鍵系組合原則上留給宿主（那些是應用
    /// 程式的快捷鍵），這是刻意開的例外——`Shift+空白` 讓給語言鎖定
    /// 之後，全半形要一個兩邊平台都到得了的位子，實測只有這個組合
    /// 過關。代價是宿主原本綁在這個組合上的功能會被吃掉（VSCode 的
    /// 參數提示），使用者已確認可接受。
    pub const fn primary_shift(key: Key) -> Self {
        Self {
            key,
            shift: true,
            primary: true,
        }
    }
}

/// 鍵位表。**這是唯一的一份，不開放使用者自訂**。
///
/// 要調鍵位就改這裡——集中成一張表正是為了「只動一個地方」。
///
/// # 為什麼不做自訂（2026-08-31 決定）
///
/// 這張表已經很擠：十個數字全是注音鍵、方向鍵在選字、Tab 開段選單、
/// Shift+空白輪替語言鎖定、主修飾鍵+Shift+空白切全半形、↑↑↓↓ 是
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
    (Mode::Typing, Combo::plain(Key::Backspace), Action::Backspace),
    (Mode::Typing, Combo::plain(Key::Esc),       Action::Cancel),
    (Mode::Typing, Combo::plain(Key::Enter),     Action::Commit),
    // TAB 開**段選單**（新的）。舊的整句選單留在程式碼裡但沒有入口，
    // 段選單實測沒問題之後一起刪。
    (Mode::Typing, Combo::plain(Key::Tab),       Action::OpenSegMenu),
    // 上下鍵走手勢偵測：湊滿「上上下下」且組字內容是指令就執行，
    // 否則退回原本的「進選字」。見 `command::Gesture`。
    (Mode::Typing, Combo::plain(Key::Down),      Action::Gesture(Dir::Down)),
    (Mode::Typing, Combo::plain(Key::Up),        Action::Gesture(Dir::Up)),
    // 左右鍵在打字中也要吃掉——組字時**所有按鍵都歸輸入法管**，
    // 放行的話游標會跑到組字區外面，組字就散了。
    // 左鍵進選字要從**最後一格**開始——使用者按左鍵的直覺是
    // 「從右邊選過來」，從第一格進來會看起來像跳過了最後一個字
    (Mode::Typing, Combo::plain(Key::Left),      Action::EnterSelectLast),
    (Mode::Typing, Combo::plain(Key::Right),     Action::EnterSelect),
    // **空白鍵在打字中是注音的一聲**，要當成輸入而不是控制鍵。
    // 沒綁的話會放行給宿主，組字就被打斷了。
    //
    // 但沒組字時（`Mode::Idle`）不綁——那時候空白就是空白。
    // 一聲必須前面已經有構成合法注音的鍵，不會憑空從空白開始。
    (Mode::Typing, Combo::plain(Key::Space),     Action::Input(' ')),
    // **Shift+空白輪替語言鎖定**：自動 → 注音 → 日文 → 英文 → 自動。
    // 沒組字時也要能切——使用者常是先設好模式再開始打。
    //
    // 這個位子原本是全半形的，2026-09-09 讓給語言鎖定（換掉單按 Ctrl，
    // 見 `Action::CycleLock`），全半形搬到 `主修飾鍵+Shift+空白`。
    (Mode::Typing,    Combo::shift(Key::Space), Action::CycleLock),
    (Mode::Idle,      Combo::shift(Key::Space), Action::CycleLock),
    (Mode::Selecting, Combo::shift(Key::Space), Action::CycleLock),
    // 主修飾鍵+Shift+空白切換全半形。三態輪流：自動 → 半形 → 全形。
    (Mode::Typing,    Combo::primary_shift(Key::Space), Action::ToggleWidth),
    (Mode::Idle,      Combo::primary_shift(Key::Space), Action::ToggleWidth),
    (Mode::Selecting, Combo::primary_shift(Key::Space), Action::ToggleWidth),
    // **日文詞界調整**（文節伸縮）。日文 IME 的通用慣例，而且我們的
    // Shift+方向鍵本來就是空的。只在選字時有意義——那時框停在某一格上，
    // 「把這一格拉長／縮短」才有指涉對象。見 `Session::widen_word`。
    (Mode::Selecting, Combo::shift(Key::Right), Action::WidenWord),
    (Mode::Selecting, Combo::shift(Key::Left),  Action::NarrowWord),

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
    (Mode::SegMenu, Combo::plain(Key::Right), Action::SegRight),
    (Mode::SegMenu, Combo::plain(Key::Left),  Action::SegLeft),
    (Mode::SegMenu, Combo::plain(Key::Down),  Action::SegNextCand),
    (Mode::SegMenu, Combo::plain(Key::Up),    Action::SegPrevCand),
    // **Shift+←→ 推邊界**：跳過清單直接改長度。跟選字的日文詞界調整
    // （`WidenWord`／`NarrowWord`）是同一個手勢、同一個概念。
    (Mode::SegMenu, Combo::shift(Key::Right), Action::SegWiden),
    (Mode::SegMenu, Combo::shift(Key::Left),  Action::SegNarrow),
    // Enter 是「選定這一段」——前面定案、後面重算，反白自動移到下一段。
    // **不是送出**：送出要先關掉選單回到 Typing。
    (Mode::SegMenu, Combo::plain(Key::Enter), Action::SegConfirm),
    // **TAB 與 Esc 都是關掉選單，已經定案的段留著**。
    //
    // Esc 原本是「全部重來」（丟掉已定案的段、交回引擎重切），2026-09-13
    // 使用者裁定統一成 macOS 那邊的「保留」——兩個平台同一顆鍵不該做
    // 不同的事，而鍵位表只有一份，也只能有一個答案。要重來就關掉
    // 選單、再按一次 Esc 取消組字重打。
    (Mode::SegMenu, Combo::plain(Key::Tab),   Action::CloseSegMenu),
    (Mode::SegMenu, Combo::plain(Key::Esc),   Action::CloseSegMenu),
    // **倒退鍵與 `Shift+`倒退鍵都送同一個動作**，三態設定在動作內部
    // 判斷（`DeleteUnitKey`）——綁定表分不出設定值，而兩個入口的
    // 判斷必須一致（Windows 的 `OnTestKeyDown` 也查這張表）。設定成
    // `Off` 或「另一顆鍵才是刪整段」時，動作自己退回刪單鍵。
    (Mode::SegMenu, Combo::plain(Key::Backspace), Action::DeleteSeg),
    (Mode::SegMenu, Combo::shift(Key::Backspace), Action::DeleteSeg),

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
    (Mode::Selecting, Combo::plain(Key::Left),  Action::SelectLeft),
    (Mode::Selecting, Combo::plain(Key::Right), Action::SelectRight),
    // 上下鍵在候選字清單裡移動反白
    (Mode::Selecting, Combo::plain(Key::Down),  Action::NextCand),
    (Mode::Selecting, Combo::plain(Key::Up),    Action::PrevCand),
    // 選字中按 TAB 也是開**段選單**（跟打字中一致）。舊的整句選單
    // 留在程式碼裡但沒有入口。
    (Mode::Selecting, Combo::plain(Key::Tab),   Action::OpenSegMenu),
    // 倒退鍵與 `Shift+`倒退鍵都送 `DeleteCell`，三態設定在動作內部判斷
    // （理由同 `SegMenu` 那兩行）
    (Mode::Selecting, Combo::plain(Key::Backspace), Action::DeleteCell),
    (Mode::Selecting, Combo::shift(Key::Backspace), Action::DeleteCell),
    // **空白鍵展開全部候選**——10 個不夠時攤開來找
    (Mode::Selecting, Combo::plain(Key::Space), Action::ExpandAllChars),
    // Enter 是「選中反白的候選字」，不是送出——
    // 送出要先 esc 退出選字選單，回到 Typing 再按 Enter
    (Mode::Selecting, Combo::plain(Key::Enter), Action::ConfirmCand),
    (Mode::Selecting, Combo::plain(Key::Esc),   Action::Cancel),

    // ── 選字中且已展開全部（多欄）──
    //
    // 上下同欄移動、左右換欄——使用者定的。
    (Mode::SelectingExpanded, Combo::plain(Key::Down),  Action::NextCand),
    (Mode::SelectingExpanded, Combo::plain(Key::Up),    Action::PrevCand),
    (Mode::SelectingExpanded, Combo::plain(Key::Right), Action::NextColumn),
    (Mode::SelectingExpanded, Combo::plain(Key::Left),  Action::PrevColumn),
    // **空白鍵是展開／收合的開關**——展開後再按一次就收回來。
    // 翻欄交給左右鍵，空白只管這一件事，語意才單純。
    (Mode::SelectingExpanded, Combo::plain(Key::Space), Action::CollapseChars),
    (Mode::SelectingExpanded, Combo::plain(Key::Enter), Action::ConfirmCand),
    // Esc 先收回展開，再按一次才離開選字。
    //
    // 跟空白鍵通到同一個動作，但語意不同——空白是「開關」，
    // Esc 是「退回上一層」。兩條路殊途同歸在輸入法裡很常見。
    (Mode::SelectingExpanded, Combo::plain(Key::Esc),       Action::CollapseChars),
    (Mode::SelectingExpanded, Combo::plain(Key::Backspace), Action::DeleteCell),
    (Mode::SelectingExpanded, Combo::shift(Key::Backspace), Action::DeleteCell),
];

/// 挑候選用的數字：主鍵盤或數字鍵盤的 1～9，回 0-based 的序號。
///
/// **只認 `'1'..='9'` 這幾個字元**。`Shift+1` 平台翻出來是 `'!'`，
/// 所以選字中按 `Shift+數字` 會變成輸入符號而不是挑候選（2026-09-13
/// 使用者接受；原本 Windows 看的是鍵碼、不管 Shift，macOS 本來就是這樣）。
fn pick_digit(key: Key) -> Option<usize> {
    let (Key::Char(c) | Key::Numpad(c)) = key else {
        return None;
    };
    c.to_digit(10)
        .filter(|d| (1..=9).contains(d))
        .map(|d| d as usize - 1)
}

/// 這個按鍵在這個模式下要做什麼？沒綁定就回 `None`（放行給宿主）。
pub fn lookup(mode: Mode, ev: KeyEvent) -> Option<Action> {
    // **選字模式下數字是選字，不是輸入**。
    //
    // 打字時十個數字全都是注音鍵，不能拿來選候選（那會讓 r4 打不出來）。
    // 但選字模式已經在挑字了，不會再輸入注音，數字就空出來了——
    // 這跟新注音的行為一致。數字鍵盤也能選——**它本來就不是注音鍵**，
    // 沒有衝突，這是主鍵盤那排做不到的事（見 `DEFAULT_BINDINGS` 的說明）。
    if matches!(mode, Mode::Selecting | Mode::SelectingExpanded) {
        if let Some(d) = pick_digit(ev.key) {
            return Some(Action::PickChar(d));
        }
    }
    // **段選單也一樣**：那時在挑「這一段是什麼」，不會再輸入注音，
    // 數字就空出來了。跟選字同一個道理。
    if mode == Mode::SegMenu {
        if let Some(d) = pick_digit(ev.key) {
            return Some(Action::SegPick(d));
        }
    }
    // **數字鍵盤：打什麼就是什麼**。要放在字元輸入之前，不然會被當成
    // 一般字元送進組字區、被切點引擎當注音鍵解讀。
    if let Key::Numpad(ch) = ev.key {
        return Some(Action::NumpadInput(ch));
    }
    // ★ 按著主修飾鍵時字元鍵不是輸入 ★
    //
    // **這一段一定要在查表之前。** 平台翻出來的 `Char` 只套了 Shift，
    // `Ctrl+C` 照樣是 `Char('c')`——不擋的話 `lookup` 回 `Some(Input('c'))`，
    // 而 Windows 的 `defer_to_host` 看的正是「`lookup` 是不是 `None`」。
    // 結果整組 `Ctrl+…` 都被判定成「綁定表裡有」，不讓回宿主，複製貼上
    // 全部失效（2026-09-09 實測回報）。
    //
    // 例外走下面的綁定表（目前只有 `主修飾鍵+Shift+空白` 全半形）——那是
    // 刻意收編的組合，見 `Combo::primary_shift`。
    if !ev.primary {
        // 字元鍵優先——它跟模式無關，隨時可以繼續打字。
        if let Key::Char(ch) = ev.key {
            return Some(Action::Input(ch));
        }
    }
    let combo = Combo {
        key: ev.key,
        shift: ev.shift,
        primary: ev.primary,
    };
    DEFAULT_BINDINGS
        .iter()
        .find(|(m, c, _)| *m == mode && *c == combo)
        .map(|(_, _, a)| *a)
        // Shift 版本沒綁的話退回無 Shift 版本——
        // 使用者按 Shift+Enter 應該還是送出。
        //
        // **按著主修飾鍵時不走這條退路**：`Ctrl+Shift+X` 沒綁的話應該
        // 放行給宿主（那是應用程式的快捷鍵），退回去會誤中 `X` 的
        // 綁定，把一堆宿主快捷鍵吃掉。
        .or_else(|| {
            if ev.primary {
                return None;
            }
            DEFAULT_BINDINGS
                .iter()
                .find(|(m, c, _)| *m == mode && c.key == ev.key && !c.shift)
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
        // **按著主修飾鍵的組合也不在此列**：那些是宿主的快捷鍵，組字中
        // 按 `Ctrl+C` 應該複製，不該被吃掉。組字散不散由宿主自己負責
        // ——它本來就知道自己那個快捷鍵會做什麼。
        .or(if mode == Mode::Idle || ev.primary {
            None
        } else {
            Some(Action::Swallow)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 沒按修飾鍵
    fn 按(mode: Mode, key: Key) -> Option<Action> {
        lookup(mode, KeyEvent::plain(key))
    }

    /// 按著 Shift
    fn shift按(mode: Mode, key: Key) -> Option<Action> {
        lookup(
            mode,
            KeyEvent {
                key,
                shift: true,
                primary: false,
            },
        )
    }

    /// 按著主修飾鍵（Windows 的 Ctrl、macOS 的 Cmd），可選 Shift
    fn 主按(mode: Mode, key: Key, shift: bool) -> Option<Action> {
        lookup(
            mode,
            KeyEvent {
                key,
                shift,
                primary: true,
            },
        )
    }

    /// 2026-09-09 換鍵位：`Shift+空白` 從全半形改成語言鎖定輪替，
    /// 全半形搬到 `主修飾鍵+Shift+空白`。單按 Ctrl 整個退休。
    ///
    /// 鍵位是實測選的——`Ctrl+Shift`、`Ctrl+空白`、`Alt+Shift` 都被
    /// 系統攔走（見 Windows 的 `keyprobe` 與開發文件 §4.17）。
    ///
    /// 搬進 core 之前 `lookup` 自己讀實體鍵盤、測試按不出 Shift，只好
    /// 直接查表；現在修飾鍵是參數，走完整的 `lookup`。
    mod 語言鎖定換鍵位 {
        use super::*;

        #[test]
        fn shift空白是語言輪替() {
            for mode in [Mode::Typing, Mode::Idle, Mode::Selecting] {
                assert_eq!(
                    shift按(mode, Key::Space),
                    Some(Action::CycleLock),
                    "{mode:?} 的 Shift+空白 應該輪替語言鎖定"
                );
            }
        }

        #[test]
        fn 主修飾鍵_shift空白是全半形() {
            for mode in [Mode::Typing, Mode::Idle, Mode::Selecting] {
                assert_eq!(
                    主按(mode, Key::Space, true),
                    Some(Action::ToggleWidth),
                    "{mode:?} 的 主修飾鍵+Shift+空白 應該切全半形"
                );
            }
        }

        /// **兩個動作不能綁在同一個組合上**——這正是這次換鍵位的重點，
        /// 綁重了會有一個永遠觸發不到。
        #[test]
        fn 兩個動作沒有綁在同一個組合() {
            for mode in [Mode::Typing, Mode::Idle, Mode::Selecting] {
                assert_ne!(
                    shift按(mode, Key::Space),
                    主按(mode, Key::Space, true),
                    "{mode:?} 兩個組合不該對到同一個動作"
                );
            }
        }

        /// 打字中的裸空白仍然是注音的一聲——換鍵位不該動到它。
        #[test]
        fn 裸空白還是注音一聲() {
            assert_eq!(
                按(Mode::Typing, Key::Space),
                Some(Action::Input(' ')),
                "空白鍵在打字中是一聲，不是控制鍵"
            );
        }

        /// 全半形只剩 `主修飾鍵+Shift+空白` 一個入口，舊的 `Shift+空白`
        /// 不該還留著（留著就會蓋掉語言輪替）。
        #[test]
        fn 全半形沒有第二個入口() {
            let 全半形入口: Vec<_> = DEFAULT_BINDINGS
                .iter()
                .filter(|(_, _, a)| *a == Action::ToggleWidth)
                .map(|(m, c, _)| (*m, *c))
                .collect();
            assert!(
                全半形入口.iter().all(|(_, c)| c.primary && c.shift),
                "全半形只該綁在 主修飾鍵+Shift 組合上，實際：{全半形入口:?}"
            );
        }
    }

    #[test]
    fn 字元鍵不分模式() {
        assert_eq!(
            按(Mode::Typing, Key::Char('a')),
            Some(Action::Input('a')),
            "字母隨時可以打字"
        );
        assert_eq!(按(Mode::SegMenu, Key::Char('a')), Some(Action::Input('a')));
    }

    /// 數字鍵盤跟主鍵盤那排數字**語意完全不同**，這組測試守住這件事。
    mod 數字鍵盤 {
        use super::*;

        #[test]
        fn 打字時打出的是數字不是注音() {
            // 主鍵盤的 5 在這個輸入法裡是ㄓ，走 Input；
            // 數字鍵盤的 5 就是 5，走 NumpadInput
            assert_eq!(按(Mode::Typing, Key::Char('5')), Some(Action::Input('5')));
            assert_eq!(
                按(Mode::Typing, Key::Numpad('5')),
                Some(Action::NumpadInput('5'))
            );
        }

        #[test]
        fn 十個數字與運算符號都認得() {
            for ch in "0123456789*+-./".chars() {
                assert_eq!(
                    按(Mode::Typing, Key::Numpad(ch)),
                    Some(Action::NumpadInput(ch)),
                    "數字鍵盤的 {ch}"
                );
            }
        }

        #[test]
        fn 選字時可以拿來選候選() {
            // 這是主鍵盤那排做不到的——它們全是注音鍵
            assert_eq!(
                按(Mode::Selecting, Key::Numpad('1')),
                Some(Action::PickChar(0))
            );
            assert_eq!(
                按(Mode::Selecting, Key::Numpad('9')),
                Some(Action::PickChar(8))
            );
        }

        #[test]
        fn 選字時的零不拿來選() {
            // 候選只編到 1~9，0 沒有對應的那一列
            assert_ne!(
                按(Mode::Selecting, Key::Numpad('0')),
                Some(Action::PickChar(9))
            );
        }
    }

    #[test]
    fn 數字鍵是輸入不是選候選() {
        // 十個數字全都是注音鍵，不能拿來選候選
        for ch in '0'..='9' {
            assert_eq!(
                按(Mode::Typing, Key::Char(ch)),
                Some(Action::Input(ch)),
                "主鍵盤的 {ch} 該是輸入"
            );
        }
    }

    #[test]
    fn 同一個鍵不同模式做不同的事() {
        // TAB 開**段選單**
        assert_eq!(按(Mode::Typing, Key::Tab), Some(Action::OpenSegMenu));
        assert_eq!(
            按(Mode::Typing, Key::Down),
            Some(Action::Gesture(Dir::Down))
        );
        assert_eq!(按(Mode::SegMenu, Key::Down), Some(Action::SegNextCand));
        assert_eq!(按(Mode::Selecting, Key::Down), Some(Action::NextCand));
    }

    /// 段選單跟選字**同一組手勢**，只是粒度不同（段 vs 格）。
    #[test]
    fn 段選單的方向鍵跟選字同一套() {
        // 左右換段
        assert_eq!(按(Mode::SegMenu, Key::Left), Some(Action::SegLeft));
        assert_eq!(按(Mode::SegMenu, Key::Right), Some(Action::SegRight));
        // 上下換這一段的解釋
        assert_eq!(按(Mode::SegMenu, Key::Up), Some(Action::SegPrevCand));
        // Shift+←→ 推邊界
        assert_eq!(shift按(Mode::SegMenu, Key::Right), Some(Action::SegWiden));
        assert_eq!(shift按(Mode::SegMenu, Key::Left), Some(Action::SegNarrow));
        // Enter 是「選定這一段」，不是送出
        assert_eq!(按(Mode::SegMenu, Key::Enter), Some(Action::SegConfirm));
        // Esc 跟 TAB 一樣是關選單、保留已定案的段（2026-09-13 跟 macOS 統一）
        assert_eq!(按(Mode::SegMenu, Key::Esc), Some(Action::CloseSegMenu));
        assert_eq!(按(Mode::SegMenu, Key::Tab), Some(Action::CloseSegMenu));
    }

    /// 數字鍵在段選單是直接挑候選——跟選字同一個道理（那時不會再
    /// 輸入注音，十個數字就空出來了）。
    #[test]
    fn 段選單的數字鍵是挑候選() {
        assert_eq!(按(Mode::SegMenu, Key::Char('1')), Some(Action::SegPick(0)));
        assert_eq!(按(Mode::SegMenu, Key::Char('9')), Some(Action::SegPick(8)));
        // 數字鍵盤也一樣
        assert_eq!(
            按(Mode::SegMenu, Key::Numpad('1')),
            Some(Action::SegPick(0))
        );
    }

    /// 2026-09-13 使用者接受的行為變化：`Shift+數字` 翻出來是符號，
    /// 選字與段選單裡**不再挑候選**，而是輸入那個符號（macOS 本來就這樣）。
    #[test]
    fn shift數字在選字中是符號不是挑候選() {
        for mode in [Mode::Selecting, Mode::SelectingExpanded, Mode::SegMenu] {
            assert_eq!(
                shift按(mode, Key::Char('!')),
                Some(Action::Input('!')),
                "{mode:?} 的 Shift+1 該是驚嘆號"
            );
        }
    }

    #[test]
    fn 沒組字時不攔按鍵() {
        assert_eq!(按(Mode::Idle, Key::Left), None);
        assert_eq!(按(Mode::Idle, Key::Enter), None);
        // 但字元鍵要接手——那是開始組字
        assert!(按(Mode::Idle, Key::Char('a')).is_some());
    }

    #[test]
    fn 沒組字時空白就是空白() {
        // 一聲要前面已經有合法注音，不會憑空從空白開始。
        assert_eq!(按(Mode::Idle, Key::Space), None);
    }

    #[test]
    fn 選字選單的上下鍵移動反白() {
        // 圖上的「上下方向鍵：改變選中選項」
        assert_eq!(按(Mode::Selecting, Key::Down), Some(Action::NextCand));
        assert_eq!(按(Mode::Selecting, Key::Up), Some(Action::PrevCand));
        // Enter 是選中反白，不是送出
        assert_eq!(按(Mode::Selecting, Key::Enter), Some(Action::ConfirmCand));
    }

    #[test]
    fn 未展開時左右是換格() {
        // **選完一個字要能移到下一個字繼續選**。
        //
        // 原本右鍵綁的是「展開全部」，結果左右不成對——選完一格
        // 沒辦法往右移，只能靠左鍵繞一圈。
        assert_eq!(按(Mode::Selecting, Key::Right), Some(Action::SelectRight));
        assert_eq!(按(Mode::Selecting, Key::Left), Some(Action::SelectLeft));
    }

    /// 日文詞界伸縮只綁在未展開的選字——展開後 ←→ 是換欄，
    /// 那時的 `Shift+←→` 走「退回無 Shift 版本」變成換欄。
    #[test]
    fn shift左右在選字中是日文詞界() {
        assert_eq!(
            shift按(Mode::Selecting, Key::Right),
            Some(Action::WidenWord)
        );
        assert_eq!(
            shift按(Mode::Selecting, Key::Left),
            Some(Action::NarrowWord)
        );
        assert_eq!(
            shift按(Mode::SelectingExpanded, Key::Right),
            Some(Action::NextColumn)
        );
    }

    #[test]
    fn 空白鍵是展開收合的開關() {
        // 同一個鍵開關：展開後再按一次就收回來。
        // 翻欄交給左右鍵，空白只管這一件事。
        assert_eq!(
            按(Mode::Selecting, Key::Space),
            Some(Action::ExpandAllChars)
        );
        assert_eq!(
            按(Mode::SelectingExpanded, Key::Space),
            Some(Action::CollapseChars)
        );
    }

    #[test]
    fn 展開後左右換欄上下同欄() {
        let m = Mode::SelectingExpanded;
        // 上下同欄移動
        assert_eq!(按(m, Key::Down), Some(Action::NextCand));
        assert_eq!(按(m, Key::Up), Some(Action::PrevCand));
        // 左右換欄
        assert_eq!(按(m, Key::Right), Some(Action::NextColumn));
        assert_eq!(按(m, Key::Left), Some(Action::PrevColumn));
        // Esc 先收回展開
        assert_eq!(按(m, Key::Esc), Some(Action::CollapseChars));
        // 數字鍵在展開狀態也要能選
        assert_eq!(按(m, Key::Char('1')), Some(Action::PickChar(0)));
    }

    #[test]
    fn shift_enter退回enter() {
        // Shift 版本沒綁就退回無 Shift 版本——Shift+Enter 應該還是送出
        assert_eq!(shift按(Mode::Typing, Key::Enter), Some(Action::Commit));
    }

    #[test]
    fn 選字模式下數字是選字() {
        // 打字時 1 是注音鍵，選字時是「選第 1 個」
        assert_eq!(按(Mode::Typing, Key::Char('1')), Some(Action::Input('1')));
        assert_eq!(
            按(Mode::Selecting, Key::Char('1')),
            Some(Action::PickChar(0))
        );
        assert_eq!(
            按(Mode::Selecting, Key::Char('9')),
            Some(Action::PickChar(8))
        );
        // 0 不是選字鍵（1~9 而已），維持輸入
        assert_eq!(
            按(Mode::Selecting, Key::Char('0')),
            Some(Action::Input('0'))
        );
    }

    #[test]
    fn 組字中沒綁定的鍵也要吃掉() {
        // F1、Home、End 都沒綁定（翻成 `Key::Other`），但組字中一律吃掉——
        // 放行的話游標會被移出組字區，組字就散了
        for mode in [Mode::Typing, Mode::SegMenu, Mode::Selecting] {
            assert_eq!(
                按(mode, Key::Other),
                Some(Action::Swallow),
                "{mode:?} 組字中該吃掉"
            );
        }
        // 但沒組字時不能干擾宿主
        assert_eq!(按(Mode::Idle, Key::Other), None);
    }

    #[test]
    fn 打字中方向鍵不會漏出去() {
        // 使用者回報：方向鍵會中斷組字。四個方向都要被接手。
        for key in [Key::Left, Key::Right, Key::Up, Key::Down] {
            assert!(按(Mode::Typing, key).is_some(), "{key:?} 該被接手");
        }
    }

    /// 2026-09-09 實測回報：**複製貼上全部失效**。
    ///
    /// 根因是 `lookup` 的「字元鍵優先」排在 Ctrl 判斷之前——`Ctrl+C`
    /// 翻出來是 `'c'`，`lookup` 就回 `Some(Input('c'))`，於是 Windows 的
    /// `defer_to_host`（它看的是 `lookup` 是不是 `None`）判定「這個組合
    /// 我們有綁」，不讓回宿主。
    ///
    /// 搬進 core 之前測試按不出真的 Ctrl，只能驗「沒按 Ctrl 時字母照樣
    /// 輸入」那一半；**現在兩半都驗得到**。
    mod 主修飾鍵組合要讓回宿主 {
        use super::*;

        #[test]
        fn 按著主修飾鍵的字母回none() {
            for ch in ['c', 'v', 'x', 'z', 'a'] {
                for mode in [Mode::Idle, Mode::Typing, Mode::Selecting] {
                    assert_eq!(
                        主按(mode, Key::Char(ch), false),
                        None,
                        "{mode:?} 的 主修飾鍵+{ch} 該讓回宿主"
                    );
                }
            }
        }

        #[test]
        fn 按著主修飾鍵沒綁的組合不退回也不吞() {
            // `Ctrl+Shift+Enter` 沒綁：不能退回 Enter 的綁定，組字中也不吞
            assert_eq!(主按(Mode::Typing, Key::Enter, true), None);
            assert_eq!(主按(Mode::Typing, Key::Other, false), None);
        }

        #[test]
        fn 沒按主修飾鍵時字母照樣進組字() {
            for ch in ['c', 'v', 'a'] {
                assert_eq!(
                    按(Mode::Typing, Key::Char(ch)),
                    Some(Action::Input(ch)),
                    "沒按主修飾鍵時 {ch} 該進組字"
                );
            }
        }

        /// `主修飾鍵+Shift+空白`（全半形）是**刻意收編**的例外，必須留在
        /// 綁定表裡——修主修飾鍵放行時別把它一起放掉了。
        #[test]
        fn 全半形那個例外還在表裡() {
            assert_eq!(
                主按(Mode::Typing, Key::Space, true),
                Some(Action::ToggleWidth),
                "主修飾鍵+Shift+空白 該還綁著全半形"
            );
        }
    }
}
