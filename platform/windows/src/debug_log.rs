//! 除錯用的檔案 log。
//!
//! # 為什麼需要這個
//!
//! TSF 跑在宿主行程裡，沒有主控台可以印東西。而這個專案的教訓是
//! 「遇到呼叫都成功但行為不對，直接埋 log 量數值，別繼續推論」——
//! 沒有 log 的話只能靠猜，猜錯要花好幾輪才發現。
//!
//! # 怎麼開啟
//!
//! 在專案的 `data/` 底下建一個空檔案 `debug.on` 就啟用，刪掉就關閉。
//!
//! 用檔案而不是環境變數，是因為輸入法跑在**宿主行程**裡——環境變數
//! 要設成使用者層級再重開宿主才讀得到，開關檔則是隨時生效。
//!
//! log 寫到 `%TEMP%\ime_debug.log`。關閉時每次呼叫只是一個原子讀取。

use std::io::Write;
use std::sync::OnceLock;

fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var("IME_DEBUG").is_ok_and(|v| v != "0")
            || crate::registration::data_dir().is_some_and(|d| d.join("debug.on").exists())
    })
}

fn path() -> Option<std::path::PathBuf> {
    std::env::var_os("TEMP").map(|t| std::path::PathBuf::from(t).join("ime_debug.log"))
}

/// 寫一行 log。關閉時什麼都不做。
pub fn log(msg: &str) {
    if !enabled() {
        return;
    }
    let Some(p) = path() else { return };
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(p)
    {
        let _ = writeln!(f, "{msg}");
    }
}

/// 格式化版本。
#[macro_export]
macro_rules! dlog {
    ($($arg:tt)*) => {
        $crate::debug_log::log(&format!($($arg)*))
    };
}

/// 攔下 panic 並寫進檔案。
///
/// # 為什麼一定要有
///
/// 輸入法是**寄生在宿主行程裡的 DLL**。一般程式 panic 是自己當掉，
/// 我們 panic 是把使用者正在編輯的文件一起帶走——而且預設的 panic
/// 訊息會寫到 stderr，宿主根本沒有主控台，等於什麼線索都沒有。
///
/// 所以 panic 一律寫到 `%TEMP%\ime_panic.log`，而且**不受除錯開關
/// 控制**：panic 是當機等級的事件，不能因為忘了開開關就查不到。
///
/// 這只負責「留下線索」，不阻止行程結束——真正要防住崩潰得在 COM
/// 邊界包 `catch_unwind`，那是另一件事。
pub fn install_panic_hook() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if let Some(p) =
                std::env::var_os("TEMP").map(|t| std::path::PathBuf::from(t).join("ime_panic.log"))
            {
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(p)
                {
                    let where_ = info
                        .location()
                        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                        .unwrap_or_else(|| "位置不明".to_string());
                    let _ = writeln!(
                        f,
                        "[panic] 行程 {} 於 {where_}
        {info}
",
                        std::process::id()
                    );
                }
            }
            previous(info);
        }));
    });
}
