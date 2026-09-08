//! 段選單的互動追蹤：一步一步看「選了就凍」發生什麼事。
//!
//! 實測前先用這支確認邏輯——比開記事本快，而且看得到內部狀態。
//!
//! # 用法
//!
//! ```text
//! cargo run --release -p ime-core --bin dbg_segmenu -- loggerul4
//! cargo run --release -p ime-core --bin dbg_segmenu -- loggerul4 1 0 2
//! ```
//!
//! 第一個參數是按鍵串，後面是「每一步要選第幾個候選」（0-based）。
//! 不給的話只印第一段的候選清單。

#[path = "common/testdata.rs"]
mod testdata;

use ime_core::session::Session;

fn main() {
    testdata::load_engine();
    testdata::load_packs_from_args();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(keys) = args.first() else {
        eprintln!("用法：dbg_segmenu <按鍵串> [每步選第幾個候選...]");
        return;
    };
    let picks: Vec<usize> = args[1..].iter().filter_map(|s| s.parse().ok()).collect();

    let mut s = Session::new();
    for c in keys.chars() {
        s.push(c);
    }

    println!("按鍵：{keys}");
    println!("引擎第一名：{}\n", show(&s));

    for (step, &pick) in picks.iter().enumerate() {
        let cands = s.seg_cands();
        println!(
            "── 第 {} 步：反白第 {} 段 {:?} ──",
            step + 1,
            s.seg_index() + 1,
            s.seg_segments()
                .get(s.seg_index())
                .map(|x| x.keys.clone())
                .unwrap_or_default()
        );
        for (i, c) in cands.iter().enumerate() {
            let mark = if i == pick { "→" } else { " " };
            println!(
                "  {mark}{}. {:<12} {:<4} 按鍵 {:?}",
                i + 1,
                c.text,
                lang_name(c.lang),
                c.keys
            );
        }
        if pick >= cands.len() {
            println!("  （沒有第 {} 個候選，停）", pick + 1);
            break;
        }
        s.seg_set_cand(pick);
        s.seg_confirm();
        println!("  選完 → {}", show(&s));
        println!("  送出的文字：{}\n", s.text());
    }

    if picks.is_empty() {
        println!("第 1 段的候選：");
        for (i, c) in s.seg_cands().iter().enumerate() {
            println!(
                "  {}. {:<12} {:<4} 按鍵 {:?}",
                i + 1,
                c.text,
                lang_name(c.lang),
                c.keys
            );
        }
    }
}

/// 印出目前的分段，已定案的用 `[]` 框起來。
fn show(s: &Session) -> String {
    let segs = s.seg_segments();
    segs.iter()
        .map(|x| format!("{}:{}", lang_name(x.lang), x.keys))
        .collect::<Vec<_>>()
        .join("｜")
}

fn lang_name(l: ime_core::language::Language) -> &'static str {
    use ime_core::language::Language;
    match l {
        Language::Bopomofo => "注",
        Language::Romaji => "日",
        Language::English => "英",
    }
}
