//! 設定檔：載入、熱重載、套用。
//!
//! 對應 Windows 的 `text_service/background.rs` 裡那半——那邊設定放在
//! `State`（一個文字服務一份），macOS 這裡放在**行程層級**：
//! `IMKInputController` 是每個宿主連線各一個，設定卻是整台機器共用的，
//! 每個控制器各存一份只會多出同步問題。
//!
//! # 設定檔在哪
//!
//! `ime_core::config::Config::find` 已經定好了：先看使用者目錄
//! （macOS 是 `~/Library/Application Support/tsunagi-ime/config.toml`），
//! 沒有才回頭看專案的 `data/config.toml`（內建預設）。**平台層不必決定
//! 位置**，跟學習檔同一個道理。

use std::cell::RefCell;
use std::time::{Duration, Instant, SystemTime};

use ime_core::config::Config;
use ime_core::theme::Theme;

/// 多久看一次檔案時間。
///
/// **每按一鍵都問檔案系統太浪費**，而設定不會一秒改好幾次。跟 Windows
/// 的 `CONFIG_CHECK_INTERVAL` 取同一個值，兩邊的反應速度才一致。
const CHECK_INTERVAL: Duration = Duration::from_secs(2);

struct Loaded {
    config: Config,
    theme: Theme,
    /// 設定檔的修改時間。跟上次不一樣就重載。
    stamp: Option<SystemTime>,
    /// 上次看檔案時間是什麼時候，用來節流。
    checked: Instant,
    /// 領域包的時間戳。**跟設定檔分開記**——見 `refresh` 的說明。
    pack_stamp: Option<SystemTime>,
}

thread_local! {
    /// **主執行緒限定**——IMK 的回呼都在主執行緒，不必上鎖。
    static CFG: RefCell<Option<Loaded>> = const { RefCell::new(None) };
}

fn data_dir() -> Option<std::path::PathBuf> {
    crate::paths::data_dir()
}

fn load() -> Loaded {
    let dir = data_dir();
    let config = Config::load(dir.as_deref());
    let theme = Theme::from_config(&config);
    Loaded {
        config,
        theme,
        stamp: ime_core::config::modified_at(dir.as_deref()),
        checked: Instant::now(),
        // **故意留 `None`**：這樣第一次 `refresh` 一定會載一次包。
        pack_stamp: None,
    }
}

/// 隨程式一起裝的那份包在哪，**整個行程只算一次**。
///
/// 必須在第一次 `pack::stamp` 之前設好（那支會去翻這個目錄），所以
/// 每次檢查包之前都叫一下，`Once` 保證只真的做一次。
fn ensure_bundled_packs() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| ime_core::pack::set_bundled_dir(crate::paths::bundled_packs_dir()));
}

/// 領域包該不該重載。
///
/// **跟設定檔分開判斷**：包是獨立的一層，換掉索引就生效，不必重建詞庫
/// （詞庫是 `OnceLock`，重建不了）。而且使用者可能**直接編輯包檔案**而
/// 沒動設定——只看設定的時間戳就會漏掉那種情況。
fn refresh_packs(l: &mut Loaded) {
    ensure_bundled_packs();
    let b = &l.config.behavior;
    let ps = ime_core::pack::stamp(&b.packs_dir, &b.packs);
    if ps == l.pack_stamp {
        return;
    }
    l.pack_stamp = ps;
    let n = ime_core::pack::load(&b.packs_dir, &b.packs);
    // **`load` 的回傳只數 en／ja／zh 三個詞表**，符號不在裡面——只印它的話
    // 純符號包（內建的那兩個就是）會顯示「載入 0 條」，看起來像失敗。
    // 實測踩過，所以這裡把索引實際的內容印出來。
    let idx = ime_core::pack::index();
    eprintln!("[通譯] 領域包：詞 {n} 條、符號 {} 組", idx.sym.len());
}

