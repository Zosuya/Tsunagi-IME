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

use std::cell::{Cell, RefCell};
use std::time::Instant;

use objc2::rc::{Allocated, Retained};
use objc2::runtime::AnyObject;
use objc2::{
    define_class, msg_send, sel, AllocAnyThread, ClassType, DefinedClass, MainThreadMarker,
    MainThreadOnly, Message,
};

use objc2_app_kit::{
    NSApplication, NSBackgroundColorAttributeName, NSColor, NSEvent, NSEventModifierFlags,
    NSEventType, NSMarkedClauseSegmentAttributeName, NSMenu, NSMenuItem, NSUnderlineStyle,
    NSUnderlineStyleAttributeName,
};
use objc2_foundation::{
    ns_string, NSAttributedString, NSBundle, NSDictionary, NSMutableAttributedString, NSNumber,
    NSRange, NSString,
};
use objc2_input_method_kit::{IMKInputController, IMKServer};

use ime_core::binding::{self, Action, Key, KeyEvent, Mode};
use ime_core::config::DeleteUnitKey;
use ime_core::session::Session;

use crate::{candidate_panel, guard, keyprobe, paths, settings, width_panel};

/// 一個控制器的狀態。
///
/// # 為什麼一定要是 ivar，不能是全域
///
/// **每個宿主連線各有一個 `IMKInputController`。** spike 階段用
/// `thread_local` 的全域緩衝區帶過（§2.52.12 記著要改），那在單一宿主下
/// 看不出問題，但同時在兩個 App 打字就會**共用同一份組字內容**——在 A
/// 打到一半切到 B，B 會接著 A 的字繼續。
///
/// 改用 ivar 之後每個控制器有自己的一份，那個問題從結構上消失。
struct Composition {
    /// 引擎。**每個宿主連線一份**——`Session` 是有狀態的，共用會串味。
    session: RefCell<Session>,
    /// 段選單開著嗎。
    ///
    /// **`Session` 沒有這個狀態**——它只提供 `seg_*` 那組操作，開關由平台
    /// 層自己記（Windows 那邊是 `State::seg_menu`）。
    seg_menu: Cell<bool>,
    /// 「上上下下」手勢的偵測器，見 `ime_core::command`。
    gesture: RefCell<ime_core::command::Gesture>,
    /// 上一下方向鍵是什麼時候按的。**隔太久就當作放棄**。
    last_gesture: Cell<Option<Instant>>,
    /// 最後一次把按鍵交給我們的宿主。
    ///
    /// **滑鼠點候選字時沒有按鍵事件可以拿 client**，所以要自己記著。
    /// 每個控制器對應一個宿主連線，所以這裡不會混到別人的。
    client: RefCell<Option<Retained<AnyObject>>>,
}

impl Default for Composition {
    fn default() -> Self {
        Self {
            session: RefCell::new(Session::new()),
            seg_menu: Cell::new(false),
            gesture: RefCell::new(ime_core::command::Gesture::default()),
            last_gesture: Cell::new(None),
            client: RefCell::new(None),
        }
    }
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
    #[ivars = Composition]
    struct EchoController;

    impl EchoController {
        /// IMK 造控制器走的就是這一支——**每個宿主連線各造一個**。
        ///
        /// 覆寫它才有地方初始化 ivars：`define_class!` 的 ivars 不會自己
        /// 生出來，沒 `set_ivars` 就去碰是未定義行為。順序也不能反——
        /// **先 `set_ivars` 再呼叫 super 的 init**，因為 super 的 init
        /// 可能回頭呼叫我們覆寫的方法，那時 ivars 必須已經在了。
        #[unsafe(method_id(initWithServer:delegate:client:))]
        fn init_with_server(
            this: Allocated<Self>,
            server: Option<&IMKServer>,
            delegate: Option<&AnyObject>,
            client: Option<&AnyObject>,
        ) -> Option<Retained<Self>> {
            // **這支也要守**：`Composition::default()` 會建一個 `Session`，
            // 那是 core 的程式碼，panic 一樣會穿過 ObjC 邊界 abort 整個行程。
            //
            // 攔下來回 `None` 是合法的——ObjC 的 init 本來就允許失敗。宿主
            // 會拿不到控制器（那個 App 打不了字），但**其他宿主不受影響**，
            // 比整個輸入法一起死好。
            guard::catch("initWithServer:", None, || {
                let this = this.set_ivars(Composition::default());
                unsafe {
                    msg_send![super(this), initWithServer: server, delegate: delegate, client: client]
                }
            })
        }

        /// 所有 keyDown 與滑鼠事件都從這裡進來。
        ///
        /// 回 `true` = 我處理了，宿主不要再看；`false` = 讓回宿主。
        #[unsafe(method(handleEvent:client:))]
        fn handle_event(&self, event: Option<&NSEvent>, sender: Option<&AnyObject>) -> bool {
            guard::catch("handleEvent:client:", false, || {
                let (Some(event), Some(sender)) = (event, sender) else {
                    return false;
                };
                let handled = self.on_key(event, sender);
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
                    self.commit(sender);
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
                // 切走輸入法時提示也要收——它是浮在最上層的面板，
                // 留在那裡會蓋在別的輸入法的畫面上。
                width_panel::hide();
                if let Some(sender) = sender {
                    self.commit(sender);
                }
            })
        }

        /// 輸入法自己的選單——點選單列的輸入法圖示會看到。
        ///
        /// **`showPreferences:` 是 IMK 的慣例**：選單項的 action 設成它，
        /// IMK 就會自動呼叫控制器的同名方法（見 `IMKInputController` 的
        /// 說明）。不必自己接 target/action。
        #[unsafe(method_id(menu))]
        fn menu(&self) -> Option<Retained<NSMenu>> {
            guard::catch("menu", None, || {
                let mtm = MainThreadMarker::new()?;
                let m = NSMenu::new(mtm);
                let item = unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(
                        NSMenuItem::alloc(mtm),
                        ns_string!("通譯設定…"),
                        Some(sel!(showPreferences:)),
                        ns_string!(""),
                    )
                };
                m.addItem(&item);
                Some(m)
            })
        }

        /// 開設定頁。IMK 在使用者點上面那個選單項時呼叫。
        #[unsafe(method(showPreferences:))]
        fn show_preferences(&self, _sender: Option<&AnyObject>) {
            guard::catch("showPreferences:", (), open_settings)
        }

