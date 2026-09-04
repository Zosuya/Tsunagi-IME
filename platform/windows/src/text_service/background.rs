//! 設定重讀、詞庫載入、指令手勢——不直接跟按鍵或文件打交道的雜項。
//!
//! **設定要真的寫回 `config.toml`**：`refresh_config` 會定期重讀再套回
//! session，只改記憶體的話下次重讀就被蓋回去，症狀是「關掉了，過幾秒
//! 又自己開回來」。
//!
//! 詞庫**背景載入**：`OnceLock` 的第二個呼叫者會等，所以先搶著在背景
//! 載、真的要用時自然會等到好，切輸入法那一秒就不見了。

use super::*;

/// 確保詞庫載好。
///
/// 原本只在 `Activate` 載一次。但**重新載入 DLL 之後 `Activate`
/// 不保證跑得到**（宿主可能沿用既有的 TIP 實例，或 `data_dir()`
/// 在那個時間點解析失敗），結果第一次打字查不到任何字，預覽列空白。
///
/// 改成打字前也確認一次。`OnceLock` 保證只真的讀一次檔，
/// 重複呼叫的成本是一個原子讀取。
/// 執行一個指令。
///
/// 設定頁是**另一個執行檔**，用 `Command::spawn` 叫起來就好——
/// 輸入法不必知道它長什麼樣，也不會被它拖垮。
pub(crate) fn run_command(state: &mut State, cmd: ime_core::command::Command) {
    use ime_core::command::Command;
    match cmd {
        Command::OpenSettings => open_settings(),
        Command::ToggleEngine(lang) => toggle_engine(state, lang),
    }
}

/// 開關一個語言引擎，並**寫回設定檔**。
///
/// # 為什麼一定要寫檔
///
/// 引擎開關存在 `config.toml`，而 `refresh_config` 會定期重讀那個檔
/// 再套回 session。只改記憶體裡的狀態的話，**下一次重讀就被覆蓋回去**
/// ——使用者會看到「關掉了，過幾秒又自己開回來」。
///
/// 寫檔之後檔案的時間戳變了，`refresh_config` 下次會讀到新值，兩邊
/// 自然一致，不必另外同步。
pub(crate) fn toggle_engine(state: &mut State, lang: ime_core::language::Language) {
    let now = state.config.behavior.engines.toggle(lang);
    let engines = state.config.behavior.engines;
    state.session.set_engines(engines);
    if let Err(e) = state.config.save() {
        crate::dlog!("[指令] 寫設定檔失敗 {e}");
    }
    // 剛打開的引擎詞庫可能還沒載——背景補上，不要卡在這裡
    if now {
        spawn_dict_load(engines);
    }
    // **不彈提示視窗**：狀態改在提示列上一直看得見（見 `show_candidates`
    // 的 `off_hint`），那比閃一下就消失的浮動視窗有用——這是持久的設定，
    // 使用者下次打字時才會想起「咦怎麼打不出日文」。
    crate::dlog!("[指令] {} {lang:?}", if now { "啟用" } else { "停用" });
}

/// 叫起設定頁。指令手勢（↑↑↓↓）與語言列的右鍵選單共用。
pub(crate) fn open_settings() {
    // 設定 exe 跟 DLL 放在同一個資料夾
    let Some(exe) = crate::registration::sibling_path("ime_settings.exe") else {
        return;
    };
    let _ = std::process::Command::new(exe).spawn();
}

