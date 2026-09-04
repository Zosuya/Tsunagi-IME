use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;

use crate::keymap::{self, Action, Mode};
use ime_core::Candidate;
use windows::core::{implement, ComObjectInterface, Error, Interface, Ref, Result, BOOL, GUID};
use windows::Win32::Foundation::{E_FAIL, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
use windows::Win32::UI::TextServices::{
    CLSID_TF_CategoryMgr, IEnumTfDisplayAttributeInfo, ITfCategoryMgr, ITfComposition,
    ITfCompositionSink, ITfCompositionSink_Impl, ITfContext, ITfContextComposition,
    ITfDisplayAttributeInfo, ITfDisplayAttributeProvider, ITfDisplayAttributeProvider_Impl,
    ITfEditSession, ITfInsertAtSelection, ITfKeyEventSink, ITfKeyEventSink_Impl, ITfKeystrokeMgr,
    ITfProperty, ITfRange, ITfTextInputProcessor, ITfTextInputProcessorEx,
    ITfTextInputProcessorEx_Impl, ITfTextInputProcessor_Impl, ITfThreadMgr, GUID_PROP_ATTRIBUTE,
    TF_AE_END, TF_ANCHOR_END, TF_DEFAULT_SELECTION, TF_ES_READ, TF_ES_READWRITE, TF_ES_SYNC,
    TF_SELECTION, TF_SELECTIONSTYLE,
};

use crate::candidate_window::{
    caret_screen_position_fallback, caret_screen_position_from_range, CandidateWindow,
};
use crate::display_attribute::{
    display_attribute_info_by_guid, enum_display_attribute_info, GUID_DISPLAY_ATTRIBUTE_INPUT,
};
use crate::edit_session::EditSession;

thread_local! {
    /// 工作列上的狀態按鈕。
    ///
    /// **放 thread-local 而不是 `State`**：COM 物件不是 `Send`，
    /// 塞進 `Mutex<State>` 會過不了型別檢查。而 TSF 本來就是
    /// 每條 UI 執行緒各一份，thread-local 剛好對應。
    static LANG_BAR: std::cell::RefCell<
        Option<(
            windows::core::ComObject<crate::lang_bar::LangBarButton>,
            windows::Win32::UI::TextServices::ITfLangBarItem,
        )>,
    > = const { std::cell::RefCell::new(None) };
}

mod background;
mod composition;
mod document;
mod ui;

use background::*;
use composition::*;
use document::*;
use ui::*;

#[derive(Default)]
pub(super) struct State {
    thread_mgr: Option<ITfThreadMgr>,
    client_id: u32,
    /// 一次輸入的完整狀態：按鍵串、選了哪種切法、選字選到哪一格。
    ///
    /// 狀態轉換的邏輯在 `ime_core::session`——那些跟平台無關，
    /// 放在 core 才測得到。這一層只負責把按鍵翻譯成呼叫哪個方法。
    session: ime_core::session::Session,
    /// 切法選單開著嗎？開著的話空白鍵是「往下選」而不是注音的一聲。
    cutting_menu: bool,
    /// 切法選單目前展開幾個（TAB 10 個，快速按兩下 50 個）。
    cutting_shown: usize,
    /// 上一次按 TAB 的時間——判斷「快速按兩下」用。
    last_tab: Option<std::time::Instant>,
    /// 使用者設定（行為與外觀）。
    config: ime_core::config::Config,
    /// 上次讀設定檔時它的修改時間。用來判斷「改過了要重讀」。
    config_stamp: Option<std::time::SystemTime>,
    /// 上次去問檔案系統的時間——用來節流，見 `refresh_config`。
    config_checked: Option<std::time::Instant>,
    /// 上次載入的領域包是什麼時候改的。**跟設定檔分開記**——
    /// 使用者直接編輯包的內容不會動到 `config.toml`。
    pack_stamp: Option<std::time::SystemTime>,
    /// 「上上下下」手勢的偵測器，見 `ime_core::command`。
    gesture: ime_core::command::Gesture,
    /// 上一次按方向鍵的時間——太久沒接續就當手勢放棄了。
    last_gesture: Option<std::time::Instant>,
    /// 目前掛在文件上的 TSF composition（有值代表底線組字串正顯示中）。
    composition: Option<ITfComposition>,
    /// 目前這個欄位是密碼欄位嗎？見 `is_password_field`。
    ///
    /// **是的話輸入法完全不介入**：不組字、不彈候選、按鍵原樣交給
    /// 宿主。密碼本來就是 ASCII，不需要輸入法，而讓它顯示在候選
    /// 視窗上是嚴重的隱私問題。
    password: bool,
    /// 空白鍵叫出來的假候選清單快取，Enter/數字鍵送出時用。
    candidates: Vec<Candidate>,
    candidate_window: Option<CandidateWindow>,
    /// 全半形切換的提示視窗。跟候選是**兩個獨立的視窗**——
    /// 那是狀態提示，這是選字，混在一起候選高度會忽大忽小。
    width_window: Option<crate::width_window::WidthWindow>,
    /// Ctrl 按著的期間，有沒有按過別的鍵？
    ///
    /// # 為什麼要記
    ///
    /// 「單按 Ctrl」＝按下、放開，中間沒碰別的鍵——那是語言輪替。
    /// 但 Ctrl 更常是組合鍵的一半（`Ctrl+C`、`Ctrl+S`），所以不能
    /// 一放開就當成單按。邏輯與測試在 `keymap::CtrlTap`。
    ctrl_tap: keymap::CtrlTap,
}

/// Phase 0 的 Echo IME：不做真正的語言辨識，只證明
/// 「按鍵 -> composition -> 候選 -> 送出」整條 TSF 管線是通的。
#[implement(
    ITfTextInputProcessor,
    ITfTextInputProcessorEx,
    ITfKeyEventSink,
    ITfCompositionSink,
    ITfDisplayAttributeProvider
)]
pub struct TextService {
    /// **用 `Arc` 是為了讓語言列共享同一份狀態**。
    ///
    /// 工作列上那個按鈕（`lang_bar`）是獨立的 COM 物件，它的回呼
    /// 要能改語言模式——兩邊指向同一個 `Mutex` 才不會各改各的。
    ///
    /// clippy 會說「`Arc` 用在不是 `Send`/`Sync` 的東西上」——
    /// `State` 裡有 COM 物件（`ITfThreadMgr` 那些），那些確實不是
    /// `Send`。但這裡**不跨執行緒**：TSF 是單執行緒模型，語言列的
    /// 回呼跟按鍵處理都在同一條 UI 執行緒上，`Arc` 只是為了共享
    /// 所有權（兩個 COM 物件都要活著時指向同一份狀態）。
    #[allow(clippy::arc_with_non_send_sync)]
    state: std::sync::Arc<Mutex<State>>,
}

