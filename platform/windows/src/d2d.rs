//! Direct2D + DirectComposition 的繪圖層。
//!
//! 取代原本的 GDI（`candidate_window.rs` 的 `paint()`）。**主題完全
//! 沿用**——`theme.rs` 的顏色角色是平台無關的資料，這裡只是把
//! `Color` 轉成 D2D 的顏色格式。
//!
//! # 為什麼換掉 GDI
//!
//! 實測（`bin/bench_render.rs`，2026-08-30）：
//!
//! | | GDI | D2D + DComp |
//! |---|---|---|
//! | 初始化 | 0.06 ms | **130 ms**（只做一次） |
//! | 每幀 | 1.88 ms | **0.14 ms** |
//! | p99 | 2.4 ms | 0.6 ms |
//!
//! 速度快 13 倍，但那不是主要理由——**GDI 的 p99 只有 2.4ms，
//! 在 16ms 預算裡本來就夠用**。真正的理由是畫質：
//!
//! | | GDI | D2D |
//! |---|---|---|
//! | 圓角 | `SetWindowRgn`，鋸齒 | `FillRoundedRectangle`，反鋸齒 |
//! | 半透明 | 無（只能混色模擬） | 原生 |
//! | 漸層 | `GradientFill`（msimg32） | 漸層筆刷，更平順 |
//! | 圖片縮放 | 最近鄰，糊 | 雙線性 |
//!
//! # 初始化很貴，要挑時機
//!
//! 130ms 不能在第一次打字時付——那會卡得很明顯。跟詞庫預載一樣，
//! 放在輸入法啟用（`Activate`）時做，使用者切輸入法時吸收掉。
//!
//! # 骨架
//!
//! ```text
//! D3D11 Device ──→ DXGI Device ──→ D2D Device ──→ D2D DeviceContext
//!                       │                              ↑
//!                       └→ DComp Device ──→ Target ──→ Visual ──→ Surface
//! ```

use crate::theme::Color;
use windows::core::{Interface, Result, PCWSTR};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap1, ID2D1Brush, ID2D1DeviceContext, ID2D1Factory1,
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
    D2D1_BUFFER_PRECISION_8BPC_UNORM, D2D1_COLOR_INTERPOLATION_MODE_PREMULTIPLIED,
    D2D1_COLOR_SPACE_SRGB, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_EXTEND_MODE_CLAMP, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionSurface, IDCompositionTarget,
    IDCompositionVisual,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_METRICS, DWRITE_WORD_WRAPPING_NO_WRAP,
    DWRITE_WORD_WRAPPING_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM,
};
use windows::Win32::Graphics::Dxgi::{IDXGIDevice, IDXGISurface};

/// 把主題的顏色轉成 D2D 的格式。
///
/// **合成器用 `PREMULTIPLIED`**：RGB 要先乘上 alpha，不然半透明的
/// 顏色會算錯（開發文件 §4.10 記過這個坑）。
pub fn d2d_color(c: Color, alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: c.r as f32 / 255.0 * alpha,
        g: c.g as f32 / 255.0 * alpha,
        b: c.b as f32 / 255.0 * alpha,
        a: alpha,
    }
}

/// 一塊矩形（視窗座標，左上原點）。
///
/// 把四個座標收成一個型別——繪製函式本來每個都要吃 4 個 f32
/// 再加顏色與圓角，參數列長到看不出哪個是哪個。
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Rect {
    pub fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }
}

/// 只量文字、不畫圖的量測器。
///
/// # 為什麼要跟 `Renderer` 分開
///
/// 視窗寬度要在 `CreateWindowExW` **之前**算出來，而 `Renderer`
/// 需要視窗才建得起來——先有雞還是先有蛋。
///
/// 但 DirectWrite 的 factory **不需要視窗**，所以量測可以獨立出來。
/// 這樣算寬度與繪製就用同一個引擎，不會像先前用 GDI 量、DirectWrite
/// 畫那樣差幾十像素（長句子時差異會累積，視窗就不夠寬）。
pub struct TextMeasurer {
    write: IDWriteFactory,
}

impl TextMeasurer {
    pub fn new() -> Result<Self> {
        unsafe {
            let write: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            Ok(Self { write })
        }
    }

    /// 建文字格式。跟 `Renderer::text_format` 同樣的規則。
    pub fn format(&self, family: &str, size_pt: f32, dpi: f32) -> Result<IDWriteTextFormat> {
        make_format(&self.write, family, size_pt, dpi)
    }

