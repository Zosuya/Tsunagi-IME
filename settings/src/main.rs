//! 通譯輸入法的設定頁。
//!
//! **獨立執行檔**，不是輸入法 DLL 的一部分——見 `Cargo.toml` 的理由。
//!
//! # 設定改完怎麼生效
//!
//! 這裡只負責把 `config.toml` 寫到 `%APPDATA%\tsunagi-ime\`。
//! 輸入法那邊在**開始組字時**比對檔案時間戳，改過就重讀。
//!
//! 所以按下儲存之後，切回文件打第一個字就生效了——不必重新載入
//! 輸入法，也不必重開 App。
//!
//! 用法：cargo run -p ime-settings

#![windows_subsystem = "windows"] // 不要跳出主控台視窗

mod color;
mod debug_page;
mod font_dialog;
mod image_dialog;
mod image_load;
mod preview_font;
mod preview_pane;

// 這兩個是從本檔拆出去的，**用 glob 匯入是刻意的**——拆的目的是
// 把「畫預覽」跟「設定項」分開看，不是要在呼叫處多一層前綴。
use color::*;
use preview_pane::*;

use eframe::egui;
use ime_core::config::{Colors, Config, EnterInSelect, Font, Metrics};

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            // **尺寸要跟著 `enlarge_ui` 的縮放一起放大**——只放大字級不放大
            // 視窗的話，內容會擠到得一直捲動（外觀分頁尤其明顯）。
            .with_inner_size([980.0, 780.0])
            .with_min_inner_size([820.0, 620.0])
            .with_title("通 · つなぎ 輸入法 設定"),
        ..Default::default()
    };
    eframe::run_native(
        "ime-settings",
        opts,
        Box::new(|cc| {
            let mut app = App::new();
            // 開場就把字型備好，不然第一幀會閃一下預設字型
            app.loaded_font = app.cfg.font.family.clone();
            install_cjk_font(&cc.egui_ctx, &app.loaded_font);
            enlarge_ui(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
}

/// egui 內建字型沒有中文字，不裝的話介面全是豆腐方塊。
///
/// 從系統字型目錄找微軟正黑體；找不到就退回內建字型（英文還是看得懂，
/// 中文會變方塊，但**不能因為字型問題就開不起來**）。
/// 預覽區專用的字型家族名稱。介面自己用的是另一份。
pub const PREVIEW_FAMILY: &str = "preview";

fn install_cjk_font(ctx: &egui::Context, preview_family: &str) {
    let candidates = [
        r"C:\Windows\Fonts\msjh.ttc",    // 微軟正黑體
        r"C:\Windows\Fonts\msjhl.ttc",   // 微軟正黑體 Light
        r"C:\Windows\Fonts\mingliu.ttc", // 細明體
    ];
    let Some(bytes) = candidates.iter().find_map(|p| std::fs::read(p).ok()) else {
        return;
    };

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "cjk".to_owned(),
        std::sync::Arc::new(egui::FontData::from_owned(bytes.clone())),
    );
    // 插在最前面：優先用它，找不到的字才往後退
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "cjk".to_owned());
    }

    // **預覽區用使用者選的字型**，介面維持原本的。
    // 撈不到就退回介面那份——換字型看不出差別，總比整個閃退好。
    let preview = preview_font::family_font(preview_family)
        .unwrap_or(preview_font::Loaded { bytes, index: 0 });
    let mut data = egui::FontData::from_owned(preview.bytes);
    // **字型集合要指定第幾個字面**，不指定的話拿到的是同一個檔案裡
    // 的別種字型（msjh.ttc 裡就有正黑體與正黑體 UI 兩個）
    data.index = preview.index;
    fonts
        .font_data
        .insert("preview".to_owned(), std::sync::Arc::new(data));
    fonts.families.insert(
        egui::FontFamily::Name(PREVIEW_FAMILY.into()),
        // 後面接 cjk：選的字型缺哪個字就退回去補，不會變豆腐方塊
        vec!["preview".to_owned(), "cjk".to_owned()],
    );

    ctx.set_fonts(fonts);
}

/// 把整個介面調大一點。
///
/// egui 的預設字級偏小，中文又比拉丁字母需要更多字面才看得清
/// （筆畫多）。用 `zoom_factor` 等比放大，不必逐一調每種文字樣式，
/// 版面間距也會跟著等比例長大。
fn enlarge_ui(ctx: &egui::Context) {
    ctx.set_zoom_factor(1.15);
}

// 版面基準值**從 `core` 拿**，不再手抄一份。
//
// 以前這裡寫著「跟 platform/windows/src/theme.rs 的 fixed 對齊」，
// 靠註解提醒人記得同步——那種對齊遲早失守。

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Behavior,
    Select,
    Appearance,
    Packs,
    Debug,
}

struct App {
    cfg: Config,
    tab: Tab,
    /// 存檔結果的提示訊息（成功或失敗）
    status: Option<(String, bool)>,
    /// 除錯分頁的狀態（測試輸入與引擎）
    dbg: debug_page::DebugState,
    /// 上次存檔時的設定長什麼樣（序列化後的字串）。
    ///
    /// **不用 `PartialEq` 比結構**：那要替整串巢狀型別都加 derive，
    /// 而這裡只需要「一樣不一樣」。存檔本來就是寫這份字串，比它最
    /// 貼近事實——使用者眼中的「有沒有改」就是「檔案內容會不會變」。
    saved: String,
    /// 正在問「還沒存要不要存」。
    asking_close: bool,
    /// 擴充包清單的快取。`None` 代表還沒掃過。
    ///
    /// **一定要快取**——egui 每一幀都重畫，直接在畫的時候掃資料夾＋
    /// 讀檔會每秒摔硬碟幾十次。掃一次存起來，按「重新整理」才重掃。
    packs: Option<Vec<ime_core::pack::Info>>,
    /// 學習檔的條數快取。**egui 是 immediate mode**，每一幀都會跑一次
    /// 畫面程式碼——不快取的話等於每秒讀六十次檔。鍵是檔案的修改時間，
    /// 使用者手動刪行之後回到這一頁會自動更新。
    learn_stats: Option<(Option<std::time::SystemTime>, (usize, usize))>,
    /// 目前載進 egui 的預覽字型是哪一個。
    ///
    /// 換字型時要重新載入才看得到效果，但**載字型很貴**（讀檔＋重建
    /// 整份字型表），不能每一幀都做——記住現在是哪個，變了才重載。
    loaded_font: String,
}