/// 拿狀態鎖，**中毒也要拿得到**。
///
/// # 為什麼不能用 `.lock().unwrap()`
///
/// 一旦有 panic 發生在持鎖期間，`Mutex` 就中毒了。`unwrap()` 會在下一次
/// 取鎖時**再 panic 一次**——`catch_unwind` 攔得住當機，但攔不住「從此
/// 每按一鍵都 panic」，使用者看到的是輸入法徹底死掉。
///
/// 用 `Ok(..) else { return }` 那種寫法一樣不行：那是悄悄什麼都不做，
/// 症狀變成「輸入法還在、按鍵完全沒反應」，比當掉更難查。
///
/// 中毒只代表「上一次有人 panic」，資料結構本身還在。**拿回來繼續用，
/// 並把 session 重設乾淨**才是對的降級——見 `recover_if_poisoned`。
pub(super) fn lock_state(m: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    match m.lock() {
        Ok(g) => g,
        Err(poisoned) => {
            crate::debug_log::log("[狀態鎖中毒] 上一次有 panic，重設組字狀態後繼續");
            // **一定要清旗標**。`into_inner()` 只是把值拿回來，中毒旗標是
            // 黏著的——不清的話之後每一次 `lock()` 都還是回 `Err`，於是
            // 每按一鍵都跑一次下面的 `clear()`，永遠只留得住最後一鍵。
            //
            // 實測過：F9 觸發一次 panic 之後，log 出現 2 次「攔下 panic」
            // 卻有 85 次「狀態鎖中毒」——輸入法沒當機、也沒再 panic，
            // 但每打一個字就把前一個吃掉，實質上不能用。
            //
            // 這是型別檢查抓不到的語意 bug，只有真的跑過才看得出來。
            m.clear_poison();
            let mut g = poisoned.into_inner();
            // panic 可能停在組字的一半，留下的狀態不一致。整串丟掉重來
            // ——使用者頂多是「剛打的那幾個字沒了」，比帶著半殘的狀態
            // 繼續組字安全得多
            g.session.clear();
            g.cutting_menu = false;
            g.gesture.clear();
            g.candidate_window = None;
            g
        }
    }
}

impl TextService {
    // 見 `state` 欄位的說明：TSF 是單執行緒模型，這個 `Arc` 不跨執行緒
    #[allow(clippy::arc_with_non_send_sync)]
    pub fn new() -> Self {
        Self {
            state: std::sync::Arc::new(Mutex::new(State::default())),
        }
    }
}

/// 把 TSF 的「鍵盤開啟」狀態設成真。
///
/// 失敗只記 log 不中斷——這只影響工作列指示器顯不顯示，
/// 不影響輸入功能本身。
fn set_keyboard_open(thread_mgr: &ITfThreadMgr, tid: u32) {
    use windows::Win32::System::Variant::VARIANT;
    use windows::Win32::UI::TextServices::{
        ITfCompartmentMgr, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE,
    };
    unsafe {
        let Ok(mgr) = thread_mgr.cast::<ITfCompartmentMgr>() else {
            crate::dlog!("[langbar] 取 ITfCompartmentMgr 失敗");
            return;
        };
        let Ok(comp) = mgr.GetCompartment(&GUID_COMPARTMENT_KEYBOARD_OPENCLOSE) else {
            crate::dlog!("[langbar] 取 OPENCLOSE compartment 失敗");
            return;
        };
        // compartment 存的是 VARIANT，開啟狀態用整數 1 表示。
        // 用 `From` 轉換而不是手填 union 欄位——後者會踩到
        // `ManuallyDrop` 的解構問題。
        let v = VARIANT::from(1i32);
        if let Err(e) = comp.SetValue(tid, &v) {
            crate::dlog!("[langbar] 設定鍵盤開啟狀態失敗: {e:?}");
        }
    }
}

/// `ITfTextInputProcessorEx` 是 `ITfTextInputProcessor` 的加強版，
/// 多一個 `dwFlags` 參數。**每個真實的輸入法都會實作它**——有些宿主
/// 只找這個介面，只實作舊版的會被當成不完整的 TIP。
///
/// 我們用不到 `dwFlags`，直接轉呼叫 `Activate`（新酷音與小狼毫也是
/// 這樣寫的）。
impl ITfTextInputProcessorEx_Impl for TextService_Impl {
    fn ActivateEx(&self, ptim: Ref<ITfThreadMgr>, tid: u32, _dwflags: u32) -> Result<()> {
        crate::guard::com("ActivateEx", || self.Activate(ptim, tid))
    }
}

