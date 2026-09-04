//! 跟宿主打聽「現在這個文件是什麼狀況」。
//!
//! 這裡的每一件事都**只能問 TSF，不能用 Win32 API 猜**：
//!
//! - 插入點在哪 → `ITfContextView::GetTextExt`（`GetCursorPos` 拿的是
//!   滑鼠位置，跟插入點無關）
//! - 是不是密碼欄 → 輸入範圍與停用旗標**兩條都要看**，原生程式與
//!   Chromium 系瀏覽器各用各的
//!
//! **問不到就當作不是密碼**：寧可漏判也不要誤判——誤判的話一般欄位
//! 突然打不了中文，使用者會以為輸入法整個壞了。

use super::*;

/// 沒組字時，向 TSF 問游標（長度 0 的選取範圍）在螢幕哪裡。
///
/// 問不到回 `None`——有些宿主在沒有 edit session 的情況下不給選取範圍。
/// 宿主有沒有明講「這裡不要用輸入法」？
///
/// # 為什麼這比輸入範圍更根本
///
/// 密碼欄位有兩種表達方式，**應用程式各用各的**：
///
/// - 宣告輸入範圍是 `IS_PASSWORD`（見 `is_password_field`）
/// - 直接把「鍵盤停用」這個旗標打開——Chromium 系的瀏覽器走這條
///
/// 實測 Brave：四種欄位（一般、密碼、關閉自動填入、搜尋）回報的輸入
/// 範圍全都是 `IS_PRIVATE`，**分不出哪個是密碼**。所以只看輸入範圍
/// 在瀏覽器裡是無效的。
///
/// 這個旗標本來就該檢查——它的語意是「宿主要求輸入法退場」，密碼欄
/// 只是最常見的用途。不理它等於無視宿主的明確要求。
pub(crate) fn keyboard_disabled(context: &ITfContext, thread_mgr: Option<&ITfThreadMgr>) -> bool {
    use windows::Win32::UI::TextServices::{
        ITfCompartmentMgr, GUID_COMPARTMENT_EMPTYCONTEXT, GUID_COMPARTMENT_KEYBOARD_DISABLED,
    };
    unsafe {
        let on = |mgr: &ITfCompartmentMgr, guid: &windows::core::GUID| -> bool {
            mgr.GetCompartment(guid)
                .ok()
                .and_then(|c| c.GetValue().ok())
                .and_then(|v| i32::try_from(&v).ok())
                .is_some_and(|n| n != 0)
        };
        // **兩個地方都要看**：旗標可以掛在這份文件上，也可以掛在整條
        // 執行緒上，宿主用哪一個沒有規定。
        if let Ok(mgr) = context.cast::<ITfCompartmentMgr>() {
            if on(&mgr, &GUID_COMPARTMENT_KEYBOARD_DISABLED)
                || on(&mgr, &GUID_COMPARTMENT_EMPTYCONTEXT)
            {
                return true;
            }
        }
        if let Some(tm) = thread_mgr {
            if let Ok(mgr) = tm.cast::<ITfCompartmentMgr>() {
                if on(&mgr, &GUID_COMPARTMENT_KEYBOARD_DISABLED) {
                    return true;
                }
            }
        }
        false
    }
}