impl App {
    fn new() -> Self {
        // 開啟時讀現有設定；沒有就是預設值
        let cfg = Config::load(project_data_dir().as_deref());
        Self {
            saved: cfg.to_toml(),
            cfg,
            tab: Tab::Behavior,
            status: None,
            dbg: debug_page::DebugState::default(),
            asking_close: false,
            packs: None,
            learn_stats: None,
            loaded_font: String::new(),
        }
    }

    /// 改過但還沒存嗎？
    fn dirty(&self) -> bool {
        self.cfg.to_toml() != self.saved
    }

    fn save(&mut self) {
        self.status = Some(match self.cfg.save() {
            Ok(p) => {
                // **存成功才更新基準**——失敗的話還是「未儲存」，
                // 關視窗時仍該攔下來
                self.saved = self.cfg.to_toml();
                (format!("已儲存到 {}", p.display()), true)
            }
            Err(e) => (format!("存檔失敗：{e}"), false),
        });
    }
}

impl App {
    /// 「還沒儲存」的確認框。
    ///
    /// 三個選項而不是兩個：**「取消」不能省**——使用者可能只是手滑
    /// 點到關閉，這時他要的既不是存也不是丟，而是回去繼續改。
    fn ask_close(&mut self, ctx: &egui::Context) {
        egui::Modal::new(egui::Id::new("unsaved")).show(ctx, |ui| {
            ui.heading("尚未儲存");
            ui.add_space(6.0);
            ui.label("設定改過了但還沒儲存，關掉的話這些調整會消失。");
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("儲存並關閉").clicked() {
                    self.save();
                    // 存失敗就不關——訊息留在畫面上讓使用者看到原因
                    if !self.dirty() {
                        self.asking_close = false;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
                if ui.button("不儲存就關閉").clicked() {
                    // 讓 `dirty()` 不再成立，不然攔截會再觸發一次
                    self.saved = self.cfg.to_toml();
                    self.asking_close = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if ui.button("取消").clicked() {
                    self.asking_close = false;
                }
            });
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // **改過還沒存就攔下關閉**——設定頁改半天關掉才發現沒存，
        // 那些調整全白做了
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.asking_close = true;
        }
        if self.asking_close {
            self.ask_close(ctx);
        }

        // **換了字型就重載**，不然預覽長得跟原本一模一樣，
        // 使用者會以為設定沒生效
        if self.loaded_font != self.cfg.font.family {
            self.loaded_font = self.cfg.font.family.clone();
            install_cjk_font(ctx, &self.loaded_font);
        }
        // 設定檔關掉 debug 之後，別停在一個看不見的分頁上
        if self.tab == Tab::Debug && !self.cfg.debug {
            self.tab = Tab::Behavior;
        }
        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Behavior, "  行為  ");
                ui.selectable_value(&mut self.tab, Tab::Select, "  選字  ");
                ui.selectable_value(&mut self.tab, Tab::Appearance, "  外觀  ");
                ui.selectable_value(&mut self.tab, Tab::Packs, "  擴充包  ");
                // 除錯分頁預設不顯示——設定檔寫 `debug = true` 才出現。
                // 平常使用者不需要看到引擎的內部狀態。
                if self.cfg.debug {
                    ui.selectable_value(&mut self.tab, Tab::Debug, "  除錯  ");
                }
            });
            ui.add_space(4.0);
        });

        // 除錯分頁沒有要存的東西，不畫底部那排按鈕
        let show_actions = self.tab != Tab::Debug;
        egui::TopBottomPanel::bottom("actions").show_animated(ctx, show_actions, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("儲存").clicked() {
                    self.save();
                }
                if ui.button("恢復預設").clicked() {
                    self.cfg = Config::default();
                    self.status = Some(("已恢復預設值（尚未儲存）".into(), true));
                }
                if let Some((msg, ok)) = &self.status {
                    ui.separator();
                    let color = if *ok {
                        egui::Color32::from_rgb(0x2E, 0x7D, 0x32)
                    } else {
                        egui::Color32::from_rgb(0xC6, 0x28, 0x28)
                    };
                    ui.colored_label(color, msg);
                }
            });
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("儲存後切回文件打第一個字就生效，不必重新載入輸入法。").weak(),
            );
            ui.add_space(6.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            match self.tab {
                // 除錯分頁自己管捲動——它下半部要獨立捲，
                // 外面再包一層捲動區會兩層互相打架
                Tab::Debug => {
                    debug_page::debug_page(ui, &mut self.dbg, project_data_dir().as_deref())
                }
                _ => {
                    egui::ScrollArea::vertical().show(ui, |ui| match self.tab {
                        Tab::Behavior => behavior_page(ui, &mut self.cfg),
                        Tab::Select => select_page(ui, &mut self.cfg, &mut self.learn_stats),
                        Tab::Appearance => appearance_page(ui, &mut self.cfg),
                        Tab::Packs => packs_page(ui, &mut self.cfg, &mut self.packs),
                        Tab::Debug => unreachable!(),
                    });
                }
            }
        });
    }
}