impl ITfTextInputProcessor_Impl for TextService_Impl {
    fn Activate(&self, ptim: Ref<ITfThreadMgr>, tid: u32) -> Result<()> {
        crate::guard::com("Activate", || {
            // 量每一步的耗時。切過去要等一秒的問題就是靠這個定位的。
            let t0 = std::time::Instant::now();
            let thread_mgr = ptim.ok()?.clone();
            let keystroke_mgr: ITfKeystrokeMgr = thread_mgr.cast()?;

            let key_sink: ITfKeyEventSink =
                (*ComObjectInterface::<ITfKeyEventSink>::as_interface_ref(self)).clone();
            unsafe {
                keystroke_mgr.AdviseKeyEventSink(tid, &key_sink, true)?;
            }

            // **只上一次鎖**。這個鎖不可重入——同一條執行緒鎖兩次會直接卡死，
            // 那正是「一打字瀏覽器就卡死」的原因（`OnKeyDown` 裡誤加了
            // 第二次上鎖）。
            let mut state = lock_state(&self.state);
            refresh_config(&mut state);
            state.thread_mgr = Some(thread_mgr.clone());
            state.client_id = tid;
            // **設定要先讀完才知道哪些詞庫該載**，見下面
            let engines = state.config.behavior.engines;
            drop(state);

            // **詞庫丟到背景載**——見 `spawn_dict_load`。
            //
            // **順序不能反**：要先讀設定才知道哪些引擎有開。關掉的引擎
            // 不載，日文那本要 0.7 秒，沒開日文的話那是純粹白等。
            spawn_dict_load(engines);
            crate::dlog!("[啟用] 完成 @{}ms", t0.elapsed().as_millis());

            // **把「鍵盤開啟」狀態設成真**。
            //
            // TSF 用 compartment（一種全域狀態格）記錄輸入法開著沒有，
            // 工作列的輸入指示器會去讀它——狀態是「關閉」的話，那格
            // 本來就該不顯示。預設值是關閉，所以要自己打開。
            set_keyboard_open(&thread_mgr, tid);

            // **工作列上的狀態按鈕**。
            //
            // 回呼裡改的是同一份 `state`（`Arc` 共享），所以按鈕點下去
            // 的效果跟按 Ctrl 一樣。
            let shared = self.state.clone();
            let button =
                windows::core::ComObject::new(crate::lang_bar::LangBarButton::new(move |action| {
                    use crate::lang_bar::Action;
                    let mut st = lock_state(&shared);
                    match action {
                        Action::Cycle => st.session.cycle_lock(),
                        Action::SetLock(l) => st.session.set_lock(l),
                        Action::OpenSettings => {
                            drop(st);
                            open_settings();
                            return;
                        }
                        // 右鍵：自己畫的選單（Win11 不給我們用系統選單）
                        Action::OpenMenu(x, y) => {
                            let lock = st.session.lock();
                            let engines = st.session.engines();
                            let theme = crate::theme::Theme::from_config(&st.config);
                            // **在彈選單前放掉鎖**——選單的回呼會再動到同一份
                            // 狀態，還握著的話會卡死（這個鎖不可重入）
                            drop(st);
                            let inner = shared.clone();
                            let _ = crate::lang_menu::show(
                                windows::Win32::Foundation::POINT { x, y },
                                lock,
                                &engines,
                                theme,
                                move |picked| {
                                    use crate::lang_menu::MenuAction;
                                    match picked {
                                        MenuAction::OpenSettings => open_settings(),
                                        MenuAction::SetLock(l) => {
                                            {
                                                let mut st = lock_state(&inner);
                                                st.session.set_lock(l);
                                                let now = st.session.lock();
                                                drop(st);
                                                // 狀態變了要通知語言列重畫
                                                LANG_BAR.with(|b| {
                                                    if let Some((btn, _)) = b.borrow().as_ref() {
                                                        btn.set_lock(now);
                                                    }
                                                });
                                            }
                                        }
                                    }
                                },
                            );
                            return;
                        }
                    }
                    let lock = st.session.lock();
                    drop(st);
                    // 狀態變了要通知語言列重畫
                    LANG_BAR.with(|b| {
                        if let Some((btn, _)) = b.borrow().as_ref() {
                            btn.set_lock(lock);
                        }
                    });
                }));
            // **滑鼠選字**：候選視窗只知道點到第幾列，怎麼處理是這裡的事
            let picked = self.state.clone();
            crate::candidate_window::set_on_pick(std::rc::Rc::new(move |i| {
                on_candidate_picked(&picked, i);
            }));
            // **拖捲軸**：視窗只知道使用者把滑塊拖到第幾欄，
            // 換算與重畫是這裡的事
            let scrolled = self.state.clone();
            crate::candidate_window::set_on_scroll(std::rc::Rc::new(move |first| {
                on_candidate_scrolled(&scrolled, first);
            }));

            let item: windows::Win32::UI::TextServices::ITfLangBarItem = button.to_interface();
            match crate::lang_bar::install(&thread_mgr, &item) {
                Ok(()) => LANG_BAR.with(|b| *b.borrow_mut() = Some((button, item))),
                Err(e) => crate::dlog!("[langbar] install 失敗: {e:?}"),
            }
            Ok(())
        })
    }

    fn Deactivate(&self) -> Result<()> {
        crate::guard::com("Deactivate", || {
            let mut state = lock_state(&self.state);
            state.candidate_window = None;
            // 動畫視窗自己會在跑完時藏起來，但切走輸入法時要真的銷毀
            state.width_window = None;
            state.composition = None;
            state.session.clear();

            if let Some(thread_mgr) = state.thread_mgr.take() {
                let tid = state.client_id;
                drop(state);
                // **一定要拿掉**，不然工作列上會留下一個點了沒反應的按鈕
                LANG_BAR.with(|b| {
                    if let Some((_, item)) = b.borrow_mut().take() {
                        let _ = crate::lang_bar::remove(&thread_mgr, &item);
                    }
                });
                if let Ok(keystroke_mgr) = thread_mgr.cast::<ITfKeystrokeMgr>() {
                    unsafe {
                        let _ = keystroke_mgr.UnadviseKeyEventSink(tid);
                    }
                }
            }
            Ok(())
        })
    }
}

impl ITfCompositionSink_Impl for TextService_Impl {
    /// 組字被系統強制中止時（例如焦點被搶走）觸發。
    /// Phase 0 先做最保守的處理：清掉本地狀態，避免殘留一個指向已死
    /// composition 的參照；細緻的「還原成純文字」留給後續 phase。
    fn OnCompositionTerminated(
        &self,
        _ecwrite: u32,
        _pcomposition: Ref<ITfComposition>,
    ) -> Result<()> {
        let mut state = lock_state(&self.state);
        state.composition = None;
        state.session.clear();
        state.candidate_window = None;
        Ok(())
    }
}

impl ITfDisplayAttributeProvider_Impl for TextService_Impl {
    fn EnumDisplayAttributeInfo(&self) -> Result<IEnumTfDisplayAttributeInfo> {
        Ok(enum_display_attribute_info())
    }

    fn GetDisplayAttributeInfo(&self, guid: *const GUID) -> Result<ITfDisplayAttributeInfo> {
        if guid.is_null() {
            return Err(Error::from(E_FAIL));
        }
        display_attribute_info_by_guid(unsafe { &*guid })
    }
}

/// 把「組字中」的樣式套到 `range` 上，宿主才知道要畫底線。
///
/// 這是底線能不能出現的最後一環：光有 provider（宣告我提供哪些樣式）
/// 還不夠，還要將樣式的 GUID 寫進 `GUID_PROP_ATTRIBUTE` 這個屬性，
/// 宿主讀到它才會去回問我們樣式長什麼樣。
///
/// TSF 的屬性值是 VARIANT，而 GUID 本身不能直接放進去，要先透過
/// `ITfCategoryMgr::RegisterGUID` 換成一個 `TfGuidAtom`（一個 u32 代碼）。
unsafe fn apply_display_attribute(context: &ITfContext, ec: u32, range: &ITfRange) -> Result<()> {
    unsafe {
        let category_mgr: ITfCategoryMgr =
            CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)?;
        let atom = category_mgr.RegisterGUID(&GUID_DISPLAY_ATTRIBUTE_INPUT)?;

        let prop: ITfProperty = context.GetProperty(&GUID_PROP_ATTRIBUTE)?;
        let value = VARIANT::from(atom as i32);
        prop.SetValue(ec, range, &value as *const VARIANT)
    }
}

impl ITfKeyEventSink_Impl for TextService_Impl {
    fn OnSetFocus(&self, _fforeground: BOOL) -> Result<()> {
        crate::guard::com("OnSetFocus", || Ok(()))
    }

