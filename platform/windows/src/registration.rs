//! DllRegisterServer / DllUnregisterServer 的實作。
//!
//! **一律寫在 HKLM，且必須以系統管理員權限執行。**
//!
//! 早期版本刻意只寫 HKCU，想讓開發期的 regsvr32 不必提權。那個推論對
//! 純 COM 註冊成立（COM 解析 CLSID 時會合併查詢 HKCU\Software\Classes），
//! 但對 TSF 不成立：`ITfCategoryMgr::RegisterCategory` 與
//! `ITfInputProcessorProfileMgr::RegisterProfile` 內部寫的是
//! `HKLM\SOFTWARE\Microsoft\CTF\TIP`，非提權行程寫不進去，msctf 只會回傳
//! 一個毫無資訊量的 `E_FAIL`。這正是開發文件.md 第三章追了很久的那個坑。
//!
//! 兩邊必須一致：TSF profile 登記在 HKLM（全機器可見），COM 的
//! InProcServer32 卻只寫 HKCU 的話，跨使用者情境的行程（登入畫面、UAC
//! 提示框、其他帳號的程式）會看得到這個 TIP、卻查不到 DLL 在哪而載入失敗。

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use windows::core::{Result, GUID, PCWSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW, HKEY_CURRENT_USER,
    HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::{
    CLSID_TF_CategoryMgr, CLSID_TF_InputProcessorProfiles, ITfCategoryMgr,
    ITfInputProcessorProfileMgr, GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER, GUID_TFCAT_TIPCAP_COMLESS,
    GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT, GUID_TFCAT_TIPCAP_INPUTMODECOMPARTMENT,
    GUID_TFCAT_TIPCAP_SECUREMODE, GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
    GUID_TFCAT_TIPCAP_UIELEMENTENABLED, GUID_TFCAT_TIP_KEYBOARD,
};

use crate::guids::{CLSID_TEXT_SERVICE, GUID_PROFILE, TEXTSERVICE_DESC, TEXTSERVICE_LANGID};

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn guid_key_name(guid: &GUID) -> String {
    format!("{{{guid:?}}}")
}

/// 詞庫目錄。
///
/// DLL 在 `target/release/`，詞庫在專案根目錄的 `data/`——往上兩層。
/// 這是開發期的擺法；正式發佈時詞庫會跟 DLL 放在一起，那時要改這裡。
///
/// **一律用 `dll_path()` 反查**，不要用目前工作目錄——TSF 的宿主是
/// 記事本、瀏覽器這些程式，它們的工作目錄跟本專案無關。
/// 跟本 DLL 放在同一個資料夾的檔案在哪？
///
/// **一律透過這裡取路徑**，不要自己呼叫 `GetModuleHandleW(None)`——
/// 那會拿到呼叫端行程的路徑而非本 DLL 的（見 CLAUDE.md 的
/// 「唯一合法存取方式」）。
pub fn sibling_path(name: &str) -> Option<std::path::PathBuf> {
    let raw = dll_path().ok()?;
    let s = String::from_utf16_lossy(&raw);
    let dll = std::path::Path::new(s.trim_end_matches(' '));
    Some(dll.parent()?.join(name))
}

/// 詞庫與資料檔在哪。
///
/// **兩種佈局都要支援**，因為開發跟安裝後的目錄形狀不一樣：
///
/// ```text
/// 安裝後   C:\Program Files\tsunagi-ime\ime_tip_windows.dll
///          C:\Program Files\tsunagi-ime\data\...        ← 就在 DLL 旁邊
///
/// 開發中   <專案>\target\release\ime_tip_windows.dll
///          <專案>\data\...                              ← 往上兩層
/// ```
///
/// 先看 DLL 旁邊，沒有才往上找。第一版只有「往上兩層」那條，裝到
/// `Program Files` 之後會算出 `C:\data`——詞庫永遠找不到，而且不會報錯，
/// 只是所有候選都變空的。
pub fn data_dir() -> Option<std::path::PathBuf> {
    let raw = dll_path().ok()?;
    let s = String::from_utf16_lossy(&raw);
    let s = s.trim_end_matches('\0');
    let dll = std::path::Path::new(s);
    let here = dll.parent()?;

    let installed = here.join("data");
    if installed.is_dir() {
        return Some(installed);
    }

    // target/release → target → 專案根
    let data = here.parent()?.parent()?.join("data");
    data.is_dir().then_some(data)
}

/// 取得「這顆 DLL 自己」的 module handle。
///
/// 載入本 DLL 內嵌的資源（例如工作列圖示）時要用它。理由同
/// `dll_path()`：`GetModuleHandleW(None)` 拿到的是宿主行程，
/// 在那裡面找不到我們的資源。
pub fn dll_module() -> Result<windows::Win32::Foundation::HMODULE> {
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::System::LibraryLoader::{
        GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
    };
    unsafe {
        let mut handle = HMODULE::default();
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
            // 用「這個函式自己的位址」反查，才保證是本 DLL
            PCWSTR(dll_module as *const () as *const u16),
            &mut handle,
        )?;
        Ok(handle)
    }
}

