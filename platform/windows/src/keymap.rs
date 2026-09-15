//! 按鍵綁定的 Windows 那一半：把 `VK_*` 鍵碼翻成 core 的 [`Key`]。
//!
//! 鍵位表本身（`Mode`／`Action`／`DEFAULT_BINDINGS`／`lookup`）在
//! `ime_core::binding`，兩個平台共用一份（2026-09-13 從這裡上搬）。這裡只留
//! 跟「Windows 的鍵盤怎麼回報」有關的事：
//!
//! - `VK_*` → `Key` 的翻譯，含 US 鍵盤配置表（`typed_char`）與數字鍵盤
//! - 用 `GetKeyState` 讀 Shift／Ctrl，填成 `KeyEvent`
//! - 自動重複的判準（`lparam` 第 30 位元）

pub use ime_core::binding::{Action, Key, KeyEvent, Mode};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_BACK, VK_CONTROL, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_RETURN, VK_RIGHT, VK_SHIFT,
    VK_SPACE, VK_TAB, VK_UP,
};

/// Ctrl 現在按著嗎？
///
/// `lookup` 用它填 `KeyEvent.primary`（Windows 的主修飾鍵是 Ctrl）。Ctrl 系組合原則上留給宿主，
/// 綁定表裡有的例外（目前只有 `Ctrl+Shift+空白`），見 `defer_to_host`。
pub fn ctrl_down() -> bool {
    unsafe { GetKeyState(VK_CONTROL.0 as i32) < 0 }
}

/// Shift 現在按著嗎？
pub fn shift_down() -> bool {
    unsafe { GetKeyState(VK_SHIFT.0 as i32) < 0 }
}

/// 這一下 key-down 是系統的**自動重複**嗎？
///
/// `lparam` 的第 30 位元是「前一次的鍵狀態」——1 代表這個鍵**本來就
/// 按著**，也就是按住不放時系統重送的那些。
///
/// **用途在 2026-09-09 換了，判準本身一字不動**（那條規則已經錯過
/// 三次，見開發文件 §2.44）。原本是給 `CtrlTap` 用的——重複的 Ctrl
/// key-down 會把「這一輪用過了」洗掉，複製貼上就變成切換語言。
///
/// 現在是給**輪替型的動作**擋自動重複：`Shift+空白`（語言鎖定）與
/// `Ctrl+Shift+空白`（全半形）按住不放，系統會連續重送 key-down，
/// 不擋的話語言會瘋狂輪替。原本的單按 Ctrl 沒這個問題——它在放開時
/// 才觸發，而放開只會發生一次。
fn is_repeat_bits(lparam: isize) -> bool {
    lparam & 0x4000_0000 != 0
}

/// 同上，收 TSF 傳來的 `LPARAM`。
pub fn is_repeat(lparam: windows::Win32::Foundation::LPARAM) -> bool {
    is_repeat_bits(lparam.0)
}

/// 這個按鍵在這個模式下要做什麼？沒綁定就回 `None`（放行給宿主）。
///
/// 讀當下的 Shift／Ctrl、把鍵碼翻成 `Key`，再交給 `ime_core::binding::lookup`。
/// **Windows 的主修飾鍵是 Ctrl**（macOS 是 Cmd）。
pub fn lookup(mode: Mode, vk: u32) -> Option<Action> {
    let ev = KeyEvent {
        key: to_key(vk),
        shift: shift_down(),
        primary: ctrl_down(),
    };
    ime_core::binding::lookup(mode, ev)
}

