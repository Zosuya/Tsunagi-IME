//! **spike：把包裡的每一條真的「打」一遍**，看組出來的字對不對。
//!
//! `spike_kana_roundtrip` 只驗到「讀音拼得出假名」，那**不等於打得出
//! 名字**——引擎怎麼切這串按鍵才是關鍵。包裡的詞只有在「某一格剛好
//! 等於它」時才會被問到，切錯了就永遠出不來，而且**完全沒有錯誤訊息**。
//!
//! 這支走完整條真實路徑：讀音 →（反查）→ 按鍵 →（`Session` 逐鍵送）
//! → 組出來的文字，跟包裡寫的表記比對。
//!
//! 用法：`cargo run --release -p ime-core --bin spike_pack_type -- <包名>`
//! 包從設定檔的 `packs_dir` 找，**其餘啟用的包照設定一起載**——
//! `--pack` 那種取代式的清單會測到一個不存在的環境（踩過這個坑）。

use ime_core::session::Session;

fn main() {
    let name = match std::env::args().nth(1) {
        Some(n) => n,
        None => {
            eprintln!("用法: spike_pack_type <包名>");
            std::process::exit(2);
        }
    };

    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data");
    let cfg = ime_core::config::Config::load(Some(&data));
    // 要測的包接在設定檔清單後面——**其餘的包照載**，不然測到的行為
    // 跟使用者看到的不一樣
    let mut enabled = cfg.behavior.packs.clone();
    if !enabled.iter().any(|p| p == &name) {
        enabled.push(name.clone());
    }
    ime_core::pack::set_bundled_dir(data.parent().map(|d| d.join("packs")));
    ime_core::pack::load(&cfg.behavior.packs_dir, &enabled);
    ime_core::english::load(&data);
    ime_core::dict::load_bopomofo(&data);
    ime_core::lm::load(&data, ime_core::dict::char_freq_map(&data));
    ime_core::dict::load_japanese(&data);
    ime_core::dict::load_connection(&data);

    let path = std::path::Path::new(&cfg.behavior.packs_dir).join(format!("{name}.txt"));
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("讀不到 {}", path.display());
        std::process::exit(2);
    };

    let (mut ja_n, mut ja_ok) = (0usize, 0usize);
    let (mut en_n, mut en_ok) = (0usize, 0usize);
    let mut bad: Vec<(String, String, String, String)> = Vec::new();

    for line in text.lines() {
        let line = line.trim_start_matches('\u{feff}');
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let mut f = line.split('\t');
        let (Some(lang), Some(input)) = (f.next(), f.next()) else {
            continue;
        };
        let input = input.trim();
        let want = f.next().map(str::trim).unwrap_or(input);

        match lang.trim() {
            "ja" => {
                ja_n += 1;
                // 讀音要先變回按鍵才能打
                let Some(keys) = ime_core::reverse::keys_for(input) else {
                    bad.push((input.into(), "—".into(), "反查不出按鍵".into(), want.into()));
                    continue;
                };
                let got = type_it(&keys);
                if got == want {
                    ja_ok += 1;
                } else {
                    bad.push((input.into(), keys, got, want.into()));
                }
            }
            "en" => {
                en_n += 1;
                let got = type_it(input);
                if got == want {
                    en_ok += 1;
                } else {
                    bad.push((input.into(), input.into(), got, want.into()));
                }
            }
            _ => {}
        }
    }

    println!("ja {ja_n} 筆，打得出來 {ja_ok} 筆");
    println!("en {en_n} 筆，打得出來 {en_ok} 筆");
    if bad.is_empty() {
        println!("\n全部打得出來");
        return;
    }
    println!("\n打不出來的 {} 筆：", bad.len());
    println!("{:<26} {:<24} {:<22} 應該是", "輸入", "按鍵", "實際出來");
    for (i, k, got, want) in &bad {
        println!("{i:<26} {k:<24} {got:<22} {want}");
    }
    std::process::exit(1);
}

/// 逐鍵送進 `Session`，回傳組出來的文字——跟使用者真的打字同一條路。
fn type_it(keys: &str) -> String {
    let mut s = Session::new();
    for c in keys.chars() {
        s.push(c);
    }
    s.text()
}
