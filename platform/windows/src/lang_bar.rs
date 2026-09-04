//! 工作列的狀態指示器（TSF 語言列）。
//!
//! ```text
//! 工作列右下角
//!   ┌────┐
//!   │ 自 │ ← 這個（自動／注／日／英）
//!   └────┘
//!    點一下 → 輪替語言模式
//!    右鍵   → 選單（切模式、開設定）
//! ```
//!
//! 就是微軟注音「中／英」那個位置。TSF 有官方 API
//! （`ITfLangBarItemButton`），不必自己畫視窗或塞系統匣圖示。
//!
//! # 要實作哪些介面
//!
//! | 介面 | 做什麼 |
//! |---|---|
//! | `ITfLangBarItem` | 這個項目叫什麼、要不要顯示 |
//! | `ITfLangBarItemButton` | 點擊、右鍵選單、顯示的文字 |
//! | `ITfSource` | 讓語言列訂閱「狀態變了」的通知 |
//!
//! # 為什麼需要 `ITfSource`
//!
//! 語言列**不會主動來問**目前是什麼狀態——它訂閱通知，我們在狀態
//! 改變時呼叫 `OnUpdate` 告訴它「重新問一次」。沒有這一環的話，
//! 使用者按 Ctrl 切了模式，工作列上的字不會跟著變。

use std::cell::RefCell;
use windows::core::{implement, Interface, Ref, Result, BOOL, BSTR, GUID, PCWSTR};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, POINT, RECT};
use windows::Win32::Graphics::Gdi::HBITMAP;
use windows::Win32::UI::TextServices::{
    ITfLangBarItem, ITfLangBarItemButton, ITfLangBarItemButton_Impl, ITfLangBarItemMgr,
    ITfLangBarItemSink, ITfLangBarItem_Impl, ITfMenu, ITfSource, ITfSource_Impl, ITfThreadMgr,
    TfLBIClick, TF_LANGBARITEMINFO, TF_LBI_STYLE_BTN_BUTTON, TF_LBI_STYLE_BTN_MENU,
    TF_LBI_STYLE_SHOWNINTRAY,
};
use windows::Win32::UI::WindowsAndMessaging::HICON;

use ime_core::language::Language;

/// 語言列項目的 GUID。
///
/// **一定要用系統保留的 `GUID_LBI_INPUTMODE`，不能用自訂值**——
/// Windows 8 起，`GetInfo` 回報的 GUID 不是這個的話，項目會被
/// **無聲忽略**：`AddItem` 照樣回 `S_OK`、`GetInfo`／`GetText` 照樣
/// 被反覆呼叫，但工作列就是不畫出來。本專案曾為此查很久，見
/// 開發文件 §3.7。
///
/// 另注意：同一條執行緒只能有一個這種項目——系統只顯示第一個，
/// 重複掛會讓舊的佔住位置。
pub use windows::Win32::UI::TextServices::GUID_LBI_INPUTMODE as GUID_LANGBAR_ITEM;

/// 選單項目的編號。
mod menu_id {
    pub const AUTO: u32 = 1;
    pub const BOPOMOFO: u32 = 2;
    pub const ROMAJI: u32 = 3;
    pub const ENGLISH: u32 = 4;
    /// 分隔線不需要編號，這裡留給「開啟設定」
    pub const SETTINGS: u32 = 10;
}

/// 目前顯示什麼字。
///
/// 跟候選視窗的提示視窗用同一套標籤（自／注／日／英），
/// 兩處看到的東西才一致。
fn label_of(lock: Option<Language>) -> &'static str {
    match lock {
        None => "自",
        Some(Language::Bopomofo) => "注",
        Some(Language::Romaji) => "日",
        Some(Language::English) => "英",
    }
}

/// 每個模式對應的圖示資源 ID（跟 `res/ime.rc` 裡的編號一致）。
fn icon_id_of(lock: Option<Language>) -> u16 {
    match lock {
        None => 10,
        Some(Language::Bopomofo) => 11,
        Some(Language::Romaji) => 12,
        Some(Language::English) => 13,
    }
}