fn behavior_page(ui: &mut egui::Ui, cfg: &mut Config) {
    ui.add_space(8.0);
    ui.heading("啟用的語言");
    ui.add_space(6.0);
    ui.label(
        // **不要用 Markdown 語法**——egui 的 RichText 不解析，`**` 會原樣
        // 印出來。同理，字串裡換行的話原始碼縮排也會變成內容的一部分，
        // 要寫成一行讓 egui 自己折行。
        egui::RichText::new(
            "關掉的語言連自動辨識都會跳過——不打日文的話關掉它，sushi 就穩定判成英文，不會忽然變成「すし」。",
        )

            .weak(),
    );
    ui.add_space(6.0);
    ui.checkbox(&mut cfg.behavior.engines.bopomofo, "中文注音");
    ui.checkbox(&mut cfg.behavior.engines.romaji, "日文（羅馬字）");
    // **英文不能關**：它是瀑布的最後一站（passthrough），
    // 關掉的話有些按鍵組合會沒有任何語言接得住。
    ui.add_enabled_ui(false, |ui| {
        let mut always = true;
        ui.checkbox(&mut always, "英文（一律啟用）");
    });
    if !cfg.behavior.engines.bopomofo && !cfg.behavior.engines.romaji {
        ui.add_space(4.0);
        ui.colored_label(
            egui::Color32::from_rgb(0xC6, 0x28, 0x28),
            "兩個都關掉的話就只剩英文，跟一般鍵盤沒有差別。",
        );
    }

    ui.add_space(18.0);
    ui.heading("標點");
    ui.add_space(6.0);
    ui.label("全形／半形（打字時按 Shift+空白可隨時切換，這裡設的是開機預設）：");
    ui.radio_value(
        &mut cfg.behavior.width,
        ime_core::width::Width::Auto,
        "自動——中日文旁邊用全形，英文旁邊用半形",
    );
    ui.radio_value(
        &mut cfg.behavior.width,
        ime_core::width::Width::Half,
        "一律半形",
    );
    ui.radio_value(
        &mut cfg.behavior.width,
        ime_core::width::Width::Full,
        "一律全形（標點、英文、數字都轉）",
    );
}

/// 把這一格撐到剩下的寬度，讓 `Grid` 的隔行底色塗滿整列。
fn fill(ui: &mut egui::Ui) {
    ui.allocate_space(egui::vec2(ui.available_width(), 0.0));
}

/// 叫出系統的「選擇資料夾」對話框，回傳選到的路徑。取消就回 `None`。
///
/// 用 `IFileOpenDialog` 加 `FOS_PICKFOLDERS`——那是 Vista 之後選資料夾的
/// 正規做法，樣式跟檔案總管一致。這個 crate 本來就相依 `windows`
/// （字型對話框、背景圖解碼都在用），所以不必為它多背套件。
///
/// # COM 初始化
///
/// `CoInitializeEx` 的回傳值**故意不看**：視窗框架（winit）多半已經在
/// 這條執行緒初始化過 COM 了，這時會回 `S_FALSE`（已初始化）或
/// `RPC_E_CHANGED_MODE`（模式不同）。兩種都不影響接下來的呼叫，
/// 硬要當成錯誤反而讓按鈕永遠沒反應。
fn pick_folder(start: Option<&std::path::Path>) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        FileOpenDialog, IFileOpenDialog, IShellItem, SHCreateItemFromParsingName, FOS_PICKFOLDERS,
        SIGDN_FILESYSPATH,
    };

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        // 預設是選檔案，要明講改成選資料夾
        let opts = dialog.GetOptions().ok()?;
        dialog.SetOptions(opts | FOS_PICKFOLDERS).ok()?;

        // 從現在這個位置開始逛，使用者不必從頭找起
        if let Some(p) = start {
            let wide: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
            if let Ok(item) =
                SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(wide.as_ptr()), None)
            {
                let _ = dialog.SetFolder(&item);
            }
        }

        // 取消會回 Err（HRESULT 是 ERROR_CANCELLED），所以 `?` 就是
        // 「使用者按了取消」的正常出口，不是錯誤
        dialog.Show(None).ok()?;
        let item = dialog.GetResult().ok()?;
        let raw = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = raw.to_string().ok();
        // 這個字串是 shell 配置的，要還給它
        CoTaskMemFree(Some(raw.0 as *const _));
        path
    }
}

/// 滑鼠停在包名上時顯示的完整基本資料。
///
/// 檔名一定列出來——設定檔存的是它，出問題時要對得回去。
fn details(info: &ime_core::pack::Info) -> String {
    let mut lines: Vec<String> = Vec::new();
    if let Some(d) = &info.meta.description {
        lines.push(d.clone());
        lines.push(String::new());
    }
    lines.push(format!("檔名：{}.txt", info.file));
    lines.push(format!("內容：{}", breakdown(info)));
    for (label, value) in [
        ("作者", &info.meta.author),
        ("授權", &info.meta.license),
        ("更新", &info.meta.updated),
        ("網址", &info.meta.homepage),
    ] {
        if let Some(v) = value {
            lines.push(format!("{label}：{v}"));
        }
    }
    lines.join(
        "
",
    )
}

/// 一個包的語言分佈，寫成「英 62・日 38・中 12」。
///
/// 零的那一項不顯示——大部分包只有一兩種語言，把零列出來只是雜訊。
fn breakdown(info: &ime_core::pack::Info) -> String {
    let mut parts: Vec<String> = Vec::new();
    if info.en > 0 {
        parts.push(format!("英 {}", info.en));
    }
    if info.ja > 0 {
        parts.push(format!("日 {}", info.ja));
    }
    if info.zh > 0 {
        parts.push(format!("中 {}", info.zh));
    }
    parts.join("・")
}

