//! 工作列狀態按鈕的右鍵選單（自繪）。
//!
//! # 為什麼要自己畫
//!
//! Win11 的輸入指示器**不提供選單 API**——`ITfLangBarItemButton::InitMenu`
//! 從來不會被呼叫（實測確認），微軟日文那個圓角選單是系統內建寫死的 UI。
//! 有開發者在微軟官方論壇問過同樣的問題，至今無解。
//!
//! 新酷音與小狼毫的解法是 `TrackPopupMenu` 彈傳統 Win32 選單，穩但外觀
//! 方正，跟候選視窗的圓角風格對不起來。本專案已經有 Direct2D 繪圖層，
//! 自己畫才能跟候選視窗用同一套主題。
//!
//! 可行性先做過雛形驗證（見開發文件 §3.7）：右鍵時彈得出視窗、點別處
//! 關得掉。過程中踩到一個 Rust 特有的坑，記在 `close()` 上面。
//!
//! # 這個視窗的特殊處境
//!
//! 它**不接受鍵盤焦點**（`WS_EX_NOACTIVATE`）——搶走焦點的話宿主會以為
//! 自己失焦而把插入點藏起來。代價是收不到按鍵，所以沒有 Esc 關閉；
//! 關閉靠 `SetCapture` 攔截視窗外的點擊。

use windows::core::{w, Result, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::UpdateWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, LoadCursorW, RegisterClassW,
    SetWindowPos, ShowWindow, CS_DROPSHADOW, CS_HREDRAW, CS_VREDRAW, HWND_TOPMOST, IDC_ARROW,
    MA_NOACTIVATEANDEAT, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_SHOWNOACTIVATE, WM_DESTROY,
    WM_ERASEBKGND, WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_PAINT, WM_RBUTTONDOWN,
    WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOPMOST, WS_POPUP,
};

use ime_core::language::Language;

use crate::d2d::{Rect, Renderer, TextMeasurer};
use crate::theme::Theme;

const CLASS_NAME: PCWSTR = w!("Tsunagi.LangMenu");
const BASE_DPI: i32 = 96;

/// 選單項目的高度（邏輯像素）。比候選列矮一點——選單項目沒有
/// 編號那一欄，塞太高會顯得空。
const ITEM_HEIGHT: i32 = 26;
/// 分隔線佔的高度
const SEPARATOR_HEIGHT: i32 = 9;
/// 打勾標記那一欄的寬度
const CHECK_WIDTH: i32 = 20;

/// 使用者在選單上選了什麼。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    /// 切到指定語言模式（`None` = 自動辨識）
    SetLock(Option<Language>),
    /// 開啟設定頁
    OpenSettings,
}

/// 一列選單。
#[derive(Debug, Clone)]
struct Item {
    label: &'static str,
    /// `None` 代表分隔線——不可點、不反白
    action: Option<MenuAction>,
    /// 前面要不要打勾（目前所在的模式）
    checked: bool,
}

impl Item {
    fn separator() -> Self {
        Self {
            label: "",
            action: None,
            checked: false,
        }
    }

    fn height(&self) -> i32 {
        if self.action.is_none() {
            SEPARATOR_HEIGHT
        } else {
            ITEM_HEIGHT
        }
    }
}

thread_local! {
    static MENU: std::cell::RefCell<Option<HWND>> = const { std::cell::RefCell::new(None) };
    static ITEMS: std::cell::RefCell<Vec<Item>> = const { std::cell::RefCell::new(Vec::new()) };
    /// 滑鼠停在第幾列。`None` = 沒停在任何可點的列上
    static HOVER: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    static THEME: std::cell::RefCell<Option<Theme>> = const { std::cell::RefCell::new(None) };
    static RENDERER: std::cell::RefCell<Option<Renderer>> = const { std::cell::RefCell::new(None) };
    static DPI: std::cell::Cell<i32> = const { std::cell::Cell::new(BASE_DPI) };
    /// 選了項目時要做什麼。`Box` 起來才存得進 thread_local
    #[allow(clippy::type_complexity)]
    static ON_PICK: std::cell::RefCell<Option<Box<dyn Fn(MenuAction)>>> =
        const { std::cell::RefCell::new(None) };
}

static REGISTER_CLASS: std::sync::Once = std::sync::Once::new();

