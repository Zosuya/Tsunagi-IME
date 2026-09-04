//! 全半形切換的獨立提示視窗。
//!
//! ```text
//!            ┌────────────────────┐
//!            │  自動 [半形] 全形   │  ← 這個視窗
//!            └────────────────────┘
//! 文件區：你好|
//!            ┌──────────┐
//!            │ ▶ 你好    │           ← 候選視窗（另一個）
//!            └──────────┘
//! ```
//!
//! # 為什麼要獨立一個視窗
//!
//! 它跟候選是兩回事：候選是「你要選哪個字」，這個是「輸入法現在
//! 什麼狀態」。混在同一個視窗裡的話，候選清單的高度會忽大忽小，
//! 而且沒在組字時得為了顯示它硬開一個空的候選視窗。
//!
//! 分開之後兩邊各自管自己的生命週期，簡單得多。
//!
//! # 動畫
//!
//! TSF 沒有「每一幀叫我一次」的機制，這個視窗得自己開 `SetTimer`
//! 每幀重畫（候選視窗的反白條滑動也是同一套做法，見 `slide.rs`）。
//! 動畫短（約 1.3 秒）就停掉——計時器跑在宿主行程的訊息迴圈裡，
//! 不能一直佔著別人的。

use std::sync::Once;

use ime_core::width::Width;
use windows::core::{w, Result, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AddFontMemResourceEx, BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
    CreatePen, CreateRoundRectRgn, CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint,
    FillRect, GetCharABCWidthsW, GetTextExtentPoint32W, GetTextMetricsW, InvalidateRect, RoundRect,
    SelectObject, SetBkMode, SetTextColor, SetWindowRgn, UpdateWindow, ABC, DT_LEFT, DT_SINGLELINE,
    PAINTSTRUCT, PS_SOLID, SRCCOPY, TEXTMETRICW, TRANSPARENT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, KillTimer, LoadCursorW,
    RegisterClassW, SetTimer, SetWindowPos, ShowWindow, CS_DROPSHADOW, CS_HREDRAW, CS_VREDRAW,
    HWND_TOPMOST, IDC_ARROW, MA_NOACTIVATEANDEAT, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE,
    SW_SHOWNOACTIVATE, WM_DESTROY, WM_ERASEBKGND, WM_MOUSEACTIVATE, WM_PAINT, WM_TIMER, WNDCLASSW,
    WS_EX_NOACTIVATE, WS_EX_TOPMOST, WS_POPUP,
};

use crate::theme::{Color, Theme};
use crate::width_bar::WidthBar;

const CLASS_NAME: PCWSTR = w!("UniversalIME.WidthWindow");
const ANIM_TIMER: usize = 1;
/// 更新間隔（毫秒）。10ms 約 100fps——比 60fps 再滑順一點，
/// 而動畫只跑不到一秒，多出來的成本很有限。
const ANIM_INTERVAL: u32 = 10;

/// 反白條的內縮與圓角（邏輯像素）。
///
/// 內縮讓反白不貼齊視窗邊緣，圓角才看得出來。
const HL_INSET: i32 = 2;
/// 圓角橢圓的直徑（`RoundRect` 收的就是直徑，不是半徑）
const HL_RADIUS: i32 = 10;

/// 每格的寬度與整體高度（邏輯像素，會按 DPI 與主題縮放）。
/// 標籤用的字型。**寫死不開放設定**（使用者要求）。
///
/// 字型檔跟著 DLL 走（見下方 `ensure_symbol_font`），所以不管對方電腦
/// 裝了什麼，這幾個字都長同一個樣子。
const SYMBOL_FONT: &str = "Tsunagi Symbols";

/// 標籤的字身高度（邏輯像素，格子高 `HEIGHT` = 28）。
///
/// # 為什麼用像素而不是「主題字級的百分比」
///
/// 第一版寫成 `font_size_pt() * 百分比`，結果**換一台電腦大小就不對**。
/// 根因是那條路會套兩次不同來源的縮放：pt 轉像素時乘的是候選視窗的
/// thread-local `DPI`，而這個視窗的格子用的是自己算的倍率（`scaled()`，
/// 由視窗實際寬度回推）。提示視窗先於候選視窗出現時，那個 thread-local
/// 還停在預設的 96，字就相對縮小。
///
/// 改成直接指定邏輯像素、再走同一個 `scaled()`——字與格子從此用**同一個
/// 倍率**，任何 DPI 與縮放設定下比例都一致。
const SYMBOL_FONT_PX: i32 = 19;

