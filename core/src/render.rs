//! 繪製的**決策**（不是繪製本身）。
//!
//! # 為什麼需要這一層
//!
//! 候選視窗有兩個實作，用的繪圖技術完全不同：
//!
//! | | 實際候選視窗 | 設定頁的預覽 |
//! |---|---|---|
//! | 身分 | DLL，寄生在宿主行程 | 獨立執行檔 |
//! | 繪圖 | Direct2D | egui |
//!
//! 分開是有理由的（設定頁獨立才不會鎖住 DLL、崩潰也不會拖垮宿主），
//! 但代價是繪圖程式碼無法共用。
//!
//! **繪圖不能共用，判斷可以**。這個模組放的是「該畫成什麼樣」——
//! 要不要鋪底色、用哪個顏色、圓角多大——回傳數值，不碰任何繪圖 API。
//! 兩邊都問同一份，剩下的差別只有「用什麼畫」。
//!
//! # 為什麼特別重要
//!
//! 這些判斷原本各寫一份，同一類 bug 出現過三次：
//!
//! 1. 反白列的編號在「只有高光」下看不清（改了實際、忘了預覽）
//! 2. 反白外框圓成膠囊（預覽照抄絕對值，沒跟著除以 2）
//! 3. 預覽列白底白字整段看不見（預覽固定鋪底，沒看樣式）
//!
//! 三次都是「改一邊、忘了另一邊」。

use crate::config::HighlightStyle;

/// 一個顏色。**平台無關**——Direct2D 與 egui 各自轉成自己的型別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// 解析 `"#RRGGBB"` 或 `"RRGGBB"`。格式不對回 `None`——
    /// 呼叫端自己決定要退到哪個預設值。
    pub fn parse_hex(s: &str) -> Option<Self> {
        let h = s.trim().trim_start_matches('#');
        if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let v = u32::from_str_radix(h, 16).ok()?;
        Some(Self::new(
            ((v >> 16) & 0xFF) as u8,
            ((v >> 8) & 0xFF) as u8,
            (v & 0xFF) as u8,
        ))
    }
}

/// 寫死的版面常數（邏輯像素，未套縮放與 DPI）。
///
/// 這些原本是設定項，但調它們的邊際效益很低，留著只是讓設定頁變複雜
/// ——使用者決定固定下來。
///
/// **兩邊都從這裡拿**。以前是設定頁手抄一份，註解還寫著「跟 theme.rs
/// 的 fixed 對齊」——那種靠人記得的對齊遲早會失守。
pub mod fixed {
    /// 視窗內距
    pub const PADDING: i32 = 8;
    /// 圓角半徑。**只套在候選清單那一塊**，預覽列不套。
    pub const CORNER_RADIUS: i32 = 7;
    /// 編號與候選字之間的間隔
    pub const INDEX_GAP: i32 = 10;
    /// 視窗最小寬度
    pub const MIN_WIDTH: i32 = 96;
    /// 視窗最大寬度
    pub const MAX_WIDTH: i32 = 640;
    /// 每一列的高度
    pub const LINE_HEIGHT: i32 = 28;
    /// 字級（點）
    pub const FONT_SIZE_PT: i32 = 12;
}

/// 反白塊的圓角是視窗圓角的幾分之一。
///
/// 外框圓潤、內部反白收斂，兩層才不會看起來一樣圓。
pub const HIGHLIGHT_RADIUS_DIVISOR: f32 = 2.0;

/// 反白塊的圓角半徑，**依所在的列高換算**。
///
/// 直接把絕對值抄到別處會走鐘：設定頁的預覽列由字級決定高度，比實際的
/// [`fixed::LINE_HEIGHT`] 矮一半以上，同樣的半徑看起來會圓得多（膠囊形）。
/// 傳入實際的列高，比例才一致。
pub fn highlight_radius_for_row(row_height: f32) -> f32 {
    row_height * (fixed::CORNER_RADIUS as f32 / HIGHLIGHT_RADIUS_DIVISOR)
        / fixed::LINE_HEIGHT as f32
}