    /// 量一段文字在限定寬度內要佔多寬多高。
    pub fn measure(&self, text: &str, fmt: &IDWriteTextFormat, max_width: f32) -> (f32, f32) {
        measure_with(&self.write, text, fmt, max_width)
    }
}

/// 把圖檔解碼成 32bppPBGRA 像素。回傳 `(寬, 高, 像素)`。
///
/// # 為什麼跟「建點陣圖」分開
///
/// 解碼要讀檔＋解壓縮，**慢**；建點陣圖只是把記憶體交給 GPU，**快**。
/// 而且兩者的生命週期完全不同：
///
/// - **像素跟裝置無關**，一路留著沒問題
/// - **點陣圖綁在繪圖裝置上**，視窗銷毀就必須跟著丟（留著會是懸空指標）
///
/// 混在一起的話只能二選一，兩種都會出事：跟著裝置丟就變成每次開視窗
/// 重新解碼一次（打字時視窗開開關關，宿主會卡到沒有回應）；不丟就是
/// 拿已銷毀裝置的資源去畫，記憶體違規。本專案兩種都踩過。
///
/// 用 Windows 內建的 WIC（PNG／JPG／BMP／GIF 都吃），不必第三方套件。
/// **要轉成 32bppPBGRA**——D2D 只認這個格式（P 是預乘 alpha），少了
/// 這一步帶透明度的 PNG 會整張畫錯。
pub fn decode_image(path: &std::path::Path) -> Result<(u32, u32, Vec<u8>)> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
        WICBitmapDitherTypeNone, WICBitmapPaletteTypeMedianCut, WICDecodeMetadataCacheOnLoad,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let decoder = factory.CreateDecoderFromFilename(
            PCWSTR(wide.as_ptr()),
            None,
            windows::Win32::Foundation::GENERIC_READ,
            WICDecodeMetadataCacheOnLoad,
        )?;
        let frame = decoder.GetFrame(0)?;
        let converter = factory.CreateFormatConverter()?;
        converter.Initialize(
            &frame,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeMedianCut,
        )?;
        let mut w = 0u32;
        let mut h = 0u32;
        converter.GetSize(&mut w, &mut h)?;
        if w == 0 || h == 0 {
            return Err(windows::core::Error::from(
                windows::Win32::Foundation::E_INVALIDARG,
            ));
        }
        let stride = w as usize * 4;
        let mut buf = vec![0u8; stride * h as usize];
        converter.CopyPixels(std::ptr::null(), stride as u32, &mut buf)?;
        Ok((w, h, buf))
    }
}

/// 建文字格式的共用實作（`Renderer` 與 `TextMeasurer` 都用）。
fn make_format(
    write: &IDWriteFactory,
    family: &str,
    size_pt: f32,
    dpi: f32,
) -> Result<IDWriteTextFormat> {
    unsafe {
        let name: Vec<u16> = if family.is_empty() {
            "Microsoft JhengHei UI\0".encode_utf16().collect()
        } else {
            family.encode_utf16().chain(Some(0)).collect()
        };
        let locale: Vec<u16> = "zh-TW\0".encode_utf16().collect();
        // 點轉像素：1 點 = 1/72 吋
        let px = size_pt * dpi / 72.0;
        let fmt = write.CreateTextFormat(
            PCWSTR(name.as_ptr()),
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            px,
            PCWSTR(locale.as_ptr()),
        )?;
        fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING)?;
        // **垂直置中交給 DirectWrite**——GDI 版要自己算字身
        // （`DT_VCENTER` 把 internal leading 算進去會偏上）。
        fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP)?;
        Ok(fmt)
    }
}

/// 量文字的共用實作。
/// 前進寬度（含尾端空白）。見 `Renderer::measure_advance`。
fn measure_advance_with(write: &IDWriteFactory, text: &str, fmt: &IDWriteTextFormat) -> f32 {
    unsafe {
        let wide: Vec<u16> = text.encode_utf16().collect();
        if wide.is_empty() {
            return 0.0;
        }
        let Ok(layout) = write.CreateTextLayout(&wide, fmt, f32::MAX / 2.0, f32::MAX / 2.0) else {
            return 0.0;
        };
        let mut m = DWRITE_TEXT_METRICS::default();
        if layout.GetMetrics(&mut m).is_err() {
            return 0.0;
        }
        m.widthIncludingTrailingWhitespace
    }
}

