//! 反白條的幾種玻璃感示範。
//!
//! **這是給人看的示範，不是產品程式碼。** 開一個視窗，把同一份候選
//! 清單用不同的反白樣式各畫一次，直接比較。
//!
//! 用法：cargo run --release -p ime-tip-windows --bin demo_glass
//! 按 Esc 或關視窗結束。

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
use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows::Win32::UI::WindowsAndMessaging::*;

const W: i32 = 1180; // 六格 × 180 + 間距 + 邊界
const H: i32 = 420;
/// 每一格示範的寬度
const CELL_W: f32 = 180.0;
const CELL_GAP: f32 = 15.0;
/// 動畫計時器
const ANIM_TIMER: usize = 1;
/// 每幀間隔（毫秒）。10ms ≈ 100fps
const ANIM_MS: u32 = 10;
/// 反白從一列滑到下一列要多久
const SLIDE_MS: f32 = 110.0;
/// 滑完之後停多久才走下一格
const HOLD_MS: f32 = 450.0;

/// 六種反白樣式。
const STYLES: [(&str, Style); 6] = [
    ("① 現在（實心）", Style::Solid),
    ("② 半透明 55%", Style::Alpha(0.55)),
    ("③ 上緣高光帶", Style::TopSheen),
    ("④ 玻璃（透＋高光＋邊）", Style::Glass),
    ("⑤ 只有高光（白字）", Style::SheenOnly { dark_text: false }),
    ("⑥ 只有高光（原色字）", Style::SheenOnly { dark_text: true }),
];

/// 留著當紀錄——正式版只採用了三種（實心／高光帶／只有高光），
/// 其餘是比較過程中被淘汰的。
#[derive(Clone, Copy)]
#[allow(dead_code)]
enum Style {
    /// 實心色塊——目前的作法
    Solid,
    /// 半透明，下面的漸層透出來
    Alpha(f32),
    /// 上亮下暗的漸層，像一片玻璃
    Glossy,
    /// 同上再加一圈**明顯的**亮邊
    GlossyEdge,
    /// 上緣壓一條白色高光帶——玻璃厚度的反光
    TopSheen,
    /// 全套：半透明 + 高光帶 + 亮邊
    Glass,
    /// **完全透明，只留高光帶與亮邊**——藍色整個不見，
    /// 只剩一道光在移動。`dark_text` 決定字要不要維持原色
    /// （全透明時白字會看不見）。
    SheenOnly { dark_text: bool },
}

/// 反白現在該畫在第幾列（可以是小數）。
///
/// 用「從啟動到現在過了多久」算，不必存狀態——示範程式夠用。
/// 一輪＝滑動 + 停留，跑完就往下一列。
fn current_row(elapsed_ms: f32) -> f32 {
    let cycle = SLIDE_MS + HOLD_MS;
    let n = 10.0; // 十列，繞圈
    let idx = (elapsed_ms / cycle) % n;
    let whole = idx.floor();
    let t = ((elapsed_ms % cycle) / SLIDE_MS).clamp(0.0, 1.0);
    // ease-out：一開始快、接近終點慢下來（跟正式版同一條曲線）
    let eased = 1.0 - (1.0 - t) * (1.0 - t);
    (whole + eased) % n
}

fn main() -> Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let hinst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)?;
        let class = w!("GlassDemoWindow");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };
        RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP,
            class,
            w!("反白樣式比較 — 按 Esc 關閉"),
            WS_OVERLAPPEDWINDOW,
            120,
            120,
            W,
            H,
            None,
            None,
            Some(hinst.into()),
            None,
        )?;

        let ctx = init(hwnd)?;
        let _ = ShowWindow(hwnd, SW_SHOW);
        // **計時器驅動動畫**——跟正式版的作法一樣
        SetTimer(Some(hwnd), ANIM_TIMER, ANIM_MS, None);
        let start = std::time::Instant::now();

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if msg.message == WM_TIMER {
                let ms = start.elapsed().as_secs_f32() * 1000.0;
                let _ = draw(&ctx, current_row(ms));
            }
        }
        Ok(())
    }
}