/// 這個文件是密碼欄位嗎？
///
/// # 為什麼一定要判
///
/// 不判的話，使用者在密碼框打字時**密碼會原封不動顯示在候選視窗上**
/// ——旁邊的人看得一清二楚，螢幕分享和截圖也會錄進去。這是輸入法
/// 最基本的一條安全底線。
///
/// # 怎麼判
///
/// 應用程式會透過 TSF 的「輸入範圍」宣告這個欄位要收什麼，密碼欄位
/// 標的是 `IS_PASSWORD`。
///
/// **但這條路在瀏覽器裡完全無效**。實測 Brave（Chromium 系），七種
/// 欄位——一般文字、密碼、`type=email`、`autocomplete=username`、
/// `autocomplete=email`、`type=url`、關掉自動填入——**回報的全都是
/// `IS_PRIVATE`**，一個都分不出來。密碼欄靠的是另一條路，
/// 見 `keyboard_disabled`。
///
/// 順帶把一個想法判死：本來想靠這個在帳號／email 欄位自動切成英文
/// （帳號一定是英文，被判成日文很煩），實測證明**辦不到**——那些
/// 欄位跟一般欄位長得一模一樣。那件事的解法是語言鎖定鍵與 Phase 4
/// 的個人化，不是輸入範圍。
///
/// 留著這條是因為**非瀏覽器的程式確實會用它**，兩條一起看才蓋得全。
///
/// **不去猜視窗樣式**：那要跨行程問 hwnd，而瀏覽器裡的欄位根本沒有
/// 自己的 hwnd，猜不到。
///
/// 要 edit cookie 才問得到屬性，所以得走一次唯讀的編輯工作階段。
/// 問不到就當作不是密碼——**寧可漏判也不要誤判**：誤判的話一般欄位
/// 突然打不了中文，使用者會以為輸入法壞了。
pub(crate) fn is_password_field(context: &ITfContext, tid: u32) -> bool {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::TextServices::{
        ITfInputScope, InputScope, GUID_PROP_INPUTSCOPE, IS_PASSWORD,
    };

    let found = Rc::new(std::cell::Cell::new(false));
    let slot = found.clone();
    let ctx = context.clone();
    let session = EditSession::new(move |ec| unsafe {
        let mut sel = [TF_SELECTION::default(); 1];
        let mut fetched = 0u32;
        if ctx
            .GetSelection(ec, TF_DEFAULT_SELECTION, &mut sel, &mut fetched)
            .is_err()
            || fetched == 0
        {
            return Ok(());
        }
        let answer = (|| -> Result<bool> {
            let range = sel[0]
                .range
                .as_ref()
                .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
            let prop = ctx.GetAppProperty(&GUID_PROP_INPUTSCOPE)?;
            let var = prop.GetValue(ec, range)?;
            let scope: ITfInputScope = windows::core::IUnknown::try_from(&var)?.cast()?;
            let mut ptr: *mut InputScope = std::ptr::null_mut();
            let mut count = 0u32;
            scope.GetInputScopes(&mut ptr, &mut count)?;
            if ptr.is_null() {
                return Ok(false);
            }
            let is_pw = std::slice::from_raw_parts(ptr, count as usize).contains(&IS_PASSWORD);
            // **這塊記憶體是 TSF 配的，要我們自己還**
            CoTaskMemFree(Some(ptr as *const std::ffi::c_void));
            Ok(is_pw)
        })();
        slot.set(answer.unwrap_or(false));
        // 用完自己 drop，理由同 `caret_from_selection`
        std::mem::ManuallyDrop::drop(&mut sel[0].range);
        Ok(())
    });
    let session_interface: ITfEditSession = session.into();
    unsafe {
        let _ = context.RequestEditSession(tid, &session_interface, TF_ES_READ | TF_ES_SYNC);
    }
    found.get()
}

/// 開始新一輪輸入時重新判斷是不是密碼欄位。
///
/// **每次都問太貴**（要跑一次編輯工作階段），組字中途焦點也不會換，
/// 所以只在「還沒開始組字」時問——剛好涵蓋每次換欄位之後的第一鍵。
pub(crate) fn refresh_password(context: &ITfContext, state: &mut State) {
    if !state.session.is_empty() {
        return;
    }
    // **兩種訊號都要看**，應用程式各用各的——見兩個函式的說明
    // **兩種訊號都要看**，應用程式各用各的——見兩個函式的說明
    let now = keyboard_disabled(context, state.thread_mgr.as_ref())
        || is_password_field(context, state.client_id);
    // 只在**變了**的時候記一筆——每一鍵都記會把 log 洗掉
    if now != state.password {
        crate::dlog!(
            "[密碼] 欄位判定改變 → {}",
            if now { "是密碼欄" } else { "一般欄位" }
        );
    }
    state.password = now;
}

