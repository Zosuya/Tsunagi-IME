//! Spike：候選視窗畫得出彩色 emoji 嗎？旗幟呢？
//!
//! **這是可行性測試，不是產品程式碼。** 跑起來開一個視窗，左半用
//! `D2D1_DRAW_TEXT_OPTIONS_NONE`（現況）、右半用 `ENABLE_COLOR_FONT`，
//! 肉眼直接比。
//!
//! 用法：cargo run -p ime-tip-windows --bin spike_emoji
//!
//! # 要驗證的三件事
//!
//! 1. **彩色**——現在的 `DrawText` 傳的是 `OPTIONS_NONE`，emoji 會是
//!    黑白線條還是彩色？開了 `ENABLE_COLOR_FONT` 有沒有差？
//! 2. **旗幟**——🇹🇼 是兩個「區域指示符」合成的，字型不支援時會顯示
//!    成兩個字母方框（TW）。Windows 的 Segoe UI Emoji **故意不含**
//!    國旗（微軟的政治立場），要實際看才知道。
//! 3. **叢集**——一個 emoji 在版面上佔幾格？候選視窗是逐格排版的，
//!    `👨‍👩‍👧` 若被當成 5 個字元排會爆開。這裡印出 DirectWrite 實際
//!    算出來的**叢集數**（cluster），那才是使用者眼裡的「一個字」。

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
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const W: i32 = 760;
const H: i32 = 560;

/// 要測的樣本。第一欄是說明，第二欄是實際字串。
const SAMPLES: &[(&str, &str)] = &[
    ("單字元符號", "★☆✦※"),
    ("基本表情", "😀😂🤔😅"),
    ("變體選擇符", "☀️❤️✔️"),
    ("膚色", "👍🏽👋🏻"),
    ("ZWJ 家庭", "👨‍👩‍👧"),
    ("ZWJ 職業", "👩‍💻👨‍🍳"),
    ("國旗", "🇹🇼🇯🇵🇺🇸"),
    ("次級旗", "🏴󠁧󠁢󠁳󠁣󠁴󠁿"),
    ("keycap", "1️⃣2️⃣"),
    ("動物食物", "🐱🍜🎉🚗"),
];