unsafe extern "system" fn wndproc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        match m {
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            WM_KEYDOWN if w.0 == VK_ESCAPE.0 as usize => {
                let _ = DestroyWindow(h);
                LRESULT(0)
            }
            _ => DefWindowProcW(h, m, w, l),
        }
    }
}

struct Ctx {
    dc: ID2D1DeviceContext,
    surface: IDCompositionSurface,
    comp: IDCompositionDevice,
    fmt: IDWriteTextFormat,
    small: IDWriteTextFormat,
    _v: IDCompositionVisual,
    _t: IDCompositionTarget,
}

unsafe fn init(hwnd: HWND) -> Result<Ctx> {
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
        let dxgi: IDXGIDevice = d3d.unwrap().cast()?;
        let factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let dev = factory.CreateDevice(&dxgi)?;
        let dc = dev.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;

        let comp: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;
        let t = comp.CreateTargetForHwnd(hwnd, true)?;
        let v = comp.CreateVisual()?;
        let surface = comp.CreateSurface(
            W as u32,
            H as u32,
            DXGI_FORMAT_B8G8R8A8_UNORM,
            DXGI_ALPHA_MODE_PREMULTIPLIED,
        )?;
        v.SetContent(&surface)?;
        t.SetRoot(&v)?;
        comp.Commit()?;

        let write: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let mk = |px: f32| -> Result<IDWriteTextFormat> {
            let f = write.CreateTextFormat(
                w!("Microsoft JhengHei UI"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                px,
                w!("zh-TW"),
            )?;
            f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            Ok(f)
        };
        Ok(Ctx {
            dc,
            surface,
            comp,
            fmt: mk(15.0)?,
            small: mk(12.0)?,
            _v: v,
            _t: t,
        })
    }
}

unsafe fn draw(ctx: &Ctx, row: f32) -> Result<()> {
    unsafe {
        let mut off = POINT::default();
        let tex: IDXGISurface = ctx.surface.BeginDraw(None, &mut off)?;
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
        let bmp = ctx.dc.CreateBitmapFromDxgiSurface(&tex, Some(&props))?;
        ctx.dc.SetTarget(&bmp);
        ctx.dc.BeginDraw();
        ctx.dc.Clear(Some(&rgba(0.10, 0.11, 0.13, 1.0)));

        let (ox, oy) = (off.x as f32, off.y as f32);
        let r = |l: f32, t: f32, rr: f32, b: f32| D2D_RECT_F {
            left: ox + l,
            top: oy + t,
            right: ox + rr,
            bottom: oy + b,
        };

        // 五格並排，每格一種樣式
        for (i, (name, style)) in STYLES.iter().enumerate() {
            let x = 12.0 + i as f32 * (CELL_W + CELL_GAP);
            draw_one(ctx, &r, x, 12.0, name, *style, row)?;
        }

        ctx.dc.EndDraw(None, None)?;
        ctx.dc.SetTarget(None);
        ctx.surface.EndDraw()?;
        ctx.comp.Commit()?;
        Ok(())
    }
}

