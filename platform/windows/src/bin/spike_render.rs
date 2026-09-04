//! Spike：驗證 Direct2D + DirectComposition 能不能取代 GDI 畫候選視窗。
//!
//! **這是可行性測試，不是產品程式碼。** 不接輸入法，就是一個獨立視窗，
//! 跑起來直接看效果。驗過再決定要不要搬進 `candidate_window.rs`。
//!
//! 用法：cargo run -p ime-tip-windows --bin spike_render
//!
//! # 要驗證的三件事
//!
//! 1. `windows` crate 0.62 的 D2D／DComp 綁定好不好用（型別對不對得上）
//! 2. 半透明與**平滑圓角**——GDI 的 `SetWindowRgn` 圓角是鋸齒的
//! 3. DirectWrite 的中文清晰度（跟 GDI 的 ClearType 不一樣）
//!
//! # 這一套的骨架
//!
//! ```text
//! D3D11 Device ──→ DXGI Device ──→ D2D Device ──→ D2D DeviceContext
//!                       │                              ↑
//!                       └→ DComp Device ──→ Target ──→ Visual ──→ Surface
//! ```
//!
//! 比 GDI 囉唆得多，但換來的是合成器層級的透明與反鋸齒。

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectComposition::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::System::Com::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const W: i32 = 320;
const H: i32 = 380;

fn main() -> Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;

        let hinst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)?;
        let class = w!("SpikeRenderWindow");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };
        RegisterClassW(&wc);

        // **WS_EX_NOREDIRECTIONBITMAP 是關鍵**。
        //
        // 沒有它的話，系統會替視窗準備一張不透明的重導向點陣圖，
        // DComp 畫的透明內容會被那張圖蓋掉——看起來就是黑底。
        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST,
            class,
            w!("D2D + DComp spike"),
            WS_POPUP,
            300,
            300,
            W,
            H,
            None,
            None,
            Some(hinst.into()),
            None,
        )?;

        let ctx = build(hwnd)?;
        draw(&ctx)?;
        let _ = ShowWindow(hwnd, SW_SHOW);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        Ok(())
    }
}

/// 建好整條管線之後要留著的東西。
struct Ctx {
    dc: ID2D1DeviceContext,
    surface: IDCompositionSurface,
    dcomp: IDCompositionDevice,
    dwrite: IDWriteFactory,
    /// 這些要活著，管線才不會被釋放
    #[allow(dead_code)]
    keep: (
        ID3D11Device,
        ID2D1Device,
        IDCompositionTarget,
        IDCompositionVisual,
    ),
}

unsafe fn build(hwnd: HWND) -> Result<Ctx> {
    unsafe {
        // ── D3D11 裝置 ──
        //
        // 要 BGRA_SUPPORT，D2D 才能接上去。
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
        let d3d = d3d.unwrap();
        let dxgi: IDXGIDevice = d3d.cast()?;

        // ── D2D 裝置與繪圖脈絡 ──
        let factory: ID2D1Factory1 = D2D1CreateFactory(
            D2D1_FACTORY_TYPE_SINGLE_THREADED,
            Some(&D2D1_FACTORY_OPTIONS::default()),
        )?;
        let d2d = factory.CreateDevice(&dxgi)?;
        let dc = d2d.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;

        // ── 合成器 ──
        let dcomp: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;
        let target = dcomp.CreateTargetForHwnd(hwnd, true)?;
        let visual = dcomp.CreateVisual()?;
        // PREMULTIPLIED：alpha 已經乘進顏色裡，這是合成器要的格式
        let surface = dcomp.CreateSurface(
            W as u32,
            H as u32,
            DXGI_FORMAT_B8G8R8A8_UNORM,
            DXGI_ALPHA_MODE_PREMULTIPLIED,
        )?;
        visual.SetContent(&surface)?;
        target.SetRoot(&visual)?;

        let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

        Ok(Ctx {
            dc,
            surface,
            dcomp,
            dwrite,
            keep: (d3d, d2d, target, visual),
        })
    }
}

