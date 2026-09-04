//! 候選視窗的主題：顏色、字型、尺寸。
//!
//! 規格見[開發文件 §4](../../../開發文件.md)。
//!
//! # 這一層在解決什麼
//!
//! 原本這些值散在 `candidate_window.rs` 的繪圖程式碼裡：
//!
//! ```ignore
//! FillRect(hdc, &rc, CreateSolidBrush(COLORREF(0x00F5FCFF)))
//! //                                          ↑ 這是「視窗背景」還是「候選列背景」？
//! ```
//!
//! 同一個數值可能代表好幾種語意，看不出差別。等要做深色模式，才會
//! 發現「背景」其實有五、六種角色，而程式碼裡全部混在一起。
//!
//! **主題層的本質是把「角色」命名清楚**——繪圖時取用名字而不是數值。
//! 之後換配色、加深色模式、讓使用者自訂，都只是換一組數值進去，
//! 繪圖程式碼不用再動。
//!
//! # 顏色為什麼要轉換
//!
//! 設定檔寫的是 `"#RRGGBB"`（跟 CSS 一樣，人看得懂），但 Win32 的
//! `COLORREF` 是 **BGR** 順序。直接把 `0xF5FCFF` 塞進去會紅藍顛倒。
//! 轉換集中在 `Color::to_colorref()`，繪圖端不必知道這件事。

use windows::Win32::Foundation::COLORREF;

/// 一個顏色。內部存 RGB，輸出時才轉成 Win32 的 BGR。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// 解析 `"#RRGGBB"` 或 `"RRGGBB"`。格式不對回 `None`。
    ///
    /// 解析本身在 `core`——設定頁也要解析同一批字串，兩邊必須一致。
    pub fn parse(s: &str) -> Option<Self> {
        ime_core::render::Rgb::parse_hex(s).map(Self::from)
    }

    /// 轉成 `core` 的平台無關顏色，餵給共用的繪製決策。
    pub const fn to_rgb(self) -> ime_core::render::Rgb {
        ime_core::render::Rgb::new(self.r, self.g, self.b)
    }

    /// 轉成 Win32 的 `COLORREF`（BGR 順序）。
    pub fn to_colorref(self) -> COLORREF {
        COLORREF((self.b as u32) << 16 | (self.g as u32) << 8 | self.r as u32)
    }
}

impl From<ime_core::render::Rgb> for Color {
    fn from(c: ime_core::render::Rgb) -> Self {
        Self::rgb(c.r, c.g, c.b)
    }
}

/// 顏色角色。**列的是用途，不是顏色**——深色模式就是同一組角色換一組值。
#[derive(Debug, Clone, Copy)]
pub struct Colors {
    /// 整個 popup 的底（漸層的**上緣**色）
    pub window_bg: Color,
    /// 視窗底的漸層**下緣**色。跟 `window_bg` 相同就是純色。
    pub window_bg2: Color,
    /// 候選字
    pub text: Color,
    /// 候選字前的編號。**跟候選字是不同角色**——層次感就來自這個差異
    pub index: Color,
    /// 反白那一列的底
    pub highlight_bg: Color,
    /// 反白那一列的字**與編號**（合併成一個角色）
    pub highlight_text: Color,
    /// 預覽列的文字**與標記符號**（合併成一個角色）
    pub preview_text: Color,
    /// 預覽列的底（漸層的**上緣**色）
    pub preview_bg: Color,
    /// 預覽列漸層的**下緣**色。跟 `preview_bg` 相同就是純色。
    pub preview_bg2: Color,
    /// 預覽列與候選清單之間的線
    pub separator: Color,
}

/// 字型。
#[derive(Debug, Clone)]
pub struct Font {
    /// 字族。**空字串代表跟隨系統 UI 字型**——
    /// 寫死字型名在沒裝那套字的機器上會退化成很醜的預設字型。
    pub family: String,
}

/// 尺寸與間距。
///
/// **這些是「邏輯像素」**，實際繪製時要乘上 DPI 縮放係數。
/// 高 DPI 螢幕若直接用實際像素會糊掉或過小。
#[derive(Debug, Clone, Copy)]
pub struct Metrics {
    /// 反白條的樣式（實心／高光帶／只有高光）。
    pub highlight_style: ime_core::config::HighlightStyle,
    /// 整體縮放百分比（50～200）。**版面的每個尺寸都乘上它**，
    /// 包括字級——使用者要的是「整個視窗大一點」，不是分開調六七個數值。
    pub scale_percent: i32,
}

/// 寫死的版面常數（邏輯像素）。
///
/// **定義在 `core` 裡**——設定頁的預覽也要用同一份。以前兩邊各抄一次，
/// 靠註解提醒「記得對齊」，那種對齊遲早失守（見 `ime_core::render`）。
pub use ime_core::render::fixed;

