//! 檔案位置：詞庫在哪。
//!
//! 對應 Windows 的 `registration::shipped_dir()`。那邊是「先看 DLL 旁邊、
//! 再往上兩層找專案根」；macOS 的 `.app` 裝在
//! `~/Library/Input Methods/`，離專案十萬八千里，往上找不到東西，
//! 所以規則不一樣。

use std::path::PathBuf;

use objc2_foundation::NSBundle;

/// 開發用的指路檔。**只有開發建置才有**，正式包不該存在。
const DEV_POINTER: &str = "data-dir.txt";

/// 詞庫目錄。
///
/// 兩條路，依序試：
///
/// 1. **`.app/Contents/Resources/data`** ——正式安裝的樣子（§2.52.5 決定的
///    佈局）。
/// 2. **`.app/Contents/Resources/data-dir.txt` 裡寫的路徑** ——開發用。
///    詞庫 146MB，每次建置都複製進 bundle 太慢，而且改詞庫還要重裝；
///    寫一個指路檔指回專案的 `data/`，建置快、改了立刻生效。
///    `build-app.sh` 負責寫它。
///
/// **不寫死絕對路徑**（CLAUDE.md 的跨電腦開發注意事項）——指路檔的內容
/// 由建置腳本用 `$PWD` 算出來，換一台機器重建就對了。
pub fn data_dir() -> Option<PathBuf> {
    let res = NSBundle::mainBundle().resourcePath()?;
    let res = PathBuf::from(res.to_string());

    let bundled = res.join("data");
    if bundled.is_dir() {
        return Some(bundled);
    }

    let pointer = res.join(DEV_POINTER);
    let raw = std::fs::read_to_string(pointer).ok()?;
    let dev = PathBuf::from(raw.trim());
    dev.is_dir().then_some(dev)
}

/// 設定頁的執行檔。
///
/// 放在 `.app/Contents/Resources/` 裡——**跟輸入法一起裝、一起走**，
/// 使用者不必自己找。`build-app.sh` 負責複製（`--all` 才會連它一起建）。
pub fn settings_exe() -> Option<PathBuf> {
    let res = NSBundle::mainBundle().resourcePath()?;
    let p = PathBuf::from(res.to_string()).join("ime_settings");
    p.is_file().then_some(p)
}

/// 隨程式一起裝的那份領域包放在哪。
///
/// 對應 Windows 的 `registration::bundled_packs_dir()`（DLL 旁邊的
/// `packs\`，開發環境是專案根）。macOS 兩條路：
///
/// 1. **`.app/Contents/Resources/packs`** ——正式安裝
/// 2. **詞庫目錄的隔壁** ——開發用。詞庫是 `$ROOT/data`（由指路檔給），
///    包就在 `$ROOT/packs`
///
/// 使用者自己裝的包不走這裡，那是設定裡的 `behavior.packs_dir`。
pub fn bundled_packs_dir() -> Option<PathBuf> {
    let res = NSBundle::mainBundle().resourcePath()?;
    let bundled = PathBuf::from(res.to_string()).join("packs");
    if bundled.is_dir() {
        return Some(bundled);
    }
    let dev = data_dir()?.parent()?.join("packs");
    dev.is_dir().then_some(dev)
}