/// 反白一列時該怎麼畫。
///
/// 只說「畫成什麼樣」，不說「怎麼畫」——填色是 `FillRect` 還是
/// `egui::Frame` 由呼叫端決定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HighlightPaint {
    /// 底色。`None` 代表**不鋪底**（「只有高光」樣式）。
    pub fill: Option<Rgb>,
    /// 反白列的候選字顏色。
    pub text: Rgb,
    /// 反白列的**編號**顏色。
    ///
    /// 跟 `text` 同色——編號平常用淡色跟候選字分層，但反白之後
    /// 那個分層沒有意義了，而且「只有高光」沒有深底，淡灰會看不見。
    pub index: Rgb,
    /// 要不要畫上緣高光帶（玻璃有厚度，上半部會反光）。
    pub sheen: bool,
    /// 要不要畫亮邊。
    ///
    /// 「只有高光」沒有底色，那條邊是**唯一**能標出範圍的東西。
    pub outline: bool,
}

/// 反白該怎麼畫。
///
/// `base_text` 是**沒反白時**該用的字色——候選列傳 `colors.text`，
/// 預覽列傳 `colors.preview_text`。「只有高光」沒有深色底，字要維持
/// 原色才看得見（換成白字會消失在淺色底上）。
pub fn highlight_paint(
    style: HighlightStyle,
    highlight_bg: Rgb,
    highlight_text: Rgb,
    base_text: Rgb,
) -> HighlightPaint {
    let solid_bg = style != HighlightStyle::SheenOnly;
    let text = if style.recolors_text() {
        highlight_text
    } else {
        base_text
    };
    HighlightPaint {
        fill: solid_bg.then_some(highlight_bg),
        text,
        index: text,
        sheen: style != HighlightStyle::Solid,
        outline: style == HighlightStyle::SheenOnly,
    }
}

/// 一般螢幕的 gamma。
const GAMMA: f32 = 2.2;

/// **深色遮罩**要用多少不透明度，才跟預覽看起來一樣。
///
/// # 為什麼需要補償
///
/// 兩邊在不同的色彩空間混色：
///
/// - 候選視窗（Direct2D）：**gamma 空間**——半透明疊上去壓得比較暗
/// - 設定頁預覽（egui）：**線性空間**——保留較多亮度
///
/// 同樣的數值，實際看起來比預覽暗。想從根本解決得換掉繪圖表面的
/// 格式，但 DirectComposition **不支援 sRGB 表面**（實測確認，會退回
/// 一般格式），那條路是死的。所以改成在數值上補償。
///
/// # 這是近似，不是等價
///
/// 正確的換算跟「底下那層的顏色」有關，而背景是一張圖、每個像素都
/// 不同——不可能有單一解。這裡取一個夠好的近似：深色遮罩的視覺效果
/// 主要是「背景還看得到多少」，也就是 `1-a`；線性空間下那個比例是
/// `(1-a)^(1/gamma)`。反推回 gamma 空間就是下面這條。
pub fn dim_alpha_for_gamma_blend(a: f32) -> f32 {
    (1.0 - (1.0 - a.clamp(0.0, 1.0)).powf(1.0 / GAMMA)).clamp(0.0, 1.0)
}

/// **淺色疊加**（上緣那道高光）要用多少不透明度。
///
/// 同樣是近似，理由見 [`dim_alpha_for_gamma_blend`]。淺色的情況取
/// 「底下全黑」這個極端——那時線性空間的結果是 `a^(1/gamma)`。
/// 底下越亮，補償就會稍微過頭，但方向是對的（實際本來就偏暗）。
pub fn lighten_alpha_for_gamma_blend(a: f32) -> f32 {
    a.clamp(0.0, 1.0).powf(1.0 / GAMMA)
}