/// 擴充包分頁。
///
/// # 為什麼順序不能調
///
/// 清單依檔名排序，衝突時誰贏就照這個順序。使用者定的（2026-09-01）：
/// 先不做上下移動，包不多的時候夠用，真的撞到衝突再說。
/// 選字分頁：選字的行為 ＋ 智慧學習。
///
/// **獨立成一頁而不是掛在「行為」底下**——學習是選字的直接結果
/// （選過兩次就記住），兩者放一起使用者才找得到「我學錯了怎麼辦」。
fn select_page(
    ui: &mut egui::Ui,
    cfg: &mut Config,
    stats: &mut Option<(Option<std::time::SystemTime>, (usize, usize))>,
) {
    ui.add_space(8.0);
    ui.heading("選字");
    ui.add_space(6.0);

    ui.label("選字時按 Enter：");
    ui.radio_value(
        &mut cfg.behavior.enter_in_select,
        EnterInSelect::Next,
        "選中反白的字，然後移到下一個字",
    );
    ui.radio_value(
        &mut cfg.behavior.enter_in_select,
        EnterInSelect::Exit,
        "選中反白的字，然後離開選字",
    );

    ui.add_space(12.0);
    let last_enabled = cfg.behavior.enter_in_select == EnterInSelect::Next;
    ui.add_enabled_ui(last_enabled, |ui| {
        ui.checkbox(
            &mut cfg.behavior.commit_on_last,
            "最後一個字選完直接送出（不然只離開選字，要再按一次 Enter）",
        );
    });
    if !last_enabled {
        ui.label(egui::RichText::new("（上面選「離開選字」時這項無效）").weak());
    }

    ui.add_space(12.0);
    ui.checkbox(
        &mut cfg.behavior.backspace_whole_cell,
        "鎖定語言時，倒退鍵刪掉整個反白的字",
    );
    ui.label(
        egui::RichText::new(
            "關掉的話回到原本的行為（刪掉尾端的一個音節）。自動模式不受影響——那時的一格未必對應一個字。",
        )
        .weak(),
    );

    ui.add_space(12.0);
    ui.label("鎖定注音時，這五個鍵（, . ; / -）：");
    ui.label(
        egui::RichText::new(
            "它們在注音鍵盤上是 ㄝㄡㄤㄥㄦ，一鍵兩用。判斷規則跟自動模式同一條：接了聲調就是注音，否則構不成字，那就是標點。",
        )
        .weak(),
    );
    ui.add_space(4.0);
    ui.radio_value(
        &mut cfg.behavior.lock_punct,
        ime_core::config::LockPunct::Auto,
        "自動判斷（打得出逗號句號，也打得出「二」「歐」）",
    );
    ui.radio_value(
        &mut cfg.behavior.lock_punct,
        ime_core::config::LockPunct::Symbol,
        "一律當注音符號（要打標點就切回自動模式）",
    );

    ui.add_space(8.0);
    ui.checkbox(
        &mut cfg.behavior.ctrl_punct,
        "用 Ctrl + 那個鍵可以明講「我要標點」",
    );
    ui.label(
        egui::RichText::new(
            "代價是那些組合在鎖定注音時到不了程式本身——Ctrl+- （瀏覽器縮小）和 Ctrl+/ （編輯器註解）會失效。會用到的話就關掉它。",
        )
        .weak(),
    );

    ui.add_space(18.0);
    ui.heading("智慧學習");
    ui.add_space(6.0);
    let (learned, watching) = learn_stats(stats);
    ui.label(
        egui::RichText::new(
            "同一個讀音選過兩次的字會自動記住，之後就直接給你那個字。記錄存在 learned.txt，跟設定檔同一個資料夾。",
        )
        .weak(),
    );
    ui.add_space(6.0);
    ui.label(format!(
        "目前記住 {learned} 條，另有 {watching} 條還在觀察（選過一次，還沒生效）。"
    ));
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button("開啟資料夾").clicked() {
            // 資料夾可能還不存在——先建再開，不然總管會說找不到。
            // 做法跟「開啟主題資料夾」一致。
            if let Some(d) =
                ime_core::learn::path(None).and_then(|p| p.parent().map(|d| d.to_path_buf()))
            {
                let _ = std::fs::create_dir_all(&d);
                let _ = std::process::Command::new("explorer").arg(d).spawn();
            }
        }
    });
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(
            "學錯了就把 learned.txt 裡那一行刪掉——不必重開程式，打下一個字就重讀了。三欄依序是：按鍵、文字、選過幾次。",
        )
        .weak(),
    );
}

/// 學習檔的條數，**檔案沒變就用快取**。
///
/// 只 stat 一次（拿修改時間）比整份讀進來便宜得多，而 egui 每一幀都會
/// 走到這裡。時間變了才重讀——使用者刪掉一行、切回這一頁就會更新。
fn learn_stats(
    cache: &mut Option<(Option<std::time::SystemTime>, (usize, usize))>,
) -> (usize, usize) {
    let stamp = ime_core::learn::path(None)
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok());
    match cache {
        Some((s, v)) if *s == stamp => *v,
        _ => {
            let v = ime_core::learn::stats(None);
            *cache = Some((stamp, v));
            v
        }
    }
}