unsafe fn draw(ctx: &Ctx) -> Result<()> {
    unsafe {
        // BeginDraw 回傳這次要畫的貼圖與它在貼圖裡的偏移——
        // 合成器可能給你一塊大貼圖的一角，不能假設從 (0,0) 開始。
        let mut offset = POINT::default();
        let tex: IDXGISurface = ctx.surface.BeginDraw(None, &mut offset)?;
        let bmp = ctx.dc.CreateBitmapFromDxgiSurface(
            &tex,
            Some(&D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                ..Default::default()
            }),
        )?;
        ctx.dc.SetTarget(&bmp);
        ctx.dc.BeginDraw();
        // 合成器給的貼圖可能是一塊大貼圖的一角，所有座標都要加上偏移。
        //
        // 正規做法是 `SetTransform(Matrix3x2::translation(..))`，但那個型別
        // 在另一個 crate（windows-numerics）；spike 階段不為此加依賴，
        // 直接把偏移加進座標裡，效果一樣。
        let (ox, oy) = (offset.x as f32, offset.y as f32);

        // 全透明清空——這是 GDI 做不到的第一件事
        ctx.dc.Clear(Some(&D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        }));

        // **平滑圓角**：D2D 的 FillRoundedRectangle 是反鋸齒的。
        // GDI 的 SetWindowRgn 裁出來的圓角有鋸齒，這是第二件事。
        let bg = ctx
            .dc
            .CreateSolidColorBrush(&color(0.98, 0.98, 0.98, 0.88), None)?;
        let border = ctx
            .dc
            .CreateSolidColorBrush(&color(0.0, 0.0, 0.0, 0.10), None)?;
        let rounded = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: ox + 0.5,
                top: oy + 0.5,
                right: ox + W as f32 - 0.5,
                bottom: oy + H as f32 - 0.5,
            },
            radiusX: 8.0,
            radiusY: 8.0,
        };
        ctx.dc.FillRoundedRectangle(&rounded, &bg);
        ctx.dc.DrawRoundedRectangle(&rounded, &border, 1.0, None);

        // ── 文字：DirectWrite ──
        let text_fmt = ctx.dwrite.CreateTextFormat(
            w!("Microsoft JhengHei UI"),
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            16.0,
            w!("zh-TW"),
        )?;
        let index_fmt = ctx.dwrite.CreateTextFormat(
            w!("Microsoft JhengHei UI"),
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            12.0,
            w!("zh-TW"),
        )?;

        let text_brush = ctx
            .dc
            .CreateSolidColorBrush(&color(0.10, 0.10, 0.10, 1.0), None)?;
        let index_brush = ctx
            .dc
            .CreateSolidColorBrush(&color(0.56, 0.56, 0.56, 1.0), None)?;
        let hl_brush = ctx
            .dc
            .CreateSolidColorBrush(&color(0.0, 0.47, 0.83, 1.0), None)?;
        let hl_text = ctx
            .dc
            .CreateSolidColorBrush(&color(1.0, 1.0, 1.0, 1.0), None)?;

        // 預覽列
        draw_text(
            ctx,
            "▶ 【你】好世界",
            &text_fmt,
            &hl_brush,
            ox + 12.0,
            oy + 8.0,
        )?;

        let cands = ["你", "擬", "妳", "泥", "尼", "膩", "逆", "匿", "溺", "暱"];
        for (i, c) in cands.iter().enumerate() {
            let y = oy + 44.0 + i as f32 * 30.0;
            let hot = i == 1;
            if hot {
                // 反白條也是圓角的——GDI 版是方的
                ctx.dc.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: ox + 6.0,
                            top: y - 2.0,
                            right: ox + W as f32 - 6.0,
                            bottom: y + 26.0,
                        },
                        radiusX: 4.0,
                        radiusY: 4.0,
                    },
                    &hl_brush,
                );
            }
            let (nb, tb) = if hot {
                (&hl_text, &hl_text)
            } else {
                (&index_brush, &text_brush)
            };
            draw_text(
                ctx,
                &format!("{}", i + 1),
                &index_fmt,
                nb,
                ox + 16.0,
                y + 3.0,
            )?;
            draw_text(ctx, c, &text_fmt, tb, ox + 44.0, y)?;
        }

        ctx.dc.EndDraw(None, None)?;
        ctx.dc.SetTarget(None);
        ctx.surface.EndDraw()?;
        ctx.dcomp.Commit()?;
        Ok(())
    }
}

unsafe fn draw_text(
    ctx: &Ctx,
    s: &str,
    fmt: &IDWriteTextFormat,
    brush: &ID2D1SolidColorBrush,
    x: f32,
    y: f32,
) -> Result<()> {
    unsafe {
        let w: Vec<u16> = s.encode_utf16().collect();
        ctx.dc.DrawText(
            &w,
            fmt,
            &D2D_RECT_F {
                left: x,
                top: y,
                right: x + W as f32,
                bottom: y + 40.0,
            },
            brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
            DWRITE_MEASURING_MODE_NATURAL,
        );
        Ok(())
    }
}

/// PREMULTIPLIED：顏色要先乘上 alpha，合成器才不會算錯。
fn color(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: r * a,
        g: g * a,
        b: b * a,
        a,
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_KEYDOWN | WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