/// 一整套主題。
#[derive(Debug, Clone)]
pub struct Theme {
    pub colors: Colors,
    pub font: Font,
    pub metrics: Metrics,
    /// 背景圖設定。直接沿用 `core` 的型別——這裡沒有需要換算的東西
    /// （路徑是字串、濃度是比例），不像顏色要解析、尺寸要套縮放。
    pub background: ime_core::config::Background,
}

impl Default for Theme {
    /// 內建預設：模仿 Windows 11 新注音的淺色配色。
    ///
    /// 這是最後防線——設定檔讀不到、解析失敗、欄位缺漏時都退到這裡，
    /// 輸入法一定畫得出東西。
    fn default() -> Self {
        Self {
            colors: Colors {
                window_bg: Color::rgb(0xFB, 0xFB, 0xFB),
                // 預設兩色相同＝純色，不改設定的人看不出差別
                window_bg2: Color::rgb(0xFB, 0xFB, 0xFB),
                text: Color::rgb(0x1A, 0x1A, 0x1A),
                index: Color::rgb(0x90, 0x90, 0x90),
                highlight_bg: Color::rgb(0x00, 0x78, 0xD4),
                highlight_text: Color::rgb(0xFF, 0xFF, 0xFF),
                preview_text: Color::rgb(0x00, 0x60, 0xA8),
                preview_bg: Color::rgb(0xF2, 0xF7, 0xFB),
                preview_bg2: Color::rgb(0xF2, 0xF7, 0xFB),
                separator: Color::rgb(0xE4, 0xE4, 0xE4),
            },
            font: Font {
                family: String::new(),
            },
            metrics: Metrics {
                scale_percent: 100,
                highlight_style: Default::default(),
            },
            // 預設不用背景圖
            background: Default::default(),
        }
    }
}

impl Theme {
    /// 從使用者設定建出主題。
    ///
    /// 顏色在設定檔裡是 `"#RRGGBB"` 字串，解析不了的**逐欄退回預設**——
    /// 一個顏色寫錯不該讓整份主題失效。
    pub fn from_config(c: &ime_core::config::Config) -> Self {
        let d = Self::default();
        let col = |s: &str, fallback: Color| Color::parse(s).unwrap_or(fallback);
        Self {
            colors: Colors {
                window_bg: col(&c.colors.window_bg, d.colors.window_bg),
                // 沒設就跟上緣同色（純色）
                window_bg2: col(&c.colors.window_bg2, d.colors.window_bg),
                text: col(&c.colors.text, d.colors.text),
                index: col(&c.colors.index, d.colors.index),
                highlight_bg: col(&c.colors.highlight_bg, d.colors.highlight_bg),
                highlight_text: col(&c.colors.highlight_text, d.colors.highlight_text),
                preview_text: col(&c.colors.preview_text, d.colors.preview_text),
                preview_bg: col(&c.colors.preview_bg, d.colors.preview_bg),
                // 沒設就跟上緣同色（純色）
                preview_bg2: col(&c.colors.preview_bg2, d.colors.preview_bg),
                separator: col(&c.colors.separator, d.colors.separator),
            },
            font: Font {
                family: c.font.family.clone(),
            },
            metrics: Metrics {
                // 夾在合法範圍內——設定檔被手改成 0 或 9999 也不能讓
                // 視窗畫成一個點或整個螢幕那麼大
                scale_percent: c.metrics.scale_percent.clamp(50, 200),
                highlight_style: c.metrics.highlight_style,
            },
            background: ime_core::config::Background {
                image: c.background.image.clone(),
                // 手改設定檔寫成 -5 或 99 也不能讓遮罩算出負數
                strength: c.background.strength.clamp(0.0, 1.0),
                text_outline: c.background.text_outline.clamp(0.0, 1.0),
            },
        }
    }
}

impl Metrics {
    /// 把一個基準尺寸乘上縮放百分比。
    ///
    /// 版面的每個數值都要經過這裡，這樣「整體放大」才是等比的——
    /// 漏掉一個就會比例失衡（例如字變大但行高沒變，字會被切掉）。
    pub fn scale(&self, base: i32) -> i32 {
        (base * self.scale_percent / 100).max(1)
    }

