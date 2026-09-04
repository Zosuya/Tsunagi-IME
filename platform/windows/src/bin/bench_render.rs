//! 繪圖效能對照：GDI vs Direct2D + DirectComposition。
//!
//! **換繪圖層之前先量**——輸入法每按一鍵都要重畫候選視窗，
//! 開發文件 §2.1 Phase 5 訂的目標是「按鍵到候選更新 < 16ms（p99）」。
//! 那 16ms 裡繪圖只是其中一段，所以繪圖本身要遠低於這個數。
//!
//! 用法：cargo run --release -p ime-tip-windows --bin bench_render
//!
//! # 量什麼
//!
//! **一次完整重畫**——那是使用者實際感受到的延遲：
//!
//! | | GDI | D2D |
//! |---|---|---|
//! | 建立畫布 | `CreateCompatibleDC` + `CreateCompatibleBitmap` | `BeginDraw` 拿貼圖 |
//! | 畫 | `FillRect` / `DrawTextW` × N | `FillRectangle` / `DrawTextLayout` × N |
//! | 送出 | `BitBlt` 到螢幕 DC | `EndDraw` + `Commit` |
//!
//! 兩邊都畫同樣的東西：底色、預覽列、10 列候選字、反白條。
//!
//! # 為什麼要分開量「初始化」與「每幀」
//!
//! D2D 的初始化（D3D device → DXGI → D2D → DComp）比 GDI 貴很多，
//! 但那**只做一次**。每幀的成本才是打字時的延遲。

use std::time::{Duration, Instant};
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
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::*;
use windows::Win32::UI::WindowsAndMessaging::*;

unsafe extern "system" fn bench_wndproc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(h, m, w, l) }
}

/// 候選視窗的典型尺寸（10 列候選 + 預覽列）。
const W: i32 = 220;
const H: i32 = 320;
/// 每輪畫幾幀。
const FRAMES: usize = 300;

/// 要畫的內容——跟實際的候選視窗一樣。
const PREVIEW: &str = "你好世界";
const CANDS: [&str; 10] = ["你", "妳", "尼", "泥", "擬", "膩", "逆", "匿", "溺", "暱"];

fn main() -> Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        println!("繪圖效能對照（{W}×{H}，每輪 {FRAMES} 幀）\n");

        let gdi = bench_gdi()?;
        report("GDI（現行）", &gdi);

        let d2d = bench_d2d()?;
        report("Direct2D + DComp", &d2d);

        println!("\n目標：< 16ms（p99），那是「按鍵到候選更新」的總預算，");
        println!("繪圖只是其中一段，所以要遠低於這個數。");
        Ok(())
    }
}

/// 一輪測量的結果。
struct Bench {
    /// 初始化花多久（只做一次）
    init: Duration,
    /// 每一幀的時間
    frames: Vec<Duration>,
}

fn report(name: &str, b: &Bench) {
    let mut v: Vec<f64> = b.frames.iter().map(|d| d.as_secs_f64() * 1000.0).collect();
    v.sort_by(|a, c| a.partial_cmp(c).unwrap());
    let pick = |p: f64| v[((v.len() as f64 - 1.0) * p) as usize];
    let avg: f64 = v.iter().sum::<f64>() / v.len() as f64;
    println!("{name}");
    println!(
        "  初始化  {:.2} ms（只做一次）",
        b.init.as_secs_f64() * 1000.0
    );
    println!(
        "  每幀    平均 {avg:.3} ms │ 中位 {:.3} │ p95 {:.3} │ p99 {:.3} │ 最差 {:.3}",
        pick(0.5),
        pick(0.95),
        pick(0.99),
        v[v.len() - 1]
    );
}

// ───────────────────────── GDI ─────────────────────────

