//! **spike 5：快捷鍵攔截**（開發文件 §2.52.7 第 4 順位）——Windows 版
//! `platform/windows/src/keyprobe.rs` 的 macOS 對應物。
//!
//! # 要回答的問題
//!
//! 哪些組合鍵**根本到不了輸入法**？按鍵要過系統、宿主 App、輸入法三關，
//! 任何一關都可能先吃掉它。這件事推論不出來——同一個 `Cmd+W` 在
//! TextEdit 是關視窗、在終端機是關分頁，行為完全不同（§2.14 在 Windows
//! 上量過同一件事）。
//!
//! 量測點只有一個：`handleEvent:client:`。那是 IMK 唯一把按鍵交給我們的
//! 地方，所以「有沒有出現在 log 裡」就等於「輸入法看不看得到這個組合」。
//!
//! # 跟 Windows 那邊的差別
//!
//! TSF 有兩個入口（`OnTestKeyDown` 先問、`OnKeyDown` 再送），而
//! §2.14.3 量到**宿主分兩派**，有些直接送不先問。**IMK 沒有這個區分**
//! ——只有一個 `handleEvent:`，所以 log 裡的階段永遠是 `Down`。
//!
//! # 只記組合鍵，不記一般打字
//!
//! 跟 Windows 那支同一條規矩：**不要把使用者打的內容寫進檔案**。
//! 有修飾鍵才記；沒有修飾鍵時只記功能鍵、方向鍵這種本來就不帶內容的。
//! `Shift` 單獨搭字母是打大寫，記下去就等於記內容，所以只在搭配非文字鍵
//! 時才記。
//!
//! # 怎麼用
//!
//! 寫到 `~/Library/Application Support/tsunagi-ime/keyprobe.log`，
//! 每行都帶宿主的 bundle id，所以同一份 log 可以一路測完 TextEdit、
//! 瀏覽器、終端機，事後再依宿主分開看。格式**刻意跟 Windows 那支一致**，
//! 分析共用 `tools/keyprobe.py`，兩邊的結果才並排比得了。
//!
//! ```text
//! python3 tools/keyprobe.py ~/Library/Application\ Support/tsunagi-ime/keyprobe.log
//! ```

use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::PathBuf;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{msg_send, sel};
use objc2_app_kit::{NSEvent, NSEventModifierFlags};
use objc2_foundation::NSString;

fn log_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let dir = PathBuf::from(home)
        .join("Library/Application Support")
        .join("tsunagi-ime");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("keyprobe.log"))
}

fn responds(obj: &AnyObject, s: Sel) -> bool {
    unsafe { msg_send![obj, respondsToSelector: s] }
}

/// 宿主的 bundle id。`keyprobe.py` 拿它當分組的鍵。
fn host(sender: &AnyObject) -> String {
    if !responds(sender, sel!(bundleIdentifier)) {
        return "?".into();
    }
    let s: Option<Retained<NSString>> = unsafe { msg_send![sender, bundleIdentifier] };
    s.map(|s| s.to_string()).unwrap_or_else(|| "?".into())
}

/// 特殊鍵的名字。**表以外的回 `None`**，交給呼叫端決定要不要記。
///
/// 只收「本來就不帶使用者內容」的鍵。macOS 的虛擬鍵碼跟 Windows 的
/// VK 完全不同，這張表是 Carbon `Events.h` 的值。
fn special_name(code: u16) -> Option<&'static str> {
    Some(match code {
        0x24 => "Return",
        0x30 => "Tab",
        0x31 => "Space",
        0x33 => "Backspace",
        0x35 => "Esc",
        0x75 => "Delete",
        0x73 => "Home",
        0x77 => "End",
        0x74 => "PageUp",
        0x79 => "PageDown",
        0x7B => "Left",
        0x7C => "Right",
        0x7D => "Down",
        0x7E => "Up",
        0x7A => "F1",
        0x78 => "F2",
        0x63 => "F3",
        0x76 => "F4",
        0x60 => "F5",
        0x61 => "F6",
        0x62 => "F7",
        0x64 => "F8",
        0x65 => "F9",
        0x6D => "F10",
        0x67 => "F11",
        0x6F => "F12",
        _ => return None,
    })
}

/// 修飾鍵前綴，如 `Cmd+Shift+`。順序固定，不然同一個組合會記成兩種寫法。
fn modifiers(flags: NSEventModifierFlags) -> String {
    let mut s = String::new();
    // 順序照 macOS 選單的慣例：Ctrl、Option、Shift、Cmd
    if flags.contains(NSEventModifierFlags::Control) {
        s.push_str("Ctrl+");
    }
    if flags.contains(NSEventModifierFlags::Option) {
        s.push_str("Option+");
    }
    if flags.contains(NSEventModifierFlags::Shift) {
        s.push_str("Shift+");
    }
    if flags.contains(NSEventModifierFlags::Command) {
        s.push_str("Cmd+");
    }
    s
}

/// 記一筆。`handled` 是我們的回答——「有到但我們放行」跟「根本沒到」
/// 是兩件事，只記到不到會分不出來。
pub fn probe(event: &NSEvent, sender: &AnyObject, handled: bool) {
    let code = event.keyCode();
    let flags = event.modifierFlags();
    // Cmd／Ctrl／Option 任一個按著就是要量的對象。
    let has_mod = flags.contains(NSEventModifierFlags::Command)
        || flags.contains(NSEventModifierFlags::Control)
        || flags.contains(NSEventModifierFlags::Option);
    let special = special_name(code);

    // **Shift 只搭配非文字鍵才記**——`Shift+字母`是打大寫，記下去等於記內容。
    if !has_mod && special.is_none() {
        return;
    }

    let name = match special {
        Some(n) => n.to_string(),
        None => match event.charactersIgnoringModifiers() {
            // 有修飾鍵才會走到這，所以這裡拿到的是組合鍵的基底字元，
            // 不是使用者打的內容。
            Some(c) if !c.to_string().is_empty() => c.to_string().to_uppercase(),
            _ => format!("KC_{code:02X}"),
        },
    };

    let line = format!(
        "[key] {} Down {}{} → {}\n",
        host(sender),
        modifiers(flags),
        name,
        if handled { "接手" } else { "放行" }
    );
    if let Some(path) = log_path() {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::special_name;

    #[test]
    fn 特殊鍵叫得出名字() {
        assert_eq!(special_name(0x30), Some("Tab"));
        assert_eq!(special_name(0x35), Some("Esc"));
        assert_eq!(special_name(0x7A), Some("F1"));
        assert_eq!(special_name(0x6F), Some("F12"));
        assert_eq!(special_name(0x7B), Some("Left"));
    }

    #[test]
    fn 字母鍵不在表裡() {
        // 表只收「不帶使用者內容」的鍵——字母走 charactersIgnoringModifiers，
        // 而且只有在有修飾鍵時才會被記。
        assert_eq!(special_name(0x00), None); // A
        assert_eq!(special_name(0x0C), None); // Q
    }
}