/// 從本 DLL 的資源載入模式圖示。
///
/// 兩個容易踩的點：
/// 1. **要從本 DLL 的 module handle 載入**，不能傳 `None`——那會去
///    呼叫端行程（宿主 App）裡找，當然找不到。
/// 2. **回傳的圖示所有權歸呼叫端**，它用完會銷毀。所以不能把快取的
///    handle 直接交出去，每次都要 `LoadImage` 一份新的（帶
///    `LR_SHARED` 的話系統會共用並自行管理生命週期，更安全）。
fn load_mode_icon(lock: Option<Language>) -> Result<HICON> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, LoadImageW, IMAGE_ICON, LR_SHARED, SM_CXSMICON, SM_CYSMICON,
    };
    let module = crate::registration::dll_module()?.into();
    // 用系統的小圖示尺寸（一般是 16×16，高 DPI 下會更大）
    let (cx, cy) = unsafe { (GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON)) };
    let handle = unsafe {
        LoadImageW(
            Some(module),
            PCWSTR(icon_id_of(lock) as usize as *const u16),
            IMAGE_ICON,
            cx,
            cy,
            LR_SHARED,
        )?
    };
    Ok(HICON(handle.0))
}

/// 工作列上的狀態按鈕。
#[implement(ITfLangBarItem, ITfLangBarItemButton, ITfSource)]
pub struct LangBarButton {
    /// 目前的語言模式。**這是一份快取**——真正的狀態在
    /// `text_service` 的 `Session` 裡，那邊變了要呼叫 `set_lock` 同步。
    lock: RefCell<Option<Language>>,
    /// 語言列訂閱的通知埠。狀態變了要通知它重新問一次。
    sink: RefCell<Option<ITfLangBarItemSink>>,
    /// `AdviseSink` 給出去的 cookie，`UnadviseSink` 要用它比對。
    cookie: RefCell<u32>,
    /// 使用者點了按鈕或選單時要做什麼。
    ///
    /// 語言列跟輸入法本體是兩條路——按鈕在這裡被按下，但實際的
    /// 狀態在 `text_service`。用回呼把兩邊接起來。
    on_action: Box<dyn Fn(Action)>,
}

/// 使用者在語言列上做了什麼。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// 輪到下一個模式（點擊按鈕）
    Cycle,
    /// 直接切到指定模式（選單）
    SetLock(Option<Language>),
    /// 開啟設定頁
    OpenSettings,
    /// 右鍵：在這個螢幕座標彈出選單
    ///
    /// 交給 `text_service` 處理——選單要知道目前鎖定的語言、啟用了
    /// 哪些引擎、還有主題，那些狀態都在那邊。
    ///
    /// 用 `(x, y)` 而不是 `POINT`：後者沒有實作 `Eq`，這個 enum 就
    /// 推導不出 `PartialEq`。
    OpenMenu(i32, i32),
}

/// 滑鼠鍵的編號。`windows` crate 沒給具名常數，這裡照實測值定義。
///
/// **編號不是 0 開頭，而且右鍵在前**——原本猜 `0` 是左鍵，條件永遠
/// 不成立；改成 `1` 之後變成右鍵在切換。實測：**1 = 右鍵、2 = 左鍵**。
const TF_LBI_CLK_RIGHT: TfLBIClick = TfLBIClick(1);
const TF_LBI_CLK_LEFT: TfLBIClick = TfLBIClick(2);

/// 這個 cookie 值代表「還沒有人訂閱」。
const NO_COOKIE: u32 = 0;
/// 給出去的 cookie。固定一個值就好——同時只會有一個訂閱者。
const SINK_COOKIE: u32 = 0x1234;

impl LangBarButton {
    pub fn new(on_action: impl Fn(Action) + 'static) -> Self {
        Self {
            lock: RefCell::new(None),
            sink: RefCell::new(None),
            cookie: RefCell::new(NO_COOKIE),
            on_action: Box::new(on_action),
        }
    }
}

impl LangBarButton_Impl {
    /// 換掉顯示的模式，並通知語言列重畫。
    ///
    /// **一定要通知**——語言列不會主動來問，不通知的話使用者按了
    /// Ctrl 切模式，工作列上的字不會變。
    pub fn set_lock(&self, lock: Option<Language>) {
        if *self.lock.borrow() == lock {
            return;
        }
        *self.lock.borrow_mut() = lock;
        if let Some(sink) = self.sink.borrow().as_ref() {
            unsafe {
                // `TF_LBI_STATUS`(0x1)：狀態變了；`TF_LBI_TEXT`(0x2)：字變了；
                // **`TF_LBI_ICON`(0x4)：圖示變了**——工作列那格畫的是圖，
                // 少了這個旗標圖不會重新載入，切模式看起來像沒反應。
                let _ = sink.OnUpdate(0x1 | 0x2 | 0x4);
            }
        }
    }
}