/// 取得「這顆 DLL 自己」在磁碟上的路徑。
///
/// 不能用 `GetModuleHandleW(None)`——那會拿到呼叫端行程（`regsvr32.exe`、
/// 甚至 `cargo test` 的測試執行檔）的路徑，而不是這顆 DLL 自己的路徑。
/// 用 `GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS` 從「這個函式本身的位址」
/// 反查，才能保證拿到的是本 DLL 的 HMODULE。
fn dll_path() -> Result<Vec<u16>> {
    use windows::Win32::System::LibraryLoader::GetModuleFileNameW;

    unsafe {
        let handle = dll_module()?;
        let mut buf = vec![0u16; 260];
        loop {
            let len = GetModuleFileNameW(Some(handle), &mut buf);
            if (len as usize) < buf.len() {
                buf.truncate(len as usize);
                buf.push(0);
                return Ok(buf);
            }
            buf.resize(buf.len() * 2, 0);
        }
    }
}

/// 寫入 CLSID 的 InProcServer32 登錄項，讓 COM 認得這顆 DLL。
///
/// 寫在 HKLM，與 TSF profile 的註冊位置保持一致（理由見本檔案開頭說明）。
fn register_com_server() -> Result<()> {
    unsafe {
        let clsid_path = format!(
            "Software\\Classes\\CLSID\\{}",
            guid_key_name(&CLSID_TEXT_SERVICE)
        );
        let inproc_path = format!("{clsid_path}\\InProcServer32");

        let mut clsid_key = Default::default();
        RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR::from_raw(wide(&clsid_path).as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_ALL_ACCESS,
            None,
            &mut clsid_key,
            None,
        )
        .ok()?;
        let desc = wide(TEXTSERVICE_DESC);
        let desc_bytes = std::slice::from_raw_parts(desc.as_ptr() as *const u8, desc.len() * 2);
        RegSetValueExW(clsid_key, PCWSTR::null(), None, REG_SZ, Some(desc_bytes)).ok()?;
        RegCloseKey(clsid_key).ok()?;

        let mut inproc_key = Default::default();
        RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR::from_raw(wide(&inproc_path).as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_ALL_ACCESS,
            None,
            &mut inproc_key,
            None,
        )
        .ok()?;

        let path = dll_path()?;
        let path_bytes = std::slice::from_raw_parts(path.as_ptr() as *const u8, path.len() * 2);
        RegSetValueExW(inproc_key, PCWSTR::null(), None, REG_SZ, Some(path_bytes)).ok()?;

        let model = wide("Apartment");
        let model_bytes = std::slice::from_raw_parts(model.as_ptr() as *const u8, model.len() * 2);
        RegSetValueExW(
            inproc_key,
            PCWSTR::from_raw(wide("ThreadingModel").as_ptr()),
            None,
            REG_SZ,
            Some(model_bytes),
        )
        .ok()?;
        RegCloseKey(inproc_key).ok()?;
    }
    Ok(())
}

/// 移除 COM 註冊項。
///
/// HKCU 那一行是在清舊版本（只寫 HKCU）留下的殘留：舊新兩份同時存在時，
/// COM 解析 CLSID 會優先命中 HKCU，若它指向舊路徑就會載入錯誤的 DLL，
/// 這種問題很難追。兩邊都刪就能彻底排除。
/// （確定所有開發機都清乾淨後，HKCU 那一行可以拿掉。）
fn unregister_com_server() {
    unsafe {
        let clsid_path = format!(
            "Software\\Classes\\CLSID\\{}",
            guid_key_name(&CLSID_TEXT_SERVICE)
        );
        let path = wide(&clsid_path);
        let _ = RegDeleteTreeW(HKEY_LOCAL_MACHINE, PCWSTR::from_raw(path.as_ptr()));
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR::from_raw(path.as_ptr()));
    }
}

