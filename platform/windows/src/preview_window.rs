//! 預覽列：組字當下會送出什麼，畫成一條獨立的視窗。
//!
//! # 為什麼是獨立的視窗
//!
//! 原本畫在候選視窗最上面那一列，兩種資訊塞在同一塊畫布上。問題是
//! **兩者的寬度不相干**：預覽列要裝整句（可以很長），候選清單只裝
//! 一個字的同音字（很短）。同一個視窗只能取兩者的最大值，短的那邊
//! 旁邊就空出一大塊。
//!
//! 留白想靠「不畫」解掉是行不通的——系統陰影（`CS_DROPSHADOW`）依
//! 視窗矩形畫，不管裡面畫了什麼，透明處會剩一塊孤零零的陰影。
//!
//! 拆成兩個視窗之後各自貼合自己的內容，陰影也各自正確。兩者上下貼
//! 齊、相接的那一邊畫直角，看起來仍是一體。
//!
//! 不搶焦點的理由與候選視窗相同，見那邊的說明。

use std::sync::Once;

use crate::candidate_window::{draw_label, layout_preview, preview_gap, preview_scroll, BASE_DPI};
use crate::d2d::{Rect, Renderer, TextMeasurer};
use crate::theme::{Color, Theme};
use ime_core::render::slide::{Span, SpanSlide};
use windows::core::{w, Result, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{InvalidateRect, UpdateWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, KillTimer, LoadCursorW,
    RegisterClassW, SetTimer, SetWindowPos, ShowWindow, CS_DROPSHADOW, CS_HREDRAW, CS_VREDRAW,
    HWND_TOPMOST, IDC_ARROW, MA_NOACTIVATE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_SHOWNOACTIVATE,
    WM_DESTROY, WM_ERASEBKGND, WM_MOUSEACTIVATE, WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_NOACTIVATE,
    WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOPMOST, WS_POPUP,
};

const CLASS_NAME: PCWSTR = w!("UniversalIME.EchoPreviewWindow");

/// 反白塊滑動動畫的計時器編號與間隔（毫秒）。跟候選視窗同一個節奏。
const ANIM_TIMER: usize = 1;
const ANIM_INTERVAL: u32 = 10;

static REGISTER_CLASS: Once = Once::new();

thread_local! {
    /// 預覽列的文字——組字當下的第一名切法。
    ///
    /// 組字區保持原始按鍵（打什麼顯示什麼），轉換結果放在這一列。
    /// 這樣打字時看到的是自己按了什麼，不會被逐字轉換干擾，
    /// 同時又看得到引擎的判斷。
    static TEXT: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };

    /// 要反白的那一段（在 `TEXT` 裡的位元組範圍）。
    ///
    /// 選字時要標出「正在選哪一格」。原本是把那一段用【】包起來，
    /// 但那會**改變文字本身**——預覽列的用意是「送出去會長這樣」，
    /// 混進不會送出的符號就不誠實了，而且中文全形括號很佔位置。
    /// 改成畫在文字上，文字保持原樣。
    ///
    /// 中間試過細外框，但小字級下不夠顯眼、得盯著找，
    /// 最後定案是跟候選清單同一組反白色（藍底白字）。
    static BOX_RANGE: std::cell::RefCell<Option<std::ops::Range<usize>>> =
        const { std::cell::RefCell::new(None) };

    /// 反白塊正在滑動的話，這裡記著它的動畫。
    static SLIDE: std::cell::RefCell<Option<SpanSlide>> =
        const { std::cell::RefCell::new(None) };

    /// 上一次**實際畫出來**的反白塊位置（左緣, 右緣）。
    ///
    /// 為什麼要記：位置得量字寬才知道，而量字寬要有繪圖器和字型，
    /// 只有 `paint` 拿得到；但「要不要開始滑、從哪裡滑」是 `show`
    /// 在決定的。所以由 `paint` 把算好的位置存起來，`show` 下次
    /// 拿它當起點。
    static SPAN: std::cell::Cell<Option<Span>> = const { std::cell::Cell::new(None) };

    /// 這一輪的反白要不要用滑的。
    ///
    /// `show` 判斷（同一句話、只是換了一格才滑），`paint` 執行——
    /// 因為只有 `paint` 量得出新位置在哪。
    static ANIMATE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };

    /// 底下有沒有接著候選視窗。有的話下緣畫直角，兩塊才貼得起來。
    static HAS_LIST_BELOW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };

    /// 目前套用的主題。
    static THEME: std::cell::RefCell<Theme> = std::cell::RefCell::new(Theme::default());

    /// 這個視窗的 DPI。主題尺寸是邏輯像素，畫之前要按它放大。
    static DPI: std::cell::Cell<i32> =
        const { std::cell::Cell::new(BASE_DPI) };

    /// Direct2D 繪圖器。跟視窗一起活著，視窗關掉才丟。
    static RENDERER: std::cell::RefCell<Option<Renderer>> = const { std::cell::RefCell::new(None) };

    /// 量字寬用的 DirectWrite 工具。**跟繪製同一個引擎**——
    /// 用 GDI 量的話兩者每字元的微小差異會累積，長句子就被切掉。
    static MEASURER: std::cell::RefCell<Option<TextMeasurer>> =
        const { std::cell::RefCell::new(None) };
}