/// 標籤往下微調的量，佔字高的百分比。
///
/// **數學上的置中不等於視覺上的置中**——漢字與注音的重心偏上（「ㄅ」
/// 「あ」尤其明顯，筆畫都集中在上半部），照字身高度算出來的正中央
/// 看起來會偏高。
///
/// 用比例而不是固定像素，這樣換字級或換 DPI 都不必重調。
const SYMBOL_NUDGE_PERCENT: i32 = 15;

/// 只含標籤那六個字的 subset 字型（約 4KB）。
///
/// # 為什麼要嵌進來
///
/// 原本只寫字型名字、讓系統去找——**別人的電腦沒裝就靜默退回系統預設**，
/// 不報錯，只是字形不一樣。而「寫死字型」的用意正是「不管在誰的電腦上
/// 都長這樣」，退回等於那個用意失效（開發文件 §4.11）。
///
/// # 為什麼不是原本那套字型
///
/// 原本指定的是商業字型，授權寫著「限兩台裝置、不得轉讓給任何人」，
/// 不能隨產品散布。改用 SIL Open Font License 的來源，那個授權明確允許
/// subset 與散布。產生方式見 `res/make_symbol_font.py`。
const SYMBOL_FONT_DATA: &[u8] = include_bytes!("../res/symbols.ttf");

/// 把嵌入的字型註冊給這個行程用。
///
/// `AddFontMemResourceEx` 載入的字型**只有本行程看得到**，不會污染使用者
/// 的系統字型清單，也不必寫檔到磁碟。
///
/// 每個宿主行程各載一次（`Once`）。**故意不呼叫 `RemoveFontMemResourceEx`**
/// ——這份字型要活到行程結束，而 DLL 卸載時機由宿主決定，提早移除會讓
/// 還開著的提示視窗畫不出字。4KB 的常駐成本可以接受。
fn ensure_symbol_font() {
    static LOADED: Once = Once::new();
    LOADED.call_once(|| {
        let count: u32 = 0;
        // SAFETY: 傳的是 include_bytes! 的靜態切片，生命週期是整個行程，
        // 長度也由切片本身給——不會有懸空或長度錯配
        let handle = unsafe {
            AddFontMemResourceEx(
                SYMBOL_FONT_DATA.as_ptr() as *const _,
                SYMBOL_FONT_DATA.len() as u32,
                None,
                &count,
            )
        };
        if handle.is_invalid() || count == 0 {
            // 載入失敗不是致命的：`CreateFontW` 找不到這個名字就會退回
            // 系統字型，字形不同但功能不受影響
            crate::dlog!("[width] 嵌入字型載入失敗，退回系統字型");
        }
    });
}

const CELL_W: i32 = 34;
const HEIGHT: i32 = 28;

static REGISTER_CLASS: Once = Once::new();

thread_local! {
    /// 這一輪要畫哪幾個標籤。
    ///
    /// **這個視窗同時服務兩個功能**：全半形（自／半／全）與語言模式
    /// （自／注／日／英）。兩者的動畫、版面、淡出完全一樣，差別只有
    /// 格數與標籤——與其複製一份幾乎相同的視窗，不如把標籤變成狀態。
    static LABELS: std::cell::RefCell<Vec<&'static str>> =
        const { std::cell::RefCell::new(Vec::new()) };
    /// 動畫狀態。`None` 代表沒在動，視窗該藏起來。
    static BAR: std::cell::RefCell<Option<WidthBar>> = const { std::cell::RefCell::new(None) };
    /// 畫的時候要用的主題。
    static THEME: std::cell::RefCell<Option<Theme>> = const { std::cell::RefCell::new(None) };
}