    /// 「這個鍵你要不要？」——TSF 會先問這個，再決定要不要送 `OnKeyDown`。
    ///
    /// **必須跟 `OnKeyDown` 的判斷一致**，否則會出現「說要卻不處理」
    /// （按鍵消失）或「說不要卻處理了」（宿主也收到一份）。
    /// 兩邊都查同一張表就不會分岔——那是抽出 `keymap` 的主要理由之一。
    fn OnTestKeyDown(&self, pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        crate::guard::key("OnTestKeyDown", || {
            let vk = wparam.0 as u32;
            // 量測快捷鍵攔截用。**這一行要在最前面**——這是 TSF 一定會問的
            // 一步，沒出現在 log 裡就代表按鍵根本沒送到輸入法（被系統或
            // 宿主先吃掉）。往後挪就分不出「沒到」與「到了但我們提早 return」
            crate::keyprobe::probe("Test", vk, None);

            // **「Ctrl 有沒有配別的鍵」要在這裡標記，不能只在 `OnKeyDown`**。
            //
            // 這個方法是 TSF 一定會問的，而下面一遇到修飾鍵就回 0——回 0
            // 之後 TSF 就不會再送 `OnKeyDown` 過來。只在那邊標記的話，
            // `Ctrl+C`／`Ctrl+V` 的那個字母我們**根本看不到**，放開 Ctrl
            // 就被當成單按，語言模式莫名其妙被切走。
            //
            // 複製貼上是最常按的組合，所以這個漏洞幾乎天天踩到。
            {
                let mut state = lock_state(&self.state);
                state
                    .ctrl_tap
                    .key_down(vk, keymap::ctrl_down(), keymap::is_repeat(lparam));
            }

            // `Ctrl+標點鍵` 是唯一被我們接走的修飾鍵組合，見 `ctrl_punct`。
            // **兩個入口的判斷必須一模一樣**，分岔的話會出現「說要卻不處理」
            // （按鍵消失）或「說不要卻處理了」（宿主收到兩份）。
            if ctrl_punct(vk).is_some() {
                let state = lock_state(&self.state);
                if !state.password
                    && state.session.lock() == Some(ime_core::language::Language::Bopomofo)
                {
                    return Ok(BOOL(1));
                }
            }
            if is_modifier_down() {
                return Ok(BOOL(0));
            }
            let mut state = lock_state(&self.state);
            // 密碼欄位一律不接手，理由同 `OnKeyDown`
            if let Ok(ctx) = pic.ok() {
                refresh_password(ctx, &mut state);
            }
            if state.password {
                return Ok(BOOL(0));
            }
            // **有綁定就一律接手**。
            //
            // 原本這裡排除了 EnterSelect 與 OpenCuttingMenu（那是還沒實作
            // 時的權宜），結果 TSF 認為我們不要 TAB／方向鍵，就不會送
            // OnKeyDown 過來——切法選單根本打不開。
            //
            // 這兩個方法的判斷必須完全一致，那正是抽出 keymap 的理由。
            let handled = keymap::lookup(state.mode(), vk).is_some();
            Ok(BOOL::from(handled))
        })
    }

    /// 「這個放開的鍵你要不要？」
    ///
    /// 一律回不要——我們只是想在 `OnKeyUp` 偷看 Shift 有沒有放開，
    /// 不需要攔截它。**回 0 但 TSF 仍會送 `OnKeyUp`**，
    /// 那跟 KeyDown 的規則不同（KeyDown 要回 1 才收得到）。
    fn OnTestKeyUp(&self, _pic: Ref<ITfContext>, wparam: WPARAM, _lparam: LPARAM) -> Result<BOOL> {
        crate::guard::key("OnTestKeyUp", || {
            // **單按 Ctrl 是語言輪替**，所以放開 Ctrl 這一下要接手。
            //
            // 其餘一律回 0——我們只是想順便偷看，不需要攔截。
            let vk = wparam.0 as u32;
            Ok(BOOL::from(vk == VK_CONTROL.0 as u32))
        })
    }

    /// 放開按鍵。
    ///
    /// 兩件事：
    ///
    /// - **Shift**：全半形提示在它按著時不淡出，放開才開始倒數。
    /// - **單按 Ctrl**（中間沒碰別的鍵）＝ 語言輪替。
    ///
    /// # 為什麼是 Ctrl 不是 Shift
    ///
    /// 一般輸入法用單按 Shift 切中英文，但這裡要輪四個模式，連按會
    /// **觸發 Windows 的相黏鍵**（連按五次 Shift 跳出協助工具對話框）。
    /// Ctrl 沒有那個機制，單按也沒有系統預設功能。
    fn OnKeyUp(&self, pic: Ref<ITfContext>, wparam: WPARAM, _lparam: LPARAM) -> Result<BOOL> {
        crate::guard::key("OnKeyUp", || {
            let vk = wparam.0 as u32;
            // 提示視窗在修飾鍵按著時不淡出，放開才開始倒數。
            //
            // 這裡通知的是**已經在畫面上**的那個（全半形的提示：使用者
            // 按著 Shift 連按空白切模式，現在放開了）。單按 Ctrl 的
            // 語言提示是下面才建出來的，那個要另外通知——見函式尾端。
            if vk == VK_SHIFT.0 as u32 {
                crate::width_window::on_shift_release();
                return Ok(BOOL(0));
            }
            if vk != VK_CONTROL.0 as u32 {
                return Ok(BOOL(0));
            }
            crate::width_window::on_shift_release();

            // 按著 Ctrl 按了別的鍵（`Ctrl+C` 這種），這一放開就不算單按。
            // 標記在 `OnTestKeyDown`／`OnKeyDown`，邏輯見 `keymap::CtrlTap`。
            let mut state = lock_state(&self.state);
            let alone = state.ctrl_tap.ctrl_released();
            if !alone {
                return Ok(BOOL(0));
            }
            let Some(context) = pic.as_ref() else {
                return Ok(BOOL(0));
            };

            let before = state.session.lock();
            state.session.cycle_lock();
            let after = state.session.lock();
            if !state.session.is_empty() {
                if direct_input_mode(&state) {
                    // **切進「直接輸入」模式時要先把手上的組字送出去**。
                    //
                    // 那個模式不該有組字存在，留著的話會變成一串永遠
                    // 送不出去的底線文字——使用者接著打的字直接進文件，
                    // 組字區卻還掛在那裡。
                    let text = state.session.text();
                    end_composition(context, &mut state, EndKind::Commit(&text))?;
                } else {
                    // 輪替之後已經打的字也要跟著重算
                    update_composition(self, context, &mut state)?;
                    show_candidates(context, &mut state)?;
                }
            }
            show_lang_window(context, &mut state, before, after)?;
            // **工作列的字也要跟著變**——語言列不會主動來問，
            // 不通知的話按了 Ctrl 工作列還顯示舊的模式
            LANG_BAR.with(|b| {
                if let Some((btn, _)) = b.borrow().as_ref() {
                    btn.set_lock(after);
                }
            });
            // **建完才通知可以淡出**。
            //
            // 順序不能反：`show_lang_window` 建的是一個全新的動畫，
            // 預設是「修飾鍵還按著、不要淡出」的狀態。開頭那次
            // `on_shift_release` 通知的是上一個視窗，對這個新的無效——
            // 結果就是提示一直掛在畫面上不消失。
            //
            // 單按 Shift 的語意本來就是「按完就放開」，沒有「按著不放
            // 繼續切」的情況（那是全半形的 Shift+空白），所以建完直接
            // 進入倒數是對的。
            crate::width_window::on_shift_release();
            // **處理了，但不消耗**。
            //
            // 回 1 會把 Ctrl 的放開事件吃掉，而按下那一邊我們是放行的
            // （`OnTestKeyDown` 的 `is_modifier_down()` 回 0）——一邊吃
            // 一邊不吃，宿主眼裡 Ctrl 就一直按著，之後每個鍵都變成組合鍵。
            //
            // 新酷音踩過同一個坑（26.4.2）：單按 Shift 切中英文時把 Shift
            // 攔下來，**遠端桌面與 UltraEdit 因此壞掉**，修法就是改成
            // 「處理但不消耗」。
            //
            // 該做的事在這一行之前都做完了，回 0 只是讓宿主收到它本來
            // 就該收到的放開事件。
            Ok(BOOL(0))
        })
    }