/// 畫一格：標題 + 一個假的候選視窗。
unsafe fn draw_one(
    ctx: &Ctx,
    r: &dyn Fn(f32, f32, f32, f32) -> D2D_RECT_F,
    x: f32,
    y: f32,
    name: &str,
    style: Style,
    row: f32,
) -> Result<()> {
    unsafe {
        let label = ctx
            .dc
            .CreateSolidColorBrush(&rgba(0.7, 0.72, 0.75, 1.0), None)?;
        let wide: Vec<u16> = name.encode_utf16().collect();
        ctx.dc.DrawText(
            &wide,
            &ctx.small,
            &r(x, y, x + CELL_W, y + 22.0),
            &label,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
            DWRITE_MEASURING_MODE_NATURAL,
        );

        let top = y + 26.0;
        let bot = top + 320.0;

        // 候選視窗的底：帶漸層，這樣半透明才看得出效果
        let stops = [
            D2D1_GRADIENT_STOP {
                position: 0.0,
                color: rgba(0.98, 0.98, 0.99, 1.0),
            },
            D2D1_GRADIENT_STOP {
                position: 1.0,
                color: rgba(0.88, 0.90, 0.94, 1.0),
            },
        ];
        let coll = ctx.dc.CreateGradientStopCollection(
            &stops,
            D2D1_COLOR_SPACE_SRGB,
            D2D1_COLOR_SPACE_SRGB,
            D2D1_BUFFER_PRECISION_8BPC_UNORM,
            D2D1_EXTEND_MODE_CLAMP,
            D2D1_COLOR_INTERPOLATION_MODE_PREMULTIPLIED,
        )?;
        let rect = r(x, top, x + CELL_W, bot);
        let bg = ctx.dc.CreateLinearGradientBrush(
            &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                startPoint: windows_numerics::Vector2 {
                    X: rect.left,
                    Y: rect.top,
                },
                endPoint: windows_numerics::Vector2 {
                    X: rect.left,
                    Y: rect.bottom,
                },
            },
            None,
            &coll,
        )?;
        ctx.dc.FillRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect,
                radiusX: 8.0,
                radiusY: 8.0,
            },
            &bg,
        );

        // 十列候選，第 3 列反白
        let cands = ["你", "妳", "尼", "泥", "擬", "膩", "逆", "匿", "溺", "暱"];
        let line_h = 30.0;
        let text = ctx
            .dc
            .CreateSolidColorBrush(&rgba(0.1, 0.1, 0.1, 1.0), None)?;
        let hot_text = ctx
            .dc
            .CreateSolidColorBrush(&rgba(1.0, 1.0, 1.0, 1.0), None)?;
        let index = ctx
            .dc
            .CreateSolidColorBrush(&rgba(0.56, 0.56, 0.56, 1.0), None)?;

        // **反白條先畫**（滑動中的位置），字再畫上去
        let hot_t = top + 8.0 + row * line_h;
        paint_highlight(
            ctx,
            r(x + 3.0, hot_t, x + CELL_W - 3.0, hot_t + line_h),
            style,
        )?;

        for (i, c) in cands.iter().enumerate() {
            let t = top + 8.0 + i as f32 * line_h;
            let row_rect = r(x + 3.0, t, x + CELL_W - 3.0, t + line_h);
            // 反白條蓋住哪一列，那列的字就用反白色
            let hot = (row - i as f32).abs() < 0.5;
            let s = format!("{}  {}", i + 1, c);
            let wide: Vec<u16> = s.encode_utf16().collect();
            ctx.dc.DrawText(
                &wide,
                &ctx.fmt,
                &D2D_RECT_F {
                    left: row_rect.left + 10.0,
                    ..row_rect
                },
                if hot && !matches!(style, Style::SheenOnly { dark_text: true }) {
                    &hot_text
                } else {
                    &text
                },
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            let _ = &index;
        }
        Ok(())
    }
}