fn ensure_class() {
    REGISTER_CLASS.call_once(|| unsafe {
        let wc = WNDCLASSW {
            // 跟候選視窗一樣的陰影，視覺才一致
            style: CS_HREDRAW | CS_VREDRAW | CS_DROPSHADOW,
            lpfnWndProc: Some(wndproc),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        RegisterClassW(&wc);
    });
}

/// 全半形提示視窗。
pub struct WidthWindow {
    hwnd: HWND,
}

impl WidthWindow {
    /// 顯示（或更新）視窗並開始動畫。
    ///
    /// `anchor` 是候選視窗的左上角——這個視窗要疊在它**上方**，
    /// 所以會自己往上挪一個身高。
    pub fn show(
        existing: Option<Self>,
        theme: &Theme,
        from: Width,
        to: Width,
        anchor: POINT,
        dpi: i32,
    ) -> Result<Self> {
        use crate::width_bar::{index_of, symbol, OPTIONS};
        let labels: Vec<&'static str> = OPTIONS.iter().map(|&o| symbol(o)).collect();
        Self::show_bar(
            existing,
            theme,
            labels,
            index_of(from),
            index_of(to),
            anchor,
            dpi,
        )
    }

    /// 通用版：直接給標籤與索引。
    ///
    /// 全半形與語言模式共用這個——兩者的動畫與版面完全一樣，
    /// 只有格數和標籤不同。
    #[allow(clippy::too_many_arguments)]
    pub fn show_bar(
        existing: Option<Self>,
        theme: &Theme,
        labels: Vec<&'static str>,
        from: usize,
        to: usize,
        anchor: POINT,
        dpi: i32,
    ) -> Result<Self> {
        ensure_class();

        let n = labels.len().max(1) as i32;
        let scale = |v: i32| v * theme.metrics.scale_percent / 100 * dpi / 96;
        let w = scale(CELL_W) * n;
        let h = scale(HEIGHT);

        THEME.with(|t| *t.borrow_mut() = Some(theme.clone()));
        LABELS.with(|l| *l.borrow_mut() = labels);
        BAR.with(|b| {
            let prev = b.borrow().clone();
            *b.borrow_mut() = Some(WidthBar::start_at(prev.as_ref(), from, to));
        });

        // 疊在候選視窗上方，中間留一點縫
        let x = anchor.x;
        let y = anchor.y - h - scale(4);

        let win = match existing {
            Some(win) => {
                unsafe {
                    let _ = SetWindowPos(
                        win.hwnd,
                        Some(HWND_TOPMOST),
                        x,
                        y,
                        w,
                        h,
                        SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    );
                }
                win
            }
            None => {
                let hwnd = unsafe {
                    CreateWindowExW(
                        WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                        CLASS_NAME,
                        w!(""),
                        WS_POPUP,
                        x,
                        y,
                        w,
                        h,
                        None,
                        None,
                        None,
                        None,
                    )?
                };
                unsafe {
                    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                }
                Self { hwnd }
            }
        };

        unsafe {
            // `corner_radius()` 已經套過主題縮放，這裡只補 DPI，
            // 再呼叫 `scale` 會被縮放兩次
            round_corners(win.hwnd, w, h, theme.metrics.corner_radius() * dpi / 96);
            SetTimer(Some(win.hwnd), ANIM_TIMER, ANIM_INTERVAL, None);
            let _ = InvalidateRect(Some(win.hwnd), None, false);
            let _ = UpdateWindow(win.hwnd);
        }
        Ok(win)
    }
}

/// Shift 放開了——通知動畫開始倒數淡出。
///
/// 按著時提示會一直亮著，使用者才看得到自己連續切到哪個模式。
pub fn on_shift_release() {
    BAR.with(|b| {
        if let Some(bar) = b.borrow_mut().as_mut() {
            bar.release();
        }
    });
}

impl Drop for WidthWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), ANIM_TIMER);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// 圓角。region 交出去之後由系統管，不能自己 `DeleteObject`。
unsafe fn round_corners(hwnd: HWND, w: i32, h: i32, r: i32) {
    unsafe {
        if r <= 0 {
            return;
        }
        let rgn = CreateRoundRectRgn(0, 0, w + 1, h + 1, r * 2, r * 2);
        if !rgn.is_invalid() {
            let _ = SetWindowRgn(hwnd, Some(rgn), false);
        }
    }
}

/// **panic 不能穿過這裡**：`wndproc` 是 `extern "system"`，宿主的訊息
/// 迴圈直接呼叫它，unwinding 出去會讓整個宿主行程 abort。攔下來交給
/// `DefWindowProcW`——見 `crate::guard`。
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    crate::guard::wndproc("全半形提示 wndproc", hwnd, msg, wp, lp, || unsafe {
        wndproc_inner(hwnd, msg, wp, lp)
    })
}