fn main() -> Result<()> {
    unsafe {
        // 沒有這行的話視窗會被系統拉伸（高 DPI 螢幕上截圖只看得到左半邊）
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;

        let write: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

        // 先在主控台印出叢集分析——這部分不必等視窗
        println!("=== 叢集分析（DirectWrite 眼裡的「一個字」）===\n");
        for (name, s) in SAMPLES {
            let fmt = text_format(&write, "", 28.0)?;
            let wide: Vec<u16> = s.encode_utf16().collect();
            let layout = write.CreateTextLayout(&wide, &fmt, f32::MAX / 2.0, f32::MAX / 2.0)?;

            // 叢集度量：一個 cluster 就是使用者眼裡的一個字
            let mut count = 0u32;
            let _ = layout.GetClusterMetrics(None, &mut count);
            let mut clusters = vec![DWRITE_CLUSTER_METRICS::default(); count as usize];
            let mut actual = 0u32;
            let _ = layout.GetClusterMetrics(Some(&mut clusters), &mut actual);

            let mut metrics = DWRITE_TEXT_METRICS::default();
            let _ = layout.GetMetrics(&mut metrics);

            println!(
                "{:14} chars={:2}  utf16={:2}  叢集={:2}  寬度={:6.1}",
                name,
                s.chars().count(),
                wide.len(),
                actual,
                metrics.widthIncludingTrailingWhitespace
            );
            let widths: Vec<String> = clusters
                .iter()
                .take(actual as usize)
                .map(|c| format!("{:.0}", c.width))
                .collect();
            println!("{:14} 每叢集寬度 = [{}]", "", widths.join(", "));
            let lens: Vec<String> = clusters
                .iter()
                .take(actual as usize)
                .map(|c| c.length.to_string())
                .collect();
            println!(
                "{:14} 每叢集 length = [{}]（總和 {}，utf16 共 {}）",
                "",
                lens.join(", "),
                clusters
                    .iter()
                    .take(actual as usize)
                    .map(|c| c.length as usize)
                    .sum::<usize>(),
                wide.len()
            );

            // **驗證正式碼的 `clusters()` 切得對不對**——預覽列就是用它
            // 排版的，切錯就會把 👩‍💻 拆成兩個人像
            let meas = ime_tip_windows::d2d::TextMeasurer::new()?;
            let f2 = meas.format("", 28.0, 96.0)?;
            let parts = meas.clusters(s, &f2);
            let pieces: Vec<&str> = parts.iter().map(|&(a, b)| &s[a..b]).collect();
            // **兩邊用的字型不同**：上面那份 metrics 是系統預設字型，
            // `clusters()` 走的是候選視窗真正用的正黑體。keycap（`1️⃣`）
            // 兩者會不一樣——正黑體自己含 U+20E3 的字形，DirectWrite
            // 就讓它獨立成一格。那是字型層的事實，不是換算錯誤。
            let ok = if parts.len() == actual as usize {
                "✓"
            } else {
                "△ 兩種字型的分群不同（見註解）"
            };
            println!("{:14} clusters() → {:?} {ok}", "", pieces);
        }
        println!("\n視窗開起來了，左欄=現況(OPTIONS_NONE)、右欄=ENABLE_COLOR_FONT");
        println!("關掉視窗結束。\n");

        // 開視窗做肉眼比對
        let hwnd = create_window()?;
        let (dc, swap, comp_target) = init_d2d(hwnd)?;
        let _ = comp_target;
        draw(&dc, &write, &swap)?;

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        Ok(())
    }
}

unsafe fn text_format(
    write: &IDWriteFactory,
    family: &str,
    size: f32,
) -> Result<IDWriteTextFormat> {
    let fam: Vec<u16> = family.encode_utf16().chain(std::iter::once(0)).collect();
    let locale: Vec<u16> = "zh-TW".encode_utf16().chain(std::iter::once(0)).collect();
    write.CreateTextFormat(
        PCWSTR(fam.as_ptr()),
        None,
        DWRITE_FONT_WEIGHT_NORMAL,
        DWRITE_FONT_STYLE_NORMAL,
        DWRITE_FONT_STRETCH_NORMAL,
        size,
        PCWSTR(locale.as_ptr()),
    )
}

unsafe fn draw(
    dc: &ID2D1DeviceContext,
    write: &IDWriteFactory,
    swap: &IDXGISwapChain1,
) -> Result<()> {
    dc.BeginDraw();
    dc.Clear(Some(&D2D1_COLOR_F {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    }));

    let black = dc.CreateSolidColorBrush(
        &D2D1_COLOR_F {
            r: 0.1,
            g: 0.1,
            b: 0.1,
            a: 1.0,
        },
        None,
    )?;
    let gray = dc.CreateSolidColorBrush(
        &D2D1_COLOR_F {
            r: 0.5,
            g: 0.5,
            b: 0.5,
            a: 1.0,
        },
        None,
    )?;

    let label_fmt = text_format(write, "", 12.0)?;
    let emoji_fmt = text_format(write, "", 26.0)?;

    // 表頭
    let head = text_format(write, "", 13.0)?;
    draw_str(dc, "說明", &head, &gray, 10.0, 8.0, 140.0);
    draw_str(dc, "OPTIONS_NONE（現況）", &head, &gray, 150.0, 8.0, 280.0);
    draw_str(dc, "ENABLE_COLOR_FONT", &head, &gray, 450.0, 8.0, 280.0);

    let mut y = 34.0f32;
    for (name, s) in SAMPLES {
        draw_str(dc, name, &label_fmt, &gray, 10.0, y + 10.0, 140.0);
        // 左欄：現況
        draw_str_opt(
            dc,
            s,
            &emoji_fmt,
            &black,
            150.0,
            y,
            280.0,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
        );
        // 右欄：開彩色
        draw_str_opt(
            dc,
            s,
            &emoji_fmt,
            &black,
            450.0,
            y,
            280.0,
            D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
        );
        y += 50.0;
    }

    dc.EndDraw(None, None)?;
    swap.Present(1, DXGI_PRESENT(0)).ok()?;
    Ok(())
}