impl ITfLangBarItem_Impl for LangBarButton_Impl {
    fn GetInfo(&self, pinfo: *mut TF_LANGBARITEMINFO) -> Result<()> {
        if pinfo.is_null() {
            return Err(E_INVALIDARG.into());
        }
        unsafe {
            let info = &mut *pinfo;
            info.clsidService = crate::guids::CLSID_TEXT_SERVICE;
            info.guidItem = GUID_LANGBAR_ITEM;
            // **`BTN_BUTTON`**：左鍵有動作；**`BTN_MENU`**：右鍵有選單。
            // 兩個一起給，才能像微軟注音那樣「點了切換、右鍵開選單」。
            //
            // **`SHOWNINTRAY`：要顯示在工作列**。官方常數說明寫「目前
            // 不支援」，但微軟自己的 SampleIME 不只設了它，還在每次
            // `GetInfo` 重設一遍——那種防禦性寫法代表系統真的會看。
            // 照做。
            info.dwStyle =
                TF_LBI_STYLE_BTN_BUTTON | TF_LBI_STYLE_BTN_MENU | TF_LBI_STYLE_SHOWNINTRAY;
            info.ulSort = 0;
            // 說明文字（滑鼠停留時顯示）
            let desc: Vec<u16> = "通 · つなぎ 輸入法".encode_utf16().collect();
            let n = desc.len().min(info.szDescription.len() - 1);
            info.szDescription[..n].copy_from_slice(&desc[..n]);
            info.szDescription[n] = 0;
        }
        Ok(())
    }

    fn GetStatus(&self) -> Result<u32> {
        // 0 = 正常顯示、可以按
        Ok(0)
    }

    fn Show(&self, _fshow: BOOL) -> Result<()> {
        // 這個項目一直顯示，不接受隱藏
        Ok(())
    }

    fn GetTooltipString(&self) -> Result<BSTR> {
        let mode = match *self.lock.borrow() {
            None => "自動辨識",
            Some(Language::Bopomofo) => "注音",
            Some(Language::Romaji) => "日文",
            Some(Language::English) => "英文",
        };
        Ok(BSTR::from(format!("通 · つなぎ 輸入法－{mode}")))
    }
}

impl ITfLangBarItemButton_Impl for LangBarButton_Impl {
    fn OnClick(&self, click: TfLBIClick, pt: &POINT, _prcarea: *const RECT) -> Result<()> {
        // 左鍵輪替模式；右鍵不處理，交給系統去叫 `InitMenu` 開選單。
        if click == TF_LBI_CLK_LEFT {
            (self.on_action)(Action::Cycle);
        } else if click == TF_LBI_CLK_RIGHT {
            // **Win11 的指示器不會呼叫 `InitMenu`**（實測確認，微軟論壇
            // 有人問過至今無解），所以選單得自己彈。見 `lang_menu`。
            (self.on_action)(Action::OpenMenu(pt.x, pt.y));
        }
        Ok(())
    }

    fn InitMenu(&self, pmenu: Ref<ITfMenu>) -> Result<()> {
        let Some(menu) = pmenu.as_ref() else {
            crate::dlog!("[langbar] InitMenu 拿到空的 menu 指標");
            return Err(E_FAIL.into());
        };
        let cur = *self.lock.borrow();
        unsafe {
            for (id, label, lock) in [
                (menu_id::AUTO, "自動辨識", None),
                (menu_id::BOPOMOFO, "注音", Some(Language::Bopomofo)),
                (menu_id::ROMAJI, "日文", Some(Language::Romaji)),
                (menu_id::ENGLISH, "英文", Some(Language::English)),
            ] {
                let text: Vec<u16> = label.encode_utf16().collect();
                // 目前那個打勾
                let flags = if cur == lock { 0x8 } else { 0 }; // TF_LBMENUF_CHECKED
                menu.AddMenuItem(
                    id,
                    flags,
                    HBITMAP::default(),
                    HBITMAP::default(),
                    &text,
                    std::ptr::null_mut(),
                )?;
            }
            // 分隔線
            // 分隔線：`TF_LBMENUF_SEPARATOR`，沒有文字
            menu.AddMenuItem(
                0,
                0x1,
                HBITMAP::default(),
                HBITMAP::default(),
                &[],
                std::ptr::null_mut(),
            )?;
            let text: Vec<u16> = "設定…".encode_utf16().collect();
            menu.AddMenuItem(
                menu_id::SETTINGS,
                0,
                HBITMAP::default(),
                HBITMAP::default(),
                &text,
                std::ptr::null_mut(),
            )?;
        }
        Ok(())
    }