/// 虛擬鍵碼 → 中性的 `Key`。
///
/// 順序：有名字的控制鍵 → 數字鍵盤 → 印得出來的字元 → 其他。
/// 字元那一步要看 Shift（`typed_char` 自己讀），所以 `Shift+1` 翻出來就是 `'!'`。
pub fn to_key(vk: u32) -> Key {
    const NAMED: [(u32, Key); 9] = [
        (VK_SPACE.0 as u32, Key::Space),
        (VK_RETURN.0 as u32, Key::Enter),
        (VK_TAB.0 as u32, Key::Tab),
        (VK_ESCAPE.0 as u32, Key::Esc),
        (VK_BACK.0 as u32, Key::Backspace),
        (VK_LEFT.0 as u32, Key::Left),
        (VK_RIGHT.0 as u32, Key::Right),
        (VK_UP.0 as u32, Key::Up),
        (VK_DOWN.0 as u32, Key::Down),
    ];
    if let Some((_, k)) = NAMED.iter().find(|(v, _)| *v == vk) {
        return *k;
    }
    // 只有 NumLock 開著時 Windows 才送 `VK_NUMPAD*`；關著送的是
    // Home/End/方向鍵那些，本來就該走別的路。
    if let Some(ch) = numpad_char(vk) {
        return Key::Numpad(ch);
    }
    typed_char(vk).map_or(Key::Other, Key::Char)
}

/// 這個虛擬鍵碼對應哪個輸入字元？不是輸入用的鍵回 `None`。
///
/// **要看 Shift**：Shift+1 是 `!` 而不是 `1`。原本沒看，所以
/// `!@#$%` 這些符號全打不出來——按下去只拿到數字，還被當成注音吃掉。
///
/// 空白鍵**不在這裡**——它在打字中是注音的一聲，在選字裡是展開，
/// 要看模式決定。見 `lookup` 與 `Mode`。
///
/// # 符號為什麼也算「輸入字元」
///
/// 符號會進組字區，切點引擎把純標點當**硬切點**——`su3cl3!wu0an`
/// 會切成「你好 │ ! │ 世界」，兩邊各自切不會合併。所以一口氣打完
/// 帶標點的句子是可行的，不必為了打一個驚嘆號先送出前半段。
/// 見 `ime_core::cutpoint::punct`。
/// 數字鍵盤上的鍵打出什麼字元。不是數字鍵盤的鍵回 `None`。
///
/// **NumLock 關著時 Windows 送的是別的 VK**（Home、End、方向鍵……），
/// 所以這裡只會在 NumLock 開著時命中——剛好就是使用者想打數字的時候。
pub fn numpad_char(vk: u32) -> Option<char> {
    match vk {
        0x60..=0x69 => char::from_digit(vk - 0x60, 10),
        0x6A => Some('*'),
        0x6B => Some('+'),
        0x6D => Some('-'),
        0x6E => Some('.'),
        0x6F => Some('/'),
        _ => None,
    }
}

pub fn typed_char(vk: u32) -> Option<char> {
    let shift = shift_down();
    match vk {
        // 字母：Shift 給大寫。**大寫要進組字**——不然打不出
        // Hello、GitHub 這種大寫開頭的英文詞。
        0x41..=0x5A => {
            let c = vk as u8 as char;
            Some(if shift { c } else { c.to_ascii_lowercase() })
        }
        // 數字列：Shift 給上排符號
        0x30..=0x39 => {
            let digit = (vk as u8) as char;
            Some(if shift { shifted_digit(digit) } else { digit })
        }
        // 注音鍵盤也會用到的符號鍵，各有 Shift 版本
        0xBC => Some(if shift { '<' } else { ',' }),
        0xBE => Some(if shift { '>' } else { '.' }),
        0xBA => Some(if shift { ':' } else { ';' }),
        0xBF => Some(if shift { '?' } else { '/' }),
        0xBD => Some(if shift { '_' } else { '-' }),
        // 其餘符號鍵。這些在注音鍵盤上沒有對應音符，
        // 純粹是標點——切點引擎會把它們當硬切點。
        0xBB => Some(if shift { '+' } else { '=' }),
        0xC0 => Some(if shift { '~' } else { '`' }),
        0xDB => Some(if shift { '{' } else { '[' }),
        0xDD => Some(if shift { '}' } else { ']' }),
        0xDC => Some(if shift { '|' } else { '\\' }),
        0xDE => Some(if shift { '"' } else { '\'' }),
        _ => None,
    }
}

