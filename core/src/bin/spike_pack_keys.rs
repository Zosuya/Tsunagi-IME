//! **spike：把包裡每個 `ja` 讀音的實際按鍵算出來**，讓外部工具比對。
//!
//! 要回答的問題是「英文名跟日文讀音打起來一不一樣」——一樣的話收
//! 英文名是白費（同一串按鍵），不一樣才需要另外收一條。
//!
//! 按鍵不能自己推：`し` 是 `si` 還是 `shi`、`ん` 是 `n` 還是 `nn`，
//! 規則在 `reverse` 裡，手推一定會漂。
//!
//! 用法：`cargo run --release -p ime-core --bin spike_pack_keys -- <包檔>`
//! 輸出 `讀音<TAB>按鍵<TAB>表記`，一行一條。

use ime_core::reverse;

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("用法: spike_pack_keys <包檔>");
        std::process::exit(2);
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("讀不到 {path}");
        std::process::exit(2);
    };

    for line in text.lines() {
        let line = line.trim_start_matches('\u{feff}');
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let mut f = line.split('\t');
        let (Some(lang), Some(input)) = (f.next(), f.next()) else {
            continue;
        };
        if lang.trim() != "ja" {
            continue;
        }
        let input = input.trim();
        let surface = f.next().map(str::trim).unwrap_or("");
        let keys = reverse::keys_for(input).unwrap_or_else(|| "—".into());
        println!("{input}\t{keys}\t{surface}");
    }
}
