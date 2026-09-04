//! 開發用小工具：直接 LoadLibrary 真正的 `ime_tip_windows.dll`，
//! 呼叫它匯出的 `DllRegisterServer` / `DllUnregisterServer`。
//!
//! 為什麼不直接 `cargo test` 呼叫 `registration::register()`？
//! 因為那個路徑是把 rlib 靜態連結進測試執行檔，`dll_path()` 用
//! `GetModuleHandleExW(FROM_ADDRESS)` 反查回來的會是測試執行檔本身，
//! 不是真正要註冊的 DLL。這支工具在行為上等同 `regsvr32`，
//! 但用一般主控台輸出取代 msgbox / 靜默結束碼，方便看清楚失敗原因。

use std::env;
use std::path::PathBuf;

use windows::core::{Error, HRESULT, PCSTR, PCWSTR};
use windows::Win32::Foundation::FreeLibrary;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

/// 把輸入法加進（或移出）**目前這個使用者**的輸入法清單。
///
/// # 跟 register 的差別
///
/// `register` 是機器層級的（寫 HKLM，讓系統知道有這個 TIP 存在），
/// `enable` 是**使用者層級**的——決定它出不出現在工作列的輸入法選單裡。
/// 兩件事分開，所以安裝程式可以讓使用者選要不要順便啟用。
///
/// # 提權執行時的陷阱
///
/// 這件事寫的是目前使用者的設定。安裝程式以系統管理員身分跑的話，
/// 「目前使用者」會變成 Administrator，加到的是**別人的清單**。
/// 所以安裝程式呼叫這個指令時必須用 Inno 的 `runasoriginaluser` 旗標
/// 降回原使用者身分。
fn set_enabled(on: bool) -> windows::core::Result<()> {
    use ime_tip_windows::guids::{CLSID_TEXT_SERVICE, GUID_PROFILE, TEXTSERVICE_LANGID};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
    use windows::Win32::UI::TextServices::{
        CLSID_TF_InputProcessorProfiles, ITfInputProcessorProfileMgr,
        TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE, TF_IPPMF_ENABLEPROFILE,
        TF_PROFILETYPE_INPUTPROCESSOR,
    };

    unsafe {
        // TSF 的 profile API 要在 STA 裡叫
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        // **COM 物件一定要在 CoUninitialize 之前釋放**。包成閉包是為了讓
        // `mgr` 在這裡就 drop——第一版把它留在外層變數，等函式結束才釋放，
        // 那時 COM 已經關掉了，行程當場 0xC0000005（存取違規）。
        let r = (|| -> windows::core::Result<()> {
            let mgr: ITfInputProcessorProfileMgr =
                CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)?;

            // ENABLEPROFILE 才是「加進清單」；沒有這個旗標只是切換到它。
            // DONTCARECURRENTINPUTLANGUAGE：不要因為現在打的是別的語言就拒絕。
            let mut flags = TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE;
            if on {
                flags |= TF_IPPMF_ENABLEPROFILE;
            }
            mgr.ActivateProfile(
                TF_PROFILETYPE_INPUTPROCESSOR,
                TEXTSERVICE_LANGID,
                &CLSID_TEXT_SERVICE,
                &GUID_PROFILE,
                HKL::default(),
                flags,
            )
        })();

        CoUninitialize();
        r
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let action = args.get(1).map(String::as_str).unwrap_or("register");

    // enable/disable 不必載入 DLL——它們動的是 TSF 的使用者設定，
    // 不是 DLL 匯出的那兩個函式
    if action == "enable" || action == "disable" {
        let on = action == "enable";
        match set_enabled(on) {
            Ok(()) => {
                println!("{action} -> OK");
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("{action} 失敗: {e:?}");
                eprintln!();
                eprintln!("這個指令動的是**目前使用者**的輸入法清單。");
                eprintln!("如果是安裝程式在呼叫，記得用 runasoriginaluser 降回原使用者，");
                eprintln!("否則會加到系統管理員的清單裡。");
                std::process::exit(1);
            }
        }
    }

    let dll_path: PathBuf = match args.get(2) {
        Some(p) => PathBuf::from(p),
        None => {
            let mut p = env::current_exe().expect("current_exe");
            p.pop();
            p.push("ime_tip_windows.dll");
            p
        }
    };

    println!("target dll: {}", dll_path.display());

    let wide: Vec<u16> = dll_path
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    let proc_name = match action {
        "unregister" => "DllUnregisterServer\0",
        _ => "DllRegisterServer\0",
    };

    unsafe {
        let module = match LoadLibraryW(PCWSTR(wide.as_ptr())) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("LoadLibraryW failed: {e:?}");
                std::process::exit(1);
            }
        };

        let addr = GetProcAddress(module, PCSTR(proc_name.as_ptr()));
        let Some(addr) = addr else {
            eprintln!("GetProcAddress({proc_name}) failed");
            let _ = FreeLibrary(module);
            std::process::exit(1);
        };

        let func: unsafe extern "system" fn() -> HRESULT = std::mem::transmute(addr);
        let hr = func();
        println!(
            "{} -> HRESULT(0x{:08X})",
            proc_name.trim_end_matches('\0'),
            hr.0 as u32
        );
        if hr.is_err() {
            println!("Error detail: {:?}", Error::from(hr));
            // DLL 內部附在 Error 上的訊息字串過不了 DLL 邊界（這裡只拿得到
            // 裸的 HRESULT），所以把最常見的失敗原因在這裡再講一次。
            if hr.0 as u32 == 0x8007_0005 {
                eprintln!();
                eprintln!("存取被拒：TSF 輸入法註冊會寫入 HKLM，必須以系統管理員權限執行。");
                eprintln!("請用「以系統管理員身分執行」開啟終端機後再跑一次。");
            }
        }

        let _ = FreeLibrary(module);
        std::process::exit(if hr.is_ok() { 0 } else { 1 });
    }
}