fn packs_page(ui: &mut egui::Ui, cfg: &mut Config, cache: &mut Option<Vec<ime_core::pack::Info>>) {
    ui.add_space(8.0);
    ui.heading("擴充包");
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(
            "擴充包是你自己加的詞表——遊戲名、專有名詞、常打的英文詞。加進來之後不只選字會有，連「這串按鍵是不是英文」的判斷都會跟著改。",
        )
        .weak(),
    );

    ui.add_space(12.0);

    // ── 資料夾 ──
    //
    // 路徑做成可編輯的欄位：留空是預設位置，填了就用填的。
    // **不再有隱形的後備位置**——畫面上寫什麼就是什麼。
    let target = ime_core::pack::resolved_dir(&cfg.behavior.packs_dir);
    let exists = target.as_ref().is_some_and(|p| p.is_dir());
    let 預設 = ime_core::pack::resolved_dir("")
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    ui.horizontal(|ui| {
        ui.label("資料夾：");
        let w = ui.available_width() - 200.0;
        let r = ui.add_sized(
            [w.max(160.0), 22.0],
            egui::TextEdit::singleline(&mut cfg.behavior.packs_dir).hint_text(&預設),
        );
        if r.changed() {
            // 路徑變了，清單要重掃
            *cache = None;
        }
        if ui.button("瀏覽…").clicked() {
            if let Some(p) = pick_folder(target.as_deref()) {
                cfg.behavior.packs_dir = p;
                *cache = None;
            }
        }
    });

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        match &target {
            // 路徑已經在上面的欄位裡了，這一行只講狀態，不再印一次
            Some(p) if exists => {
                if ui.button("開啟資料夾").clicked() {
                    let _ = std::process::Command::new("explorer").arg(p).spawn();
                }
            }
            Some(p) => {
                ui.colored_label(
                    egui::Color32::from_rgb(0xC6, 0x28, 0x28),
                    "這個資料夾還不存在",
                );
                let p = p.clone();
                if ui.button("建立").clicked() && std::fs::create_dir_all(&p).is_ok() {
                    *cache = None;
                }
            }
            None => {
                ui.colored_label(
                    egui::Color32::from_rgb(0xC6, 0x28, 0x28),
                    "找不到可用的位置（%APPDATA% 讀不到？）",
                );
            }
        }
        if !cfg.behavior.packs_dir.trim().is_empty() && ui.button("用預設位置").clicked() {
            cfg.behavior.packs_dir.clear();
            *cache = None;
        }
        if ui.button("重新整理").clicked() {
            *cache = None;
        }
    });

    ui.add_space(12.0);

    // 掃一次存起來——egui 每一幀都重畫，不快取等於每秒讀幾十次磁碟
    let list = cache.get_or_insert_with(|| {
        let mut v: Vec<ime_core::pack::Info> = ime_core::pack::available(&cfg.behavior.packs_dir)
            .into_iter()
            .map(|f| ime_core::pack::info(&cfg.behavior.packs_dir, &f))
            .collect();
        // **依顯示名排序，不是檔名**——使用者看到的是包名，照檔名排
        // 會看起來像沒排序（`devterms.txt` 顯示成「程式術語」）。
        v.sort_by(|a, b| a.title().cmp(b.title()));
        v
    });

    // 設定裡啟用了、但檔案不見的包。也排進清單裡（標紅），
    // 不然使用者會困惑「明明開了卻沒作用」
    let missing: Vec<String> = cfg
        .behavior
        .packs
        .iter()
        .filter(|n| !list.iter().any(|i| &i.file == *n))
        .cloned()
        .collect();

    if list.is_empty() && missing.is_empty() {
        ui.label("這個資料夾裡還沒有任何包。");
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("放一個 .txt 進去，按上面的「重新整理」就會出現在這裡。").weak(),
        );
    } else {
        // **用 Grid 而不是一行一個 horizontal**：欄位要對齊，
        // 名字長短不一的時候「詞彙數」那欄才不會參差不齊。
        // `striped` 讓相鄰的列有淡淡的底色，行數多也掃得下去。
        egui::Grid::new("pack_list")
            .num_columns(5)
            .striped(true)
            .spacing([16.0, 8.0])
            .show(ui, |ui| {
                ui.label(egui::RichText::new("啟用").weak());
                ui.label(egui::RichText::new("名稱").weak());
                ui.label(egui::RichText::new("詞彙數").weak());
                ui.label(egui::RichText::new("內容").weak());
                // 補一格空的撐滿寬度——隔行底色只塗到最後一格為止，
                // 不撐滿的話色帶會斷在半路，看起來不像一整列
                fill(ui);
                ui.end_row();

                let mut toggled = false;
                for info in list.iter() {
                    let mut on = cfg.behavior.packs.iter().any(|p| p == &info.file);
                    let total = info.total();
                    // 空包不給勾——勾了也沒有任何作用，讓它可勾只會讓人
                    // 以為壞掉。停在這裡比讓使用者去猜好。
                    ui.add_enabled_ui(total > 0, |ui| {
                        if ui.checkbox(&mut on, "").changed() {
                            if on {
                                cfg.behavior.packs.push(info.file.clone());
                            } else {
                                cfg.behavior.packs.retain(|p| p != &info.file);
                            }
                            toggled = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        // 顯示名來自檔頭的 `# name:`，沒寫就是檔名
                        ui.label(info.title()).on_hover_text(details(info));
                        if let Some(v) = &info.meta.version {
                            ui.label(egui::RichText::new(format!("v{v}")).weak().small());
                        }
                    });
                    if total > 0 {
                        ui.label(format!("{total}"));
                        // 有寫說明就顯示說明——那比語言分佈更有用；
                        // 分佈滑到名字上就看得到
                        let text = info
                            .meta
                            .description
                            .clone()
                            .unwrap_or_else(|| breakdown(info));
                        ui.label(egui::RichText::new(text).weak())
                            .on_hover_text(details(info));
                    } else {
                        ui.label(egui::RichText::new("0").weak());
                        ui.label(
                            egui::RichText::new("還沒有詞（沒填，或格式不對）")
                                .weak()
                                .italics(),
                        );
                    }
                    fill(ui);
                    ui.end_row();
                }

                // **設定裡的順序就是衝突時的優先序**，所以要跟畫面上
                // 看到的順序一致。不排的話順序等於「勾選的先後」，
                // 那是使用者完全看不見的東西。
                if toggled {
                    let order: std::collections::HashMap<&str, usize> = list
                        .iter()
                        .enumerate()
                        .map(|(i, x)| (x.file.as_str(), i))
                        .collect();
                    cfg.behavior
                        .packs
                        .sort_by_key(|p| order.get(p.as_str()).copied().unwrap_or(usize::MAX));
                }

                for name in &missing {
                    ui.label("");
                    ui.label(egui::RichText::new(name).strikethrough().weak());
                    ui.label(egui::RichText::new("—").weak());
                    ui.horizontal(|ui| {
                        ui.colored_label(egui::Color32::from_rgb(0xC6, 0x28, 0x28), "找不到檔案");
                        if ui.small_button("從清單移除").clicked() {
                            cfg.behavior.packs.retain(|n| n != name);
                        }
                    });
                    fill(ui);
                    ui.end_row();
                }
            });

        ui.add_space(8.0);
        let on = cfg.behavior.packs.len() - missing.len();
        let words: usize = list
            .iter()
            .filter(|i| cfg.behavior.packs.iter().any(|p| p == &i.file))
            .map(|i| i.total())
            .sum();
        ui.label(egui::RichText::new(format!("已啟用 {on} 個包，共 {words} 個詞")).weak());
    }

    ui.add_space(18.0);
    ui.collapsing("檔案格式", |ui| {
        ui.label("檔案開頭可以寫這個包的基本資料（都可以省略，沒寫就用檔名當名稱）：");
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(
                "# name: Hololive 詞庫
# version: 1.2
# author: 你的名字
# description: VTuber 名字與常見的梗
# license: CC0
# updated: 2026-09-01",
            )
            .monospace(),
        );
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new("只認開頭那一段連續註解，遇到第一行詞就停。啟用狀態記的是檔名，所以改名稱不會讓已啟用的包失效。")
                .weak(),
        );

        ui.add_space(12.0);
        ui.label("接下來是詞，每行三欄用 Tab 分隔：語言、輸入、輸出。");
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(
                "en	hololive
ja	ほろらいぶ	ホロライブ
zh	ㄏㄨˊㄊㄠˊ	胡桃",
            )
            .monospace(),
        );
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(
                "英文是原樣送出，第三欄可以省略。中文那欄寫注音符號不是按鍵，手動維護時看得懂。# 開頭的是註解。",
            )
            .weak(),
        );
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(
                "同一個包裡三種語言可以混著寫——「hololive」「ほろらいぶ」「ㄏㄨˊㄊㄠˊ」是三種不同的輸入方式，各自要登記。",
            )
            .weak(),
        );
    });
}