/// 反白條——五種樣式的差別都在這裡。
unsafe fn paint_highlight(ctx: &Ctx, row: D2D_RECT_F, style: Style) -> Result<()> {
    unsafe {
        // 主題的強調色
        let (hr, hg, hb) = (0.0, 0.47, 0.83);
        let rr = D2D1_ROUNDED_RECT {
            rect: row,
            radiusX: 5.0,
            radiusY: 5.0,
        };
        match style {
            Style::Solid => {
                let b = ctx.dc.CreateSolidColorBrush(&rgba(hr, hg, hb, 1.0), None)?;
                ctx.dc.FillRoundedRectangle(&rr, &b);
            }
            Style::Alpha(a) => {
                let b = ctx.dc.CreateSolidColorBrush(&rgba(hr, hg, hb, a), None)?;
                ctx.dc.FillRoundedRectangle(&rr, &b);
            }
            Style::SheenOnly { .. } => {
                // **不畫底色**——完全透明，只有下面的高光帶與亮邊
            }
            Style::Glossy | Style::GlossyEdge | Style::TopSheen | Style::Glass => {
                // 上亮下暗——玻璃片反射環境光的樣子。
                // **差異拉大**，不然在藍底上看不出來。
                let alpha = if matches!(style, Style::Glass) {
                    0.6
                } else {
                    0.95
                };
                let stops = [
                    D2D1_GRADIENT_STOP {
                        position: 0.0,
                        color: rgba(hr + 0.30, hg + 0.25, hb + 0.15, alpha),
                    },
                    D2D1_GRADIENT_STOP {
                        position: 0.45,
                        color: rgba(hr, hg, hb, alpha),
                    },
                    D2D1_GRADIENT_STOP {
                        position: 1.0,
                        color: rgba((hr - 0.05).max(0.0), hg - 0.18, hb - 0.20, alpha),
                    },
                ];
                let coll = ctx.dc.CreateGradientStopCollection(
                    &stops,
                    D2D1_COLOR_SPACE_SRGB,
                    D2D1_COLOR_SPACE_SRGB,
                    D2D1_BUFFER_PRECISION_8BPC_UNORM,
                    D2D1_EXTEND_MODE_CLAMP,
                    D2D1_COLOR_INTERPOLATION_MODE_PREMULTIPLIED,
                )?;
                let br = ctx.dc.CreateLinearGradientBrush(
                    &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                        startPoint: windows_numerics::Vector2 {
                            X: row.left,
                            Y: row.top,
                        },
                        endPoint: windows_numerics::Vector2 {
                            X: row.left,
                            Y: row.bottom,
                        },
                    },
                    None,
                    &coll,
                )?;
                ctx.dc.FillRoundedRectangle(&rr, &br);

                // （高光帶與亮邊移到 match 外面，SheenOnly 也要用）
            }
        }

        // ── 高光帶與亮邊：這兩個是「玻璃感」的來源 ──
        {
            let want_sheen = matches!(
                style,
                Style::TopSheen | Style::Glass | Style::SheenOnly { .. }
            );
            let want_edge = matches!(
                style,
                Style::GlossyEdge | Style::Glass | Style::SheenOnly { .. }
            );
            let rr = D2D1_ROUNDED_RECT {
                rect: row,
                radiusX: 5.0,
                radiusY: 5.0,
            };
            // **上緣高光帶**：玻璃有厚度，上半部會反光
            if want_sheen {
                let h = (row.bottom - row.top) * 0.42;
                let sheen = [
                    D2D1_GRADIENT_STOP {
                        position: 0.0,
                        color: rgba(1.0, 1.0, 1.0, 0.38),
                    },
                    D2D1_GRADIENT_STOP {
                        position: 1.0,
                        color: rgba(1.0, 1.0, 1.0, 0.0),
                    },
                ];
                let sc = ctx.dc.CreateGradientStopCollection(
                    &sheen,
                    D2D1_COLOR_SPACE_SRGB,
                    D2D1_COLOR_SPACE_SRGB,
                    D2D1_BUFFER_PRECISION_8BPC_UNORM,
                    D2D1_EXTEND_MODE_CLAMP,
                    D2D1_COLOR_INTERPOLATION_MODE_PREMULTIPLIED,
                )?;
                let sb = ctx.dc.CreateLinearGradientBrush(
                    &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                        startPoint: windows_numerics::Vector2 {
                            X: row.left,
                            Y: row.top,
                        },
                        endPoint: windows_numerics::Vector2 {
                            X: row.left,
                            Y: row.top + h,
                        },
                    },
                    None,
                    &sc,
                )?;
                ctx.dc.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            bottom: row.top + h,
                            ..row
                        },
                        radiusX: 5.0,
                        radiusY: 5.0,
                    },
                    &sb,
                );
            }

            // **亮邊**：2px 白框
            if want_edge {
                let edge = ctx
                    .dc
                    .CreateSolidColorBrush(&rgba(1.0, 1.0, 1.0, 0.75), None)?;
                ctx.dc.DrawRoundedRectangle(&rr, &edge, 2.0, None);
            }
        }
        Ok(())
    }
}

/// **PREMULTIPLIED**：RGB 要先乘上 alpha，合成器才不會算錯。
fn rgba(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: r * a,
        g: g * a,
        b: b * a,
        a,
    }
}
