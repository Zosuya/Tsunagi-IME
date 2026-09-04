//! 除錯分頁：不必切進輸入法就能看引擎怎麼判斷。
//!
//! # 為什麼需要這個
//!
//! 輸入法跑在宿主行程裡，沒有主控台可以印東西。要知道「這串按鍵
//! 被切成什麼、為什麼選了這個字」，以前得埋 log、重編 DLL、切輸入法、
//! 打字、再回來讀檔——一輪好幾分鐘。
//!
//! 這裡直接跑同一套 `ime_core::session`，打字就看得到結果。

use eframe::egui;
use ime_core::session::Session;

#[derive(Default)]
pub struct DebugState {
    /// 測試用的按鍵串
    input: String,
    /// 上次算過的輸入，用來判斷要不要重算
    last: String,
    /// 引擎狀態
    session: Session,
    /// 詞庫載入了沒
    loaded: bool,
}

impl DebugState {
    /// 重新用目前的輸入跑一次引擎。
    ///
    /// 累加式沒有「跳到某個狀態」的走法，只能整串重放——
    /// 按鍵串通常十幾個字元，成本可接受。
    fn recompute(&mut self, data_dir: Option<&std::path::Path>) {
        if !self.loaded {
            if let Some(d) = data_dir {
                // 除錯區要能試所有語言，這裡不看設定
                ime_core::preload(d, ime_core::config::Engines::default());
            }
            self.loaded = ime_core::dict::bopomofo_loaded();
        }
        self.session = Session::new();
        for c in self.input.chars() {
            self.session.push(c);
        }
        self.last = self.input.clone();
    }
}

pub fn debug_page(ui: &mut egui::Ui, st: &mut DebugState, data_dir: Option<&std::path::Path>) {
    ui.add_space(8.0);
    ui.heading("測試輸入");
    ui.label(
        egui::RichText::new("打按鍵序列（例如 su3cl3、sushi、check），看引擎怎麼切、選了什麼字")
            .small()
            .weak(),
    );
    ui.add_space(6.0);

    let resp = ui.add(
        egui::TextEdit::singleline(&mut st.input)
            .desired_width(f32::INFINITY)
            .hint_text("su3cl3"),
    );
    if resp.changed() || st.input != st.last {
        st.recompute(data_dir);
    }

    ui.add_space(4.0);
    if !st.loaded {
        ui.colored_label(
            egui::Color32::from_rgb(0xC6, 0x28, 0x28),
            "詞庫沒載入——結果只有切點，沒有文字（跑 data/download.ps1）",
        );
    }

    ui.add_space(10.0);
    ui.separator();
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        ui.heading("除錯資訊");
        ui.add_space(12.0);
        if ui.button("開啟 log 資料夾").clicked() {
            open_log_folder();
        }
        ui.label(egui::RichText::new(log_path_display()).small().weak());
    });
    ui.add_space(6.0);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if st.input.is_empty() {
                ui.label(egui::RichText::new("（還沒輸入）").weak());
                return;
            }
            info_grid(ui, st);
            ui.add_space(10.0);
            cuttings_list(ui, st);
        });
}

/// 一眼看得到的幾個數字。
fn info_grid(ui: &mut egui::Ui, st: &DebugState) {
    egui::Grid::new("dbg_info")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.label("按鍵串");
            ui.label(mono(st.session.keys()));
            ui.end_row();

            ui.label("輸出文字");
            ui.label(egui::RichText::new(st.session.text()).size(16.0).strong());
            ui.end_row();

            ui.label("切法數");
            ui.label(mono(&format!("{}", st.session.cutting_count())));
            ui.end_row();

            ui.label("選字格");
            let slots: Vec<String> = st
                .session
                .slots()
                .iter()
                .map(|s| format!("{}{}", s.text, if s.selectable { "" } else { "*" }))
                .collect();
            ui.label(mono(&slots.join(" │ ")));
            ui.end_row();
        });
    ui.label(
        egui::RichText::new("* 代表那一格不能選字（英文段沒有同音字問題）")
            .small()
            .weak(),
    );
}

/// 前幾種切法各是什麼。
fn cuttings_list(ui: &mut egui::Ui, st: &DebugState) {
    ui.label(egui::RichText::new("切法候選（前 10）").strong());
    ui.add_space(4.0);
    let menu = st.session.cutting_menu(10);
    if menu.is_empty() {
        ui.label(egui::RichText::new("（沒有切法）").weak());
        return;
    }
    egui::Grid::new("dbg_cuts")
        .num_columns(2)
        .spacing([10.0, 3.0])
        .striped(true)
        .show(ui, |ui| {
            for (i, t) in menu.iter().enumerate() {
                let mark = if i == st.session.cutting_index() {
                    "▶"
                } else {
                    " "
                };
                ui.label(mono(&format!("{mark} {}", i + 1)));
                ui.label(egui::RichText::new(t).size(14.0));
                ui.end_row();
            }
        });
}

fn mono(s: &str) -> egui::RichText {
    egui::RichText::new(s).monospace()
}

/// log 檔在 `%TEMP%\ime_debug.log`——跟 `platform/windows/src/debug_log.rs`
/// 寫的位置一致。
fn log_path() -> Option<std::path::PathBuf> {
    std::env::var_os("TEMP").map(|t| std::path::PathBuf::from(t).join("ime_debug.log"))
}

fn log_path_display() -> String {
    match log_path() {
        Some(p) => p.display().to_string(),
        None => "（找不到 %TEMP%）".into(),
    }
}

/// 開檔案總管並選中 log 檔。
///
/// 檔案還不存在時就只開資料夾——log 是「輸入法那邊出問題時才產生」的，
/// 平常不存在很正常，不該當成錯誤。
fn open_log_folder() {
    let Some(p) = log_path() else { return };
    let arg = if p.is_file() {
        format!("/select,{}", p.display())
    } else {
        match p.parent() {
            Some(d) => d.display().to_string(),
            None => return,
        }
    };
    let _ = std::process::Command::new("explorer.exe").arg(arg).spawn();
}