unsafe fn bench_gdi() -> Result<Bench> {
    unsafe {
        let t0 = Instant::now();
        let screen = GetDC(None);
        let font = CreateFontW(
            -16,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            Default::default(),
            CLEARTYPE_QUALITY,
            (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            w!(""),
        );
        let init = t0.elapsed();

        let mut frames = Vec::with_capacity(FRAMES);
        for _ in 0..FRAMES {
            let t = Instant::now();
            // **每幀都重建畫布**——這正是現行 `paint()` 的做法
            let mem = CreateCompatibleDC(Some(screen));
            let bmp = CreateCompatibleBitmap(screen, W, H);
            let old_bmp = SelectObject(mem, bmp.into());
            let old_font = SelectObject(mem, font.into());

            // 底
            let rc = RECT {
                left: 0,
                top: 0,
                right: W,
                bottom: H,
            };
            let bg = CreateSolidBrush(COLORREF(0x00FBFBFB));
            FillRect(mem, &rc, bg);
            let _ = DeleteObject(bg.into());

            // 預覽列
            let prow = RECT {
                left: 0,
                top: 0,
                right: W,
                bottom: 44,
            };
            let pb = CreateSolidBrush(COLORREF(0x00FBF7F2));
            FillRect(mem, &prow, pb);
            let _ = DeleteObject(pb.into());
            SetBkMode(mem, TRANSPARENT);
            SetTextColor(mem, COLORREF(0x00A86000));
            let mut line: Vec<u16> = PREVIEW.encode_utf16().chain(Some(0)).collect();
            let mut r = RECT {
                left: 8,
                top: 8,
                right: W - 8,
                bottom: 36,
            };
            DrawTextW(mem, &mut line, &mut r, DT_LEFT | DT_VCENTER | DT_SINGLELINE);

            // 10 列候選 + 反白
            for (i, c) in CANDS.iter().enumerate() {
                let top = 44 + i as i32 * 28;
                let row = RECT {
                    left: 0,
                    top,
                    right: W,
                    bottom: top + 28,
                };
                if i == 2 {
                    let hb = CreateSolidBrush(COLORREF(0x00D47800));
                    FillRect(mem, &row, hb);
                    let _ = DeleteObject(hb.into());
                    SetTextColor(mem, COLORREF(0x00FFFFFF));
                } else {
                    SetTextColor(mem, COLORREF(0x001A1A1A));
                }
                let text = format!("{} {}", i + 1, c);
                let mut l: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
                let mut rr = RECT {
                    left: 8,
                    top,
                    right: W - 8,
                    bottom: top + 28,
                };
                DrawTextW(mem, &mut l, &mut rr, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
                // 量字寬——實際的 `paint` 每列都會量一次
                let mut size = SIZE::default();
                let wide: Vec<u16> = text.encode_utf16().collect();
                let _ = GetTextExtentPoint32W(mem, &wide, &mut size);
            }

            // 送出
            let _ = BitBlt(screen, 0, 0, W, H, Some(mem), 0, 0, SRCCOPY);
            SelectObject(mem, old_font);
            SelectObject(mem, old_bmp);
            let _ = DeleteObject(bmp.into());
            let _ = DeleteDC(mem);
            frames.push(t.elapsed());
        }
        let _ = DeleteObject(font.into());
        ReleaseDC(None, screen);
        Ok(Bench { init, frames })
    }
}

// ───────────────────── Direct2D + DComp ─────────────────────

struct D2dCtx {
    dc: ID2D1DeviceContext,
    surface: IDCompositionSurface,
    device: IDCompositionDevice,
    write: IDWriteFactory,
    format: IDWriteTextFormat,
    _visual: IDCompositionVisual,
    _target: IDCompositionTarget,
    _hwnd: HWND,
}

unsafe fn bench_d2d() -> Result<Bench> {
    unsafe {
        let t0 = Instant::now();
        let ctx = init_d2d()?;
        let init = t0.elapsed();

        let mut frames = Vec::with_capacity(FRAMES);
        for _ in 0..FRAMES {
            let t = Instant::now();
            draw_frame(&ctx)?;
            frames.push(t.elapsed());
        }
        Ok(Bench { init, frames })
    }
}

unsafe fn init_d2d() -> Result<D2dCtx> {
    unsafe {
        let hinst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)?;
        let class = w!("BenchRenderWindow");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(bench_wndproc),
            hInstance: hinst.into(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc);
        // 不顯示——只是要一個 DComp target 掛的地方
        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP,
            class,
            w!("bench"),
            WS_POPUP,
            0,
            0,
            W,
            H,
            None,
            None,
            Some(hinst.into()),
            None,
        )?;

        // D3D → DXGI → D2D → DComp
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
        let dxgi: IDXGIDevice = d3d.unwrap().cast()?;
        let factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let d2d_device = factory.CreateDevice(&dxgi)?;
        let dc = d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;

        let device: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;
        let target = device.CreateTargetForHwnd(hwnd, true)?;
        let visual = device.CreateVisual()?;
        let surface = device.CreateSurface(
            W as u32,
            H as u32,
            DXGI_FORMAT_B8G8R8A8_UNORM,
            DXGI_ALPHA_MODE_PREMULTIPLIED,
        )?;
        visual.SetContent(&surface)?;
        target.SetRoot(&visual)?;
        device.Commit()?;

        let write: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let format = write.CreateTextFormat(
            w!("Microsoft JhengHei UI"),
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            16.0,
            w!("zh-TW"),
        )?;

        Ok(D2dCtx {
            dc,
            surface,
            device,
            write,
            format,
            _visual: visual,
            _target: target,
            _hwnd: hwnd,
        })
    }
}