/// 數字列按著 Shift 是哪個符號。
///
/// 這是 US 鍵盤的排列。之後要支援別種鍵盤配置的話，這張表要跟著換。
fn shifted_digit(d: char) -> char {
    match d {
        '1' => '!',
        '2' => '@',
        '3' => '#',
        '4' => '$',
        '5' => '%',
        '6' => '^',
        '7' => '&',
        '8' => '*',
        '9' => '(',
        '0' => ')',
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 自動重複的判準。**這條規則已經錯過三次**（見開發文件 §2.44），
    /// 用測試釘住——現在擋的是「按著 Shift+空白 不放會連續輪替語言」。
    #[test]
    fn 自動重複的位元判得出來() {
        assert!(!is_repeat_bits(0x0000_0001), "第一次按下");
        assert!(is_repeat_bits(0x4000_0001), "按著不放的重複");
    }

    #[test]
    fn shift_數字給符號() {
        // 使用者回報：!@#$% 這些打不出來。原因是 typed_char 沒看 Shift，
        // 按 Shift+1 只拿到 '1'，還被當成注音吃掉。
        //
        // 注意：`typed_char` 讀的是真實鍵盤狀態，沒按著 Shift 時
        // 回的是數字——所以這裡直接驗換算表。
        assert_eq!(shifted_digit('1'), '!');
        assert_eq!(shifted_digit('2'), '@');
        assert_eq!(shifted_digit('3'), '#');
        assert_eq!(shifted_digit('4'), '$');
        assert_eq!(shifted_digit('5'), '%');
        assert_eq!(shifted_digit('0'), ')');
    }

    #[test]
    fn 符號鍵有被接手() {
        // 這些鍵原本完全沒綁，按下去輸入法不管，符號進不了組字區
        for vk in [0xBBu32, 0xC0, 0xDB, 0xDD, 0xDC, 0xDE] {
            assert!(
                typed_char(vk).is_some(),
                "VK {vk:#x} 該是輸入字元（標點會當硬切點）"
            );
        }
    }

    /// 鍵位表搬進 core 之後，Windows 這邊唯一會錯的地方就是翻譯。
    /// 表裡用到的每一顆控制鍵都要翻對，不然那一整列綁定等於消失。
    #[test]
    fn 控制鍵翻得對() {
        for (vk, key) in [
            (VK_SPACE, Key::Space),
            (VK_RETURN, Key::Enter),
            (VK_TAB, Key::Tab),
            (VK_ESCAPE, Key::Esc),
            (VK_BACK, Key::Backspace),
            (VK_LEFT, Key::Left),
            (VK_RIGHT, Key::Right),
            (VK_UP, Key::Up),
            (VK_DOWN, Key::Down),
        ] {
            assert_eq!(to_key(vk.0 as u32), key, "{vk:?}");
        }
    }

    /// 數字鍵盤要翻成 `Numpad`，**不能翻成 `Char`**——那樣數字鍵盤的 5
    /// 會被當成注音的ㄓ。
    #[test]
    fn 數字鍵盤翻成numpad() {
        for d in 0..=9u32 {
            let ch = char::from_digit(d, 10).unwrap();
            assert_eq!(to_key(0x60 + d), Key::Numpad(ch), "VK_NUMPAD{d}");
        }
        for (vk, ch) in [
            (0x6A, '*'),
            (0x6B, '+'),
            (0x6D, '-'),
            (0x6E, '.'),
            (0x6F, '/'),
        ] {
            assert_eq!(to_key(vk), Key::Numpad(ch));
        }
    }

    #[test]
    fn 字母與主鍵盤數字翻成char() {
        // 測試環境沒按 Shift，字母是小寫、數字是數字
        assert_eq!(to_key(0x41), Key::Char('a'));
        assert_eq!(to_key(0x35), Key::Char('5'));
    }

    #[test]
    fn 沒有名字的鍵翻成other() {
        // F1、Home、End、PageUp、PageDown——組字中靠 `Key::Other` 吞掉
        for vk in [0x70u32, 0x24, 0x23, 0x21, 0x22] {
            assert_eq!(to_key(vk), Key::Other, "VK {vk:#x}");
        }
    }
}
