//! 候選視窗：一個不搶焦點的自繪 popup。
//!
//! **繪製用 Direct2D + DirectComposition**（2026-08-30 從 GDI 換過來），
//! 見 `d2d` 模組。換掉的只有「怎麼畫到螢幕上」——版面計算
//! （`layout`）、動畫（`slide`）、主題（`theme`）都沿用。
//!
//! 刻意不用 WS_EX_APPWINDOW / 一般 activate 流程──若視窗搶走焦點，
//! TSF 會把目前的 focus document 換成這個視窗的 context，
//! 打字對象就從原本的 App 換成候選視窗本身，組字會整個對不上。
//! 用 `WS_EX_NOACTIVATE` + `SW_SHOWNOACTIVATE` 讓它純粹當「畫面」。

use std::sync::Once;

use crate::d2d::{Rect, Renderer, TextMeasurer};
use crate::slide::{Cell, Slide, Span, SpanSlide};
use crate::theme::{Color, Theme};
use ime_core::Candidate;
use windows::core::{w, Result, BOOL, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, CreateFontW, InvalidateRect, UpdateWindow, CLEARTYPE_QUALITY, DEFAULT_CHARSET,
    DEFAULT_PITCH, FF_DONTCARE, FW_NORMAL, HFONT, OUT_DEFAULT_PRECIS,
};
use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::TextServices::{ITfContext, ITfRange};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetCursorPos, GetGUIThreadInfo,
    KillTimer, LoadCursorW, RegisterClassW, SetTimer, SetWindowPos, ShowWindow, CS_DROPSHADOW,
    CS_HREDRAW, CS_VREDRAW, GUITHREADINFO, HWND_TOPMOST, IDC_ARROW, MA_NOACTIVATE, SWP_NOACTIVATE,
    SWP_SHOWWINDOW, SW_SHOWNOACTIVATE, WM_DESTROY, WM_ERASEBKGND, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_NOACTIVATE,
    WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOPMOST, WS_POPUP,
};

const CLASS_NAME: PCWSTR = w!("UniversalIME.EchoCandidateWindow");

/// 反白條滑動動畫的計時器編號與間隔（毫秒）。
///
/// 跟全半形提示列同一個節奏（10ms ≈ 100fps）。計時器跑在宿主
/// 行程的訊息迴圈裡，滑完（約 0.1 秒）就停掉，不會一直佔著。
const ANIM_TIMER: usize = 1;
const ANIM_INTERVAL: u32 = 10;

/// 主題規格用的基準 DPI。Windows 的 100% 縮放是 96 DPI，
/// 主題裡的尺寸都以這個為基準，實際繪製時按螢幕 DPI 等比放大。
const BASE_DPI: i32 = 96;

static REGISTER_CLASS: Once = Once::new();

thread_local! {
    /// 預覽列的文字——組字當下的第一名切法。
    ///
    /// 組字區保持原始按鍵（打什麼顯示什麼），轉換結果放在這一列。
    /// 這樣打字時看到的是自己按了什麼，不會被逐字轉換干擾，
    /// 同時又看得到引擎的判斷。
    static PREVIEW: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };

    /// 預覽列裡要反白的那一段（在 `PREVIEW` 裡的位元組範圍）。
    ///
    /// 選字時要標出「正在選哪一格」。原本是把那一段用【】包起來，
    /// 但那會**改變文字本身**——預覽列的用意是「送出去會長這樣」，
    /// 混進不會送出的符號就不誠實了，而且中文全形括號很佔位置。
    /// 改成畫在文字上，文字保持原樣。
    ///
    /// 中間試過細外框，但小字級下不夠顯眼、得盯著找，
    /// 最後定案是跟候選清單同一組反白色（藍底白字）。
    static PREVIEW_BOX: std::cell::RefCell<Option<std::ops::Range<usize>>> =
        const { std::cell::RefCell::new(None) };

    /// 底部那行小字提示（例如「↑↑↓↓ 開啟設定」）。空字串就不畫。
    ///
    /// **不放進候選清單**——那裡是候選，提示是提示。混在一起的話
    /// 打 `config` 這個英文字時每次都會多一個不想選的東西。
    static HINT: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };


    /// 反白哪一列（候選清單的索引）。`None` 代表不反白。
    ///
    /// 切法選單要靠它——使用者按空白鍵時，反白條在清單裡上下跑，
    /// 而不是每按一次就整個清單重排。
    static SELECTED: std::cell::RefCell<Option<usize>> = const { std::cell::RefCell::new(None) };

    /// 預覽列反白塊正在滑動的話，這裡記著它的動畫。
    static PREVIEW_SLIDE: std::cell::RefCell<Option<SpanSlide>> =
        const { std::cell::RefCell::new(None) };

    /// 上一次**實際畫出來**的預覽列反白塊位置（左緣, 右緣）。
    ///
    /// 為什麼要記：位置得量字寬才知道，而量字寬要有 DC 和字型，
    /// 只有 `paint` 拿得到；但「要不要開始滑、從哪裡滑」是 `show`
    /// 在決定的。所以由 `paint` 把算好的位置存起來，`show` 下次
    /// 拿它當起點。
    static PREVIEW_SPAN: std::cell::Cell<Option<Span>> = const { std::cell::Cell::new(None) };

    /// 預覽列的反白這一輪要不要用滑的。
    ///
    /// `show` 判斷（同一句話、只是換了一格才滑），`paint` 執行——
    /// 因為只有 `paint` 量得出新位置在哪。
    static PREVIEW_ANIMATE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };

    /// 反白條正在滑動的話，這裡記著它的動畫；`None` 就直接畫在
    /// `SELECTED` 那一格。
    ///
    /// 只有「同一份清單、反白換了格」才會滑——清單換掉（左右換格
    /// 選字、展開全部）或反白第一次出現時直接跳，不然會從不相干
    /// 的位置滑過來。
    static SLIDE: std::cell::RefCell<Option<Slide>> = const { std::cell::RefCell::new(None) };

    /// 目前套用的主題。繪圖時取用的是角色名稱，不是寫死的數值。
    static THEME: std::cell::RefCell<Theme> = std::cell::RefCell::new(Theme::default());

    /// 這個視窗的 DPI。主題尺寸是邏輯像素，畫之前要按它放大。
    static DPI: std::cell::Cell<i32> = const { std::cell::Cell::new(BASE_DPI) };

    /// 分成幾欄。`1` 是一般的一直排，`>1` 是展開全部的多欄網格。
    ///
    /// 每欄獨立編號 1-9、向下數——使用者定的，跟日文 IME 的排法一致
    /// （九個是為了對齊數字鍵，見 `session::CHAR_PAGE`）。
    static COLUMNS: std::cell::Cell<usize> = const { std::cell::Cell::new(1) };

    /// 每一欄放幾個。
    static PER_COLUMN: std::cell::Cell<usize> = const { std::cell::Cell::new(9) };

    /// 每一列候選的高度。**不再等高**——長候選會換行佔好幾行。
    ///
    /// `show` 算好（那時要決定視窗多高），`paint` 拿來畫。兩邊共用
    /// 同一份才不會對不上，跟 `layout()` 是同一個道理。
    static ROW_HEIGHTS: std::cell::RefCell<Vec<i32>> = const { std::cell::RefCell::new(Vec::new()) };

    /// 只量文字、不畫圖的量測器。
    ///
    /// **視窗寬度要在建視窗之前算**，而 `Renderer` 要有視窗才建得
    /// 起來——所以量測獨立出來。用同一個引擎量與畫，長句子的寬度
    /// 才不會差到讓視窗不夠寬（先前用 GDI 量、DirectWrite 畫，
    /// 58 個字元就差了 35px）。
    static MEASURER: std::cell::RefCell<Option<TextMeasurer>> =
        const { std::cell::RefCell::new(None) };

    /// D2D 的繪圖環境。
    ///
    /// **建立要 130ms**（見 `d2d` 模組），所以視窗建好時建一次，
    /// 之後每幀重用。視窗銷毀時跟著清掉。
    static RENDERER: std::cell::RefCell<Option<Renderer>> =
        const { std::cell::RefCell::new(None) };
    /// 背景圖**解碼後的像素**。跟繪圖裝置無關，所以視窗開開關關都留著。
    ///
    /// 存「哪個路徑」配「解出什麼」：路徑用來比對（設定換圖才重解），
    /// 值是 `None` 代表試過但失敗（檔案不存在、格式壞掉），記下來才不會
    /// 一直重試讀檔。
    ///
    /// **解碼很慢（讀檔＋解壓縮），絕對不能跟著視窗生滅**——打字時候選
    /// 視窗開開關關，每次重解會讓宿主卡到沒有回應（實際發生過）。
    #[allow(clippy::type_complexity)]
    static BG_PIXELS: std::cell::RefCell<Option<(String, Option<(u32, u32, Vec<u8>)>)>> =
        const { std::cell::RefCell::new(None) };
    /// 背景圖交給 GPU 的那份。**綁在 `RENDERER` 上**——點陣圖是從那個
    /// 繪圖裝置建出來的，裝置沒了它就是懸空指標，拿去畫會記憶體違規
    /// （也實際發生過）。所以視窗銷毀時要跟 `RENDERER` 一起清。
    ///
    /// 重建很快：從已解碼的像素建，不必再讀檔。
    static BG_BITMAP: std::cell::RefCell<
        Option<windows::Win32::Graphics::Direct2D::ID2D1Bitmap1>,
    > = const { std::cell::RefCell::new(None) };
}

/// 換掉目前套用的主題。設定改過時呼叫。
pub fn set_theme(t: Theme) {
    THEME.with(|x| *x.borrow_mut() = t);
}

/// 把主題的邏輯像素換算成這個螢幕的實際像素。
fn scaled(v: i32) -> i32 {
    let dpi = DPI.with(|d| d.get());
    v * dpi / BASE_DPI
}

/// 依**像素高度**建立字型。
///
/// # 為什麼是像素而不是點
///
/// **這是目前唯一用 GDI 畫字的地方**（候選視窗走 DirectWrite），而它的
/// 呼叫端——全半形提示視窗——自己就算好了縮放：倍率是用「視窗實際寬度
/// ÷ 基準寬度」回推的，那個值天生含 DPI 與主題縮放。
///
/// 先前的版本收 pt，內部再乘這個模組的 thread-local `DPI`，於是**套了
/// 兩次不同來源的縮放**。症狀是「同一組參數，換一台電腦字就大小不對」
/// ——提示視窗先於候選視窗出現時，那個 thread-local 還停在預設的 96。
pub fn make_ui_font_px(family: &str, height_px: i32) -> HFONT {
    make_font_px(family, height_px)
}

fn make_font_px(family: &str, height_px: i32) -> HFONT {
    // 負值代表「字身高度」（不含 internal leading），正值是整個行高
    let height = -height_px;
    let mut name: Vec<u16> = family.encode_utf16().collect();
    name.push(0);
    unsafe {
        CreateFontW(
            height,
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
            PCWSTR(name.as_ptr()),
        )
    }
}

fn ensure_class_registered() {
    REGISTER_CLASS.call_once(|| unsafe {
        let wc = WNDCLASSW {
            // `CS_DROPSHADOW` 給 popup 一層系統畫的陰影——
            // 那是新注音那種「浮在文件上」的感覺的來源。
            style: CS_HREDRAW | CS_VREDRAW | CS_DROPSHADOW,
            lpfnWndProc: Some(wndproc),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        RegisterClassW(&wc);
    });
}

/// **panic 不能穿過這裡**：`wndproc` 是 `extern "system"`，宿主的訊息
/// 迴圈直接呼叫它，unwinding 出去會讓整個宿主行程 abort。攔下來交給
/// `DefWindowProcW`——見 `crate::guard`。
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    crate::guard::wndproc("候選視窗 wndproc", hwnd, msg, wparam, lparam, || unsafe {
        wndproc_inner(hwnd, msg, wparam, lparam)
    })
}