/// 上緣高光帶佔整列高度的比例。
pub const SHEEN_BAND_RATIO: f32 = 0.42;
/// 上緣高光帶**最上緣**的白色不透明度。
///
/// `1.0` = 純白（`#FFFFFF`）。往下漸層到全透明，所以只有最上面那條
/// 是實白，整體看起來仍是一道光而不是一塊白。
pub const SHEEN_ALPHA: f32 = 1.0;
/// 亮邊的顏色是 `highlight_bg` 乘上這個係數（壓暗一點才不會太搶）。
pub const OUTLINE_DIM: f32 = 0.75;
/// 等比填滿（超出裁掉）時該用的縮放倍率。
///
/// # 為什麼只共用倍率
///
/// 「像手機桌布那樣填滿、多的裁掉」這個規則，兩邊的**決策**是同一個
/// ——取寬高比例中較大的那個倍率。但輸出形式不同：實際繪製要的是
/// 貼圖矩陣，設定頁的預覽要的是 uv 座標。共用機制反而綁死其中一邊。
///
/// **置中的位移可以由倍率推導**，所以共用這一個數字就足以保證兩邊
/// 對得起來：
///
/// - 繪製：`offset = (目標邊長 − 圖邊長 × 倍率) / 2`
/// - 預覽：`用掉的比例 = 目標邊長 / (圖邊長 × 倍率)`，再置中
///
/// 尺寸有零或負數時回 `1.0`——那時畫不出東西，回什麼都一樣，但不能
/// 讓它變成 `NaN` 或無限大傳出去。
pub fn cover_scale(dst_w: f32, dst_h: f32, img_w: f32, img_h: f32) -> f32 {
    if dst_w <= 0.0 || dst_h <= 0.0 || img_w <= 0.0 || img_h <= 0.0 {
        return 1.0;
    }
    (dst_w / img_w).max(dst_h / img_h)
}

/// 亮邊的線寬（邏輯像素）。
///
/// 「只有高光」樣式沒有底色，這條邊是唯一能標出範圍的東西，
/// 所以要夠明顯——1.5 在實機上偏細，使用者要求加粗。
pub const OUTLINE_WIDTH: f32 = 2.5;

#[cfg(test)]
mod tests {
    /// 等比填滿的倍率。實際繪製與設定頁預覽共用這一份，所以這裡釘的
    /// 是「兩邊一定會得到同一個答案」。
    mod 等比填滿 {
        use super::*;

        #[test]
        fn 取比較大的那個倍率() {
            // 目標 100x100、圖 200x100：寬要 0.5 倍、高要 1.0 倍
            // 取 0.5 的話高度會不夠、露出底色，所以要取 1.0
            assert_eq!(cover_scale(100.0, 100.0, 200.0, 100.0), 1.0);
            // 反過來也一樣
            assert_eq!(cover_scale(100.0, 100.0, 100.0, 200.0), 1.0);
        }

        #[test]
        fn 一定填得滿() {
            // 隨便挑幾組尺寸，縮放後的圖都不該小於目標
            for (dw, dh, iw, ih) in [
                (300.0, 80.0, 1920.0, 1080.0),
                (80.0, 300.0, 1920.0, 1080.0),
                (640.0, 480.0, 100.0, 100.0),
                (1.0, 1000.0, 500.0, 3.0),
            ] {
                let s = cover_scale(dw, dh, iw, ih);
                assert!(iw * s >= dw - 0.001, "寬度沒填滿：{dw}x{dh} 圖 {iw}x{ih}");
                assert!(ih * s >= dh - 0.001, "高度沒填滿：{dw}x{dh} 圖 {iw}x{ih}");
            }
        }

        #[test]
        fn 尺寸有零不會變成無限大() {
            // 視窗還沒量到大小、圖還沒載完都會出現零，
            // 讓 NaN 或 inf 流進繪圖層的話畫面會整片消失
            for (dw, dh, iw, ih) in [
                (0.0, 100.0, 100.0, 100.0),
                (100.0, 0.0, 100.0, 100.0),
                (100.0, 100.0, 0.0, 100.0),
                (100.0, 100.0, 100.0, 0.0),
                (-5.0, 100.0, 100.0, 100.0),
            ] {
                assert!(cover_scale(dw, dh, iw, ih).is_finite());
            }
        }
    }

    use super::*;

    #[test]
    fn 解析顏色() {
        assert_eq!(Rgb::parse_hex("#ECEDF2"), Some(Rgb::new(0xEC, 0xED, 0xF2)));
        assert_eq!(Rgb::parse_hex("ECEDF2"), Some(Rgb::new(0xEC, 0xED, 0xF2)));
        assert_eq!(Rgb::parse_hex("#GGG"), None);
        assert_eq!(Rgb::parse_hex(""), None);
    }

