//! **spike 2：Echo 輸入法**（開發文件 §2.52.7 的第一順位）。
//!
//! 什麼功能都沒有，**刻意不接 core**——接了出問題會分不清是 IMK 的錯
//! 還是引擎的錯。要驗證的就三件事：
//!
//! 1. **按鍵進得來**：IMK 有把 keyDown 交給我們
//! 2. **標記文字出得去**：`setMarkedText:` 畫得出組字區
//! 3. **送出文字進得了宿主**：`insertText:`
//!
//! 打 `a` 顯示 `a`、Enter 送出、Esc 取消、Backspace 退一個字。
//!
//! # 跟 Windows 版的對照
//!
//! | 這裡 | TSF 那邊 |
//! |---|---|
//! | `setMarkedText:` | `SetText` ＋ display attribute 底線 |
//! | `insertText:` | 結束組字（`EndComposition` **不會**刪字，要先清空） |
//! | 空字串的 `setMarkedText:` | 取消時 `SetText(ec, 0, &[])` |
//!
//! 底線在 macOS 是 attributed string 的一部分，**不必另外註冊
//! display attribute provider**——這比 TSF 那套簡單很多（§2.52.4）。

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, AllocAnyThread, ClassType, MainThreadMarker};

use objc2_app_kit::{
    NSApplication, NSEvent, NSEventType, NSMarkedClauseSegmentAttributeName, NSUnderlineStyle,
    NSUnderlineStyleAttributeName,
};
use objc2_foundation::{
    ns_string, NSAttributedString, NSBundle, NSDictionary, NSNumber, NSRange, NSString,
};
use objc2_input_method_kit::{IMKInputController, IMKServer};

use crate::{candidate_panel, guard, keyprobe};

// 組字中的字串。
//
// **用 thread_local 而不是 ivar 是刻意的**：IMK 自己用
// `initWithServer:delegate:client:` 造控制器，我們沒有經手那個 init，
// `define_class!` 的 ivars 就不會被初始化，碰它是未定義行為。spike 只要
// 證明三條路通不通，一個全域緩衝區就夠——**真的接 core 時要改成
// 覆寫 init 並用 ivars**，因為每個宿主連線各有一個控制器。
thread_local! {
    static BUF: RefCell<String> = const { RefCell::new(String::new()) };
}

/// `NSRange` 的「沒有這個範圍」。
///
/// `setMarkedText:` 與 `insertText:` 的 `replacementRange` 要用它表示
/// 「不要取代既有文字」。**不能傳 (0,0)**——那是「取代開頭零個字」，
/// 在某些宿主上會把游標跳到文件開頭。
fn none_range() -> NSRange {
    NSRange::new(objc2_foundation::NSNotFound as usize, 0)
}

define_class!(
    #[unsafe(super(IMKInputController))]
    // **這個名字要跟 Info.plist 的 InputMethodServerControllerClass 一字不差**
    // ——IMKServer 是拿字串去 ObjC runtime 找類別的，打錯不會編譯失敗，
    // 是執行期靜悄悄地什麼都不發生。
    #[name = "TsunagiEchoController"]
    struct EchoController;

    impl EchoController {
        /// 所有 keyDown 與滑鼠事件都從這裡進來。
        ///
        /// 回 `true` = 我處理了，宿主不要再看；`false` = 讓回宿主。
        #[unsafe(method(handleEvent:client:))]
        fn handle_event(&self, event: Option<&NSEvent>, sender: Option<&AnyObject>) -> bool {
            guard::catch("handleEvent:client:", false, || {
                let (Some(event), Some(sender)) = (event, sender) else {
                    return false;
                };
                let handled = on_key(event, sender);
                // **spike 5 的取樣點**：這裡是 IMK 唯一把按鍵交給我們的地方，
                // 所以「有沒有進到這裡」就等於「輸入法看不看得到這個組合」。
                // 記在 `on_key` **之後**才知道我們接手還是放行——
                // 「有到但放行」跟「根本沒到」是兩件事（開發文件 §2.52.7 spike 5）。
                if event.r#type() == NSEventType::KeyDown {
                    keyprobe::probe(event, sender, handled);
                }
                handled
            })
        }

        /// 宿主要求立刻結束組字（切換視窗、點別的地方）。
        ///
        /// **一定要實作**：不實作的話那串字會就這樣消失。
        #[unsafe(method(commitComposition:))]
        fn commit_composition(&self, sender: Option<&AnyObject>) {
            guard::catch("commitComposition:", (), || {
                if let Some(sender) = sender {
                    commit(sender);
                }
            })
        }

        /// 這個輸入法被切走了（使用者換輸入法、或換到別的宿主）。
        ///
        /// 跟 `commitComposition:` **不是同一件事**，兩個都要實作：宿主
        /// 內部換焦點走前者，離開這個輸入法走這裡。少了它，切走之後面板
        /// 會留在螢幕上（§2.52.21）。
        #[unsafe(method(deactivateServer:))]
        fn deactivate_server(&self, sender: Option<&AnyObject>) {
            guard::catch("deactivateServer:", (), || {
                candidate_panel::hide();
                if let Some(sender) = sender {
                    commit(sender);
                }
            })
        }
    }
);

