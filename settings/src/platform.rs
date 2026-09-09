//! **這個平台的輸入法實際上會用到哪些設定。**
//!
//! # 為什麼需要這張表
//!
//! 設定頁是跨平台的同一份程式，但兩邊的繪製層做的事不一樣：Windows 有
//! 預覽列、漸層、文字描邊、背景圖；macOS 的候選面板目前只畫「單色圓角底
//! ＋候選字＋反白＋提示列」。**列出使用者改了不會有任何反應的欄位，比讓
//! 他改半天再發現沒用好**——那種欄位比沒有還糟，會讓人以為是壞了。
//!
//! # 隱藏不等於清掉
//!
//! 設定檔裡的值**照樣留著**。同一份 `config.toml` 帶回 Windows 仍然有效，
//! 也不會因為在 macOS 上開過設定頁就被抹掉——這裡只決定「畫不畫這個控制
//! 項」，不碰資料。
//!
//! macOS 補上對應功能時（例如背景圖，見開發文件 §2.52.31），把那一項改成
//! `true` 就好，UI 不必再動。

/// 預覽列（組字當下會送出什麼，畫成獨立的一條）。
///
/// **macOS 沒有這個東西**：組字區是宿主畫的（marked text），我們只能把
/// 屬性交給它，畫不了自己的預覽列（§2.52.27）。連帶「分隔線」也沒有——
/// 那條線分的就是預覽列與候選清單。
pub const HAS_PREVIEW_BAR: bool = cfg!(windows);

/// 底色的漸層下緣。macOS 的面板畫的是單色圓角底。
pub const HAS_GRADIENT: bool = cfg!(windows);

/// 文字描邊。macOS 還沒接（它主要是為了背景圖上的可讀性）。
pub const HAS_TEXT_OUTLINE: bool = cfg!(windows);

/// 候選視窗的背景圖。macOS 的面板還沒畫它，見開發文件 §2.52.31。
pub const HAS_BACKGROUND_IMAGE: bool = cfg!(windows);

/// 反白樣式（實心／高光帶／只有高光）。macOS 目前只有實心。
pub const HAS_HIGHLIGHT_STYLE: bool = cfg!(windows);

/// 系統的「選擇字型」對話框。
///
/// macOS 沒有跟 `ChooseFontW` 對等的東西（`NSFontPanel` 是非模態面板，
/// 跟 egui 的即時模式 UI 整合彆扭，見開發文件 §2.52.31），所以那邊改成
/// 直接打字型名稱。
pub const HAS_FONT_DIALOG: bool = cfg!(windows);

/// 「用 Ctrl + 那個鍵明講我要標點」這條路走不走得通。
///
/// **macOS 完全收不到 `Ctrl+…`**——IMK 不把它轉給輸入法，實測 154KB 的
/// `keyprobe.log` 裡零筆 Ctrl（開發文件 §2.52.46）。所以那個核取方塊在
/// macOS 上勾了也不會有任何事發生，是「比沒有還糟」的那種欄位。
///
/// 這不是「還沒做」，是**做不到**——改成 `Cmd+` 會撞掉系統一整排快捷鍵。
pub const HAS_CTRL_PUNCT: bool = cfg!(windows);

/// 全形／半形的切換鍵叫什麼，寫給使用者看的。
///
/// **兩個平台的鍵不一樣**（macOS 收不到 `Ctrl`，見 `HAS_CTRL_PUNCT`），
/// 設定頁的說明文字不能寫死一邊——寫錯的說明比沒有說明更糟。
pub const WIDTH_TOGGLE_KEY: &str = if cfg!(target_os = "macos") {
    "Command+Shift+空白"
} else {
    "Ctrl+Shift+空白"
};

/// 語言鎖定的切換鍵叫什麼，寫給使用者看的。理由同 `WIDTH_TOGGLE_KEY`。
pub const LOCK_TOGGLE_KEY: &str = if cfg!(target_os = "macos") {
    "Shift+空白"
} else {
    "單按 Ctrl"
};

