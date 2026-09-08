//! **spike 3：候選視窗**（開發文件 §2.52.7 第 3 順位）。
//!
//! 這個 spike 真正要回答的只有一件事：**做不做得出「浮在宿主上方、點它
//! 也不搶走宿主焦點」的面板**。答得出來才輪到「用什麼畫」。
//!
//! # 為什麼是 `NSPanel` 而不是 egui
//!
//! §2.52.7 原本列的是「手刻 Cocoa 或沿用設定頁的 egui 預覽」，而**不搶
//! 焦點正是 egui 那條路最大的未知數**——egui 在 macOS 上要透過 winit 開
//! 視窗，winit 開的是 `NSWindow`，而不搶焦點這件事在 macOS 是
//! **`NSPanel` 的 `NonactivatingPanel` style mask 決定的**，不是事後能
//! 用 API 關掉的屬性。所以先用 `NSPanel` 把「行為對不對」證明出來，
//! 「畫得漂不漂亮」是之後的事。
//!
//! # 跟 Windows 那邊的對照
//!
//! Windows 的坑是 `WS_EX_NOACTIVATE` **擋不住滑鼠點擊**搶焦點，得在
//! `wndproc` 回 `MA_NOACTIVATEANDEAT` 才行（CLAUDE.md 已知陷阱）。
//! macOS 這邊 `NonactivatingPanel` 是連滑鼠一起管的——**這正是驗收要
//! 實測的點**，不能只看文件。
//!
//! # 定位靠 spike 4 的成果
//!
//! 座標一律問 `attributesForCharacterIndex:0`（§2.52.20：7 個宿主都老實，
//! **只有索引 0 可信**）。矩形的高度就是行高，面板貼在它下面。

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{msg_send, sel, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSFont, NSPanel, NSPopUpMenuWindowLevel, NSScreen, NSTextField,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};

/// 面板與游標那一行之間留的空隙。
const GAP: f64 = 4.0;
/// 文字四周的內距。
const PAD: f64 = 6.0;

thread_local! {
    /// 面板只造一次，之後重複使用——每次組字都重造會閃。
    /// **主執行緒限定**，所以用 `thread_local` 而不是全域。
    static PANEL: RefCell<Option<Panel>> = const { RefCell::new(None) };
}

struct Panel {
    panel: Retained<NSPanel>,
    label: Retained<NSTextField>,
}

/// 這個物件認不認得這個 selector。
///
/// **一定要先問**：對不認得的物件送訊息會丟 ObjC 例外，而例外穿過 Rust
/// 的邊界是直接 abort（`guard.rs` 攔得住 panic，攔不住 ObjC 例外）。
/// `IMKTextInput` 是個 informal protocol，宿主實作到哪算哪。
fn responds(obj: &AnyObject, s: Sel) -> bool {
    unsafe { msg_send![obj, respondsToSelector: s] }
}

/// 向宿主問插入點那一行的矩形（螢幕座標）。拿不到就回 `None`。
///
/// **只問索引 0**。§2.52.20 量過 7 個宿主：索引 0 全部老實，其他索引
/// 不可靠（終端機回過跟索引 0 對不起來的值）。
pub fn caret_rect(sender: &AnyObject) -> Option<NSRect> {
    if !responds(
        sender,
        sel!(attributesForCharacterIndex:lineHeightRectangle:),
    ) {
        return None;
    }
    let mut rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));
    let _: Option<Retained<AnyObject>> = unsafe {
        msg_send![
            sender,
            attributesForCharacterIndex: 0usize,
            lineHeightRectangle: &mut rect,
        ]
    };
    // 零矩形代表宿主答不出來。**退路要留**——雖然 §2.52.20 量到的 7 個
    // 宿主都沒發生過，但這是防禦性的，不是常態路徑。
    if rect.size.height == 0.0 {
        return None;
    }
    Some(rect)
}

fn make_panel(mtm: MainThreadMarker) -> Panel {
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(10.0, 10.0));
    let panel = {
        NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            frame,
            // ★ 整個 spike 的重點就在 NonactivatingPanel ★
            //   少了它，點面板會把焦點從宿主搶過來，組字當場中斷。
            NSWindowStyleMask::NonactivatingPanel | NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };

    // 浮在一般視窗之上。用選單的層級（101）而不是 Floating（3）——
    // 候選視窗跟選單是同一類東西，要蓋得過宿主自己的浮動面板。
    panel.setLevel(NSPopUpMenuWindowLevel);
    panel.setFloatingPanel(true);
    // 只有真的需要時才當 key window。預設是「點了就變 key」，那會中斷組字。
    panel.setBecomesKeyOnlyIfNeeded(true);
    // 我們的行程是 LSUIElement，本來就不會「作用中」；沒有這行的話
    // 面板會在宿主切換時自己消失。
    panel.setHidesOnDeactivate(false);
    // 跟著使用者跑：換桌面、宿主全螢幕都要看得到。
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Stationary,
    );
    panel.setHasShadow(true);
    panel.setOpaque(false);
    panel.setBackgroundColor(Some(&NSColor::windowBackgroundColor()));

    let label = NSTextField::labelWithString(&NSString::from_str(""), mtm);
    label.setFont(Some(&NSFont::systemFontOfSize(16.0)));
    label.setTextColor(Some(&NSColor::labelColor()));
    label.setDrawsBackground(false);
    panel.setContentView(Some(&label));

    Panel { panel, label }
}

/// 顯示候選列。`caret` 是 `caret_rect()` 問來的那一行的矩形。
///
/// 用 `orderFront` **不是** `makeKeyAndOrderFront`——後者會把面板變成
/// key window，焦點就跑了。
pub fn show(text: &str, caret: NSRect) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    PANEL.with(|cell| {
        let mut slot = cell.borrow_mut();
        let p = slot.get_or_insert_with(|| make_panel(mtm));

        p.label.setStringValue(&NSString::from_str(text));
        p.label.sizeToFit();
        let text_size = p.label.frame().size;
        let w = text_size.width + PAD * 2.0;
        let h = text_size.height + PAD * 2.0;
        p.label
            .setFrame(NSRect::new(NSPoint::new(PAD, PAD), text_size));

        // 螢幕座標是**左下角原點**，所以「貼在游標那行下面」是減。
        let mut x = caret.origin.x;
        let mut y = caret.origin.y - h - GAP;

        // 掉出螢幕就翻到行的上方／往左收。找游標所在的那個螢幕來夾，
        // 不能用 mainScreen——多螢幕時宿主不一定在主螢幕上。
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
            if y < f.origin.y {
                y = caret.origin.y + caret.size.height + GAP;
            }
            if x + w > f.origin.x + f.size.width {
                x = f.origin.x + f.size.width - w;
            }
            if x < f.origin.x {
                x = f.origin.x;
            }
        }

        p.panel
            .setFrame_display(NSRect::new(NSPoint::new(x, y), NSSize::new(w, h)), true);
        p.panel.orderFront(None);
    });
}

/// 收起面板。組字結束、取消、送出都要叫。
pub fn hide() {
    PANEL.with(|cell| {
        if let Some(p) = cell.borrow().as_ref() {
            p.panel.orderOut(None);
        }
    });
}