unsafe fn wndproc_inner(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            // 自己畫滿整個範圍，不要系統先塗白（那一下就是閃爍）
            WM_ERASEBKGND => LRESULT(1),
            WM_TIMER if wp.0 == ANIM_TIMER => {
                let done = BAR.with(|b| b.borrow().as_ref().map(|x| x.done()).unwrap_or(true));
                if done {
                    let _ = KillTimer(Some(hwnd), ANIM_TIMER);
                    BAR.with(|b| *b.borrow_mut() = None);
                    let _ = ShowWindow(hwnd, SW_HIDE);
                    return LRESULT(0);
                }
                let _ = InvalidateRect(Some(hwnd), None, false);
                let _ = UpdateWindow(hwnd);
                LRESULT(0)
            }
            // 點下去不要搶宿主的焦點，也不要把點擊傳進來
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATEANDEAT as isize),
            WM_DESTROY => LRESULT(0),
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

unsafe fn paint(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let screen_dc = BeginPaint(hwnd, &mut ps);

        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let w = (rc.right - rc.left).max(1);
        let h = (rc.bottom - rc.top).max(1);

        // 雙緩衝：先畫到記憶體點陣圖再一次貼上，不然一層層畫會閃
        let mem = CreateCompatibleDC(Some(screen_dc));
        let bmp = CreateCompatibleBitmap(screen_dc, w, h);
        let old_bmp = SelectObject(mem, bmp.into());
        let hdc = mem;

        let theme = THEME.with(|t| t.borrow().clone());
        let bar = BAR.with(|b| b.borrow().clone());
        if let (Some(theme), Some(bar)) = (theme, bar) {
            let c = &theme.colors;
            let alpha = bar.opacity();
            // **格子填滿整個視窗**，不留多餘空間。
            //
            // 每格的左右邊界都用「視窗寬 × 索引 ÷ 格數」直接算，
            // 而不是「格寬 × 索引」——後者除不盡時餘數會堆在右邊
            // 變成空白，整組看起來就偏左。
            let labels = LABELS.with(|l| l.borrow().clone());
            let n_opts = labels.len().max(1) as i32;
            let edge = |i: i32| w * i / n_opts;
            // 小數版：反白條滑動時位置在兩格之間，但左右緣仍要
            // 落在跟 `edge` 一致的刻度上，字才會正好在反白的正中央。
            let edge_f = |i: f32| (w as f32 * i / n_opts as f32).round() as i32;

            // 底：整片視窗
            let bg = CreateSolidBrush(mix(c.window_bg, c.window_bg, 1.0).to_colorref());
            FillRect(hdc, &rc, bg);
            let _ = DeleteObject(bg.into());

            // 反白條：位置是小數，滑動才平順。**畫成圓角**——
            // `RoundRect` 會同時填色與描邊，所以筆刷與畫筆都要換掉。
            let inset = scaled(HL_INSET, w);
            let radius = scaled(HL_RADIUS, w);
            // **反白的左右緣要跟文字用同一套刻度**。
            //
            // 原本左緣用 `edge_f`、右緣卻是「左緣 + 格寬」——格寬是
            // `edge(1)`，除不盡時比實際格子窄一像素，反白就整塊偏左，
            // 字看起來沒在中間。改成兩緣都用 `edge_f` 各自算。
            let idx = bar.visual_index();
            let x0 = edge_f(idx);
            let x1 = edge_f(idx + 1.0);
            let fill = mix(c.highlight_bg, c.window_bg, alpha).to_colorref();
            let b = CreateSolidBrush(fill);
            let pen = CreatePen(PS_SOLID, 1, fill);
            let old_b = SelectObject(hdc, b.into());
            let old_p = SelectObject(hdc, pen.into());
            // `RoundRect` 最後兩個參數是**圓角橢圓的寬高（直徑）**，
            // 不是半徑——先前多乘了 2，圓角大到把方塊吃掉，
            // 看起來就像反白偏移了。
            let _ = RoundRect(
                hdc,
                x0 + inset,
                inset,
                x1 - inset,
                h - inset,
                radius,
                radius,
            );
            SelectObject(hdc, old_b);
            SelectObject(hdc, old_p);
            let _ = DeleteObject(b.into());
            let _ = DeleteObject(pen.into());

            SetBkMode(hdc, TRANSPARENT);
            // **字型寫死**，不跟著設定走——使用者要求這一格固定。
            // 設定頁選的字型只影響候選視窗。
            ensure_symbol_font();
            // 用 `scaled()` 換算，跟格子共用同一個倍率——見 `SYMBOL_FONT_PX`
            let font =
                crate::candidate_window::make_ui_font_px(SYMBOL_FONT, scaled(SYMBOL_FONT_PX, w));
            let old_font = SelectObject(hdc, font.into());

            for (i, label) in labels.iter().enumerate() {
                let hot = i == bar.target();
                let fg = if hot { c.highlight_text } else { c.index };
                SetTextColor(hdc, mix(fg, c.window_bg, alpha).to_colorref());
                let mut line: Vec<u16> = label.encode_utf16().collect();
                line.push(0);
                // **自己算垂直置中**，不靠 `DT_VCENTER`。
                //
                // `DT_VCENTER` 按「字型行高」置中，行高含字身上方的
                // 內建行距（internal leading）——那段空白算進去，
                // 字看起來就偏上。
                //
                // 真正的字身頂端是 `tmAscent - tmInternalLeading`
                // （`tmAscent` 本身已經含 internal leading，不要再扣一次）。
                let mut tm = TEXTMETRICW::default();
                let _ = GetTextMetricsW(hdc, &mut tm);
                let cap = tm.tmAscent - tm.tmInternalLeading; // 字身上半
                let glyph_h = cap + tm.tmDescent; // 實際看得到的字高
                                                  // 讓字身置中：頂端要往上退掉 internal leading 那段
                let top =
                    (h - glyph_h) / 2 - tm.tmInternalLeading + glyph_h * SYMBOL_NUDGE_PERCENT / 100;

                // **水平也自己算**，不靠 `DT_CENTER`。
                //
                // `DT_CENTER` 按字元的**宣告寬度**置中，那是排版用的
                // 前進量（advance width），含字形兩側的空隙。字形在
                // 那個框裡不一定置中——量到「自」在 12pt 下左空 2、
                // 右空 1，而「半」「全」是 0/0，三個字的視覺中心就
                // 對不齊。
                //
                // 改成按 `GetCharABCWidthsW` 的實際字形寬度（B）置中，
                // 並扣掉左側空隙（A），看到的字才真的在格子正中央。
                let cell_l = edge(i as i32);
                let cell_r = edge(i as i32 + 1);
                let ch = label.chars().next().unwrap_or(' ') as u32;
                let mut abc = [ABC::default(); 1];
                let left = if GetCharABCWidthsW(hdc, ch, ch, abc.as_mut_ptr()).as_bool() {
                    let a = abc[0].abcA;
                    let b = abc[0].abcB as i32;
                    // 字形置中之後，畫的起點要往左退掉左空隙那一段
                    (cell_l + cell_r - b) / 2 - a
                } else {
                    // 取不到 ABC（非 TrueType 字型）就退回宣告寬度置中
                    let mut size = windows::Win32::Foundation::SIZE::default();
                    let _ = GetTextExtentPoint32W(hdc, &line[..line.len() - 1], &mut size);
                    (cell_l + cell_r - size.cx) / 2
                };
                let mut r = RECT {
                    left,
                    top,
                    // 從算好的左緣往右畫，不要再讓 `DT_CENTER` 動它
                    right: cell_r + (cell_r - cell_l),
                    bottom: top + tm.tmHeight,
                };
                DrawTextW(hdc, &mut line, &mut r, DT_LEFT | DT_SINGLELINE);
            }

            SelectObject(hdc, old_font);
            let _ = DeleteObject(font.into());
        }

        let _ = BitBlt(screen_dc, 0, 0, w, h, Some(mem), 0, 0, SRCCOPY);
        SelectObject(mem, old_bmp);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        let _ = EndPaint(hwnd, &ps);
    }
}

/// 把邏輯像素換成這個視窗的實際像素。
///
/// 畫的時候拿不到 DPI 與縮放設定，但視窗寬度是照那些算出來的——
/// 用「實際寬度 ÷ 基準寬度」回推倍率就好。
fn scaled(v: i32, window_w: i32) -> i32 {
    let n = LABELS.with(|l| l.borrow().len()).max(1) as i32;
    let base = CELL_W * n;
    (v * window_w / base.max(1)).max(1)
}

/// 把 `fg` 往 `bg` 混，`t` 是 `fg` 的比重。
///
/// GDI 沒有 alpha 通道，淡出只能靠混色模擬——不透明度 0.3 就是
/// 「三成前景色、七成背景色」，視覺上等同於淡出。
fn mix(fg: Color, bg: Color, t: f32) -> Color {
    let f = |a: u8, b: u8| (a as f32 * t + b as f32 * (1.0 - t)).round() as u8;
    Color::rgb(f(fg.r, bg.r), f(fg.g, bg.g), f(fg.b, bg.b))
}
