//! 候選視窗（macOS）。
//!
//! spike 3（[開發文件 §2.52.21]）證明了兩件事，這個模組建立在上面：
//!
//! 1. **`NSPanel` 的 `NonactivatingPanel` style mask 是唯一能做到「點了不搶
//!    焦點」的方式**——egui／winit 開的是 `NSWindow`，做不到。Windows 那邊
//!    `WS_EX_NOACTIVATE` 擋不住滑鼠、還要在 `wndproc` 回
//!    `MA_NOACTIVATEANDEAT`；macOS 這個 style mask 連滑鼠一起管住了。
//! 2. 座標問 `attributesForCharacterIndex:0`（§2.52.20：7 個宿主都老實）。
//!
//! # 繪製的「決策」在 core，這裡只做「實作」
//!
//! 顏色、字級、內距、圓角、反白樣式全部來自 `ime_core::theme::Theme` 與
//! `ime_core::render`，跟 Windows 版與設定頁預覽是**同一份**。這裡只負責
//! 把那些決策用 Cocoa 畫出來——等同 Windows 那邊 `d2d.rs` 的角色。
//!
//! 所以自訂 `NSView` 是必要的：`NSTextField` 只能顯示一段字，畫不出
//! 「編號＋候選字、選中的那個有反白底」，也接不到滑鼠。

use std::cell::{Cell, RefCell};

use ime_core::render;
use ime_core::theme::{Color, Theme};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadOnly, Message};
use objc2_app_kit::{
    NSBackingStoreType, NSBezierPath, NSColor, NSEvent, NSFont, NSFontAttributeName,
    NSForegroundColorAttributeName, NSPanel, NSPopUpMenuWindowLevel, NSScreen, NSStringDrawing,
    NSView, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSDictionary, NSPoint, NSRect, NSSize, NSString};

/// 面板與游標那一行之間留的空隙（邏輯像素，不縮放——它是視覺呼吸空間，
/// 不是版面的一部分）。
const GAP: f64 = 4.0;

thread_local! {
    /// 面板只造一次，之後重複使用——每次組字都重造會閃。
    /// **主執行緒限定**，所以用 `thread_local` 而不是全域。
    static PANEL: RefCell<Option<Panel>> = const { RefCell::new(None) };
}

struct Panel {
    panel: Retained<NSPanel>,
    view: Retained<CandidateView>,
}

/// 一格候選字在 view 座標裡的矩形，給繪製與滑鼠命中判定共用。
///
/// **量與畫用同一組數字**——分開算遲早會對不上，那跟
/// [§2.46](DirectWrite 的 width 不含尾端空白) 是同一類的坑。
#[derive(Clone, Copy)]
struct CellRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// view 自己的狀態。
struct ViewState {
    /// 候選字。第 n 個顯示的編號是 n+1。
    items: RefCell<Vec<String>>,
    /// 反白哪一個。
    selected: Cell<usize>,
    /// 每一格的矩形，`layout` 算好給 `draw_rect` 與 `mouse_down` 共用。
    cells: RefCell<Vec<CellRect>>,
    /// 分成幾欄。`1` 是一般的一直排，`>1` 是展開全部的多欄網格。
    columns: Cell<usize>,
    /// 繪製決策的來源。**每次 `show` 都換成最新的**——使用者改了設定
    /// 要立刻看得到，不能停在面板建立時的那一份。
    theme: RefCell<Theme>,
    /// 點下去要通知誰。**弱不弱持有無所謂**——控制器活得比面板久，
    /// 而且每次 `show` 都會覆寫。
    target: RefCell<Option<Retained<AnyObject>>>,
    /// 底部那行小字。空字串代表不畫，那一列的高度也不佔。
    hint: RefCell<String>,
    /// 要不要自己畫底色。
    ///
    /// **只有毛玻璃的展示會關掉**——那時底是 `NSVisualEffectView` 畫的，
    /// 我們再蓋一層不透明的底上去就什麼都看不到了。正式路徑一律 `true`。
    paint_bg: Cell<bool>,
}

