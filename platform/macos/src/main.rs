//! 通譯輸入法的 macOS（IMK）整合層。
//!
//! 引擎（`ime_core`）已經接上，見 `echo_ime` 與開發文件 §2.52.27 起。
//! 模組名字裡的 `echo` 是 spike 2 留下的——那時它真的只是個回聲程式。
//!
//! macOS 的輸入法是**獨立的 .app 行程**服務所有 App，不像 Windows 那樣
//! 把 DLL 載進每個宿主行程（§2.52.3）。

#[cfg(target_os = "macos")]
mod appearance_demo;
#[cfg(target_os = "macos")]
mod candidate_panel;

#[cfg(target_os = "macos")]
mod echo_ime;
#[cfg(target_os = "macos")]
mod guard;
#[cfg(target_os = "macos")]
mod keyprobe;
#[cfg(target_os = "macos")]
mod paths;
#[cfg(target_os = "macos")]
mod settings;
#[cfg(target_os = "macos")]
mod width_panel;

#[cfg(target_os = "macos")]
fn main() {
    // 外觀方案的展示視窗（spike）。**不是產品功能**，決定做哪些之後
    // 連同 `appearance_demo.rs` 一起刪掉。
    if std::env::args().any(|a| a == "--appearance-demo") {
        appearance_demo::run();
        return;
    }
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