/// 一次按鍵。回傳「有沒有處理」。
fn on_key(event: &NSEvent, sender: &AnyObject) -> bool {
    // 只吃 keyDown。修飾鍵在 macOS 走 flagsChanged、**而且不自動重複**，
    // 所以 §2.44 那個「修飾鍵自動重複洗掉狀態」的坑在這裡不存在（§2.52.3）。
    if event.r#type() != NSEventType::KeyDown {
        return false;
    }
    let code = event.keyCode();
    match code {
        // Esc：有字就取消（吃掉），沒字就讓回宿主
        53 => {
            if BUF.with(|b| b.borrow().is_empty()) {
                return false;
            }
            BUF.with(|b| b.borrow_mut().clear());
            set_marked(sender, "");
            true
        }
        // Return / Enter：有字就送出
        36 | 76 => {
            if BUF.with(|b| b.borrow().is_empty()) {
                return false;
            }
            commit(sender);
            true
        }
        // 游標移動類：**組字中一律吃掉**，沒組字就讓回宿主。
        //
        // 讓回去的話宿主會去移動它自己的游標，組字區當場被打斷
        // （實測：按方向鍵組字狀態整個消失）。真的輸入法在組字中會拿
        // 方向鍵在組字區內或候選清單裡移動——Echo 沒有那些，先單純吃掉，
        // 至少狀態不會被弄壞。
        //
        // 左 123／右 124／下 125／上 126、Home 115／PageUp 116／
        // End 119／PageDown 121。
        123..=126 | 115 | 116 | 119 | 121 => !BUF.with(|b| b.borrow().is_empty()),
        // Backspace：退一個字（**按字元退，不是位元組**）
        51 => {
            if BUF.with(|b| b.borrow().is_empty()) {
                return false;
            }
            BUF.with(|b| {
                b.borrow_mut().pop();
            });
            let now = BUF.with(|b| b.borrow().clone());
            set_marked(sender, &now);
            true
        }
        _ => {
            let Some(chars) = event.charactersIgnoringModifiers() else {
                return false;
            };
            let s = chars.to_string();
            // 只收印得出來的單一字元。控制字元、功能鍵一律讓回宿主。
            //
            // **`is_control()` 擋不住功能鍵**：macOS 把方向鍵、F1～F12、
            // Home/End 這些對應到 U+F700～U+F8FF 的**私用區**，那一段
            // `is_control()` 是 false，會被當成一般字元吞進組字區
            // （spike 5 量測時發現，按方向鍵組字區會冒出看不見的東西）。
            let is_fn_key = |c: char| ('\u{F700}'..='\u{F8FF}').contains(&c);
            let mut cs = s.chars();
            let (Some(c), None) = (cs.next(), cs.next()) else {
                return false;
            };
            if c.is_control() || is_fn_key(c) {
                return false;
            }
            BUF.with(|b| b.borrow_mut().push_str(&s));
            let now = BUF.with(|b| b.borrow().clone());
            set_marked(sender, &now);
            true
        }
    }
}

/// 把組字字串包成帶底線的 `NSAttributedString`。
///
/// # 為什麼不能傳純 `NSString`
///
/// **傳純字串會弄當宿主。** 實測在終端機切換輸入法時，AppKit 的
/// `-[NSTextInputContext _forceAttributedString]` 會拿我們的字串去
/// `CFEqual`，第二個參數是 NULL，整個 Terminal 當場 `SIGTRAP`
/// （開發文件 §2.52.21）。§2.52.12 原本把「傳純 NSString」記成「之後要改成
/// 帶 attribute 的，才分得出兩種底線」——**那是低估了，它不是美觀問題**。
///
/// 兩個 attribute 都要給：
/// - `NSUnderlineStyle`：底線。macOS 的底線是 attribute 的一部分，
///   不像 TSF 要另外註冊 display attribute provider（§2.52.4）
/// - `NSMarkedClauseSegment`：第幾個「文節」。日文輸入法用它把組字區
///   切成幾段、各畫各的底線。我們只有一段，固定給 0——**但不能不給**，
///   宿主拿不到會走到上面那條 NULL 的路
fn marked_attributed(text: &str) -> Retained<NSAttributedString> {
    let s = NSString::from_str(text);
    let keys: [&NSString; 2] = unsafe {
        [
            NSUnderlineStyleAttributeName,
            NSMarkedClauseSegmentAttributeName,
        ]
    };
    let vals: [&AnyObject; 2] = [
        &NSNumber::new_isize(NSUnderlineStyle::Single.0),
        &NSNumber::new_isize(0),
    ];
    let attrs = NSDictionary::from_slices(&keys, &vals);
    unsafe {
        NSAttributedString::initWithString_attributes(NSAttributedString::alloc(), &s, Some(&attrs))
    }
}

