//! 「精簡版」（§2.64.8 待決事項 1 的 (b)）到底要動多少？
//!
//! # 這支在問什麼
//!
//! §2.64.5 量過「帶走 80%、可丟 20%」，但那是**行數**的帳。真正該問的是
//! **執行期**：關掉日文之後，引擎還走不走那些程式碼？如果現成的
//! `config::Engines` 就辦得到，(b) 的「編譯開關」其實只剩包裝問題，
//! 不是架構問題。
//!
//! 三種組態各打同一批按鍵，比對結果：
//!
//! | 組態 | 怎麼來的 | 代表什麼 |
//! |---|---|---|
//! | 全開 | `Engines::default()` | 現在的通譯 |
//! | 關日文 | `Engines { romaji: false, .. }` | 執行期的「精簡版」 |
//! | 鎖注音 | `Input::for_lock(Some(Bopomofo))` | §2.8 那條獨立的單注音路徑 |
//!
//! 用法：`cargo run --release -p ime-core --bin spike_taigi_slim`

use ime_core::config::Engines;
use ime_core::input::Input;
use ime_core::language::Language;

fn data_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("core 的上層")
        .join("data")
}

/// 把一串按鍵餵進某個組態的 `Input`，回傳切出來的段落描述。
fn run(keys: &str, lock: Option<Language>, engines: Engines) -> String {
    let mut input = Input::with_engines(lock, engines);
    for ch in keys.chars() {
        input.push(ch, lock);
    }
    let cuts = input.cuttings();
    let Some(first) = cuts.first() else {
        return "（無）".to_string();
    };
    first
        .iter()
        .map(|s| {
            let l = match s.lang {
                Language::Bopomofo => "注",
                Language::Romaji => "日",
                Language::English => "英",
            };
            format!("{l}:{}", s.keys)
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn main() {
    let dir = data_dir();
    ime_core::pack::set_bundled_dir(dir.parent().map(|d| d.join("packs")));
    ime_core::pack::load(
        "__不存在的資料夾__",
        &[ime_core::pack::BUNDLED_SYMBOLS.to_string()],
    );
    ime_core::english::load(&dir);
    ime_core::dict::load_bopomofo(&dir);
    ime_core::lm::load(&dir, ime_core::dict::char_freq_map(&dir));
    ime_core::dict::load_japanese(&dir);

    let full = Engines::default();
    let no_ja = Engines {
        bopomofo: true,
        romaji: false,
    };

    // 按鍵一律 `mkkeys` 產。前兩組是中文，第三組故意是日文羅馬字
    // （`sushi`）——精簡版該把它當英文 passthrough，不該變成すし
    let cases: &[(&str, &str)] = &[
        ("su3cl3", "你好"),
        ("rup wu0 ", "今天"),
        ("sushi", "（日文，精簡版應退成英文）"),
        ("vu,4vu,4", "謝謝"),
    ];

    println!("{:<12} {:<26} {:<26} 鎖注音", "按鍵", "全開", "關日文");
    println!("{}", "─".repeat(96));
    for (keys, note) in cases {
        println!(
            "{:<12} {:<26} {:<26} {}",
            keys,
            run(keys, None, full),
            run(keys, None, no_ja),
            run(keys, Some(Language::Bopomofo), full),
        );
        println!("             （{note}）");
    }
}
