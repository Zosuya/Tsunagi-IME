//! 擴充包編輯器：**不必再開記事本**。
//!
//! 見開發文件 §2.75。原本要加一個詞得離開輸入法去開記事本、記得存成
//! UTF-8、手打 Tab、還要自己拼出注音符號——格式很容易弄壞，而注音
//! 符號本來就是引擎的內部格式，不該外洩給使用者。
//!
//! # 兩個狀態
//!
//! **預設是預覽**（唯讀，看得到裡面有什麼），上面一顆「編輯」；按下去
//! 進入編輯狀態，同一顆按鈕變成「儲存」。改壞了可以取消。
//!
//! # 語言不是選的，是算出來的
//!
//! 使用者不必知道 `en`／`zh`／`ja` 這些代號，那些只存在於檔案裡。
//! 按鍵一改就重算——引擎的 `Input::from_keys` 現成就在做「一串按鍵
//! → 語言段落」。
//!
//! # 「怎麼打」那欄：顯示用人話，編輯用按鍵
//!
//! `cj6wl6` 對使用者就是一串亂碼，`ㄏㄨˊㄊㄠˊ` 才看得懂。所以平常
//! 中文顯示注音、日文顯示假名；**點進去編輯才變成按鍵**讓他打，跳出來
//! 又變回去。使用者能打的是按鍵，看得懂的是注音。
//!
//! # 檔案格式完全不動
//!
//! 存出來還是一樣的三欄 Tab、一樣的 `# name:` 檔頭、一樣 UTF-8，
//! `pack.rs` 的 `parse` 一行都不用改。既有的包不必轉檔，別人分享的
//! 包照樣打得開。**檔案是程式寫的，不是人寫的**——Tab、UTF-8、
//! `zh` 標籤這三個坑一次全消失。

use eframe::egui;

// **欄位一律不放提示文字**（使用者定的）——欄位窄的時候提示字會被
// 切成「打一個」「按鍵（」，看起來像壞掉，而表頭與標籤已經說了那一欄
// 是什麼。唯一的例外是資料夾那格，它的提示是**預設路徑**（有內容，
// 不是說明）。

/// 一列詞。**這是編輯器的工作狀態**，不是檔案格式——
/// 存檔時才轉回「語言 TAB 輸入 TAB 輸出」。
#[derive(Default, Clone, PartialEq)]
pub struct Row {
    /// 使用者要的東西（檔案的第三欄）。**擺第一個**，那才是他心裡想的
    pub output: String,
    /// 按鍵。中文是 `cj6wl6`、日文是 `hororaibu`、英文就是原字串
    pub keys: String,
    /// 這一列是哪一種。**算出來的，不是選的**
    pub lang: Kind,
}

/// 一列是哪一種。**對使用者只有名字，沒有代號**。
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// 還沒填按鍵，或按鍵切不出來
    #[default]
    Unknown,
    Chinese,
    Japanese,
    English,
    /// 符號。**`\名字\` 叫出一整排候選**——跟詞不同，一個名字對到
    /// 好幾個輸出。2026-09-11 起可以編（原本只看得到）
    Symbol,
    /// 台語，這一輪也不給編
    Taiwanese,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Unknown => "？",
            Kind::Chinese => "中文",
            Kind::Japanese => "日文",
            Kind::English => "英文",
            Kind::Symbol => "符號",
            Kind::Taiwanese => "台語",
        }
    }

    /// 這一輪的編輯器管不管得到它。
    ///
    /// **台語還不行**：它的鍵是華語國字、跟其他層形狀不同，而且自動
    /// 辨識分不出「這是中文詞還是台語對照」（§2.75.6）。
    fn editable(self) -> bool {
        !matches!(self, Kind::Taiwanese)
    }

    /// 它的「怎麼打」是**按鍵**嗎？
    ///
    /// 符號不是——那一欄放的是名字（`星`），而名字常常得用輸入法打。
    /// 分開問是因為兩件事跟著它走：要不要攔實體按鍵（`keys_field`），
    /// 以及改完要不要重算語言（符號沒有語言可算）。
    fn keys_are_keys(self) -> bool {
        !matches!(self, Kind::Symbol)
    }
}

/// 檔頭那幾行。
#[derive(Default, Clone, PartialEq)]
pub struct Meta {
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub license: String,
}

/// 編輯器的全部狀態。
#[derive(Default)]
pub struct State {
    /// 現在開著哪個包（檔名，不含 `.txt`）。空的代表還沒選
    pub file: String,
    /// 在編輯狀態嗎？預設 false（預覽）
    pub editing: bool,
    /// 這是新的、還沒存過的包嗎
    pub is_new: bool,
    /// **這個包整包唯讀**（檔頭寫了 `# readonly: true`）。
    ///
    /// 內建包與官方語言包（台語那類）走這條——它們隨程式一起發布，
    /// 使用者改了下次更新就被覆蓋，而批量刪除更是一按就整包沒了
    /// （實測回報）。UI 上直接**不顯示編輯按鈕**。
    pub readonly: bool,
    pub meta: Meta,
    pub rows: Vec<Row>,
    /// 「新增一條」的輸出欄
    pub draft: String,
    /// 「新增一條」的按鍵欄。
    ///
    /// **跟著 `draft` 自動填，但使用者動過就不再覆寫**——不然他改到
    /// 一半又被自動填蓋掉。`draft_touched` 記的就是「動過了沒」。
    pub draft_keys: String,
    /// 使用者自己改過按鍵欄了嗎？
    pub draft_touched: bool,
    /// 「新增一條」現在要加的是**符號**嗎？
    ///
    /// **這是唯一需要使用者明講的類別**。詞的語言算得出來（`detect`
    /// 走引擎那條路），但符號算不出來——名字 `星` 跟注音按鍵、
    /// `sekibun` 跟英文詞，字面上分不開。**包的類別仍然是推導的**
    /// （`kind_tag`），這裡選的只是「這一列是什麼」。
    pub draft_sym: bool,
    /// 上一幀的輸出欄內容。**用來判斷「內容真的變了」**——
    /// `Response::changed()` 在組字送出那一幀不一定為真
    pub last_draft: String,
    /// 使用者剛才在輸出欄打字時的**原始按鍵串**（從 `Ime(Preedit)` 撈的）。
    ///
    /// **詞庫查不到的詞就靠它**：「英雄聯盟」反查不出按鍵，但使用者
    /// 剛剛就是用注音打出來的，那串按鍵直接拿來用最準。
    pub typed_keys: String,
    /// **輸入法正在組字嗎？**
    ///
    /// 不能只看「這一幀有沒有 `Preedit` 事件」——使用者按 Enter 選字
    /// 的那一幀不一定有 Preedit，就會被誤判成「要加入這一條」，
    /// 於是那個 Enter 被設定頁吃掉，**選字選不了**（實測回報）。
    ///
    /// 所以要記著狀態：`Preedit` 非空時進入組字，`Commit` 或空的
    /// `Preedit` 時離開。組字中的 Enter 一律不算數。
    pub composing: bool,
    /// 每一列勾了沒（批量刪除用）。**長度要跟 `rows` 一致**
    pub checked: Vec<bool>,
    /// 存檔時把版本的最後一段數字加一。
    /// **預設關著**——它會覆寫使用者手填的版本，見 §2.75.10
    pub bump_version: bool,
    /// 正在編輯哪一列的按鍵欄（`None` 代表沒有）。
    /// **編輯中那格顯示按鍵，其餘顯示注音／假名**
    pub editing_keys: Option<usize>,
    /// 正在編輯哪一列的輸出欄。**跟 `editing_keys` 互斥**——
    /// 兩欄同時開著的話 Tab 的跳法會亂掉
    pub editing_out: Option<usize>,
    /// 存檔結果的提示
    pub status: Option<(String, bool)>,
    /// 可選的包清單快取
    pub available: Option<Vec<String>>,
    /// 「刪除擴充包」按到第二段了嗎。**刪檔案不可逆**，要問一次
    pub confirm_delete: bool,
    /// 這個包有幾條日文的鍵**存成羅馬字**（舊版編輯器存壞的）。
    ///
    /// 載入層拿假名當鍵，存成 `ja asee game` 的話整條查不到（§2.75.13
    /// 第一件）。存檔那條已經修好，但**使用者手上已經存錯的包要重存
    /// 一次**才會修正——不主動說的話他只會覺得「設定頁看起來明明是對
    /// 的，怎麼打不出來」。
    pub legacy_ja: usize,
}

/// 畫整個分頁。**清單與編輯器合在一起**（使用者定的）：上面選一個包，
/// 下面就是它的內容。
///
/// 勾選框管「這個包啟不啟用」，點那一列管「現在看哪一個」——
/// **兩件事分開**，不會互相干擾。
pub fn page(
    ui: &mut egui::Ui,
    state: &mut State,
    cfg: &mut ime_core::config::Config,
    cache: &mut Option<Vec<ime_core::pack::Info>>,
) {
    // **組字狀態要在畫任何文字框之前就掃好**。
    //
    // `state.composing` 原本只在 `draft_row` 裡更新，那是「新增一條」
    // 那一列的事；但鎖鍵的 `EventFilter` 三個欄位都要用，而列的欄位
    // 畫在 `draft_row` 之前——用舊值的話**編輯既有列時鎖不住**。
    //
    // 掃的是這一幀的事件（`TextEdit` 還沒吃掉它們），判準跟
    // `main::scan_ime` 一致：`Preedit` 非空才算組字。
    ui.input(|i| {
        for e in &i.events {
            match e {
                egui::Event::Ime(egui::ImeEvent::Preedit(s)) => state.composing = !s.is_empty(),
                egui::Event::Ime(egui::ImeEvent::Commit(_))
                | egui::Event::Ime(egui::ImeEvent::Disabled) => state.composing = false,
                _ => {}
            }
        }
    });

    ui.add_space(8.0);
    ui.heading("擴充包");
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(
            r"擴充包是你自己加的詞表——遊戲名、專有名詞、常打的英文詞。加進來之後不只選字會有，連「這串按鍵是不是英文」的判斷都會跟著改。也可以加符號：打「\名字\」就叫得出你自己那一排。",
        )
        .weak(),
    );
    ui.add_space(12.0);

    folder_row(ui, state, cfg, cache);
    ui.add_space(12.0);

    let current = (!state.is_new && !state.file.is_empty()).then(|| state.file.clone());
    if let Some(f) = crate::pack_list::show(ui, cfg, cache, current.as_deref()) {
        load(state, &cfg.behavior.packs_dir, &f);
    }

    ui.add_space(16.0);
    ui.separator();
    ui.add_space(10.0);

    // **管整個包的三個操作放這裡**（使用者定的）：新增、編輯、刪除。
    // 它們管的是「這個包」，所以排在包的基本資料上面；下面清單旁邊
    // 那排（儲存／取消／刪除勾選的）管的是「這些詞」。
    actions_row(ui, state, cfg, cache);

    // 選中了才顯示下半部
    if state.file.is_empty() && !state.is_new {
        ui.add_space(16.0);
        ui.label(egui::RichText::new("點清單裡的一列，下面就會顯示它的內容。").weak());
        return;
    }

    // **不再重複顯示包名**——清單上已經反白標出來了，
    // 而基本資料的「名稱」欄就是它的名字（使用者定的）
    if let Some((msg, ok)) = &state.status {
        ui.add_space(4.0);
        let c = if *ok {
            egui::Color32::from_rgb(0x2E, 0x7D, 0x32)
        } else {
            egui::Color32::from_rgb(0xC6, 0x28, 0x28)
        };
        ui.colored_label(c, msg);
    }

    legacy_ja_notice(ui, state, &cfg.behavior.packs_dir);

    ui.add_space(10.0);
    meta_section(ui, state);
    ui.add_space(14.0);
    rows_section(ui, state);
}

