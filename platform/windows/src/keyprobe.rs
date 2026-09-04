//! 快捷鍵攔截的量測探針。
//!
//! # 要回答的問題
//!
//! 哪些組合鍵**根本到不了輸入法**？TSF 底下按鍵會經過系統、宿主 App、
//! 輸入法三關，任何一關都可能先吃掉它。這件事推論不出來——同一個
//! `Ctrl+W` 在記事本沒人要、在瀏覽器是關分頁，行為完全不同。
//!
//! 量測點只有一個：`OnTestKeyDown`。那是 TSF **一定會問**的一步，
//! 所以「有沒有出現在 log 裡」就等於「輸入法看不看得到這個組合」。
//!
//! # 只記組合鍵，不記一般打字
//!
//! 一般字母鍵不記。理由不是省空間，是**不要把使用者打的內容寫進檔案**
//! ——密碼欄的判斷發生在探針之後，等判斷完再記就會漏掉要量的東西。
//! 組合鍵不構成這個風險，而要量的正好就是組合鍵。
//!
//! # 怎麼用
//!
//! 跟其他 log 共用同一個開關（`data/debug.on`），寫到
//! `%TEMP%\ime_debug.log`。每一行都帶宿主程式名，所以同一份 log 可以
//! 一路測完記事本、瀏覽器、Word，事後再依程式分開看。
//!
//! 分析用 `tools/keyprobe.py`。

use std::sync::OnceLock;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};

/// 宿主程式名（`notepad`、`brave`…）。
///
/// 輸入法的 DLL 就載在宿主行程裡，所以問自己的執行檔就是答案，
/// 不必去查前景視窗屬於誰。
fn host() -> &'static str {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "?".into())
    })
}

fn down(vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY) -> bool {
    unsafe { GetKeyState(vk.0 as i32) < 0 }
}

/// 現在按著哪些修飾鍵，組成 `Ctrl+Alt+` 這種前綴。
///
/// **按下的鍵自己是修飾鍵時不算進前綴**——按 Ctrl 的那一刻 Ctrl 當然
/// 是按著的，不濾掉就會記成 `Ctrl+Ctrl`，而「單按 Ctrl」正是要量的
/// 一格（它是語言輪替鍵）。
fn modifiers(vk: u32) -> String {
    let mut s = String::new();
    if down(VK_CONTROL) && !matches!(vk, 0x11 | 0xA2 | 0xA3) {
        s.push_str("Ctrl+");
    }
    if down(VK_MENU) && !matches!(vk, 0x12 | 0xA4 | 0xA5) {
        s.push_str("Alt+");
    }
    if down(VK_SHIFT) && !matches!(vk, 0x10 | 0xA0 | 0xA1) {
        s.push_str("Shift+");
    }
    if (down(VK_LWIN) || down(VK_RWIN)) && !matches!(vk, 0x5B | 0x5C) {
        s.push_str("Win+");
    }
    s
}

/// 虛擬鍵碼的可讀名字。
///
/// 印得出來的 ASCII 直接印字元，其餘查表；表外的印十六進位——
/// 表要收全沒有意義，看到 `VK_5B` 再回頭查就好。
fn key_name(vk: u32) -> String {
    let named = match vk {
        0x08 => "Back",
        0x09 => "Tab",
        0x0D => "Enter",
        0x10 => "Shift",
        0x11 => "Ctrl",
        0x12 => "Alt",
        0x14 => "CapsLock",
        0x1B => "Esc",
        0x20 => "Space",
        0x21 => "PageUp",
        0x22 => "PageDown",
        0x23 => "End",
        0x24 => "Home",
        0x25 => "Left",
        0x26 => "Up",
        0x27 => "Right",
        0x28 => "Down",
        0x2D => "Insert",
        0x2E => "Delete",
        0x5B => "LWin",
        0x5C => "RWin",
        0x5D => "Menu",
        0x70..=0x7B => return format!("F{}", vk - 0x6F),
        0x60..=0x69 => return format!("Num{}", vk - 0x60),
        _ => "",
    };
    if !named.is_empty() {
        return named.into();
    }
    // 字母與數字：`A`～`Z`、`0`～`9` 的虛擬鍵碼就是那個 ASCII
    if (0x30..=0x39).contains(&vk) || (0x41..=0x5A).contains(&vk) {
        return (vk as u8 as char).to_string();
    }
    format!("VK_{vk:02X}")
}

/// 這個按鍵值不值得記一筆。
///
/// **有修飾鍵**就記——那是要量的對象。沒有修飾鍵時只記功能鍵
/// （F1～F12、方向鍵、Tab、Esc 這些），一般字母數字一律不記，
/// 免得把使用者打的內容寫進檔案。
fn worth_logging(vk: u32) -> bool {
    if down(VK_CONTROL) || down(VK_MENU) || down(VK_LWIN) || down(VK_RWIN) {
        return true;
    }
    // 功能鍵、方向鍵這類本來就不帶內容的，沒有修飾鍵也記
    let special = matches!(vk, 0x09 | 0x1B | 0x21..=0x28 | 0x2D | 0x2E | 0x70..=0x7B);
    // **Shift 只搭配非文字鍵才記**。`Shift+空白`是我們的全半形鍵，
    // 一定要量得到；但 `Shift+字母`是打大寫，記下去就等於記內容了
    let shift_combo = down(VK_SHIFT) && (special || vk == 0x20);
    special || shift_combo
}

/// 記一筆按鍵事件。
///
/// `stage` 是哪一關（`Test` / `Down` / `Preserved`），`answer` 是我們
/// 對 TSF 的回答——`Some(true)` 代表接手。兩者合起來才看得出
/// 「有到但我們放行」與「根本沒到」的差別。
pub fn probe(stage: &str, vk: u32, answer: Option<bool>) {
    if !worth_logging(vk) {
        return;
    }
    let reply = match answer {
        Some(true) => "接手",
        Some(false) => "放行",
        None => "-",
    };
    crate::dlog!(
        "[key] {} {stage} {}{} → {reply}",
        host(),
        modifiers(vk),
        key_name(vk)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 鍵名可讀() {
        assert_eq!(key_name(0x09), "Tab");
        assert_eq!(key_name(0x41), "A");
        assert_eq!(key_name(0x31), "1");
        assert_eq!(key_name(0x70), "F1");
        assert_eq!(key_name(0x7B), "F12");
        assert_eq!(key_name(0x61), "Num1");
    }

    #[test]
    fn 表外的印十六進位() {
        // 收全整張表沒有意義——看到這種再回頭查就好
        assert_eq!(key_name(0xBA), "VK_BA");
    }
}