/// 有沒有東西被藏起來。有的話設定頁要說一聲，不然使用者會以為選項不見了。
pub const HIDES_ANYTHING: bool = !HAS_PREVIEW_BAR
    || !HAS_GRADIENT
    || !HAS_TEXT_OUTLINE
    || !HAS_BACKGROUND_IMAGE
    || !HAS_HIGHLIGHT_STYLE;

/// 把**這個平台用不到的設定**正規化成「等於沒設」。
///
/// 展示區畫之前一律先過這一關。理由見 `preview_pane::preview` 的說明：
/// 隱藏控制項不會讓值消失，照原樣畫的話展示區會出現實際面板不會有的效果。
///
/// **只影響畫出來的樣子，不碰使用者的設定檔**——回傳的是複本。
pub fn effective(cfg: &ime_core::config::Config) -> ime_core::config::Config {
    use ime_core::config::HighlightStyle;
    let mut c = cfg.clone();
    if !HAS_GRADIENT {
        // 漸層下緣＝底色就是沒有漸層（`gradient_bottom` 兩色相同時走單色路徑）
        c.colors.window_bg2 = c.colors.window_bg.clone();
        c.colors.preview_bg2 = c.colors.preview_bg.clone();
    }
    if !HAS_TEXT_OUTLINE {
        c.background.text_outline = 0.0;
    }
    if !HAS_BACKGROUND_IMAGE {
        c.background.image.clear();
    }
    if !HAS_HIGHLIGHT_STYLE {
        c.metrics.highlight_style = HighlightStyle::Solid;
    }
    c
}

/// 有沒有「從設定頁解除安裝」這條路。
///
/// Windows 走系統的「新增或移除程式」，那是使用者本來就知道要去哪裡找的
/// 地方，設定頁不必重複。
pub const HAS_UNINSTALL: bool = cfg!(target_os = "macos");

/// 隨程式一起裝的解除安裝腳本在哪。
///
/// # 為什麼 macOS 才有這條路
///
/// Windows 走系統的「新增或移除程式」，那是使用者本來就知道要去哪裡找的
/// 地方。**macOS 沒有那種東西**，而輸入法裝在 `~/Library/Input Methods/`
/// ——Finder 預設看不到的路徑。不給入口的話一般人根本移除不掉。
/// fcitx5-macos 也是把解除安裝按鈕放在設定頁。
///
/// 設定頁自己就在 `Tsunagi.app/Contents/Resources/`，腳本是它的鄰居——
/// **不要寫死路徑**，使用者可能把 app 放在別的地方。
#[cfg(target_os = "macos")]
pub fn uninstall_script() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let p = exe.parent()?.join("uninstall.sh");
    p.is_file().then_some(p)
}

/// 跑解除安裝腳本。
///
/// **必須放生（detach）而且立刻結束自己**：腳本會 `pkill` 掉輸入法與設定頁
/// ——我們就是設定頁。不放生的話會在腳本殺到自己時中斷，app 刪一半。
/// 輸出留在 `/tmp/tsunagi-uninstall.log`（fcitx5 也是寫到 /tmp）。
#[cfg(target_os = "macos")]
pub fn run_uninstall(all: bool) -> bool {
    use std::process::{Command, Stdio};
    let Some(script) = uninstall_script() else {
        return false;
    };
    let log = std::fs::File::create("/tmp/tsunagi-uninstall.log").ok();
    let mut cmd = Command::new("/bin/sh");
    cmd.arg(&script);
    if all {
        cmd.arg("--all");
    }
    if let Some(f) = log {
        let err = f.try_clone().ok();
        cmd.stdout(Stdio::from(f));
        if let Some(e) = err {
            cmd.stderr(Stdio::from(e));
        }
    }
    cmd.spawn().is_ok()
}

/// 其他平台不從設定頁解除安裝，這支不會被叫到。
#[cfg(not(target_os = "macos"))]
pub fn run_uninstall(_all: bool) -> bool {
    false
}
