//! 終端機版輸入法：打一串按鍵，看切點引擎怎麼切。
//!
//! 這不是真的輸入法（沒有組字、沒有選字、不會送出文字），是**切點
//! 引擎的手動測試工具**——輸入按鍵序列，看候選切法與排序結果。
//!
//! 用法：
//!
//! ```text
//! cargo run --release -p ime-core --bin repl
//! ```
//!
//! 互動指令：
//!
//! | 輸入 | 意思 |
//! |---|---|
//! | 任何按鍵串 | 切它，列出前幾名候選 |
//! | `:n 20` | 改成列 20 個候選（預設 8） |
//! | `:step <按鍵串>` | 逐鍵展開，看候選怎麼累加 |
//! | `:s` | 開關「印出分數」——查排序問題非看不可 |
//! | `:q` | 離開 |
//!
//! 注音的一聲要打空白，所以按鍵串裡的空白有兩種意思——引擎自己會判。

use ime_core::cutpoint::{incremental::Incremental, normalize, rank, Segment};
use std::io::{self, BufRead, Write};

fn show(segs: &[Segment]) -> String {
    segs.iter()
        .map(|s| format!("{}:{}", s.lang.short(), s.keys.replace(' ', "␣")))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn main() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data");
    print!("載入詞庫…");
    io::stdout().flush().ok();
    ime_core::english::load(&data);
    ime_core::dict::load_bopomofo(&data);
    ime_core::dict::load_japanese(&data);
    println!(
        " 英文={} 注音={} 日文={}",
        ime_core::english::is_loaded(),
        ime_core::dict::bopomofo_loaded(),
        ime_core::dict::japanese_loaded()
    );
    println!();
    println!("  打一串按鍵看切法。注音的一聲打空白。");
    println!("  :n <數字>  改變列出的候選數（預設 8）");
    println!("  :step <按鍵串>  逐鍵展開");
    println!("  :q  離開");
    println!();
    println!("  例：su3cl3（你好）  sushi  check u vu84（check␣一下）");
    println!();

    let mut top = 8usize;
    // `:s` 切換要不要印分數——查排序問題非看不可
    let mut scores = false;
    let stdin = io::stdin();
    loop {
        print!("> ");
        io::stdout().flush().ok();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
            break; // EOF
        }
        // **只剝換行，不剝空白**——結尾的空白可能是注音的一聲
        let line = line.trim_end_matches(['\u{d}', '\u{a}']);
        if line.is_empty() {
            continue;
        }
        if line == ":q" {
            break;
        }
        if line == ":s" {
            scores = !scores;
            println!("  分數 {}", if scores { "開" } else { "關" });
            continue;
        }
        if let Some(n) = line.strip_prefix(":n ") {
            match n.trim().parse::<usize>() {
                Ok(v) if v > 0 => {
                    top = v;
                    println!("  候選數改為 {top}");
                }
                _ => println!("  用法：:n 20"),
            }
            continue;
        }
        if let Some(keys) = line.strip_prefix(":step ") {
            step(keys, top);
            continue;
        }
        cut(line, top, scores);
    }
    println!("bye");
}

/// 切一串按鍵，列出前 `top` 名候選。
fn cut(keys: &str, top: usize, scores: bool) {
    let inc = Incremental::from_keys(keys);
    let cands = rank::sort(inc.cuttings());
    if cands.is_empty() {
        println!("  （沒有合法切法）");
        return;
    }
    // 正規化後去重——同語言內部切不切不影響輸出，那些是同一個答案
    //
    // **分數要拿正規化前的那一份算**：排序看的是原始候選，顯示看的是
    // 正規化後的。查排序問題時混用兩者會得出完全錯誤的結論。
    let mut seen = std::collections::HashSet::new();
    let uniq: Vec<(Vec<Segment>, Vec<Segment>)> = cands
        .iter()
        .map(|c| (normalize(c), c.clone()))
        .filter(|(n, _)| seen.insert(show(n)))
        .collect();
    println!(
        "  相異輸出 {} 種（原始候選 {}），前 {}：",
        uniq.len(),
        cands.len(),
        top.min(uniq.len())
    );
    for (i, (n, raw)) in uniq.iter().take(top).enumerate() {
        let mark = if i == 0 { "★" } else { " " };
        println!("   {mark}{:2}. {}", i + 1, show(n));
        if scores {
            // 原始候選跟正規化後不同時一併印出來——看得到「顯示成同一個
            // 答案，但參與排序的是另一種切法」
            if show(raw) != show(n) {
                println!("        原始 {}", show(raw));
            }
            println!("        {:?}", rank::score(raw));
        }
    }
}

/// 逐鍵展開，看候選數怎麼變化。
fn step(keys: &str, top: usize) {
    let chars: Vec<char> = keys.chars().collect();
    let mut inc = Incremental::new();
    for (n, &c) in chars.iter().enumerate() {
        inc.push(c);
        let sofar: String = chars[..n + 1].iter().collect();
        let cands = rank::sort(inc.cuttings());
        let best = cands
            .first()
            .map(|c| show(&normalize(c)))
            .unwrap_or_default();
        println!(
            "  {:20} {:4} 種  第一名 {}",
            format!("{:?}", sofar.replace(' ', "␣")),
            cands.len(),
            best
        );
    }
    if top > 1 {
        println!();
        cut(keys, top, false);
    }
}