    fn OnKeyDown(&self, pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        crate::guard::key("OnKeyDown", || {
            // 有到這裡就代表我們在 OnTestKeyDown 說了「要」。兩邊對照才看得出
            // 「說要卻沒處理」（按鍵消失）這種分岔
            crate::keyprobe::probe("Down", wparam.0 as u32, Some(true));
            let Some(context) = pic.as_ref() else {
                return Ok(BOOL(0));
            };
            // **先記下「這一下把修飾鍵用掉了」，再決定要不要擋**。
            //
            // 順序很重要：`Ctrl+C` 會在下面被擋掉直接 return，如果標記
            // 寫在 return 之後就永遠執行不到——放開 Ctrl 時就會被誤判成
            // 單按，複製一次就切一次語言。
            {
                // 兩個入口都標記：`OnTestKeyDown` 是主力（見那邊的說明），
                // 這裡是保險——有些宿主不見得會先問。邏輯與測試在 `CtrlTap`。
                let mut state = lock_state(&self.state);
                state.ctrl_tap.key_down(
                    wparam.0 as u32,
                    keymap::ctrl_down(),
                    keymap::is_repeat(lparam),
                );
            }

            let vk = wparam.0 as u32;
            // `Ctrl+標點鍵`：鎖定注音時明講「我要標點」，見 `ctrl_punct`。
            // 判斷跟 `OnTestKeyDown` 必須一致。
            if let Some(ch) = ctrl_punct(vk) {
                let mut state = lock_state(&self.state);
                if state.config.behavior.ctrl_punct
                    && !state.password
                    && state.session.push_punct(ch)
                {
                    state.cutting_menu = false;
                    state.gesture.clear();
                    update_composition(self, context, &mut state)?;
                    show_candidates(context, &mut state)?;
                    return Ok(BOOL(1));
                }
            }

            // Ctrl/Alt 的其餘組合留給宿主——那些是應用程式的快捷鍵。
            // Shift 不擋，它要參與按鍵綁定（Shift+空白 = 切全半形）。
            if is_modifier_down() {
                return Ok(BOOL(0));
            }

            let mut state = lock_state(&self.state);

            // **密碼欄位一律不接手**：不組字、不彈候選，按鍵原樣給宿主。
            // 不擋的話密碼會顯示在候選視窗上，見 `is_password_field`。
            if let Ok(ctx) = pic.ok() {
                refresh_password(ctx, &mut state);
            }
            if state.password {
                // 萬一還有殘留的候選視窗，一起收掉
                state.candidate_window = None;
                return Ok(BOOL(0));
            }

            // 收掉沒在組字時留下的提示視窗（見 `show_width_hint`）。
            // 使用者按了別的鍵，代表那個提示看完了。
            if state.session.is_empty() && state.candidate_window.is_some() {
                let toggling = keymap::lookup(state.mode(), vk) == Some(Action::ToggleWidth);
                if !toggling {
                    state.candidate_window = None;
                }
            }

            // **按鍵綁定查表**（見 `keymap`）——按鍵與動作分開，
            // 之後要改鍵位或加設定檔只要動那張表。
            let Some(action) = keymap::lookup(state.mode(), vk) else {
                return Ok(BOOL(0)); // 沒綁定 → 放行給宿主
            };

            match action {
                Action::Input(ch) => {
                    // 第一個字之前看看設定改過沒（見 `refresh_config`），
                    // 再確認詞庫在（見 `ensure_dict_loaded`）。
                    //
                    // **順序不能反**：要先讀設定才知道哪些引擎有開，
                    // 關掉的引擎不必載那本詞庫。
                    if state.session.is_empty() {
                        refresh_config(&mut state);
                        ensure_dict_loaded(state.config.behavior.engines);
                    }
                    // **鎖定英文＋非全形＝直接打進文件**，不組字、不彈候選。
                    //
                    // 那個狀態的語意是「等同關掉輸入法」，中間隔一層組字
                    // 只是干擾。全形模式要排除——`ａｂｃ` 的轉換是在組字
                    // 階段做的，不組字就沒機會轉。
                    if state.session.is_empty() && direct_input_mode(&state) {
                        insert_directly(context, &mut state, ch)?;
                        return Ok(BOOL(1));
                    }
                    // **開頭就打符號的話直接送出去**，不進組字區。
                    //
                    // 只想打一個驚嘆號，卻要看候選視窗彈出來、還得按 Enter
                    // 才送得出去——多兩道手續換不到任何好處。
                    // 判準見 `passthrough_alone`（要排除注音鍵，`,.;/-`
                    // 那幾個是ㄝㄡㄤㄥㄦ）。
                    if state.session.is_empty() && ime_core::input::passthrough_alone(ch) {
                        // 全形模式下標點也要跟著變全形，跟組字那條路一致
                        let ch = ime_core::width::convert(ch, state.session.width(), None);
                        insert_directly(context, &mut state, ch)?;
                        return Ok(BOOL(1));
                    }
                    state.session.push(ch);
                    state.cutting_menu = false;
                    // 手勢必須是連續四下，打了字就重來
                    state.gesture.clear();
                    update_composition(self, context, &mut state)?;
                    // **每一鍵都更新預覽**——那是這個設計的重點，
                    // 使用者要在打字當下就看到引擎怎麼切。
                    show_candidates(context, &mut state)?;
                }
                // **數字鍵盤打的就是數字**，不進組字區。
                //
                // 正在組字的話先把它送出，再打數字進去——像是「這一段
                // 打完了，接著輸入數字」。混進注音串裡的話 `5` 會被當成
                // ㄓ，那正是主鍵盤那排數字不能拿來選字的原因。
                Action::NumpadInput(ch) => {
                    if !state.session.is_empty() {
                        let text = state.session.text();
                        end_composition(context, &mut state, EndKind::Commit(&text))?;
                    }
                    // 全形模式下數字也要變全形，跟組字那條路一致
                    let ch = ime_core::width::convert(ch, state.session.width(), None);
                    insert_directly(context, &mut state, ch)?;
                }
                Action::Backspace => {
                    state.session.backspace();
                    state.cutting_menu = false;
                    if state.session.is_empty() {
                        end_composition(context, &mut state, EndKind::Cancel)?;
                    } else {
                        update_composition(self, context, &mut state)?;
                        show_candidates(context, &mut state)?;
                    }
                }
                Action::Cancel => {
                    // 選字或選單開著時，Esc 先退回打字狀態；再按一次才取消組字
                    if state.cutting_menu || state.session.select_index().is_some() {
                        state.cutting_menu = false;
                        state.session.exit_select();
                        show_candidates(context, &mut state)?;
                    } else {
                        end_composition(context, &mut state, EndKind::Cancel)?;
                    }
                }
                // 日文詞界調整。**調不動就當沒按**（例如框在最後一格還想
                // 往右吃），不要把按鍵吃掉之後什麼都不做。
                Action::WidenWord | Action::NarrowWord => {
                    let ok = if matches!(action, Action::WidenWord) {
                        state.session.widen_word()
                    } else {
                        state.session.narrow_word()
                    };
                    if !ok {
                        return Ok(BOOL(1));
                    }
                    update_composition(self, context, &mut state)?;
                    show_candidates(context, &mut state)?;
                }

                Action::Commit => {
                    let text = state.session.text();
                    state.cutting_menu = false;
                    learn_from(&mut state);
                    end_composition(context, &mut state, EndKind::Commit(&text))?;
                }

                // ── 切法選單 ──
                Action::OpenCuttingMenu => {
                    // **鎖定語言時沒有切法可選**——整串就是一段，
                    // 開一個只有一項的選單只會擋住畫面。
                    if state.session.lock().is_some() {
                        return Ok(BOOL(1));
                    }
                    // **快速按兩下展開全部**——使用者定的。
                    //
                    // 判定 400ms：太短來不及按第二下，太長會把「按 TAB 翻頁」
                    // 誤判成雙擊。這是常見的雙擊門檻。
                    let now = std::time::Instant::now();
                    let double = state
                        .last_tab
                        .is_some_and(|t| now.duration_since(t) < DOUBLE_TAB);
                    state.last_tab = Some(now);
                    state.cutting_menu = true;
                    state.cutting_shown = if double {
                        ime_core::session::CUTTING_PAGE_ALL
                    } else {
                        ime_core::session::CUTTING_PAGE
                    };
                    show_candidates(context, &mut state)?;
                }
                // **反白條在清單裡跑**，組字區不動。
                //
                // 組字區顯示的一直是原始按鍵（打什麼顯示什麼），所以翻切法
                // 不必改組字區——只要移動反白、重畫預覽列就好。
                // 之前每次都呼叫 `update_composition`，那是多餘的重寫。
                Action::NextCutting => {
                    state.session.next_cutting();
                    show_candidates(context, &mut state)?;
                }
                Action::PrevCutting => {
                    state.session.prev_cutting();
                    show_candidates(context, &mut state)?;
                }

                // 兩種退出選單的方式，差別在「有沒有選中」：
                //
                //   Enter → 就用反白這個切法，關選單、留在組字狀態
                //   TAB   → 單純關掉選單，切法維持原本選中的那個
                //
                // 兩者都**不送出**——送出要在關掉選單之後再按一次 Enter。
                // 目前反白的切法就是 `session.cutting_index()`，兩條路都已經
                // 是它了，所以差別只在使用者的意圖，實作上都只是關選單。
                Action::ConfirmCutting | Action::CloseCuttingMenu => {
                    state.cutting_menu = false;
                    // 關掉選單就不該再被雙擊判定黏住——
                    // 不清掉的話下一次按 TAB 會被誤判成「雙擊展開全部」
                    state.last_tab = None;
                    show_candidates(context, &mut state)?;
                }

                // ── 選字 ──
                // **方向鍵一律交給 `arrow_*`**——「進選字」與「移動」對使用者
                // 來說是同一件事（往右），差別是內部細節，見 `arrow_right`
                Action::EnterSelect => {
                    state.cutting_menu = false;
                    state.session.arrow_right();
                    show_candidates(context, &mut state)?;
                }
                Action::EnterSelectLast => {
                    state.cutting_menu = false;
                    state.session.arrow_left();
                    show_candidates(context, &mut state)?;
                }
                Action::SelectLeft => {
                    state.session.arrow_left();
                    show_candidates(context, &mut state)?;
                }
                Action::SelectRight => {
                    state.session.arrow_right();
                    show_candidates(context, &mut state)?;
                }
                // 空白鍵展開全部：一般狀態只列前 9 個，展開後全部分欄列出
                Action::ExpandAllChars => {
                    state.session.expand_cands();
                    show_candidates(context, &mut state)?;
                }
                // Esc 先收回展開，再按一次才離開選字
                Action::CollapseChars => {
                    state.session.collapse_cands();
                    show_candidates(context, &mut state)?;
                }
                Action::NextColumn => {
                    state.session.cand_right_column();
                    show_candidates(context, &mut state)?;
                }
                Action::PrevColumn => {
                    state.session.cand_left_column();
                    show_candidates(context, &mut state)?;
                }
                Action::NextCand => {
                    state.session.next_cand();
                    show_candidates(context, &mut state)?;
                }
                Action::PrevCand => {
                    state.session.prev_cand();
                    show_candidates(context, &mut state)?;
                }
                // 「上上下下」手勢：組字內容是指令時，不必往下選就能執行。
                //
                // 沒湊滿手勢就退回原本的「進選字」——方向鍵不能因為多了
                // 手勢偵測就失去本來的功能。
                Action::Gesture(dir) => {
                    // **隔太久就當作放棄**。
                    //
                    // 使用者按了 ↑↑ 然後改變主意去做別的事，那兩下不該
                    // 一直留著等下次的 ↓↓ 來湊成手勢。
                    let now = std::time::Instant::now();
                    if state
                        .last_gesture
                        .is_some_and(|t| now.duration_since(t) > GESTURE_TIMEOUT)
                    {
                        state.gesture.clear();
                    }
                    state.last_gesture = Some(now);

                    let cmd = ime_core::command::match_keys(state.session.keys());
                    // 組字內容不是指令的話，方向鍵就只是方向鍵
                    if cmd.is_none() {
                        state.gesture.clear();
                        state.session.enter_select_last();
                        // **上下鍵是「讓我看看有哪些字」**，所以一次到位——
                        // 框與清單一起出來，不必再按一次。左右鍵才是只出框。
                        state.session.open_cands();
                        show_candidates(context, &mut state)?;
                        return Ok(BOOL(1));
                    }

                    let hit = state.gesture.push(dir);
                    if hit {
                        state.gesture.clear();
                        end_composition(context, &mut state, EndKind::Cancel)?;
                        run_command(&mut state, cmd.expect("上面已經確認是指令"));
                    } else if state.gesture.promising() {
                        // **還有希望湊成手勢就先不要進選字**。
                        //
                        // 進了選字模式之後方向鍵的意義就變了（變成移動反白），
                        // 手勢再也收不到第二下——第一版就是敗在這裡。
                        // 這一下先吃掉，等後面幾下。
                    } else {
                        // 湊不成了（例如第一下按的是 ↓），當普通方向鍵處理
                        state.gesture.clear();
                        state.session.enter_select_last();
                        show_candidates(context, &mut state)?;
                    }
                }
                // Enter 在選字選單裡是「選中反白的候選字」，不是送出。
                //
                // 選完最後一格會自動離開選字模式（`confirm_cand` 回 true）——
                // 後面沒格子可選了，卡在原地會讓使用者按 Enter 沒反應。
                // 離開之後模式回到 Typing，這時再按 Enter 才是送出。
                Action::ConfirmCand => {
                    use ime_core::config::EnterInSelect;
                    // 使用者設定：選完往下一格（新注音式），還是直接退出
                    let advance = state.config.behavior.enter_in_select == EnterInSelect::Next;
                    let left_select = state.session.confirm_cand_with(advance);
                    // 設定成「最後一個字選完直接送出」的話，離開選字就送出
                    if left_select && state.config.behavior.commit_on_last {
                        let text = state.session.text();
                        end_composition(context, &mut state, EndKind::Commit(&text))?;
                    } else {
                        update_composition(self, context, &mut state)?;
                        show_candidates(context, &mut state)?;
                    }
                }
                // 組字中沒綁定的鍵——吃掉就好，不能放行給宿主（游標會跑掉）
                Action::Swallow => {}
                // 切換全半形：三態輪流，已經打好的標點也跟著重畫
                Action::ToggleWidth => {
                    let before = state.session.width();
                    state.session.toggle_width();
                    let after = state.session.width();

                    if !state.session.is_empty() {
                        if direct_input_mode(&state) {
                            // 鎖定英文時從全形切到半形＝進入「直接輸入」，
                            // 手上的組字要先送出去，理由同語言輪替那段
                            let text = state.session.text();
                            end_composition(context, &mut state, EndKind::Commit(&text))?;
                        } else {
                            // 組字中的話標點要重畫（半形變全形）
                            update_composition(self, context, &mut state)?;
                            show_candidates(context, &mut state)?;
                        }
                    }
                    show_width_window(context, &mut state, before, after)?;
                }
                Action::PickChar(n) => {
                    // **清單沒開就沒東西可以按號碼選**——那時畫面上只有框，
                    // 使用者看不到編號，按下去等於盲選
                    if !state.session.cands_open() {
                        return Ok(BOOL(0));
                    }
                    // **數字鍵認的是目前反白那一欄**——展開後每欄都各自
                    // 標 1-9，直接拿 n 當絕對索引會選到第一欄去
                    let pick = state
                        .session
                        .cand_number_index(n)
                        .and_then(|i| state.session.char_candidates().get(i).cloned());
                    if let Some(choice) = pick {
                        state.session.pick_char(&choice);
                        // **按號碼就是「我要這個」，選完直接收掉清單**
                        // （使用者定的）。方向鍵那條路是逐格慢慢挑，
                        // 數字鍵是知道答案的快捷鍵，挑完不該還留在選字裡。
                        state.session.exit_select();
                        update_composition(self, context, &mut state)?;
                        show_candidates(context, &mut state)?;
                    }
                }
            }

            Ok(BOOL(1))
        })
    }