    /// 這幾個測試盯的是那三次重複出現的 bug。
    mod 反白 {
        use super::*;

        const BG: Rgb = Rgb::new(0xEC, 0xED, 0xF2); // 近白
        const HL_TEXT: Rgb = Rgb::new(0xFF, 0xFF, 0xFF); // 純白
        const BASE: Rgb = Rgb::new(0x7F, 0xC4, 0xF5); // 淺藍（預覽列原色）

        #[test]
        fn 實心_鋪底並換成反白字色() {
            let p = highlight_paint(HighlightStyle::Solid, BG, HL_TEXT, BASE);
            assert_eq!(p.fill, Some(BG));
            assert_eq!(p.text, HL_TEXT);
            assert!(!p.sheen, "實心不需要高光帶");
            assert!(!p.outline);
        }

        #[test]
        fn 高光帶_鋪底且有高光() {
            let p = highlight_paint(HighlightStyle::Sheen, BG, HL_TEXT, BASE);
            assert_eq!(p.fill, Some(BG));
            assert_eq!(p.text, HL_TEXT);
            assert!(p.sheen);
            assert!(!p.outline, "有底色就不必再靠亮邊標範圍");
        }

        #[test]
        fn 只有高光_不鋪底且字維持原色() {
            // 這是那個「白底白字整段看不見」的 bug：預覽列曾經固定鋪
            // highlight_bg(近白) 配 highlight_text(純白)
            let p = highlight_paint(HighlightStyle::SheenOnly, BG, HL_TEXT, BASE);
            assert_eq!(p.fill, None, "沒有底色");
            assert_eq!(p.text, BASE, "字要維持原色，換成白字會消失");
            assert!(p.sheen);
            assert!(p.outline, "沒底色時亮邊是唯一能標出範圍的東西");
        }

        #[test]
        fn 編號永遠跟文字同色() {
            // 這是那個「↑↑↓↓ 看不清」的 bug：編號留著淡灰，
            // 在深色主題背景上幾乎看不見
            for s in [
                HighlightStyle::Solid,
                HighlightStyle::Sheen,
                HighlightStyle::SheenOnly,
            ] {
                let p = highlight_paint(s, BG, HL_TEXT, BASE);
                assert_eq!(p.index, p.text, "{s:?} 的編號該跟文字同色");
            }
        }
    }

    /// 補償的方向要對：深色遮罩要變**淡**（讓背景透出來），
    /// 淺色高光要變**濃**（實際本來就偏暗）。
    mod 混色補償 {
        use super::*;

        #[test]
        fn 深色遮罩補償後變淡() {
            let a = 0.4;
            let c = dim_alpha_for_gamma_blend(a);
            assert!(c < a, "遮罩要變淡，背景圖才透得出來（{c} 應小於 {a}）");
            assert!(c > 0.0);
        }

        #[test]
        fn 淺色高光補償後變濃() {
            let a = 0.38;
            let c = lighten_alpha_for_gamma_blend(a);
            assert!(c > a, "光要更明顯（{c} 應大於 {a}）");
            assert!(c <= 1.0);
        }

        #[test]
        fn 兩端不變() {
            // 全透明與全不透明沒有補償的餘地，補過頭會出現破圖
            assert_eq!(dim_alpha_for_gamma_blend(0.0), 0.0);
            assert_eq!(dim_alpha_for_gamma_blend(1.0), 1.0);
            assert_eq!(lighten_alpha_for_gamma_blend(0.0), 0.0);
            assert_eq!(lighten_alpha_for_gamma_blend(1.0), 1.0);
        }

        #[test]
        fn 超出範圍的輸入會被夾住() {
            // 設定檔被手改壞時不能算出負數或大於 1 的透明度
            assert_eq!(dim_alpha_for_gamma_blend(-1.0), 0.0);
            assert_eq!(dim_alpha_for_gamma_blend(9.0), 1.0);
            assert_eq!(lighten_alpha_for_gamma_blend(-1.0), 0.0);
            assert_eq!(lighten_alpha_for_gamma_blend(9.0), 1.0);
        }
    }

    #[test]
    fn 圓角依列高換算() {
        // 實際的列高就是基準，換算出來要等於「視窗圓角的一半」
        let full = highlight_radius_for_row(fixed::LINE_HEIGHT as f32);
        assert!((full - fixed::CORNER_RADIUS as f32 / 2.0).abs() < 0.001);

        // 這是那個「圓成膠囊」的 bug：設定頁的列矮一半，
        // 半徑要跟著減半，不能照抄絕對值
        let half = highlight_radius_for_row(fixed::LINE_HEIGHT as f32 / 2.0);
        assert!((half - full / 2.0).abs() < 0.001);
    }
}