/// 舊版存壞的日文鍵：說清楚，並給一顆按鈕當場修掉。
///
/// **這不是使用者做錯事**，所以語氣是「幫你修」不是「你填錯」。
/// 修法就是原封不動重存一次——`to_entry` 現在會把按鍵轉回假名，
/// 而 `write_editable` 覆寫前會先備份成 `.bak`。
fn legacy_ja_notice(ui: &mut egui::Ui, state: &mut State, packs_dir: &str) {
    if state.legacy_ja == 0 {
        return;
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.colored_label(
            egui::Color32::from_rgb(0xE6, 0x7E, 0x22),
            format!(
                "這個包有 {} 條日文存的是羅馬字，打字時叫不出來（舊版存的）。",
                state.legacy_ja
            ),
        );
        // **唯讀的包不給修**——它隨程式一起發布，下次更新就被覆蓋，
        // 而 `save` 那邊本來也擋著
        if state.readonly {
            ui.label(egui::RichText::new("這個包是唯讀的，無法修改。").weak());
        } else if ui.button("修正").clicked() {
            save(state, packs_dir);
        }
    });
}

/// 資料夾那一行：路徑、瀏覽、開啟、重新整理、新增包。
fn folder_row(
    ui: &mut egui::Ui,
    state: &mut State,
    cfg: &mut ime_core::config::Config,
    cache: &mut Option<Vec<ime_core::pack::Info>>,
) {
    // 路徑做成可編輯的欄位：留空是預設位置，填了就用填的。
    // **不再有隱形的後備位置**——畫面上寫什麼就是什麼。
    let target = ime_core::pack::resolved_dir(&cfg.behavior.packs_dir);
    let exists = target.as_ref().is_some_and(|p| p.is_dir());
    let 預設 = ime_core::pack::resolved_dir("")
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    // **全部擠成一行**（使用者定的）：按鈕靠右排，路徑欄吃掉剩下的。
    //
    // 原本路徑欄的寬度是「可用寬度減 200」，那個 200 是猜的——按鈕
    // 實際只要 60 多，右邊就空一大片（使用者實測回報）。用
    // `right_to_left` 讓 egui 自己量按鈕，不必猜。
    //
    // **按鈕由右往左擺**，所以程式碼的順序跟畫面上是反的：
    // 這裡先寫「重新整理」，它會出現在最右邊。
    ui.horizontal(|ui| {
        ui.label("資料夾：");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("重新整理").clicked() {
                *cache = None;
                state.available = None;
            }
            if !cfg.behavior.packs_dir.trim().is_empty() && ui.button("用預設位置").clicked() {
                cfg.behavior.packs_dir.clear();
                *cache = None;
                state.available = None;
            }
            match &target {
                Some(p) if exists => {
                    if ui.button("開啟資料夾").clicked() {
                        crate::reveal_folder(p);
                    }
                }
                Some(p) => {
                    let p = p.clone();
                    if ui.button("建立").clicked() && std::fs::create_dir_all(&p).is_ok() {
                        *cache = None;
                    }
                    ui.colored_label(
                        egui::Color32::from_rgb(0xC6, 0x28, 0x28),
                        "這個資料夾還不存在",
                    );
                }
                None => {
                    ui.colored_label(
                        egui::Color32::from_rgb(0xC6, 0x28, 0x28),
                        "找不到可用的位置（使用者資料夾讀不到？）",
                    );
                }
            }
            if ui.button("瀏覽…").clicked() {
                if let Some(p) = crate::pick_folder(target.as_deref()) {
                    cfg.behavior.packs_dir = p;
                    *cache = None;
                    state.available = None;
                }
            }
            // 路徑欄最後畫，吃掉剩下的全部
            let r = ui.add_sized(
                [ui.available_width(), 22.0],
                egui::TextEdit::singleline(&mut cfg.behavior.packs_dir).hint_text(&預設),
            );
            if r.changed() {
                *cache = None;
                state.available = None;
            }
        });
    });
}

/// 基本資料（對應檔頭那幾行 `# name:`）。
///
/// **三列排法**（使用者定的）：名稱＋作者、說明整列、授權＋版本。
/// 說明最長，自己一列才不會被擠掉。
fn meta_section(ui: &mut egui::Ui, state: &mut State) {
    ui.label(egui::RichText::new("基本資料").strong());
    ui.add_space(6.0);

    let on = state.editing;
    // **不放提示文字**（使用者定的）——旁邊的標籤已經說了那一欄是
    // 什麼，提示字只是在窄欄位裡被切成半截
    let field = |ui: &mut egui::Ui, label: &str, v: &mut String, w: f32| {
        ui.label(label);
        ui.add_enabled(on, egui::TextEdit::singleline(v).desired_width(w));
    };

    // **名稱就是檔名**（使用者定的）：新增一個包時它決定檔案叫什麼，
    // 內部登記的名字跟檔名相同。
    ui.horizontal(|ui| {
        field(ui, "名稱", &mut state.meta.name, 220.0);
        ui.add_space(12.0);
        field(ui, "作者", &mut state.meta.author, 180.0);
    });
    // 名稱直接當檔名，所以要擋掉檔名不能用的字元。
    // **只有新的包才提醒**——既有的包名字已經是合法檔名了
    if on && state.is_new {
        let bad = bad_filename_chars(&state.meta.name);
        if !bad.is_empty() {
            ui.add_space(2.0);
            ui.colored_label(
                egui::Color32::from_rgb(0xC6, 0x28, 0x28),
                format!("名稱不可包含下列字元：{bad}"),
            );
        }
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        // 說明**吃掉整列**——它最長，固定寬度的話視窗拉寬也還是那麼短
        let w = (ui.available_width() - 60.0).max(200.0);
        field(ui, "說明", &mut state.meta.description, w);
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        field(ui, "授權", &mut state.meta.license, 180.0);
        ui.add_space(12.0);
        // **整數版號**（使用者定的）：`1`、`2`、`3`，不用小數點。
        // 小數點會讓人猶豫「改一個字算 1.1 還是 1.0.1」，整數沒這個問題
        field(ui, "版本", &mut state.meta.version, 70.0);
        // **自動進版做成可關的開關**：它會覆寫使用者手填的版本，
        // 而「存一次就是一版」不一定是每個人要的語意。見 §2.75.10
        ui.add_space(8.0);
        ui.add_enabled(
            on,
            egui::Checkbox::new(&mut state.bump_version, "存檔時自動 +1"),
        )
        .on_hover_text("每次儲存把版本加一：1 → 2 → 3");
    });
}