        /// 滑鼠點了第 `idx` 個候選字，由 `candidate_panel` 送過來。
        ///
        /// **名字前綴 `tsunagi` 是刻意的**——這是我們自己加在
        /// `IMKInputController` 子類別上的方法，不是 IMK 的協定。沒有前綴
        /// 的話，哪天 Apple 加了同名方法就會靜悄悄地互相覆蓋。
        #[unsafe(method(tsunagiSelectCandidate:))]
        fn select_candidate(&self, idx: usize) {
            guard::catch("tsunagiSelectCandidate:", (), || {
                // 宿主是 `on_key` 記下來的——**滑鼠事件沒有 client 可以拿**。
                let Some(client) = self.ivars().client.borrow().clone() else {
                    return;
                };
                {
                    let mut sess = self.session().borrow_mut();
                    // **`idx` 是畫面上的位移，不是候選清單的絕對索引。**
                    // `char_candidates()` 是完整清單，展開又捲動過之後
                    // 畫面第一個就不是第 0 個了。`set_cand_index` 收的正是
                    // 畫面位移，換算交給引擎做（跟鍵盤那條路同一個入口）。
                    sess.set_cand_index(idx);
                    let Some(choice) = sess.char_candidates().get(sess.cand_index()).cloned()
                    else {
                        return;
                    };
                    // **走引擎的選字，不是自己把字串挑出來**——選了哪個字
                    // 要進學習層（`learn_on_commit`），繞過去就學不到。
                    sess.pick_char(&choice);
                    // **點下去就是「我要這個」**，選完直接收掉清單，
                    // 跟數字鍵那條路一致（方向鍵才是逐格慢慢挑）。
                    sess.exit_select();
                }
                let now = self.composition();
                self.set_marked(&client, &now);
            })
        }
    }
);

impl EchoController {
    /// 引擎。所有讀寫都經過這裡，免得到處寫 `self.ivars().session`。
    fn session(&self) -> &RefCell<Session> {
        &self.ivars().session
    }

    fn is_composing(&self) -> bool {
        !self.session().borrow().is_empty()
    }