fn appearance_page(ui: &mut egui::Ui, cfg: &mut Config) {
    ui.add_space(4.0);
    // **展示區放最上面**——調下面的控制項時它一直看得見
    preview(ui, cfg);
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(6.0);
    theme_section(ui, cfg);
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(6.0);

    // **兩欄並排**：顏色 11 項放左邊，字型 4 項 + 尺寸 6 項放右邊。
    // 單欄排下來會超出一個畫面，得捲動才看得到——調配色時看不到
    // 上面的展示區，等於白做。
    ui.columns(2, |cols| {
        colors_section(&mut cols[0], &mut cfg.colors);
        outline_section(&mut cols[0], &mut cfg.background);
        let ui = &mut cols[1];
        font_section(ui, &mut cfg.font);
        ui.add_space(10.0);
        metrics_section(ui, &mut cfg.metrics);
        ui.add_space(10.0);
        background_section(ui, &mut cfg.background);
    });
}

/// 候選視窗的背景圖。
fn background_section(ui: &mut egui::Ui, bg: &mut ime_core::config::Background) {
    ui.heading("背景圖");
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui.button("選擇圖片…").clicked() {
            if let Some(p) = crate::image_dialog::choose() {
                bg.image = p;
            }
        }
        if ui.button("清除").clicked() {
            bg.image.clear();
        }
    });
    ui.add_space(2.0);
    if bg.enabled() {
        // 路徑可能很長，用 tooltip 顯示完整的
        let short = std::path::Path::new(&bg.image)
            .file_name()
            .map(|x| x.to_string_lossy().to_string())
            .unwrap_or_else(|| bg.image.clone());
        ui.label(egui::RichText::new(short).weak())
            .on_hover_text(&bg.image);
        ui.add_space(4.0);
        ui.add(
            egui::Slider::new(&mut bg.strength, 0.0..=1.0)
                .text("濃度")
                .fixed_decimals(2),
        );
        ui.label(
            egui::RichText::new(
                "0 = 只有底色，1 = 只有圖片。圖片之上會蓋一層底色壓住對比，不然字會看不清。",
            )
            .weak(),
        );
    } else {
        ui.label(egui::RichText::new("沒有設定圖片，用純色背景。").weak());
    }
}

/// 文字描邊。
///
/// **放在顏色旁邊而不是背景圖裡**：描邊決定的是「字讀不讀得到」，
/// 跟配色是同一件事，而且純色背景想描邊也可以——它不依賴背景圖。
fn outline_section(ui: &mut egui::Ui, bg: &mut ime_core::config::Background) {
    ui.add_space(10.0);
    ui.heading("文字描邊");
    ui.add_space(4.0);
    ui.add(
        egui::Slider::new(&mut bg.text_outline, 0.0..=1.0)
            .text("濃度")
            .fixed_decimals(2),
    );
    ui.label(egui::RichText::new("在字的四周描一圈黑邊，背景再花也讀得到。0 = 不描邊。").weak());
}

/// 主題：套用內建或自訂的配色。
///
/// **主題只是一組顏色**——套用之後還是可以繼續微調個別欄位，
/// 也不會動到行為設定。
/// 目前套用的是哪個主題？**用反查的**。
///
/// 設定檔只存顏色、不存主題名，所以拿目前的配色去比對所有主題：
/// 完全相同就是那個主題，底下任何一格顏色被動過就對不上，回「自訂」。
///
/// 為什麼不在設定檔多存一份主題名：那份得跟著顏色一起維護，使用者
/// 改了顏色而它沒更新，畫面就會說謊。反查沒有這個同步問題。
fn current_theme_name(
    colors: &Colors,
    builtin: &[ime_core::theme_preset::Theme],
    custom: &[ime_core::theme_preset::Theme],
) -> String {
    builtin
        .iter()
        .chain(custom.iter())
        .find(|t| &t.colors == colors)
        .map(|t| t.name.clone())
        .unwrap_or_else(|| "自訂".to_string())
}