/// 換主題。候選視窗那邊有自己的一份，兩邊都要換。
pub fn set_theme(t: Theme) {
    THEME.with(|x| *x.borrow_mut() = t);
}

fn ensure_class_registered() {
    REGISTER_CLASS.call_once(|| unsafe {
        let wc = WNDCLASSW {
            // `CS_DROPSHADOW` 給 popup 一層系統畫的陰影，跟候選視窗一致
            style: CS_HREDRAW | CS_VREDRAW | CS_DROPSHADOW,
            lpfnWndProc: Some(wndproc),
            lpszClassName: CLASS_NAME,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);
    });
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // COM／wndproc 邊界一律包 `catch_unwind`——panic 穿過去會帶走宿主
    crate::guard::wndproc("預覽列 wndproc", hwnd, msg, wparam, lparam, || unsafe {
        wndproc_inner(hwnd, msg, wparam, lparam)
    })
}

unsafe fn wndproc_inner(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            // 背景一律自己畫（D2D），讓系統擦一次是白費工還會閃
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == ANIM_TIMER => {
                let done = SLIDE.with(|s| s.borrow().as_ref().map(|x| x.done()).unwrap_or(true));
                let _ = InvalidateRect(Some(hwnd), None, false);
                let _ = UpdateWindow(hwnd);
                if done {
                    let _ = KillTimer(Some(hwnd), ANIM_TIMER);
                    SLIDE.with(|s| *s.borrow_mut() = None);
                }
                LRESULT(0)
            }
            // **點到這裡不要搶焦點也不要把點擊傳下去**。
            // `WS_EX_NOACTIVATE` 只擋得住「視窗被啟用」，
            // 擋不住點擊讓宿主失去焦點（見候選視窗的說明）。
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_DESTROY => {
                RENDERER.with(|r| *r.borrow_mut() = None);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// 主題裡的邏輯像素換算成這個視窗實際要畫的像素。
///
/// **不能借候選視窗那個同名函式**——它讀的是候選視窗的 DPI。
/// 兩個視窗在不同螢幕上時（拖到副螢幕的瞬間）就會用錯縮放。
fn scaled(v: i32) -> i32 {
    v * DPI.with(|d| d.get()) / BASE_DPI
}

/// 這一列有多高（像素，已含 DPI）。上下各一個內距加一行字。
///
/// **候選視窗要靠它決定自己擺在哪**，所以獨立成函式，兩邊算的是同
/// 一份——各自推導的話兩塊會錯開幾像素，接縫處露出一條縫。
pub(crate) fn height_px(theme: &Theme, dpi: i32) -> i32 {
    let s = |v: i32| v * dpi / BASE_DPI;
    s(theme.metrics.padding()) * 2 + s(theme.metrics.line_height())
}

fn paint(hwnd: HWND) {
    unsafe {
        let mut ps = windows::Win32::Graphics::Gdi::PAINTSTRUCT::default();
        let _ = windows::Win32::Graphics::Gdi::BeginPaint(hwnd, &mut ps);
        // **繪製的 panic 不能傳出去**——一路往上穿過 COM／Win32
        // 邊界就是整個宿主崩潰。走 `guard` 而不是自己 `catch_unwind`：
        // 測試用的觸發點掛在那裡面，自己包一層就繞過去了
        crate::guard::guard("預覽列 paint", (), || paint_inner(hwnd));
        let _ = windows::Win32::Graphics::Gdi::EndPaint(hwnd, &ps);
    }
}

fn paint_inner(hwnd: HWND) {
    let theme = THEME.with(|t| t.borrow().clone());
    let (w, h) = unsafe {
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        (
            (rc.right - rc.left).max(1) as f32,
            (rc.bottom - rc.top).max(1) as f32,
        )
    };

    RENDERER.with(|r| {
        let mut slot = r.borrow_mut();
        let Some(renderer) = slot.as_mut() else {
            return;
        };
        // 文字變長變短時視窗寬度會變，surface 要跟著重建
        if renderer.resize(w as u32, h as u32).is_err() {
            return;
        }
        let dpi = DPI.with(|d| d.get()) as f32;
        let Ok(frame) = renderer.begin() else {
            return;
        };
        let c = &theme.colors;
        let pad = scaled(theme.metrics.padding()) as f32;
        let line_h = scaled(theme.metrics.line_height()) as f32;
        let radius = scaled(theme.metrics.corner_radius()) as f32;
        // **底下接著候選視窗時下緣畫直角**——兩塊要貼得起來。
        // 各自圓角的話接縫處會露出兩個內凹的小缺口。
        let joined = HAS_LIST_BELOW.with(|x| x.get());
        let rc = Rect::new(0.0, 0.0, w, h);
        if joined {
            frame.fill_top_round_gradient(rc, radius, c.preview_bg, c.preview_bg2, 1.0);
        } else {
            frame.fill_round_gradient(rc, radius, c.preview_bg, c.preview_bg2, 1.0);
        }

        // **固定一行，太長就捲動顯示尾端**。
        //
        // 使用者關心的是剛打的字，不是句子開頭。換行的話視窗會
        // 忽高忽低，打字時很干擾。
        TEXT.with(|t| {
            let text = t.borrow();
            if text.is_empty() {
                return;
            }
            let Ok(font) = renderer.text_format_nowrap(
                &theme.font.family,
                theme.metrics.font_size_pt() as f32,
                dpi,
            ) else {
                return;
            };
            // **反白該畫成什麼樣，一律問 `core`**——設定頁的預覽問
            // 同一份，兩邊才不會走鐘
            let hl = ime_core::render::highlight_paint(
                theme.metrics.highlight_style,
                c.highlight_bg.to_rgb(),
                c.highlight_text.to_rgb(),
                c.preview_text.to_rgb(),
            );
            let avail = w - pad * 2.0;
            let gap = preview_gap(BOX_RANGE.with(|bx| bx.borrow().is_some()));
            let cells = layout_preview(renderer, &text, &font, gap);
            let full_w = cells.last().map(|(_, _, x, cw)| x + cw).unwrap_or(0.0);
            // 反白框在**還沒捲動**時的位置——捲動量要看它才決定得了
            let box_span = BOX_RANGE.with(|bx| {
                let range = bx.borrow().clone()?;
                // 落在反白範圍內的那幾個字
                let first = cells.iter().find(|(a, _, _, _)| *a >= range.start)?;
                let last = cells.iter().rev().find(|(_, b, _, _)| *b <= range.end)?;
                // 左右各留半個間隙，框才會置中在空隙裡
                Some((first.2 - gap / 2.0, last.2 + last.3 + gap / 2.0))
            });

            let scroll = preview_scroll(full_w, avail, box_span);
            let marked = box_span.map(|(a, b)| (pad + a - scroll, pad + b - scroll));

            // 滑動動畫：`show` 說要滑的話這裡才建得出來
            // （到這一步才量得出位置）
            if ANIMATE.with(|a| a.replace(false)) {
                if let (Some(from), Some(to)) = (SPAN.with(|p| p.get()), marked) {
                    SLIDE.with(|s| {
                        let next =
                            SpanSlide::start(s.borrow().as_ref(), from, (to.0 as i32, to.1 as i32));
                        *s.borrow_mut() = Some(next);
                    });
                }
            }
            SPAN.with(|p| p.set(marked.map(|(a, b)| (a as i32, b as i32))));

            let bar = SLIDE
                .with(|s| {
                    s.borrow()
                        .as_ref()
                        .filter(|x| !x.done())
                        .map(|x| x.position())
                })
                .or(marked);

            // 底色先畫，字才不會被蓋掉
            if let Some((x0, x1)) = bar {
                let inset = scaled(1) as f32;
                frame.fill_highlight(
                    Rect::new(x0, pad + inset, x1, pad + line_h - inset),
                    radius / 2.0,
                    c.highlight_bg,
                    theme.metrics.highlight_style,
                    // **不畫上緣的光**——這一格很矮，加了會糊成一塊，
                    // 只留外框比較清楚
                    false,
                );
            }

            // **逐字畫**，位置用上面那份排版——含字間距，框才對得準。
            //
            // 反白那幾個字直接換色。換色的判斷用**終點範圍**而不是
            // 滑動中的位置，不然動畫途中會出現半個字變色。
            let hot_range = BOX_RANGE.with(|bx| bx.borrow().clone());
            for (a, b, x, cw) in &cells {
                let Some(ch) = text.get(*a..*b) else {
                    continue;
                };
                let left = pad + x - scroll;
                // 捲出可視範圍的就不用畫了
                if left + cw < 0.0 || left > w {
                    continue;
                }
                let hot = hot_range
                    .as_ref()
                    .is_some_and(|r| *a >= r.start && *b <= r.end);
                draw_label(
                    &frame,
                    &theme,
                    ch,
                    Rect::new(left, pad, left + cw, pad + line_h),
                    &font,
                    if hot {
                        Color::from(hl.text)
                    } else {
                        c.preview_text
                    },
                    1.0,
                );
            }
        });
        // frame 解構時自動送出
    });
}

/// 這一列該多寬：量整串文字，夾在主題的 min/max 之間。
///
/// **用 DirectWrite 量**，跟繪製同一個引擎。先前用 GDI 量的話，
/// 兩者每個字元的微小差異會累積——實測 58 個字元就差 35px，
/// 視窗因此不夠寬，長句子被推出可視範圍。
pub(crate) fn measure_width_px(theme: &Theme, text: &str, selecting: bool, dpi: i32) -> i32 {
    // 量之前要先設好 DPI——`scaled` 讀的是這個
    DPI.with(|d| d.set(dpi));
    measure_width(theme, text, selecting)
}

fn measure_width(theme: &Theme, text: &str, selecting: bool) -> i32 {
    let min_w = scaled(theme.metrics.min_width());
    let max_w = scaled(theme.metrics.max_width());
    MEASURER.with(|m| {
        let mut slot = m.borrow_mut();
        if slot.is_none() {
            *slot = TextMeasurer::new().ok();
        }
        let Some(meas) = slot.as_ref() else {
            return min_w;
        };
        let dpi = DPI.with(|d| d.get()) as f32;
        let Ok(fmt) = meas.format(&theme.font.family, theme.metrics.font_size_pt() as f32, dpi)
        else {
            return min_w;
        };
        // 選字時每個字左右各撐開半個間隙，視窗要跟著變寬才裝得下。
        // 少算的話內容溢出，不是頭被切就是尾被切。
        // 格數要跟 `layout_preview` 算的一致——那邊照叢集切，這裡也要，
        // 不然 emoji 那類多碼位的字會把視窗算得太寬
        let gap_total = preview_gap(selecting) * meas.clusters(text, &fmt).len() as f32;
        let w = meas.measure(text, &fmt, f32::MAX / 2.0).0 + gap_total;
        let pad = scaled(theme.metrics.padding()) * 2;
        (w.ceil() as i32 + pad).clamp(min_w, max_w)
    })
}

pub struct PreviewWindow {
    hwnd: HWND,
}

impl PreviewWindow {
    /// 顯示或更新預覽列。
    ///
    /// `box_range` 是要反白的那一段（位元組範圍），選字時用來標出
    /// 正在選哪一格。`None` 就不標。
    ///
    /// `has_list_below` 說底下有沒有接著候選視窗——有的話下緣畫直角。
    ///
    /// `pos` 是視窗左上角（螢幕座標），由呼叫端算好：兩個視窗要當成
    /// 一整塊來定位，不然螢幕邊界翻轉時會一個在上、一個在下。
    ///
    /// # 為什麼要「更新」而不是重建
    ///
    /// 每按一鍵都銷毀重建的話，中間那一瞬間畫面上什麼都沒有——
    /// **那就是使用者看到的閃爍**。視窗只建一次，之後只換內容。
    pub fn show(
        existing: Option<Self>,
        text: &str,
        box_range: Option<std::ops::Range<usize>>,
        has_list_below: bool,
        pos: (i32, i32),
        dpi: i32,
    ) -> Result<Self> {
        ensure_class_registered();
        DPI.with(|d| d.set(dpi));
        let theme = THEME.with(|t| t.borrow().clone());
        let width = measure_width(&theme, text, box_range.is_some());
        let height = height_px(&theme, dpi);

        // **要不要滑**：得在覆寫狀態前決定，因為要拿舊的來比。
        //
        // 只有「同一句話、反白從某一格移到另一格」才滑。文字換了
        // （又打了一個字）就直接跳——那時整串都在動，滑反而怪。
        let same_text = existing.is_some() && TEXT.with(|t| *t.borrow() == text);
        let prev_box = BOX_RANGE.with(|b| b.borrow().clone());
        ANIMATE.with(|a| {
            a.set(same_text && prev_box.is_some() && box_range.is_some() && prev_box != box_range)
        });
        if !same_text {
            // 換了句子，上一次的位置不再有參考價值
            SLIDE.with(|s| *s.borrow_mut() = None);
            SPAN.with(|p| p.set(None));
        }

        TEXT.with(|t| *t.borrow_mut() = text.to_string());
        BOX_RANGE.with(|b| *b.borrow_mut() = box_range);
        HAS_LIST_BELOW.with(|x| x.set(has_list_below));

        // 已經有視窗就沿用——這是不閃的關鍵
        if let Some(win) = existing {
            unsafe {
                let _ = SetWindowPos(
                    win.hwnd,
                    Some(HWND_TOPMOST),
                    pos.0,
                    pos.1,
                    width,
                    height,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
                let animating = SLIDE.with(|s| s.borrow().is_some()) || ANIMATE.with(|a| a.get());
                if animating {
                    SetTimer(Some(win.hwnd), ANIM_TIMER, ANIM_INTERVAL, None);
                } else {
                    let _ = KillTimer(Some(win.hwnd), ANIM_TIMER);
                }
                // 內容換過了，要求重畫。`InvalidateRect` 只是排進佇列，
                // TSF 按鍵處理是同步的、訊息迴圈沒在跑，所以還要
                // `UpdateWindow` 直接送 WM_PAINT。
                let _ = InvalidateRect(Some(win.hwnd), None, false);
                let _ = UpdateWindow(win.hwnd);
            }
            return Ok(win);
        }

        let hwnd = unsafe {
            CreateWindowExW(
                // **`WS_EX_NOREDIRECTIONBITMAP` 是 DComp 的前提**——
                // 沒有它的話系統會準備一張不透明的重導向點陣圖，
                // 把合成器畫的內容蓋掉（看起來就是全黑）。
                WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
                CLASS_NAME,
                w!(""),
                WS_POPUP,
                pos.0,
                pos.1,
                width,
                height,
                None,
                None,
                None,
                None,
            )?
        };

        RENDERER.with(|r| {
            if r.borrow().is_none() {
                if let Ok(rend) = Renderer::new(hwnd, width as u32, height as u32) {
                    *r.borrow_mut() = Some(rend);
                }
            }
        });
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = UpdateWindow(hwnd);
        }
        Ok(Self { hwnd })
    }
}

impl Drop for PreviewWindow {
    fn drop(&mut self) {
        // **繪圖器要在視窗之前丟**——它持有這個 hwnd 的合成目標，
        // 視窗先沒了的話那些 COM 物件會指向不存在的視窗
        RENDERER.with(|r| *r.borrow_mut() = None);
        MEASURER.with(|m| *m.borrow_mut() = None);
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