    /// 這個按鍵代表的可見字元。控制字元與功能鍵回 `None`。
    ///
    /// **`is_control()` 擋不住功能鍵**：macOS 把方向鍵、F1～F12、Home/End
    /// 這些對應到 U+F700～U+F8FF 的**私用區**，那一段 `is_control()` 是
    /// false，會被當成一般字元吞進組字區（spike 5 量測時發現，按方向鍵
    /// 組字區會冒出看不見的東西）。
    fn printable_char(event: &NSEvent) -> Option<char> {
        // ★ 先問 `characters()`，不是 `charactersIgnoringModifiers()` ★
        //
        // 後者回的是「**沒按修飾鍵的話**會是什麼字」——`Shift+1` 於是變成
        // `1`，而在這個輸入法裡 `1` 是注音鍵（ㄅ）。症狀是驚嘆號、問號、
        // 冒號這些要按 Shift 的標點通通打不出來，大寫字母同理。
        //
        // `characters()` 回的是使用者**實際打出來的**那個字，正是我們要
        // 送進引擎的東西。Cmd／Ctrl／Option 在上面已經整批放行給宿主了，
        // 所以走到這裡的修飾鍵只剩 Shift 與 Caps Lock——兩者都是「真的
        // 輸入」，該照著它們的結果走。
        //
        // 退回 `charactersIgnoringModifiers()` 是因為某些鍵（死鍵、部分
        // 版面配置）`characters()` 會是空字串，那時舊的那支還答得出東西。
        let s = event
            .characters()
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| event.charactersIgnoringModifiers().map(|s| s.to_string()))?;
        let mut cs = s.chars();
        let (Some(c), None) = (cs.next(), cs.next()) else {
            return None;
        };
        let is_fn_key = ('\u{F700}'..='\u{F8FF}').contains(&c);
        (!c.is_control() && !is_fn_key).then_some(c)
    }

    /// 現在是不是在選字模式。
    fn is_selecting(&self) -> bool {
        self.session().borrow().select_index().is_some()
    }

    /// 面板要畫的候選字、反白哪一個、分成幾欄。
    ///
    /// **只取「看得見的那一頁」**（`cand_visible_range`），不是整份清單——
    /// 同音字動輒幾十個，全排成一條會長到跨越整個螢幕。編號 1-9 對應的
    /// 也是這一頁，跟 Windows 一致。
    fn visible_candidates(&self) -> (Vec<String>, usize, usize) {
        let s = self.session().borrow();
        if self.ivars().seg_menu.get() {
            // 段選單的候選是「這一段可以當成什麼」，一直排不展開。
            let all = s.seg_cands();
            let view = s.seg_visible_range();
            let items: Vec<String> = all
                .get(view)
                .map(|xs| xs.iter().map(|c| c.text.clone()).collect())
                .unwrap_or_default();
            return (items, s.seg_cand_in_view().unwrap_or(0), 1);
        }
        if !s.cands_open() {
            return (Vec::new(), 0, 1);
        }
        let all = s.char_candidates();
        let view = s.cand_visible_range();
        let items = all.get(view).map(<[String]>::to_vec).unwrap_or_default();
        (items, s.cand_index_in_view().unwrap_or(0), s.cand_columns())
    }

    /// 底部提示列要寫什麼。**這份判斷跟 Windows 的 `ui.rs` 一字對一字。**
    ///
    /// 三選一，優先序由上而下：
    ///
    /// 1. **指令**：組字內容是觸發詞時顯示 `↑↑↓↓ ⚙ 開啟設定`。
    ///    這一行本身就是提示——第一次打到 `config` 就知道有這條路，
    ///    不必先去讀說明。**指令做成提示而不是候選項**：放進候選清單的話，
    ///    打 `config` 這個英文單字時每次都會多一個不想選的東西
    /// 2. **鎖定**：`鎖定：注音`。macOS 目前沒有鎖定的入口（使用者裁定
    ///    不做語言鎖定），所以實務上不會出現，但判斷留著——`Session`
    ///    有這個狀態，漏掉就是兩個平台默默分岔
    /// 3. **停用的引擎**：`日文已停用`。**這個一定要一直看得見**——它會
    ///    讓某些字突然打不出來，不提示的話使用者只會覺得「怎麼壞了」
    ///
    /// 全開、沒鎖定、不是指令時回空字串，那一列的高度也不佔。
    fn hint(&self) -> String {
        let s = self.session().borrow();
        let engines = settings::engines();
        if let Some(cmd) = ime_core::command::match_keys(s.keys()) {
            return format!("↑↑↓↓ {}", cmd.label(engines));
        }
        // 兩種狀態可能同時成立（鎖定注音、又關掉日文），都要講。
        let lock = s.lock().map(|l| format!("鎖定：{}", l.short()));
        let off: Vec<&str> = [(engines.bopomofo, "注音"), (engines.romaji, "日文")]
            .iter()
            .filter(|(on, _)| !on)
            .map(|(_, name)| *name)
            .collect();
        let off = (!off.is_empty()).then(|| format!("{}已停用", off.join("、")));
        [lock, off]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("　")
    }

    /// 狀態變了，把組字區與面板都重畫一次。
    fn refresh(&self, sender: &AnyObject) {
        let now = self.composition();
        self.set_marked(sender, &now);
    }

    /// 語言鎖定輪替：自動 → 注音 → 日文 → 英文 → 自動。
    ///
    /// **停用的語言會跳過**（`Session::cycle_lock` 負責），所以關掉日文之後
    /// 是三態輪替，不會停在一個沒有引擎的模式上。
    ///
    /// 鎖定之後：組字區顯示注音符號、`,` 之類一鍵兩用的鍵照 `lock_punct`
    /// 的設定處理、台語包（如果裝了）也是在鎖定注音時才啟動。
    fn cycle_lock(&self, sender: &AnyObject) {
        let (from, to) = {
            let mut sess = self.session().borrow_mut();
            let from = sess.lock();
            sess.cycle_lock();
            (from, sess.lock())
        };
        if from == to {
            return;
        }
        // 組字中就重畫——鎖定會換一條切法（自動是累加式、鎖定注音是單一
        // 切法），畫面上的組字內容跟著變。
        if self.is_composing() {
            self.refresh(sender);
        }
        width_panel::show_lang(from, to, &settings::engines(), sender);
    }

    /// 一下方向鍵（↑ 或 ↓）進手勢偵測器。
    ///
    /// 三條出路，跟 Windows 的 `Action::Gesture` 一字對一字：
    ///
    /// 1. **組字內容不是指令** → 手勢清掉，當普通方向鍵（進選字＋開清單）
    /// 2. **湊滿上上下下** → 取消組字，執行指令
    /// 3. **還有希望** → 這一下先吃掉，什麼都不做
    ///
    /// 第 3 條是關鍵：**進了選字模式方向鍵的意義就變了**（變成移動反白），
    /// 手勢再也收不到第二下。Windows 第一版正是敗在這裡。
    fn on_gesture(&self, sender: &AnyObject, dir: ime_core::command::Dir) {
        // 隔太久就當作放棄——使用者按了 ↑↑ 然後改變主意去做別的事，
        // 那兩下不該一直留著等下次的 ↓↓ 來湊。
        let now = Instant::now();
        if self
            .ivars()
            .last_gesture
            .get()
            .is_some_and(|t| now.duration_since(t) > GESTURE_TIMEOUT)
        {
            self.ivars().gesture.borrow_mut().clear();
        }
        self.ivars().last_gesture.set(Some(now));

        let cmd = ime_core::command::match_keys(self.session().borrow().keys());

        // 不是指令：方向鍵就只是方向鍵。**上下鍵是「讓我看看有哪些字」**，
        // 所以一次到位——框與清單一起出來，不必再按一次（左右鍵才是只出框）。
        let Some(cmd) = cmd else {
            self.ivars().gesture.borrow_mut().clear();
            self.enter_select_showing(sender);
            return;
        };

        let hit = self.ivars().gesture.borrow_mut().push(dir);
        if hit {
            self.ivars().gesture.borrow_mut().clear();
            // **取消組字而不是送出**——指令用的那串鍵（`config`／`gk42u/4`）
            // 是拿來下指令的，不該留在文件裡。
            self.session().borrow_mut().clear();
            self.ivars().seg_menu.set(false);
            self.set_marked(sender, "");
            candidate_panel::hide();
            self.run_command(cmd);
        } else if self.ivars().gesture.borrow().promising() {
            // 還有希望湊成手勢：這一下先吃掉，等後面幾下。
        } else {
            // 湊不成了（例如第一下按的是 ↓），當普通方向鍵處理
            self.ivars().gesture.borrow_mut().clear();
            self.enter_select_showing(sender);
        }
    }

    /// 進選字並把清單打開。手勢退場的兩條路共用。
    fn enter_select_showing(&self, sender: &AnyObject) {
        {
            let mut s = self.session().borrow_mut();
            s.enter_select_last();
            s.open_cands();
        }
        self.refresh(sender);
    }

    /// 執行一個指令。對應 Windows 的 `background::run_command`。
    fn run_command(&self, cmd: ime_core::command::Command) {
        use ime_core::command::Command;
        match cmd {
            Command::OpenSettings => open_settings(),
            Command::ToggleEngine(lang) => {
                let now = settings::toggle_engine(lang);
                settings::apply_to(&mut self.session().borrow_mut());
                // 剛打開的引擎詞庫可能還沒載——背景補上，不要卡在這裡。
                // `preload` 讀好幾十 MB，在按鍵路徑上等它等於當掉。
                if now {
                    if let Some(dir) = paths::data_dir() {
                        let engines = settings::engines();
                        std::thread::spawn(move || ime_core::preload(&dir, engines));
                    }
                }
                // **不彈提示視窗**：跟 Windows 一樣，狀態改了使用者下次
                // 打字自然會發現，閃一下就消失的浮動視窗沒有用。
                eprintln!(
                    "[通譯] 指令：{} {lang:?}",
                    if now { "啟用" } else { "停用" }
                );
            }
        }
    }

    /// macOS 的 `keyCode` → 中性的 `Key`，查 `ime_core::binding` 的鍵位表用。
    ///
    /// **鍵位表只有一份**（2026-09-13 上搬 core），這裡只負責翻譯。以前這裡是
    /// 照著 Windows 的表人工抄一份 `match keyCode`，抄漏過三處（§2.52.32），
    /// 還有數字鍵盤被當成注音鍵、F1 會打散組字這兩個洞，一併由查表補上。
    ///
    /// 順序：有名字的控制鍵 → 數字鍵盤 → 印得出來的字元 → 其他。
    /// 控制鍵要看 `keyCode` 而不是字元——空白鍵的字元是 `' '`，但它在表裡
    /// 是 `Key::Space`（打字中是一聲、選字中是展開），不能當一般輸入。
    fn to_key(code: u16, ch: Option<char>) -> Key {
        match code {
            49 => Key::Space,
            // Return 36 與數字鍵盤的 Enter 76 是同一個動作（Windows 也不分）
            36 | 76 => Key::Enter,
            48 => Key::Tab,
            53 => Key::Esc,
            51 => Key::Backspace,
            123 => Key::Left,
            124 => Key::Right,
            125 => Key::Down,
            126 => Key::Up,
            // ── 數字鍵盤 ──
            //
            // **要翻成 `Numpad` 而不是 `Char`**：`characters()` 對數字鍵盤的 5
            // 回的就是 `"5"`，照字元走的話會被當成注音的ㄓ。鍵碼不連續
            // （8 和 9 跳過了 90），照 Carbon 的 `kVK_ANSI_Keypad*` 逐一列。
            // 數字鍵盤的 `=`（81）Windows 那邊沒有，照一般字元走。
            82 => Key::Numpad('0'),
            83 => Key::Numpad('1'),
            84 => Key::Numpad('2'),
            85 => Key::Numpad('3'),
            86 => Key::Numpad('4'),
            87 => Key::Numpad('5'),
            88 => Key::Numpad('6'),
            89 => Key::Numpad('7'),
            91 => Key::Numpad('8'),
            92 => Key::Numpad('9'),
            65 => Key::Numpad('.'),
            67 => Key::Numpad('*'),
            69 => Key::Numpad('+'),
            75 => Key::Numpad('/'),
            78 => Key::Numpad('-'),
            // Home／End／PgUp／PgDn／F1… 的字元落在私用區，`printable_char`
            // 會回 `None`，於是變成 `Other`——組字中一律吞掉
            _ => ch.map_or(Key::Other, Key::Char),
        }
    }

    /// 現在的模式。**判斷順序跟 Windows 的 `mode_of` 一致**：沒字 → 選字 → 段選單 → 打字。
    fn mode(&self) -> Mode {
        let s = self.session().borrow();
        if s.is_empty() {
            Mode::Idle
        } else if s.select_index().is_some() {
            if s.cand_expanded() {
                Mode::SelectingExpanded
            } else {
                Mode::Selecting
            }
        } else if self.ivars().seg_menu.get() {
            Mode::SegMenu
        } else {
            Mode::Typing
        }
    }

    /// 刪整格／整段那兩組設定的共同判斷：這一下要不要刪整個單位。
    ///
    /// 跟 Windows 的 `DeleteCell`／`DeleteSeg` 分派一字對一字。
    fn want_delete_unit(setting: DeleteUnitKey, shift: bool) -> bool {
        match setting {
            // 關掉：倒退鍵與 Shift+倒退鍵都退回刪單鍵
            DeleteUnitKey::Off => false,
            // 倒退鍵刪整個單位。**`Shift+`倒退鍵也算**——這個設定下 Shift
            // 沒有別的意思，讓它一起生效比「按了沒反應」好
            DeleteUnitKey::Backspace => true,
            // 並存：只有按著 Shift 才刪整個單位
            DeleteUnitKey::ShiftBackspace => shift,
        }
    }

    /// 執行查表查出來的動作。回傳「有沒有處理」（`true` = 宿主不要再看）。
    ///
    /// **動作的實作是平台的事**（組字區、面板怎麼畫），按哪顆鍵觸發是
    /// `ime_core::binding` 的事——兩邊分開，換鍵位不必動這裡。
    fn dispatch(&self, action: Action, sender: &AnyObject, shift: bool) -> bool {
        match action {
            // ── 輸入 ──
            Action::Input(c) => {
                // 打字就離開段選單（跟 Windows 一致）；手勢必須是連續四下，
                // 打了字就重來
                self.ivars().seg_menu.set(false);
                self.ivars().gesture.borrow_mut().clear();
                self.session().borrow_mut().push(c);
                self.refresh(sender);
            }
            // **數字鍵盤打的就是數字**，不進組字區。
            //
            // 正在組字的話先把它送出，再打數字進去——像是「這一段打完了，
            // 接著輸入數字」。混進注音串裡的話 `5` 會被當成ㄓ。
            Action::NumpadInput(c) => {
                if self.is_composing() {
                    self.commit(sender);
                }
                // 全形模式下數字也要變全形，跟組字那條路一致
                let c = ime_core::width::convert(c, self.session().borrow().width(), None);
                let s = NSString::from_str(&c.to_string());
                unsafe {
                    let _: () = msg_send![sender, insertText: &*s, replacementRange: none_range()];
                }
            }
            // Backspace：退一個鍵（**按字元退，不是位元組**）
            Action::Backspace => {
                self.ivars().seg_menu.set(false);
                self.session().borrow_mut().backspace();
                self.refresh(sender);
            }
            // **刪掉反白這一格**（選字）。刪不了（設定關掉、對不上）就退回一般的退格，
            // 使用者按下去總得有反應。刪完留在選字模式，框往前挪一格。
            Action::DeleteCell => {
                let want = Self::want_delete_unit(settings::delete_marked_cell(), shift);
                let mut s = self.session().borrow_mut();
                if !want || !s.delete_marked_cell() {
                    s.backspace();
                    self.ivars().seg_menu.set(false);
                }
                drop(s);
                self.refresh(sender);
            }
            // **刪掉反白這一段**，選單留著——刪掉一段之後使用者多半要接著改
            // 遞補上來的那一段。整串刪光了才關選單。
            Action::DeleteSeg => {
                let want = Self::want_delete_unit(settings::delete_marked_seg(), shift);
                let mut s = self.session().borrow_mut();
                if !want || !s.delete_marked_seg() {
                    s.backspace();
                    self.ivars().seg_menu.set(false);
                } else if s.seg_done() {
                    self.ivars().seg_menu.set(false);
                }
                drop(s);
                self.refresh(sender);
            }
            // Esc：選字開著時先退回打字；再按一次才取消組字
            Action::Cancel => {
                self.ivars().seg_menu.set(false);
                if self.is_selecting() {
                    self.session().borrow_mut().exit_select();
                    self.refresh(sender);
                } else {
                    self.session().borrow_mut().clear();
                    self.set_marked(sender, "");
                }
            }
            Action::Commit => self.commit(sender),

            // ── 段選單 ──
            //
            // 打字中與選字中都是同一個入口。開之前先退出選字——兩層互斥。
            Action::OpenSegMenu => {
                {
                    let mut s = self.session().borrow_mut();
                    s.exit_select();
                    s.seg_open();
                }
                self.ivars().seg_menu.set(true);
                self.refresh(sender);
            }
            // **TAB 與 Esc 都是關掉選單、退回組字狀態**，已經定案的段留著。
            // `seg_reset()` 因此沒有入口——要重來就關掉選單、再按 Esc 取消組字。
            Action::CloseSegMenu => {
                self.ivars().seg_menu.set(false);
                self.refresh(sender);
            }
            // ── 台語模式下這四個動作換的是「詞」不是「段」 ──
            //
            // 鎖定注音＋裝了台語包時（`tw_mode()`），整串按鍵就是一段，換段沒有
            // 意義；反白單位改成「詞」（`tw_words` 的最大匹配斷詞），所以 `←→` 是
            // 跳詞、`Shift+←→` 是調那個詞的寬度（「我們」↔「我」）。
            //
            // **起點固定、只調長度**（使用者裁定）：要選「們」用 `←→` 跳過去，
            // 不必再加一組推左邊界的鍵。
            Action::SegRight | Action::SegLeft | Action::SegWiden | Action::SegNarrow => {
                {
                    let mut s = self.session().borrow_mut();
                    match (action, s.tw_mode()) {
                        (Action::SegRight, true) => s.tw_next(),
                        (Action::SegRight, false) => s.seg_right(),
                        (Action::SegLeft, true) => s.tw_prev(),
                        (Action::SegLeft, false) => s.seg_left(),
                        (Action::SegWiden, true) => s.tw_widen(),
                        (Action::SegWiden, false) => s.seg_widen(),
                        (_, true) => s.tw_narrow(),
                        (_, false) => s.seg_narrow(),
                    }
                }
                self.after_seg(sender);
            }
            Action::SegNextCand => {
                self.session().borrow_mut().seg_next_cand();
                self.after_seg(sender);
            }
            Action::SegPrevCand => {
                self.session().borrow_mut().seg_prev_cand();
                self.after_seg(sender);
            }
            // Enter 是「選定這一段」——前面定案、後面重算，反白自動移到下一段。
            // **不是送出**：送出要先關掉選單回到打字。
            //
            // **段選單有自己的開關**（`behavior.enter_in_segmenu`，使用者要求
            // 2026-09-09 拆開）。原本跟選字的 `enter_in_select` 共用，但兩層的
            // 粒度不同——選字是逐**字**挑、段選單是逐**段**挑。**這裡查錯一支
            // 不會有任何症狀**，只有使用者去改那個設定時才會發現改的是另一層。
            Action::SegConfirm => {
                self.session()
                    .borrow_mut()
                    .seg_confirm_with(settings::enter_advance_seg());
                self.after_seg(sender);
            }
            // 數字直接挑這一段的第 n 個解釋，挑完直接定案。**往不往下一段仍照
            // `enter_in_segmenu`**——跟 Enter 同一個開關，不然兩條路行為不一致。
            Action::SegPick(n) => {
                {
                    let mut s = self.session().borrow_mut();
                    if let Some(i) = s.seg_number_index(n) {
                        s.seg_set_cand(i);
                        s.seg_confirm_with(settings::enter_advance_seg());
                    }
                }
                self.after_seg(sender);
            }

            // ── 選字 ──
            //
            // **左鍵從最後一格進**：使用者按左鍵的直覺是「從右邊選過來」，
            // 從第一格進來會看起來像跳過了最後一個字。進了選字就把清單打開，
            // 不然畫面上只有框、沒有候選字可以看。
            Action::EnterSelect | Action::EnterSelectLast => {
                {
                    let mut s = self.session().borrow_mut();
                    if action == Action::EnterSelectLast {
                        s.enter_select_last();
                    } else {
                        s.enter_select_first();
                    }
                    s.open_cands();
                }
                self.refresh(sender);
            }
            // ↑↓ 先走手勢偵測（`ime_core::command::Gesture`）——組字內容是指令時，
            // 上上下下就直接執行。湊不成手勢的話原樣退回「進選字」。
            Action::Gesture(dir) => self.on_gesture(sender, dir),
            // ── Shift+←→：日文詞界（文節）伸縮 ──
            //
            // 推不動就**什麼都不做**（已經到頭了），但仍然吃掉——放行的話宿主
            // 會拿去移動自己的游標，組字就散了。
            Action::WidenWord | Action::NarrowWord => {
                let ok = {
                    let mut s = self.session().borrow_mut();
                    if action == Action::WidenWord {
                        s.widen_word()
                    } else {
                        s.narrow_word()
                    }
                };
                if ok {
                    self.refresh(sender);
                }
            }
            Action::SelectLeft
            | Action::SelectRight
            | Action::NextCand
            | Action::PrevCand
            | Action::NextColumn
            | Action::PrevColumn
            | Action::ExpandAllChars
            | Action::CollapseChars => {
                {
                    let mut s = self.session().borrow_mut();
                    match action {
                        Action::SelectLeft => s.select_left(),
                        Action::SelectRight => s.select_right(),
                        Action::NextCand => s.next_cand(),
                        Action::PrevCand => s.prev_cand(),
                        Action::NextColumn => s.cand_right_column(),
                        Action::PrevColumn => s.cand_left_column(),
                        Action::ExpandAllChars => s.expand_cands(),
                        _ => s.collapse_cands(),
                    }
                }
                self.refresh(sender);
            }
            // Enter：選中反白的那個。**不是送出**——送出要退出選字回到打字再按一次。
            Action::ConfirmCand => {
                // **照設定走**（`behavior.enter_in_select`）：新注音式是選完往下一格
                // 繼續選，另一種是選完直接離開選字。
                //
                // 數字鍵與滑鼠那兩條路**刻意不看這個設定**，一律當成「我要這個」
                // 直接收掉清單——那是明確指名某一個候選，跟方向鍵逐格慢慢挑不是
                // 同一回事（Windows 的數字鍵也一樣不套）。
                let left_select = self
                    .session()
                    .borrow_mut()
                    .confirm_cand_with(settings::enter_advance());
                // 離開選字（沒有下一格可挑了）而且設定要「選完直接送出」的話，
                // 就在這裡送出，不必再按一次 Enter。
                if left_select && settings::commit_on_last() {
                    self.commit(sender);
                } else {
                    self.refresh(sender);
                }
            }
            // **數字是選字，不是輸入**——打字時十個數字全是注音鍵，但選字時已經
            // 在挑字了，數字就空出來了（跟新注音一致）。
            Action::PickChar(n) => {
                let pick = {
                    let s = self.session().borrow();
                    // 清單沒開就沒東西可以按號碼選——那時畫面上只有框，
                    // 使用者看不到編號，按下去等於盲選。
                    if !s.cands_open() {
                        return true;
                    }
                    s.cand_number_index(n)
                        .and_then(|i| s.char_candidates().get(i).cloned())
                };
                if let Some(choice) = pick {
                    let mut s = self.session().borrow_mut();
                    s.pick_char(&choice);
                    // **按號碼就是「我要這個」**，挑完直接收掉清單，
                    // 不像方向鍵那條路是逐格慢慢挑。
                    s.exit_select();
                }
                self.refresh(sender);
            }

            // ── 全半形與語言鎖定 ──
            Action::ToggleWidth => {
                let (before, after) = {
                    let mut sess = self.session().borrow_mut();
                    let before = sess.width();
                    sess.toggle_width();
                    (before, sess.width())
                };
                // 組字中要重畫——**標點的形狀跟著全半形變**（`,` ↔ `，`）。
                if self.is_composing() {
                    self.refresh(sender);
                }
                width_panel::show(before, after, sender);
            }
            Action::CycleLock => self.cycle_lock(sender),

            // 組字中沒綁定的鍵一律吃掉（Home／End／F1…），放行的話宿主會把
            // 游標移出組字區，組字就散了
            Action::Swallow => {}
        }
        true
    }

    /// 段選單動作之後的收尾：全部定案就自己關掉選單，然後重畫。
    ///
    /// **每一段都定案了就關掉選單**——沒有東西可挑了，留著只會讓反白停在
    /// 最後一段、候選是空的。
    fn after_seg(&self, sender: &AnyObject) {
        if self.session().borrow().seg_done() {
            self.ivars().seg_menu.set(false);
            // 「最後一段選完直接送出」（`behavior.commit_on_last_seg`）。
            //
            // **段選單有自己一組設定**：使用者要求把它跟選字的
            // `commit_on_last` 拆開（2026-09-09），兩層的操作節奏不同。
            // 早先兩層共用一個設定，Windows 那邊還一度漏接，實測回報
            // 「這功能在台語 TAB 中沒生效」——拆開之後**兩支都要各自查**。
            if settings::commit_on_last_seg() {
                self.commit(sender);
                return;
            }
        }
        self.refresh(sender);
    }

    /// 組字區的 attributed string——**逐格上屬性**。
    ///
    /// # 為什麼不能只丟一段純文字
    ///
    /// 選字時使用者要看得出**正在選哪一格**。Windows 那邊是在獨立的預覽列
    /// 上畫一個框；macOS 沒有那條預覽列（§2.52.27），所以反白要做在組字區
    /// 本身——而組字區是宿主畫的，我們只能透過 attributed string 的屬性
    /// 告訴它怎麼畫。**畫矩形在這裡沒有用**，那塊畫布不是我們的。
    ///
    /// 順帶把 `NSMarkedClauseSegment` 逐格編號（原本整串固定給 0）——那是
    /// 「文節」的正規用法，宿主靠它知道組字區分成幾段。
    fn marked_attributed(&self) -> Retained<NSAttributedString> {
        let s = self.session().borrow();
        let out = NSMutableAttributedString::new();
        if self.ivars().seg_menu.get() {
            // 段選單的反白單位是**段**，不是格——一段可能橫跨好幾格
            // （`ㄋㄧˇㄏㄠˇ` 是一段兩格）。
            let (pieces, at) = s.seg_preview_texts();
            for (i, t) in pieces.iter().enumerate() {
                out.appendAttributedString(&piece(t, i, i == at));
            }
            return out.into_super();
        }
        // ★ 用 `marked_index()` 不是 `select_index()` ★
        //
        // **選完字離開選字模式之後，那一格的框要留著**——讓使用者看到自己
        // 剛改了哪個字。`select_index()` 一離開就變 `None`，反白會整個消失。
        // 候選面板那邊仍然看 `select_index`（`visible_candidates`）：離開了
        // 就不該再列候選，但框要在。這條跟 Windows 的 `ui.rs` 一字對一字。
        let sel = s.marked_index();
        for (i, slot) in s.slots().iter().enumerate() {
            out.appendAttributedString(&piece(&slot.text, i, Some(i) == sel));
        }
        let pending = s.pending_symbols();
        if !pending.is_empty() {
            // 還沒湊成一個單位的殘留按鍵自成一段，永遠不反白。
            out.appendAttributedString(&piece(&pending, s.slots().len(), false));
        }
        out.into_super()
    }

    /// 組字區要顯示的文字。**問引擎，不是自己記**。
    ///
    /// # 為什麼不是 `composition_text()`
    ///
    /// 那支在**自動模式**下回的是原始按鍵（`su3cl3`），因為 Windows 版
    /// 的設計是「組字區顯示按鍵、另一條獨立的預覽列顯示會送出什麼」
    /// （見 `platform/windows/src/preview_window.rs` 開頭）。
    ///
    /// **macOS 不這樣做**：每個原生輸入法都是組字區直接顯示轉換後的結果
    /// （日文輸入法打 `nihongo` 當場顯示假名，不是顯示羅馬字）。所以這裡
    /// 用跟「鎖定語言」時同一條公式——已轉換的部分加上還沒湊成一個單位
    /// 的殘留按鍵：
    ///
    /// ```text
    /// su3cl3 → 你好      hello → hello      su3c → 你c
    /// ```
    ///
    /// 這是兩個平台**刻意不一致**的一處，理由是遵循各自的慣例。
    fn composition(&self) -> String {
        let s = self.session().borrow();
        if self.ivars().seg_menu.get() {
            // **段選單顯示「按下 Enter 會變成什麼」**，不是現況——使用者
            // 按 ↑↓ 換解釋、Shift+←→ 推邊界時要立刻看得到結果。
            return s.seg_preview_texts().0.concat();
        }
        let mut t = s.text();
        t.push_str(&s.pending_symbols());
        t
    }

    /// 一次按鍵。回傳「有沒有處理」。
    fn on_key(&self, event: &NSEvent, sender: &AnyObject) -> bool {
        // 設定變了就重新套用。**節流在 `settings` 裡面**（兩秒看一次檔案
        // 時間），所以放在每一鍵的路徑上不會拖慢。
        if settings::refresh() {
            settings::apply_to(&mut self.session().borrow_mut());
        }

        // ★ 記住宿主 ★
        //
        // **滑鼠點候選字時沒有按鍵事件可以拿 client**，而送出文字一定要
        // 對著某個宿主。每個控制器對應一個宿主連線（§2.52.25），所以記在
        // 自己的 ivar 裡不會混到別人的。
        //
        // 少了這一行的症狀是「候選字點了完全沒反應」——訊息有送到、命中
        // 也對，但 `tsunagiSelectCandidate:` 一進來就因為 client 是 None
        // 提早 return。查了好幾輪才發現（開發文件 §2.52.28）。
        *self.ivars().client.borrow_mut() = Some(sender.retain());

        // 只吃 keyDown。修飾鍵在 macOS 走 flagsChanged、**而且不自動重複**，
        // 所以 §2.44 那個「修飾鍵自動重複洗掉狀態」的坑在這裡不存在（§2.52.3）。
        if event.r#type() != NSEventType::KeyDown {
            return false;
        }

        // ★ Ctrl／Option 的一律讓回宿主，Cmd 交給鍵位表判斷 ★
        //
        // **`Ctrl+…` 根本到不了輸入法**（`keyprobe.log` 154KB 一筆都沒有，
        // §2.52.46），偶爾進得來也是系統層的東西。Option 放行是因為
        // `Option+字母` 是 macOS 輸入特殊字元的正規用法（`Option+A` 打 å、
        // `Option+R` 打 ®，§2.52.22），交給宿主處理才對。
        //
        // **Cmd 是這個平台的「主修飾鍵」**（Windows 是 Ctrl），跟著按鍵一起
        // 送去查表：表裡只有 `Cmd+Shift+空白`（全半形）是刻意收編的，其餘
        // `Cmd+C/V/X/Z` 查不到就回 `None`、讓回宿主。spike 5（§2.52.22）量到
        // 那些**到得了輸入法**，但「到得了」跟「該不該吃」是兩件事。
        //
        // 以前這裡是自己寫「Cmd 一律放行、只開 `Cmd+Shift+空白` 一個洞」，
        // 而且洞一定要開在放行之前（順序寫反過：`keyprobe.log` 裡 50 次
        // `Shift+Cmd+Space` 全是「放行」）。查表之後順序由 `lookup` 保證。
        let flags = event.modifierFlags();
        if flags.contains(NSEventModifierFlags::Control)
            || flags.contains(NSEventModifierFlags::Option)
        {
            return false;
        }
        let shift = flags.contains(NSEventModifierFlags::Shift);
        let ev = KeyEvent {
            key: Self::to_key(event.keyCode(), Self::printable_char(event)),
            shift,
            primary: flags.contains(NSEventModifierFlags::Command),
        };

        let Some(action) = binding::lookup(self.mode(), ev) else {
            return false;
        };

        // **輪替型的動作要擋自動重複**。
        //
        // 按著 `Shift+空白` 不放，系統會持續重送 keyDown（修飾鍵本身不重複，
        // 但空白鍵會）——不擋的話語言鎖定會瘋狂輪替，放開時停在哪一格全看
        // 運氣。一般打字相反，按著注音鍵就是要連續輸入，所以只擋這兩個。
        // 吃掉而不是放行——這一下本來就是我們的鍵。跟 Windows 的
        // `keymap::is_repeat` 那段同一件事。
        if matches!(action, Action::CycleLock | Action::ToggleWidth) && event.isARepeat() {
            return true;
        }

        self.dispatch(action, sender, shift)
    }
}