    /// 每一列的高度（已縮放）
    pub fn line_height(&self) -> i32 {
        self.scale(fixed::LINE_HEIGHT)
    }
    /// 字級（已縮放）
    pub fn font_size_pt(&self) -> i32 {
        self.scale(fixed::FONT_SIZE_PT)
    }
    /// 視窗內距（已縮放）
    pub fn padding(&self) -> i32 {
        self.scale(fixed::PADDING)
    }
    /// 圓角半徑（已縮放）
    pub fn corner_radius(&self) -> i32 {
        self.scale(fixed::CORNER_RADIUS)
    }
    /// 編號與候選字的間隔（已縮放）
    pub fn index_gap(&self) -> i32 {
        self.scale(fixed::INDEX_GAP)
    }
    /// 視窗最小寬度（已縮放）
    pub fn min_width(&self) -> i32 {
        self.scale(fixed::MIN_WIDTH)
    }
    /// 視窗最大寬度（已縮放）
    pub fn max_width(&self) -> i32 {
        self.scale(fixed::MAX_WIDTH)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 解析十六進位顏色() {
        assert_eq!(Color::parse("#0078D4"), Some(Color::rgb(0x00, 0x78, 0xD4)));
        // 不加井字號也要收
        assert_eq!(Color::parse("0078D4"), Some(Color::rgb(0x00, 0x78, 0xD4)));
        // 前後空白要忽略
        assert_eq!(Color::parse(" #FFFFFF "), Some(Color::rgb(255, 255, 255)));
    }

    #[test]
    fn 格式不對回_none() {
        assert_eq!(Color::parse("#12345"), None, "長度不對");
        assert_eq!(Color::parse("#GGGGGG"), None, "非十六進位");
        assert_eq!(Color::parse(""), None);
    }

    #[test]
    fn 轉成_colorref_是_bgr_順序() {
        // 純紅在 RGB 是 FF0000，在 COLORREF 要是 0x0000FF
        assert_eq!(Color::rgb(0xFF, 0, 0).to_colorref().0, 0x0000FF);
        // 純藍反過來
        assert_eq!(Color::rgb(0, 0, 0xFF).to_colorref().0, 0xFF0000);
        // 綠色在中間，不受影響
        assert_eq!(Color::rgb(0, 0xFF, 0).to_colorref().0, 0x00FF00);
    }

    #[test]
    fn 預設主題的角色都不同() {
        let t = Theme::default();
        // 反白底跟一般底必須不同，不然看不出選中哪個
        assert_ne!(t.colors.highlight_bg, t.colors.window_bg);
        // 反白字要跟反白底對比
        assert_ne!(t.colors.highlight_text, t.colors.highlight_bg);
        // 編號要比候選字淡，才有層次
        assert_ne!(t.colors.index, t.colors.text);
    }

    #[test]
    fn 從設定建主題() {
        let mut c = ime_core::config::Config::default();
        c.colors.highlight_bg = "#FF0000".into();
        c.font.family = "標楷體".into();
        let t = Theme::from_config(&c);
        assert_eq!(t.colors.highlight_bg, Color::rgb(0xFF, 0, 0));
        assert_eq!(t.font.family, "標楷體");
    }

    #[test]
    fn 顏色寫錯只退那一欄() {
        // 一個顏色寫錯不該讓整份主題失效
        let mut c = ime_core::config::Config::default();
        c.colors.highlight_bg = "不是顏色".into();
        c.colors.text = "#123456".into();
        let t = Theme::from_config(&c);
        assert_eq!(
            t.colors.highlight_bg,
            Theme::default().colors.highlight_bg,
            "壞的退預設"
        );
        assert_eq!(t.colors.text, Color::rgb(0x12, 0x34, 0x56), "好的照用");
    }

    #[test]
    fn 縮放百分之百時等於基準值() {
        let t = Theme::from_config(&ime_core::config::Config::default());
        assert_eq!(t.metrics.line_height(), fixed::LINE_HEIGHT);
        assert_eq!(t.metrics.font_size_pt(), fixed::FONT_SIZE_PT);
    }

    #[test]
    fn 縮放等比放大每個尺寸() {
        let mut c = ime_core::config::Config::default();
        c.metrics.scale_percent = 200;
        let t = Theme::from_config(&c);
        // 每個數值都要跟著放大，漏掉一個就會比例失衡
        assert_eq!(t.metrics.line_height(), fixed::LINE_HEIGHT * 2);
        assert_eq!(t.metrics.font_size_pt(), fixed::FONT_SIZE_PT * 2);
        assert_eq!(t.metrics.padding(), fixed::PADDING * 2);
        assert_eq!(t.metrics.corner_radius(), fixed::CORNER_RADIUS * 2);
        assert_eq!(t.metrics.index_gap(), fixed::INDEX_GAP * 2);
    }

    #[test]
    fn 縮放被夾在合法範圍內() {
        let mut c = ime_core::config::Config::default();
        // 設定檔被手改成離譜的值也不能讓視窗畫成一個點
        c.metrics.scale_percent = 0;
        assert_eq!(Theme::from_config(&c).metrics.scale_percent, 50);
        c.metrics.scale_percent = 9999;
        assert_eq!(Theme::from_config(&c).metrics.scale_percent, 200);
    }
}