    fn OnPreservedKey(&self, _pic: Ref<ITfContext>, _rguid: *const GUID) -> Result<BOOL> {
        crate::guard::key("OnPreservedKey", || Ok(BOOL(0)))
    }
}

impl State {
    /// 目前在哪個模式——按鍵綁定要靠它決定同一個鍵做什麼。
    ///
    /// 切法選單與選字模式還沒實作，先只分「有沒有在組字」。
    fn mode(&self) -> Mode {
        if self.session.is_empty() {
            Mode::Idle
        } else if self.session.select_index().is_some() {
            if self.session.cand_expanded() {
                Mode::SelectingExpanded
            } else {
                Mode::Selecting
            }
        } else if self.cutting_menu {
            Mode::CuttingMenu
        } else {
            Mode::Typing
        }
    }
}

/// 「快速按兩下 TAB」的判定時間。
///
/// 太短來不及按第二下，太長會把「按 TAB 翻頁」誤判成雙擊。
const DOUBLE_TAB: std::time::Duration = std::time::Duration::from_millis(400);

/// 手勢兩下之間最多隔多久。
///
/// 太短來不及按完四下，太長會把「剛才按過 ↑↑」記到下一次操作。
const GESTURE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1500);

/// 多久檢查一次設定檔有沒有改過。
///
/// 設定頁存檔後，最多隔這麼久就會生效（實務上是「切回文件打字」的
/// 那個空檔，感覺不到）。
const CONFIG_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// Ctrl 或 Alt 按著嗎？這種組合原則上留給宿主。
///
/// **例外是語言鎖定**（`Ctrl+Shift+B/J/E`）——那三個組合要我們自己
/// 收，見 `is_reserved_combo`。
/// `Ctrl+標點鍵`：鎖定注音時用它明講「我要標點」。
///
/// # 攔截的判準
///
/// **這個鍵印出來是標點，而且被注音佔用**——導出來剛好是 `,` `.` `;`
/// `/` `-` 五個（在大千配置上是 ㄝㄡㄤㄥㄦ）。
///
/// 兩個條件缺一不可：
///
/// - 不看「是不是注音鍵」就攔，會吃掉 `Ctrl+C`／`V`／`Z`／`S`——大千把
///   26 個字母全用掉了，那些字母鍵全都是注音鍵。
/// - 不看「是不是標點」就攔，會攔到 `=` `[` `]` `'` 這些**本來就打得出來**
///   的鍵，白白弄壞 `Ctrl+=`（縮放）、`Ctrl+[`（縮排）。
///
/// # 已知的衝突
///
/// 鎖定注音時 `Ctrl+-`（瀏覽器縮小）與 `Ctrl+/`（編輯器註解）會被我們
/// 接走。其他模式不受影響——那些模式的標點鍵本來就打得出標點。
fn ctrl_punct(vk: u32) -> Option<char> {
    unsafe {
        if GetKeyState(VK_CONTROL.0 as i32) >= 0 || GetKeyState(VK_MENU.0 as i32) < 0 {
            return None;
        }
    }
    let ch = keymap::typed_char(vk)?;
    ime_core::cutpoint::punct::is_ambiguous(ch).then_some(ch)
}

