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

    /// 段選單的按鍵。回 `None` 代表這個鍵不歸段選單管。
    ///
    /// 鍵位照 Windows 的 `DEFAULT_BINDINGS`。**跟選字同一組手勢，只是反白
    /// 的單位是「段」不是「格」**——使用者不必學新東西，換一個粒度而已。
    ///
    /// | 鍵 | 段選單 | 選字 |
    /// |---|---|---|
    /// | ←→ | 換一段 | 換一格 |
    /// | ↑↓ | 換這段的解釋 | 換這格的字 |
    /// | Shift+←→ | 推這段的邊界 | 推日文詞界 |
    /// | 1-9 | 直接挑 | 直接挑 |
    fn on_key_segmenu(
        &self,
        sender: &AnyObject,
        code: u16,
        ch: Option<char>,
        shift: bool,
    ) -> Option<bool> {
        // 數字直接挑這一段的第 n 個解釋，挑完直接定案。
        if let Some(n) = ch
            .and_then(|c| c.to_digit(10))
            .filter(|d| (1..=9).contains(d))
            .map(|d| d as usize - 1)
        {
            let mut s = self.session().borrow_mut();
            if let Some(i) = s.seg_number_index(n) {
                s.seg_set_cand(i);
                // 挑了就直接定案（數字鍵的語意是「就是這個」），但**往不往
                // 下一段仍照 `enter_in_segmenu`**——跟 Enter 同一個開關，
                // 不然數字鍵與 Enter 的行為會不一致。
                s.seg_confirm_with(settings::enter_advance_seg());
            }
            drop(s);
            self.after_seg(sender);
            return Some(true);
        }

        {
            let mut s = self.session().borrow_mut();
            match code {
                // ── 台語模式下這四顆鍵換的是「詞」不是「段」 ──
                //
                // 鎖定注音＋裝了台語包時（`tw_mode()`），整串按鍵就是一段，
                // 換段沒有意義；反白單位改成「詞」（`tw_words` 的最大匹配
                // 斷詞），所以 `←→` 是跳詞、`Shift+←→` 是調那個詞的寬度
                // （「我們」↔「我」）。
                //
                // **起點固定、只調長度**（使用者裁定）：要選「們」用 `←→`
                // 跳過去，不必再加一組推左邊界的鍵。
                //
                // 這四處跟 Windows 的 `SegRight`／`SegLeft`／`SegWiden`／
                // `SegNarrow` 一字對一字。**接上語言鎖定之後這條路才通**
                // （§2.52.46），在那之前 macOS 根本進不了台語模式。
                124 if shift && s.tw_mode() => s.tw_widen(),
                123 if shift && s.tw_mode() => s.tw_narrow(),
                124 if shift => s.seg_widen(),
                123 if shift => s.seg_narrow(),
                124 if s.tw_mode() => s.tw_next(),
                123 if s.tw_mode() => s.tw_prev(),
                124 => s.seg_right(),
                123 => s.seg_left(),
                125 => s.seg_next_cand(),
                126 => s.seg_prev_cand(),
                // Enter 是「選定這一段」——前面定案、後面重算，反白自動
                // 移到下一段。**不是送出**：送出要先關掉選單回到打字。
                //
                // **段選單有自己的開關**（`behavior.enter_in_segmenu`，使用者
                // 要求 2026-09-09 拆開）：`Next` 是「選完往下一段」，`Exit`
                // 是「選完就停住」。
                //
                // 原本跟選字的 `enter_in_select` 共用，但兩層的粒度不同——
                // 選字是逐**字**挑、段選單是逐**段**挑，習慣可以不一樣。
                // **這裡查錯一支不會有任何症狀**，只有使用者去改那個設定時
                // 才會發現改的是另一層。
                36 | 76 => s.seg_confirm_with(settings::enter_advance_seg()),
                // **TAB 與 Esc 都是關掉選單、退回組字狀態**，已經定案的段留著。
                //
                // Windows 那邊兩顆鍵**效果不同**：`Esc` 走 `SegReset`
                // ——除了關選單，還會**丟掉已定案的段**，整串按鍵重新交給
                // 引擎判斷；`TAB` 才是「關掉但保留」。
                //
                // macOS 這裡兩顆都是「關掉但保留」（使用者裁定）。差別只在
                // 已定案的段留不留，關選單那半是一樣的。`seg_reset()` 因此
                // 在 macOS 沒有入口——要重來就關掉選單、用 Esc 取消組字重打。
                48 | 53 => {
                    drop(s);
                    self.ivars().seg_menu.set(false);
                    self.refresh(sender);
                    return Some(true);
                }
                51 => s.backspace(),
                _ => return None,
            }
        }
        self.after_seg(sender);
        Some(true)
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

    /// 選字模式的按鍵。
    ///
    /// 回 `None` 代表「這個鍵不歸選字管」，讓打字那邊接手（例如選字中
    /// 直接打注音）。鍵位照 Windows 的 `DEFAULT_BINDINGS` 對過，兩個平台
    /// 的手勢要一樣——使用者不該為了換平台重學。
    ///
    /// | 鍵 | 未展開 | 展開後 |
    /// |---|---|---|
    /// | ↑↓ | 換候選字 | 同欄上下 |
    /// | ←→ | 在字與字之間移動 | 換欄 |
    /// | 空白 | 展開全部 | 收合（同一個鍵開關） |
    /// | 1-9 | 直接挑**這一頁**的第 n 個 | 同左 |
    /// | Enter | 選中反白的，往下一格 | 同左 |
    /// | Esc | 離開選字 | 先收合，再按才離開 |
    fn on_key_selecting(
        &self,
        sender: &AnyObject,
        code: u16,
        ch: Option<char>,
        shift: bool,
    ) -> Option<bool> {
        let expanded = self.session().borrow().cand_expanded();

        // **數字是選字，不是輸入**——打字時十個數字全是注音鍵，但選字時
        // 已經在挑字了，數字就空出來了（跟新注音一致）。
        if let Some(n) = ch
            .and_then(|c| c.to_digit(10))
            .filter(|d| (1..=9).contains(d))
            .map(|d| d as usize - 1)
        {
            let pick = {
                let s = self.session().borrow();
                // 清單沒開就沒東西可以按號碼選——那時畫面上只有框，
                // 使用者看不到編號，按下去等於盲選。
                if !s.cands_open() {
                    return Some(true);
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
            return Some(true);
        }

        // 空白：展開／收合的開關。
        if ch == Some(' ') {
            let mut s = self.session().borrow_mut();
            if expanded {
                s.collapse_cands();
            } else {
                s.expand_cands();
            }
            drop(s);
            self.refresh(sender);
            return Some(true);
        }

        {
            let mut s = self.session().borrow_mut();
            match code {
                // ── Shift+←→：日文詞界（文節）伸縮 ──
                //
                // 日文 IME 的通用慣例，而且我們的 Shift+方向鍵本來就是空的。
                // **只在未展開時有意義**——展開後 ←→ 是換欄，那時框停在
                // 哪一格已經不是重點了（Windows 也只綁在 `Selecting`，
                // 沒綁 `SelectingExpanded`）。
                //
                // 推不動就**什麼都不做**（已經到頭了），但仍然回「吃掉」
                // ——放行的話宿主會拿去移動自己的游標，組字就散了。
                124 if shift && !expanded => {
                    if !s.widen_word() {
                        return Some(true);
                    }
                }
                123 if shift && !expanded => {
                    if !s.narrow_word() {
                        return Some(true);
                    }
                }
                // 左 123／右 124：未展開是換格，展開後是換欄
                123 if expanded => s.cand_left_column(),
                124 if expanded => s.cand_right_column(),
                123 => s.select_left(),
                124 => s.select_right(),
                // 下 125／上 126：換候選字
                125 => s.next_cand(),
                126 => s.prev_cand(),
                // Enter：選中反白的那個。預設往下一格（新注音式），
                // 沒有下一格就離開選字。**不是送出**——送出要退出選字
                // 回到打字再按一次。
                36 | 76 => {
                    // **照設定走**（`behavior.enter_in_select`）：新注音式
                    // 是選完往下一格繼續選，另一種是選完直接離開選字。
                    //
                    // 數字鍵與滑鼠那兩條路**刻意不看這個設定**，一律當成
                    // 「我要這個」直接收掉清單——那是明確指名某一個候選，
                    // 跟方向鍵逐格慢慢挑不是同一回事（Windows 的數字鍵也
                    // 一樣不套）。
                    let left_select = s.confirm_cand_with(settings::enter_advance());
                    // 離開選字（沒有下一格可挑了）而且設定要「選完直接送出」
                    // 的話，就在這裡送出，不必再按一次 Enter。
                    if left_select && settings::commit_on_last() {
                        drop(s);
                        self.ivars().seg_menu.set(false);
                        self.commit(sender);
                        return Some(true);
                    }
                }
                // Esc：展開時先收合，再按一次才離開選字。
                53 if expanded => s.collapse_cands(),
                53 => s.exit_select(),
                51 => s.backspace(),
                _ => return None,
            }
        }
        self.refresh(sender);
        Some(true)
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

        // ★ 帶 Cmd／Ctrl／Option 的一律讓回宿主 ★
        //
        // **這是快捷鍵，不是輸入。** `charactersIgnoringModifiers` 對
        // `Cmd+C` 回的是 `"c"`——單一可見字元，底下那條「收印得出來的
        // 字元」會把它當成打字吞進組字區，結果就是複製貼上全被輸入法
        // 攔走（實測回報）。
        //
        // spike 5（§2.52.22）量到 `Cmd+C/V/X/Z` **到得了輸入法**，但
        // 「到得了」跟「該不該吃」是兩件事——到得了只代表我們有機會
        // 決定，而正確的決定是放行。
        //
        // Shift 不算：`Shift+字母`是打大寫，那是真的輸入。
        //
        // Option 也放行。macOS 的 `Option+字母` 是輸入特殊字元的正規用法
        // （`Option+A` 打 å、`Option+R` 打 ®，§2.52.22），交給宿主處理才
        // 對；**要不要改成由輸入法自己產生那些字元，接 core 時再決定**。
        let flags = event.modifierFlags();
        let code = event.keyCode();

        // ★ 唯一的例外：`Cmd+Shift+空白`（全半形）★
        //
        // **一定要在放行之前判**——順序寫反的話 `Cmd` 系會在這裡整批讓回
        // 宿主，我們的判斷永遠輪不到（實測踩過：`keyprobe.log` 裡 50 次
        // `Shift+Cmd+Space` 全是「放行」）。
        //
        // 洞開得很窄：只有這一個組合，其餘 Cmd 系照舊。
        if code == 49
            && flags.contains(NSEventModifierFlags::Command)
            && flags.contains(NSEventModifierFlags::Shift)
            && !flags.contains(NSEventModifierFlags::Option)
        {
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
            return true;
        }

        if flags.contains(NSEventModifierFlags::Command)
            || flags.contains(NSEventModifierFlags::Control)
            || flags.contains(NSEventModifierFlags::Option)
        {
            return false;
        }

        let shift = flags.contains(NSEventModifierFlags::Shift);

        // ── Shift+空白：語言鎖定輪替 ──
        //
        // 自動 → 注音 → 日文 → 英文。全半形是 `Cmd+Shift+空白`，判斷在更
        // 上面（那個要在 Cmd 放行之前攔）。
        //
        // # 為什麼不是 Windows 那邊的「單按 Ctrl」
        //
        // **`Ctrl+…` 根本到不了輸入法。** 這不是推論，是量出來的：
        // `keyprobe.log` 累積 154KB、涵蓋 spike 5 那輪完整的快捷鍵量測，
        // **一筆 `Ctrl` 組合都沒有**；同一份紀錄裡 `Cmd+V` 23 次、`Cmd+A`
        // 5 次、`Cmd+C` 4 次、`Cmd+Shift+空白` 50 次，全都進得來。macOS 把
        // Ctrl 系當成應用層／系統層的指令事件，IMK 不轉給我們。
        //
        // 挑組合鍵時**要分開驗兩件事**（§2.52.22 的教訓）：
        //
        // 1. 系統快捷鍵有沒有佔用它——查 `com.apple.symbolichotkeys`
        // 2. **它到不到得了我們**——只能靠 `keyprobe` 實測
        //
        // 第一次選 `⌃⇧空白` 就是只驗了第 1 件（0 衝突）而漏了第 2 件。
        //
        // 也不用「單按 Ctrl」：修飾鍵走 `flagsChanged`，IMK 預設不送，要放寬
        // `recognizedEvents:` 才收得到——而放寬之後**系統就不再自動送
        // `commitComposition:`**（點組字區外面的收尾），得自己補。
        if code == 49 && flags.contains(NSEventModifierFlags::Shift) {
            self.cycle_lock(sender);
            return true;
        }

        // ── 段選單最優先 ──
        //
        // **兩層是互斥的**（使用者裁定）：段選單開著就只選段，不進選字。
        if self.ivars().seg_menu.get() {
            if let Some(handled) =
                self.on_key_segmenu(sender, code, Self::printable_char(event), shift)
            {
                return handled;
            }
        }

        // ── TAB 開段選單 ──
        //
        // 打字中與選字中都是同一個入口（跟 Windows 一致）。開之前先退出
        // 選字——兩層互斥。
        if code == 48 && self.is_composing() {
            {
                let mut s = self.session().borrow_mut();
                s.exit_select();
                s.seg_open();
            }
            self.ivars().seg_menu.set(true);
            self.refresh(sender);
            return true;
        }

        // ── 選字模式優先 ──
        //
        // 同一顆鍵在兩個模式意思不同（方向鍵在打字中是「進選字」，在選字
        // 中是「換字／換格」），所以要先分模式再看鍵。回 `None` 代表這個鍵
        // 不歸選字管，往下走打字那條——例如選字中直接打注音。
        if self.is_selecting() {
            if let Some(handled) =
                self.on_key_selecting(sender, code, Self::printable_char(event), shift)
            {
                return handled;
            }
        }

        match code {
            // Esc：有字就取消（吃掉），沒字就讓回宿主
            53 => {
                if !self.is_composing() {
                    return false;
                }
                self.session().borrow_mut().clear();
                self.set_marked(sender, "");
                true
            }
            // Return / Enter：有字就送出
            36 | 76 => {
                if !self.is_composing() {
                    return false;
                }
                self.commit(sender);
                true
            }
            // ── 方向鍵：組字中進選字，沒組字就讓回宿主 ──
            //
            // 讓回去的話宿主會去移動它自己的游標，組字區當場被打斷。
            //
            // **左鍵從最後一格進**：使用者按左鍵的直覺是「從右邊選過來」，
            // 從第一格進來會看起來像跳過了最後一個字。這條跟 Windows 的
            // `EnterSelectLast` 一致。
            // ↑↓ 先走手勢偵測（`ime_core::command::Gesture`）——組字內容
            // 是指令時，上上下下就直接執行，不必往下找選項。湊不成手勢
            // 的話原樣退回「進選字」，方向鍵不會因為多了手勢而失去本來
            // 的功能。
            125 | 126 if self.is_composing() => {
                let dir = if code == 126 {
                    ime_core::command::Dir::Up
                } else {
                    ime_core::command::Dir::Down
                };
                self.on_gesture(sender, dir);
                true
            }
            123..=126 if self.is_composing() => {
                {
                    let mut sess = self.session().borrow_mut();
                    if code == 123 {
                        sess.enter_select_last();
                    } else {
                        sess.enter_select_first();
                    }
                    // 進了選字就把清單打開，不然畫面上只有框、沒有候選字
                    // 可以看（Windows 的 `PickChar` 註解講的是同一件事）。
                    sess.open_cands();
                }
                self.refresh(sender);
                true
            }
            // Home 115／PageUp 116／End 119／PageDown 121：組字中吃掉但不
            // 做事——它們同樣會移動宿主的游標，放行就散了。
            123..=126 | 115 | 116 | 119 | 121 => self.is_composing(),
            // Backspace：退一個字（**按字元退，不是位元組**）
            51 => {
                if !self.is_composing() {
                    return false;
                }
                self.session().borrow_mut().backspace();
                let now = self.composition();
                self.set_marked(sender, &now);
                true
            }
            _ => {
                let Some(c) = Self::printable_char(event) else {
                    return false;
                };
                // **沒在組字時的空白就是空白**，讓回宿主。組字中的空白是
                // 注音的一聲，要吃掉——一聲必須前面已經有構成合法注音的
                // 鍵，不會憑空從空白開始（跟 Windows 的鍵位表一致）。
                if c == ' ' && !self.is_composing() {
                    return false;
                }
                self.session().borrow_mut().push(c);
                self.refresh(sender);
                true
            }
        }
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
}