/// 確保設定是新的。**回傳「這次有沒有重載」**——有的話呼叫端要重新套用。
///
/// 節流過，可以放在每一鍵的路徑上。
pub fn refresh() -> bool {
    CFG.with(|cell| {
        let mut slot = cell.borrow_mut();
        let Some(cur) = slot.as_mut() else {
            let mut fresh = load();
            refresh_packs(&mut fresh);
            *slot = Some(fresh);
            return true;
        };
        if cur.checked.elapsed() < CHECK_INTERVAL {
            return false;
        }
        cur.checked = Instant::now();
        let stamp = ime_core::config::modified_at(data_dir().as_deref());
        let changed = stamp != cur.stamp;
        if changed {
            *slot = Some(load());
        }
        // **包的檢查在設定之外**：設定沒變不代表包沒變（使用者可能直接
        // 編輯包檔案），所以不能放在上面那個 `changed` 裡面。
        if let Some(l) = slot.as_mut() {
            refresh_packs(l);
        }
        changed
    })
}

/// 目前的主題。面板每次顯示都問它一次——換設定要立刻看得到。
pub fn theme() -> Theme {
    CFG.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(load());
        }
        slot.as_ref().map(|l| l.theme.clone()).unwrap_or_default()
    })
}

/// 把設定套到一個 `Session` 上。
///
/// 逐項跟 Windows 的 `apply_config` 對齊——**漏掉一項就是兩個平台行為
/// 分岔**，而那種分岔不會有任何錯誤訊息。
pub fn apply_to(session: &mut ime_core::session::Session) {
    CFG.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(load());
        }
        let Some(l) = slot.as_ref() else { return };

        // ★ 解構，而且**不准寫 `..`** ★
        //
        // 這是這個檔案裡最重要的一行。`Behavior` 加了新欄位的話，這裡會
        // **編譯不過**，逼你在 macOS 這邊明講怎麼處理它。
        //
        // # 為什麼需要這道機制
        //
        // 「設定頁改得動、平台層不理它」這種洞**不會有任何症狀**：編譯過、
        // 控制項畫得出來、使用者也改得動，就是沒反應。macOS 移植的過程中
        // 連續踩了四次（領域包、`enter_in_select`、`commit_on_last`，
        // 加上設定頁的預覽字型），見開發文件 §2.52.38。
        //
        // 用測試去掃也不可靠——欄位可能透過方法（`overlay_alpha()`）或
        // 被複製進 `Theme` 才被讀到，grep 一律誤報。**只有編譯器數得準。**
        //
        // 用不到的欄位就 `let _ =` 掉並寫清楚為什麼——把「靜默的洞」換成
        // 「一句明確的決定」。
        let ime_core::config::Behavior {
            enter_in_select,
            commit_on_last,
            enter_in_segmenu,
            commit_on_last_seg,
            width,
            engines,
            backspace_whole_cell,
            packs,
            packs_dir,
            lock_punct,
            ctrl_punct,
        } = &l.config.behavior;

        // 全半形的開機預設。使用者按 Shift+空白切過之後以那個為準，
        // 直到下次重讀設定。
        session.set_width(*width);
        // 啟用哪些語言引擎。關掉的**連自動辨識都跳過**。
        session.set_engines(*engines);
        // 鎖定時倒退鍵刪整格。
        session.set_backspace_whole_cell(*backspace_whole_cell);
        // 鎖定注音時 , . ; / - 這五個一鍵兩用的鍵怎麼處理。
        session.set_lock_punct(*lock_punct);

        // ── 以下不在這裡套用，但**必須交代** ──
        //
        // 選字按 Enter 的行為：在按鍵當下現查（`enter_advance()`），因為它
        // 只影響那一瞬間的判斷，不是 session 的持久狀態。
        let _ = enter_in_select;
        // 最後一格選完直接送出：同上，在 `after_seg` 與選字的 Enter 現查。
        let _ = commit_on_last;
        // **段選單自己一組**（`enter_in_segmenu`／`commit_on_last_seg`）：
        // 兩層的操作節奏不同，使用者要求拆開，見 `config::Behavior`。
        // 兩支都在按鍵當下現查——`enter_in_segmenu` 走 `enter_advance_seg()`、
        // `commit_on_last_seg` 走 `commit_on_last_seg()`，理由同上面兩支。
        let _ = enter_in_segmenu;
        let _ = commit_on_last_seg;
        // 領域包：**獨立的一層**，換掉索引就生效，時間戳也跟設定分開記，
        // 所以走 `refresh_packs` 而不是這裡（見那支的說明）。
        let _ = (packs, packs_dir);
        // ★ macOS 用不到 ★
        //
        // `ctrl_punct` 是「鎖定注音時按 Ctrl+標點鍵明講我要標點」的逃生口，
        // 而 **macOS 不做語言鎖定**（使用者裁定：要鎖定就回頭用系統內建的
        // 輸入法）。沒有鎖定就沒有需要逃生的情境。
        //
        // 而且 macOS 這邊 `Ctrl` 是整批讓回宿主的（§2.52.22），要接的話
        // 得先在 `on_key` 開一個洞給它，不是加一行就好。
        let _ = ctrl_punct;
    });
}

