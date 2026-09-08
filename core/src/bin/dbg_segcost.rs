//! 段選單按下 Enter 到底慢在哪？
//!
//! `check_segmenu` 量到重算 p99 208ms（一幀預算 16ms 的 13 倍）。
//! 這支拆開來看成本從哪來——是算候選慢，還是重算慢，跟後區長度
//! 有什麼關係。
//!
//! # 用法
//!
//! ```text
//! cargo run --release -p ime-core --bin dbg_segcost
//! cargo run --release -p ime-core --bin dbg_segcost -- <按鍵串>
//! ```

#[path = "common/testdata.rs"]
mod testdata;

use ime_core::session::Session;
use std::time::Instant;

fn main() {
    testdata::load_engine();
    let keys = std::env::args().nth(1).unwrap_or_else(|| {
        "boring desu email commit oishii,data daijoubu hk4g4 kyoutest au/6wu0  meeting sushi sockete93".into()
    });

    // ── 對照組：打字的成本是怎麼分攤的 ──
    let mut s = Session::new();
    let t0 = Instant::now();
    let mut worst_key = std::time::Duration::ZERO;
    for c in keys.chars() {
        let t = Instant::now();
        s.push(c);
        worst_key = worst_key.max(t.elapsed());
    }
    let total_typing = t0.elapsed();
    println!(
        "打完 {} 鍵：總共 {:.1?}，最慢的一鍵 {:.2?}",
        keys.chars().count(),
        total_typing,
        worst_key
    );
    println!(
        "（使用者分 {} 次付這個成本，所以不覺得慢）\n",
        keys.chars().count()
    );
    println!("分成 {} 段\n", s.seg_count());

    println!("按 Enter 定案一段之後，剩下的要重算：");
    println!(
        "{:<4} {:<14} {:>9} │ {:>10} {:>12}",
        "步", "定案的段", "後區剩", "算候選", "定案+重算"
    );
    println!("{}", "─".repeat(58));

    for step in 0..8 {
        let cands = s.seg_cands();
        if cands.is_empty() {
            break;
        }
        let seg = s
            .seg_segments()
            .get(s.seg_index())
            .map(|x| x.keys.clone())
            .unwrap_or_default();

        // 算候選要多久？（每按一次方向鍵都要算一次）
        let t1 = Instant::now();
        let _ = s.seg_cands();
        let dt_cands = t1.elapsed();

        // 定案之後還剩幾鍵要重算？
        let rest: usize = s
            .seg_segments()
            .iter()
            .skip(s.seg_index() + 1)
            .map(|x| x.keys.chars().count())
            .sum();

        let t2 = Instant::now();
        s.seg_confirm();
        let dt_confirm = t2.elapsed();

        println!(
            "{:<4} {:<14} {:>7} 鍵 │ {:>10.1?} {:>12.1?}",
            step + 1,
            format!("{seg:?}"),
            rest,
            dt_cands,
            dt_confirm
        );
    }

    // **驗證「重算 = 累加一遍」**：如果兩者接近，代表重放本身沒有
    // 異常，成本純粹來自「不該重放這麼多」。
    println!("\n對照：從頭打同樣長度的按鍵要多久？");
    let n_all = keys.chars().count();
    for n in [68usize, 76, 82, 87] {
        if n > n_all {
            continue;
        }
        let sub: String = keys.chars().skip(n_all - n).collect();
        println!("  {n} 鍵：{:.1?}", replay_cost(&sub));
    }

    println!("\n重點：");
    println!("  算候選是 µs 等級——不是問題");
    println!("  定案+重算的成本**正比於後區長度**——那就是「把剩下的重打一次」");
    println!(
        "  打完整句 {:.1?} vs 重算一次 40ms 上下 ＝ 每定案一段就重打半句",
        total_typing
    );
}

/// 對照：單純重放同樣長度的按鍵要多久？
///
/// 用來驗證「重算 = 累加一遍」——如果兩者接近，就代表重放本身沒有
/// 異常，成本純粹來自「不該重放這麼多」。
fn replay_cost(keys: &str) -> std::time::Duration {
    let t = Instant::now();
    let mut s = Session::new();
    for c in keys.chars() {
        s.push(c);
    }
    let _ = s.seg_count();
    t.elapsed()
}