/// 設定檔改過就重讀。
///
/// # 為什麼用時間戳而不是廣播通知
///
/// 設定頁是另一個行程，而輸入法同時活在好幾個宿主行程裡
/// （記事本一份、瀏覽器一份）。要「按下儲存立刻生效」得廣播訊息
/// 給每一份，那要多一個隱形視窗收訊息。
///
/// 比對檔案時間戳便宜得多，代價只是**要打下一個字才生效**——
/// 使用者按了儲存、切回文件打第一個字，中間那零點幾秒感覺不到。
pub(crate) fn refresh_config(state: &mut State) {
    // **節流**：每開始組字都問檔案系統太浪費。設定不會一秒改好幾次，
    // 隔一段時間看一次就夠了。
    let now = std::time::Instant::now();
    if state
        .config_checked
        .is_some_and(|t| now.duration_since(t) < CONFIG_CHECK_INTERVAL)
    {
        return;
    }
    state.config_checked = Some(now);

    let dir = crate::registration::data_dir();
    let stamp = ime_core::config::modified_at(dir.as_deref());
    if stamp != state.config_stamp {
        state.config_stamp = stamp;
        state.config = ime_core::config::Config::load(dir.as_deref());
        apply_config(state);
    }

    // 領域包是**獨立的一層**，換掉索引就生效，不必重建詞庫
    // （詞庫是 `OnceLock`，重建不了）。所以這裡不必跟著設定一起走，
    // 只要包的檔案時間變了就重載——使用者直接編輯包也吃得到。
    let ps = ime_core::pack::stamp(
        &state.config.behavior.packs_dir,
        &state.config.behavior.packs,
    );
    if ps != state.pack_stamp {
        state.pack_stamp = ps;
        let n = ime_core::pack::load(
            &state.config.behavior.packs_dir,
            &state.config.behavior.packs,
        );
        crate::dlog!("[設定] 領域包載入 {} 條", n);
    }
}

/// 把讀進來的設定套到各處。
fn apply_config(state: &mut State) {
    // 全半形的開機預設。使用者按 Shift+空白切過之後以那個為準，
    // 直到下次重讀設定。
    state.session.set_width(state.config.behavior.width);
    // 啟用哪些語言引擎。關掉的**連自動辨識都跳過**，
    // 見 `ime_core::config::Engines`。
    state.session.set_engines(state.config.behavior.engines);
    // 鎖定時倒退鍵刪整格，見 `Session::delete_marked_slot`
    state
        .session
        .set_backspace_whole_cell(state.config.behavior.backspace_whole_cell);
    // 鎖定注音時 , . ; / - 這五個一鍵兩用的鍵怎麼處理
    state
        .session
        .set_lock_punct(state.config.behavior.lock_punct);
    // 外觀立刻套用到候選視窗
    crate::candidate_window::set_theme(crate::theme::Theme::from_config(&state.config));
}

/// 把詞庫丟到背景載，`Activate` 就不必等。
///
/// # 為什麼可以放心開執行緒
///
/// 1. **DLL 不會在行程活著時被卸載**——`DllCanUnloadNow` 一律回
///    `S_FALSE`（見 `lib.rs`），所以背景執行緒不會做到一半踩空。
/// 2. **載到一半使用者就打字也安全**——詞庫是 `OnceLock`，第二個
///    呼叫者會**擋在那裡等載完**，不會看到半成品。按鍵那條路本來
///    就有一次 `ensure_dict_loaded`，剛好接住這種情況：使用者只會
///    等剩下的時間，而不是從頭等。
///
/// # 為什麼要擋重複
///
/// 每切一次 App 就 `Activate` 一次，不擋的話會開出一堆執行緒——
/// 它們只是排隊等同一個 `OnceLock`，毫無意義。
pub(crate) fn spawn_dict_load(engines: ime_core::config::Engines) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static LOADING: AtomicBool = AtomicBool::new(false);

    // 該載的都載好了就什麼都不必做。
    //
    // **判準不是「載過一次沒」**——使用者可能在設定裡才把日文打開，
    // 那時要能補載。看的是「現在需要的那幾本在不在」。
    let ready = (!engines.bopomofo || ime_core::dict::bopomofo_loaded())
        && (!engines.romaji || ime_core::dict::japanese_loaded());
    if ready {
        return;
    }
    // 已經有一條在載了就不必再開
    if LOADING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        let t = std::time::Instant::now();
        ensure_dict_loaded(engines);
        crate::dlog!("[啟用] 詞庫背景載入完成 {}ms", t.elapsed().as_millis());
        LOADING.store(false, Ordering::SeqCst);
    });
}

pub(crate) fn ensure_dict_loaded(engines: ime_core::config::Engines) {
    if let Some(dir) = crate::registration::data_dir() {
        // 學習記錄跟詞庫一起載——它是分層的第三層，見 `ime_core::learn`
        let n = ime_core::learn::load(Some(&dir));
        crate::dlog!("[學習] 載入 {} 條", n);
        // 不必自己擋重複呼叫——每本詞庫各自是 `OnceLock`，
        // already-loaded 的情況只是一次原子讀取
        ime_core::preload(&dir, engines);
    }
}