/// 向 TSF 註冊這顆 CLSID 為鍵盤類文字服務，並掛上一個語言設定檔。
///
/// 用的是 `ITfInputProcessorProfileMgr`（Vista 以後的新介面，一次呼叫完成
/// 註冊），而非舊版 `ITfInputProcessorProfiles::Register` +
/// `AddLanguageProfile` 兩段式 API。
///
/// **必須提權才能成功**：這兩個呼叫寫的是
/// `HKLM\SOFTWARE\Microsoft\CTF\TIP`。非提權時 msctf 只回一個沒有任何
/// 資訊量的 `E_FAIL`（不是 `E_ACCESSDENIED`），極度誤導。呼叫端的
/// `register()` 已經會先擋下並給出明確訊息。
/// 本 TIP 要宣告的 TSF 類別。
///
/// **註冊與反註冊共用這一份**——分開寫的話遲早有一邊漏改。
///
/// 每一項的意思：
///
/// | 類別 | 宣告了什麼 |
/// |---|---|
/// | `TIP_KEYBOARD` | 這是鍵盤類的輸入法（最基本，沒有它不會出現在清單） |
/// | `DISPLAYATTRIBUTEPROVIDER` | 提供組字文字的顯示樣式（底線） |
/// | `TIPCAP_SYSTRAYSUPPORT` | **可以出現在工作列的輸入指示器那格** |
/// | `TIPCAP_IMMERSIVESUPPORT` | 支援 UWP／市集 App（新版工作列宿主也吃這個） |
/// | `TIPCAP_INPUTMODECOMPARTMENT` | 會維護「輸入模式」狀態，指示器要讀它 |
/// | `TIPCAP_UIELEMENTENABLED` | 支援 `ITfUIElement`，宿主可以接管或抑制我們的 UI |
/// | `TIPCAP_SECUREMODE` | 可以在安全桌面（UAC 提示、鎖定畫面）使用 |
/// | `TIPCAP_COMLESS` | 不需要完整 COM 註冊也能載入 |
///
/// **少了 `SYSTRAYSUPPORT` 的後果很難查**：語言列按鈕照樣掛得上、
/// `AddItem` 回 `S_OK`、Windows 也照樣反覆呼叫 `GetInfo`／`GetIcon`，
/// 但工作列就是不畫——因為那個宿主根本沒把這個 TIP 登記進去。
/// 見開發文件 §3.7。
const TSF_CATEGORIES: [GUID; 8] = [
    GUID_TFCAT_TIP_KEYBOARD,
    GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
    GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
    GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
    GUID_TFCAT_TIPCAP_INPUTMODECOMPARTMENT,
    GUID_TFCAT_TIPCAP_UIELEMENTENABLED,
    GUID_TFCAT_TIPCAP_SECUREMODE,
    GUID_TFCAT_TIPCAP_COMLESS,
];

fn register_tsf_profile() -> Result<()> {
    unsafe {
        let profiles: ITfInputProcessorProfileMgr =
            CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)?;

        let category_mgr: ITfCategoryMgr =
            CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)?;
        for cat in TSF_CATEGORIES {
            category_mgr.RegisterCategory(&CLSID_TEXT_SERVICE, &cat, &CLSID_TEXT_SERVICE)?;
        }

        let desc = wide(TEXTSERVICE_DESC);
        let icon_file = dll_path()?;
        profiles.RegisterProfile(
            &CLSID_TEXT_SERVICE,
            TEXTSERVICE_LANGID,
            &GUID_PROFILE,
            &desc,
            &icon_file,
            0,
            HKL::default(),
            0,
            true,
            0,
        )?;
    }
    Ok(())
}