fn theme_section(ui: &mut egui::Ui, cfg: &mut Config) {
    ui.heading("主題");
    ui.add_space(4.0);

    // 內建 + 使用者放在主題資料夾裡的，併成一份清單餵給下拉選單。
    // 主題一多，平鋪按鈕會塞滿整排——下拉選單不管幾個都是一格。
    let dir = ime_core::theme_preset::themes_dir();
    let custom = dir
        .as_deref()
        .map(ime_core::theme_preset::load_dir)
        .unwrap_or_default();

    let builtin = ime_core::theme_preset::builtin();
    let current = current_theme_name(&cfg.colors, &builtin, &custom);

    ui.horizontal(|ui| {
        ui.label("目前主題：");
        egui::ComboBox::from_id_salt("theme_picker")
            .selected_text(&current)
            .width(220.0)
            .show_ui(ui, |ui| {
                for t in &builtin {
                    if ui.selectable_label(current == t.name, &t.name).clicked() {
                        cfg.colors = t.colors.clone();
                        // 描邊也是主題的一部分，見 `Theme::text_outline`
                        cfg.background.text_outline = t.text_outline;
                    }
                }
                if !custom.is_empty() {
                    ui.separator();
                    ui.label(egui::RichText::new("主題資料夾").weak());
                    for t in &custom {
                        if ui.selectable_label(current == t.name, &t.name).clicked() {
                            cfg.colors = t.colors.clone();
                            cfg.background.text_outline = t.text_outline;
                        }
                    }
                }
            });
    });
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        if ui.button("把目前配色存成主題").clicked() {
            if let Some(d) = &dir {
                let t = ime_core::theme_preset::Theme {
                    name: format!("我的主題 {}", chrono_stamp()),
                    colors: cfg.colors.clone(),
                    // 連描邊一起存——不然在別台電腦套用會少一半
                    text_outline: cfg.background.text_outline,
                };
                let _ = ime_core::theme_preset::save(&t, d);
            }
        }
        if ui.button("開啟主題資料夾").clicked() {
            if let Some(d) = &dir {
                // 資料夾可能還不存在——先建再開，不然總管會說找不到
                let _ = std::fs::create_dir_all(d);
                let _ = std::process::Command::new("explorer").arg(d).spawn();
            }
        }
    });
    ui.label(
        egui::RichText::new("主題檔是 toml，可以直接從 config.toml 的 [colors] 那段複製貼上。")
            .weak(),
    );
}

/// 給主題檔名用的時間戳（不引入 chrono，用系統時間湊一個就好）。
fn chrono_stamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}", secs % 100000)
}

fn colors_section(ui: &mut egui::Ui, c: &mut Colors) {
    ui.heading("顏色");
    ui.label(egui::RichText::new("列的是「用途」不是顏色").weak());
    ui.add_space(4.0);
    // **三欄不是兩欄**：中間那欄專門放漸層的開關。
    //
    // 開關跟色塊擠在同一格的話，有開關的那幾列色塊會被往右推，
    // 整排色塊就參差不齊——顏色本來就要並排比較才看得出差別。
    egui::Grid::new("colors")
        .num_columns(3)
        .spacing([6.0, 4.0])
        .show(ui, |ui| {
            color_row(ui, "視窗底色", &mut c.window_bg);
            gradient_row(ui, "　↳ 漸層下緣", &mut c.window_bg2, &c.window_bg);
            color_row(ui, "候選字", &mut c.text);
            // 編號已經跟候選字同色了，這個顏色現在只影響提示列與
            // 全半形提示視窗。**設定檔的鍵名不動**（還是 `index`），
            // 改了會讓使用者既有的設定失效。
            color_row(ui, "提示文字", &mut c.index);
            color_row(ui, "反白底", &mut c.highlight_bg);
            color_row(ui, "反白文字", &mut c.highlight_text);
            color_row(ui, "預覽列文字", &mut c.preview_text);
            color_row(ui, "預覽列底色", &mut c.preview_bg);
            gradient_row(ui, "　↳ 漸層下緣", &mut c.preview_bg2, &c.preview_bg);
            color_row(ui, "分隔線", &mut c.separator);
        });
}

fn font_section(ui: &mut egui::Ui, f: &mut Font) {
    ui.heading("字型");
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let shown = if f.family.is_empty() {
            "（系統預設）".to_string()
        } else {
            f.family.clone()
        };
        if ui.button(shown).clicked() {
            // 用系統的字型對話框——它自帶完整清單與預覽，
            // 比自己列幾百項再過濾符號字型省事得多。
            // 字級在對話框裡選的會被忽略，這裡只取字族。
            if let Some((name, _)) = font_dialog::choose(&f.family, 12) {
                f.family = name;
            }
        }
        if !f.family.is_empty() && ui.button("清除").clicked() {
            f.family.clear();
        }
    });
}

fn metrics_section(ui: &mut egui::Ui, m: &mut Metrics) {
    use ime_core::config::HighlightStyle as HS;
    ui.heading("反白樣式");
    ui.add_space(4.0);
    for (v, label, hint) in [
        (HS::Solid, "實心", "最清楚，看得最準"),
        (HS::Sheen, "高光帶", "色塊上緣加一道白光，像一片玻璃"),
        (HS::SheenOnly, "只有高光", "底色全透明，只剩光與邊框"),
    ] {
        ui.horizontal(|ui| {
            ui.radio_value(&mut m.highlight_style, v, label);
            ui.label(egui::RichText::new(hint).weak());
        });
    }
    ui.add_space(14.0);

    ui.heading("整體尺寸");
    ui.add_space(4.0);
    egui::ComboBox::from_id_salt("scale")
        .selected_text(format!("{}%", m.scale_percent))
        .show_ui(ui, |ui| {
            for v in ime_core::config::SCALE_STEPS {
                ui.selectable_value(&mut m.scale_percent, v, format!("{v}%"));
            }
        });
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new("字級、行高、內距、圓角全部等比縮放——版面比例由設計決定。").weak(),
    );
}