impl ViewState {
    fn new(theme: Theme) -> Self {
        Self {
            items: RefCell::new(Vec::new()),
            selected: Cell::new(0),
            cells: RefCell::new(Vec::new()),
            columns: Cell::new(1),
            theme: RefCell::new(theme),
            target: RefCell::new(None),
            hint: RefCell::new(String::new()),
            paint_bg: Cell::new(true),
        }
    }
}

define_class!(
    #[unsafe(super(NSView))]
    #[name = "TsunagiCandidateView"]
    #[ivars = ViewState]
    struct CandidateView;

    impl CandidateView {
        /// **原點放左上角**。文字排版由上往下算比較直覺，而且跟
        /// `core::theme` 的版面常數（行高、內距）對得起來——那些數字
        /// 本來就是照「從上往下堆」寫的。
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            crate::guard::catch("drawRect:", (), || self.paint());
        }

        /// 點候選字。
        ///
        /// **這裡不搶焦點**——`NonactivatingPanel` 已經處理掉了（§2.52.21
        /// 實測「可以點但不會中斷選字」）。所以這支只要算出點到第幾格，
        /// 再通知控制器就好。
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            crate::guard::catch("mouseDown:", (), || {
                let win_pt = event.locationInWindow();
                let pt = self.convertPoint_fromView(win_pt, None);
                let hit = self.ivars().cells.borrow().iter().position(|c| {
                    pt.x >= c.x && pt.x < c.x + c.w && pt.y >= c.y && pt.y < c.y + c.h
                });
                let Some(idx) = hit else { return };
                let target = self.ivars().target.borrow().clone();
                if let Some(target) = target {
                    // 用 selector 通知而不是 Rust callback：callback 要嘛
                    // 得裝進 `Box<dyn Fn>` 掛在 ivars 裡（生命週期麻煩），
                    // 要嘛得用 block2（多一個相依）。ObjC 的訊息本來就是
                    // 為這件事設計的。
                    unsafe {
                        let _: () = msg_send![&*target, tsunagiSelectCandidate: idx];
                    }
                }
            });
        }
    }
);