fn ensure_class() {
    REGISTER_CLASS.call_once(|| unsafe {
        let wc = WNDCLASSW {
            // 陰影跟候選視窗一致，視覺才是同一套
            style: CS_HREDRAW | CS_VREDRAW | CS_DROPSHADOW,
            lpfnWndProc: Some(wndproc),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        RegisterClassW(&wc);
    });
}

fn scaled(v: i32) -> i32 {
    v * DPI.with(|d| d.get()) / BASE_DPI
}

/// 依設定組出選單內容。
///
/// **關掉的語言不列出來**——輪替時本來就會跳過它（見開發文件 §2.9），
/// 留在選單裡會變成選了沒反應的死項目。「自動辨識」永遠在，它不是
/// 某個引擎而是預設狀態；英文也永遠在（瀑布的最後一站，關不掉）。
fn build_items(lock: Option<Language>, engines: &ime_core::config::Engines) -> Vec<Item> {
    let mut items: Vec<Item> = Vec::new();
    for (label, l) in [
        ("自動辨識", None),
        ("注音", Some(Language::Bopomofo)),
        ("日文", Some(Language::Romaji)),
        ("英文", Some(Language::English)),
    ] {
        if let Some(lang) = l {
            if !engines.enabled(lang) {
                continue;
            }
        }
        items.push(Item {
            label,
            action: Some(MenuAction::SetLock(l)),
            checked: lock == l,
        });
    }
    items.push(Item::separator());
    items.push(Item {
        label: "設定…",
        action: Some(MenuAction::OpenSettings),
        checked: false,
    });
    items
}

/// 在 `pt`（螢幕座標）彈出選單。
///
/// 工作列在螢幕底部，所以選單畫在點擊位置的**上方**——往下畫會被切掉。
pub fn show(
    pt: POINT,
    lock: Option<Language>,
    engines: &ime_core::config::Engines,
    theme: Theme,
    on_pick: impl Fn(MenuAction) + 'static,
) -> Result<()> {
    ensure_class();
    close();

    DPI.with(|d| d.set(crate::candidate_window::dpi_at(pt)));
    let items = build_items(lock, engines);

    // ── 算視窗大小：量最長那一列的字寬 ──
    let pad = scaled(theme.metrics.padding());
    let dpi = DPI.with(|d| d.get()) as f32;
    let mut text_w = 0.0f32;
    if let Ok(meas) = TextMeasurer::new() {
        if let Ok(fmt) = meas.format(&theme.font.family, theme.metrics.font_size_pt() as f32, dpi) {
            for it in &items {
                if it.action.is_some() {
                    text_w = text_w.max(meas.measure(it.label, &fmt, f32::MAX).0);
                }
            }
        }
    }
    let w = (text_w.ceil() as i32 + scaled(CHECK_WIDTH) + pad * 2).max(scaled(120));
    let h: i32 = items.iter().map(|i| scaled(i.height())).sum::<i32>() + pad;

    let x = pt.x - w / 2;
    let y = pt.y - h - scaled(8);

    let hwnd = unsafe {
        CreateWindowExW(
            // `WS_EX_NOREDIRECTIONBITMAP` 是 DComp 的前提——沒有它
            // 系統會用一張不透明的重導向點陣圖蓋掉合成器畫的內容
            WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
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

    let renderer = Renderer::new(hwnd, w.max(1) as u32, h.max(1) as u32)?;
    RENDERER.with(|r| *r.borrow_mut() = Some(renderer));
    ITEMS.with(|i| *i.borrow_mut() = items);
    THEME.with(|t| *t.borrow_mut() = Some(theme));
    HOVER.with(|x| x.set(None));
    ON_PICK.with(|c| *c.borrow_mut() = Some(Box::new(on_pick)));
    MENU.with(|m| *m.borrow_mut() = Some(hwnd));

    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            x,
            y,
            w,
            h,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let _ = UpdateWindow(hwnd);
        // **抓滑鼠**：點視窗外面也會收到訊息，才知道該關了。
        // 這個視窗不接受鍵盤焦點，所以沒有 Esc 可用，全靠這個。
        SetCapture(hwnd);
    }
    paint(hwnd);
    Ok(())
}

/// 關掉選單。
pub fn close() {
    // **借用一定要先放掉再 `DestroyWindow`**。
    //
    // `DestroyWindow` 會**同步**送出 `WM_DESTROY`，那個處理函式又要借
    // 同一份 `MENU`——`RefCell` 同時借兩次會 panic，而 Rust 的 panic
    // 一穿過 COM 邊界就是整個宿主行程崩潰（記事本、瀏覽器直接關掉）。
    // 雛形第一次實測就是這樣把程式弄掛的。
    //
    // 所以分兩步：先把 handle 取出來讓借用歸還，再動手銷毀。
    let hwnd = MENU.with(|m| m.borrow_mut().take());
    if let Some(hwnd) = hwnd {
        unsafe {
            let _ = ReleaseCapture();
            let _ = DestroyWindow(hwnd);
        }
    }
    RENDERER.with(|r| *r.borrow_mut() = None);
    ITEMS.with(|i| i.borrow_mut().clear());
    ON_PICK.with(|c| *c.borrow_mut() = None);
}

/// 這個 y 座標落在第幾列？分隔線與空白處回 `None`。
fn item_at(y: i32) -> Option<usize> {
    let pad = ITEMS.with(|_| {
        scaled(THEME.with(|t| {
            t.borrow()
                .as_ref()
                .map(|x| x.metrics.padding())
                .unwrap_or(8)
        }))
    }) / 2;
    ITEMS.with(|items| {
        let mut top = pad;
        for (i, it) in items.borrow().iter().enumerate() {
            let hgt = scaled(it.height());
            if y >= top && y < top + hgt {
                return it.action.is_some().then_some(i);
            }
            top += hgt;
        }
        None
    })
}

fn paint(hwnd: HWND) {
    let Some(theme) = THEME.with(|t| t.borrow().clone()) else {
        return;
    };
    let c = &theme.colors;
    let (w, h) = unsafe {
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        (
            (rc.right - rc.left).max(1) as f32,
            (rc.bottom - rc.top).max(1) as f32,
        )
    };
    let dpi = DPI.with(|d| d.get()) as f32;
    let pad = scaled(theme.metrics.padding()) as f32;
    let radius = scaled(theme.metrics.corner_radius()) as f32;
    let hover = HOVER.with(|x| x.get());

    RENDERER.with(|r| {
        let mut slot = r.borrow_mut();
        let Some(renderer) = slot.as_mut() else {
            return;
        };
        let Ok(font) = renderer.text_format_nowrap(
            &theme.font.family,
            theme.metrics.font_size_pt() as f32,
            dpi,
        ) else {
            return;
        };
        let Ok(frame) = renderer.begin() else {
            return;
        };

        // 底：跟候選視窗同一套漸層
        frame.fill_round_gradient(
            Rect::new(0.0, 0.0, w, h),
            radius,
            c.window_bg,
            c.window_bg2,
            1.0,
        );

        ITEMS.with(|items| {
            let mut top = pad / 2.0;
            for (i, it) in items.borrow().iter().enumerate() {
                let hgt = scaled(it.height()) as f32;
                match it.action {
                    // 分隔線
                    None => {
                        let y = top + hgt / 2.0;
                        frame.fill_rect(Rect::new(pad, y, w - pad, y + 1.0), c.separator, 1.0);
                    }
                    Some(_) => {
                        let hot = hover == Some(i);
                        if hot {
                            frame.fill_highlight(
                                Rect::new(pad / 2.0, top, w - pad / 2.0, top + hgt),
                                radius / 2.0,
                                c.highlight_bg,
                                theme.metrics.highlight_style,
                                true,
                            );
                        }
                        // 反白時的字色一律問 `core`，跟候選視窗同一份決策
                        let paint = ime_core::render::highlight_paint(
                            theme.metrics.highlight_style,
                            c.highlight_bg.to_rgb(),
                            c.highlight_text.to_rgb(),
                            c.text.to_rgb(),
                        );
                        let fg = if hot {
                            crate::theme::Color::from(paint.text)
                        } else {
                            c.text
                        };
                        // 打勾：用小圓點而不是「✓」——字型不一定有那個字
                        if it.checked {
                            let cx = pad + scaled(CHECK_WIDTH) as f32 / 2.0;
                            let cy = top + hgt / 2.0;
                            let r = scaled(3) as f32;
                            frame.fill_round(Rect::new(cx - r, cy - r, cx + r, cy + r), r, fg, 1.0);
                        }
                        frame.draw_text(
                            it.label,
                            Rect::new(pad + scaled(CHECK_WIDTH) as f32, top, w - pad, top + hgt),
                            &font,
                            fg,
                            1.0,
                        );
                    }
                }
                top += hgt;
            }
        });
    });
}

/// **panic 不能穿過這裡**：`wndproc` 是 `extern "system"`，宿主的訊息
/// 迴圈直接呼叫它，unwinding 出去會讓整個宿主行程 abort。攔下來交給
/// `DefWindowProcW`——見 `crate::guard`。
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    crate::guard::wndproc("語言選單 wndproc", hwnd, msg, wp, lp, || unsafe {
        wndproc_inner(hwnd, msg, wp, lp)
    })
}

unsafe fn wndproc_inner(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        // 不要因為被點就搶走宿主焦點（同候選視窗的考量）
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATEANDEAT as isize),
        // D2D 全幅重畫，不需要系統先擦背景（擦了會閃）
        WM_ERASEBKGND => LRESULT(1),

        WM_PAINT => {
            paint(hwnd);
            // `BeginPaint`/`EndPaint` 由 D2D 取代，這裡只要把無效區清掉
            unsafe {
                let _ = windows::Win32::Graphics::Gdi::ValidateRect(Some(hwnd), None);
            }
            LRESULT(0)
        }

        WM_MOUSEMOVE => {
            let y = (lp.0 >> 16) as i16 as i32;
            let x = (lp.0 & 0xFFFF) as i16 as i32;
            // 有 SetCapture，視窗外的移動也會進來——先確認還在視窗內
            let inside = unsafe {
                let mut rc = RECT::default();
                let _ = GetClientRect(hwnd, &mut rc);
                x >= 0 && y >= 0 && x < rc.right && y < rc.bottom
            };
            let next = if inside { item_at(y) } else { None };
            if HOVER.with(|h| h.get()) != next {
                HOVER.with(|h| h.set(next));
                paint(hwnd);
            }
            LRESULT(0)
        }

        WM_LBUTTONDOWN | WM_RBUTTONDOWN => {
            let y = (lp.0 >> 16) as i16 as i32;
            let x = (lp.0 & 0xFFFF) as i16 as i32;
            let inside = unsafe {
                let mut rc = RECT::default();
                let _ = GetClientRect(hwnd, &mut rc);
                x >= 0 && y >= 0 && x < rc.right && y < rc.bottom
            };
            let picked = if inside {
                item_at(y).and_then(|i| ITEMS.with(|it| it.borrow().get(i).and_then(|x| x.action)))
            } else {
                None
            };
            // **先把回呼取出來、關掉視窗，最後才執行動作**。
            // 動作會回頭動 `text_service` 的狀態（甚至再次重畫語言列），
            // 在選單還活著的時候執行容易繞回來造成重入。
            let cb = ON_PICK.with(|c| c.borrow_mut().take());
            close();
            if let (Some(cb), Some(a)) = (cb, picked) {
                cb(a);
            }
            LRESULT(0)
        }

        WM_DESTROY => {
            // `try_borrow_mut`：借不到就算了，不要 panic（見 `close()`）
            MENU.with(|m| {
                if let Ok(mut b) = m.try_borrow_mut() {
                    *b = None;
                }
            });
            LRESULT(0)
        }

        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ime_core::config::Engines;

    fn 引擎(bopomofo: bool, romaji: bool) -> Engines {
        Engines { bopomofo, romaji }
    }

    fn 標籤(items: &[Item]) -> Vec<&'static str> {
        items
            .iter()
            .filter(|i| i.action.is_some())
            .map(|i| i.label)
            .collect()
    }

    #[test]
    fn 全開時四種語言都在() {
        let items = build_items(None, &引擎(true, true));
        assert_eq!(
            標籤(&items),
            vec!["自動辨識", "注音", "日文", "英文", "設定…"]
        );
    }

    #[test]
    fn 關掉的語言不列出來() {
        // 輪替時本來就跳過它，留在選單裡會變成選了沒反應的死項目
        let items = build_items(None, &引擎(true, false));
        assert_eq!(標籤(&items), vec!["自動辨識", "注音", "英文", "設定…"]);
    }

    #[test]
    fn 兩個都關還有自動與英文() {
        let items = build_items(None, &引擎(false, false));
        assert_eq!(標籤(&items), vec!["自動辨識", "英文", "設定…"]);
    }

    #[test]
    fn 目前的模式會打勾且只有一個() {
        for lock in [None, Some(Language::Bopomofo), Some(Language::English)] {
            let items = build_items(lock, &引擎(true, true));
            let checked: Vec<_> = items.iter().filter(|i| i.checked).collect();
            assert_eq!(checked.len(), 1, "{lock:?} 應該只有一項打勾");
            assert_eq!(checked[0].action, Some(MenuAction::SetLock(lock)));
        }
    }

    #[test]
    fn 設定那項不會被打勾() {
        let items = build_items(None, &引擎(true, true));
        let settings = items
            .iter()
            .find(|i| i.action == Some(MenuAction::OpenSettings))
            .unwrap();
        assert!(!settings.checked);
    }

    #[test]
    fn 分隔線不可點() {
        let items = build_items(None, &引擎(true, true));
        let seps: Vec<_> = items.iter().filter(|i| i.action.is_none()).collect();
        assert_eq!(seps.len(), 1, "語言與設定之間要有一條分隔線");
        assert_eq!(seps[0].height(), SEPARATOR_HEIGHT);
    }
}