fn measure_with(
    write: &IDWriteFactory,
    text: &str,
    fmt: &IDWriteTextFormat,
    max_width: f32,
) -> (f32, f32) {
    unsafe {
        let wide: Vec<u16> = text.encode_utf16().collect();
        if wide.is_empty() {
            return (0.0, 0.0);
        }
        let Ok(layout) = write.CreateTextLayout(&wide, fmt, max_width.max(1.0), f32::MAX / 2.0)
        else {
            return (0.0, 0.0);
        };
        let mut m = DWRITE_TEXT_METRICS::default();
        if layout.GetMetrics(&mut m).is_err() {
            return (0.0, 0.0);
        }
        (m.width, m.height)
    }
}

/// 一個 D2D + DComp 的繪圖環境，綁在一個視窗上。
///
/// 建立很貴（約 130ms），所以**一個視窗只建一次**，之後每幀重用。
pub struct Renderer {
    dc: ID2D1DeviceContext,
    surface: IDCompositionSurface,
    comp: IDCompositionDevice,
    write: IDWriteFactory,
    /// 目前 surface 的尺寸。改變時要重建——DComp 的 surface 是固定
    /// 大小的，而候選視窗的高度會隨候選數變。
    size: (u32, u32),
    /// 這些要活著，不然合成樹會斷掉
    _visual: IDCompositionVisual,
    _target: IDCompositionTarget,
}

impl Renderer {
    /// 在一個視窗上建立繪圖環境。
    ///
    /// 視窗**必須**用 `WS_EX_NOREDIRECTIONBITMAP` 建立——沒有的話
    /// 系統會準備一張不透明的重導向點陣圖，把 DComp 畫的內容蓋掉
    /// （看起來就是全黑）。
    pub fn new(hwnd: HWND, width: u32, height: u32) -> Result<Self> {
        unsafe {
            let mut d3d: Option<ID3D11Device> = None;
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut d3d),
                None,
                None,
            )?;
            let dxgi: IDXGIDevice = d3d.ok_or_else(windows::core::Error::empty)?.cast()?;

            let factory: ID2D1Factory1 =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let d2d_device = factory.CreateDevice(&dxgi)?;
            let dc = d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;

            let comp: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;
            let target = comp.CreateTargetForHwnd(hwnd, true)?;
            let visual = comp.CreateVisual()?;
            let surface = comp.CreateSurface(
                width.max(1),
                height.max(1),
                DXGI_FORMAT_B8G8R8A8_UNORM,
                DXGI_ALPHA_MODE_PREMULTIPLIED,
            )?;
            visual.SetContent(&surface)?;
            target.SetRoot(&visual)?;
            comp.Commit()?;

            let write: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