/// 詞的清單。
///
/// **編輯時最前面多一欄勾選**（批量刪除），**新增列就在清單第一列**
/// ——加完立刻看得到它排在哪，不必捲到最下面去找。
fn rows_section(ui: &mut egui::Ui, state: &mut State) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("內容").strong());
        ui.label(egui::RichText::new(format!("（{} 條）", state.rows.len())).weak());

        // **新增列要加的是哪一種**——擺在表格外面，不進格子。
        //
        // 這是唯一需要使用者明講的類別：詞的語言算得出來（`detect`），
        // 符號算不出來——名字 `星` 跟注音按鍵字面上分不開。**包的類別
        // 仍然是推導的**（清單上那個〔詞〕〔符號〕〔詞＋符號〕）。
        if state.editing {
            ui.add_space(12.0);
            ui.label(egui::RichText::new("新增：").weak());
            if ui
                .selectable_label(!state.draft_sym, "詞")
                .on_hover_text("打一串按鍵出一個詞")
                .clicked()
            {
                state.draft_sym = false;
            }
            if ui
                .selectable_label(state.draft_sym, "符號")
                .on_hover_text(r"打 \名字\ 叫出一整排符號")
                .clicked()
            {
                state.draft_sym = true;
            }
        }

        // 勾了才出現，平常不佔位置
        let picked = state.checked.iter().filter(|c| **c).count();
        if state.editing && picked > 0 {
            ui.add_space(12.0);
            if ui.button(format!("刪除勾選的 {picked} 條")).clicked() {
                let mut i = 0;
                state.rows.retain(|_| {
                    let keep = !state.checked.get(i).copied().unwrap_or(false);
                    i += 1;
                    keep
                });
                state.checked.retain(|c| !*c);
                state.editing_keys = None;
            }
        }
    });
    ui.add_space(6.0);

    // 勾選狀態跟列數對齊——載入新的包、加減列之後都要補
    state.checked.resize(state.rows.len(), false);

    // **欄寬依實際內容分配，以不吃掉資料為原則**（使用者定的）。
    //
    // 平分的話短的那欄浪費、長的那欄被截斷——`ホロライブ` 跟
    // `hololive` 差很多，而「怎麼打」放注音時又比輸出長。
    // 所以先量兩欄各自最長的內容，照比例分，放不下才等比縮。
    //
    // 上限只設在「窄到不能再窄」那一端——設了上限的話視窗拉寬之後
    // 這張表撐不滿，右邊留一大片空白，跟上面的擴充包清單看起來像
    // 兩套東西（使用者實測回報）。兩張表都靠最後一格 `fill` 撐滿。
    // 捲軸佔掉的寬度要扣兩次：**內容清單自己那支**（表格內容變窄），
    // 以及**整個捲動區往左退**（讓出位置給整頁那支，見下面）
    let bar = ui.spacing().scroll.bar_width + ui.spacing().scroll.bar_inner_margin;
    // ★ **那兩個數字是估的**：語言欄按「兩個字」、刪除欄按一顆「刪除」鈕。★
    //
    // **往那兩欄加任何東西之前先改這裡**。估太小的話 `avail` 高估，
    // 兩欄文字分到太多寬度，整張表撐出去，**內層捲軸就跟整頁那支疊在
    // 一起**（§2.75.12 記過一次；2026-09-11 加符號編輯時，語言欄寫成
    // 「符號 3」、刪除欄塞兩顆切換鈕，同一個坑再犯一次）。
    //
    // 現在的規矩是**那兩欄只放固定大小的東西**：語言欄一律兩個字
    // （中文／日文／英文／符號／台語），變動的資訊走 hover 或表格
    // 下面那一行。
    let fixed = if state.editing { 30.0 } else { 0.0 } + 60.0 + 90.0 + 6.0 * 14.0 + bar * 2.0;
    let avail = (ui.available_width() - fixed).max(280.0);
    let (w_out, w_keys) = split_widths(ui, state, avail);

    // 文字不給選取——理由同 `pack_list`：拖曳選字會吃掉點擊，
    // 「點一下改」就沒反應了
    ui.style_mut().interaction.selectable_labels = false;

    // **固定 20 列高，超過就捲、不足就空著**（使用者定的）。
    //
    // 高度固定的好處是畫面不會跟著資料量跳動——3 條的包跟 9 萬條的
    // 包，下面那排提示都在同一個位置。
    //
    // **這是整頁唯一自己捲的地方**：包清單交給外層那個大捲軸（它不會
    // 有幾百個），這一張非自己捲不可（台語包 9 萬條）。兩支捲軸疊在
    // 一起過一次，分工要說清楚。
    let rows_h = 20.0 * 26.0;

    // 最後多一欄專門撐滿，跟上面的擴充包清單同一套做法
    let cols = if state.editing { 6 } else { 5 };
    let mut remove = None;

    // **整個捲動區往左退一個捲軸的寬度**。
    //
    // 不退的話它撐滿到最右邊，自己的捲軸就跟整頁那支疊在一起
    // （使用者實測回報）。扣掉表格內容的寬度不夠——那只讓文字變窄，
    // 捲動區本身還是貼著右緣。
    //
    // **表頭也要用同一個寬度**：它在捲動區外面（見下面），`fill` 拿到的
    // 是沒退讓過的 `available_width`，於是比資料列多凸出整整一個捲軸寬
    // ——量出來是「表頭右緣 892、捲動區右緣 878」，那 14px 就疊在整頁
    // 那支捲軸上（2026-09-11 實測，§2.75.12 那條的第二種面貌）。
    let inner_w = (ui.available_width() - bar).max(200.0);

    // ── 表頭與新增列：**留在捲動區外面** ──
    //
    // 兩個好處：捲到第一千條時表頭還在，而且新增一條之後不必捲回
    // 最上面去看它。
    let _頭 = ui.scope(|ui| {
        // ★ 跟下面的捲動區同寬，理由見 `inner_w` ★
        ui.set_max_width(inner_w);
        egui::Grid::new("pack_rows_head")
            .num_columns(cols)
            .spacing([14.0, 6.0])
            .show(ui, |ui| {
                if state.editing {
                    // 全選／全不選
                    let all = !state.checked.is_empty() && state.checked.iter().all(|c| *c);
                    let mut toggle = all;
                    if ui.checkbox(&mut toggle, "").changed() {
                        state.checked.iter_mut().for_each(|c| *c = toggle);
                    }
                }
                // **表頭要撐到跟資料列一樣寬**。
                //
                // Grid 的一欄有多寬，是由**這一欄最寬的那一格**決定的，
                // 而 `TextEdit` 的 `desired_width` 會被它所在那一格的可用
                // 寬度壓縮。表頭只放「輸出」兩個字的話這一欄就只有那麼寬，
                // 下面的輸入框跟著被切成「打一個詞・E」（使用者實測回報）。
                cell(ui, egui::RichText::new("輸出").weak(), w_out, false);
                cell(ui, egui::RichText::new("怎麼打").weak(), w_keys, false);
                ui.label(egui::RichText::new("語言").weak());
                ui.label("");
                crate::fill(ui);
                ui.end_row();

                // **新增列排在第一列**，只有編輯時才出現
                if state.editing {
                    draft_row(ui, state, w_out, w_keys);
                    crate::fill(ui);
                    ui.end_row();
                }
            });
    });

    // 底線開頭是因為 release 版真的沒人用它——記錄點整段 `cfg` 掉了
    let _頭寬 = _頭.response.rect.right();

    // ── 資料列：**只畫看得見的那幾列** ──
    //
    // 台語包有 9 萬條。整份都建 widget 的話 egui 每一幀要做 9 萬次
    // 排版與命中測試，設定頁整個卡死（使用者實測回報）。
    // `show_rows` 只建捲到的那一段，其餘用空白撐出捲軸的長度。
    let row_h = 26.0;
    let _身體 = egui::ScrollArea::vertical()
        .id_salt("pack_rows_scroll")
        .min_scrolled_height(rows_h)
        .max_height(rows_h)
        .max_width(inner_w)
        .auto_shrink([false, false])
        .show_rows(ui, row_h, state.rows.len(), |ui, range| {
            egui::Grid::new("pack_rows")
                .num_columns(cols)
                .striped(true)
                .spacing([14.0, 6.0])
                // **起始列的奇偶要跟真正的列號一致**，不然捲動時
                // 隔行底色會跳來跳去
                .start_row(range.start)
                .show(ui, |ui| {
                    for i in range {
                        row_widgets(ui, state, i, &mut remove, w_out, w_keys);
                        crate::fill(ui);
                        ui.end_row();
                    }
                });
        });

    // **表頭與資料列的右緣要對齊**，見 `inner_w` 上面那段。
    // 只有測試會看（release 整段編不進去）
    #[cfg(test)]
    記下排版(_頭寬, _身體.inner_rect.right());

    if let Some(i) = remove {
        state.rows.remove(i);
        state.checked.remove(i);
        state.editing_keys = None;
    }

    // **符號的拆解預覽**：打的當下就看得到會拆成幾個。
    //
    // `★☆✦` 是三個還是一個、`👨+👩+👧` 是三個人還是一個圖——肉眼分不
    // 出來，而那正是符號最容易寫錯的地方。放在表格**下面**而不是格子
    // 裡：那一欄的寬度是估出來的，寫長會把整張表撐寬（見 `draft_row`）。
    if state.editing && state.draft_sym && !state.draft.trim().is_empty() {
        ui.add_space(6.0);
        ui.label(egui::RichText::new(sym_preview(&state.draft)).weak());
    }

    // 有問題的列列出來——**這些是無效資料，存進去引擎讀不出東西**
    if state.editing {
        let bad = invalid_rows(state);
        if !bad.is_empty() {
            ui.add_space(8.0);
            ui.colored_label(
                egui::Color32::from_rgb(0xC6, 0x28, 0x28),
                "有幾條有問題，修好才能儲存：",
            );
            for (i, why) in bad.iter().take(5) {
                ui.label(
                    egui::RichText::new(format!(
                        "· 第 {} 列「{}」：{why}",
                        i + 1,
                        state.rows[*i].output
                    ))
                    .weak()
                    .small(),
                );
            }
            if bad.len() > 5 {
                ui.label(
                    egui::RichText::new(format!("· 還有 {} 條", bad.len() - 5))
                        .weak()
                        .small(),
                );
            }
        }
    }
}

/// 哪幾列存不得？回傳（第幾列, 為什麼）。
///
/// **辨識不出語言的一律擋下來**（使用者定的）——那種列存進檔案，
/// `pack::parse` 讀到的是一個認不得的語言代號，整列被默默丟掉。
/// 與其存了沒作用，不如當場說清楚。
fn invalid_rows(state: &State) -> Vec<(usize, &'static str)> {
    // 畫面上只列前 5 條，找到夠多就可以停——**每一幀都會跑一次**，
    // 台語包 9 萬條的話掃完整份是白費力氣
    const ENOUGH: usize = 6;
    let mut out = Vec::new();
    for (i, r) in state.rows.iter().enumerate() {
        if out.len() >= ENOUGH {
            break;
        }
        // 不給編的那種（台語）原樣保留，不檢查
        if !r.lang.editable() {
            continue;
        }
        // **符號的規矩跟詞不一樣**：第二欄是名字不是按鍵，第三欄是
        // 一整排符號不是一個輸出
        if r.lang == Kind::Symbol {
            if r.output.trim().is_empty() {
                out.push((i, "沒有符號"));
            } else if r.keys.trim().is_empty() {
                out.push((i, "還沒填名字"));
            } else if r.keys.contains('\\') {
                // `merge_symbols` 是靠一對 `\` 找名字的，名字裡再有一個
                // 就永遠配不起來——存得進去但叫不出來
                out.push((i, "名字裡不能有反斜線"));
            } else if ime_core::pack::split_symbols(&r.output).is_empty() {
                out.push((i, "這一格拆不出任何符號"));
            }
            continue;
        }
        if r.output.trim().is_empty() {
            out.push((i, "輸出是空的"));
        } else if r.keys.trim().is_empty() {
            out.push((i, "還沒填按鍵"));
        } else if r.lang == Kind::Unknown {
            out.push((i, "這串按鍵辨識不出語言，打不出這個詞"));
        } else if r.lang == Kind::Japanese && ime_core::romaji::kana::to_kana(&r.keys).is_none() {
            // **日文存進檔案的是假名**，轉不出來的話存得進去、載入時
            // 靜靜消失——使用者只會看到「明明加了卻沒作用」（§2.75.13）
            out.push((i, "這串按鍵拼不出假名"));
        }
    }
    out
}

/// 畫一列。
/// **組字中把移動焦點的鍵全部留給輸入法**，沒在組字就恢復正常。
///
/// 鎖住的是 **Tab ＋ 四個方向鍵**——組字時它們都是輸入法的：
/// Tab 開段選單、方向鍵選段與翻候選。
///
/// # 為什麼要用 `EventFilter`，不能刪事件
///
/// egui 的焦點是在 `begin_pass` 決定的——Tab 與方向鍵在那裡被轉成
/// `FocusDirection`，而那**早於 `update()`**。所以在 `update()` 裡把
/// 這些鍵從事件裡刪掉**阻止不了焦點跑掉**（實測回報：刪了還是會跳）。
///
/// `EventFilter` 是 egui 給 widget 宣告「這個鍵我自己要」的正式管道，
/// `begin_pass` 的焦點處理會先問它（`event_filter.matches(event)`）。
///
/// # 為什麼四個方向鍵都要鎖
///
/// 實測回報（2026-09-14）：只鎖 Tab 的話，**按 Tab 開了段選單之後按
/// 方向鍵會直接跳離組字**——那一下被 egui 拿去移焦點，文字框失焦，
/// 組字跟著中止。段選單的左右是選段、上下是翻這一段的候選，**四個
/// 都是輸入法的**，少鎖一個就從那個方向漏掉。
///
/// # 為什麼跟著組字狀態切換
///
/// 使用者裁決（2026-09-14）：**跳欄要留著，只是組字中不能跳**。
/// 設定頁本來就是拿來打字試效果的地方；但沒在組字時 Tab 與方向鍵
/// 就該是一般的介面操作，那是所有表單都有的行為。
///
/// 每一幀都要設——`EventFilter` 記在 widget 的焦點狀態裡，組字狀態
/// 一變就要跟著換。
fn lock_ime_keys_while_composing(ui: &egui::Ui, id: egui::Id, composing: bool) {
    ui.ctx().memory_mut(|m| {
        m.set_focus_lock_filter(
            id,
            egui::EventFilter {
                // 組字中這幾個鍵都是輸入法的，不要讓焦點跑掉
                tab: composing,
                horizontal_arrows: composing,
                vertical_arrows: composing,
                // Esc **不鎖**：那是 shield_ime_keys 管的（它會在組字中
                // 把 Esc 從事件裡拿掉）。兩邊都管的話沒在組字時 Esc
                // 反而關不掉文字框的焦點。
                escape: false,
            },
        );
    });
}