/// 手勢的四下之間最多能隔多久。跟 Windows 的 `GESTURE_TIMEOUT` 同值。
const GESTURE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1500);

/// 叫起設定頁。手勢（↑↑↓↓）與輸入法選單共用。
///
/// 設定頁是**另一個執行檔**，`spawn` 之後就不管了——輸入法不必知道它長
/// 什麼樣，也不會被它拖垮。
fn open_settings() {
    let Some(exe) = paths::settings_exe() else {
        eprintln!("[通譯] 找不到設定頁執行檔——用 build-app.sh --all 重裝");
        return;
    };
    if let Err(e) = std::process::Command::new(&exe).spawn() {
        eprintln!("[通譯] 開設定頁失敗：{e}");
    }
}

/// 一段組字文字的屬性。
///
/// `segment` 是第幾個「文節」，`active` 代表這一段是不是正在選字的那一格。
fn piece_attrs(segment: usize, active: bool) -> Retained<NSDictionary<NSString, AnyObject>> {
    // **正在選的那一格用加粗底線 ＋ 系統的文字選取底色。**
    //
    // 這是 macOS 的原生做法——日文輸入法選文節時就是這個樣子。用
    // `selectedTextBackgroundColor` 而不是自己的主題色，是因為它會跟著
    // 使用者的強調色走，而且那正是系統其他地方「選取的文字」長的樣子。
    let underline = if active {
        NSUnderlineStyle::Thick
    } else {
        NSUnderlineStyle::Single
    };
    if active {
        let keys: [&NSString; 3] = unsafe {
            [
                NSUnderlineStyleAttributeName,
                NSMarkedClauseSegmentAttributeName,
                NSBackgroundColorAttributeName,
            ]
        };
        let bg = NSColor::selectedTextBackgroundColor();
        let vals: [&AnyObject; 3] = [
            &NSNumber::new_isize(underline.0),
            &NSNumber::new_isize(segment as isize),
            bg.as_ref(),
        ];
        NSDictionary::from_slices(&keys, &vals)
    } else {
        let keys: [&NSString; 2] = unsafe {
            [
                NSUnderlineStyleAttributeName,
                NSMarkedClauseSegmentAttributeName,
            ]
        };
        let vals: [&AnyObject; 2] = [
            &NSNumber::new_isize(underline.0),
            &NSNumber::new_isize(segment as isize),
        ];
        NSDictionary::from_slices(&keys, &vals)
    }
}

