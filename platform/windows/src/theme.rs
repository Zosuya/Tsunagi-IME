//! 主題的 Windows 專屬部分：把顏色轉成 Win32 的 `COLORREF`。
//!
//! 主題本身（角色命名、顏色值、字型、尺寸）在 [`ime_core::theme`]——
//! 那些跟平台無關，macOS 也用同一份。
//!
//! # 為什麼只有轉換留在這裡
//!
//! 設定檔寫的是 `"#RRGGBB"`（跟 CSS 一樣，人看得懂），但 Win32 的
//! `COLORREF` 是 **BGR** 順序。直接把 `0xF5FCFF` 塞進去會紅藍顛倒。
//!
//! 「顏色是多少」是決策，兩個平台一樣；「這個平台的顏色型別長什麼樣」
//! 是繪圖細節，只有這裡需要。所以 core 存 RGB，轉換在平台層做。

use windows::Win32::Foundation::COLORREF;

pub use ime_core::theme::*;

/// 讓 [`Color`] 能轉成 Win32 的 `COLORREF`。
///
/// 做成擴充 trait 而不是 `From`，是為了讓呼叫端讀起來跟搬動前一樣
/// （`c.to_colorref()`），22 處引用點不用改寫法。
pub trait ToColorRef {
    /// 轉成 Win32 的 `COLORREF`（BGR 順序）。
    fn to_colorref(self) -> COLORREF;
}

impl ToColorRef for Color {
    fn to_colorref(self) -> COLORREF {
        COLORREF((self.b as u32) << 16 | (self.g as u32) << 8 | self.r as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 轉成_colorref_是_bgr_順序() {
        // 純紅在 RGB 是 FF0000，在 COLORREF 要是 0x0000FF
        assert_eq!(Color::rgb(0xFF, 0, 0).to_colorref().0, 0x0000FF);
        // 純藍反過來
        assert_eq!(Color::rgb(0, 0, 0xFF).to_colorref().0, 0xFF0000);
        // 綠色在中間，不受影響
        assert_eq!(Color::rgb(0, 0xFF, 0).to_colorref().0, 0x00FF00);
    }
}