fn row_widgets(
    ui: &mut egui::Ui,
    state: &mut State,
    i: usize,
    remove: &mut Option<usize>,
    w_out: f32,
    w_keys: f32,
) {
    let editing = state.editing;
    let kind = state.rows[i].lang;
    let locked = !kind.editable();

    // 勾選（批量刪除用）。不給編的那幾種也不給勾
    if editing {
        if locked {
            ui.label("");
        } else if let Some(c) = state.checked.get_mut(i) {
            ui.checkbox(c, "");
        } else {
            ui.label("");
        }
    }

    // 輸出欄。**跟「怎麼打」同一種行為**（使用者定的）：平常是文字，
    // 點了才變成文字框。兩欄的操作一致，不必記哪一欄能點哪一欄不能
    if editing && !locked && state.editing_out == Some(i) {
        let id = egui::Id::new(("row_out", i));
        let r = ui.add(
            egui::TextEdit::singleline(&mut state.rows[i].output)
                .id(id)
                .desired_width(w_out),
        );
        // 組字中的 Tab 與方向鍵都是輸入法的（段選單），不是移焦點
        lock_ime_keys_while_composing(ui, id, state.composing);
        if r.lost_focus() {
            state.editing_out = None;
        }
    } else {
        let text = if state.rows[i].output.trim().is_empty() {
            egui::RichText::new("（空的）")
                .color(egui::Color32::from_rgb(0xC6, 0x28, 0x28))
                .italics()
        } else {
            egui::RichText::new(&state.rows[i].output)
        };
        let r = cell(ui, text, w_out, editing && !locked);
        if editing && !locked && r.clicked() {
            state.editing_out = Some(i);
            state.editing_keys = None;
            // 點下去就直接可以打，不必再點一次
            ui.ctx()
                .memory_mut(|m| m.request_focus(egui::Id::new(("row_out", i))));
        }
        if editing && !locked {
            r.on_hover_text("點選以修改");
        }
    }

    // 「怎麼打」欄：**編輯中那格顯示按鍵，其餘顯示注音／假名**
    let is_focus = state.editing_keys == Some(i);
    if editing && !locked && is_focus {
        let r = if kind.keys_are_keys() {
            // 攔實體按鍵自己組，繞開輸入法——理由見 `keys_field`
            keys_field(
                ui,
                &mut state.rows[i].keys,
                egui::Id::new(("row_keys", i)),
                w_keys,
            )
        } else {
            // **符號那欄放的是名字**，而名字常常是中文（`星`）——
            // 要讓輸入法組字，攔實體按鍵的話根本打不出來
            let id = egui::Id::new(("row_keys", i));
            let r = ui.add(
                egui::TextEdit::singleline(&mut state.rows[i].keys)
                    .id(id)
                    .desired_width(w_keys),
            );
            // 這一欄會組字，所以也要把組字中的那幾個鍵留給輸入法
            lock_ime_keys_while_composing(ui, id, state.composing);
            r
        };
        if r.lost_focus() {
            state.editing_keys = None;
        }
        // 打的當下就重算語言，看得到辨識結果在變。
        // **符號沒有語言可算**——拿名字去 `detect` 只會判成別的東西
        if kind.keys_are_keys() {
            state.rows[i].lang = detect(&state.rows[i].keys);
        }
    } else {
        let shown = pretty(&state.rows[i].keys, kind);
        let text = if shown.is_empty() {
            egui::RichText::new("（還沒填）")
                .color(egui::Color32::from_rgb(0xC6, 0x28, 0x28))
                .italics()
        } else {
            egui::RichText::new(shown)
        };
        let r = cell(ui, text, w_keys, editing && !locked);
        if editing && !locked && r.clicked() {
            state.editing_keys = Some(i);
            state.editing_out = None;
            // 點下去就直接可以打，不必再點一次
            ui.ctx()
                .memory_mut(|m| m.request_focus(egui::Id::new(("row_keys", i))));
        }
        if editing && !locked {
            r.on_hover_text("點選以修改按鍵");
        }
    }

    // 語言欄：算出來的，不能改
    let c = if kind == Kind::Unknown {
        egui::Color32::from_rgb(0xC6, 0x28, 0x28)
    } else {
        ui.visuals().weak_text_color()
    };
    let lbl = ui.colored_label(c, kind.label());
    // **符號要看得到拆成幾個**：`★☆✦` 是三個還是一個，肉眼分不出來，
    // 而拆法的規則只有 `pack::split_symbols` 知道（見那支的說明）
    if kind == Kind::Symbol {
        lbl.on_hover_text(sym_preview(&state.rows[i].output));
    }

    // 刪。**靠右對齊**（使用者定的）——按鈕貼著表格右緣排成一直線，
    // 比跟在語言欄後面浮著整齊
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if editing && !locked {
            // **用中文字不用符號**：`✕`（U+2715）微軟正黑體沒有這個字，
            // 顯示成空方框（使用者實測回報）。字型缺字是防不勝防的事，
            // 介面上一律用一定有的字
            if ui.small_button("刪除").clicked() {
                *remove = Some(i);
            }
        } else if locked {
            // 字要短——這一欄的寬度由最長的內容決定，寫長句會把
            // 上面的刪除鈕推得離右緣很遠
            ui.label(egui::RichText::new("唯讀").weak().small().italics())
                .on_hover_text("台語包目前不支援在此編輯，請以文字編輯器修改");
        }
    });
}

/// 「新增一條」：**就是清單的第一列**，不另外開一區。
///
/// 打一個詞 → 自動填按鍵與語言 → Enter 送出 → 游標留著等下一個。
/// 欄位跟下面的列對齊，所以看得出加進去會長什麼樣。
fn draft_row(ui: &mut egui::Ui, state: &mut State, w_out: f32, w_keys: f32) {
    // **Enter 與組字狀態都要在文字框畫出來之前先看**。
    //
    // `TextEdit` 會**吃掉**這些事件（單行框收到 Enter 就結束編輯，
    // `Ime` 事件也一樣被消耗），等它畫完再問 `ui.input` 就沒了——
    // 那正是「Enter 送不出去」與「選字時被當成加入」的共同原因
    // （使用者實測回報）。
    let mut enter_pressed = false;
    let mut just_committed = false;
    ui.input(|i| {
        for e in &i.events {
            match e {
                egui::Event::Key {
                    key: egui::Key::Enter,
                    pressed: true,
                    ..
                } => enter_pressed = true,
                // 組字中：`Preedit` 非空代表輸入法正在給候選
                egui::Event::Ime(egui::ImeEvent::Preedit(s)) => {
                    state.composing = !s.is_empty();
                    // **順手把原始按鍵記下來**：那是使用者剛剛打的注音，
                    // 反查失敗時（詞庫連逐字都拼不出來）拿它當退路
                    if !s.is_empty() {
                        state.typed_keys = s.clone();
                    }
                }
                // 組字結束：送出了，或被取消
                egui::Event::Ime(egui::ImeEvent::Commit(_))
                | egui::Event::Ime(egui::ImeEvent::Disabled) => {
                    state.composing = false;
                    just_committed = true;
                }
                _ => {}
            }
        }
    });

    // 勾選欄空著（還不是一列，沒得勾）
    ui.label("");

    let draft_id = egui::Id::new("draft_out");
    let r = ui.add(
        egui::TextEdit::singleline(&mut state.draft)
            .id(draft_id)
            .desired_width(w_out),
    );
    // 「新增一條」的輸出欄是最常打字的地方，組字中的那幾個鍵留給輸入法
    lock_ime_keys_while_composing(ui, draft_id, state.composing);

    let word = state.draft.trim().to_string();
    // 輸出欄變了就重新自動填按鍵——**除非使用者已經自己動過**。
    //
    // 判準是**內容真的變了**，不是 `r.changed()`：組字送出那一幀
    // 不一定算 `changed`，只看它的話「打完字按鍵欄還是空的」
    // （使用者實測回報）。
    if state.draft != state.last_draft {
        state.last_draft = state.draft.clone();
        // **符號不自動填**：那一欄放的是名字，反查是「詞 → 按鍵」，
        // 拿符號串去查只會得到一堆亂七八糟的東西
        if !state.draft_touched && !state.draft_sym {
            state.draft_keys = if word.is_empty() {
                String::new()
            } else {
                // **先查詞庫，查不到就用剛才打字的按鍵**
                let looked_up = guess_keys(&word);
                if looked_up.is_empty() {
                    std::mem::take(&mut state.typed_keys)
                } else {
                    looked_up
                }
            };
        }
    }
    // 輸出欄清空時把「動過」一起重置，下一個詞才會恢復自動填
    if word.is_empty() && state.draft_keys.is_empty() {
        state.draft_touched = false;
        state.typed_keys.clear();
    }

    // 「怎麼打」欄：**是文字框，可以就地改**（使用者定的）
    let before = state.draft_keys.clone();
    let kr = if state.draft_sym {
        // 符號那欄是**名字**，多半要用輸入法打（`星`）——理由同
        // `row_widgets` 裡那一段
        ui.add(
            egui::TextEdit::singleline(&mut state.draft_keys)
                .id(egui::Id::new("draft_keys"))
                .desired_width(w_keys),
        )
    } else {
        keys_field(
            ui,
            &mut state.draft_keys,
            egui::Id::new("draft_keys"),
            w_keys,
        )
    };
    if state.draft_keys != before {
        state.draft_touched = true;
    }

    // **按鍵不能 trim**：注音的**一聲就是空白鍵**（`bopomofo::keymap`
    // 的 `TONES`），砍掉結尾的空白等於刪掉聲調——「今天」的
    // `rup wu0 ` 變成 `rup wu0`，引擎就把後半當成英文，語言辨識不出來
    // （使用者實測回報）。
    let keys = state.draft_keys.clone();
    // **符號的類別是使用者選的，不是算的**——見 `State::draft_sym`
    let lang = if state.draft_sym {
        Kind::Symbol
    } else {
        detect(&keys)
    };

    // 語言欄
    if state.draft_sym {
        if word.is_empty() {
            ui.label("");
        } else {
            // **這一格的字要短**（兩個字，跟「中文」「日文」一樣）——
            // 它的欄寬是估的，寫長會把整張表撐寬。拆成幾個改在表格
            // 下面那一行即時顯示，見 `rows_section`
            let n = ime_core::pack::split_symbols(&word).len();
            let c = if n == 0 {
                egui::Color32::from_rgb(0xC6, 0x28, 0x28)
            } else {
                ui.visuals().weak_text_color()
            };
            ui.colored_label(c, "符號")
                .on_hover_text(sym_preview(&word));
        }
    } else if word.is_empty() && keys.is_empty() {
        ui.label("");
    } else if keys.is_empty() {
        // **這一格是語言欄**，字要短——寫長句會把整欄撐開，
        // 下面每一列的「中文／日文」就跟著跑掉（使用者實測回報）
        ui.colored_label(egui::Color32::from_rgb(0xC6, 0x28, 0x28), "？")
            .on_hover_text("查不到讀音，請在「怎麼打」欄位手動填入按鍵");
    } else {
        // 打字當下就看得到辨識成什麼，以及按鍵長成人看得懂的樣子
        ui.label(egui::RichText::new(lang.label()).weak())
            .on_hover_text(pretty(&keys, lang));
    }

    // 送出。**Enter 是加入的出口**，但要避開輸入法。
    //
    // # 為什麼組字中的 Enter 不能算
    //
    // 使用者打詞的時候輸入法是開著的（他就是要用注音打「胡桃」），
    // 而通譯自己把 Enter 當送出。組字中按 Enter 是「選這個字」，
    // 不是「加入這一條」——搶走的話**選字就選不了**（實測回報）。
    //
    // # 為什麼不能只看這一幀有沒有 Preedit
    //
    // 按 Enter 選字的那一幀**不一定有 `Preedit` 事件**，只看單幀會
    // 誤判成「沒在組字」，那個 Enter 就被吃掉了。所以要**記著狀態**：
    // `Preedit` 非空時進入組字，`Commit`／`Disabled` 時離開
    // （見 `State::composing`）。
    // 實際的判斷在函式最上面就做好了（`TextEdit` 會吃掉 `Ime` 事件，
    // 畫完再問就沒了），這裡只是取用
    let composing = state.composing || just_committed;

    // **選完字要把游標留住**。
    //
    // `TextEdit` 的單行框收到 Enter 就會放棄焦點，而使用者選字按的
    // 正是 Enter——字送進來了，游標卻跑掉，得再點一次才能繼續打
    // （實測回報）。那個 Enter 的語意是「選這個字」，不是「我打完了」。
    if just_committed {
        if r.has_focus() || r.lost_focus() {
            r.request_focus();
        } else if kr.has_focus() || kr.lost_focus() {
            kr.request_focus();
        }
    }

    // **視窗自己在背景時一律不算**——最外層的閘門。egui 在沒有焦點
    // 時仍然可能收到事件，不擋的話別的視窗按 Enter 會動到這裡。
    //
    // **判準是「現在有焦點」不是 `lost_focus()`**：後者在切換到別的
    // 視窗時也會觸發，那一幀剛好有 Enter 就會誤判（實測回報）。
    let window_focused = ui.ctx().input(|i| i.viewport().focused.unwrap_or(true));
    // **兩個欄位按 Enter 都算**——填完按鍵直接送出，不必再點回去
    // **焦點要算「這一幀之前在不在」**：Enter 一按，`TextEdit` 就
    // 失去焦點了，這時 `has_focus()` 已經是 false——只看它的話永遠
    // 送不出去。`lost_focus()` 補的正是「剛剛還在」這一段。
    let mine = r.has_focus() || r.lost_focus() || kr.has_focus() || kr.lost_focus();
    let enter = window_focused && mine && !composing && enter_pressed;

    // 按鍵是空的就不給加——跟「空的不給存」同一條規則，
    // 與其加進去再變成一列紅字，不如當場擋下來。
    //
    // **沒有「加入」按鈕**（使用者定的）：那一欄只為了一顆按鈕存在，
    // 而下面每一列的同一欄是刪除鈕，兩者對不起來。Enter 就是出口。
    let ready = !word.is_empty()
        && !keys.is_empty()
        && (!state.draft_sym || !ime_core::pack::split_symbols(&word).is_empty());

    // **這一格要留空**。類別切換擺在表格上面那一行，不放這裡——
    // 這一欄的寬度是按「刪除」那顆鈕估的（`fixed` 裡的 90.0），塞進
    // 兩顆按鈕就會撐爆估計值，整張表跟著變寬，內層捲軸就跑到跟整頁
    // 那支疊在一起（§2.75.12 的老毛病，2026-09-11 用這個方式再犯一次）
    ui.label("");
    if enter && ready {
        // **同一組按鍵只能有一筆，新的蓋掉舊的**。
        //
        // 載入時是「先出現的贏」（`pack.rs` 的 `or_insert`），所以留兩筆
        // 同按鍵的話**後面那筆永遠不會生效**，卻靜靜躺在檔案裡——
        // 使用者以為改掉了，實際打出來還是舊的（實測回報：打同音的
        // 「擬郝」想覆蓋「你好」，結果兩筆都在）。
        //
        // 舊的先拿掉、新的排最上面，畫面就跟實際行為一致。
        // **同類別才算重複**：符號的名字跟某個英文詞的按鍵可能長得
        // 一模一樣（`star`），那是兩回事，不該互相蓋掉
        let is_sym = lang == Kind::Symbol;
        let dup = state
            .rows
            .iter()
            .position(|x| x.keys == keys && (x.lang == Kind::Symbol) == is_sym);
        if let Some(i) = dup {
            state.rows.remove(i);
            if i < state.checked.len() {
                state.checked.remove(i);
            }
            // 正在編輯的那一列如果排在被刪的後面，索引要往前挪
            state.editing_keys = state.editing_keys.and_then(|k| match k.cmp(&i) {
                std::cmp::Ordering::Equal => None,
                std::cmp::Ordering::Greater => Some(k - 1),
                std::cmp::Ordering::Less => Some(k),
            });
            state.editing_out = state.editing_out.and_then(|k| match k.cmp(&i) {
                std::cmp::Ordering::Equal => None,
                std::cmp::Ordering::Greater => Some(k - 1),
                std::cmp::Ordering::Less => Some(k),
            });
        }

        // **新的排在最上面**——剛加的立刻看得到，不必捲下去找
        state.rows.insert(
            0,
            Row {
                output: word,
                keys,
                lang,
            },
        );
        state.checked.insert(0, false);
        // 原本正在編輯的那一列往下移了一格
        state.editing_keys = state.editing_keys.map(|i| i + 1);
        state.editing_out = state.editing_out.map(|i| i + 1);
        state.draft.clear();
        state.draft_keys.clear();
        state.draft_touched = false;
        state.last_draft.clear();
        state.typed_keys.clear();
        // **游標留在輸出欄**：加完一條直接打下一個，不必再點一次。
        // 這也是 Tab 能跳到按鍵欄的前提——焦點要先在這一列上
        r.request_focus();
    }
}