fn piece(text: &str, segment: usize, active: bool) -> Retained<NSAttributedString> {
    let s = NSString::from_str(text);
    let attrs = piece_attrs(segment, active);
    unsafe {
        NSAttributedString::initWithString_attributes(NSAttributedString::alloc(), &s, Some(&attrs))
    }
}

impl EchoController {
    /// 畫組字區。傳空字串就是清掉（取消）。
    fn set_marked(&self, sender: &AnyObject, text: &str) {
        let s = self.marked_attributed();
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

        // 候選列。
        //
        // **沒組字就一定收掉**：提示講的是「這串字可以怎樣」，沒有那串字
        // 就沒有提示的對象。組字中則是「有候選字或有提示就開」——只有
        // 提示的情況是打了 `config` 但還沒有候選字可選。
        let (items, sel, cols) = self.visible_candidates();
        let hint = if text.is_empty() {
            String::new()
        } else {
            self.hint()
        };
        if text.is_empty() || (items.is_empty() && hint.is_empty()) {
            candidate_panel::hide();
        } else if let Some(caret) = candidate_panel::caret_rect(sender) {
            candidate_panel::show(&items, sel, cols, &hint, self.as_ref(), caret);
        }
    }
}

impl EchoController {
    /// 送出並清空。
    fn commit(&self, sender: &AnyObject) {
        // ★ 先收面板，而且**無條件收** ★
        //
        // 送出這條路不會經過 `set_marked`，所以「組字區清空時順便收面板」那個
        // 規則在這裡不成立——實測就是這樣漏的：滑鼠點宿主空白處會走
        // `commitComposition:`，字送出去了、面板卻還浮在螢幕上（§2.52.21）。
        //
        // 放在 `is_empty` 的提前返回**之前**：緩衝是空的但面板還開著的狀態
        // 一樣要收得掉，不然它會永遠留在那。
        candidate_panel::hide();
        width_panel::hide();
        self.ivars().seg_menu.set(false);

        let text = self.composition();
        if text.is_empty() {
            self.session().borrow_mut().clear();
            return;
        }

        // ★ 先學再清 ★
        //
        // `learn_on_commit()` 讀的是 `slots`——**清掉就什麼都學不到了**。
        // 我第一版把它放在 `clear()` 之後，症狀是學習檔永遠不會生出來，
        // 而且完全沒有錯誤訊息（它只是回 0 筆）。
        self.learn_and_save();
        self.session().borrow_mut().clear();

        let s = NSString::from_str(&text);
        unsafe {
            let _: () = msg_send![sender, insertText: &*s, replacementRange: none_range()];
        }
    }

