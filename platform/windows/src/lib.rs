//! 通譯輸入法（通 · つなぎ · Tsunagi）的 Windows TSF Text Input Processor。
//!
//! 這顆 DLL 是一個標準 in-proc COM 伺服器，透過 `DllGetClassObject`
//! 曝露唯一的 CLSID（見 `guids::CLSID_TEXT_SERVICE`），TSF 用它建立
//! `ITfTextInputProcessor` 執行個體。註冊/反註冊（`DllRegisterServer` /
//! `DllUnregisterServer`）另外還會呼叫 TSF 的 profile / category API，
//! 讓這顆文字服務出現在系統的輸入法清單裡。
//!
//! 詳細踩坑記錄見開發文件.md 第三章「TSF 踩坑筆記」。

mod candidate_window;
mod class_factory;
pub mod d2d; // `pub` 是為了讓 bin/bench_render、bin/spike_render 用得到
pub mod debug_log;
mod display_attribute;
mod edit_session;
mod guard;
// register_tool（同 crate 的 bin）要用這裡的 CLSID 與 profile GUID
// 去啟用／停用輸入法，所以是 pub
pub mod guids;
mod keymap;
mod keyprobe;
mod lang_bar;
mod lang_menu;
mod preview_window;
mod registration;
mod text_service;
mod theme;
mod width_window;

use core::ffi::c_void;

use windows::core::{Interface, GUID, HRESULT};
use windows::Win32::Foundation::CLASS_E_CLASSNOTAVAILABLE;

use class_factory::ClassFactory;

const S_OK: HRESULT = HRESULT(0);
const S_FALSE: HRESULT = HRESULT(1);

/// Windows 載入器把這顆 DLL 映射進行程時最先呼叫的東西。
///
/// # 為什麼要有（只為了量一行時間）
///
/// 量 LOL 的卡頓時遇到一個死角：整份 log 每一步都是毫秒級，使用者
/// 卻實際等了十幾秒——**那段空白落在第一行 log 之前**。`DllGetClassObject`
/// 是 COM 要物件的時刻，而在那之前還有一整段「載入器映射模組、跑
/// 相依項、（有反作弊的話）掃描剛進來的程式碼」，那段我們量不到。
///
/// 這支只做一件事：在 `PROCESS_ATTACH` 記一行。它跟後面
/// `[載入] DllGetClassObject` 的時間差，就是那段空白有多長。
///
/// # 為什麼這樣寫是安全的
///
/// `DllMain` 裡幾乎什麼都不能做——載入器鎖著一把全域鎖，在裡面碰
/// COM、開執行緒、載別的 DLL 都可能死鎖。所以這裡**只寫一行 log**
/// （開檔、寫入、關檔），而且只在 `PROCESS_ATTACH` 那一次。
///
/// 除錯開關關著的時候整支是一個原子讀取就回來了，正式使用沒有成本。
///
/// # Safety
/// 由 Windows 載入器呼叫，簽章必須與 `BOOL DllMain(HINSTANCE, DWORD, LPVOID)`
/// 相符。
#[no_mangle]
unsafe extern "system" fn DllMain(_hinst: *mut c_void, reason: u32, _reserved: *mut c_void) -> i32 {
    const DLL_PROCESS_ATTACH: u32 = 1;
    if reason == DLL_PROCESS_ATTACH {
        // **這是整份 log 的第一行**，也是相對毫秒的原點。
        crate::debug_log::log("[載入] DllMain PROCESS_ATTACH");
    }
    1 // TRUE：載入成功
}

/// COM 用這個入口向 DLL 要一個「類別工廠」，再由工廠生出實際物件。
///
/// # Safety
/// 由 COM 執行期以標準 in-proc server 慣例呼叫；`rclsid`/`riid` 必須是
/// 有效指標，`ppv` 必須是可寫入指標指標。
#[no_mangle]
unsafe extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    // **最早的進入點**——在這裡裝 panic 攔截器，之後任何 panic
    // 都會留下線索（見 `debug_log::install_panic_hook`）
    crate::debug_log::install_panic_hook();
    // 這一行是**量卡頓的原點**。它與後面 `[啟用] Activate` 的時間差
    // ＝「宿主決定用這個輸入法」到「我們開始初始化」之間的空白；那段
    // 不在我們的程式裡，卡在那裡代表是外部因素（例如反作弊逐一掃描
    // 剛載入的模組），不是我們能改的。見相容性測試清單 H11。
    crate::dlog!("[載入] DllGetClassObject");
    unsafe {
        if ppv.is_null() {
            return windows::core::Error::from(windows::Win32::Foundation::E_POINTER).code();
        }
        *ppv = std::ptr::null_mut();
        if *rclsid != guids::CLSID_TEXT_SERVICE {
            return CLASS_E_CLASSNOTAVAILABLE;
        }
        let factory: windows::core::IUnknown = ClassFactory.into();
        factory.query(riid, ppv)
    }
}

/// Phase 0 原型：不追蹤全域物件計數，一律回報「還不能卸載」，
/// 避免過早卸載造成 use-after-free；正式版才需要做精確的 refcount 追蹤。
///
/// # Safety
/// 由 COM 執行期呼叫。
#[no_mangle]
unsafe extern "system" fn DllCanUnloadNow() -> HRESULT {
    S_FALSE
}

/// # Safety
/// 由 `regsvr32` 或安裝流程呼叫。
#[no_mangle]
unsafe extern "system" fn DllRegisterServer() -> HRESULT {
    match registration::register() {
        Ok(()) => S_OK,
        Err(e) => e.code(),
    }
}

/// # Safety
/// 由 `regsvr32 /u` 或解除安裝流程呼叫。
#[no_mangle]
unsafe extern "system" fn DllUnregisterServer() -> HRESULT {
    match registration::unregister() {
        Ok(()) => S_OK,
        Err(e) => e.code(),
    }
}
