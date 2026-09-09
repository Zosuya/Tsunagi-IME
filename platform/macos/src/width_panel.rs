//! 全半形切換的提示面板（macOS）。
//!
//! ```text
//! 文件區：你好|
//!        ┌──────────────┐
//!        │  自  [半]  全 │   ← 這個面板（反白條會滑動，然後淡出）
//!        └──────────────┘
//! ```
//!
//! # 為什麼獨立一個面板，不塞進候選面板
//!
//! 跟 Windows 的 `width_window.rs` 同一個理由：它跟候選是兩回事——候選是
//! 「你要選哪個字」，這個是「輸入法現在什麼狀態」。混在一起的話候選清單
//! 的高度會忽大忽小，而且**沒在組字時得為了顯示它硬開一個空的候選面板**。
//!
//! # 動畫在 macOS 比較好做
//!
//! 滑動與淡出的**決策**在 `ime_core::render::width_bar`（跟 Windows 同一
//! 份），這裡只負責每幀重畫。Windows 那邊得開 `SetTimer`、而且計時器跑在
//! 宿主行程的訊息迴圈裡，所以動畫刻意壓在 1.3 秒內結束；macOS 的輸入法是
//! **自己的行程**，`NSTimer` 跑在我們自己的 run loop 上，不佔別人的
//! （§2.52.3 講的執行模型差異，這裡是站在我們這邊的那一面）。

use std::cell::RefCell;

use ime_core::language::Language;
use ime_core::render::width_bar::{
    index_of, lang_index_in, lang_options, lang_symbol, symbol, WidthBar, OPTIONS,
};
use ime_core::theme::{Color, Theme};
use ime_core::width::Width;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSBezierPath, NSColor, NSPanel, NSPopUpMenuWindowLevel, NSScreen,
    NSStringDrawing, NSView, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString, NSTimer};

/// 一格的寬與整體的高（邏輯像素，會照主題的縮放比例放大）。
/// **跟 Windows 的 `CELL_W`／`HEIGHT` 同值**，兩個平台看起來要一樣。
const CELL_W: i32 = 34;
const HEIGHT: i32 = 28;

/// 標籤的字身高度（邏輯像素）。
///
/// **直接指定像素、不用「主題字級的百分比」**——Windows 那邊試過後者，
/// 換一台電腦大小就跑掉（兩個來源的縮放各套一次）。這裡跟格子走同一個
/// `Metrics::scale`，任何縮放設定下比例都一致。
const LABEL_PX: i32 = 19;

/// 面板與游標那一行之間的縫。跟候選面板同值。
const GAP: f64 = 4.0;

/// 動畫的更新間隔。60fps——動畫不到一秒，這個成本可以忽略。
const TICK: f64 = 1.0 / 60.0;

thread_local! {
    /// 面板只造一次。**主執行緒限定**，所以是 `thread_local` 不是全域。
    static PANEL: RefCell<Option<Panel>> = const { RefCell::new(None) };
    /// 上一次問到的插入點位置。
    ///
    /// **沒在組字時宿主不一定答得出座標**，而全半形恰恰是「沒組字也要能
    /// 切」的功能。答不出來就用上一次的位置，總比不顯示好。
    static LAST_CARET: RefCell<Option<NSRect>> = const { RefCell::new(None) };
}

struct Panel {
    panel: Retained<NSPanel>,
    view: Retained<WidthView>,
}

struct ViewState {
    /// 這一輪要畫哪幾格。
    ///
    /// **這個面板同時服務兩件事**：全半形（自／半／全）與語言鎖定
    /// （自／注／日／英）。兩者的動畫、版面、淡出完全一樣，差別只有格數
    /// 與標籤——與其複製一份幾乎相同的面板，不如把標籤變成狀態。
    /// Windows 那邊（`width_window.rs`）也是同一個做法。
    labels: RefCell<Vec<&'static str>>,
    /// 動畫狀態。`None` 代表沒在動，面板該收起來。
    bar: RefCell<Option<WidthBar>>,
    theme: RefCell<Theme>,
    /// 重畫用的計時器。動畫跑完要 `invalidate`——不停掉的話它會一直
    /// 持有 view（`NSTimer` 會 retain target），而且白白每秒醒 60 次。
    timer: RefCell<Option<Retained<NSTimer>>>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[name = "TsunagiWidthView"]
    #[ivars = ViewState]
    struct WidthView;

    impl WidthView {
        /// 原點放左上角，跟候選面板一致。
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            crate::guard::catch("width drawRect:", (), || self.paint());
        }

        /// 動畫的一幀。
        #[unsafe(method(tsunagiWidthTick:))]
        fn tick(&self, _timer: Option<&AnyObject>) {
            crate::guard::catch("tsunagiWidthTick:", (), || {
                let done = self
                    .ivars()
                    .bar
                    .borrow()
                    .as_ref()
                    .map(WidthBar::done)
                    .unwrap_or(true);
                if done {
                    self.stop();
                    hide();
                    return;
                }
                // 淡出交給視窗的透明度——比在每個顏色上混底色簡單，
                // 而且連陰影一起淡，看起來才對。
                let alpha = self
                    .ivars()
                    .bar
                    .borrow()
                    .as_ref()
                    .map(WidthBar::opacity)
                    .unwrap_or(0.0) as f64;
                if let Some(w) = self.window() {
                    w.setAlphaValue(alpha);
                }
                self.setNeedsDisplay(true);
            });
        }
    }
);