/// 送出時把使用者的選擇學起來。
///
/// # 守門在這裡，不在核心
///
/// **密碼欄與 `IS_PRIVATE` 的欄位不能學**（開發文件 §2.12.4——帳號、
/// 身分證、信用卡都在那裡，而學進去是持久的、存在硬碟上）。核心看不到
/// 那些訊號，所以判斷放在平台層。
///
/// 密碼欄其實根本不會走到這裡（那時完全不介入、不組字），這道守門是
/// **第二層保險**——那種代價的東西不該只靠一條路擋著。
///
/// # 為什麼順便存檔
///
/// 存檔要寫磁碟，不能放在每一鍵的熱路徑上；送出是天然的節點——使用者
/// 打完一段話，那一刻多幾毫秒感覺不到。淘汰也在存檔時做。
fn learn_from(state: &mut State) {
    if state.password {
        return;
    }
    if state.session.learn_on_commit() > 0 {
        let dir = crate::registration::data_dir();
        if let Err(e) = ime_core::learn::save(dir.as_deref()) {
            crate::dlog!("[學習] 存檔失敗：{e}");
        }
    }
}

fn is_modifier_down() -> bool {
    unsafe { (GetKeyState(VK_CONTROL.0 as i32) < 0) || (GetKeyState(VK_MENU.0 as i32) < 0) }
}

