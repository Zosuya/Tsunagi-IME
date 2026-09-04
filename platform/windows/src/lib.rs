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
mod registration;
mod slide;
mod text_service;
mod theme;
mod width_bar;
mod width_window;

use core::ffi::c_void;

use windows::core::{Interface, GUID, HRESULT};
use windows::Win32::Foundation::CLASS_E_CLASSNOTAVAILABLE;

use class_factory::ClassFactory;

const S_OK: HRESULT = HRESULT(0);
const S_FALSE: HRESULT = HRESULT(1);

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
    // **最早的進入點**——在這裡裝 panic 攔截器，之後任何 panic
    // 都會留下線索（見 `debug_log::install_panic_hook`）
    crate::debug_log::install_panic_hook();
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