/// 一列顏色：色塊選擇器 + 十六進位字串。
///
/// 兩邊雙向同步——點色塊改的會寫回字串，直接編輯字串也會反映到色塊。
/// 漸層下緣色。**留空就是純色**——勾起來才會出現顏色選擇器。
///
/// 跟一般的 `color_row` 分開是因為「沒設定」是有意義的狀態：
/// 空字串代表不要漸層，而不是「顏色是黑的」。
fn gradient_row(ui: &mut egui::Ui, label: &str, hex: &mut String, base: &str) {
    ui.label(label);
    let mut on = !hex.is_empty();
    // 開關自己佔一欄，色塊才跟其他列對得齊
    if ui.checkbox(&mut on, "").changed() {
        // 打開時預設從基底色開始，使用者再去調
        *hex = if on { base.to_string() } else { String::new() };
    }
    ui.horizontal(|ui| {
        if on {
            let mut rgb = parse_hex(hex).unwrap_or([0, 0, 0]);
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                *hex = format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]);
            }
            ui.add(egui::TextEdit::singleline(hex).desired_width(72.0));
        } else {
            ui.label(egui::RichText::new("純色").weak());
        }
    });
    ui.end_row();
}

fn color_row(ui: &mut egui::Ui, label: &str, hex: &mut String) {
    ui.label(label);
    ui.label(""); // 佔住漸層開關那一欄，色塊才對得齊
    ui.horizontal(|ui| {
        let mut rgb = parse_hex(hex).unwrap_or([0, 0, 0]);
        if ui.color_edit_button_srgb(&mut rgb).changed() {
            *hex = format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]);
        }
        let bad = parse_hex(hex).is_none();
        let edit = egui::TextEdit::singleline(hex).desired_width(72.0);
        // 格式錯就把框染紅，不另外佔一格空間放「格式錯」三個字
        if bad {
            ui.style_mut().visuals.extreme_bg_color = egui::Color32::from_rgb(0x3A, 0x1A, 0x1A);
        }
        ui.add(edit);
    });
    ui.end_row();
}

/// 資料檔的 `data/` 在哪。
///
/// **兩種佈局都要支援**——安裝後 `data/` 就在 exe 旁邊，開發時 exe 在
/// `target/release/` 而 `data/` 在專案根。跟 `registration::data_dir()`
/// 是同一件事，只是這裡沒有 DLL 可以反查，改用 `current_exe()`。
///
/// 找不到就回 `None`，那代表只讀使用者目錄的設定。
fn project_data_dir() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let here = exe.parent()?;

    let installed = here.join("data");
    if installed.is_dir() {
        return Some(installed);
    }

    let d = here.parent()?.parent()?.join("data");
    d.is_dir().then_some(d)
}

#[cfg(test)]
mod tests {
    /// **這一組是為了「換字型就閃退」那個 bug 寫的。**
    ///
    /// 光驗字型資料的結構還不夠——真正會炸的是 egui 拿去排版的那一刻。
    /// 這裡建一個沒有視窗的 egui 環境，把字型載進去、真的排一次版：
    /// 資料有問題的話這個測試會直接 panic，就像設定頁閃退那樣。
    mod 載字型不會炸 {
        use super::super::{install_cjk_font, PREVIEW_FAMILY};

        fn 跑一輪(family: &str) {
            let ctx = egui::Context::default();
            install_cjk_font(&ctx, family);
            // 真的排版一次——字型是這時候才被解析的
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.label(
                        egui::RichText::new("測試ABC123あア")
                            .family(egui::FontFamily::Name(PREVIEW_FAMILY.into())),
                    );
                });
            });
        }

        #[test]
        fn 微軟正黑體() {
            跑一輪("Microsoft JhengHei");
        }

        #[test]
        fn 細明體() {
            跑一輪("MingLiU");
        }

        #[test]
        fn 新細明體() {
            跑一輪("PMingLiU");
        }

        #[test]
        fn 標楷體() {
            跑一輪("DFKai-SB");
        }

        #[test]
        fn 英文字型() {
            跑一輪("Segoe UI");
        }

        #[test]
        fn 不存在的字型也不能炸() {
            跑一輪("這個字型不存在12345");
        }
    }

    use super::*;

    /// 反查主題名：動過顏色就該顯示「自訂」，沒動過就顯示原本的名字。
    mod 主題反查 {
        use super::*;

        #[test]
        fn 沒動過顏色時顯示主題名() {
            let builtin = ime_core::theme_preset::builtin();
            assert!(!builtin.is_empty(), "應該至少有一個內建主題");
            for t in &builtin {
                let name = current_theme_name(&t.colors, &builtin, &[]);
                assert_eq!(name, t.name, "剛套用「{}」時該顯示它自己", t.name);
            }
        }

        #[test]
        fn 動過任何一格就變自訂() {
            let builtin = ime_core::theme_preset::builtin();
            let mut c = builtin[0].colors.clone();
            c.text = "#123456".to_string();
            assert_eq!(current_theme_name(&c, &builtin, &[]), "自訂");
        }

        #[test]
        fn 主題資料夾裡的也查得到() {
            let builtin = ime_core::theme_preset::builtin();
            let mine = ime_core::theme_preset::Theme {
                name: "我的配色".to_string(),
                colors: {
                    let mut c = builtin[0].colors.clone();
                    c.text = "#ABCDEF".to_string();
                    c
                },
                text_outline: 0.0,
            };
            let custom = vec![mine.clone()];
            assert_eq!(
                current_theme_name(&mine.colors, &builtin, &custom),
                "我的配色"
            );
        }

        #[test]
        fn 內建主題彼此不同() {
            // 兩個主題配色完全一樣的話反查會指到錯的那個
            let builtin = ime_core::theme_preset::builtin();
            for (i, a) in builtin.iter().enumerate() {
                for b in builtin.iter().skip(i + 1) {
                    assert_ne!(a.colors, b.colors, "「{}」與「{}」配色重複", a.name, b.name);
                }
            }
        }
    }
}