            Ok(Self {
                dc,
                surface,
                comp,
                write,
                size: (width.max(1), height.max(1)),
                _visual: visual,
                _target: target,
            })
        }
    }

    /// 視窗大小變了就重建 surface。
    ///
    /// DComp 的 surface 尺寸是固定的——候選視窗的高度會隨候選數變，
    /// 所以每次繪製前都要確認。
    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        let (w, h) = (width.max(1), height.max(1));
        if self.size == (w, h) {
            return Ok(());
        }
        unsafe {
            let surface = self.comp.CreateSurface(
                w,
                h,
                DXGI_FORMAT_B8G8R8A8_UNORM,
                DXGI_ALPHA_MODE_PREMULTIPLIED,
            )?;
            self._visual.SetContent(&surface)?;
            self.comp.Commit()?;
            self.surface = surface;
            self.size = (w, h);
        }
        Ok(())
    }

    /// 用已解碼的像素建點陣圖。**快**——沒有讀檔也沒有解碼。
    ///
    /// 像素要是 32bppPBGRA（預乘 alpha、BGR 順序），跟
    /// [`decode_image`] 產出的一致。
    pub fn bitmap_from_pixels(&self, w: u32, h: u32, px: &[u8]) -> Result<ID2D1Bitmap1> {
        let stride = w as usize * 4;
        if w == 0 || h == 0 || px.len() < stride * h as usize {
            return Err(windows::core::Error::from(
                windows::Win32::Foundation::E_INVALIDARG,
            ));
        }
        unsafe {
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            };
            self.dc.CreateBitmap(
                windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_U {
                    width: w,
                    height: h,
                },
                Some(px.as_ptr() as *const _),
                stride as u32,
                &props,
            )
        }
    }

    /// 開始畫一幀。回傳的 `Frame` 解構時自動送出。
    ///
    /// `BeginDraw` 給的貼圖**不保證從 (0,0) 開始**——可能是一大塊
    /// 貼圖的一角，所有座標都要加上回傳的偏移（§4.10 記過）。
    pub fn begin(&self) -> Result<Frame<'_>> {
        unsafe {
            let mut offset = POINT::default();
            let tex: IDXGISurface = self.surface.BeginDraw(None, &mut offset)?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                ..Default::default()
            };
            let bitmap: ID2D1Bitmap1 = self.dc.CreateBitmapFromDxgiSurface(&tex, Some(&props))?;
            self.dc.SetTarget(&bitmap);
            self.dc.BeginDraw();
            // 全部清成透明——沒清的話上一幀的內容會留著
            self.dc.Clear(Some(&D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            }));
            Ok(Frame {
                r: self,
                ox: offset.x as f32,
                oy: offset.y as f32,
            })
        }
    }

    /// 建一個文字格式（字型 + 字級）。
    ///
    /// `family` 空字串時交給系統挑——跟 GDI 版同樣的規則。
    pub fn text_format(&self, family: &str, size_pt: f32, dpi: f32) -> Result<IDWriteTextFormat> {
        make_format(&self.write, family, size_pt, dpi)
    }

    /// 同上，但**不換行**。
    ///
    /// 預覽列用這個：它固定一行，長了就捲動顯示尾端。讓它換行的話
    /// 打字時視窗會忽高忽低，很干擾。
    pub fn text_format_nowrap(
        &self,
        family: &str,
        size_pt: f32,
        dpi: f32,
    ) -> Result<IDWriteTextFormat> {
        let fmt = self.text_format(family, size_pt, dpi)?;
        unsafe {
            fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            Ok(fmt)
        }
    }

    /// 量一段文字有多寬（像素）。**不換行**。
    ///
    /// 反白塊的位置要靠它算——跟 GDI 版的 `text_width` 對等。
    ///
    /// **不含尾端空白**（那是 DirectWrite `DWRITE_TEXT_METRICS.width` 的
    /// 語意，為了右對齊時不讓尾巴的空白撐開版面）。要排字的前進寬度
    /// 請用 `measure_advance`。
    pub fn measure(&self, text: &str, fmt: &IDWriteTextFormat) -> f32 {
        self.measure_wrapped(text, fmt, f32::MAX / 2.0).0
    }

    /// 量一段文字的**前進寬度**——含尾端空白。
    ///
    /// # 為什麼需要跟 `measure` 分開
    ///
    /// 預覽列是**逐字**排版的（框要對得準每一個字），而 `measure` 用的
    /// `DWRITE_TEXT_METRICS.width` **不含尾端空白**——單獨一個空白整個
    ///就是尾端空白，於是量到 **0**。症狀是「`footer 早安` 的空白在候選
    /// 視窗裡看不見，送出去卻是對的」。
    ///
    /// 排字要的是前進寬度，`widthIncludingTrailingWhitespace` 才是。
    pub fn measure_advance(&self, text: &str, fmt: &IDWriteTextFormat) -> f32 {
        measure_advance_with(&self.write, text, fmt)
    }

    /// 量一段文字在**限定寬度內**要佔多寬多高。
    ///
    /// 回傳 `(寬, 高)`。文字超過 `max_width` 時 DirectWrite 會自動
    /// 換行，高度就變成好幾行——候選視窗的每一列因此不再等高。
    ///
    /// **這是 GDI 版做不到的**：`GetTextExtentPoint32W` 只量單行，
    /// 要自己算換行點。DirectWrite 的 layout 直接把換行後的尺寸給你。
    pub fn measure_wrapped(
        &self,
        text: &str,
        fmt: &IDWriteTextFormat,
        max_width: f32,
    ) -> (f32, f32) {
        measure_with(&self.write, text, fmt, max_width)
    }
}

/// 一幀的繪製範圍。解構時自動 `EndDraw` + `Commit`。
///
/// 所有座標都會自動加上 `BeginDraw` 給的偏移，呼叫端用視窗座標就好。
pub struct Frame<'a> {
    r: &'a Renderer,
    ox: f32,
    oy: f32,
}