impl CandidateView {
    fn new(mtm: MainThreadMarker, theme: Theme) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ViewState::new(theme));
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(10.0, 10.0));
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    fn font(&self) -> Retained<NSFont> {
        let th = self.ivars().theme.borrow();
        let size = th.metrics.font_size_pt() as f64;
        let family = &th.font.family;
        // 空字串＝跟隨系統 UI 字型（`core::theme::Font` 的約定）。
        // 指名的字型可能不存在，`fontWithName:` 會回 nil，退回系統字型。
        if family.is_empty() {
            return NSFont::systemFontOfSize(size);
        }
        NSFont::fontWithName_size(&NSString::from_str(family), size)
            .unwrap_or_else(|| NSFont::systemFontOfSize(size))
    }

    /// 提示列的字型。**比候選字小一號**，比例由 `core::render` 決定
    /// （兩個平台共用同一條公式）。
    fn hint_font(&self) -> Retained<NSFont> {
        let th = self.ivars().theme.borrow();
        let size = render::hint_font_pt(th.metrics.font_size_pt()) as f64;
        let family = &th.font.family;
        if family.is_empty() {
            return NSFont::systemFontOfSize(size);
        }
        NSFont::fontWithName_size(&NSString::from_str(family), size)
            .unwrap_or_else(|| NSFont::systemFontOfSize(size))
    }

    /// 提示列要佔多寬多高。沒有提示就是 `(0, 0)`。
    fn hint_size(&self) -> NSSize {
        let hint = self.ivars().hint.borrow();
        if hint.is_empty() {
            return NSSize::new(0.0, 0.0);
        }
        let font = self.hint_font();
        let attrs = text_attrs(&font, &self.ivars().theme.borrow().colors.index);
        let s = NSString::from_str(&hint);
        unsafe { s.sizeWithAttributes(Some(&attrs)) }
    }

    /// 每一欄放幾個。跟 `ime_core::session::CHAR_COLUMN` 一致——
    /// **每欄獨立編號 1-9、向下數**，跟日文 IME 的排法一樣。
    const PER_COLUMN: usize = 9;

    /// 這一格要不要畫編號。
    ///
    /// **只畫在反白所在的那一欄**（跟 Windows 一致）：數字鍵挑的是
    /// 「目前選中那一欄」的第幾個，每欄都畫 1-9 的話看起來像每欄都能按，
    /// 實際上只有一欄有用——那是會讓人按錯的假資訊。只有一欄時照畫。
    fn numbered_column(&self) -> usize {
        self.ivars().selected.get() / Self::PER_COLUMN
    }

    /// 編號欄位的寬度。**每一欄都預留**，即使那一欄不畫編號——
    /// 不預留的話，反白換一欄時所有的字會左右跳動。
    fn number_width(&self, font: &NSFont) -> f64 {
        let attrs = text_attrs(font, &self.ivars().theme.borrow().colors.index);
        let s = NSString::from_str("9 ");
        unsafe { s.sizeWithAttributes(Some(&attrs)) }.width
    }

    /// 這一格的編號，`None` 代表這一欄不畫編號。
    fn cell_number(&self, i: usize) -> Option<usize> {
        (i / Self::PER_COLUMN == self.numbered_column()).then(|| i % Self::PER_COLUMN + 1)
    }

    /// 算出整個面板要多大，順便把每一格的矩形存好。
    ///
    /// 排法是**直排／多欄網格**：一欄由上往下最多九個，滿了換下一欄。
    /// 未展開時就是一欄（`columns == 1`），看起來是單純的一直排。
    fn layout(&self) -> NSSize {
        let th = self.ivars().theme.borrow();
        let m = &th.metrics;
        let pad = m.padding() as f64;
        let row = m.line_height() as f64;
        let gap = m.index_gap() as f64;
        let font = self.font();
        let attrs = text_attrs(&font, &th.colors.text);

        let items = self.ivars().items.borrow();
        let cols = self.ivars().columns.get().max(1);

        // 每一欄各自貼合自己最寬的那一格——欄寬取全域最大值的話，
        // 短的欄旁邊會空出一大塊。
        let num_w = self.number_width(&font);
        let mut col_w = vec![0.0f64; cols];
        for (i, item) in items.iter().enumerate() {
            let c = (i / Self::PER_COLUMN).min(cols - 1);
            let s = NSString::from_str(item);
            let w = unsafe { s.sizeWithAttributes(Some(&attrs)) }.width;
            // **編號寬度一律算進去**，那一欄有沒有畫編號都一樣
            col_w[c] = col_w[c].max(num_w + w);
        }

        // **欄寬只由內容決定，面板貼合內容。**
        //
        // 這裡刻意**不用 `min_width`**。那是 Windows 候選視窗的版面常數，
        // 套過來會有兩個後果：內容比它窄時要把欄撐開才點得到、才有完整的
        // 反白條，而撐開的量又隨欄數變——**同一欄在展開前後寬度不一樣**，
        // 換欄時整片會抖。macOS 的浮動面板本來就該貼合內容，拿掉之後
        // 欄寬在兩個狀態下一致，格子也剛好鋪滿內容區。
        let content = col_w.iter().sum::<f64>() + gap * (cols.saturating_sub(1)) as f64;
        // **提示列可能比候選還寬**（「↑↑↓↓ ⚙ 開啟設定」比一個字長得多），
        // 寬度取兩者的最大值，不然提示會被切掉。
        let hint = self.hint_size();
        let total_w = content.max(hint.width) + pad * 2.0;

        let mut col_x = Vec::with_capacity(cols);
        let mut x = pad;
        for w in &col_w {
            col_x.push(x);
            x += w + gap;
        }

        let mut cells = Vec::with_capacity(items.len());
        for (i, _) in items.iter().enumerate() {
            let c = (i / Self::PER_COLUMN).min(cols - 1);
            let r = i % Self::PER_COLUMN;
            cells.push(CellRect {
                x: col_x[c],
                y: pad + r as f64 * row,
                w: col_w[c],
                h: row,
            });
        }
        *self.ivars().cells.borrow_mut() = cells;

        // 沒有候選字時不佔任何列——那時面板上只有提示列。
        let rows = items.len().min(Self::PER_COLUMN) as f64;
        NSSize::new(total_w, rows * row + hint.height + pad * 2.0)
    }

    fn paint(&self) {
        let th = self.ivars().theme.borrow();
        let m = &th.metrics;
        let bounds = self.bounds();

        // 底
        if self.ivars().paint_bg.get() {
            ns_color(&th.colors.window_bg).setFill();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                bounds,
                m.corner_radius() as f64,
                m.corner_radius() as f64,
            )
            .fill();
        }

        let font = self.font();
        let num_w = self.number_width(&font);
        let cells = self.ivars().cells.borrow().clone();
        let sel = self.ivars().selected.get();

        for (i, item) in self.ivars().items.borrow().iter().enumerate() {
            let Some(cell) = cells.get(i) else { continue };
            let hit = i == sel;
            if hit {
                // 反白底。圓角半徑由 core 決定——跟 Windows 版同一條公式。
                let r = render::highlight_radius_for_row(cell.h as f32) as f64;
                let rect = NSRect::new(
                    NSPoint::new(cell.x - 2.0, cell.y),
                    NSSize::new(cell.w + 4.0, cell.h),
                );
                ns_color(&th.colors.highlight_bg).setFill();
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, r, r).fill();
            }
            let color = if hit {
                &th.colors.highlight_text
            } else {
                &th.colors.text
            };
            let attrs = text_attrs(&font, color);
            let s = NSString::from_str(item);
            // 垂直置中：字的行高跟版面的 `line_height` 不一定一樣，差額對半分。
            let text_h = unsafe { s.sizeWithAttributes(Some(&attrs)) }.height;
            let y = cell.y + (cell.h - text_h) / 2.0;
            // ★ 編號跟候選字**一律同色** ★
            //
            // 原本未反白時用 `colors.index`（淡灰）。Windows 早就改掉了，
            // 理由是「同一個東西在兩種狀態下換顏色，看起來像兩種資訊」
            // （`candidate_window.rs` 的 `row_color`）——macOS 這邊沿用了
            // 更早的寫法，是漏抄不是刻意。設定頁的展示區跟的也是 Windows，
            // 不改的話展示與實際永遠對不上。
            //
            // 改完之後 `index` 這個顏色在 macOS 上**只剩提示列與全半形提示
            // 面板在用**，跟設定頁把它標成「提示文字」一致。
            //
            // 編號仍然分開畫：不畫編號的那一欄也要把位置空出來，
            // 不然換欄時字會左右跳。
            if let Some(n) = self.cell_number(i) {
                let na = text_attrs(&font, color);
                let ns = NSString::from_str(&format!("{n} "));
                unsafe { ns.drawAtPoint_withAttributes(NSPoint::new(cell.x, y), Some(&na)) };
            }
            let tx = cell.x + num_w;
            unsafe { s.drawAtPoint_withAttributes(NSPoint::new(tx, y), Some(&attrs)) };
        }

        // 底部提示列。**畫在最下面那一列**，用 `colors.index`（跟編號同色）
        // ——它跟編號一樣是輔助資訊，同一個顏色階層。
        let hint = self.ivars().hint.borrow().clone();
        if !hint.is_empty() {
            let hf = self.hint_font();
            let attrs = text_attrs(&hf, &th.colors.index);
            let hs = NSString::from_str(&hint);
            let size = unsafe { hs.sizeWithAttributes(Some(&attrs)) };
            let pad = m.padding() as f64;
            let y = bounds.size.height - pad - size.height;
            unsafe { hs.drawAtPoint_withAttributes(NSPoint::new(pad, y), Some(&attrs)) };
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

/// 字型＋顏色的屬性字典。`width_panel` 也用同一支——兩個面板的文字
/// 屬性沒有理由各寫一份。
pub(crate) fn text_attrs(
    font: &NSFont,
    color: &Color,
) -> Retained<NSDictionary<NSString, AnyObject>> {
    let keys: [&NSString; 2] = unsafe { [NSFontAttributeName, NSForegroundColorAttributeName] };
    let c = ns_color(color);
    let vals: [&AnyObject; 2] = [font.as_ref(), c.as_ref()];
    NSDictionary::from_slices(&keys, &vals)
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
/// **只問索引 0**——§2.52.20 量過 7 個宿主，索引 0 全部老實，而且它的語意
/// 就是「組字區起點」，不隨字數移動。候選視窗釘在那裡才不會邊打邊跑。
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

fn make_panel(mtm: MainThreadMarker, theme: Theme) -> Panel {
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(10.0, 10.0));
    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm),
        frame,
        // ★ 整個 spike 3 的重點就在 NonactivatingPanel ★
        //   少了它，點面板會把焦點從宿主搶過來，組字當場中斷。
        NSWindowStyleMask::NonactivatingPanel | NSWindowStyleMask::Borderless,
        NSBackingStoreType::Buffered,
        false,
    );

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
    // 圓角要靠自己畫，所以視窗本身要透明——不然圓角外會露出一塊方形底。
    panel.setOpaque(false);
    panel.setBackgroundColor(Some(&NSColor::clearColor()));

    let view = CandidateView::new(mtm, theme);
    panel.setContentView(Some(&view));
    Panel { panel, view }
}