    fn OnMenuSelect(&self, wid: u32) -> Result<()> {
        let action = match wid {
            menu_id::AUTO => Action::SetLock(None),
            menu_id::BOPOMOFO => Action::SetLock(Some(Language::Bopomofo)),
            menu_id::ROMAJI => Action::SetLock(Some(Language::Romaji)),
            menu_id::ENGLISH => Action::SetLock(Some(Language::English)),
            menu_id::SETTINGS => Action::OpenSettings,
            _ => return Ok(()),
        };
        (self.on_action)(action);
        Ok(())
    }

    fn GetIcon(&self) -> Result<HICON> {
        // **工作列那格只畫圖，不畫文字**。`GetText` 給的字只會用在
        // tooltip 與傳統語言列——這裡不回傳有效圖示的話，那格就是空的。
        let icon = load_mode_icon(*self.lock.borrow());
        icon
    }

    fn GetText(&self) -> Result<BSTR> {
        let t = label_of(*self.lock.borrow());
        Ok(BSTR::from(t))
    }
}

impl ITfSource_Impl for LangBarButton_Impl {
    fn AdviseSink(&self, riid: *const GUID, punk: Ref<windows::core::IUnknown>) -> Result<u32> {
        unsafe {
            // 只接受語言列的通知埠
            if riid.is_null() || *riid != ITfLangBarItemSink::IID {
                return Err(windows::Win32::System::Ole::CONNECT_E_CANNOTCONNECT.into());
            }
            if *self.cookie.borrow() != NO_COOKIE {
                return Err(windows::Win32::System::Ole::CONNECT_E_ADVISELIMIT.into());
            }
            let sink: ITfLangBarItemSink = punk.ok()?.cast()?;
            *self.sink.borrow_mut() = Some(sink);
            *self.cookie.borrow_mut() = SINK_COOKIE;
            Ok(SINK_COOKIE)
        }
    }

    fn UnadviseSink(&self, dwcookie: u32) -> Result<()> {
        if dwcookie != *self.cookie.borrow() {
            return Err(windows::Win32::System::Ole::CONNECT_E_NOCONNECTION.into());
        }
        *self.sink.borrow_mut() = None;
        *self.cookie.borrow_mut() = NO_COOKIE;
        Ok(())
    }
}

/// 把按鈕掛上語言列。
///
/// 傳進來的是**介面**而不是具體型別——`RemoveItem` 也要同一個
/// 介面物件才拿得掉，所以呼叫端留著它。
pub fn install(thread_mgr: &ITfThreadMgr, item: &ITfLangBarItem) -> Result<()> {
    unsafe {
        // 兩個失敗點分開記——`cast` 拿不到管理員，跟拿到了但 `AddItem`
        // 被拒絕，是完全不同的兩件事，混在一起會查錯方向。
        let mgr: ITfLangBarItemMgr = match thread_mgr.cast() {
            Ok(m) => m,
            Err(e) => {
                crate::dlog!("[langbar] cast ITfLangBarItemMgr 失敗: {e:?}");
                return Err(e);
            }
        };
        // 只記失敗——成功是常態，每次啟用都寫一行只是噪音
        mgr.AddItem(item).inspect_err(|e| {
            crate::dlog!("[langbar] AddItem 失敗: {e:?}");
        })
    }
}

/// 從語言列拿掉。**輸入法停用時一定要做**——不然工作列上會留下
/// 一個點了沒反應的按鈕。
pub fn remove(thread_mgr: &ITfThreadMgr, item: &ITfLangBarItem) -> Result<()> {
    unsafe {
        let mgr: ITfLangBarItemMgr = thread_mgr.cast()?;
        mgr.RemoveItem(item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 標籤跟提示視窗一致() {
        // 兩處看到的東西要一樣，不然使用者會困惑
        assert_eq!(label_of(None), "自");
        assert_eq!(label_of(Some(Language::Bopomofo)), "注");
        assert_eq!(label_of(Some(Language::Romaji)), "日");
        assert_eq!(label_of(Some(Language::English)), "英");
    }

    #[test]
    fn 標籤都是單字() {
        for l in [
            None,
            Some(Language::Bopomofo),
            Some(Language::Romaji),
            Some(Language::English),
        ] {
            assert_eq!(label_of(l).chars().count(), 1, "工作列空間小，要單字");
        }
    }
}
