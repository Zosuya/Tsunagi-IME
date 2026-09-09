//! 系統裝了哪些字型。
//!
//! # 為什麼不是每個平台都要這個
//!
//! Windows 用系統的「選擇字型」對話框（`font_dialog`），它自帶完整清單、
//! 預覽與樣式篩選，比自己列一份好。**macOS 沒有對等的東西**
//! （`NSFontPanel` 是非模態面板，跟 egui 的即時模式 UI 整合彆扭，見開發
//! 文件 §2.52.31），所以那邊自己列。

/// 系統上所有的字型家族名稱，已排序。
///
/// **只算一次**——這份清單在設定頁開著的期間不會變，而每幀重算要跟系統
/// 要幾百個字串。
pub fn families() -> &'static [String] {
    static CACHE: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    CACHE.get_or_init(imp::collect)
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2_app_kit::NSFontManager;
    use objc2_foundation::MainThreadMarker;

    pub fn collect() -> Vec<String> {
        // 拿不到主執行緒標記就回空的——呼叫端會退回「直接打名字」。
        // `NSFontManager` 是主執行緒限定，而 egui 的 `update` 本來就在
        // 主執行緒，所以實務上一定拿得到。
        let Some(mtm) = MainThreadMarker::new() else {
            return Vec::new();
        };
        let mut v: Vec<String> = NSFontManager::sharedFontManager(mtm)
            .availableFontFamilies()
            .iter()
            .map(|s| s.to_string())
            // **開頭是點的是系統內部字型**（`.SF NS`、`.PingFangUI` 那些），
            // 使用者選了多半不是他要的樣子，而且有些根本畫不出中文。
            .filter(|s| !s.starts_with('.'))
            .collect();
        v.sort();
        // **撈不到會靜靜地退回打字**，那種失敗從畫面上看不出來，
        // 所以留一行診斷。正常情況下不會印。
        if v.is_empty() {
            eprintln!("[通譯] 撈不到系統字型清單，字型改成手動輸入");
        }
        v
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    /// 其他平台用系統對話框，不需要這份清單。
    pub fn collect() -> Vec<String> {
        Vec::new()
    }
}