/// 顯示候選列。
///
/// - `items`：候選字（**只給看得見的那一頁**，不是整份清單）
/// - `selected`：反白哪一個（在 `items` 裡的位移）
/// - `columns`：分成幾欄。`1` 是一般的一直排，`>1` 是展開全部的網格
/// - `hint`：底部那行小字（指令提示、引擎停用之類），空字串就不畫
/// - `target`：點下去要收 `tsunagiSelectCandidate:` 的物件
/// - `caret`：`caret_rect()` 問來的那一行的矩形
///
/// **只有提示、沒有候選字時照樣要開面板**——那正是「打了 `config`，
/// 提示告訴你可以按 ↑↑↓↓」的情況。
///
/// 用 `orderFront` **不是** `makeKeyAndOrderFront`——後者會把面板變成
/// key window，焦點就跑了。
pub fn show(
    items: &[String],
    selected: usize,
    columns: usize,
    hint: &str,
    target: &AnyObject,
    caret: NSRect,
) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    if items.is_empty() && hint.is_empty() {
        hide();
        return;
    }
    PANEL.with(|cell| {
        let mut slot = cell.borrow_mut();
        let p = slot.get_or_insert_with(|| make_panel(mtm, crate::settings::theme()));
        // 主題每次都換成最新的——改設定要立刻看得到。
        *p.view.ivars().theme.borrow_mut() = crate::settings::theme();

        *p.view.ivars().items.borrow_mut() = items.to_vec();
        *p.view.ivars().hint.borrow_mut() = hint.to_owned();
        p.view
            .ivars()
            .selected
            .set(selected.min(items.len().saturating_sub(1)));
        p.view.ivars().columns.set(columns.max(1));
        *p.view.ivars().target.borrow_mut() = Some(target.retain());

        let size = p.view.layout();
        let (w, h) = (size.width, size.height);

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
        p.view.setNeedsDisplay(true);
        p.panel.orderFront(None);
    });
}

