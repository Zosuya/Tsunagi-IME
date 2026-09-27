//! 產生網站開頭「打字示範」要播的資料：`site/demo.json`。
//!
//! 每一句逐鍵走 `Session::push`，記下每一步的每一格（字＋語言），
//! 網頁照著播——所以畫面上「す 先出來、按下 3 才翻成 你」這種中途
//! 狀態都是引擎真的算出來的，不是手寫的動畫。
//!
//! **不載擴充包、不載學習層**：網站要呈現的是剛裝好的預設行為，
//! 不能被開發機上的個人設定帶偏。
//!
//! 引擎改了排序或選字之後重跑一次：
//!
//! ```text
//! cargo run --release -p ime-core --bin gen_site_demo
//! ```

use ime_core::language::Language;
use ime_core::session::Session;

/// 示範的句子。第一句最先播，所以放最能說明「混著打」的那句。
const DEMOS: &[&str] = &[
    "ul41j6ul4fm4t lunch",
    "rup wu0 g4iitennkikara,ul41j6ul4tj fm4play?",
    "su3cl3",
    "watashihagakuseidesu",
];

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let data = root.join("data");

    // 載入順序照 show_ime：包要先於詞庫。這裡只給內建符號包、不給任何使用者的包
    ime_core::pack::set_bundled_dir(Some(root.join("packs")));
    let cfg = ime_core::config::Config::load(Some(&data));
    ime_core::pack::load(&cfg.behavior.packs_dir, &[]);
    ime_core::english::load(&data);
    ime_core::dict::load_bopomofo(&data);
    ime_core::lm::load(&data, ime_core::dict::char_freq_map(&data));
    ime_core::dict::load_japanese(&data);
    ime_core::dict::load_connection(&data);

    let mut demos = Vec::new();
    for keys in DEMOS {
        let mut s = Session::new();
        let mut steps = Vec::new();
        for c in keys.chars() {
            s.push(c);
            let cells: Vec<String> = s
                .slots()
                .iter()
                .map(|slot| {
                    let lang = if slot.is_mark {
                        "p"
                    } else {
                        match slot.lang {
                            Language::Bopomofo => "zh",
                            Language::Romaji => "ja",
                            Language::English => "en",
                        }
                    };
                    format!("[{},\"{lang}\"]", json_str(&slot.text))
                })
                .collect();
            steps.push(format!("[{}]", cells.join(",")));
        }
        println!("{keys}  →  {}", s.text());
        demos.push(format!(
            "{{\"keys\":{},\"steps\":[\n{}\n]}}",
            json_str(keys),
            steps.join(",\n")
        ));
    }

    let out = root.join("site").join("demo.json");
    std::fs::write(&out, format!("[\n{}\n]\n", demos.join(",\n"))).expect("寫不進 site/demo.json");
    println!("已寫入 {}", out.display());
}