impl WidthView {
    fn new(mtm: MainThreadMarker, theme: Theme) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ViewState {
            labels: RefCell::new(Vec::new()),
            bar: RefCell::new(None),
            theme: RefCell::new(theme),
            timer: RefCell::new(None),
        });
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(10.0, 10.0));
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    /// 開始（或接續）動畫。
    fn start(&self, from: usize, to: usize) {
        {
            let mut slot = self.ivars().bar.borrow_mut();
            // 上一輪還在滑就從**目前的位置**接著走，不是跳回起點——
            // 連按兩次 Shift+空白才不會閃一下。
            let mut bar = WidthBar::start_at(slot.as_ref(), from, to);
            // ★ 立刻 `release()` ★
            //
            // `WidthBar` 有「Shift 還按著就不淡出」的設計，那需要收到
            // 修飾鍵放開的事件。IMK 預設只送 keyDown，要收 flagsChanged
            // 得覆寫 `recognizedEvents:`——**為了一個「按著不淡出」的細節
            // 去改事件遮罩不划算**，而且連按時每一下都會重新開始動畫，
            // 使用者仍然看得到自己切到哪。所以這裡當作「按下就放開」。
            bar.release();
            *slot = Some(bar);
        }
        self.ensure_timer();
        if let Some(w) = self.window() {
            w.setAlphaValue(1.0);
        }
        self.setNeedsDisplay(true);
    }

    fn ensure_timer(&self) {
        if self.ivars().timer.borrow().is_some() {
            return;
        }
        let t = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                TICK,
                self,
                sel!(tsunagiWidthTick:),
                None,
                true,
            )
        };
        *self.ivars().timer.borrow_mut() = Some(t);
    }

    fn stop(&self) {
        if let Some(t) = self.ivars().timer.borrow_mut().take() {
            t.invalidate();
        }
        *self.ivars().bar.borrow_mut() = None;
    }

    /// 面板要多大。格子填滿整個面板，不留多餘空間。
    fn size(&self) -> NSSize {
        let m = &self.ivars().theme.borrow().metrics;
        let n = self.ivars().labels.borrow().len().max(1) as f64;
        NSSize::new(m.scale(CELL_W) as f64 * n, m.scale(HEIGHT) as f64)
    }

    fn paint(&self) {
        let th = self.ivars().theme.borrow();
        let c = &th.colors;
        let Some(bar) = self.ivars().bar.borrow().clone() else {
            return;
        };
        let bounds = self.bounds();
        let (w, h) = (bounds.size.width, bounds.size.height);
        let labels = self.ivars().labels.borrow().clone();
        let n = labels.len().max(1) as f64;

        // 底
        let radius = th.metrics.corner_radius() as f64;
        ns_color(&c.window_bg).setFill();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(bounds, radius, radius).fill();

        // **每格的左右緣都用「面板寬 × 索引 ÷ 格數」直接算**，不是
        // 「格寬 × 索引」——後者除不盡時餘數會堆在右邊，整組看起來偏左。
        // 反白條的位置是小數（滑動中），但用同一套刻度，字才會正好在
        // 反白的正中央。這條是 Windows 版debug 出來的，照抄。
        let edge = |i: f64| w * i / n;

        // 反白條
        let inset = 2.0;
        let idx = bar.visual_index() as f64;
        let rect = NSRect::new(
            NSPoint::new(edge(idx) + inset, inset),
            NSSize::new(edge(idx + 1.0) - edge(idx) - inset * 2.0, h - inset * 2.0),
        );
        let hl_r = ime_core::render::highlight_radius_for_row(h as f32) as f64;
        ns_color(&c.highlight_bg).setFill();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, hl_r, hl_r).fill();

        // 標籤。**字型不跟著設定走**（跟 Windows 一致，使用者要求這一格
        // 固定）：設定頁選的字型只影響候選視窗。macOS 用系統字型就好——
        // Windows 那邊得嵌一份字型檔進 DLL，是因為別人的電腦不一定裝了
        // 指定的那套；系統字型不會有這個問題。
        let font = objc2_app_kit::NSFont::systemFontOfSize(th.metrics.scale(LABEL_PX) as f64);
        for (i, label) in labels.iter().enumerate() {
            let hot = i == bar.target();
            let color = if hot { &c.highlight_text } else { &c.index };
            let attrs = crate::candidate_panel::text_attrs(&font, color);
            let s = NSString::from_str(label);
            let size = unsafe { s.sizeWithAttributes(Some(&attrs)) };
            let x = edge(i as f64) + (edge(1.0) - size.width) / 2.0;
            let y = (h - size.height) / 2.0;
            unsafe { s.drawAtPoint_withAttributes(NSPoint::new(x, y), Some(&attrs)) };
        }
    }
}

