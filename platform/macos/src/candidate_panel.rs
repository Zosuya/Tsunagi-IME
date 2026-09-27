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

    /// 上一筆診斷紀錄的內容——**一樣就不再寫**。組字中每按一鍵都會問一次
    /// 座標，不去重的話異常一發生就會把 log 灌爆。
    static LAST_DIAG: RefCell<Option<String>> = const { RefCell::new(None) };

    /// 面板因為「不在目前的 Space」被丟掉重建過幾次（兩個面板合計）。
    /// 寫進診斷紀錄——帶著次數，連續兩次才不會被上面的去重吃掉。
    static REBUILDS: Cell<u32> = const { Cell::new(0) };
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
        diag(sender, "宿主沒有 attributesForCharacterIndex:，面板不顯示");
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
        diag(sender, "宿主回零矩形，問不到插入點座標，面板不顯示");
        return None;
    }
    // 座標落在所有螢幕之外。**面板還是畫得出來**（`place` 會夾回螢幕內），
    // 記這一筆純粹是留線索：宿主答錯座標跟宿主答不出座標，從外面看都是
    // 「打得出字、面板不見」，分不出來。
    if let Some(mtm) = MainThreadMarker::new() {
        let inside = NSScreen::screens(mtm).iter().any(|s| {
            let f = s.frame();
            rect.origin.x >= f.origin.x
                && rect.origin.x <= f.origin.x + f.size.width
                && rect.origin.y >= f.origin.y
                && rect.origin.y <= f.origin.y + f.size.height
        });
        if !inside {
            diag(
                sender,
                &format!(
                    "插入點座標落在所有螢幕之外：({:.0}, {:.0}) 高 {:.0}",
                    rect.origin.x, rect.origin.y, rect.size.height
                ),
            );
        }
    }
    Some(rect)
}

/// 診斷紀錄：寫進跟 `keyprobe` 同一個 log，前綴用 `[panel]`。
///
/// # 為什麼需要這個
///
/// 候選面板出不來時，從外面**完全分不出**是「宿主答不出插入點座標」還是
/// 「答出來的座標離譜」——兩者的症狀一模一樣：打得出字、面板不見。
/// 2026-09-16 在 VS Code 的全螢幕視窗遇到一次，就因為沒有這行紀錄，只能
/// 靠讀程式碼推理，最後仍然沒能定案。
///
/// **正常情況一行都不會寫**，所以不必擔心檔案長大。
/// `tools/keyprobe.py` 只認 `[key]` 開頭的行，混在同一個檔案不干擾它。
fn diag(sender: &AnyObject, msg: &str) {
    let host = if responds(sender, sel!(bundleIdentifier)) {
        let s: Option<Retained<NSString>> = unsafe { msg_send![sender, bundleIdentifier] };
        s.map(|s| s.to_string()).unwrap_or_else(|| "?".into())
    } else {
        "?".to_string()
    };
    let line = format!("[panel] {host} {msg}\n");

    let dup = LAST_DIAG.with(|c| {
        let mut last = c.borrow_mut();
        if last.as_deref() == Some(line.as_str()) {
            true
        } else {
            *last = Some(line.clone());
            false
        }
    });
    if dup {
        return;
    }

    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let path = std::path::PathBuf::from(home)
        .join("Library/Application Support/tsunagi-ime")
        .join("keyprobe.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        use std::io::Write as _;
        let _ = f.write_all(line.as_bytes());
    }
}

/// 面板剛打開，卻不在使用者眼前的 Space——回 `true` 時呼叫端要**丟掉
/// 面板重建**。候選面板與全半形那條共用。
///
/// # 病因（2026-09-23 量到）
///
/// **任何一個全螢幕 Space 被收掉的那一刻**（退出全螢幕、或全螢幕中的
/// app 結束），**收起中**的面板會被系統從「每個 Space 都在」降成「只屬於
/// 一般桌面」。之後在**原本就開著**的全螢幕 app 裡打字，面板開在桌面那
/// 一格，使用者看不到——「打得出字、面板不見」。新開的全螢幕 Space 會把
/// 它收進去、回到桌面也正常，所以看起來時有時無。
///
/// `collectionBehavior` 讀回來**完全沒變**，是 window server 那邊掉的。
/// 同值重設、清空再設回、`orderFrontRegardless`、收起再開**實測全部無效**，
/// **只有重建有效**——新造的視窗重新向系統登記一次。
///
/// # 為什麼只在「收起 → 打開」時查
///
/// **開著的面板不會被降級**（同一輪實測），所以組字中每按一鍵不必多問
/// 一次。`isOnActiveSpace` 在 `orderFront` 當下就準，不用等。
pub(crate) fn stranded(panel: &NSPanel, was_hidden: bool, sender: &AnyObject, which: &str) -> bool {
    if !was_hidden || panel.isOnActiveSpace() {
        return false;
    }
    let n = REBUILDS.with(|c| {
        c.set(c.get() + 1);
        c.get()
    });
    diag(
        sender,
        &format!("{which}不在目前的 Space（全螢幕 Space 收掉時被系統降級），丟掉重建（第 {n} 次）"),
    );
    true
}