/// 「輸出」與「怎麼打」兩欄各該多寬。
///
/// **以不吃掉資料為原則**（使用者定的）：先量兩欄各自最長的那一條
/// 實際要多寬，照那個比例分可用寬度。空間夠就照實際需要給（誰也不會
/// 被截斷），不夠才等比縮——縮的時候兩欄一起縮，不會只犧牲一邊。
///
/// 平分的話短的那欄浪費、長的那欄被截斷：`hololive` 跟 `ホロライブ`
/// 差很多，而「怎麼打」放注音時又常比輸出長。
fn split_widths(ui: &egui::Ui, state: &State, avail: f32) -> (f32, f32) {
    // **最小寬度**：窄到不能再窄的那一端。
    //
    // 抓「六個中文字」左右——注音一個字大約是兩個中文字寬
    // （`ㄏㄨˊ`），所以三個字的詞就要這麼寬。太小的話新增一個空包時
    // 欄位會塌到只剩幾個字（使用者實測回報）。
    const MIN: f32 = 220.0;

    let measure = |s: &str| {
        if s.is_empty() {
            return 0.0;
        }
        ui.fonts(|f| {
            f.layout_no_wrap(
                s.to_string(),
                egui::TextStyle::Body.resolve(ui.style()),
                egui::Color32::PLACEHOLDER,
            )
            .size()
            .x
        })
    };

    // 兩欄各自最長的那一條要多寬。**「怎麼打」量的是顯示出來的樣子**
    // （注音／假名），不是按鍵——畫面上放的是前者。
    //
    // **表頭與提示字也要量**：新增一個空包時清單裡一條資料都沒有，
    // 只量資料的話寬度會塌到只夠放表頭那兩個字
    let mut need_out: f32 = measure("輸出");
    let mut need_keys: f32 = measure("怎麼打");
    // **只量前面幾百條**。
    //
    // egui 每一幀都重畫，量整份的話台語包（9 萬條）每秒要跑幾百萬次
    // 文字排版——實測整個設定頁卡住（使用者回報）。抽樣夠準：欄寬只
    // 需要「大概多寬」，而真的有超長的那條也只是被截斷，不是壞掉。
    const SAMPLE: usize = 300;
    for r in state.rows.iter().take(SAMPLE) {
        need_out = need_out.max(measure(&r.output));
        need_keys = need_keys.max(measure(&pretty(&r.keys, r.lang)));
    }
    // 文字框自己的左右內距（`TextEdit` 的邊框與游標空間）
    need_out += 16.0;
    need_keys += 16.0;
    // 文字兩側各留一點呼吸空間
    need_out = need_out.max(MIN) + 12.0;
    need_keys = need_keys.max(MIN) + 12.0;

    let total = need_out + need_keys;
    if total <= avail {
        // 空間夠：照實際需要給，多出來的平分（表格才撐得滿）
        let extra = (avail - total) / 2.0;
        (need_out + extra, need_keys + extra)
    } else {
        // 不夠：等比縮，但都不低於 MIN
        let k = avail / total;
        ((need_out * k).max(MIN), (need_keys * k).max(MIN))
    }
}

