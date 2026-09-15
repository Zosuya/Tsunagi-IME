//! Spike：egui 的文字框收不收得到「原始按鍵」。
//!
//! 擴充包編輯器要讓使用者在「按鍵」欄打 `cj6wl6`，但他的系統輸入法
//! 很可能就是通譯自己——那樣 `cj6` 會被輸入法吃掉組成「胡」，按鍵
//! 根本打不進去。這支就是要實測這件事。
//!
//! 畫面上並排顯示同一次輸入的三種來源：
//!
//! - **文字框的內容**：egui 認為使用者打了什麼
//! - **本幀的 `Event::Text`**：輸入法送進來的字（組好的字會出現在這）
//! - **本幀的 `Event::Key`**：實體按鍵（輸入法吃掉時這裡也可能沒有）
//!
//! 用法：`cargo run -p ime-settings --bin spike_keyfield`
//! 開起來之後**分別在「輸入法關著」與「輸入法開著」兩種情況下**
//! 打一次 `cj6wl6`，看三欄各收到什麼。

use eframe::egui;

#[derive(Default)]
struct App {
    /// 一般的文字框
    plain: String,
    /// 想當成「按鍵欄」的那個
    keys: String,
    /// 最近幾幀收到的事件，新的在最前面
    log: Vec<String>,
    /// 詞庫載到哪了（語言辨識靠它）
    dict_status: String,
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _f: &mut eframe::Frame) {
        // 先把本幀的原始事件抄一份——文字框吃掉之後就看不到了
        let mut line = Vec::new();
        ctx.input(|i| {
            for e in &i.events {
                match e {
                    egui::Event::Text(t) => line.push(format!("Text({t:?})")),
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => line.push(format!("Key({key:?}{})", mods(modifiers))),
                    egui::Event::Ime(im) => line.push(format!("Ime({im:?})")),
                    _ => {}
                }
            }
        });
        if !line.is_empty() {
            self.log.insert(0, line.join("  "));
            self.log.truncate(30);
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("按鍵欄 spike");
            ui.label("分別在「系統輸入法關著」與「開著」時打 cj6wl6，比對下面三處。");
            ui.label(egui::RichText::new(&self.dict_status).weak().small());
            ui.separator();

            ui.label("① 一般文字框（使用者打詞的地方）：");
            ui.text_edit_singleline(&mut self.plain);
            ui.add_space(8.0);

            ui.label("② 想當成按鍵欄的文字框（打 cj6wl6 看看）：");
            ui.text_edit_singleline(&mut self.keys);
            // 編輯器實際要顯示的三樣東西，一起在這裡驗
            ui.label(egui::RichText::new(format!("→ 注音：{}", to_bopomofo(&self.keys))).weak());
            let kana = ime_core::romaji::kana::to_kana(&self.keys)
                .unwrap_or_else(|| "（不是合法的羅馬字）".into());
            ui.label(egui::RichText::new(format!("→ 假名：{kana}")).weak());
            ui.label(egui::RichText::new(format!("→ 語言辨識：{}", detect(&self.keys))).weak());
            ui.add_space(8.0);

            ui.separator();
            ui.label("③ 本幀收到的原始事件（新的在上）：");
            egui::ScrollArea::vertical().show(ui, |ui| {
                for l in &self.log {
                    ui.label(egui::RichText::new(l).monospace());
                }
            });
        });
    }
}

fn mods(m: &egui::Modifiers) -> String {
    let mut s = String::new();
    if m.ctrl {
        s.push_str("+Ctrl");
    }
    if m.shift {
        s.push_str("+Shift");
    }
    if m.alt {
        s.push_str("+Alt");
    }
    s
}

/// 按鍵 → 注音符號，就是編輯器要顯示的那個轉換。
fn to_bopomofo(keys: &str) -> String {
    keys.chars()
        .map(|c| ime_core::bopomofo::keymap::symbol_of(c).unwrap_or(c))
        .collect()
}

/// 這串按鍵是什麼語言？**用引擎現成的那條路**——編輯器的「語言」欄
/// 就是這樣算出來的，不是使用者選的。
fn detect(keys: &str) -> String {
    if keys.trim().is_empty() {
        return "（還沒輸入）".into();
    }
    let input = ime_core::input::Input::from_keys(keys, None);
    let Some(best) = input.cuttings().first() else {
        return "切不出來".into();
    };
    best.iter()
        .map(|s| format!("{:?}:{}", s.lang, s.keys))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// 裝中文字型——不裝的話注音符號全是豆腐塊，這支 spike 就白開了。
/// 設定頁自己有一份完整的（含預覽字型），這裡只要最小的一份。
fn install_cjk(ctx: &egui::Context) {
    let candidates: &[(&str, u32)] = &[
        (r"C:\Windows\Fonts\msjh.ttc", 0),
        (r"C:\Windows\Fonts\mingliu.ttc", 0),
    ];
    let Some((bytes, face)) = candidates
        .iter()
        .find_map(|(p, i)| std::fs::read(p).ok().map(|b| (b, *i)))
    else {
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    let mut cjk = egui::FontData::from_owned(bytes);
    cjk.index = face;
    fonts
        .font_data
        .insert("cjk".to_owned(), std::sync::Arc::new(cjk));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "cjk".to_owned());
    }
    ctx.set_fonts(fonts);
}

/// 載詞庫。**語言辨識非它不可**——沒載的話英文詞典是空的，
/// 辨識結果全是假的。找不到就照實在畫面上說。
fn load_dicts() -> String {
    // 從執行檔往上找 `data\`（開發時是 target\release\ 往上兩層）
    let mut dir = std::env::current_dir().unwrap_or_default();
    for _ in 0..4 {
        let d = dir.join("data");
        if d.join("bopomofo").is_dir() {
            ime_core::preload(&d, ime_core::config::Engines::default());
            return format!("詞庫：{}", d.display());
        }
        if !dir.pop() {
            break;
        }
    }
    "詞庫：找不到 data\\，語言辨識的結果不可信".to_string()
}

fn main() -> eframe::Result<()> {
    let dict_status = load_dicts();
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([680.0, 620.0])
            .with_title("spike: 按鍵欄收得到什麼"),
        ..Default::default()
    };
    eframe::run_native(
        "spike_keyfield",
        opts,
        Box::new(move |cc| {
            install_cjk(&cc.egui_ctx);
            Ok(Box::new(App {
                dict_status,
                ..Default::default()
            }))
        }),
    )
}