/// 畫組字區。傳空字串就是清掉（取消）。
fn set_marked(sender: &AnyObject, text: &str) {
    let s = marked_attributed(text);
    // 游標放在最後：selectionRange 是**字元**位移，不是位元組。
    let sel = NSRange::new(text.chars().count(), 0);
    unsafe {
        let _: () = msg_send![
            sender,
            setMarkedText: &*s,
            selectionRange: sel,
            replacementRange: none_range(),
        ];
    }

    // **spike 3**：候選列。Echo 沒有引擎，候選字是假造的——這個 spike 要
    // 驗的是**面板的行為**（浮在上面、點了不搶焦點），不是候選內容。
    if text.is_empty() {
        candidate_panel::hide();
    } else if let Some(caret) = candidate_panel::caret_rect(sender) {
        candidate_panel::show(&fake_candidates(text), caret);
    }
}

/// 假候選字。spike 3 只要有東西可以看、可以點就夠了。
fn fake_candidates(text: &str) -> String {
    let upper = text.to_uppercase();
    let wide: String = text
        .chars()
        .map(|c| {
            // 半形轉全形，讓候選列看起來像真的中日文輸入法
            if c.is_ascii_graphic() {
                char::from_u32(c as u32 - 0x21 + 0xFF01).unwrap_or(c)
            } else {
                c
            }
        })
        .collect();
    format!("1 {text}　2 {upper}　3 {wide}")
}

/// 送出並清空。
fn commit(sender: &AnyObject) {
    // ★ 先收面板，而且**無條件收** ★
    //
    // 送出這條路不會經過 `set_marked`，所以「組字區清空時順便收面板」那個
    // 規則在這裡不成立——實測就是這樣漏的：滑鼠點宿主空白處會走
    // `commitComposition:`，字送出去了、面板卻還浮在螢幕上（§2.52.21）。
    //
    // 放在 `is_empty` 的提前返回**之前**：緩衝是空的但面板還開著的狀態
    // 一樣要收得掉，不然它會永遠留在那。
    candidate_panel::hide();

    let text = BUF.with(|b| std::mem::take(&mut *b.borrow_mut()));
    if text.is_empty() {
        return;
    }
    let s = NSString::from_str(&text);
    unsafe {
        let _: () = msg_send![sender, insertText: &*s, replacementRange: none_range()];
    }
}

// TISRegisterInputSource 在 Carbon 裡，objc2 沒有現成綁定，直接連。
//
// **為什麼要自己註冊自己**：威注音的安裝程式也是這樣做（登記 → 啟用），
// 而且這支 API 會強制系統重掃 `~/Library/Input Methods/`，不必登出登入。
// 曾經懷疑它「只認簽過名的 app 發出的請求」——不是，當時是 bundle id
// 少了 `.inputmethod.` 被掃描器靜默跳過，見開發文件 §2.52.16。
#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn TISRegisterInputSource(location: *const std::ffi::c_void) -> i32;
}

/// 把自己登記進系統的輸入源清單。**冪等**，已經登記過再叫一次也無妨。
///
/// `NSURL` 與 `CFURLRef` 是 toll-free bridged，指標直接傳過去就行。
fn register_self(bundle: &NSBundle) {
    let url = bundle.bundleURL();
    let st = unsafe { TISRegisterInputSource(Retained::as_ptr(&url) as *const std::ffi::c_void) };
    if st == 0 {
        eprintln!("[通譯] 已向系統登記輸入源");
    } else {
        eprintln!("[通譯] 登記輸入源失敗，OSStatus={st}");
    }
}

/// 起動：註冊類別、開 IMKServer、跑 run loop。
pub fn run() {
    let mtm = MainThreadMarker::new().expect("輸入法的 main 一定在主執行緒");
    let app = NSApplication::sharedApplication(mtm);

    // **類別一定要先碰一下**：`define_class!` 是惰性註冊的，不取一次
    // `class()` 它在 runtime 裡根本不存在，IMKServer 拿 Info.plist 上的
    // 名字去找就會找不到——而且不會報錯，只是什麼都不會發生。
    let cls = EchoController::class();
    eprintln!("[通譯] 控制器類別已註冊：{}", cls.name().to_string_lossy());

    let bundle = NSBundle::mainBundle();
    let conn = bundle.objectForInfoDictionaryKey(ns_string!("InputMethodConnectionName"));
    let conn: Option<Retained<NSString>> = conn.and_then(|o| o.downcast::<NSString>().ok());
    let bid = bundle.bundleIdentifier();

    let (Some(conn), Some(bid)) = (conn, bid) else {
        eprintln!("[通譯] Info.plist 缺 InputMethodConnectionName 或 bundle identifier，收工");
        return;
    };
    eprintln!("[通譯] 連線名 {conn}／bundle {bid}");

    // 先把自己登記進系統，再開 server。
    register_self(&bundle);

    // server 要活到行程結束——被回收的話宿主那邊會靜悄悄地連不上。
    let server = unsafe {
        IMKServer::initWithName_bundleIdentifier(IMKServer::alloc(), Some(&conn), Some(&bid))
    };
    std::mem::forget(server);

    eprintln!("[通譯] IMKServer 起來了，進 run loop");
    app.run();
}