fn ns_color(c: &Color) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(
        c.r as f64 / 255.0,
        c.g as f64 / 255.0,
        c.b as f64 / 255.0,
        1.0,
    )
}

fn make_panel(mtm: MainThreadMarker, theme: Theme) -> Panel {
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(10.0, 10.0));
    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm),
        frame,
        // 跟候選面板同一組旗標，理由見那邊——**不搶焦點**是全部的前提。
        NSWindowStyleMask::NonactivatingPanel | NSWindowStyleMask::Borderless,
        NSBackingStoreType::Buffered,
        false,
    );
    panel.setLevel(NSPopUpMenuWindowLevel);
    panel.setFloatingPanel(true);
    panel.setBecomesKeyOnlyIfNeeded(true);
    panel.setHidesOnDeactivate(false);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Stationary,
    );
    panel.setHasShadow(true);
    panel.setOpaque(false);
    panel.setBackgroundColor(Some(&NSColor::clearColor()));
    // **這個面板不收滑鼠**——它只是狀態提示，點它沒有任何意義，
    // 而且它會蓋在文字上方，吃掉點擊會讓使用者以為游標點不動。
    panel.setIgnoresMouseEvents(true);

    let view = WidthView::new(mtm, theme);
    panel.setContentView(Some(&view));
    Panel { panel, view }
}

/// 全半形：自／半／全。
pub fn show(from: Width, to: Width, sender: &AnyObject) {
    let labels: Vec<&'static str> = OPTIONS.iter().map(|&o| symbol(o)).collect();
    show_bar(labels, index_of(from), index_of(to), sender);
}

/// 語言鎖定：自／注／日／英。
///
/// **停用的語言不畫**（`lang_options` 依設定過濾）——輪替時本來就會跳過
/// 它，畫出來只會多一個永遠輪不到的格子。所以索引要用**過濾後的清單**算，
/// 拿固定的 `LANG_OPTIONS` 算的話關掉中間某個語言時反白會滑到錯的格。
pub fn show_lang(
    from: Option<Language>,
    to: Option<Language>,
    engines: &ime_core::config::Engines,
    sender: &AnyObject,
) {
    let opts = lang_options(engines);
    let labels: Vec<&'static str> = opts.iter().map(|&o| lang_symbol(o)).collect();
    show_bar(
        labels,
        lang_index_in(&opts, from),
        lang_index_in(&opts, to),
        sender,
    );
}

/// 顯示提示，反白從第 `from` 格滑到第 `to` 格。
///
/// `sender` 是宿主，用來問插入點在哪。問不到就用上一次的位置——
/// 沒在組字時宿主不一定答得出來，而這個功能恰恰是沒組字也要能用。
fn show_bar(labels: Vec<&'static str>, from: usize, to: usize, sender: &AnyObject) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let caret = crate::candidate_panel::caret_rect(sender)
        .inspect(|r| LAST_CARET.with(|c| *c.borrow_mut() = Some(*r)))
        .or_else(|| LAST_CARET.with(|c| *c.borrow()));
    let Some(caret) = caret else {
        return;
    };

    PANEL.with(|cell| {
        let mut slot = cell.borrow_mut();
        let p = slot.get_or_insert_with(|| make_panel(mtm, crate::settings::theme()));
        *p.view.ivars().theme.borrow_mut() = crate::settings::theme();
        *p.view.ivars().labels.borrow_mut() = labels;
        p.view.start(from, to);

        let size = p.view.size();
        // ★ 放在游標那一行的**上方** ★
        //
        // 候選面板在下方（`candidate_panel::show`），兩個一上一下就不會
        // 疊在一起。螢幕座標是左下角原點，所以「上方」是加。
        let mut x = caret.origin.x;
        let mut y = caret.origin.y + caret.size.height + GAP;
        if let Some(screen) = NSScreen::screens(mtm)
            .iter()
            .find(|s| {
                let f = s.frame();
                caret.origin.x >= f.origin.x
                    && caret.origin.x <= f.origin.x + f.size.width
                    && caret.origin.y >= f.origin.y
                    && caret.origin.y <= f.origin.y + f.size.height
            })
            .or_else(|| NSScreen::mainScreen(mtm))
        {
            let f = screen.visibleFrame();
            // 頂到螢幕上緣就翻到行的下方
            if y + size.height > f.origin.y + f.size.height {
                y = caret.origin.y - size.height - GAP;
            }
            if x + size.width > f.origin.x + f.size.width {
                x = f.origin.x + f.size.width - size.width;
            }
            if x < f.origin.x {
                x = f.origin.x;
            }
        }
        p.panel
            .setFrame_display(NSRect::new(NSPoint::new(x, y), size), true);
        p.panel.orderFront(None);
    });
}

/// 收起面板。動畫跑完自己會叫，切輸入法時也要叫。
pub fn hide() {
    PANEL.with(|cell| {
        if let Some(p) = cell.borrow().as_ref() {
            p.view.stop();
            p.panel.orderOut(None);
        }
    });
}
