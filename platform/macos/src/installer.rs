//! **spike 2 的安裝程式**：一個前景 GUI app，唯一的工作是對輸入法的
//! bundle 呼叫 `TISRegisterInputSource`。
//!
//! # 為什麼需要這一支
//!
//! 威注音的做法是「登記 → 啟用」都在一支獨立的安裝程式裡做，使用者從
//! Finder 點開。這支是它的對應物，目前只做登記那半。
//!
//! 它誕生時是為了排除一個假設——「只有前景 GUI app 發起的註冊才算數」
//! （開發文件 §2.52.15）。那個假設後來證明是錯的：真正的根因是 bundle id
//! 少了 `.inputmethod.`，掃描器靜默跳過（§2.52.16）。留著是因為安裝程式
//! 本來就該有；要查系統到底收下了沒，用 `tools/tisq.swift`。

#[cfg(target_os = "macos")]
fn main() {
    imp::run();
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("tsunagi_installer 只在 macOS 上有意義。");
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSAlert, NSApplication, NSApplicationActivationPolicy};
    use objc2_foundation::{NSString, NSURL};

    // 見 echo_ime.rs 上的說明。這裡再宣告一次而不是抽共用模組：
    // 兩支 bin 共用要多開一個 lib，為了三行不值得。
    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        fn TISRegisterInputSource(location: *const std::ffi::c_void) -> i32;
    }

    pub fn run() {
        let mtm = MainThreadMarker::new().expect("main 一定在主執行緒");
        let app = NSApplication::sharedApplication(mtm);
        // **Regular 才是前景 app**——輸入法本身是 LSUIElement（背景），
        // 這支刻意不是，那正是要測的差異。
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        app.activate();

        let home = std::env::var("HOME").unwrap_or_default();
        let path = format!("{home}/Library/Input Methods/Tsunagi.app");
        let exists = std::path::Path::new(&path).is_dir();

        let (title, body) = if !exists {
            (
                "找不到輸入法".to_string(),
                format!("{path}\n\n先跑 build-app.sh 把它裝好。"),
            )
        } else {
            let url = NSURL::fileURLWithPath(&NSString::from_str(&path));
            let st = unsafe {
                TISRegisterInputSource(Retained::as_ptr(&url) as *const std::ffi::c_void)
            };
            if st == 0 {
                (
                    "註冊呼叫成功".to_string(),
                    format!("TISRegisterInputSource 回 noErr。\n\n{path}\n\n回終端機跑 tools/tisq.swift 確認系統真的收下了。"),
                )
            } else {
                (
                    "註冊失敗".to_string(),
                    format!("TISRegisterInputSource 回 OSStatus={st}。"),
                )
            }
        };

        let alert = NSAlert::new(mtm);
        alert.setMessageText(&NSString::from_str(&title));
        alert.setInformativeText(&NSString::from_str(&body));
        alert.runModal();
    }
}