/// 把宿主 App 那支「會閃的插入點」移到目前組字文字的結尾。
///
/// 兩件事在 TSF 裡是分開的：`SetText` / `InsertTextAtSelection` 只負責把
/// 文字放進文件，**不會**順便移動插入點。不明講的話，宿主會把插入點
/// 畫回預設位置（實測在記事本會跑到文件最前面）。
///
/// **必須重新 `GetRange()`，不能重用寫入前的 range。**
/// `composition.GetRange()` 拿到的 range，其端點在 `SetText` 之後不會跟著
/// 新文字延展——它還記著舊的邊界。拿這種過時座標去 `SetSelection`，
/// 每個呼叫都回 S_OK，游標卻停在文字前面——這就是之前靠看 HRESULT
/// 怎麼查都查不出來的原因。
///
/// `range.Collapse()` 不能代替這個函式：它只調整 range 物件本身，
/// 不會通知宿主，所以宿主的游標不會動。
unsafe fn sync_caret_to_composition_end(
    context: &ITfContext,
    composition: &ITfComposition,
    ec: u32,
) -> Result<()> {
    unsafe {
        let fresh = composition.GetRange()?;
        let end = fresh.Clone()?;
        end.Collapse(ec, TF_ANCHOR_END)?;

        // TF_SELECTION.range 是 ManuallyDrop，釋放責任在呼叫端；SetSelection
        // 只讀取、不接手所有權，呼叫完要自己 drop，不然每按一次鍵
        // 就漏一個 ITfRange 的參考計數。
        let mut selection = [TF_SELECTION {
            range: std::mem::ManuallyDrop::new(Some(end)),
            style: TF_SELECTIONSTYLE {
                ase: TF_AE_END,
                fInterimChar: false.into(),
            },
        }];
        let result = context.SetSelection(ec, &selection);
        std::mem::ManuallyDrop::drop(&mut selection[0].range);
        result
    }
}

/// 結束 composition 的兩種意圖。
///
/// 分成具名的兩種而不是用 `Option<&str>`，是因為 `None` 很容易被讀成
/// 「取消」，但實際上「不寫入任何文字」跟「取消」是兩件事——見下方
/// `end_composition` 的說明。
enum EndKind<'a> {
    /// 送出：把組字範圍的內容換成這段文字，然後結束組字。
    Commit(&'a str),
    /// 取消：把組字範圍清空（寫入空字串），然後結束組字。
    Cancel,
}

// ── 測試 ──────────────────────────────────────────────
//
// **放在檔尾**：clippy 的 `items_after_test_module` 會抓「測試模組
// 後面還有正式程式碼」，那讓人以為檔案到此為止。

#[cfg(test)]
mod poison_tests {
    use super::*;

    /// 中毒復原之後，**下一次取鎖必須是正常路徑**。
    ///
    /// # 這條測試在擋什麼
    ///
    /// 第一版只做了 `into_inner()`，沒清旗標。結果是每一次 `lock()` 都
    /// 還是回 `Err`，每按一鍵都跑一次狀態重設——輸入法沒當機、也沒再
    /// panic，但每打一個字就把前一個吃掉。
    ///
    /// 實測 log 的形狀最清楚：攔下 panic 2 次，狀態鎖中毒 **85 次**。
    ///
    /// **型別檢查抓不到這個**，`catch_unwind` 的單元測試也抓不到——
    /// 它是「復原之後鎖還在不在中毒狀態」的語意問題，只有跑過才知道。
    #[test]
    fn 中毒復原之後不能還是中毒() {
        let m = Mutex::new(State::default());
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = m.lock().unwrap();
            panic!("裝的");
        }));
        assert!(m.is_poisoned(), "前提：panic 應該讓鎖中毒");

        drop(lock_state(&m));
        assert!(
            !m.is_poisoned(),
            "復原之後旗標必須清掉，否則之後每一鍵都會走重設路徑"
        );
        assert!(m.lock().is_ok(), "下一次取鎖要走正常路徑");
    }

    /// 沒中毒時不該動到狀態。
    #[test]
    fn 沒中毒就原樣拿到() {
        let m = Mutex::new(State::default());
        {
            let mut g = lock_state(&m);
            g.cutting_menu = true;
        }
        assert!(lock_state(&m).cutting_menu, "正常路徑不該重設任何東西");
    }

    /// 中毒那一次要把組字狀態清乾淨——panic 可能停在組字的一半。
    #[test]
    fn 中毒那一次要重設組字狀態() {
        let m = Mutex::new(State::default());
        {
            let mut g = m.lock().unwrap();
            g.cutting_menu = true;
        }
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = m.lock().unwrap();
            panic!("裝的");
        }));
        assert!(!lock_state(&m).cutting_menu, "中毒復原要把組字狀態清掉");
    }
}