/// 這個點落在哪台螢幕的可用範圍（`visibleFrame`，已扣掉選單列與 Dock）。
///
/// **不能直接用 `mainScreen`**——多螢幕時宿主不一定在主螢幕上。點落在
/// 所有螢幕之外（宿主回了離譜的座標）才退回主螢幕。
pub(crate) fn screen_at(mtm: MainThreadMarker, pt: NSPoint) -> Option<NSRect> {
    NSScreen::screens(mtm)
        .iter()
        .find(|s| {
            let f = s.frame();
            pt.x >= f.origin.x
                && pt.x <= f.origin.x + f.size.width
                && pt.y >= f.origin.y
                && pt.y <= f.origin.y + f.size.height
        })
        .or_else(|| NSScreen::mainScreen(mtm))
        .map(|s| s.visibleFrame())
}

/// 面板該擺在哪（螢幕座標，回傳**左下角**）。
///
/// `caret` 是組字那一行在螢幕上的矩形，`screen` 是那台螢幕的可用範圍。
/// `prefer_above` 決定優先擺游標的上方還是下方——候選面板在下、全半形
/// 那條在上，兩個才不會疊在一起。
///
/// 規則跟 Windows 的 `candidate_window::place` 對齊（**那邊是左上原點、
/// 這邊是左下原點**，所以上下的加減相反）：
///
/// 1. 先放偏好的那一側
/// 2. 放不下就翻到另一側
/// 3. 兩側都放不下（螢幕很矮）就貼著下緣，至少看得到前幾列
/// 4. 水平方向超出右緣就往左推，但不推出左緣
/// 5. **最後無條件夾回螢幕內**——見下面那段
///
/// # 第 5 條是這支函式的重點
///
/// 前四條處理的是「螢幕放不放得下」，前提是游標座標本身合理。但**宿主
/// 回的座標可能整個落在螢幕外**，那時前四條算出來的位置照樣在螢幕外，
/// 面板就被擺到看不見的地方——症狀是「**打得出字、面板無聲消失**」，
/// 極難查（2026-09-16 在 VS Code 的全螢幕視窗遇過一次）。所以最後一定
/// 要再夾一次，不管前面算出什麼。
///
/// 全部是純數值運算，所以測得起來——邊界情況用手測很難蓋全。
pub(crate) fn place(caret: NSRect, w: f64, h: f64, screen: NSRect, prefer_above: bool) -> NSPoint {
    let bottom = screen.origin.y;
    let top = screen.origin.y + screen.size.height;

    // 兩個候選位置都是「面板左下角的 y」。左下原點，所以往下是減。
    let below = caret.origin.y - h - GAP;
    let above = caret.origin.y + caret.size.height + GAP;
    let fits_below = below >= bottom;
    let fits_above = above + h <= top;

    let y = if prefer_above {
        if fits_above {
            above
        } else if fits_below {
            below
        } else {
            bottom
        }
    } else if fits_below {
        below
    } else if fits_above {
        above
    } else {
        bottom
    };

    // ★ 最後的保險：一律夾回可視範圍 ★
    //
    // `min` 在前、`max` 在後：面板比螢幕高時寧可貼著下緣被切掉上面，
    // 也不要整個看不到。跟下面 x 的處理對稱。
    let y = y.min(top - h).max(bottom);

    // `max` 放在 `min` 之後：面板比螢幕寬時，寧可切右邊也要對齊左緣
    let x = caret
        .origin
        .x
        .min(screen.origin.x + screen.size.width - w)
        .max(screen.origin.x);

    NSPoint::new(x, y)
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
    //
    // ★ `setFloatingPanel` 一定要在 `setLevel` **前面** ★
    //   它的副作用是把層級設成 Floating（3）。原本順序反過來，層級實際
    //   一直是 3——2026-09-23 用 `CGWindowList` 量到才發現，程式碼寫著
    //   101 不代表視窗就是 101。層級 3 會被宿主自己的浮動視窗蓋掉。
    panel.setFloatingPanel(true);
    panel.setLevel(NSPopUpMenuWindowLevel);
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
/// - `sender`：宿主，只用在診斷紀錄（記下是哪個 app）
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
    sender: &AnyObject,
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
        // 最多兩輪：第一輪發現面板被系統降級（見 `stranded`）就丟掉，
        // 第二輪用新造的。新造的不再查——剛向系統登記過，不會有事。
        for attempt in 0..2 {
            let p = slot.get_or_insert_with(|| make_panel(mtm, crate::settings::theme()));
            let was_hidden = !p.panel.isVisible();
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
            // 定位整段交給 `place`——它保證算出來的位置一定落在螢幕內，
            // 不管宿主回的游標座標多離譜。
            let pos = match screen_at(mtm, caret.origin) {
                Some(f) => place(caret, size.width, size.height, f, false),
                // 連一台螢幕都問不到（幾乎不可能）：至少擺在游標下方
                None => NSPoint::new(caret.origin.x, caret.origin.y - size.height - GAP),
            };

            p.panel.setFrame_display(NSRect::new(pos, size), true);
            p.view.setNeedsDisplay(true);
            p.panel.orderFront(None);

            if attempt == 0 && stranded(&p.panel, was_hidden, sender, "候選面板") {
                p.panel.orderOut(None);
                *p.view.ivars().target.borrow_mut() = None;
                *slot = None;
                continue;
            }
            break;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 一台 1920x1080、底部有 40px Dock 的螢幕（`visibleFrame` 已扣掉）。
    /// **左下原點**：origin 是左下角。
    fn 螢幕() -> NSRect {
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1920.0, 1040.0))
    }

    /// 一行文字的矩形：`(x, y)` 是**左下角**，寬 100、高 20
    fn 文字(x: f64, y: f64) -> NSRect {
        NSRect::new(NSPoint::new(x, y), NSSize::new(100.0, 20.0))
    }

    // ── 下面六條對應 Windows 的 `candidate_window::place` 那六條 ──
    // 兩邊的規則刻意一致，只有座標系上下相反。

    #[test]
    fn 位置夠時放在文字下方() {
        let p = place(文字(300.0, 500.0), 200.0, 300.0, 螢幕(), false);
        assert_eq!(
            (p.x, p.y),
            (300.0, 196.0),
            "貼著文字下緣（500-300-4）、左緣對齊"
        );
    }

    #[test]
    fn 螢幕底部放不下就翻到上方() {
        // 文字下緣在 100，下方只剩 100px，放不下 300px 高的面板
        let p = place(文字(300.0, 100.0), 200.0, 300.0, 螢幕(), false);
        assert_eq!((p.x, p.y), (300.0, 124.0), "面板下緣貼著文字上緣（120+4）");
        assert!(p.y >= 120.0, "不能蓋住正在打的字");
    }

    #[test]
    fn 上下都放不下就貼著下緣() {
        // 螢幕只有 200px 高，面板 300px——怎麼放都超出
        let 矮螢幕 = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1920.0, 200.0));
        let p = place(文字(300.0, 100.0), 200.0, 300.0, 矮螢幕, false);
        assert_eq!(p.y, 0.0, "至少對齊下緣，看得到前面幾列");
    }

    #[test]
    fn 超出右緣就往左推() {
        let p = place(文字(1850.0, 500.0), 200.0, 300.0, 螢幕(), false);
        assert_eq!(p.x, 1720.0, "右緣貼齊螢幕（1920-200）");
    }

    #[test]
    fn 面板比螢幕寬時對齊左緣() {
        // 寧可切右邊也不要左邊看不到——編號在左邊
        let p = place(文字(100.0, 500.0), 3000.0, 300.0, 螢幕(), false);
        assert_eq!(p.x, 0.0);
    }

    #[test]
    fn 第二台螢幕的負座標也要正確() {
        // 副螢幕常在主螢幕左邊，座標是負的
        let 副螢幕 = NSRect::new(NSPoint::new(-2560.0, 0.0), NSSize::new(2560.0, 1400.0));
        let p = place(文字(-2500.0, 100.0), 200.0, 300.0, 副螢幕, false);
        assert_eq!((p.x, p.y), (-2500.0, 124.0), "一樣要翻到上方");
    }

    // ── 下面兩條是 macOS 這邊補的：**游標座標本身就在螢幕外** ──
    //
    // 2026-09-16 在 VS Code 的全螢幕視窗遇到「打得出字、候選面板不出現」，
    // 沒有量到當時的座標所以根因未定，但舊的定位邏輯確實會在這種輸入下
    // 把面板擺到看不見的地方。這兩條把那個缺口鎖住。

    #[test]
    fn 游標座標高到離譜時面板仍留在螢幕內() {
        // 宿主回了一個遠超螢幕上緣的 y
        let p = place(文字(300.0, 5000.0), 200.0, 300.0, 螢幕(), false);
        assert!(
            p.y >= 0.0 && p.y + 300.0 <= 1040.0,
            "面板必須完全在螢幕內，實際 y={}",
            p.y
        );
        assert_eq!(p.y, 740.0, "頂到上緣就貼著上緣（1040-300）");
    }

    #[test]
    fn 游標座標掉到螢幕下方外時面板仍留在螢幕內() {
        let p = place(文字(300.0, -500.0), 200.0, 300.0, 螢幕(), false);
        assert!(
            p.y >= 0.0 && p.y + 300.0 <= 1040.0,
            "面板必須完全在螢幕內，實際 y={}",
            p.y
        );
        assert_eq!(p.y, 0.0, "貼著下緣");
    }

    // ── prefer_above：全半形那條 bar 走這一側 ──

    #[test]
    fn 偏好上方時放在文字上方() {
        let p = place(文字(300.0, 500.0), 200.0, 100.0, 螢幕(), true);
        assert_eq!(p.y, 524.0, "貼著文字上緣（500+20+4）");
    }

    #[test]
    fn 偏好上方但上方放不下就翻到下方() {
        // 文字接近螢幕頂端，上方塞不進 100px
        let p = place(文字(300.0, 1000.0), 200.0, 100.0, 螢幕(), true);
        assert_eq!(p.y, 896.0, "翻到文字下方（1000-100-4）");
    }
}