/// 組字範圍在螢幕上的哪裡？候選視窗與全半形視窗都靠它定位。
///
/// 向 TSF 問（`GetTextExt`），問不到才走退路——`GetGUIThreadInfo`
/// 在多行程 App（瀏覽器）查不到，見 `candidate_window` 的說明。
pub(crate) fn caret_anchor(context: &ITfContext, state: &State) -> RECT {
    let Some(composition) = state.composition.clone() else {
        // **沒組字時改問「目前選取範圍」**。
        //
        // 游標處是一個長度 0 的範圍，對它 `GetTextExt` 一樣拿得到
        // 螢幕座標。這條路在多行程 App（瀏覽器）也有效，因為問的是
        // TSF 而不是 Win32——`GetGUIThreadInfo` 在那些 App 查不到，
        // 會一路掉到「滑鼠位置」，看起來就像視窗跟著滑鼠跑。
        return caret_from_selection(context, state)
            .or_else(|| crate::candidate_window::screen_ext(context))
            .unwrap_or_else(|| {
                crate::dlog!("[定位] 沒組字、選取範圍與整塊區域都問不到 → 退路");
                caret_screen_position_fallback()
            });
    };
    let context_owned = context.clone();
    let slot: Rc<RefCell<Option<RECT>>> = Rc::new(RefCell::new(None));
    let slot_write = slot.clone();
    let session = EditSession::new(move |ec| unsafe {
        let range = composition.GetRange()?;
        *slot_write.borrow_mut() =
            Some(caret_screen_position_from_range(&context_owned, ec, &range));
        Ok(())
    });
    let session_interface: ITfEditSession = session.into();
    unsafe {
        // 這裡只讀不寫，用 TF_ES_READ 就夠——要求寫權限會讓宿主更容易拒絕。
        if context
            .RequestEditSession(state.client_id, &session_interface, TF_ES_READ | TF_ES_SYNC)
            .is_err()
        {
            crate::dlog!("[定位] 宿主拒絕同步 edit session（組字中）→ 退路");
            return crate::candidate_window::screen_ext(context)
                .unwrap_or_else(caret_screen_position_fallback);
        }
    }
    let taken = slot.borrow_mut().take();
    taken.unwrap_or_else(caret_screen_position_fallback)
}

pub(crate) fn caret_from_selection(context: &ITfContext, state: &State) -> Option<RECT> {
    let context_owned = context.clone();
    let slot: Rc<RefCell<Option<RECT>>> = Rc::new(RefCell::new(None));
    let slot_write = slot.clone();
    let session = EditSession::new(move |ec| unsafe {
        let mut sel = [TF_SELECTION::default(); 1];
        let mut fetched = 0u32;
        context_owned.GetSelection(ec, TF_DEFAULT_SELECTION, &mut sel, &mut fetched)?;
        if fetched == 0 {
            return Ok(());
        }
        // `range` 是 ManuallyDrop，取出來用完由這個 scope 負責釋放
        if let Some(range) = sel[0].range.as_ref() {
            *slot_write.borrow_mut() =
                Some(caret_screen_position_from_range(&context_owned, ec, range));
        }
        // 明確釋放，不然 COM 參考數會漏
        std::mem::ManuallyDrop::drop(&mut sel[0].range);
        Ok(())
    });
    let session_interface: ITfEditSession = session.into();
    unsafe {
        // 兩層都要看：外層是呼叫本身，內層 HRESULT 是宿主的答覆。
        // 宿主可能拒絕同步 edit session，那就沒有座標可用。
        let hr = context
            .RequestEditSession(state.client_id, &session_interface, TF_ES_READ | TF_ES_SYNC)
            .ok()?;
        if hr.is_err() {
            return None;
        }
    }
    let taken = slot.borrow_mut().take();
    taken
}

/// 目前有輸入焦點的那個文件的 context。
///
/// **按鍵事件會由 TSF 把 `context` 傳進來，滑鼠點擊不會**——那是視窗
/// 訊息，跟 TSF 無關。所以滑鼠這條路要自己問 thread manager。
pub(crate) fn focused_context(state: &State) -> Option<ITfContext> {
    let tm = state.thread_mgr.as_ref()?;
    unsafe { tm.GetFocus().ok()?.GetTop().ok() }
}

/// 現在是「打什麼直接進文件」的狀態嗎？
///
/// 條件是**鎖定英文**且**不是全形模式**。見 `insert_directly`。
pub(crate) fn direct_input_mode(state: &State) -> bool {
    use ime_core::language::Language;
    use ime_core::width::Width;
    state.session.lock() == Some(Language::English) && state.session.width() != Width::Full
}