    /// 送出之後把這次的選擇記進學習層，有東西才存檔。
    ///
    /// # 密碼欄一律不學
    ///
    /// macOS 的判準是 `IsSecureEventInputEnabled()`——密碼欄會開啟安全輸入。
    /// 那時系統本來就不太會把按鍵交給輸入法，所以**這是第二層保險**
    /// （Windows 那邊的 `state.password` 同一個定位）。那種代價的東西不該
    /// 只靠一條路擋著。
    ///
    /// # 為什麼順便存檔
    ///
    /// 存檔要寫磁碟，不能放在每一鍵的熱路徑上；**送出是天然的節點**——
    /// 使用者打完一段話，那一刻多幾毫秒感覺不到。
    fn learn_and_save(&self) {
        if unsafe { IsSecureEventInputEnabled() } {
            return;
        }
        // `learn_on_commit()` **本身就會記錄**並回傳筆數，所以一次送出
        // 只能叫一次——不是純粹的查詢。
        if self.session().borrow().learn_on_commit() == 0 {
            return;
        }
        let dir = paths::data_dir();
        if let Err(e) = ime_core::learn::save(dir.as_deref()) {
            eprintln!("[通譯] 學習檔存檔失敗：{e}");
        }
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
    /// 安全輸入有沒有開著。**密碼欄會開它**，那時什麼都不該記。
    fn IsSecureEventInputEnabled() -> bool;
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

    // ★ 詞庫在背景載 ★
    //
    // `preload` 會讀好幾十 MB，缺 `dict_ja.bin` 時還要從文字重建（實測
    // 700ms）。**放在主執行緒會拖慢輸入法啟動**——而宿主等我們連上線
    // 只等 3 秒（§2.52.23 的 log 裡看得到那個逾時），拖過頭就變成
    // 「選得到卻打不出字」。
    //
    // core 的詞庫是 `OnceLock` 快取，載完之後任何執行緒建的 `Session`
    // 都看得到，所以背景載是安全的。載完之前打字會查不到候選字——那是
    // 開頭一秒內的事，Windows 那邊也是同一個作法（`background.rs`）。
    // 領域包是另一層，跟詞庫分開找。**開機就印出來**——找不到的話
    // 症狀是「擴充包開了沒作用」，那從畫面上看不出是路徑錯還是包壞了。
    match paths::bundled_packs_dir() {
        Some(dir) => eprintln!("[通譯] 預載包目錄 {}", dir.display()),
        None => eprintln!("[通譯] 沒有預載包目錄（使用者自己裝的包不受影響）"),
    }

    match paths::data_dir() {
        Some(dir) => {
            eprintln!("[通譯] 詞庫目錄 {}", dir.display());
            std::thread::spawn(move || {
                // **執行緒裡的 panic 不會 abort 行程，但會靜悄悄地死掉**——
                // 症狀是「打字查不到任何候選字」而完全沒有訊息。包起來至少
                // 留一行話。
                let t = std::time::Instant::now();
                // **學習記錄跟詞庫一起載**——它是查詢的第三層
                // （學習層→領域包→靜態詞庫），少了它使用者的選字偏好
                // 每次重開都白費。
                let loaded = std::panic::catch_unwind(|| {
                    let n = ime_core::learn::load(Some(&dir));
                    ime_core::preload(&dir, settings::engines());
                    n
                });
                match loaded {
                    Ok(n) => eprintln!("[通譯] 詞庫載完（學習 {n} 條），花了 {:?}", t.elapsed()),
                    Err(_) => eprintln!("[通譯] ★ 載詞庫時 panic，打字會查不到候選字"),
                }
            });
        }
        None => eprintln!("[通譯] ★ 找不到詞庫目錄，打不出字。跑 build-app.sh 重裝"),
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_app_kit::NSEventType;
    use objc2_foundation::NSPoint;

    /// 造一個假的鍵盤事件。
    ///
    /// **兩個字串刻意給不一樣的值**——真實世界裡 `Shift+1` 就是這樣：
    /// `characters` 是 `!`、`charactersIgnoringModifiers` 是 `1`。
    fn key(chars: &str, ignoring: &str) -> Retained<NSEvent> {
        NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            NSEventType::KeyDown,
            NSPoint::new(0.0, 0.0),
            NSEventModifierFlags::Shift,
            0.0,
            0,
            None,
            &NSString::from_str(chars),
            &NSString::from_str(ignoring),
            false,
            18,
        )
        .expect("造得出假事件")
    }

    /// 這條守的是 §2.52.39：問錯 API 會讓所有 Shift 組合鍵失效。
    ///
    /// `charactersIgnoringModifiers` 回的是「沒按修飾鍵的話會是什麼字」，
    /// `Shift+1` 於是變成 `1`——而 `1` 在這個輸入法裡是注音鍵，症狀是
    /// 驚嘆號問號通通打不出來。
    #[test]
    fn shift_的標點要拿實際打出來的字() {
        assert_eq!(EchoController::printable_char(&key("!", "1")), Some('!'));
        assert_eq!(EchoController::printable_char(&key("A", "a")), Some('A'));
    }

    /// `characters()` 空的時候（死鍵、某些版面配置）要退回另一支。
    #[test]
    fn characters_是空的就退回去() {
        assert_eq!(EchoController::printable_char(&key("", "3")), Some('3'));
    }

    /// 功能鍵與控制字元不是「可以打出來的字」。
    ///
    /// macOS 把方向鍵那些放在 Unicode 的私用區（U+F700 起），
    /// `is_control()` 擋不住它們。
    #[test]
    fn 功能鍵與控制字元不算() {
        assert_eq!(
            EchoController::printable_char(&key("\u{F700}", "\u{F700}")),
            None
        );
        assert_eq!(
            EchoController::printable_char(&key("\u{1b}", "\u{1b}")),
            None
        );
    }

    /// 鍵位表搬進 core 之後，macOS 這邊唯一會錯的地方就是翻譯。表裡用到的
    /// 每一顆控制鍵都要翻對，不然那一整列綁定等於消失。
    ///
    /// **控制鍵要看鍵碼不看字元**：空白鍵的字元是 `' '`，照字元走會變成
    /// `Char(' ')`，選字中按空白就不會展開、而是打出一個空白。
    #[test]
    fn 控制鍵照鍵碼翻() {
        for (code, ch, key) in [
            (49, Some(' '), Key::Space),
            (36, Some('\r'), Key::Enter),
            (76, Some('\u{3}'), Key::Enter),
            (48, Some('\t'), Key::Tab),
            (53, Some('\u{1b}'), Key::Esc),
            (51, Some('\u{7f}'), Key::Backspace),
            (123, None, Key::Left),
            (124, None, Key::Right),
            (125, None, Key::Down),
            (126, None, Key::Up),
        ] {
            assert_eq!(EchoController::to_key(code, ch), key, "keyCode {code}");
        }
    }

    /// 以前 macOS 沒分數字鍵盤，**數字鍵盤的 5 會被當成注音的ㄓ**。
    /// `characters()` 對它回的就是 `"5"`，所以一定要靠鍵碼分出來。
    #[test]
    fn 數字鍵盤翻成numpad不是char() {
        let pad = [
            (82, '0'),
            (83, '1'),
            (84, '2'),
            (85, '3'),
            (86, '4'),
            (87, '5'),
            (88, '6'),
            (89, '7'),
            (91, '8'),
            (92, '9'),
            (65, '.'),
            (67, '*'),
            (69, '+'),
            (75, '/'),
            (78, '-'),
        ];
        for (code, ch) in pad {
            assert_eq!(
                EchoController::to_key(code, Some(ch)),
                Key::Numpad(ch),
                "keyCode {code}"
            );
        }
        // 主鍵盤那排的 5（keyCode 23）照字元走
        assert_eq!(EchoController::to_key(23, Some('5')), Key::Char('5'));
    }

    /// 印不出來的鍵（Home、F1、向前刪除）翻成 `Other`——組字中靠它吞掉。
    /// 以前這些會放行給宿主，按 F1 或 Delete 組字就散了。
    #[test]
    fn 印不出來的鍵翻成other() {
        for code in [115u16, 119, 116, 121, 122, 117] {
            assert_eq!(
                EchoController::to_key(code, None),
                Key::Other,
                "keyCode {code}"
            );
        }
        assert_eq!(EchoController::to_key(0, Some('a')), Key::Char('a'));
    }

    /// 刪整格／整段的三態設定。**兩層各查各的設定**，但判斷規則一樣。
    #[test]
    fn 刪整個單位的三態() {
        use DeleteUnitKey::*;
        assert!(!EchoController::want_delete_unit(Off, false));
        assert!(!EchoController::want_delete_unit(Off, true));
        assert!(EchoController::want_delete_unit(Backspace, false));
        assert!(
            EchoController::want_delete_unit(Backspace, true),
            "Shift 也算"
        );
        assert!(!EchoController::want_delete_unit(ShiftBackspace, false));
        assert!(EchoController::want_delete_unit(ShiftBackspace, true));
    }
}
