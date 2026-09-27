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
//! 用檔案而不是環境變數，是因為輸入法跑在**宿主行程**裡：環境變數要
//! 設成使用者層級，而開關檔只要動一個檔案。
//!
//! **但改了要重開宿主才生效**——`enabled()` 是 `OnceLock`，那個檔案
//! 只在該行程第一次寫 log 時讀一次就鎖定。中途建立或刪除對**已經在跑
//! 的**宿主無效，症狀是「建了 `debug.on` 卻沒有任何 log」或「刪掉了
//! 還在寫」。2026-09-15 跑相容性測試 M3 時踩到。
//!
//! 這是刻意的取捨：關閉時每次呼叫只是一個原子讀取，不必碰檔案系統。
//! log 是熱路徑上的東西（每個按鍵、每次定位都會叫），為了「隨時生效」
//! 讓它每次去 stat 一個檔案不划算。
//!
//! log 寫到 `%TEMP%\ime_debug.log`，每行的形狀是：
//!
//! ```text
//! [notepad 23:57:27.891 +1234ms] [啟用] 詞庫背景載入完成 712ms
//!  └ 宿主程式名 └ 牆上時鐘      └ 這個行程第一次寫 log 以來的毫秒
//! ```
//!
//! **三個欄位各有各的用途**：好幾個宿主行程寫同一個檔案，不標宿主就
//! 分不出誰是誰；相對毫秒量「這一段花多久」；而牆上時鐘量**log 沒
//! 涵蓋到的空白**——量 LOL 時卡頓不落在任何兩行之間，靠它才對得上
//! 使用者的體感時刻（見 `clock`）。

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

/// 宿主程式名（`notepad`、`brave`、`League of Legends`…）。
///
/// **每一行都要帶**。輸入法同時活在好幾個宿主行程裡，它們寫的是
/// **同一個檔案**——不標宿主的話，記事本與遊戲的記錄長得一模一樣。
/// 2026-09-07 量 LOL 那次就是這樣把 VS Code 的 `ActivateEx` 誤認成
/// 遊戲的，整條結論因此量錯（見相容性測試清單 H11）。
///
/// 做法跟 `keyprobe::host` 同一套：DLL 就載在宿主行程裡，問自己的
/// 執行檔就是答案，不必去查前景視窗屬於誰。
fn host() -> &'static str {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "?".into())
    })
}

/// 行程啟動以來的毫秒數。
///
/// 起點是第一次寫 log 的時刻，不是行程啟動——`Instant` 沒有「行程
/// 啟動」那個原點。所以第一行一定是 `+0ms`，有意義的是**後面每行
/// 跟它的差**。
fn elapsed_ms() -> u128 {
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis()
}

/// 牆上時鐘，`時:分:秒.毫秒`。
///
/// # 為什麼相對毫秒不夠
///
/// 初版只記相對毫秒，理由是「卡在哪一段是兩行之間的差」。量 LOL 時
/// 那個理由塌了：**卡頓不落在任何兩行之間**——整份 log 每一步都是
/// 毫秒級，而使用者實際等了十幾秒。那代表卡在我們的第一行 log
/// 之前（模組載入那段），相對時間從定義上就看不到它。
///
/// 牆上時鐘補得起這個洞：拿它跟宿主行程的啟動時刻、跟使用者「我大概
/// 幾點按下去的」對照，就量得出那段空白有多長。
///
/// 不用 `chrono` 那類函式庫——只要時分秒，自己從 Unix 時間換算就好，
/// 不值得為了一行 log 多一個相依。**沒有時區轉換**（拿 UTC 換算後
/// 直接取餘數），所以顯示的時與本地時間可能差好幾個小時；這裡要的是
/// **兩個時刻之間差多久**，那個差不受時區影響。
fn clock() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        (secs / 3600) % 24,
        (secs / 60) % 60,
        secs % 60,
        now.subsec_millis()
    )
}

/// 寫一行 log。關閉時什麼都不做。
///
/// 每行的形狀是 `[宿主 時鐘 +毫秒] 訊息`，三個欄位的用途見模組說明。
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
        let _ = writeln!(f, "[{} {} +{}ms] {msg}", host(), clock(), elapsed_ms());
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