impl Frame<'_> {
    /// 把視窗座標換成貼圖座標。
    fn rect(&self, rc: Rect) -> D2D_RECT_F {
        D2D_RECT_F {
            left: self.ox + rc.left,
            top: self.oy + rc.top,
            right: self.ox + rc.right,
            bottom: self.oy + rc.bottom,
        }
    }

    /// 純色矩形。
    pub fn fill_rect(&self, rc: Rect, c: Color, alpha: f32) {
        unsafe {
            let Ok(brush) = self.r.dc.CreateSolidColorBrush(&d2d_color(c, alpha), None) else {
                return;
            };
            self.r.dc.FillRectangle(&self.rect(rc), &brush);
        }
    }

    /// 圓角矩形。**反鋸齒**——這是 GDI 做不到的。
    pub fn fill_round(&self, rc: Rect, radius: f32, c: Color, alpha: f32) {
        unsafe {
            let Ok(brush) = self.r.dc.CreateSolidColorBrush(&d2d_color(c, alpha), None) else {
                return;
            };
            let rr = D2D1_ROUNDED_RECT {
                rect: self.rect(rc),
                radiusX: radius,
                radiusY: radius,
            };
            self.r.dc.FillRoundedRectangle(&rr, &brush);
        }
    }

    /// 用一張圖填滿圓角矩形，**等比填滿、超出的裁掉**（像手機桌布）。
    ///
    /// # 為什麼用點陣筆刷而不是 `DrawBitmap`
    ///
    /// `DrawBitmap` 畫的是矩形，圓角要另外推一層裁切遮罩（`PushLayer`），
    /// 多一次離屏合成。改用 `FillRoundedRectangle` 配點陣筆刷的話，
    /// **圓角由 D2D 直接處理**，一次畫完。
    ///
    /// 縮放與位移靠筆刷的變換矩陣：取寬高比例較大的那個當縮放倍率
    /// （這樣一定填滿），再把多出來的部分置中裁掉。
    pub fn fill_round_image(&self, rc: Rect, radius: f32, img: &ID2D1Bitmap1, alpha: f32) {
        unsafe {
            let size = img.GetSize();
            if size.width <= 0.0 || size.height <= 0.0 {
                return;
            }
            let dst = self.rect(rc);
            let (dw, dh) = (dst.right - dst.left, dst.bottom - dst.top);
            if dw <= 0.0 || dh <= 0.0 {
                return;
            }
            // 等比填滿的倍率跟設定頁的預覽共用同一份，不然預覽會騙人
            let scale = ime_core::render::cover_scale(dw, dh, size.width, size.height);
            // 多出來的部分置中裁掉
            let ox = dst.left + (dw - size.width * scale) / 2.0;
            let oy = dst.top + (dh - size.height * scale) / 2.0;

            let Ok(brush) = self.r.dc.CreateBitmapBrush(img, None, None) else {
                return;
            };
            brush.SetTransform(&windows_numerics::Matrix3x2 {
                M11: scale,
                M12: 0.0,
                M21: 0.0,
                M22: scale,
                M31: ox,
                M32: oy,
            });
            brush.SetOpacity(alpha);
            // 邊緣不要重複——超出的部分本來就該被圓角裁掉
            brush.SetExtendModeX(D2D1_EXTEND_MODE_CLAMP);
            brush.SetExtendModeY(D2D1_EXTEND_MODE_CLAMP);
            self.r.dc.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: dst,
                    radiusX: radius,
                    radiusY: radius,
                },
                &brush,
            );
        }
    }

    /// 垂直漸層（上→下）的圓角矩形。兩色相同就是純色。
    pub fn fill_round_gradient(
        &self,
        rc: Rect,
        radius: f32,
        top: Color,
        bottom: Color,
        alpha: f32,
    ) {
        if top == bottom {
            self.fill_round(rc, radius, top, alpha);
            return;
        }
        unsafe {
            let stops = [
                D2D1_GRADIENT_STOP {
                    position: 0.0,
                    color: d2d_color(top, alpha),
                },
                D2D1_GRADIENT_STOP {
                    position: 1.0,
                    color: d2d_color(bottom, alpha),
                },
            ];
            // `ID2D1DeviceContext` 版要六個參數（比 RenderTarget 版多）：
            // 前後內插色彩空間、緩衝精度、延伸模式、色彩內插方式。
            let Ok(coll) = self.r.dc.CreateGradientStopCollection(
                &stops,
                D2D1_COLOR_SPACE_SRGB,
                D2D1_COLOR_SPACE_SRGB,
                D2D1_BUFFER_PRECISION_8BPC_UNORM,
                D2D1_EXTEND_MODE_CLAMP,
                D2D1_COLOR_INTERPOLATION_MODE_PREMULTIPLIED,
            ) else {
                return;
            };
            let rect = self.rect(rc);
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                startPoint: windows_numerics::Vector2 {
                    X: rect.left,
                    Y: rect.top,
                },
                endPoint: windows_numerics::Vector2 {
                    X: rect.left,
                    Y: rect.bottom,
                },
            };
            let Ok(brush) = self.r.dc.CreateLinearGradientBrush(&props, None, &coll) else {
                return;
            };
            let rr = D2D1_ROUNDED_RECT {
                rect,
                radiusX: radius,
                radiusY: radius,
            };
            self.r.dc.FillRoundedRectangle(&rr, &brush);
        }
    }

    /// **畫反白條**——三種樣式都在這裡。
    ///
    /// | 樣式 | 長什麼樣 |
    /// |---|---|
    /// | `Solid` | 實心色塊，最清楚 |
    /// | `Sheen` | 色塊 + 上緣白色高光帶（玻璃的厚度反光） |
    /// | `SheenOnly` | **只有高光與亮邊**，底色全透明 |
    ///
    /// 高光帶是「玻璃感」的來源：實體玻璃有厚度，上半部會反射
    /// 環境光。單純把顏色調淡不會有這個效果。
    /// `with_sheen` 為 `false` 時**只畫底色與亮邊，不畫上緣的光**。
    ///
    /// 預覽列那個框住文字的反白用得上——那一格很矮，加了光會糊成
    /// 一塊，只留外框比較清楚（使用者的決定）。候選列則保留光。
    pub fn fill_highlight(
        &self,
        rc: Rect,
        radius: f32,
        color: Color,
        style: ime_core::config::HighlightStyle,
        with_sheen: bool,
    ) {
        // **要不要鋪底、要不要高光、要不要亮邊，一律問 `core`**——
        // 設定頁的預覽問同一份決策，兩邊才不會走鐘。
        let paint = ime_core::render::highlight_paint(
            style,
            color.to_rgb(),
            color.to_rgb(),
            color.to_rgb(),
        );
        if let Some(bg) = paint.fill {
            self.fill_round(rc, radius, Color::from(bg), 1.0);
        }
        let draw_sheen = paint.sheen && with_sheen;
        if !draw_sheen && !paint.outline {
            return;
        }

        unsafe {
            if draw_sheen {
                // ── 上緣高光帶 ──
                //
                // 從上緣往下 42% 的白色漸層，由不透明淡出到全透明。
                let h = (rc.bottom - rc.top) * ime_core::render::SHEEN_BAND_RATIO;
                let stops = [
                    D2D1_GRADIENT_STOP {
                        position: 0.0,
                        // **補償混色的色彩空間差異**——同樣的數值在
                        // gamma 空間混出來比預覽暗（見
                        // `lighten_alpha_for_gamma_blend`）
                        color: d2d_color(
                            Color::rgb(0xFF, 0xFF, 0xFF),
                            ime_core::render::lighten_alpha_for_gamma_blend(
                                ime_core::render::SHEEN_ALPHA,
                            ),
                        ),
                    },
                    D2D1_GRADIENT_STOP {
                        position: 1.0,
                        color: d2d_color(Color::rgb(0xFF, 0xFF, 0xFF), 0.0),
                    },
                ];
                if let Ok(coll) = self.r.dc.CreateGradientStopCollection(
                    &stops,
                    D2D1_COLOR_SPACE_SRGB,
                    D2D1_COLOR_SPACE_SRGB,
                    D2D1_BUFFER_PRECISION_8BPC_UNORM,
                    D2D1_EXTEND_MODE_CLAMP,
                    D2D1_COLOR_INTERPOLATION_MODE_PREMULTIPLIED,
                ) {
                    let band = self.rect(Rect {
                        bottom: rc.top + h,
                        ..rc
                    });
                    if let Ok(brush) = self.r.dc.CreateLinearGradientBrush(
                        &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                            startPoint: windows_numerics::Vector2 {
                                X: band.left,
                                Y: band.top,
                            },
                            endPoint: windows_numerics::Vector2 {
                                X: band.left,
                                Y: band.bottom,
                            },
                        },
                        None,
                        &coll,
                    ) {
                        // **帶子畫成純長方形，不套圓角**。
                        //
                        // 套了的話四個角都會圓——而帶子只有列高的 42%，
                        // 下緣那兩個圓角會讓它看起來像漂浮的膠囊，而不是
                        // 貼在上緣的一道光。設定頁的預覽畫的就是純長方形，
                        // 兩邊要一致（使用者以預覽那種為準）。
                        self.r.dc.FillRectangle(&band, &brush);
                    }
                }
            }

            // ── 亮邊 ──
            //
            // 只有「只有高光」模式需要——沒有底色的話，
            // 邊框是唯一能標出範圍的東西。
            if paint.outline {
                // **半透明，不是壓暗的實色**。
                //
                // 查證過 egui 的 `gamma_multiply`：它連 alpha 一起乘，
                // 所以設定頁畫的其實就是「預乘的半透明白」——跟這裡的
                // `d2d_color(color, 0.75)` 是同一件事。
                //
                // 曾經誤以為那邊是「亮度乘 0.75、維持不透明」而改成實色，
                // 反而把本來一致的兩邊改歪了。
                if let Ok(edge) = self
                    .r
                    .dc
                    .CreateSolidColorBrush(&d2d_color(color, ime_core::render::OUTLINE_DIM), None)
                {
                    // **畫在內側**。D2D 預設把線畫在框線**上**，等於
                    // 一半在矩形外面——看起來比實際範圍大一圈，也比
                    // 預覽粗。往內縮半個線寬才對得上（設定頁用的是
                    // `StrokeKind::Inside`）。
                    // **線寬也要跟著縮放**。`OUTLINE_WIDTH` 是邏輯像素，
                    // 直接用的話在 125%／150% 的螢幕上會比周圍細一截。
                    // `radius` 進來時已經套過縮放，拿它跟基準值的比例
                    // 反推目前的倍率。
                    let scale = (radius
                        / (ime_core::render::fixed::CORNER_RADIUS as f32
                            / ime_core::render::HIGHLIGHT_RADIUS_DIVISOR))
                        .clamp(1.0, 4.0);
                    let width = ime_core::render::OUTLINE_WIDTH * scale;
                    let half = width / 2.0;
                    let r0 = self.rect(rc);
                    self.r.dc.DrawRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F {
                                left: r0.left + half,
                                top: r0.top + half,
                                right: r0.right - half,
                                bottom: r0.bottom - half,
                            },
                            // 半徑也要跟著縮，不然轉角會跟邊線對不齊
                            radiusX: (radius - half).max(0.0),
                            radiusY: (radius - half).max(0.0),
                        },
                        &edge,
                        width,
                        None,
                    );
                }
            }
        }
    }

    /// 畫一段文字，**四周描一圈深色細邊**。
    ///
    /// 背景是圖片時字很容易融進去（深色圖上的深色字、亮處的白字）。
    /// 描邊四面都擋得住，比陰影可靠——陰影只擋一個方向。
    ///
    /// 做法是把同一段字**先往八個方向各畫一次深色**，再把正常的字畫在
    /// 上面。八個方向（含斜角）才不會在轉角處留缺口；只畫上下左右的話
    /// 斜邊會露出來。
    ///
    /// 代價是同一段字畫九次。字級小、字數少（候選視窗最多十幾個字），
    /// 實務上感覺不出來；`alpha` 為 0 時直接跳過描邊那八次。
    #[allow(clippy::too_many_arguments)]
    pub fn draw_text_outlined(
        &self,
        text: &str,
        rc: Rect,
        fmt: &IDWriteTextFormat,
        c: Color,
        alpha: f32,
        outline: Color,
        outline_alpha: f32,
        width: f32,
    ) {
        if outline_alpha > 0.0 && width > 0.0 {
            // 八個方向。用 0.7 而不是 1.0 讓斜角的距離跟正向差不多，
            // 不然斜角會比較粗
            const DIRS: [(f32, f32); 8] = [
                (-1.0, 0.0),
                (1.0, 0.0),
                (0.0, -1.0),
                (0.0, 1.0),
                (-0.7, -0.7),
                (0.7, -0.7),
                (-0.7, 0.7),
                (0.7, 0.7),
            ];
            for (dx, dy) in DIRS {
                self.draw_text(
                    text,
                    Rect::new(
                        rc.left + dx * width,
                        rc.top + dy * width,
                        rc.right + dx * width,
                        rc.bottom + dy * width,
                    ),
                    fmt,
                    outline,
                    outline_alpha,
                );
            }
        }
        self.draw_text(text, rc, fmt, c, alpha);
    }

    /// 畫一段文字。垂直置中由 `text_format` 的段落對齊處理。
    pub fn draw_text(&self, text: &str, rc: Rect, fmt: &IDWriteTextFormat, c: Color, alpha: f32) {
        if text.is_empty() {
            return;
        }
        unsafe {
            let Ok(brush) = self.r.dc.CreateSolidColorBrush(&d2d_color(c, alpha), None) else {
                return;
            };
            let wide: Vec<u16> = text.encode_utf16().collect();
            self.r.dc.DrawText(
                &wide,
                fmt,
                &self.rect(rc),
                &brush as &ID2D1Brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    /// 只有**上緣**圓角的矩形（下緣是直角）。
    ///
    /// 預覽列在視窗最上面、底下還有候選清單時要用這個。畫成直角的話
    /// 那兩個角會填滿視窗的圓角缺口，看起來像預覽列比下面的面板「寬
    /// 出去一截」——實際上寬度一樣，是轉角形狀不一致。
    ///
    /// 做法是把圓角矩形的**下緣推到 `rc` 之外**，下面那兩個圓角就落在
    /// 看不見的地方，剩上緣是圓的。
    ///
    /// **一定要先裁切**。推出去的那一段是真的會被畫出來的——少了裁切，
    /// 它會往下蓋掉兩倍半徑那麼高的東西（提示列就是這樣被蓋住的）。
    /// 這是「把東西移到視野外」和「不要畫」的差別。
    ///
    /// 用 `PushAxisAlignedClip` 而不是 `PushLayer`：後者要 geometry 物件
    /// 與 `Matrix3x2`，為了一個圓角引入那些複雜度不划算。
    ///
    /// 副作用是漸層被拉長了兩倍半徑（可視範圍內只走到約八成），對這種
    /// 本來就很淡的底色漸層看不出來，換來的是沒有接縫。
    pub fn fill_top_round_gradient(
        &self,
        rc: Rect,
        radius: f32,
        top: Color,
        bottom: Color,
        alpha: f32,
    ) {
        unsafe {
            self.r.dc.PushAxisAlignedClip(
                &self.rect(rc),
                windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_ALIASED,
            );
        }
        let pushed = Rect {
            bottom: rc.bottom + radius * 2.0,
            ..rc
        };
        self.fill_round_gradient(pushed, radius, top, bottom, alpha);
        unsafe {
            self.r.dc.PopAxisAlignedClip();
        }
    }
}

impl Drop for Frame<'_> {
    fn drop(&mut self) {
        unsafe {
            let _ = self.r.dc.EndDraw(None, None);
            self.r.dc.SetTarget(None);
            let _ = self.r.surface.EndDraw();
            let _ = self.r.comp.Commit();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::*;

    unsafe extern "system" fn wp(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        unsafe { DefWindowProcW(h, m, w, l) }
    }

    /// **這個測試要真的建立 D3D 裝置**，在沒有顯示卡的 CI 上會失敗。
    /// 本機開發時它證明整條 D3D→DXGI→D2D→DComp 的鏈路接得起來。
    #[test]
    #[ignore = "需要顯示裝置，用 cargo test -- --ignored 跑"]
    fn 建得起來也畫得出東西() {
        unsafe {
            let class = windows::core::w!("D2dSmokeTest");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wp),
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP,
                class,
                windows::core::w!(""),
                WS_POPUP,
                0,
                0,
                200,
                300,
                None,
                None,
                None,
                None,
            )
            .expect("建視窗");

            let r = Renderer::new(hwnd, 200, 300).expect("建 renderer");
            let fmt = r.text_format("", 12.0, 96.0).expect("建字型");

            // 量字寬要回合理的值
            let w = r.measure("你好", &fmt);
            assert!(w > 0.0, "量出來的寬度該大於 0：{w}");

            // **空白的兩種量法要分得開**。`measure` 不含尾端空白，所以
            // 單獨一個空白量到 0；預覽列逐字排版靠的是前進寬度，
            // 用錯就會「候選視窗裡看不到空白，送出去卻是對的」。
            assert_eq!(r.measure(" ", &fmt), 0.0, "measure 不含尾端空白");
            assert!(
                r.measure_advance(" ", &fmt) > 0.0,
                "measure_advance 要含尾端空白"
            );

            // 畫一幀——畫得出來就不會 panic
            {
                let f = r.begin().expect("開始畫");
                f.fill_round_gradient(
                    Rect::new(0.0, 0.0, 200.0, 300.0),
                    7.0,
                    Color::rgb(0xFB, 0xFB, 0xFB),
                    Color::rgb(0xF0, 0xF0, 0xF0),
                    1.0,
                );
                f.draw_text(
                    "你好",
                    Rect::new(8.0, 8.0, 190.0, 36.0),
                    &fmt,
                    Color::rgb(0, 0, 0),
                    1.0,
                );
            } // Frame 解構時送出

            let _ = DestroyWindow(hwnd);
        }
    }
}