unsafe fn wndproc_inner(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                paint(hwnd);
                // **一定要把無效區標記為已處理**。
                //
                // 一般是靠 `BeginPaint`／`EndPaint` 順手做掉，但這裡改用
                // D2D，沒有那一對——不補這一行的話 Windows 會認為視窗
                // 還沒畫完，**立刻再送一次 `WM_PAINT`**，形成無窮迴圈：
                // CPU 燒滿、宿主「沒有回應」。
                //
                // 這個洞從換成 D2D 起就在（Phase 0 的 GDI 版本有
                // `BeginPaint`），一直沒被發現是因為那時每幀夠便宜、
                // 訊息迴圈還跟得上；等到繪製變重才壓垮。
                let _ = windows::Win32::Graphics::Gdi::ValidateRect(Some(hwnd), None);
                LRESULT(0)
            }
            // **不要讓系統擦背景**。
            //
            // 預設行為是先用類別的背景刷子把整個視窗塗白，再送 WM_PAINT。
            // 那一下塗白就是閃爍。
            //
            // 換成 D2D 之後更是完全不需要——合成器自己管緩衝，
            // 而且視窗是 `WS_EX_NOREDIRECTIONBITMAP`（沒有重導向點陣圖
            // 可以擦）。留著這一行是為了擋掉系統的預設行為。
            WM_ERASEBKGND => LRESULT(1),
            // 反白條滑動的每一幀。滑完就停掉計時器，之後只在按鍵時重畫。
            WM_TIMER if wparam.0 == ANIM_TIMER => {
                // 兩個動畫都跑完了才停計時器
                let cell_done =
                    SLIDE.with(|s| s.borrow().as_ref().map(|x| x.done()).unwrap_or(true));
                let span_done =
                    PREVIEW_SLIDE.with(|s| s.borrow().as_ref().map(|x| x.done()).unwrap_or(true));
                let scroll_done =
                    SCROLL_ANIM.with(|s| s.borrow().as_ref().map(|x| x.done()).unwrap_or(true));
                if scroll_done {
                    SCROLL_ANIM.with(|s| *s.borrow_mut() = None);
                }
                if cell_done {
                    SLIDE.with(|s| *s.borrow_mut() = None);
                }
                if span_done {
                    PREVIEW_SLIDE.with(|s| *s.borrow_mut() = None);
                }
                if cell_done && span_done && scroll_done {
                    let _ = KillTimer(Some(hwnd), ANIM_TIMER);
                }
                // 停掉的那一幀也要畫——把反白條收到終點的整數格
                let _ = InvalidateRect(Some(hwnd), None, false);
                let _ = UpdateWindow(hwnd);
                LRESULT(0)
            }
            // 點候選視窗時，明確告訴 Windows：**不要啟用我**。
            //
            // `WS_EX_NOACTIVATE` 只能防止視窗被啟用，擋不住點擊的副作用：
            // 交給 `DefWindowProcW` 預設處理的話，宿主 App 會認為自己失去了輸入
            // 焦點而把插入點藏起來，而我們又不接受啟用——結果兩邊都沒有插入點。
            //
            // **為什麼是 `MA_NOACTIVATE` 而不是 `MA_NOACTIVATEANDEAT`**：
            // 後者連點擊事件本身一起吐掉，那樣就永遠收不到 `WM_LBUTTONDOWN`，
            // 滑鼠選字做不了。改成只擋啟用、讓訊息進來，我們自己處理完就
            // 回傳，不交給 `DefWindowProcW`——宿主一樣不會被驚動。
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),

            // **滑鼠移到哪一列就淡淡反白哪一列**。沒有這個提示的話，
            // 使用者點下去之前不知道會點到哪。
            WM_MOUSEMOVE => {
                let x = (lparam.0 & 0xFFFF) as u16 as i16 as i32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
                // **拖曳中的滑鼠全歸捲軸**：這時不該再去反白候選列，
                // 使用者的手正忙著別的事
                if let Some(grab) = SCROLL_DRAG.with(|d| d.get()) {
                    scroll_to_pointer(x as f32, grab);
                    return LRESULT(0);
                }
                // 滑鼠在不在捲軸上——濃度會跟著變，告訴使用者它抓得動
                let on_bar = scrollbar_hit(x as f32, y as f32).is_some();
                if SCROLL_HOVER.with(|s| s.replace(on_bar)) != on_bar {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                let hit = candidate_at(x, y);
                if HOVER.with(|h| h.get()) != hit {
                    HOVER.with(|h| h.set(hit));
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                // **要主動要求「滑鼠離開」的通知**，Windows 不會自己送。
                // 少了它，滑鼠移出視窗之後反白會留在最後指著的那一列。
                let mut tme = windows::Win32::UI::Input::KeyboardAndMouse::TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<
                        windows::Win32::UI::Input::KeyboardAndMouse::TRACKMOUSEEVENT,
                    >() as u32,
                    dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::TrackMouseEvent(&mut tme);
                LRESULT(0)
            }
            // `windows` crate 沒有匯出這個常數，照 winuser.h 定義
            0x02A3 => {
                let had_hover = HOVER.with(|h| h.replace(None)).is_some();
                // 拖曳中不清——那時游標本來就常常在視窗外
                let on_bar = !SCROLL_DRAG.with(|d| d.get()).is_some()
                    && SCROLL_HOVER.with(|s| s.replace(false));
                if had_hover || on_bar {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                LRESULT(0)
            }

            // **滑鼠選字**。點到哪一列就選哪一個候選。
            //
            // 座標取自 `lparam` 的低／高 16 位元，是**客戶區**座標，
            // 剛好跟繪製時記下的列座標同一個座標系。
            //
            // 這裡不呼叫 `DefWindowProcW`——預設處理會去動焦點。
            WM_LBUTTONDOWN => {
                let x = (lparam.0 & 0xFFFF) as u16 as i16 as i32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
                // **先算完再呼叫**：回呼會一路做到重畫，那時會再borrow
                // 一次同樣的 thread-local，還握著就直接卡死
                // **捲軸優先**：它疊在清單下方，但按在它上面時
                // 使用者要的是捲動，不是選字
                if let Some(on_thumb) = scrollbar_hit(x as f32, y as f32) {
                    // 抓在滑塊上就記住抓的位置；點軌道空白處視為
                    // 「把滑塊中心搬過來」，接著就進入拖曳
                    let grab = if on_thumb {
                        SCROLL_THUMB
                            .with(|t| t.get())
                            .map(|(l, r)| {
                                let _ = r;
                                x as f32 - l
                            })
                            .unwrap_or(0.0)
                    } else {
                        SCROLL_THUMB
                            .with(|t| t.get())
                            .map(|(l, r)| (r - l) / 2.0)
                            .unwrap_or(0.0)
                    };
                    SCROLL_DRAG.with(|d| d.set(Some(grab)));
                    SCROLL_HOVER.with(|s| s.set(true));
                    // 手正在拖捲軸，候選列上那道「你會點到這裡」的
                    // 淡反白就是誤導，先清掉
                    HOVER.with(|h| h.set(None));
                    // **要抓住滑鼠**，不然游標移出視窗（很容易，那條很細）
                    // 之後就收不到移動訊息，滑塊卡在半路
                    windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(hwnd);
                    if !on_thumb {
                        scroll_to_pointer(x as f32, grab);
                    } else {
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                    return LRESULT(0);
                }
                let hit = candidate_at(x, y);
                if let Some(i) = hit {
                    let cb = ON_PICK.with(|p| p.borrow().clone());
                    if let Some(cb) = cb {
                        cb(i);
                    }
                }
                LRESULT(0)
            }
            // 放開就結束拖曳。**一定要 `ReleaseCapture`**——不放的話
            // 整個桌面的滑鼠都還被這個視窗抓著
            WM_LBUTTONUP => {
                if SCROLL_DRAG.with(|d| d.replace(None)).is_some() {
                    let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                LRESULT(0)
            }
            WM_DESTROY => LRESULT(0),
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// 點在 `(x, y)` 的是不是捲軸？回傳 `Some(true)` 代表正落在滑塊上。
///
/// 軌道的範圍是繪製時記下的（已經上下放寬過），所以這裡只做矩形比對。
fn scrollbar_hit(x: f32, y: f32) -> Option<bool> {
    let (l, t, r, b) = SCROLL_TRACK.with(|s| s.get())?;
    if x < l || x >= r || y < t || y >= b {
        return None;
    }
    let on_thumb = SCROLL_THUMB
        .with(|s| s.get())
        .is_some_and(|(tl, tr)| x >= tl && x < tr);
    Some(on_thumb)
}

/// 拖到 `x` 了——換算成欄號，跟上次不同才通知外面。
///
/// **相同就不通知**：拖一下會來十幾個 `WM_MOUSEMOVE`，每個都重算候選
/// 與重畫的話會卡頓，而且欄號根本沒變。
fn scroll_to_pointer(x: f32, grab: f32) {
    let Some((first, total)) = SCROLL.with(|s| s.get()) else {
        return;
    };
    let Some((l, _, r, _)) = SCROLL_TRACK.with(|s| s.get()) else {
        return;
    };
    let visible = COLUMNS.with(|c| c.get()).max(1).min(total.max(1));
    let bar_h = scaled(SCROLLBAR_H) as f32;
    let (_, travel) = scroll_thumb_metrics((r - l).max(1.0), bar_h, visible, total);
    let want = scroll_first_at(x, l, travel, grab, visible, total);
    if want == first {
        return;
    }
    let cb = ON_SCROLL.with(|p| p.borrow().clone());
    if let Some(cb) = cb {
        cb(want);
    }
}

/// 點在 `(x, y)` 的是第幾個候選？`rects` 是繪製時記下的每列座標。
///
/// 抽成純函式才測得起來——邊界（剛好落在分隔線上、點在清單外）
/// 用手測很難蓋全。
fn hit_test(rects: &[(f32, f32, f32, f32)], x: f32, y: f32) -> Option<usize> {
    rects
        .iter()
        .position(|(l, t, r, b)| x >= *l && x < *r && y >= *t && y < *b)
}

/// 點在候選視窗的 `(x, y)`（客戶區座標）上的是第幾個候選字。
pub fn candidate_at(x: i32, y: i32) -> Option<usize> {
    ROW_RECTS.with(|r| hit_test(&r.borrow(), x as f32, y as f32))
}

/// 註冊「使用者用滑鼠點了第幾個候選」的回呼。
///
/// 候選視窗只知道「點到第幾列」，該怎麼處理是 `text_service` 的事
/// （要看現在是切法選單還是選字模式）。用回呼把兩邊接起來，做法同
/// `lang_bar`。
///
/// **用 `Rc` 不用 `Box`**：呼叫前要先把它從 `RefCell` 裡複製出來，
/// 因為回呼跑起來會一路做到重畫，那時會再借用同一批 thread-local。
pub fn set_on_pick(f: std::rc::Rc<dyn Fn(usize)>) {
    ON_PICK.with(|p| *p.borrow_mut() = Some(f));
}

/// 使用者拖捲軸時要呼叫誰。參數是「可見的第一欄」該換成第幾欄。
pub fn set_on_scroll(f: std::rc::Rc<dyn Fn(usize)>) {
    ON_SCROLL.with(|p| *p.borrow_mut() = Some(f));
}

// 目前繪製中的候選清單；Phase 0 用最簡單的方式——存成 thread-local，
// 由 WM_PAINT 讀取。真正的產品應該用 GWLP_USERDATA 存 per-window 狀態。
/// 滑鼠點了候選、或拖了捲軸時要通知誰——參數是被點到的索引。
type IndexHandler = std::rc::Rc<dyn Fn(usize)>;

thread_local! {
    /// 滑鼠正指著第幾個候選。`None` = 沒指著任何一列。
    ///
    /// **候選換掉時要清掉**（見 `update`）——舊索引配新清單會反白到
    /// 不相干的那一列。
    static HOVER: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    /// 滑鼠點了候選字時要通知誰。見 `set_on_pick`。
    static ON_PICK: std::cell::RefCell<Option<IndexHandler>> =
        const { std::cell::RefCell::new(None) };
    /// 每個候選字佔的矩形（客戶區座標），繪製時記下來給滑鼠命中測試用。
    static ROW_RECTS: std::cell::RefCell<Vec<(f32, f32, f32, f32)>> =
        const { std::cell::RefCell::new(Vec::new()) };

    static CURRENT_CANDIDATES: std::cell::RefCell<Vec<Candidate>> = const { std::cell::RefCell::new(Vec::new()) };

    /// 展開時的橫向捲動狀態 `(可見的第一欄, 總欄數)`。`None` 不畫捲軸。
    static SCROLL: std::cell::Cell<Option<(usize, usize)>> = const { std::cell::Cell::new(None) };
    /// 捲軸軌道的矩形（客戶區座標），繪製時記下來給滑鼠命中測試用。
    static SCROLL_TRACK: std::cell::Cell<Option<(f32, f32, f32, f32)>> =
        const { std::cell::Cell::new(None) };
    /// 滑塊現在的左右緣（客戶區座標）。判斷「按在滑塊上還是軌道空白處」用。
    static SCROLL_THUMB: std::cell::Cell<Option<(f32, f32)>> =
        const { std::cell::Cell::new(None) };
    /// 拖曳中：`(按下時的滑鼠 x, 按下時滑塊左緣與滑鼠的距離)`。
    ///
    /// **記的是「抓在滑塊的哪個位置」**，不是按下時的欄號——用欄號的話
    /// 每次換算都要取整，拖久了會累積誤差，滑塊跟不上游標。
    static SCROLL_DRAG: std::cell::Cell<Option<f32>> = const { std::cell::Cell::new(None) };
    /// 滑鼠正指著捲軸嗎。指著就把滑塊畫濃一點，告訴使用者它抓得動。
    static SCROLL_HOVER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// 滑塊位置的滑動動畫（存 0～1 的比例，見 `ValueSlide`）。
    static SCROLL_ANIM: std::cell::RefCell<Option<crate::slide::ValueSlide>> =
        const { std::cell::RefCell::new(None) };
    /// 使用者拖了捲軸要通知誰。見 `set_on_scroll`。
    static ON_SCROLL: std::cell::RefCell<Option<IndexHandler>> =
        const { std::cell::RefCell::new(None) };
}

/// 捲軸那條的粗細與它跟候選清單之間的間隔（邏輯像素）。
///
/// **只有真的候選視窗會捲**，設定頁的預覽沒有這回事，所以不放進
/// `core` 的共用版面常數裡。
const SCROLLBAR_H: i32 = 4;
const SCROLLBAR_GAP: i32 = 5;

/// 捲軸多佔掉的高度（已縮放）。沒有捲軸就是 0。
///
/// **`show` 與 `paint` 都問這一份**——高度算多算少，畫出來的捲軸
/// 不是壓在提示列上就是浮在半空中。
fn scroll_extra(has_scroll: bool) -> i32 {
    if has_scroll {
        scaled(SCROLLBAR_H + SCROLLBAR_GAP)
    } else {
        0
    }
}

/// 垂直版面：候選清單從哪裡開始、整個視窗要多高。
///
/// **只有這一個地方算這件事**。原本 `paint` 與 `show` 各算各的，
/// 兩邊差了一個 `pad`——結果第一列候選往上侵入預覽列，字被壓掉
/// 一截。抽成純函式之後兩邊不可能再對不上，也測得到。
///
/// 版面由上而下：
///
/// ```text
///   pad          ┐ 預覽列（有的話）
///   line_h       │
///   pad          ┘  ← 這份內距就是它與候選清單的間隔
///   line_h × n      候選清單（最高那一欄的列數）
///   line_h          提示列（有的話）
///   pad             底部內距
/// ```
///
/// 沒有預覽列時，候選清單上面要自己補一份 `pad`。
///
/// **只有預覽列時**（打字中、還沒按空白）視窗就收緊成它自己那麼高，
/// 底下不留空——黃色底會填滿整個視窗，多留的話會露出一條白邊。
fn layout(pad: i32, line_h: i32, has_preview: bool, rows: i32, has_hint: bool) -> (i32, i32) {
    layout_with(pad, line_h, has_preview, line_h * rows, rows == 0, has_hint)
}

/// 同上，但候選清單的**總高度**直接給——因為每列不等高。
///
/// 長候選會換行佔好幾行，所以不能再用「行高 × 列數」算。
/// `list_h` 是所有列加起來的高度，`empty` 是「一列都沒有」。
fn layout_with(
    pad: i32,
    line_h: i32,
    has_preview: bool,
    list_h: i32,
    empty: bool,
    has_hint: bool,
) -> (i32, i32) {
    // **預覽列固定一行**——它不換行，長了就捲動顯示尾端。
    // 讓它變高的話打字時視窗會忽高忽低，很干擾。
    let list_top = if has_preview { pad * 2 + line_h } else { pad };
    if has_preview && empty && !has_hint {
        return (list_top, list_top);
    }
    // 什麼都沒有時也要留一列的高度，不然視窗會扁掉
    let list_h = if empty && !has_preview {
        line_h
    } else {
        list_h
    };
    let height = list_top + list_h + line_h * i32::from(has_hint) + pad;
    (list_top, height)
}

/// 畫一幀。**用 Direct2D**（原本是 GDI，見 `d2d` 模組的說明）。
///
/// 版面計算（`layout`）、動畫（`slide`）、主題（`theme`）都沿用——
/// 換掉的只有「怎麼把東西畫到螢幕上」這一層。
fn paint(hwnd: HWND) {
    // **繪製的 panic 不能傳出去**。這裡是從 `wndproc` 被叫的，panic
    // 一路往上穿過 COM／Win32 邊界就是整個宿主行程崩潰（使用者正在
    // 編輯的文件一起沒了）。畫不出來頂多是這一幀空白，比當掉好太多。
    //
    // `AssertUnwindSafe`：裡面用的是 thread_local 的 RefCell，panic
    // 之後那些狀態可能不一致——但下一幀會整個重畫，不會沿用。
    // 走 `guard` 而不是自己 `catch_unwind`：測試用的觸發點掛在那裡面，
    // 自己包一層就繞過去了
    crate::guard::guard("paint", (), || paint_inner(hwnd));
}

/// 滑鼠指著那一列的反白濃度。
///
/// **要淡到一眼分得出跟真正的反白不同**——太濃會讓使用者以為那格
/// 已經選中了。只是「你現在會點到這一列」的提示。
const HOVER_ALPHA: f32 = 0.28;

/// 一幀超過這個時間就記一筆。60fps 是 16ms，超過 30ms 已經明顯頓。
const SLOW_FRAME_MS: u128 = 30;

fn paint_inner(hwnd: HWND) {
    let t_frame = std::time::Instant::now();
    let theme = THEME.with(|t| t.borrow().clone());
    let (w, h) = unsafe {
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        (
            (rc.right - rc.left).max(1) as f32,
            (rc.bottom - rc.top).max(1) as f32,
        )
    };

    // **在借用繪圖器之前備妥背景圖的像素**——解碼會走 COM，
    // 那可能抽空跑訊息迴圈、回頭呼叫繪製，握著借用就會爆
    let t_px = std::time::Instant::now();
    ensure_background_pixels(&theme.background.image);
    let ms_px = t_px.elapsed().as_millis();

    RENDERER.with(|r| {
        let mut slot = r.borrow_mut();
        let Some(renderer) = slot.as_mut() else {
            return;
        };
        // 視窗大小變了要重建 surface（候選數不同高度就不同）
        if renderer.resize(w as u32, h as u32).is_err() {
            return;
        }
        let dpi = DPI.with(|d| d.get()) as f32;
        let Ok(font) =
            renderer.text_format(&theme.font.family, theme.metrics.font_size_pt() as f32, dpi)
        else {
            return;
        };
        // 提示列用小一點的字級——它是輔助資訊，不該搶走候選的注意力
        let hint_font = renderer.text_format(
            &theme.font.family,
            (theme.metrics.font_size_pt() * 4 / 5).max(7) as f32,
            dpi,
        );

        let x = PaintCtx::new(&theme, w, h, dpi);

        let Ok(frame) = renderer.begin() else {
            return;
        };

        paint_background(renderer, &frame, &x, t_frame, ms_px);
        paint_preview(renderer, &frame, &x);
        paint_candidates(renderer, &frame, &x, &font);
        paint_hint(&frame, &x, hint_font.as_ref().ok());
        paint_scrollbar(&frame, &x);
        // frame 解構時自動送出
    });
}

/// 一次繪製要用到的尺寸與反白配色。
///
/// 四個繪製段落共用這些值。**算一次傳下去**，各段自己再算一次的話，
/// 只要有一段的算法漂掉，畫面就會對不齊（預覽列跟候選清單錯開半格
/// 這種）。
struct PaintCtx<'a> {
    theme: &'a Theme,
    /// 視窗大小（像素，已含 DPI）
    w: f32,
    h: f32,
    /// 內縮、每列高、編號與候選字的間距、圓角半徑
    pad: f32,
    line_h: f32,
    gap: f32,
    radius: f32,
    dpi: f32,
    /// 候選清單的上緣——預覽列在不在會影響它
    list_top: f32,
    hl_style: ime_core::config::HighlightStyle,
    /// 候選列與預覽列的反白配色。**原色不同**（候選字色 vs 淺藍），
    /// 所以要分別算，不能共用一份
    hl_row: ime_core::render::HighlightPaint,
    hl_preview: ime_core::render::HighlightPaint,
}

impl<'a> PaintCtx<'a> {
    fn new(theme: &'a Theme, w: f32, h: f32, dpi: f32) -> Self {
        let c = &theme.colors;
        let pad = scaled(theme.metrics.padding()) as f32;
        let line_h = scaled(theme.metrics.line_height()) as f32;
        let hl_style = theme.metrics.highlight_style;
        let has_preview = PREVIEW.with(|p| !p.borrow().is_empty());
        let (list_top, _) = layout(pad as i32, line_h as i32, has_preview, 0, false);
        Self {
            theme,
            w,
            h,
            pad,
            line_h,
            gap: scaled(theme.metrics.index_gap()) as f32,
            radius: scaled(theme.metrics.corner_radius()) as f32,
            dpi,
            list_top: list_top as f32,
            hl_style,
            // **反白該畫成什麼樣，一律問 `core`**——設定頁的預覽問同一份，
            // 兩邊才不會走鐘（那類 bug 出現過三次，見 `ime_core::render`）。
            hl_row: ime_core::render::highlight_paint(
                hl_style,
                c.highlight_bg.to_rgb(),
                c.highlight_text.to_rgb(),
                c.text.to_rgb(),
            ),
            hl_preview: ime_core::render::highlight_paint(
                hl_style,
                c.highlight_bg.to_rgb(),
                c.highlight_text.to_rgb(),
                c.preview_text.to_rgb(),
            ),
        }
    }

    fn colors(&self) -> &crate::theme::Colors {
        &self.theme.colors
    }
}

/// 視窗底：圓角、背景圖、蓋在圖上的漸層。
///
/// 反鋸齒的圓角是換 D2D 的主要理由。
///
/// 有背景圖的話：**先畫圖，再把原本的漸層半透明蓋上去**。圖透出來
/// 有質感，而現有的配色系統照樣壓得住對比——不然深色圖上的深色字、
/// 亮處的白字都會消失。
///
/// `t_frame` / `ms_px` 只給慢幀的 log 用：這一段是整幀裡唯一會慢的
/// 地方（解碼與 GPU 上傳都在這），所以量測留在這裡。
fn paint_background(
    renderer: &crate::d2d::Renderer,
    frame: &crate::d2d::Frame,
    x: &PaintCtx,
    t_frame: std::time::Instant,
    ms_px: u128,
) {
    let theme = x.theme;
    let c = x.colors();
    let (w, h, radius) = (x.w, x.h, x.radius);

    let t_bmp = std::time::Instant::now();
    let bg = BG_BITMAP.with(|slot| {
        let mut b = slot.borrow_mut();
        if b.is_none() {
            // 點陣圖不在（第一次、或視窗重開過）就從像素重建。
            // 這一步很快——像素已經在記憶體裡了
            *b = BG_PIXELS.with(|px| {
                let px = px.borrow();
                px.as_ref()
                    .and_then(|(_, v)| v.as_ref())
                    .and_then(|(w, h, data)| renderer.bitmap_from_pixels(*w, *h, data).ok())
            });
        }
        b.clone()
    });
    let ms_bmp = t_bmp.elapsed().as_millis();
    let t_img = std::time::Instant::now();
    if let Some(img) = &bg {
        frame.fill_round_image(Rect::new(0.0, 0.0, w, h), radius, img, 1.0);
    }
    let ms_img = t_img.elapsed().as_millis();
    let ms_frame = t_frame.elapsed().as_millis();
    if ms_frame >= SLOW_FRAME_MS {
        crate::dlog!("[paint] 慢幀 {ms_frame}ms（解碼 {ms_px} / 建圖 {ms_bmp} / 畫圖 {ms_img}）");
    }
    frame.fill_round_gradient(
        Rect::new(0.0, 0.0, w, h),
        radius,
        c.window_bg,
        c.window_bg2,
        // 有圖時蓋薄一點讓圖透出來，濃度由使用者調。
        //
        // **要補償混色的色彩空間差異**——D2D 在 gamma 空間混色，
        // 同樣的數值會比設定頁的預覽暗一截（見
        // `dim_alpha_for_gamma_blend`）。
        if bg.is_some() {
            ime_core::render::dim_alpha_for_gamma_blend(theme.background.overlay_alpha())
        } else {
            1.0
        },
    );
}

/// 預覽列：正在組的那一串字，含反白框與滑動動畫。
fn paint_preview(renderer: &crate::d2d::Renderer, frame: &crate::d2d::Frame, x: &PaintCtx) {
    let theme = x.theme;
    let c = x.colors();
    let (w, h, pad, line_h, radius, dpi) = (x.w, x.h, x.pad, x.line_h, x.radius, x.dpi);
    let list_top = x.list_top;
    let hl_style = x.hl_style;
    let hl_preview = &x.hl_preview;

    // ── 預覽列 ──
    PREVIEW.with(|p| {
        let preview = p.borrow();
        if preview.is_empty() {
            return;
        }
        let row_bottom = pad * 2.0 + line_h;
        // 底下沒有候選清單時填到視窗底，並跟著視窗圓角收邊
        let alone = h <= list_top;
        if c.preview_bg != c.window_bg || c.preview_bg2 != c.preview_bg {
            let rc = Rect::new(0.0, 0.0, w, if alone { h } else { row_bottom });
            if alone {
                // 上下都要收邊——整個視窗就只有這一列
                frame.fill_round_gradient(rc, radius, c.preview_bg, c.preview_bg2, 1.0);
            } else {
                // **只收上緣**。畫成直角的話那兩個角會填滿視窗的圓角
                // 缺口，看起來像預覽列比下面的面板寬出去一截
                frame.fill_top_round_gradient(rc, radius, c.preview_bg, c.preview_bg2, 1.0);
            }
        }

        // **預覽列固定一行，太長就捲動顯示尾端**。
        //
        // 使用者關心的是剛打的字，不是句子開頭。換行的話視窗會
        // 忽高忽低，打字時很干擾。
        let Ok(pfont) = renderer.text_format_nowrap(
            &theme.font.family,
            theme.metrics.font_size_pt() as f32,
            dpi,
        ) else {
            return;
        };
        let avail = w - pad * 2.0;
        let gap = preview_gap(PREVIEW_BOX.with(|bx| bx.borrow().is_some()));
        let cells = layout_preview(renderer, &preview, &pfont, gap);
        let full_w = cells.last().map(|(_, _, x, cw)| x + cw).unwrap_or(0.0);
        // 反白框在**還沒捲動**時的位置——捲動量要看它才決定得了
        let box_span = PREVIEW_BOX.with(|bx| {
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
        if PREVIEW_ANIMATE.with(|a| a.replace(false)) {
            if let (Some(from), Some(to)) = (PREVIEW_SPAN.with(|p| p.get()), marked) {
                PREVIEW_SLIDE.with(|s| {
                    let next =
                        SpanSlide::start(s.borrow().as_ref(), from, (to.0 as i32, to.1 as i32));
                    *s.borrow_mut() = Some(next);
                });
            }
        }
        PREVIEW_SPAN.with(|p| p.set(marked.map(|(a, b)| (a as i32, b as i32))));

        let bar = PREVIEW_SLIDE
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
                hl_style,
                // **不畫上緣的光**——這一格很矮，加了會糊成一塊，
                // 只留外框比較清楚
                false,
            );
        }

        // **逐字畫**，位置用上面那份排版——含字間距，框才對得準。
        //
        // 反白那幾個字直接換色，不必像以前那樣整段畫完再重畫一次。
        // 換色的判斷用**終點範圍**而不是滑動中的位置，不然動畫途中
        // 會出現半個字變色。
        let hot_range = PREVIEW_BOX.with(|bx| bx.borrow().clone());
        for (a, b, x, cw) in &cells {
            let Some(ch) = preview.get(*a..*b) else {
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
                frame,
                theme,
                ch,
                Rect::new(left, pad, left + cw, pad + line_h),
                &pfont,
                if hot {
                    Color::from(hl_preview.text)
                } else {
                    c.preview_text
                },
                1.0,
            );
        }

        // 分隔線。只有預覽列時不畫——下面沒東西可分隔。
        if !alone {
            frame.fill_rect(
                Rect::new(0.0, row_bottom - 1.0, w, row_bottom),
                c.separator,
                1.0,
            );
        }
    });
}

/// 候選清單：反白條、滑鼠指著的那列、每一列的編號與候選字。
///
/// 順便收集命中測試要用的列座標——見函式內的說明。
fn paint_candidates(
    renderer: &crate::d2d::Renderer,
    frame: &crate::d2d::Frame,
    x: &PaintCtx,
    font: &windows::Win32::Graphics::DirectWrite::IDWriteTextFormat,
) {
    let theme = x.theme;
    let c = x.colors();
    let (w, pad, line_h, gap, radius) = (x.w, x.pad, x.line_h, x.gap, x.radius);
    let list_top = x.list_top;
    let hl_style = x.hl_style;
    let hl_row = &x.hl_row;

    // ── 候選清單 ──
    let selected = SELECTED.with(|s| *s.borrow());
    let per_col = PER_COLUMN.with(|x| x.get()).max(1);
    let n_cols = COLUMNS.with(|x| x.get()).max(1);
    let col_w = if n_cols > 1 { w / n_cols as f32 } else { w };

    // 反白條先鋪底色，字再畫上去
    let bar_at: Option<(f32, f32)> = SLIDE
        .with(|s| {
            s.borrow()
                .as_ref()
                .filter(|x| !x.done())
                .map(|x| x.position())
        })
        .or_else(|| selected.map(|i| ((i / per_col) as f32, (i % per_col) as f32)));
    // **每列高度不同**，所以位置要用累加的而不是「列號 × 行高」。
    // 滑動途中的小數列號用內插——在兩列之間時取兩者的加權。
    let heights = ROW_HEIGHTS.with(|r| r.borrow().clone());
    let row_top = |col: usize, row: f32| -> (f32, f32) {
        let base = col * per_col;
        let whole = row.floor() as usize;
        let frac = row - whole as f32;
        let mut y = list_top;
        for k in 0..whole {
            y += heights.get(base + k).copied().unwrap_or(line_h as i32) as f32;
        }
        let this_h = heights.get(base + whole).copied().unwrap_or(line_h as i32) as f32;
        let next_h = heights
            .get(base + whole + 1)
            .copied()
            .unwrap_or(line_h as i32) as f32;
        // 滑到一半時高度也跟著內插，反白塊才不會忽然變高
        (y + this_h * frac, this_h + (next_h - this_h) * frac)
    };

    if let Some((col, row)) = bar_at {
        let ci = col.round() as usize;
        let left = col * col_w;
        let (top, hgt) = row_top(ci, row);
        frame.fill_highlight(
            Rect::new(left, top, left + col_w, top + hgt),
            radius / 2.0,
            c.highlight_bg,
            hl_style,
            true,
        );
    }

    // **命中測試要用的列座標**。滑鼠點下去得知道點到第幾個候選，
    // 而那個答案必須跟畫出來的完全一致——所以不另外算一次，
    // 就收集繪製當下用的這一份。
    let mut hit_rects: Vec<(f32, f32, f32, f32)> = Vec::new();
    // **滑鼠指著的那一列**：比選中的那條淡很多，才不會跟真正的
    // 反白搞混。指著的就是選中的那一列時不重畫，疊上去會變深。
    let hovered = HOVER.with(|h| h.get());
    if let Some(hi) = hovered {
        let n = CURRENT_CANDIDATES.with(|c| c.borrow().len());
        if hi < n && Some(hi) != selected {
            let col = hi / per_col;
            let (top, row_h) = row_top(col, (hi % per_col) as f32);
            let left = col as f32 * col_w;
            frame.fill_round(
                Rect::new(left, top, left + col_w, top + row_h),
                radius / 2.0,
                c.highlight_bg,
                HOVER_ALPHA,
            );
        }
    }

    CURRENT_CANDIDATES.with(|cands| {
        for (i, cand) in cands.borrow().iter().enumerate() {
            let col = i / per_col;
            let row_in_col = i % per_col;
            let col_left = col as f32 * col_w;
            let (top, row_h) = row_top(col, row_in_col as f32);
            let hot = selected == Some(i);
            hit_rects.push((col_left, top, col_left + col_w, top + row_h));

            // **編號只畫在反白所在的那一欄**（使用者定的）。
            //
            // 數字鍵按下去挑的是「目前選中那一欄」的第幾個（見
            // `session::CHAR_COLUMN`）。每一欄都畫 1-9 的話，看起來
            // 像每一欄都能按，實際上只有一欄有用——那是會讓人按錯的
            // 假資訊。只有一欄時（一般狀態）當然照畫。
            let numbered = n_cols <= 1 || selected.map(|s| s / per_col) == Some(col);

            // **編號跟候選字同色**（使用者定的）。
            //
            // 原本編號用淡灰、反白時才跟文字同色——同一個東西在兩種
            // 狀態下換顏色，看起來像兩種資訊。整列同色單純得多。
            // **位置照留、只是不畫**——寬度仍用編號量。
            // 空字串會讓那一欄的候選字往左移，欄與欄就對不齊了。
            let label = format!("{}", row_in_col + 1);
            let row_color = if hot {
                Color::from(hl_row.text)
            } else {
                c.text
            };
            let index_color = row_color;
            if numbered {
                draw_label(
                    frame,
                    theme,
                    &label,
                    Rect::new(col_left + pad, top, col_left + col_w - pad, top + row_h),
                    font,
                    index_color,
                    1.0,
                );
            }
            let num_w = renderer.measure(&label, font);

            draw_label(
                frame,
                theme,
                &cand.text,
                Rect::new(
                    col_left + pad + num_w + gap,
                    top,
                    col_left + col_w - pad,
                    top + row_h,
                ),
                font,
                row_color,
                1.0,
            );
        }
    });
    ROW_RECTS.with(|r| *r.borrow_mut() = hit_rects);
}

/// 底部提示列。字級較小、顏色較淡——它是輔助資訊。
/// 捲軸滑塊的長度與它可以移動的距離（像素）。
///
/// 抽成純函式是為了測得到邊界：只捲一欄、欄數多到滑塊縮成一點、
/// 剛好裝得下不用捲——這幾種手測很難蓋全。
///
/// 滑塊長度＝看得到的欄數佔總欄數的比例，但**給一個最短長度**
/// （四倍粗細），不然三十幾欄時細到看不見也抓不到。
fn scroll_thumb_metrics(track_w: f32, bar_h: f32, visible: usize, total: usize) -> (f32, f32) {
    let ratio = (visible as f32 / total.max(1) as f32).clamp(0.0, 1.0);
    let thumb_w = (track_w * ratio).max(bar_h * 4.0).min(track_w);
    (thumb_w, (track_w - thumb_w).max(0.0))
}

/// 拖到 `x` 這個位置時，可見的第一欄該是第幾欄。
///
/// `grab` 是按下時抓在滑塊上的相對位置——**沒有它的話滑塊會跳到
/// 游標中心**，抓哪裡都一樣，手感很怪。
fn scroll_first_at(
    x: f32,
    track_left: f32,
    travel: f32,
    grab: f32,
    visible: usize,
    total: usize,
) -> usize {
    let max_first = total.saturating_sub(visible);
    if travel <= 0.0 || max_first == 0 {
        return 0;
    }
    let pos = ((x - grab - track_left) / travel).clamp(0.0, 1.0);
    (pos * max_first as f32).round() as usize
}

/// 底部那條橫向捲軸。
///
/// **它是唯一的線索**：展開之後畫面只畫十欄，不畫這條的話使用者
/// 沒有任何跡象知道右邊還有兩百多個字。
///
/// 位置走動畫（`SCROLL_ANIM`），鍵盤捲動時滑塊是滑過去的；
/// 拖曳中則直接跟手，中間插一段動畫反而像在拖橡皮筋。
fn paint_scrollbar(frame: &crate::d2d::Frame, x: &PaintCtx) {
    let Some((first, total)) = SCROLL.with(|s| s.get()) else {
        SCROLL_TRACK.with(|t| t.set(None));
        SCROLL_THUMB.with(|t| t.set(None));
        return;
    };
    let visible = COLUMNS.with(|c| c.get()).max(1).min(total.max(1));
    let c = x.colors();
    let (w, h, pad) = (x.w, x.h, x.pad);
    let bar_h = scaled(SCROLLBAR_H) as f32;
    // 提示列在的話捲軸讓到它上面——兩者都貼著底部，會疊在一起
    let has_hint = HINT.with(|hint| !hint.borrow().is_empty());
    let bottom = h - pad - if has_hint { x.line_h } else { 0.0 };
    let top = bottom - bar_h;
    let track = Rect::new(pad, top, w - pad, bottom);
    let track_w = (track.right - track.left).max(1.0);
    let (thumb_w, travel) = scroll_thumb_metrics(track_w, bar_h, visible, total);

    let hover = SCROLL_HOVER.with(|s| s.get());
    let dragging = SCROLL_DRAG.with(|d| d.get()).is_some();
    // 軌道：平常淡淡一條，滑鼠靠過來才明顯一點
    frame.fill_round(
        track,
        bar_h / 2.0,
        c.separator,
        if hover || dragging { 0.8 } else { 0.5 },
    );

    // 滑塊位置：動畫在跑就用它算到一半的值
    let max_first = total.saturating_sub(visible).max(1) as f32;
    let target = (first as f32 / max_first).clamp(0.0, 1.0);
    let pos = SCROLL_ANIM.with(|a| {
        a.borrow()
            .as_ref()
            .filter(|s| !s.done())
            .map(|s| s.value())
            .unwrap_or(target)
    });
    let left = track.left + travel * pos;
    let thumb = Rect::new(left, top, left + thumb_w, bottom);
    frame.fill_round(
        thumb,
        bar_h / 2.0,
        c.index,
        if hover || dragging { 1.0 } else { 0.85 },
    );

    // 命中測試要用**畫出來的**座標，不另外算一次（算兩次遲早會漂）。
    // 軌道往上下各放寬一點——四像素的細條用滑鼠精準點很痛苦。
    let grab_pad = bar_h * 1.5;
    SCROLL_TRACK.with(|t| {
        t.set(Some((
            track.left,
            top - grab_pad,
            track.right,
            bottom + grab_pad,
        )))
    });
    SCROLL_THUMB.with(|t| t.set(Some((thumb.left, thumb.right))));
}

fn paint_hint(
    frame: &crate::d2d::Frame,
    x: &PaintCtx,
    hint_font: Option<&windows::Win32::Graphics::DirectWrite::IDWriteTextFormat>,
) {
    let theme = x.theme;
    let c = x.colors();
    let (w, h, pad, line_h) = (x.w, x.h, x.pad, x.line_h);

    // ── 底部提示列 ──
    HINT.with(|hint| {
        let hint = hint.borrow();
        if hint.is_empty() {
            return;
        }
        let Some(hf) = hint_font else {
            return;
        };
        draw_label(
            frame,
            theme,
            &hint,
            Rect::new(pad, h - pad - line_h, w - pad, h - pad),
            hf,
            c.index,
            1.0,
        );
    });
}

/// 描邊的顏色。**固定黑色**——描邊的用途是把字跟背景隔開，
/// 用主題色的話遇到同色系背景就失效了。濃度由設定調。
const OUTLINE_COLOR: crate::theme::Color = crate::theme::Color::rgb(0, 0, 0);

/// 畫一段文字，依設定決定要不要描邊。
///
/// 五個呼叫處都走這裡，不然「描邊」這個開關會漏掉其中幾處。
fn draw_label(
    frame: &crate::d2d::Frame,
    theme: &Theme,
    text: &str,
    rc: Rect,
    fmt: &windows::Win32::Graphics::DirectWrite::IDWriteTextFormat,
    c: crate::theme::Color,
    alpha: f32,
) {
    let bg = &theme.background;
    if bg.outlined() {
        frame.draw_text_outlined(
            text,
            rc,
            fmt,
            c,
            alpha,
            OUTLINE_COLOR,
            bg.outline_alpha(),
            // 描邊寬度跟著 DPI 走，至少 1 像素——不然高 DPI 下細到看不見
            scaled(1).max(1) as f32,
        );
    } else {
        // **這裡一定要是 `frame.draw_text`，不能是 `draw_label`**——
        // 呼叫自己就是無限遞迴。release 編譯會把它最佳化成原地跳躍
        // （機器碼 `EB FE`），表現出來是宿主 CPU 燒滿、完全沒有回應。
        // 而且預設不描邊，所以每次畫字都會踩到。
        frame.draw_text(text, rc, fmt, c, alpha);
    }
}

/// 確保背景圖的**像素**已備妥（路徑變了才重新解碼）。
///
/// **要在借用繪圖器之前呼叫**：解碼會走 COM（WIC），而 COM 在 STA
/// 執行緒上可能抽空跑訊息迴圈，訊息迴圈可能又回頭呼叫繪製——那時
/// 若還握著繪圖器的借用就會爆掉。在借用外做就沒這個問題。
fn ensure_background_pixels(setting: &str) {
    let want = setting.trim().to_string();
    let changed = BG_PIXELS.with(|slot| {
        let mut b = slot.borrow_mut();
        if b.as_ref().map(|(k, _)| k.as_str()) == Some(want.as_str()) {
            return false; // 路徑沒變，已經解過了
        }
        let decoded =
            ime_core::config::resolve_image_path(&want, crate::registration::data_dir().as_deref())
                .and_then(|p| {
                    // **前後各記一筆**：log 停在「開始」就代表卡在解碼裡面
                    // （WIC 走 COM，在 STA 執行緒上有機會等訊息迴圈而死鎖）
                    crate::dlog!("[bg] 開始解碼 {p:?}");
                    let t = std::time::Instant::now();
                    let r = crate::d2d::decode_image(&p);
                    crate::dlog!(
                        "[bg] 解碼結束 {}ms 成功={}",
                        t.elapsed().as_millis(),
                        r.is_ok()
                    );
                    match r {
                        Ok(x) => Some(x),
                        Err(e) => {
                            crate::dlog!("[bg] 解碼背景圖失敗 {p:?}: {e:?}");
                            None
                        }
                    }
                });
        *b = Some((want, decoded));
        true
    });
    // **只在真的換了圖時才丟**。無條件丟的話等於每幀重建點陣圖，
    // 白白多做一次 GPU 上傳
    if changed {
        BG_BITMAP.with(|b| *b.borrow_mut() = None);
    }
}

/// 預覽列該往左捲多少。
///
/// 預設**看尾端**——使用者關心的是剛打的字，不是句子開頭。
///
/// 但**選字時要讓被選的那格留在畫面內**：只看尾端的話，往回選到前面
/// 的字時那一格會被推出左邊，使用者根本看不到自己在選什麼。
fn preview_scroll(full_w: f32, avail: f32, box_span: Option<(f32, f32)>) -> f32 {
    let max_scroll = (full_w - avail).max(0.0);
    let Some((x0, x1)) = box_span else {
        return max_scroll;
    };
    let mut s = max_scroll;
    // 先處理右邊超出，再處理左邊——框比可視範圍還寬時以**左緣**為準，
    // 至少看得到開頭
    if x1 > s + avail {
        s = x1 - avail;
    }
    if x0 < s {
        s = x0;
    }
    s.clamp(0.0, max_scroll)
}

/// 預覽列每個字的位置：`(起始位元組, 結束位元組, x, 寬)`。
///
/// # 為什麼要逐字算
///
/// 平常整串一次畫就好。但**選字時字之間要留空隙**——反白框的外框有
/// 厚度，貼著旁邊的字會擠成一團，留一個等於線寬的間隙剛好。
///
/// 有了間隙之後就不能再用「量子字串」推位置（那不含間隙）。量測與
/// 繪製都改走這一份，兩者才不會對不上——框畫歪一兩像素很顯眼。
/// 選字時字與字之間留多寬的間隙。
///
/// **只在選字時才有**——平常打字不該讓字忽然散開。寬度剛好等於反白
/// 框的線寬，框才不會貼著旁邊的字。
///
/// **量測與繪製共用這一份**：先前繪製有間隙、算視窗寬度時卻沒有，
/// 視窗因此少了 `間隙 × 字數` 那麼寬，一進選字內容就溢出被切掉。
fn preview_gap(selecting: bool) -> f32 {
    if selecting {
        scaled(ime_core::render::OUTLINE_WIDTH as i32).max(1) as f32
    } else {
        0.0
    }
}

fn layout_preview(
    renderer: &Renderer,
    text: &str,
    font: &windows::Win32::Graphics::DirectWrite::IDWriteTextFormat,
    gap: f32,
) -> Vec<(usize, usize, f32, f32)> {
    let mut out = Vec::new();
    let mut x = 0.0;
    let mut buf = [0u8; 4];
    for (i, ch) in text.char_indices() {
        let one = ch.encode_utf8(&mut buf);
        // **要前進寬度，不是 `measure`**：單獨一個空白在 `measure` 下
        // 量到 0（那個 API 不含尾端空白），排版就會把空白吃掉。
        let cw = renderer.measure_advance(one, font);
        out.push((i, i + ch.len_utf8(), x, cw));
        x += cw + gap;
    }
    out
}

/// 候選視窗該擺在哪（螢幕座標）。
///
/// `caret` 是組字文字在螢幕上的矩形，`work` 是那台螢幕的工作區
/// （扣掉工作列）。回傳視窗左上角。
///
/// 規則：
/// 1. **預設放在文字下方**——順著閱讀方向，不會擋住剛打的字
/// 2. 下方放不下就**翻到上方**。在畫面底部的輸入框打字時一定會遇到，
///    不翻的話視窗整個掉到螢幕外
/// 3. 上下都放不下（螢幕很矮）就貼著底部，至少看得到
/// 4. 水平方向超出右緣就往左推，但不推出左緣
///
/// 全部是純數值運算，所以測得起來——邊界情況（貼底、貼右、螢幕太矮）
/// 用手測很難蓋全。
fn place(caret: RECT, w: i32, h: i32, work: RECT) -> (i32, i32) {
    let below = caret.bottom;
    let above = caret.top - h;
    let y = if below + h <= work.bottom {
        below
    } else if above >= work.top {
        above
    } else {
        (work.bottom - h).max(work.top)
    };
    // `max` 放在 `min` 之後：視窗比螢幕寬時，寧可切右邊也要對齊左緣
    let x = caret.left.min(work.right - w).max(work.left);
    (x, y)
}

/// 這個螢幕位置的工作區（扣掉工作列）。查不到就退回整個虛擬桌面。
fn work_area_at(pt: POINT) -> RECT {
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFO};
    unsafe {
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(mon, &mut mi).as_bool() {
            return mi.rcWork;
        }
        // 保底：給一個夠大的範圍，至少不會把視窗夾到奇怪的位置
        RECT {
            left: -32768,
            top: -32768,
            right: 32767,
            bottom: 32767,
        }
    }
}

/// 這個螢幕位置的 DPI。
///
/// 用 `MonitorFromPoint` + `GetDpiForMonitor`——接了兩台不同縮放的
/// 螢幕時，候選視窗要跟著它出現的那一台走。查不到就退回 96。
///
/// **提示視窗也用這一份**：它原本把 DPI 寫死 96，在 125%／150% 的
/// 螢幕上會小一號。兩邊共用同一個算法才不會又走鐘。
pub fn dpi_at(pt: POINT) -> i32 {
    unsafe {
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut x = 0u32;
        let mut y = 0u32;
        if GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut x, &mut y).is_ok() && x > 0 {
            return x as i32;
        }
        BASE_DPI
    }
}

/// 算視窗該多寬：量最長那一行，夾在主題的 min/max 之間。
///
/// 為了量字寬要先有 DC 和字型，這裡借螢幕 DC（`GetDC(None)`）——
/// 視窗還沒建立，拿不到它自己的 DC。
/// 每一列候選在限定寬度內要佔多高（像素）。
///
/// **候選太長會換行**，那一列就不只一個行高。切法選單的候選常常是
/// 整個句子，一定會遇到。
///
/// 跟 `measure_width` 一樣用 DirectWrite——量與畫同一個引擎，
/// 換行的位置才會一致。
fn measure_row_heights(
    theme: &Theme,
    candidates: &[Candidate],
    col_w: i32,
    line_h: i32,
) -> Vec<i32> {
    MEASURER.with(|m| {
        let mut slot = m.borrow_mut();
        if slot.is_none() {
            *slot = TextMeasurer::new().ok();
        }
        let Some(meas) = slot.as_ref() else {
            return vec![line_h; candidates.len()];
        };
        let dpi = DPI.with(|d| d.get()) as f32;
        let Ok(fmt) = meas.format(&theme.font.family, theme.metrics.font_size_pt() as f32, dpi)
        else {
            return vec![line_h; candidates.len()];
        };
        let pad = scaled(theme.metrics.padding());
        let gap = scaled(theme.metrics.index_gap());
        // 編號那段佔掉的寬度（用兩位數估，那是最寬的情況）
        let num_w = meas.measure("10", &fmt, f32::MAX / 2.0).0;
        let avail = (col_w as f32 - pad as f32 * 2.0 - num_w - gap as f32).max(20.0);

        candidates
            .iter()
            .map(|c| {
                let h = meas.measure(&c.text, &fmt, avail).1;
                // 至少一個行高——短候選不該比一般列矮
                (h.ceil() as i32).max(line_h)
            })
            .collect()
    })
}

/// 算視窗該多寬：量最長那一行，夾在主題的 min/max 之間。
///
/// **用 DirectWrite 量**，跟繪製同一個引擎。先前用 GDI 量的話，
/// 兩者每個字元的微小差異會累積——實測 58 個字元就差 35px，
/// 視窗因此不夠寬，長句子被推出可視範圍。
fn measure_width(
    theme: &Theme,
    candidates: &[Candidate],
    preview: &str,
    hint: &str,
    selecting: bool,
) -> i32 {
    MEASURER.with(|m| {
        let mut slot = m.borrow_mut();
        if slot.is_none() {
            *slot = TextMeasurer::new().ok();
        }
        let Some(meas) = slot.as_ref() else {
            return scaled(theme.metrics.min_width());
        };
        let dpi = DPI.with(|d| d.get()) as f32;
        let Ok(fmt) = meas.format(&theme.font.family, theme.metrics.font_size_pt() as f32, dpi)
        else {
            return scaled(theme.metrics.min_width());
        };
        let wide = |t: &str| meas.measure(t, &fmt, f32::MAX / 2.0).0;

        // 選字時每個字左右各撐開半個間隙，視窗要跟著變寬才裝得下。
        // 少算的話內容溢出，不是頭被切就是尾被切。
        let gap_total = preview_gap(selecting) * preview.chars().count() as f32;
        let mut widest = (wide(preview) + gap_total).max(wide(hint));
        for (i, c) in candidates.iter().enumerate() {
            // 編號那段用的是小字，但用大字量是安全的高估
            widest = widest.max(wide(&format!("{}. {}", i + 1, c.text)));
        }

        let pad = scaled(theme.metrics.padding()) * 2 + scaled(theme.metrics.index_gap());
        (widest.ceil() as i32 + pad).clamp(
            scaled(theme.metrics.min_width()),
            scaled(theme.metrics.max_width()),
        )
    })
}

pub struct CandidateWindow {
    hwnd: HWND,
}

impl CandidateWindow {
    /// `preview` 是組字當下的第一名切法，畫在候選清單上方那一列。
    /// 傳空字串就不畫預覽列。
    ///
    /// `preview_box` 是預覽列裡要反白的那一段（位元組範圍），
    /// 選字時用來標出正在選哪一格。`None` 就不標。
    ///
    /// `selected` 是反白哪一列（切法選單／選字用），`None` 不反白。
    /// 顯示或更新候選視窗。
    ///
    /// # 為什麼要「更新」而不是重建
    ///
    /// 原本每按一鍵都是銷毀舊視窗、建一個新的，中間那一瞬間畫面上
    /// 什麼都沒有——**那就是使用者看到的閃爍**。
    ///
    /// 改成視窗只建一次，之後只換內容、調位置大小、觸發重畫。
    ///
    /// `per_column` 是每一欄放幾個。傳 `0` 或大於候選數時是一直排。
    /// `scroll` 是 `(可見的第一欄, 總欄數)`，展開到十欄裝不下時才有值
    /// ——底部會多畫一條捲軸，告訴使用者左右還有東西。
    /// `hint` 是底部那行小字（例如「↑↑↓↓ 開啟設定」），空字串不畫。
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        existing: Option<Self>,
        candidates: &[Candidate],
        preview: &str,
        preview_box: Option<std::ops::Range<usize>>,
        hint: &str,
        selected: Option<usize>,
        per_column: usize,
        scroll: Option<(usize, usize)>,
        caret: RECT,
    ) -> Result<Self> {
        ensure_class_registered();

        let anchor = POINT {
            x: caret.left,
            y: caret.bottom,
        };
        let theme = THEME.with(|t| t.borrow().clone());
        // 這個位置在哪個螢幕上？DPI 要按那個螢幕算，
        // 不然接了兩台不同縮放的螢幕時會一邊糊一邊太小。
        DPI.with(|d| d.set(dpi_at(anchor)));

        // 分欄：per_column 為 0 或候選裝得下一欄時就是一直排
        let per_col = if per_column == 0 {
            candidates.len().max(1)
        } else {
            per_column
        };
        let n_cols = candidates.len().div_ceil(per_col.max(1)).max(1);

        // **寬度依內容算**——固定寬度會把切法選單的整句切斷。
        // 多欄時每欄各自要那麼寬。
        let work = work_area_at(anchor);
        let mut col_w = measure_width(&theme, candidates, preview, hint, preview_box.is_some());
        // **總寬不得超過螢幕**：`max_width` 夾的是「一欄」，展開全部時
        // 欄數一多就相乘出去（三欄就是三倍），而 `place` 只會把視窗推到
        // 貼齊左緣、右邊直接被切掉。這裡把每欄等比壓回可用寬度。
        // 壓過頭也不再回夾 `min_width`——螢幕就這麼窄的話，寧可窄也不要看不到。
        let avail = (work.right - work.left).max(1);
        if col_w * n_cols as i32 > avail {
            col_w = (avail / n_cols as i32).max(1);
        }
        let width = col_w * n_cols as i32;

        let pad_px = scaled(theme.metrics.padding());
        let line_h = scaled(theme.metrics.line_height());
        // **每列高度各自算**——長候選會換行，不再等高
        let heights = measure_row_heights(&theme, candidates, col_w, line_h);
        // 高度看**最高的那一欄**：第 i 列落在第 (i/per_col) 欄，
        // 每欄各自加總，取最高的那一欄
        let mut col_sums = vec![0i32; n_cols];
        for (i, hgt) in heights.iter().enumerate() {
            col_sums[(i / per_col).min(n_cols - 1)] += hgt;
        }
        let list_h = col_sums.iter().copied().max().unwrap_or(0);
        let (_, height) = layout_with(
            pad_px,
            line_h,
            !preview.is_empty(),
            list_h,
            candidates.is_empty(),
            !hint.is_empty(),
        );
        // 捲軸自己那一條的高度加在最下面（`layout` 只管清單與提示列）
        let height = height + scroll_extra(scroll.is_some());
        ROW_HEIGHTS.with(|r| *r.borrow_mut() = heights);

        // **要不要滑**：得在覆寫狀態前決定，因為要拿舊的來比。
        //
        // 只有「視窗還在、清單一模一樣、反白從某格移到另一格」才滑。
        // 清單換掉（左右換格選字、展開全部）或反白第一次出現時
        // 直接跳到新位置——從不相干的位置滑過來反而誤導。
        // 反白沒動（按了個不影響選字的鍵）就讓還在跑的動畫繼續跑。
        let same_list = existing.is_some()
            && CURRENT_CANDIDATES.with(|c| *c.borrow() == candidates)
            && PER_COLUMN.with(|c| c.get()) == per_col;
        let prev_selected = SELECTED.with(|s| *s.borrow());
        let cell = |i: usize| -> Cell { (i / per_col, i % per_col) };
        SLIDE.with(|s| {
            let mut slot = s.borrow_mut();
            match (prev_selected, selected) {
                (Some(a), Some(b)) if same_list && a != b => {
                    let next = Slide::start(slot.as_ref(), cell(a), cell(b));
                    *slot = Some(next);
                }
                (Some(a), Some(b)) if same_list && a == b => {}
                _ => *slot = None,
            }
        });

        // **預覽列的反白要不要滑**：同一句話、標記換了一段才滑。
        //
        // 預覽文字變了就是重打或送出，跳過去；標記沒動就讓還在跑的
        // 動畫繼續跑完。實際的位置這裡量不出來（要有 DC 和字型），
        // 只放一個旗標，`paint` 量完新位置才真的建動畫。
        let same_text = existing.is_some() && PREVIEW.with(|p| *p.borrow() == preview);
        let prev_box = PREVIEW_BOX.with(|b| b.borrow().clone());
        let moved = match (&prev_box, &preview_box) {
            (Some(a), Some(b)) => a != b,
            _ => false,
        };
        if same_text && moved {
            PREVIEW_ANIMATE.with(|a| a.set(true));
        } else if !same_text || preview_box.is_none() {
            // 換句話或不再標記了：動畫與記住的位置一起作廢，
            // 不然下次會從一個不相干的位置滑過來
            PREVIEW_ANIMATE.with(|a| a.set(false));
            PREVIEW_SLIDE.with(|s| *s.borrow_mut() = None);
            PREVIEW_SPAN.with(|p| p.set(None));
        }

        // **換了候選就清掉滑鼠反白**：同一個索引在新清單裡是別的字，
        // 留著會反白到不相干的那一列。滑鼠再動一下就會重新亮起來。
        if !same_list {
            HOVER.with(|h| h.set(None));
        }
        CURRENT_CANDIDATES.with(|c| *c.borrow_mut() = candidates.to_vec());
        PREVIEW.with(|p| *p.borrow_mut() = preview.to_string());
        PREVIEW_BOX.with(|b| *b.borrow_mut() = preview_box);
        HINT.with(|h| *h.borrow_mut() = hint.to_string());
        SELECTED.with(|s| *s.borrow_mut() = selected);
        THEME.with(|t| *t.borrow_mut() = theme);
        // **滑塊要滑過去，不是跳過去**——但拖曳中直接跟手，
        // 中間插一段動畫會像在拖橡皮筋
        let prev_scroll = SCROLL.with(|c| c.get());
        SCROLL.with(|c| c.set(scroll));
        let dragging = SCROLL_DRAG.with(|d| d.get()).is_some();
        SCROLL_ANIM.with(|a| {
            let mut slot = a.borrow_mut();
            match scroll {
                Some((first, total)) if !dragging => {
                    let visible = n_cols.max(1);
                    let max_first = total.saturating_sub(visible).max(1) as f32;
                    let to = (first as f32 / max_first).clamp(0.0, 1.0);
                    // 起點：上一輪捲到哪。第一次展開就不用滑
                    let from = prev_scroll
                        .map(|(pf, _)| (pf as f32 / max_first).clamp(0.0, 1.0))
                        .unwrap_or(to);
                    let same_target = slot.as_ref().is_some_and(|x| x.target() == to);
                    if !same_target && from != to {
                        *slot = Some(crate::slide::ValueSlide::start(slot.as_ref(), from, to));
                    }
                }
                _ => *slot = None,
            }
        });
        COLUMNS.with(|c| c.set(n_cols));
        PER_COLUMN.with(|c| c.set(per_col));

        // **決定真正的位置**：預設在組字文字下方，螢幕底部放不下就
        // 翻到上方（見 `place`）。以前直接用 `anchor`，在畫面底部的
        // 輸入框打字時視窗會整個掉到螢幕外。
        let (pos_x, pos_y) = place(caret, width, height, work);

        // 已經有視窗就沿用——這是不閃的關鍵
        if let Some(win) = existing {
            unsafe {
                let _ = SetWindowPos(
                    win.hwnd,
                    Some(HWND_TOPMOST),
                    pos_x,
                    pos_y,
                    width,
                    height,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
                // 有動畫要跑就開計時器（重複呼叫 `SetTimer` 同一個編號
                // 只是重設，不會疊出第二個）；沒有就確保它是關的。
                let animating = SLIDE.with(|s| s.borrow().is_some())
                    || PREVIEW_SLIDE.with(|s| s.borrow().is_some())
                    || PREVIEW_ANIMATE.with(|a| a.get())
                    || SCROLL_ANIM.with(|s| s.borrow().is_some());
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
                pos_x,
                pos_y,
                width,
                height,
                None,
                None,
                None,
                None,
            )?
        };

        // **圓角交給 D2D 畫**，不再用 `SetWindowRgn` 裁——
        // 那個裁切邊緣有鋸齒，正是換掉 GDI 的理由之一。
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

impl Drop for CandidateWindow {
    fn drop(&mut self) {
        // 動畫狀態跟著視窗一起清掉——下一個視窗建起來時
        // 反白該直接出現，不是從上一個視窗的位置滑過來
        SLIDE.with(|s| *s.borrow_mut() = None);
        PREVIEW_SLIDE.with(|s| *s.borrow_mut() = None);
        // 繪圖環境綁在這個視窗上，視窗沒了它也不能用
        RENDERER.with(|r| *r.borrow_mut() = None);
        // **點陣圖也要一起清掉**——它是從上面那個繪圖裝置建出來的，
        // 裝置沒了就是懸空指標，下次開視窗拿它去畫會記憶體違規。
        //
        // 但**解碼後的像素留著**（`BG_PIXELS`）：那份跟裝置無關，
        // 下次要用時從記憶體重建點陣圖就好，不必再讀檔解碼。
        BG_BITMAP.with(|b| *b.borrow_mut() = None);
        PREVIEW_SPAN.with(|p| p.set(None));
        PREVIEW_ANIMATE.with(|a| a.set(false));
        unsafe {
            let _ = KillTimer(Some(self.hwnd), ANIM_TIMER);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// 拿不到組字範圍的螢幕座標時的退路：用前景視窗的 caret 位置。
///
/// **這只是退路，不是主要手段。** 正規做法是 `caret_screen_position_from_context()`，
/// 直接向 TSF 問組字範圍在螢幕上的位置。
///
/// 這裡用 `GetGUIThreadInfo(0, ...)`，那個 `0` 代表「目前執行線」——在輸入法
/// 被宿主行程載入的情況下，這通常就是宿主的 UI 執行線，所以單行程的
/// App（記事本）可以靠它蒙對；但多行程架構的 App（瀏覽器）就不一定了。
pub fn caret_screen_position_fallback() -> RECT {
    unsafe {
        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        if GetGUIThreadInfo(0, &mut info).is_ok() && !info.hwndCaret.is_invalid() {
            let mut tl = POINT {
                x: info.rcCaret.left,
                y: info.rcCaret.top,
            };
            let mut br = POINT {
                x: info.rcCaret.right,
                y: info.rcCaret.bottom,
            };
            let _ = ClientToScreen(info.hwndCaret, &mut tl);
            let _ = ClientToScreen(info.hwndCaret, &mut br);
            crate::dlog!(
                "[定位] 退路：GetGUIThreadInfo caret=({},{},{},{})",
                tl.x,
                tl.y,
                br.x,
                br.y
            );
            return RECT {
                left: tl.x,
                top: tl.y,
                right: br.x,
                bottom: br.y,
            };
        }
        // 最後的保底：滑鼠位置。這並非文字插入點，只是確保視窗不會
        // 出現在螢幕外面讓使用者找不到。高度給 0——沒有真的行高可用，
        // 需要往上翻時就貼著這個點翻。
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        crate::dlog!("[定位] 退路：只剩滑鼠位置 ({},{})", pt.x, pt.y);
        RECT {
            left: pt.x,
            top: pt.y,
            right: pt.x,
            bottom: pt.y,
        }
    }
}

/// 向 TSF 問「組字範圍在螢幕上的位置」，回傳候選視窗該靠的點（組字區左下角）。
///
/// 這才是正確的做法：由宿主告訴我們文字畫在哪，而不是去猜。不同 App
/// 的文字區可能在子視窗、另一個行程（瀏覽器）、甚至是自繪的畫布（Electron），
/// 靠 Win32 API 去推都不可靠。
///
/// 必須在 edit session 裡呼叫（需要 edit cookie）。
pub fn caret_screen_position_from_range(context: &ITfContext, ec: u32, range: &ITfRange) -> RECT {
    unsafe {
        let ok = (|| -> windows::core::Result<(RECT, BOOL)> {
            let view = context.GetActiveView()?;
            let mut rc = RECT::default();
            let mut clipped = BOOL(0);
            view.GetTextExt(ec, range, &mut rc, &mut clipped)?;
            // GetTextExt 回的已經是螢幕座標，不需再 ClientToScreen。
            //
            // **回整個矩形而不只是左下角**——候選視窗預設畫在下方，
            // 但螢幕底部放不下時要翻到上方，那就需要知道上緣在哪。
            //
            // `clipped` 一併帶出來只為了記 log：它代表「矩形被裁到可視
            // 範圍」，捲動容器或 overlay 裡的欄位（網頁搜尋欄）可能因此
            // 回到容器邊界而不是插入點。
            Ok((rc, clipped))
        })();
        match ok {
            // 空矩形代表宿主沒真的算出位置（有些 App 會回 S_OK 但給全零），
            // 這種情況要當成失敗走退路。
            Ok((rc, clipped)) if rc.left != 0 || rc.bottom != 0 => {
                crate::dlog!(
                    "[定位] GetTextExt ok rc=({},{},{},{}) clipped={}",
                    rc.left,
                    rc.top,
                    rc.right,
                    rc.bottom,
                    clipped.0
                );
                rc
            }
            Ok((rc, clipped)) => {
                crate::dlog!(
                    "[定位] GetTextExt 回空矩形 rc=({},{},{},{}) clipped={} → 退路",
                    rc.left,
                    rc.top,
                    rc.right,
                    rc.bottom,
                    clipped.0
                );
                screen_ext(context).unwrap_or_else(caret_screen_position_fallback)
            }
            Err(e) => {
                crate::dlog!(
                    "[定位] GetTextExt 失敗 hr={:#010x} {} → 退路",
                    e.code().0 as u32,
                    hresult_hint(e.code().0 as u32)
                );
                // **版面沒算好時退而求其次問整塊區域**，見 `screen_ext`。
                // 這條在網頁欄位幾乎一定成功，而最後那個「滑鼠位置」
                // 保底會讓視窗跟著滑鼠跑，是使用者實際回報的災情。
                screen_ext(context).unwrap_or_else(caret_screen_position_fallback)
            }
        }
    }
}

/// 把 `GetTextExt` 常見的失敗碼翻成看得懂的字，只給 log 用。
///
/// **`TF_E_NOLAYOUT` 是這裡最重要的一個**——它不是錯誤，是宿主說
/// 「版面還沒算好，現在問不到」。網頁的搜尋欄在打字當下常常正在重排，
/// 很容易碰到。
/// 問宿主「這塊文字區整體在螢幕的哪個矩形」。
///
/// # 為什麼需要這條路
///
/// 網頁的輸入欄（YouTube 搜尋欄是典型）常常在打字當下正在重排，這時
/// 問 `GetTextExt`（逐字的精確位置）會被回 `TS_E_NOLAYOUT`——那不是
/// 錯誤，是宿主說「版面還沒算好，答不出來」。
///
/// 但 `GetScreenExt` 問的是整塊區域，**不需要逐字版面**，這時候仍
/// 答得出來。本機實測 YouTube 搜尋欄回的是 1838×35 的矮扁矩形，正是
/// 搜尋框本身——候選視窗貼著它的下緣，比掉到滑鼠位置好太多。
///
/// **它不是精確解**：多行的編輯區（留言框、Word 內文）回的是整塊
/// 區域，貼的位置會是那塊的左下角而不是游標處。精確解是等宿主的
/// `OnLayoutChange` 通知再重問一次，那是另一件事。
pub fn screen_ext(context: &ITfContext) -> Option<RECT> {
    unsafe {
        let r = (|| -> windows::core::Result<RECT> {
            let view = context.GetActiveView()?;
            let rc = view.GetScreenExt()?;
            Ok(rc)
        })();
        match r {
            // 全零一樣代表宿主其實沒算出來，當成失敗
            Ok(rc) if rc.right > rc.left && rc.bottom > rc.top => {
                crate::dlog!(
                    "[定位] 退路：GetScreenExt rc=({},{},{},{}) 寬{} 高{}",
                    rc.left,
                    rc.top,
                    rc.right,
                    rc.bottom,
                    rc.right - rc.left,
                    rc.bottom - rc.top
                );
                Some(rc)
            }
            Ok(rc) => {
                crate::dlog!(
                    "[定位] GetScreenExt 回空矩形 ({},{},{},{})",
                    rc.left,
                    rc.top,
                    rc.right,
                    rc.bottom
                );
                None
            }
            Err(e) => {
                crate::dlog!(
                    "[定位] GetScreenExt 失敗 hr={:#010x} {}",
                    e.code().0 as u32,
                    hresult_hint(e.code().0 as u32)
                );
                None
            }
        }
    }
}

fn hresult_hint(hr: u32) -> &'static str {
    match hr {
        0x8004_0206 => "(TS_E_NOLAYOUT 版面還沒算好)",
        0x8004_0200 => "(TS_E_INVALIDPOS 位置不合法)",
        0x8004_0205 => "(TS_E_NOSELECTION 沒有選取範圍)",
        0x8004_0201 => "(TS_E_NOLOCK 沒有文件鎖)",
        0x8007_0057 => "(E_INVALIDARG)",
        0x8000_4005 | 0x8000_4001 => "(未實作/一般失敗)",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::{layout, layout_with, scroll_first_at, scroll_thumb_metrics};

    /// 滑鼠命中測試。座標是繪製時記下來的，這裡守住邊界。
    mod 滑鼠命中 {
        use super::super::hit_test;

        /// 單欄三列，每列高 20，從 y=10 開始
        fn 單欄() -> Vec<(f32, f32, f32, f32)> {
            vec![
                (0.0, 10.0, 100.0, 30.0),
                (0.0, 30.0, 100.0, 50.0),
                (0.0, 50.0, 100.0, 70.0),
            ]
        }

        #[test]
        fn 點在列上就選那一列() {
            assert_eq!(hit_test(&單欄(), 50.0, 20.0), Some(0));
            assert_eq!(hit_test(&單欄(), 50.0, 40.0), Some(1));
            assert_eq!(hit_test(&單欄(), 50.0, 60.0), Some(2));
        }

        #[test]
        fn 交界歸下面那一列() {
            // 半開區間 [top, bottom)——不然兩列都會命中，選到哪個看順序
            assert_eq!(hit_test(&單欄(), 50.0, 30.0), Some(1));
        }

        #[test]
        fn 點在清單外回無() {
            assert_eq!(hit_test(&單欄(), 50.0, 5.0), None, "在第一列上方");
            assert_eq!(hit_test(&單欄(), 50.0, 70.0), None, "在最後一列下方");
            assert_eq!(hit_test(&單欄(), 150.0, 20.0), None, "在右邊界外");
            assert_eq!(hit_test(&單欄(), -1.0, 20.0), None, "在左邊界外");
        }

        #[test]
        fn 多欄要選對欄() {
            // 兩欄各兩列：0,1 在左欄，2,3 在右欄
            let r = vec![
                (0.0, 10.0, 100.0, 30.0),
                (0.0, 30.0, 100.0, 50.0),
                (100.0, 10.0, 200.0, 30.0),
                (100.0, 30.0, 200.0, 50.0),
            ];
            assert_eq!(hit_test(&r, 50.0, 20.0), Some(0));
            assert_eq!(hit_test(&r, 150.0, 20.0), Some(2), "右欄第一列");
            assert_eq!(hit_test(&r, 150.0, 40.0), Some(3), "右欄第二列");
        }

        #[test]
        fn 沒有候選時點不到東西() {
            assert_eq!(hit_test(&[], 50.0, 20.0), None);
        }
    }

    /// 選字間隙只在選字時出現，而且量測與繪製共用同一份。
    #[test]
    fn 間隙只在選字時出現() {
        use super::preview_gap;
        assert_eq!(preview_gap(false), 0.0, "打字時不該讓字散開");
        assert!(preview_gap(true) >= 1.0, "選字時至少要有一個像素的間隙");
    }

    /// 預覽列的捲動。往回選字時被選的那格不能被推出畫面。
    mod 預覽捲動 {
        use super::super::preview_scroll;

        #[test]
        fn 沒選字時看尾端() {
            // 文字 500 寬、可視 200 → 往左推 300，尾端剛好落在右邊界
            assert_eq!(preview_scroll(500.0, 200.0, None), 300.0);
        }

        #[test]
        fn 塞得下就不捲() {
            assert_eq!(preview_scroll(100.0, 200.0, None), 0.0);
            assert_eq!(preview_scroll(100.0, 200.0, Some((10.0, 30.0))), 0.0);
        }

        #[test]
        fn 選到前面的字會捲回去() {
            // 這就是「前面會頂出去」那個問題：只看尾端的話 scroll=300，
            // 而框在 20~50，整個在畫面外
            let s = preview_scroll(500.0, 200.0, Some((20.0, 50.0)));
            assert!(s <= 20.0, "要捲到看得見框（得到 {s}）");
            assert!(s >= 0.0);
        }

        #[test]
        fn 選到後面的字維持看尾端() {
            // 框在尾端附近，本來就看得到，不必多捲
            assert_eq!(preview_scroll(500.0, 200.0, Some((450.0, 480.0))), 300.0);
        }

        #[test]
        fn 框比可視範圍還寬時以左緣為準() {
            // 至少看得到開頭，而不是中間一段
            let s = preview_scroll(500.0, 100.0, Some((50.0, 300.0)));
            assert!(s <= 50.0, "左緣要進得了畫面（得到 {s}）");
        }

        #[test]
        fn 不會捲過頭() {
            // 夾在合法範圍內，不然會露出文字後面的空白
            let s = preview_scroll(500.0, 200.0, Some((490.0, 495.0)));
            assert!((0.0..=300.0).contains(&s), "得到 {s}");
        }
    }

    /// 候選視窗的擺放。這類邊界情況（貼底、貼右、螢幕太矮）手測很難蓋全，
    /// 所以 `place` 寫成純函式。
    mod 擺放位置 {
        use super::super::place;
        use windows::Win32::Foundation::RECT;

        /// 一台 1920x1080、下方有 40px 工作列的螢幕
        fn 螢幕() -> RECT {
            RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1040,
            }
        }
        /// 一行文字的矩形
        fn 文字(left: i32, top: i32, bottom: i32) -> RECT {
            RECT {
                left,
                top,
                right: left + 100,
                bottom,
            }
        }

        #[test]
        fn 位置夠時放在文字下方() {
            let (x, y) = place(文字(300, 500, 520), 200, 300, 螢幕());
            assert_eq!((x, y), (300, 520), "貼著文字下緣、左緣對齊");
        }

        #[test]
        fn 螢幕底部放不下就翻到上方() {
            // 文字在 1000~1020，下方只剩 20px，放不下 300px 高的視窗
            let (x, y) = place(文字(300, 1000, 1020), 200, 300, 螢幕());
            assert_eq!((x, y), (300, 700), "視窗底部貼著文字上緣（1000-300）");
            assert!(y + 300 <= 1020, "不能蓋住正在打的字");
        }

        #[test]
        fn 上下都放不下就貼著上緣() {
            // 螢幕只有 200px 高，視窗 300px——怎麼放都超出
            let 矮螢幕 = RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 200,
            };
            let (_, y) = place(文字(300, 100, 120), 200, 300, 矮螢幕);
            assert_eq!(y, 0, "至少對齊上緣，看得到前面幾列");
        }

        #[test]
        fn 超出右緣就往左推() {
            let (x, _) = place(文字(1850, 500, 520), 200, 300, 螢幕());
            assert_eq!(x, 1720, "右緣貼齊螢幕（1920-200）");
        }

        #[test]
        fn 視窗比螢幕寬時對齊左緣() {
            // 寧可切右邊也不要左邊看不到——編號在左邊
            let (x, _) = place(文字(100, 500, 520), 3000, 300, 螢幕());
            assert_eq!(x, 0);
        }

        #[test]
        fn 第二台螢幕的負座標也要正確() {
            // 副螢幕常在主螢幕左邊，座標是負的
            let 副螢幕 = RECT {
                left: -2560,
                top: 0,
                right: 0,
                bottom: 1400,
            };
            let (x, y) = place(文字(-2500, 1300, 1320), 200, 300, 副螢幕);
            assert_eq!(y, 1000, "一樣要翻到上方");
            assert_eq!(x, -2500);
        }
    }

    /// 主題預設值：內距 8、行高 28（`theme::fixed`，100% 縮放）
    const PAD: i32 = 8;
    const LH: i32 = 28;

    #[test]
    fn 候選清單不會侵入預覽列() {
        // 這是實際發生過的 bug：預覽列的底在 pad*2 + line_h，
        // 但候選列從 pad + line_h 開始算——第一列往上蓋掉預覽列的字。
        let (list_top, _) = layout(PAD, LH, true, 5, false);
        let preview_bottom = PAD * 2 + LH;
        assert!(
            list_top >= preview_bottom,
            "候選清單起點 {list_top} 不該在預覽列底部 {preview_bottom} 之上"
        );
    }

    #[test]
    fn 沒有預覽列時從內距開始() {
        let (list_top, _) = layout(PAD, LH, false, 5, false);
        assert_eq!(list_top, PAD);
    }

    #[test]
    fn 高度容得下所有內容() {
        // 有預覽列、5 列候選、有提示列
        let (list_top, height) = layout(PAD, LH, true, 5, true);
        // 最後一列候選的底 + 提示列 + 底部內距，都要在視窗內
        let last_row_bottom = list_top + LH * 5;
        assert!(
            height >= last_row_bottom + LH + PAD,
            "高度 {height} 裝不下內容（最後一列底 {last_row_bottom}）"
        );
    }

    #[test]
    fn 提示列的位置跟高度算法一致() {
        // `paint` 把提示列畫在 `rc.bottom - pad - line_h`，
        // 那裡必須正好接在最後一列候選的下面，不能疊到它
        let (list_top, height) = layout(PAD, LH, true, 5, true);
        let hint_top = height - PAD - LH;
        let last_row_bottom = list_top + LH * 5;
        assert_eq!(hint_top, last_row_bottom, "提示列該正好接在候選清單下面");
    }

    #[test]
    fn 長候選換行時高度要跟著加() {
        // **候選太長會換行**，那一列就佔好幾個行高。
        // 用「行高 × 列數」算的話視窗會不夠高，底下的列被切掉。
        let normal = layout_with(PAD, LH, true, LH * 3, false, false).1;
        // 中間那列換成兩行高
        let wrapped = layout_with(PAD, LH, true, LH * 4, false, false).1;
        assert_eq!(wrapped - normal, LH, "多一行就該多一個行高");
    }

    #[test]
    fn 舊介面仍等價() {
        // `layout` 是 `layout_with` 的薄包裝（等高的情況）
        for rows in 0..5 {
            assert_eq!(
                layout(PAD, LH, true, rows, false),
                layout_with(PAD, LH, true, LH * rows, rows == 0, false),
            );
        }
    }

    #[test]
    fn 只有預覽列時視窗收緊到剛好() {
        // 打字中的狀態：只有預覽列、沒有候選。
        // 高度要**正好**是預覽列那麼高——黃色底會填滿整個視窗，
        // 底下多留一點就會露出一條白邊，而視窗是圓角的，
        // 直角的黃色矩形填到底還會從圓角外面漏出來（串色）。
        let (list_top, height) = layout(PAD, LH, true, 0, false);
        assert_eq!(height, PAD * 2 + LH, "只有預覽列時視窗該收緊");
        assert_eq!(height, list_top, "底下不該多留空間");
    }

    #[test]
    fn 只有預覽列加提示列時不收緊() {
        // 有提示列就不是「只有預覽列」，要照一般版面算
        let (list_top, height) = layout(PAD, LH, true, 0, true);
        assert!(height > list_top, "提示列要有地方畫");
        assert_eq!(height, list_top + LH + PAD);
    }

    #[test]
    fn 什麼都沒有時仍留一列高度() {
        let (_, height) = layout(PAD, LH, false, 0, false);
        assert!(height >= PAD * 2 + LH, "高度 {height} 太扁");
    }

    // ── 捲軸 ──

    const BAR: f32 = 4.0;

    #[test]
    fn 滑塊長度是可見比例() {
        // 二十欄裡看得到十欄 → 滑塊佔一半，可移動另一半
        let (thumb, travel) = scroll_thumb_metrics(200.0, BAR, 10, 20);
        assert_eq!(thumb, 100.0);
        assert_eq!(travel, 100.0);
    }

    #[test]
    fn 欄數再多滑塊也不會消失() {
        // 三十八欄（ㄧˋ 的實際欄數）：照比例算只剩五十像素出頭，
        // 但最短長度撐著，還抓得到
        let (thumb, travel) = scroll_thumb_metrics(200.0, BAR, 10, 38);
        assert!(thumb >= BAR * 4.0, "滑塊 {thumb} 太短會抓不到");
        assert!(travel > 0.0);
    }

    #[test]
    fn 全部看得到就沒有可移動距離() {
        let (thumb, travel) = scroll_thumb_metrics(200.0, BAR, 10, 10);
        assert_eq!(thumb, 200.0, "滿版");
        assert_eq!(travel, 0.0, "沒得捲");
    }

    #[test]
    fn 拖到哪就捲到第幾欄() {
        // 軌道 0..200、滑塊 100 寬 → 可移動 100，二十欄裡可捲十欄
        let travel = 100.0;
        // 抓在滑塊左緣（grab=0），拖到最左＝第 0 欄
        assert_eq!(scroll_first_at(0.0, 0.0, travel, 0.0, 10, 20), 0);
        // 拖到中間＝一半
        assert_eq!(scroll_first_at(50.0, 0.0, travel, 0.0, 10, 20), 5);
        // 拖到底＝最後一欄，再往右也不會超過
        assert_eq!(scroll_first_at(100.0, 0.0, travel, 0.0, 10, 20), 10);
        assert_eq!(scroll_first_at(999.0, 0.0, travel, 0.0, 10, 20), 10);
        // 往左拖過頭也夾在 0
        assert_eq!(scroll_first_at(-50.0, 0.0, travel, 0.0, 10, 20), 0);
    }

    #[test]
    fn 抓在滑塊中間不會讓它跳到游標() {
        // grab = 抓住的點離滑塊左緣多遠。抓中間（50）時，游標在 50
        // 代表滑塊左緣仍在 0——沒有這一項的話滑塊會跳半個身位
        assert_eq!(scroll_first_at(50.0, 0.0, 100.0, 50.0, 10, 20), 0);
        assert_eq!(scroll_first_at(100.0, 0.0, 100.0, 50.0, 10, 20), 5);
    }

    #[test]
    fn 沒得捲時拖曳一律回第零欄() {
        assert_eq!(scroll_first_at(80.0, 0.0, 0.0, 0.0, 10, 10), 0);
    }
}
