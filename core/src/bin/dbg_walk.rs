//! 段選單逐段走過去，看每一段的候選——重現實測回報的問題。
//!
//! # 用法
//!
//! ```text
//! cargo run --release -p ime-core --bin dbg_walk -- "<按鍵串>" [走到第幾段]
//! ```
//!
//! 不給段號就一路走到底，每一段都印候選數；給了就停在那裡印完整清單。

#[path = "common/testdata.rs"]
mod testdata;

use ime_core::session::Session;

fn main() {
    testdata::load_engine();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(keys) = args.first() else {
        eprintln!("用法：dbg_walk <按鍵串> [走到第幾段]");
        return;
    };
    let stop: Option<usize> = args.get(1).and_then(|s| s.parse().ok());

    let mut s = Session::new();
    for c in keys.chars() {
        s.push(c);
    }

    let n = s.seg_count();
    println!("共 {n} 段\n");

    for i in 0..n {
        let segs = s.seg_segments();
        let Some(seg) = segs.get(s.seg_index()) else {
            break;
        };
        let cands = s.seg_cands();
        let here = stop == Some(i + 1);
        println!(
            "第{:>2}段 {:<14} {} 個候選{}",
            i + 1,
            format!("{:?}", seg.keys),
            cands.len(),
            if cands.is_empty() {
                "  ← 選不出東西！"
            } else {
                ""
            }
        );
        if here || cands.is_empty() {
            for (k, c) in cands.iter().enumerate() {
                println!(
                    "     {}. {:<14} {:<4} 按鍵 {:?}",
                    k + 1,
                    c.text,
                    lang(c.lang),
                    c.keys
                );
            }
            if here {
                return;
            }
        }
        let before = s.seg_index();
        s.seg_right();
        if s.seg_index() == before {
            break;
        }
    }
}

fn lang(l: ime_core::language::Language) -> &'static str {
    use ime_core::language::Language;
    match l {
        Language::Bopomofo => "注",
        Language::Romaji => "日",
        Language::English => "英",
    }
}
