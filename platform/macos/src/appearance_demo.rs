//! 外觀方案的展示視窗（spike，不是產品功能）。
//!
//! 跑法：`./target/release/tsunagi_ime --appearance-demo`
//!
//! # 為什麼要一個真的視窗
//!
//! 「接系統外觀」有三種做法、代價各不相同，用講的講不清楚——尤其毛玻璃
//! 的風險是**可讀性**，那必須看到「候選卡疊在真的文字上」才判斷得出來。
//! 所以這裡：
//!
//! - 卡片一律用 `candidate_panel` 的 `CandidateView` 畫。**展示如果用另一
//!   份繪製程式碼，看到的就不是真的長相**
//! - 卡片底下鋪一段假文件，毛玻璃才有東西可以透
//! - 淺色／深色各鋪一排，用 `NSAppearance` 強制，不必去改系統設定
//!
//! 決定做哪些之後這個檔案就可以刪掉。

use ime_core::config::Config;
use ime_core::theme::Theme;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSColor, NSFont,
    NSStringDrawing, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial,
    NSVisualEffectState, NSVisualEffectView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};

/// 假文件的內容。**要有中文也要有標點**——毛玻璃底下糊掉的通常是筆畫多的字。
const SAMPLE: &[&str] = &[
    "候選卡疊在文件上的樣子。這一段是背景，不是輸入法畫的。",
    "毛玻璃會把它糊掉——糊掉之後還讀不讀得到卡片上的字，",
    "就是要用眼睛判斷的那件事。",
];

/// 一排的四種做法。
const VARIANTS: [&str; 4] = [
    "① 現況：設定檔的固定色",
    "② 跟隨系統深／淺",
    "③ ＋系統強調色",
    "④ 毛玻璃底",
];

/// 假文件那一塊。自己畫底色與幾行字。
struct DocState {
    dark: bool,
}

define_class!(
    #[unsafe(super(NSView))]
    #[name = "TsunagiDemoDoc"]
    #[ivars = DocState]
    struct DocView;

    impl DocView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            crate::guard::catch("demo drawRect:", (), || {
                let dark = self.ivars().dark;
                let bounds = self.bounds();
                // 文件的底：淺色是紙白，深色是編輯器那種深灰
                let bg = if dark {
                    NSColor::colorWithSRGBRed_green_blue_alpha(0.12, 0.12, 0.13, 1.0)
                } else {
                    NSColor::colorWithSRGBRed_green_blue_alpha(1.0, 1.0, 1.0, 1.0)
                };
                bg.setFill();
                objc2_app_kit::NSBezierPath::fillRect(bounds);

                let fg = if dark {
                    NSColor::colorWithSRGBRed_green_blue_alpha(0.82, 0.82, 0.84, 1.0)
                } else {
                    NSColor::colorWithSRGBRed_green_blue_alpha(0.15, 0.15, 0.16, 1.0)
                };
                let font = NSFont::systemFontOfSize(15.0);
                let keys: [&NSString; 2] = unsafe {
                    [
                        objc2_app_kit::NSFontAttributeName,
                        objc2_app_kit::NSForegroundColorAttributeName,
                    ]
                };
                let vals: [&AnyObject; 2] = [font.as_ref(), fg.as_ref()];
                let attrs = objc2_foundation::NSDictionary::from_slices(&keys, &vals);
                for (i, line) in SAMPLE.iter().enumerate() {
                    let s = NSString::from_str(line);
                    let y = 14.0 + i as f64 * 24.0;
                    unsafe { s.drawAtPoint_withAttributes(NSPoint::new(20.0, y), Some(&attrs)) };
                }

            });
        }
    }
);

/// 把一組預設顏色變成繪製用的 `Theme`。
fn theme_from(preset: ime_core::theme_preset::Theme) -> Theme {
    let cfg = Config {
        colors: preset.colors,
        background: ime_core::config::Background {
            text_outline: preset.text_outline,
            ..Default::default()
        },
        ..Default::default()
    };
    Theme::from_config(&cfg)
}

/// 系統的強調色，換成我們的 `Color`。
///
/// `controlAccentColor` 是**動態顏色**（跟著系統設定與深淺變），要先轉到
/// sRGB 才取得出分量——直接問會因為它的色彩空間不是 RGB 而爆。
fn accent() -> Option<ime_core::theme::Color> {
    let c = NSColor::controlAccentColor();
    let srgb = objc2_app_kit::NSColorSpace::sRGBColorSpace();
    let c = c.colorUsingColorSpace(&srgb)?;
    Some(ime_core::theme::Color {
        r: (c.redComponent() * 255.0).round() as u8,
        g: (c.greenComponent() * 255.0).round() as u8,
        b: (c.blueComponent() * 255.0).round() as u8,
    })
}