/// 造一張「就地擺著」的候選卡，給外觀展示用（`appearance_demo`）。
///
/// **刻意走同一個 `CandidateView`**——展示如果用另一份繪製程式碼，看到的
/// 就不是真的長相。回傳的 view 已經照內容量好大小，呼叫端只要決定位置。
/// 回傳的是 `NSView`（不是 `CandidateView`）——那個型別是這個模組的私有
/// 實作，不該漏到外面去。呼叫端只需要「一個排好版的 view」。
pub(crate) fn demo_card(
    mtm: MainThreadMarker,
    theme: Theme,
    items: &[String],
    hint: &str,
    paint_bg: bool,
) -> (Retained<NSView>, NSSize) {
    let view = CandidateView::new(mtm, theme);
    *view.ivars().items.borrow_mut() = items.to_vec();
    *view.ivars().hint.borrow_mut() = hint.to_owned();
    view.ivars().selected.set(0);
    view.ivars().columns.set(1);
    view.ivars().paint_bg.set(paint_bg);
    let size = view.layout();
    (Retained::into_super(view), size)
}

/// 收起面板。組字結束、取消、送出都要叫。
pub fn hide() {
    PANEL.with(|cell| {
        if let Some(p) = cell.borrow().as_ref() {
            p.panel.orderOut(None);
            // 別留著宿主的參照——它會讓宿主的控制器活得比該有的久。
            *p.view.ivars().target.borrow_mut() = None;
        }
    });
}
