//! 通譯輸入法的 macOS（IMK）整合層。
//!
//! **現況：spike 2 的 Echo 輸入法**，見 `echo_ime` 與開發文件 §2.52。
//! 還沒接 core，只驗證 IMK 的三條路通不通。
//!
//! macOS 的輸入法是**獨立的 .app 行程**服務所有 App，不像 Windows 那樣
//! 把 DLL 載進每個宿主行程（§2.52.3）。

#[cfg(target_os = "macos")]
mod candidate_panel;

#[cfg(target_os = "macos")]
mod echo_ime;
#[cfg(target_os = "macos")]
mod guard;
#[cfg(target_os = "macos")]
mod keyprobe;

#[cfg(target_os = "macos")]
fn main() {
    echo_ime::run();
}

/// 在非 macOS 上這個 crate 整個 cfg 掉，只留一支會講話的 main。
///
/// 這樣根目錄的 `cargo build` 在 Windows 上照樣過，Apple 的依賴也不會
/// 被拉下來（掛在 Cargo.toml 的 target 區段）。
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("ime-tip-macos 只在 macOS 上有意義；Windows 請用 platform/windows。");
}