unsafe fn draw_frame(ctx: &D2dCtx) -> Result<()> {
    unsafe {
        let mut offset = POINT::default();
        let tex: IDXGISurface = ctx.surface.BeginDraw(None, &mut offset)?;
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
        let bitmap = ctx.dc.CreateBitmapFromDxgiSurface(&tex, Some(&props))?;
        ctx.dc.SetTarget(&bitmap);
        ctx.dc.BeginDraw();

        // `BeginDraw` 給的貼圖不保證從 (0,0) 開始，所有座標要加偏移
        let (ox, oy) = (offset.x as f32, offset.y as f32);
        let rect = |l: f32, t: f32, r: f32, b: f32| D2D_RECT_F {
            left: ox + l,
            top: oy + t,
            right: ox + r,
            bottom: oy + b,
        };

        // 底（圓角，反鋸齒——這正是 GDI 做不到的）
        let bg = ctx
            .dc
            .CreateSolidColorBrush(&color(0.98, 0.98, 0.98), None)?;
        let rr = D2D1_ROUNDED_RECT {
            rect: rect(0.0, 0.0, W as f32, H as f32),
            radiusX: 7.0,
            radiusY: 7.0,
        };
        ctx.dc.FillRoundedRectangle(&rr, &bg);

        // 預覽列
        let pb = ctx
            .dc
            .CreateSolidColorBrush(&color(0.95, 0.97, 0.99), None)?;
        ctx.dc.FillRectangle(&rect(0.0, 0.0, W as f32, 44.0), &pb);
        let pt = ctx
            .dc
            .CreateSolidColorBrush(&color(0.0, 0.38, 0.66), None)?;
        draw_text(ctx, PREVIEW, rect(8.0, 8.0, W as f32 - 8.0, 40.0), &pt)?;

        // 10 列候選 + 反白
        let hl = ctx
            .dc
            .CreateSolidColorBrush(&color(0.0, 0.47, 0.83), None)?;
        let fg = ctx.dc.CreateSolidColorBrush(&color(0.1, 0.1, 0.1), None)?;
        let hf = ctx.dc.CreateSolidColorBrush(&color(1.0, 1.0, 1.0), None)?;
        for (i, c) in CANDS.iter().enumerate() {
            let top = 44.0 + i as f32 * 28.0;
            if i == 2 {
                let r = D2D1_ROUNDED_RECT {
                    rect: rect(2.0, top, W as f32 - 2.0, top + 28.0),
                    radiusX: 4.0,
                    radiusY: 4.0,
                };
                ctx.dc.FillRoundedRectangle(&r, &hl);
            }
            let brush = if i == 2 { &hf } else { &fg };
            let text = format!("{} {}", i + 1, c);
            draw_text(
                ctx,
                &text,
                rect(8.0, top, W as f32 - 8.0, top + 28.0),
                brush,
            )?;
        }

        ctx.dc.EndDraw(None, None)?;
        ctx.dc.SetTarget(None);
        ctx.surface.EndDraw()?;
        ctx.device.Commit()?;
        Ok(())
    }
}

/// 畫一段文字。**每次都建 layout**——那跟 GDI 版每列都量字寬對等。
unsafe fn draw_text(
    ctx: &D2dCtx,
    text: &str,
    r: D2D_RECT_F,
    brush: &ID2D1SolidColorBrush,
) -> Result<()> {
    unsafe {
        let wide: Vec<u16> = text.encode_utf16().collect();
        // **先量再畫**——實際的候選視窗要靠字寬算反白塊的位置，
        // 那個成本要算進來才公平（GDI 版也每列量一次）。
        //
        // `DrawTextLayout` 的原點型別在 `windows-numerics`（另一個
        // crate），跟開發文件 §4.10 記過的 `Matrix3x2` 是同一個坑。
        // 這裡改用 `DrawText`——它吃 `D2D_RECT_F`，不必多一個依賴。
        let layout =
            ctx.write
                .CreateTextLayout(&wide, &ctx.format, r.right - r.left, r.bottom - r.top)?;
        let mut metrics = DWRITE_TEXT_METRICS::default();
        layout.GetMetrics(&mut metrics)?;
        ctx.dc.DrawText(
            &wide,
            &ctx.format,
            &r,
            brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
            DWRITE_MEASURING_MODE_NATURAL,
        );
        Ok(())
    }
}

fn color(r: f32, g: f32, b: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a: 1.0 }
}