unsafe fn draw_str(
    dc: &ID2D1DeviceContext,
    s: &str,
    fmt: &IDWriteTextFormat,
    brush: &ID2D1SolidColorBrush,
    x: f32,
    y: f32,
    w: f32,
) {
    draw_str_opt(dc, s, fmt, brush, x, y, w, D2D1_DRAW_TEXT_OPTIONS_NONE);
}

// spike 是攤平寫的，參數多但一眼看得完，不為它抽型別
#[allow(clippy::too_many_arguments)]
unsafe fn draw_str_opt(
    dc: &ID2D1DeviceContext,
    s: &str,
    fmt: &IDWriteTextFormat,
    brush: &ID2D1SolidColorBrush,
    x: f32,
    y: f32,
    w: f32,
    opts: D2D1_DRAW_TEXT_OPTIONS,
) {
    let wide: Vec<u16> = s.encode_utf16().collect();
    dc.DrawText(
        &wide,
        fmt,
        &D2D_RECT_F {
            left: x,
            top: y,
            right: x + w,
            bottom: y + 46.0,
        },
        brush as &ID2D1Brush,
        opts,
        DWRITE_MEASURING_MODE_NATURAL,
    );
}

// ─────────── 以下是開視窗的樣板，跟 spike_render.rs 同一套 ───────────

unsafe fn create_window() -> Result<HWND> {
    let class: Vec<u16> = "SpikeEmojiClass\0".encode_utf16().collect();
    let wc = WNDCLASSW {
        lpfnWndProc: Some(wndproc),
        lpszClassName: PCWSTR(class.as_ptr()),
        hCursor: LoadCursorW(None, IDC_ARROW)?,
        ..Default::default()
    };
    RegisterClassW(&wc);
    let title: Vec<u16> = "Emoji spike\0".encode_utf16().collect();
    CreateWindowExW(
        WINDOW_EX_STYLE(0),
        PCWSTR(class.as_ptr()),
        PCWSTR(title.as_ptr()),
        WS_OVERLAPPEDWINDOW | WS_VISIBLE,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        W,
        H,
        None,
        None,
        None,
        None,
    )
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_DESTROY {
            PostQuitMessage(0);
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, msg, w, l)
    }
}

unsafe fn init_d2d(
    hwnd: HWND,
) -> Result<(ID2D1DeviceContext, IDXGISwapChain1, IDCompositionTarget)> {
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

    let factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
    let device = factory.CreateDevice(&dxgi)?;
    let dc = device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;

    let adapter = dxgi.GetAdapter()?;
    let dxgi_factory: IDXGIFactory2 = adapter.GetParent()?;
    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: W as u32,
        Height: H as u32,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 2,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
        AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
        ..Default::default()
    };
    let swap = dxgi_factory.CreateSwapChainForComposition(&d3d, &desc, None)?;

    let surface: IDXGISurface = swap.GetBuffer(0)?;
    let props = D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 96.0,
        dpiY: 96.0,
        bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
        colorContext: std::mem::ManuallyDrop::new(None),
    };
    let bitmap = dc.CreateBitmapFromDxgiSurface(&surface, Some(&props))?;
    dc.SetTarget(&bitmap);

    let comp_device: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;
    let target = comp_device.CreateTargetForHwnd(hwnd, true)?;
    let visual = comp_device.CreateVisual()?;
    visual.SetContent(&swap)?;
    target.SetRoot(&visual)?;
    comp_device.Commit()?;

    Ok((dc, swap, target))
}