/// 一排（淺色或深色）：假文件 ＋ 四張卡。
fn build_row(mtm: MainThreadMarker, dark: bool, frame: NSRect) -> Retained<NSView> {
    let doc = DocView::alloc(mtm).set_ivars(DocState { dark });
    let doc: Retained<DocView> = unsafe { msg_send![super(doc), initWithFrame: frame] };

    // 這一排強制成淺色或深色——**不必叫使用者去改系統設定**，而且
    // 兩種可以並排比較。`controlAccentColor` 之類的動態色會跟著解析。
    let name = unsafe {
        if dark {
            NSAppearanceNameDarkAqua
        } else {
            NSAppearanceNameAqua
        }
    };
    if let Some(ap) = NSAppearance::appearanceNamed(name) {
        doc.setAppearance(Some(&ap));
    }

    let items: Vec<String> = ["你好", "妳好", "你號", "尼好"]
        .iter()
        .map(|s| s.to_string())
        .collect();

    let base = if dark {
        ime_core::theme_preset::dark()
    } else {
        ime_core::theme_preset::light()
    };

    let mut x = 24.0;
    for (i, label) in VARIANTS.iter().enumerate() {
        // ① 現況＝設定檔怎麼寫就怎麼畫，不管系統是深是淺
        // ②③④ 都以「跟隨系統」為底，差別在再疊什麼
        let mut theme = match i {
            0 => crate::settings::theme(),
            _ => theme_from(base.clone()),
        };
        // ③④ 反白改用系統強調色
        if i >= 2 {
            if let Some(a) = accent() {
                theme.colors.highlight_bg = a;
            }
        }
        let blur = i == 3;
        let (card, size) = crate::candidate_panel::demo_card(mtm, theme, &items, label, !blur);
        let card_frame = NSRect::new(NSPoint::new(x, 96.0), size);

        if blur {
            // 毛玻璃：材質用 `Menu`——原生選單與 Apple 自己的候選視窗用的
            // 就是它。`WithinWindow` 是「糊掉同一個視窗裡排在我後面的東西」，
            // 也就是底下那段假文件；正式用的話候選面板是獨立的透明面板，
            // 要換成 `BehindWindow`（糊掉的是宿主的文件），視覺效果同一類。
            let fx = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), card_frame);
            fx.setMaterial(NSVisualEffectMaterial::Menu);
            fx.setBlendingMode(NSVisualEffectBlendingMode::WithinWindow);
            fx.setState(NSVisualEffectState::Active);
            fx.setWantsLayer(true);
            // 圓角。`objc2-quartz-core` 的 `CALayer` 繫結沒生出這兩支
            // （要另外開 feature），為了展示不值得多拉一個依賴，直接送訊息。
            if let Some(layer) = fx.layer() {
                unsafe {
                    let _: () = msg_send![&*layer, setCornerRadius: 8.0f64];
                    let _: () = msg_send![&*layer, setMasksToBounds: true];
                }
            }
            doc.addSubview(&fx);
        }

        card.setFrame(card_frame);
        doc.addSubview(&card);
        x += size.width + 24.0;
    }
    Retained::into_super(doc)
}

pub fn run() {
    let mtm = MainThreadMarker::new().expect("展示視窗要在主執行緒開");
    let app = NSApplication::sharedApplication(mtm);
    // **要 Regular**：輸入法本體是 LSUIElement（沒有 Dock 圖示也搶不到焦點），
    // 那樣的話這個視窗開了也不會浮上來、Cmd+Q 也關不掉。
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

    let w = 1180.0;
    let row_h = 210.0;
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(w, row_h * 2.0));
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            frame,
            NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    window.setTitle(&NSString::from_str("通譯：macOS 外觀方案比較"));

    let content = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(w, row_h * 2.0)),
    );
    // 上排淺色、下排深色。NSView 預設原點在左下，所以「上排」的 y 比較大。
    let light = build_row(
        mtm,
        false,
        NSRect::new(NSPoint::new(0.0, row_h), NSSize::new(w, row_h)),
    );
    let dark = build_row(
        mtm,
        true,
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(w, row_h)),
    );
    content.addSubview(&light);
    content.addSubview(&dark);
    window.setContentView(Some(&content));

    window.center();
    window.makeKeyAndOrderFront(None);
    app.activate();
    app.run();
}