/// 清單裡一個「唯讀的格子」：固定寬度、**文字靠左**。
///
/// # 兩件 `add_sized` 做不到的事
///
/// - **靠左**：`add_sized` 會把內容擺到格子正中間，一整欄的文字
///   長短不一時看起來參差不齊（使用者實測回報）。要靠左得自己
///   配一塊空間再在裡面用左對齊的版面。
/// - **編輯模式才反應**：`sense(click)` 一掛上去，滑鼠碰到就變亮。
///   預覽模式根本點不了，變亮只是誤導——所以 `clickable` 是參數。
fn cell(ui: &mut egui::Ui, text: egui::RichText, w: f32, clickable: bool) -> egui::Response {
    let sense = if clickable {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    // **要用 `allocate_exact_size` 而不是 `allocate_ui_with_layout`**。
    //
    // 後者的尺寸是「最多用這麼多」，裡面的 `Label` 只會用文字實際
    // 需要的寬度——那一格就縮到文字大小，Grid 的欄寬跟著縮，下面的
    // 輸入框也被壓扁（使用者實測回報：算出 287 卻只顯示 80）。
    // `allocate_exact_size` 是「這塊就是我的」，欄寬才撐得開。
    let (rect, r) = ui.allocate_exact_size(egui::vec2(w, 20.0), sense);
    // **文字靠左**：`ui.put` 會置中，所以在配到的矩形裡自己起一個
    // 靠左的版面（使用者定的：一整欄的文字長短不一時置中看起來參差）
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.add(egui::Label::new(text).truncate().selectable(false));
    r
}

/// 名稱裡有哪些字元不能當檔名？回傳它們（沒有就是空字串）。
///
/// 名稱直接當檔名（§2.75.11），所以 Windows 不收的那幾個要擋。
/// **兩個平台都擋同一組**——包會被分享出去，在 Mac 上做的包拿到
/// Windows 開不了才是真的麻煩。
fn bad_filename_chars(name: &str) -> String {
    const BAD: &[char] = &['\\', '/', ':', '*', '?', '"', '<', '>', '|'];
    let mut out = String::new();
    for c in name.chars() {
        if BAD.contains(&c) && !out.contains(c) {
            out.push(c);
            out.push(' ');
        }
    }
    out.trim_end().to_string()
}

/// 按鍵欄：**攔實體按鍵自己組，完全繞開輸入法**。
///
/// # 為什麼不能用一般的文字框
///
/// 使用者要在這一格打 `cj6wl6`，但他的系統輸入法（就是通譯自己）會把
/// 它組成「胡桃」。先前試過讀 `Ime(Preedit)` 補救，**實測會掉字**：
/// 第一個 `c` 已經被文字框當成一般字元吃掉，`Preedit` 才接手覆寫，
/// 結果是 `:j6wl6`（使用者實測回報）。兩條路搶同一個欄位就是會漂。
///
/// 所以這裡不看文字內容，只看 `Event::Key`——**實體按鍵不經過輸入法**，
/// 而注音鍵盤要的鍵（字母、數字、`;,./-` 與空白）egui 全都給得出來。
///
/// 回傳這一格有沒有焦點。
fn keys_field(ui: &mut egui::Ui, keys: &mut String, id: egui::Id, width: f32) -> egui::Response {
    // 唯讀的框：**顯示交給 egui，輸入自己來**。給它一份複本，
    // 使用者打不進去（改動不會寫回 `keys`），但游標、選取、捲動照常
    let mut shown = keys.clone();
    let r = ui.add(
        egui::TextEdit::singleline(&mut shown)
            .id(id)
            .desired_width(width),
    );

    if !r.has_focus() {
        return r;
    }
    // 視窗在背景時一律不收——不然別的視窗打字會寫進來
    if !ui.ctx().input(|i| i.viewport().focused.unwrap_or(true)) {
        return r;
    }

    // **兩種來源都要收**。
    //
    // 輸入法關著時按鍵直接來（`Event::Key`）；**開著時按鍵被輸入法
    // 吃掉組字**，egui 收到的是 `Ime` 事件，`Event::Key` 一個都沒有
    // ——只認前者的話這一格永遠打不進東西（使用者實測回報：
    // 「英雄聯盟」的按鍵欄填不了）。
    //
    // spike 驗過 `Ime(Preedit)` 帶著完整的原始按鍵串（§2.75.8），
    // 拿它就對了。
    let mut preedit: Option<String> = None;
    ui.input(|i| {
        for e in &i.events {
            match e {
                // 組字中：整串原始按鍵在這裡
                egui::Event::Ime(egui::ImeEvent::Preedit(s)) => {
                    preedit = Some(s.clone());
                }
                // 組字結束：輸入法送出組好的字，但**我們要的是按鍵**，
                // 所以把最後看到的那串 preedit 留著，不理會送出的內容
                egui::Event::Ime(egui::ImeEvent::Commit(_)) => {}
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    // 有修飾鍵的一律不收（Ctrl+A 之類的留給 egui）
                    if modifiers.ctrl || modifiers.alt || modifiers.command {
                        continue;
                    }
                    match key {
                        egui::Key::Backspace => {
                            keys.pop();
                        }
                        egui::Key::Delete => keys.clear(),
                        _ => {
                            if let Some(c) = key_char(*key) {
                                keys.push(c);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    });
    // 組字中的話，畫面上顯示的就是那串原始按鍵
    if let Some(p) = preedit {
        *keys = p;
    }
    r
}

/// 實體按鍵 → 它在鍵盤上印的那個字元。
///
/// **只收注音鍵盤用得到的**：26 個字母、10 個數字、`;,./-` 與空白。
/// 大千配置把這些全用掉了（見 `bopomofo::keymap`）。
fn key_char(key: egui::Key) -> Option<char> {
    use egui::Key as K;
    Some(match key {
        K::A => 'a',
        K::B => 'b',
        K::C => 'c',
        K::D => 'd',
        K::E => 'e',
        K::F => 'f',
        K::G => 'g',
        K::H => 'h',
        K::I => 'i',
        K::J => 'j',
        K::K => 'k',
        K::L => 'l',
        K::M => 'm',
        K::N => 'n',
        K::O => 'o',
        K::P => 'p',
        K::Q => 'q',
        K::R => 'r',
        K::S => 's',
        K::T => 't',
        K::U => 'u',
        K::V => 'v',
        K::W => 'w',
        K::X => 'x',
        K::Y => 'y',
        K::Z => 'z',
        K::Num0 => '0',
        K::Num1 => '1',
        K::Num2 => '2',
        K::Num3 => '3',
        K::Num4 => '4',
        K::Num5 => '5',
        K::Num6 => '6',
        K::Num7 => '7',
        K::Num8 => '8',
        K::Num9 => '9',
        K::Semicolon => ';',
        K::Comma => ',',
        K::Period => '.',
        K::Slash => '/',
        K::Minus => '-',
        // **空白是一聲**，注音打字非它不可（見 `bopomofo::keymap::TONES`）
        K::Space => ' ',
        _ => return None,
    })
}

/// 按鍵 → 看得懂的形態。**依語言只顯示那一種**（§2.75.9）。
fn pretty(keys: &str, kind: Kind) -> String {
    if keys.trim().is_empty() {
        return String::new();
    }
    match kind {
        Kind::Chinese => keys
            .chars()
            .map(|c| ime_core::bopomofo::keymap::symbol_of(c).unwrap_or(c))
            .collect(),
        Kind::Japanese => ime_core::romaji::kana::to_kana(keys).unwrap_or_else(|| keys.to_string()),
        _ => keys.to_string(),
    }
}

// 最後一次排版的「表頭右緣、資料列捲動區右緣」。**只有測試用得到**。
//
// 這兩個數字必須相等。不等的話表頭會凸進整頁捲軸的地盤，畫面上就是
// 兩支捲軸疊在一起——那個坑踩過兩次（§2.75.12、2026-09-11），
// 所以把它釘成測試而不是只修掉。
#[cfg(test)]
thread_local! {
    static 排版: std::cell::Cell<(f32, f32)> = const { std::cell::Cell::new((0.0, 0.0)) };
}

#[cfg(test)]
fn 記下排版(頭右緣: f32, 身體右緣: f32) {
    排版.with(|c| c.set((頭右緣, 身體右緣)));
}

/// 這串符號會拆成幾個？**問 core，不要自己拆**。
///
/// 拆法有兩套（空白分隔／舊格式逐字元），還要處理 `+` 展開成 ZWJ 與
/// 變體選擇符——規則只有 `pack::split_symbols` 知道，複製一份到 UI
/// 兩邊一定會漂掉。使用者眼裡「一個符號」跟字元數對不上，`👨+👩+👧`
/// 是一個圖、`★☆✦` 是三個，不講的話他不知道自己寫出了什麼。
fn sym_preview(syms: &str) -> String {
    let v = ime_core::pack::split_symbols(syms);
    if v.is_empty() {
        return "這一格拆不出任何符號".to_string();
    }
    // 太多的話只列前面幾個——一組有 27 個的（`\轉彎\`）列滿會擋住畫面
    const SHOW: usize = 12;
    let head: Vec<&str> = v.iter().take(SHOW).map(String::as_str).collect();
    let more = if v.len() > SHOW { "…" } else { "" };
    format!("將拆成 {} 個：{}{}", v.len(), head.join(" "), more)
}

/// 一串按鍵是什麼語言？**引擎現成的那條路**。
fn detect(keys: &str) -> Kind {
    if keys.trim().is_empty() {
        return Kind::Unknown;
    }
    let input = ime_core::input::Input::from_keys(keys, None);
    let Some(best) = input.cuttings().first() else {
        return Kind::Unknown;
    };
    // 整串只有一種語言才算數——混的話交給使用者自己看
    let mut kind = Kind::Unknown;
    for seg in best {
        let k = match seg.lang {
            ime_core::language::Language::Bopomofo => Kind::Chinese,
            ime_core::language::Language::Romaji => Kind::Japanese,
            ime_core::language::Language::English => Kind::English,
        };
        if kind == Kind::Unknown {
            kind = k;
        } else if kind != k {
            return Kind::Unknown;
        }
    }
    kind
}

/// 一個詞要按哪些鍵？查不到就是空字串（讓使用者自己填）。
///
/// 真正的反查在 `ime_core::reverse`——中文掃詞典建反向索引、日文把
/// 假名轉回羅馬字、英文就是它自己。詳見 §2.75.3。
fn guess_keys(word: &str) -> String {
    ime_core::reverse::keys_for(word).unwrap_or_default()
}

/// 讀一個包進來。**檔案格式不變**，只是換一種呈現方式（§2.75.2）。
fn load(state: &mut State, packs_dir: &str, file: &str) {
    state.file = file.to_string();
    state.is_new = false;
    state.editing = false;
    state.editing_keys = None;
    state.editing_out = None;
    state.checked.clear();
    state.draft.clear();
    state.draft_keys.clear();
    state.draft_touched = false;
    state.draft_sym = false;
    state.last_draft.clear();
    state.typed_keys.clear();

    match ime_core::pack::read_editable(packs_dir, file) {
        Ok(data) => {
            state.readonly = data.meta.readonly;
            state.meta = Meta {
                // **名稱沒寫就用檔名**——名稱同時是檔名（§2.75.11），
                // 兩者本來就該一致
                name: data.meta.name.unwrap_or_else(|| file.to_string()),
                version: data.meta.version.unwrap_or_default(),
                author: data.meta.author.unwrap_or_default(),
                description: data.meta.description.unwrap_or_default(),
                license: data.meta.license.unwrap_or_default(),
            };
            state.rows = data.entries.iter().map(to_row).collect();
            // **用檔案裡原本的樣子判**，不是畫面上的：`to_row` 已經把
            // 假名轉成按鍵了，轉完兩種來源長得一模一樣
            state.legacy_ja = data.entries.iter().filter(|e| is_legacy_ja(e)).count();
            state.status = None;
        }
        Err(e) => {
            state.meta = Meta::default();
            state.readonly = false;
            state.rows.clear();
            state.legacy_ja = 0;
            let msg = match e {
                ime_core::pack::PackReadError::NotUtf8 => {
                    // **編碼不對跟「沒有詞」是兩回事**（§2.49.3）——
                    // 格式可能完全正確，錯的是編碼
                    "這個檔不是 UTF-8（多半是用記事本存成「ANSI」）。用記事本開起來，另存為時把編碼改成 UTF-8。"
                }
                ime_core::pack::PackReadError::Missing => "讀不到這個檔案。",
                // 官方包是二進位的，編輯器本來就開不了——但走到這裡
                // 代表使用者按了「編輯」，要講清楚為什麼不行
                ime_core::pack::PackReadError::BadBin => {
                    "這是官方包（二進位），不能在這裡編輯。想改的話請另外建一個包。"
                }
            };
            state.status = Some((msg.to_string(), false));
        }
    }
}

/// 檔案裡這一條的日文鍵是**羅馬字**嗎？——舊版編輯器存壞的樣子。
///
/// 假名裡不會有 ASCII 字母，所以「有字母」就是判準。載入層拿假名當鍵
/// （`pack::編輯::日文的鍵是假名` 釘著），存成羅馬字的整條查不到。
fn is_legacy_ja(e: &ime_core::pack::Entry) -> bool {
    e.lang == "ja" && e.input.chars().any(|c| c.is_ascii_alphabetic())
}

/// 檔案裡的一條 → 畫面上的一列。
///
/// **語言重新算過**，不照檔案裡寫的：檔案是手寫的，`zh` 那一欄可能
/// 寫了根本不是注音的東西。畫面要反映「這串按鍵真的打得出來嗎」。
fn to_row(e: &ime_core::pack::Entry) -> Row {
    // **這兩種不必轉**：符號的第二欄是名字、台語的是華語國字，
    // 兩者都不是按鍵，原樣讀進來就是畫面上要顯示的東西。
    // （符號 2026-09-11 起編得動了，台語還不行——見 `Kind::editable`）
    match e.lang.as_str() {
        "sym" => {
            return Row {
                output: e.output.clone(),
                keys: e.input.clone(),
                lang: Kind::Symbol,
            }
        }
        "tw" => {
            return Row {
                output: e.output.clone(),
                keys: e.input.clone(),
                lang: Kind::Taiwanese,
            }
        }
        _ => {}
    }

    // **檔案裡存的是讀音，畫面上編輯的是按鍵**——兩邊要轉。
    // 中文是注音符號、日文是假名，英文本來就是字母。
    let keys = if e.lang == "zh" {
        // 一聲要補空白，音節邊界的判斷交給 core（見 `pack::bopomofo_to_keys`）
        ime_core::pack::bopomofo_to_keys(&e.input).unwrap_or_else(|| e.input.clone())
    } else if e.lang == "ja" {
        // 假名 → 羅馬字。轉不出來就原樣留著，使用者才看得出哪裡怪
        ime_core::reverse::keys_for(&e.input).unwrap_or_else(|| e.input.clone())
    } else {
        e.input.clone()
    };
    // 英文省略第三欄時，輸出等於輸入
    let output = if e.output.is_empty() {
        e.input.clone()
    } else {
        e.output.clone()
    };
    // **載入時信任檔案寫的語言，不重新辨識**。
    //
    // `detect` 走的是完整的切點引擎（`Input::from_keys`），一條就要
    // 幾毫秒——台語包 9 萬條的話開這個包要等好幾分鐘（使用者實測
    // 回報「怎麼那麼卡」）。而檔案裡的代號本來就是對的，重算沒有
    // 好處。**使用者改了按鍵才重算那一條**（見 `row_widgets`）。
    let lang = match e.lang.as_str() {
        "zh" => Kind::Chinese,
        "ja" => Kind::Japanese,
        "en" => Kind::English,
        // 認不得的代號：那一條在載入層本來就會被丟掉，標成未知讓
        // 使用者看得見
        _ => Kind::Unknown,
    };
    Row { lang, keys, output }
}

/// 畫面上的一列 → 檔案裡的一條。
fn to_entry(r: &Row) -> ime_core::pack::Entry {
    let lang = match r.lang {
        Kind::Chinese => "zh",
        Kind::Japanese => "ja",
        Kind::English => "en",
        Kind::Symbol => "sym",
        Kind::Taiwanese => "tw",
        // 辨識不出來的存不了（`invalid_rows` 會先擋下來），
        // 真的走到這裡就當中文——至少不會靜靜消失
        Kind::Unknown => "zh",
    };
    // 符號與台語原樣送回去，沒有按鍵這回事
    if matches!(r.lang, Kind::Symbol | Kind::Taiwanese) {
        return ime_core::pack::Entry {
            lang: lang.to_string(),
            input: r.keys.clone(),
            output: r.output.clone(),
        };
    }
    // **中文轉回注音、日文轉回假名**——檔案裡存的是「人看得懂的讀音」，
    // 不是按鍵。載入層（`pack::build_index`）也是照這個假設查的：
    // 日文的鍵是假名，存成羅馬字的話**整條查不到**（實測回報：
    // 包裡寫 `ja asee game`，打 `asee` 出不來 `game`）。
    let input = if r.lang == Kind::Chinese {
        ime_core::pack::keys_to_bopomofo(&r.keys)
    } else if r.lang == Kind::Japanese {
        // 轉不出來就原樣留著——使用者才看得出哪裡怪，而不是靜靜存錯
        ime_core::romaji::kana::to_kana(&r.keys).unwrap_or_else(|| r.keys.clone())
    } else {
        r.keys.clone()
    };
    ime_core::pack::Entry {
        lang: lang.to_string(),
        input,
        output: r.output.clone(),
    }
}

/// 存檔。**寫回同樣的 `.txt`**，格式一行都沒變（§2.75.2）。
fn save(state: &mut State, packs_dir: &str) {
    // **唯讀的包一律不存**。UI 上已經不給編了，這是第二道閘門——
    // 「不給編」是畫面狀態，可能因為載入順序之類的原因失守，
    // 而真正會毀掉檔案的是這裡
    if state.readonly {
        state.editing = false;
        state.status = Some(("這個包是唯讀的，沒有存進去。".into(), false));
        return;
    }

    // 名稱就是檔名（§2.75.11）
    let name = state.meta.name.trim().to_string();
    if name.is_empty() {
        state.status = Some(("名稱要填——它同時是檔名。".into(), false));
        return;
    }

    // 存檔時自動進版
    if state.bump_version {
        state.meta.version = bump(&state.meta.version);
    }

    let data = ime_core::pack::Editable {
        meta: ime_core::pack::Meta {
            name: Some(name.clone()),
            version: opt(&state.meta.version),
            author: opt(&state.meta.author),
            description: opt(&state.meta.description),
            license: opt(&state.meta.license),
            // **更新日期自己填**——使用者不必去想今天幾號
            updated: Some(today()),
            homepage: None,
            // 走到這裡的一定不是唯讀的包（上面擋掉了），
            // 而使用者自己做的包本來就該可以再改
            readonly: false,
        },
        entries: state.rows.iter().map(to_entry).collect(),
    };

    match ime_core::pack::write_editable(packs_dir, &name, &data) {
        Ok(path) => {
            let renamed = !state.is_new && state.file != name;
            state.file = name;
            state.is_new = false;
            state.editing = false;
            state.editing_keys = None;
            state.editing_out = None;
            // 存回去的鍵已經是假名了（`to_entry` 轉過），舊格式的帳清掉
            state.legacy_ja = 0;
            let mut msg = format!("已儲存：{}", path.display());
            if renamed {
                // **改名等於另存一份**——舊檔還在，而設定裡啟用的是舊檔名
                msg.push_str("（名稱改了，等於新的一份；舊的那個還在，記得去上面重新勾選）");
            }
            state.status = Some((msg, true));
        }
        Err(e) => state.status = Some((format!("儲存失敗：{e}"), false)),
    }
}

/// 空字串當成「沒填」。
fn opt(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// 版本加一。**整數版號**（§2.75）——認不得的就從 1 開始。
fn bump(v: &str) -> String {
    v.trim()
        .parse::<u32>()
        .map(|n| n + 1)
        .unwrap_or(1)
        .to_string()
}

/// 今天的日期，`2026-09-10` 這種寫法。
///
/// 不拉 chrono 進來——只為了一個日期多背一個套件不划算，
/// 而「從 Unix 紀元換算年月日」是幾行算術。
fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86400) as i64;
    // 民用曆法的反算（Howard Hinnant 的 civil_from_days）
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// 管**整個包**的三個操作：新增、編輯、刪除。
///
/// 跟下面清單旁邊那排分開——那排管的是「這些詞」（儲存、取消、
/// 刪除勾選的），這排管的是「這個包」。
fn actions_row(
    ui: &mut egui::Ui,
    state: &mut State,
    cfg: &mut ime_core::config::Config,
    cache: &mut Option<Vec<ime_core::pack::Info>>,
) {
    let picked = state.is_new || !state.file.is_empty();

    ui.horizontal(|ui| {
        if ui.button("＋ 新增擴充包").clicked() {
            state.file.clear();
            state.is_new = true;
            state.editing = true;
            state.meta = Meta::default();
            state.readonly = false; // 新的包一定不唯讀
            state.legacy_ja = 0;
            state.rows.clear();
            state.checked.clear();
            state.draft.clear();
            state.draft_keys.clear();
            state.draft_touched = false;
            state.draft_sym = false;
            state.last_draft.clear();
            state.typed_keys.clear();
            state.editing_keys = None;
            state.editing_out = None;
            state.confirm_delete = false;
            state.status = None;
        }

        // 底下的要先選一個包
        if !picked {
            return;
        }

        // **刪除擴充包排在編輯前面**（使用者定的）。
        // 不可逆，所以兩段式確認；唯讀的包與還沒存過的新包不給刪
        if !state.readonly && !state.is_new {
            if state.confirm_delete {
                if ui.button("確定").clicked() {
                    let dir = cfg.behavior.packs_dir.clone();
                    delete(state, &dir, cfg, cache);
                }
                if ui.button("取消").clicked() {
                    state.confirm_delete = false;
                }
                ui.colored_label(
                    egui::Color32::from_rgb(0xC6, 0x28, 0x28),
                    format!("確定要刪除「{}」嗎？", state.file),
                );
            } else if ui.button("刪除擴充包").clicked() {
                state.confirm_delete = true;
            }
            ui.add_space(12.0);
        }

        // **編輯／儲存／取消同一個位置**（使用者定的）：
        // 預覽時是「編輯」，編輯時換成「儲存」與「取消」
        if state.readonly {
            ui.label(egui::RichText::new("唯讀").weak())
                .on_hover_text("這是隨程式一起裝的包，改了下次更新會被覆蓋。要改的話複製一份到你自己的資料夾，改那一份。");
        } else if state.editing {
            save_cancel(ui, state, cfg, cache);
        } else if ui.button("編輯").clicked() {
            state.editing = true;
            state.status = None;
            state.confirm_delete = false;
        }
    });
}

/// 「儲存」與「取消」。**跟「編輯」同一個位置**，所以抽出來共用。
fn save_cancel(
    ui: &mut egui::Ui,
    state: &mut State,
    cfg: &mut ime_core::config::Config,
    cache: &mut Option<Vec<ime_core::pack::Info>>,
) {
    // **有無效資料就不給存**（使用者定的）。名稱同時是檔名，
    // 所以它空著或有非法字元也一樣擋
    let bad = invalid_rows(state);
    let name_ok =
        !state.meta.name.trim().is_empty() && bad_filename_chars(&state.meta.name).is_empty();
    // **一條都沒有不給存**：存出來是個空檔案，清單上顯示「還沒有詞」，
    // 勾選框還是灰的——等於什麼都沒發生
    let has_rows = !state.rows.is_empty();
    let can_save = bad.is_empty() && name_ok && has_rows;

    let btn = ui.add_enabled(can_save, egui::Button::new("儲存"));
    let btn = if can_save {
        btn
    } else if !name_ok {
        btn.on_disabled_hover_text("名稱要填，而且不能有檔名不允許的字元")
    } else if !has_rows {
        btn.on_disabled_hover_text("一條詞都還沒有——在下面加一條再存")
    } else {
        btn.on_disabled_hover_text("有資料無效，見下面的清單")
    };
    if btn.clicked() {
        let dir = cfg.behavior.packs_dir.clone();
        save(state, &dir);
        *cache = None; // 詞數變了，清單要重掃
    }

    if ui.button("取消").clicked() {
        // **回到磁碟上的內容**——改壞了可以退回去
        if state.is_new {
            state.is_new = false;
            state.file.clear();
            state.meta = Meta::default();
            state.readonly = false;
            state.rows.clear();
            state.checked.clear();
            state.editing = false;
            state.status = None;
        } else {
            let f = state.file.clone();
            let dir = cfg.behavior.packs_dir.clone();
            load(state, &dir, &f);
        }
    }
}

/// 刪掉目前這個包。
///
/// **檔案真的會不見**，所以呼叫端要先問過（`confirm_delete`）。
/// 順手把它從「已啟用」的設定裡拿掉——不拿的話清單上會多一列
/// 紅字「找不到檔案」，使用者得再點一次「從清單移除」。
fn delete(
    state: &mut State,
    packs_dir: &str,
    cfg: &mut ime_core::config::Config,
    cache: &mut Option<Vec<ime_core::pack::Info>>,
) {
    let Some(dir) = ime_core::pack::resolved_dir(packs_dir) else {
        state.status = Some(("找不到擴充包資料夾。".into(), false));
        return;
    };
    let path = dir.join(format!("{}.txt", state.file));
    match std::fs::remove_file(&path) {
        Ok(()) => {
            cfg.behavior.packs.retain(|p| p != &state.file);
            let name = std::mem::take(&mut state.file);
            state.is_new = false;
            state.editing = false;
            state.confirm_delete = false;
            state.meta = Meta::default();
            state.rows.clear();
            state.checked.clear();
            state.editing_keys = None;
            state.editing_out = None;
            *cache = None;
            state.available = None;
            state.status = Some((format!("已刪除「{name}」。"), true));
        }
        Err(e) => {
            state.confirm_delete = false;
            state.status = Some((format!("刪除失敗：{e}"), false));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ime_core::pack::Entry;

    fn entry(lang: &str, input: &str, output: &str) -> Entry {
        Entry {
            lang: lang.into(),
            input: input.into(),
            output: output.into(),
        }
    }

    /// 舊版把**羅馬字**存進日文那一欄，載入層拿假名當鍵，整條查不到
    /// （§2.75.13 第一件）。判準是「有沒有 ASCII 字母」。
    #[test]
    fn 認得出舊版存壞的日文鍵() {
        assert!(is_legacy_ja(&entry("ja", "asee", "game")));
        assert!(is_legacy_ja(&entry("ja", "hororaibu", "ホロライブ")));
        assert!(!is_legacy_ja(&entry("ja", "あせえ", "game")));
        assert!(!is_legacy_ja(&entry("ja", "ホロライブ", "hololive")));
        // 長音符號不是字母
        assert!(!is_legacy_ja(&entry("ja", "ラーメン", "拉麵")));
        // 別的語言不管——英文的鍵本來就是字母
        assert!(!is_legacy_ja(&entry("en", "hololive", "")));
        assert!(!is_legacy_ja(&entry("zh", "ㄏㄨˊㄊㄠˊ", "胡桃")));
    }

    /// **重存一次就修好**：存檔轉成假名、再讀回來轉回按鍵，
    /// 畫面上看到的按鍵跟修正前一樣，但檔案裡的鍵已經是假名了。
    #[test]
    fn 重存一次就把舊版的鍵修回假名() {
        let 壞的 = entry("ja", "asee", "game");
        let row = to_row(&壞的);
        assert_eq!(row.lang, Kind::Japanese);
        assert_eq!(row.keys, "asee", "畫面上編輯的是按鍵");

        let 存回去 = to_entry(&row);
        assert_eq!(存回去.input, "あせえ", "檔案裡存的要是假名");
        assert!(!is_legacy_ja(&存回去), "存完就不該再被認成舊格式");

        // 再讀回來，使用者看到的按鍵不變
        assert_eq!(to_row(&存回去).keys, "asee");
    }

    /// 日文表記跟讀音一樣的詞（純假名）也要存得成三欄——
    /// 第三欄省掉的話載入層會丟掉整條（§2.75.13 第二件）。
    #[test]
    fn 純假名的詞存完仍然讀得回來() {
        let row = Row {
            output: "すごい".into(),
            keys: "sugoi".into(),
            lang: Kind::Japanese,
        };
        let e = to_entry(&row);
        assert_eq!(e.input, "すごい");
        assert_eq!(e.output, "すごい");
    }

    /// 符號列**讀進來、存回去都不轉**——第二欄是名字不是按鍵。
    #[test]
    fn 符號列原樣往返() {
        let e = entry("sym", "星,hoshi,star", "★ ☆ ✦");
        let row = to_row(&e);
        assert_eq!(row.lang, Kind::Symbol);
        assert_eq!(row.keys, "星,hoshi,star", "名字原樣");
        assert_eq!(row.output, "★ ☆ ✦");
        assert_eq!(to_entry(&row), e, "存回去要一模一樣");
    }

    /// 符號的「怎麼打」是名字，**不能拿去 detect、也不能攔實體按鍵**。
    #[test]
    fn 符號那一欄不是按鍵() {
        assert!(!Kind::Symbol.keys_are_keys());
        assert!(Kind::Chinese.keys_are_keys());
        // 名字拿去辨識語言會判成別的東西——這正是不能重算的理由
        assert_ne!(detect("星"), Kind::Symbol);
    }

    /// 符號編得動了，台語還不行（§2.75.6）。
    #[test]
    fn 哪幾種編得動() {
        assert!(Kind::Symbol.editable());
        assert!(Kind::Chinese.editable());
        assert!(!Kind::Taiwanese.editable());
    }

    fn 符號列(name: &str, syms: &str) -> State {
        let mut st = State {
            editing: true,
            ..Default::default()
        };
        st.rows = vec![Row {
            output: syms.into(),
            keys: name.into(),
            lang: Kind::Symbol,
        }];
        st.checked = vec![false];
        st
    }

    #[test]
    fn 符號列的四種擋法() {
        assert!(
            invalid_rows(&符號列("星", "★ ☆ ✦")).is_empty(),
            "正常的不該被擋"
        );
        assert_eq!(invalid_rows(&符號列("星", ""))[0].1, "沒有符號");
        assert_eq!(invalid_rows(&符號列("", "★"))[0].1, "還沒填名字");
        // `merge_symbols` 靠一對 `\` 找名字，名字裡再有一個就永遠配不起來
        assert_eq!(
            invalid_rows(&符號列(r"星\亮", "★"))[0].1,
            "名字裡不能有反斜線"
        );
    }

    /// 拆解預覽要**問 core**，不能自己拆——兩套規則（空白分隔／舊格式
    /// 逐字元）與 `+` 展開成 ZWJ 只有 `pack::split_symbols` 知道。
    #[test]
    fn 拆解預覽跟core同一套() {
        assert!(sym_preview("★ ☆ ✦").starts_with("將拆成 3 個："));
        // 舊格式連著寫，逐字元拆
        assert!(sym_preview("★☆✦").starts_with("將拆成 3 個："));
        // `+` 是 ZWJ 標記，展開之後是**一個**圖
        assert!(sym_preview("👨+👩+👧").starts_with("將拆成 1 個："));
        assert_eq!(sym_preview(""), "這一格拆不出任何符號");
    }

    /// 拼不出假名的按鍵要當場擋下來，不能讓它存進去之後靜靜消失。
    #[test]
    fn 拼不出假名的日文列存不得() {
        let mut st = State {
            editing: true,
            ..Default::default()
        };
        st.rows = vec![Row {
            output: "測試".into(),
            // `q` 在羅馬字表裡拼不成音節
            keys: "qqq".into(),
            lang: Kind::Japanese,
        }];
        st.checked = vec![false];
        let bad = invalid_rows(&st);
        assert_eq!(bad.len(), 1);
        assert_eq!(bad[0].1, "這串按鍵拼不出假名");
    }
}

#[cfg(test)]
mod 排版 {
    use super::*;

    /// 把整個擴充包分頁在無視窗的 egui 裡排一次版，回傳
    /// （表頭右緣, 資料列捲動區右緣）。
    ///
    /// **巢狀要跟 `main.rs` 一樣**：中央面板包一個整頁的捲動區，
    /// 裡面才是這一頁——兩支捲軸會不會打架，取決於這個巢狀。
    fn 排一次(寬: f32) -> (f32, f32) {
        let ctx = egui::Context::default();
        let mut st = State {
            file: "測試包".into(),
            editing: true,
            ..Default::default()
        };
        st.rows = (0..50)
            .map(|i| Row {
                output: format!("測試詞{i}"),
                keys: "cj6wl6".into(),
                lang: Kind::Chinese,
            })
            .collect();
        st.checked = vec![false; st.rows.len()];
        let mut cfg = ime_core::config::Config {
            behavior: ime_core::config::Behavior {
                // 不要去翻使用者真正的包資料夾——測的是排版
                packs_dir: "/__不存在的資料夾__".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut cache = None;

        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(寬, 700.0),
            )),
            ..Default::default()
        };
        // **跑兩幀**：第一幀 egui 還在量東西，寬度要第二幀才穩定
        for _ in 0..2 {
            let _ = ctx.run(raw.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        page(ui, &mut st, &mut cfg, &mut cache);
                    });
                });
            });
            raw = egui::RawInput {
                screen_rect: raw.screen_rect,
                ..Default::default()
            };
        }
        排版.with(|c| c.get())
    }

    /// **整頁不可以比視窗寬**——一寬，所有「往左退一個捲軸寬」的退讓
    /// 都退在灌水的父容器裡，畫面上就是兩支捲軸疊在一起。
    ///
    /// 觸發點是清單裡自由文字的欄（說明、作者、名稱）在 Grid 裡不換行。
    /// 這個情境**無視窗的測試原本重現不了**（包資料夾是空的），所以這裡
    /// 真的做一個說明很長的包給它讀。
    #[test]
    fn 清單有長說明時整頁不會比視窗寬() {
        let dir = std::env::temp_dir().join(format!("tsunagi_pack_list_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("說明很長的包.txt"),
            "# name: 名字也很長很長很長很長很長很長的擴充包\n\
             # author: 作者名字也很長很長很長很長很長很長很長\n\
             # description: 這一句說明非常長非常長非常長非常長非常長非常長非常長非常長非常長非常長非常長非常長，長到一定比視窗寬\n\
             \nen\thello\n",
        )
        .unwrap();

        let ctx = egui::Context::default();
        let mut st = State::default();
        let mut cfg = ime_core::config::Config {
            behavior: ime_core::config::Behavior {
                packs_dir: dir.display().to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut cache = None;
        let rect = Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(852.0, 700.0),
        ));
        let mut worst: Option<(f32, f32)> = None;
        // 三幀：撐寬是跨幀的連鎖反應，第一幀看不出來
        for _ in 0..3 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: rect,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let out = egui::ScrollArea::vertical().show(ui, |ui| {
                            page(ui, &mut st, &mut cfg, &mut cache);
                        });
                        let w = (out.content_size.x, out.inner_rect.width());
                        if worst.is_none_or(|(c, v)| w.0 - w.1 > c - v) {
                            worst = Some(w);
                        }
                    });
                },
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
        let (content, viewport) = worst.unwrap();
        assert!(
            content <= viewport + 0.5,
            "整頁內容寬 {content} 超過視窗 {viewport}——清單裡有一欄沒設上限"
        );
    }

    /// **表頭不可以比資料列寬**。
    ///
    /// 表頭刻意放在捲動區外面（捲到第一千條時還看得到），但 `fill()`
    /// 拿的是沒退讓過的 `available_width`，於是比捲動區多凸出整整一個
    /// 捲軸寬——畫面上就是「兩支捲軸疊在一起」（實測回報兩次：
    /// §2.75.12，以及 2026-09-11 加符號編輯之後）。
    #[test]
    fn 表頭與資料列的右緣要對齊() {
        for 寬 in [700.0, 900.0, 1200.0] {
            let (頭, 身體) = 排一次(寬);
            assert!(
                (頭 - 身體).abs() < 0.5,
                "視窗 {寬}px：表頭右緣 {頭} 跟資料列捲動區右緣 {身體} 對不齊，\
                 差 {} px——表頭會凸到整頁捲軸上面",
                頭 - 身體
            );
        }
    }
}