/// 開關一個語言引擎，**並寫回設定檔**。回傳「現在是開著嗎」。
///
/// # 為什麼一定要寫檔
///
/// 引擎開關存在 `config.toml`，而 `refresh()` 會定期重讀那個檔再套回
/// session。只改記憶體的話**下一次重讀就被蓋回去**——使用者會看到
/// 「關掉了，過幾秒又自己開回來」。這條 Windows 那邊踩過（`toggle_engine`
/// 的註解）。
///
/// 寫完直接整份重載：檔案是唯一的真相，重載一次就不必自己維護時間戳。
pub fn toggle_engine(lang: ime_core::language::Language) -> bool {
    CFG.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(load());
        }
        let Some(l) = slot.as_mut() else {
            return false;
        };
        let now = l.config.behavior.engines.toggle(lang);
        if let Err(e) = l.config.save() {
            eprintln!("[通譯] 寫設定檔失敗：{e}");
        }
        *slot = Some(load());
        now
    })
}

/// 選字時按 Enter 要不要移到下一格。
///
/// 對應設定 `behavior.enter_in_select`：`Next` 是新注音式（選完往下一格
/// 繼續選），`Exit` 是選完就離開選字。
pub fn enter_advance() -> bool {
    use ime_core::config::EnterInSelect;
    CFG.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(load());
        }
        slot.as_ref()
            .map(|l| l.config.behavior.enter_in_select == EnterInSelect::Next)
            .unwrap_or(true)
    })
}

/// 最後一格選完要不要直接送出。
///
/// 對應設定 `behavior.commit_on_last`。`false` 只離開選字狀態，字還留在
/// 組字區，要再按一次 Enter 才送出。
pub fn commit_on_last() -> bool {
    CFG.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(load());
        }
        slot.as_ref()
            .map(|l| l.config.behavior.commit_on_last)
            .unwrap_or(false)
    })
}

/// 段選單按 Enter 要不要移到下一段。
///
/// 對應設定 `behavior.enter_in_segmenu`。**跟選字的 `enter_in_select` 是
/// 分開的兩組設定**——選字是逐**字**挑、段選單是逐**段**挑，粒度不同，
/// 習慣可以不一樣（使用者要求拆開，2026-09-09）。查錯一支不會有編譯錯誤
/// 也不會有測試失敗，只會在使用者手上壞掉。
pub fn enter_advance_seg() -> bool {
    use ime_core::config::EnterInSelect;
    CFG.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(load());
        }
        slot.as_ref()
            .map(|l| l.config.behavior.enter_in_segmenu == EnterInSelect::Next)
            .unwrap_or(true)
    })
}

/// 段選單的最後一段選完要不要直接送出。
///
/// **跟選字的 `commit_on_last` 是分開的兩組設定**——兩層的操作節奏不同，
/// 使用者要求拆開（2026-09-09）。Windows 那邊 `after_seg` 的對應處也是查
/// 這一支，兩個平台不能一邊查舊的。
pub fn commit_on_last_seg() -> bool {
    CFG.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(load());
        }
        slot.as_ref()
            .map(|l| l.config.behavior.commit_on_last_seg)
            .unwrap_or(false)
    })
}

/// 詞庫預載要用的引擎設定。
pub fn engines() -> ime_core::config::Engines {
    CFG.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(load());
        }
        slot.as_ref()
            .map(|l| l.config.behavior.engines)
            .unwrap_or_default()
    })
}
