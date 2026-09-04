//! 十六進位色碼與各家顏色型別之間的轉換。
//!
//! 設定頁面（色票、輸入框）與候選視窗預覽都要用，所以獨立一組——
//! 它跟「畫什麼」無關，只是把 `"#1e1e1e"` 這種字串換成能畫的東西。

use eframe::egui;

pub(crate) fn to_rgb(hex: &str) -> ime_core::render::Rgb {
    ime_core::render::Rgb::parse_hex(hex).unwrap_or(ime_core::render::Rgb::new(0x80, 0x80, 0x80))
}

pub(crate) fn from_rgb(c: ime_core::render::Rgb) -> egui::Color32 {
    egui::Color32::from_rgb(c.r, c.g, c.b)
}

/// 解析 `"#RRGGBB"`。**解析本身在 `core`**——實際繪製解析的是同一批
/// 字串，兩邊各寫一份的話，容錯行為可能不一致。
pub(crate) fn parse_hex(s: &str) -> Option<[u8; 3]> {
    ime_core::render::Rgb::parse_hex(s).map(|c| [c.r, c.g, c.b])
}

pub(crate) fn to_color(s: &str) -> egui::Color32 {
    from_rgb(to_rgb(s))
}