/// 反註冊 TSF 的類別與 profile。
///
/// # 為什麼要收集錯誤而不是吞掉
///
/// 第一版每個呼叫都寫 `let _ =`／`if let Ok`，於是**失敗也回報成功**——
/// 使用者看到「unregister -> S_OK」卻發現註冊還在，完全無從查起。
/// 現在改成：**每一項都試（一項失敗不該讓其他項不清）**，但把第一個
/// 錯誤留下來回報。
fn unregister_tsf_profile() -> Result<()> {
    let mut first_err: Option<windows::core::Error> = None;
    let mut keep = |r: Result<()>| {
        if let Err(e) = r {
            if first_err.is_none() {
                first_err = Some(e);
            }
        }
    };

    unsafe {
        match CoCreateInstance::<_, ITfCategoryMgr>(
            &CLSID_TF_CategoryMgr,
            None,
            CLSCTX_INPROC_SERVER,
        ) {
            Ok(category_mgr) => {
                for cat in TSF_CATEGORIES {
                    keep(category_mgr.UnregisterCategory(
                        &CLSID_TEXT_SERVICE,
                        &cat,
                        &CLSID_TEXT_SERVICE,
                    ));
                }
            }
            Err(e) => keep(Err(e)),
        }

        match CoCreateInstance::<_, ITfInputProcessorProfileMgr>(
            &CLSID_TF_InputProcessorProfiles,
            None,
            CLSCTX_INPROC_SERVER,
        ) {
            Ok(profiles) => keep(profiles.UnregisterProfile(
                &CLSID_TEXT_SERVICE,
                TEXTSERVICE_LANGID,
                &GUID_PROFILE,
                0,
            )),
            Err(e) => keep(Err(e)),
        }
    }

    match first_err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// 清掉 msctf 留下的空殼。
///
/// `UnregisterCategory` / `UnregisterProfile` **只刪自己寫的葉節點**，
/// 空掉的父目錄它不管。實測反註冊之後 `CTF\TIP\{我們的 CLSID}` 底下還
/// 剩 14 個空鍵——功能上等於沒註冊，但使用者用 regedit 看會以為沒移乾淨。
///
/// 刪的是**我們自己 CLSID 那一棵**，路徑寫死、不含萬用字元。
fn sweep_tsf_registry_leftovers() {
    unsafe {
        let path = wide(&format!(
            "SOFTWARE\\Microsoft\\CTF\\TIP\\{}",
            guid_key_name(&CLSID_TEXT_SERVICE)
        ));
        let _ = RegDeleteTreeW(HKEY_LOCAL_MACHINE, PCWSTR::from_raw(path.as_ptr()));
    }
}

/// 這個行程是不是以「提升權限」（系統管理員）在跑。
///
/// 用來在真正動手前先擋下來，給出一句人看得懂的話，而不是讓呼叫端
/// 對著 msctf 回傳的 `E_FAIL` 猜半天——這個專案就曾為此卡了很久。
fn is_elevated() -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = Default::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut size = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

/// 未提權時要回報的錯誤。用 `E_ACCESSDENIED` 而非 msctf 那個沒有資訊量的
/// `E_FAIL`，並附上一句說明，讓失敗訊息自己就能指出解法。
fn not_elevated_error() -> windows::core::Error {
    windows::core::Error::new(
        windows::Win32::Foundation::E_ACCESSDENIED,
        "TSF 輸入法註冊必須以系統管理員權限執行（會寫入 HKLM）。\
         請用「以系統管理員身分執行」開啟終端機後再跑一次。",
    )
}

pub fn register() -> Result<()> {
    if !is_elevated() {
        return Err(not_elevated_error());
    }
    register_com_server()?;
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
    }
    let result = register_tsf_profile();
    unsafe {
        CoUninitialize();
    }
    result
}

pub fn unregister() -> Result<()> {
    if !is_elevated() {
        return Err(not_elevated_error());
    }
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
    }
    // 先讓 TSF 自己反註冊（msctf 要更新它的內部狀態），再掃空殼——
    // 順序反過來的話 msctf 會對著已經被刪掉的鍵操作
    let tsf = unregister_tsf_profile();
    unsafe {
        CoUninitialize();
    }
    unregister_com_server();
    sweep_tsf_registry_leftovers();

    // **TSF 那邊的錯誤要往上報。** COM 與空殼照樣清乾淨了（清理不該
    // 因為前面失敗就跳過），但呼叫端必須知道有東西沒成功。
    tsf
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 這個測試是跑在 `cargo test` 產生的獨立執行檔裡（`ime-tip-windows`
    /// 以 rlib 形式靜態連結進測試 exe），`dll_path()` 因此會回報「測試 exe
    /// 自己」的路徑，不是真正的 `ime_tip_windows.dll`——這只夠驗證 TSF
    /// profile / category 呼叫本身的參數是否正確，不能取代對著真正 DLL
    /// 用 regsvr32（或 `register_tool` bin）跑一次真實註冊。
    ///
    /// 這個測試需要系統管理員權限（註冊會寫 HKLM），非提權的
    /// `cargo test` 一定會失敗，所以預設標成 `#[ignore]`。要跑它請在提權
    /// 的終端機下：`cargo test -p ime-tip-windows -- --ignored`。
    #[test]
    #[ignore = "需要管理員權限（TSF 註冊寫 HKLM），請在提權終端機用 --ignored 執行"]
    fn try_register_then_unregister() {
        if let Err(e) = register() {
            panic!("register() failed: {e:?}");
        }
        if let Err(e) = unregister() {
            panic!("unregister() failed: {e:?}");
        }
    }
}
