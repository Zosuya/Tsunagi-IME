//! spike：段選單「選了就凍、後面重算」要花多久？會不會卡？
//!
//! # 要驗證什麼
//!
//! 使用者的模型是「選了某一段就凍住前面、後面重算」。重算是同步的——
//! 使用者按下候選的那一刻要看到新的分段。**超過一幀（16ms）就感覺
//! 得到頓**，判準跟 `bench_typing` 一樣。
//!
//! 直覺上重算應該很快：後區比整句短，而組合爆炸對長度極度敏感。但
//! 有兩件事要實測：
//!
//! 1. **`Incremental::from_keys` 是從零重建**，不像打字時是一鍵一鍵
//!    累加。同樣長度下它比較貴嗎？
//! 2. **最壞情況**：使用者在長句的**第一段**就選了，後區幾乎跟整句
//!    一樣長。那一下要多久？
//!
//! # 怎麼量
//!
//! 對每一句，模擬使用者在每個可能的段界上選一次，量後區重算的耗時。
//! 拿它跟「打字時最後一鍵」的耗時對照——後者是現在已經在承受的成本。
//!
//! # 用法
//!
//! ```text
//! cargo run --release -p ime-core --bin spike_seg_speed
//! cargo run --release -p ime-core --bin spike_seg_speed -- --verbose
//! ```

use ime_core::cutpoint::{incremental::Incremental, rank};
use ime_core::input::Input;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

#[path = "common/testdata.rs"]
mod testdata;

/// 一幀。超過使用者就感覺得到頓（跟 `bench_typing` 同一個判準）。
const FRAME_BUDGET: Duration = Duration::from_millis(16);

/// 為什麼打字每一鍵只要 3.8ms，重算 90 鍵卻要 20ms？
///
/// `from_keys` 本來就是逐鍵 `push`（見它的說明），所以重算 90 鍵
/// ≈ 打 90 鍵的總時間。打字時使用者是**分 90 次**付這個成本，一次
/// 只感覺到一鍵；重算是**一次付清**。
///
/// 這也是為什麼「使用者在越前面選、越慢」——後區越長，要重放的越多。

#[derive(Default)]
struct Tally {
    rows: usize,
    /// 模擬了幾次「選一段」
    picks: usize,
    /// 每次重算的耗時
    times: Vec<Duration>,
    /// 打字時最後一鍵的耗時（對照組）
    typing: Vec<Duration>,
    /// **只重算一截**（`WINDOW` 鍵）的耗時
    window: Vec<Duration>,
}

fn main() {
    let verbose = std::env::args().any(|a| a == "--verbose");
    testdata::load_engine();
    let rows = testdata::load("spike_seg_speed");
    if rows.is_empty() {
        eprintln!("沒有測資");
        return;
    }

    let mut by_tag: BTreeMap<String, Tally> = BTreeMap::new();
    let mut all = Tally::default();
    let mut worst: Vec<(Duration, String)> = Vec::new();

    for row in &rows {
        let input = Input::from_keys(&row.keys, None);
        let cuttings = input.cuttings();
        let Some(first) = cuttings.first() else {
            continue;
        };

        let t = by_tag.entry(row.tag.clone()).or_default();
        t.rows += 1;
        all.rows += 1;

        // 對照組：打字時的最後一鍵有多貴？
        //
        // 那是現在已經在承受的成本——重算只要不比它慢太多就不會有
        // 新的體驗問題。
        let keys_n = row.keys.chars().count();
        if keys_n > 1 {
            let head: String = row.keys.chars().take(keys_n - 1).collect();
            let last = row.keys.chars().last().unwrap();
            let mut inc = Incremental::from_keys(&head);
            let t0 = Instant::now();
            inc.push(last);
            let _ = rank::sort(inc.cuttings());
            let dt = t0.elapsed();
            t.typing.push(dt);
            all.typing.push(dt);
        }

        // 使用者在每個段界上各選一次，量後區重算
        //
        // **段界從第一名的切法取**——那正是畫面上顯示的分段，使用者
        // 反白的就是這些段。
        let mut pos = 0usize;
        for s in first {
            pos += s.keys.chars().count();
            if pos >= keys_n {
                break;
            }
            let rest: String = row.keys.chars().skip(pos).collect();
            let t0 = Instant::now();
            let inc = Incremental::from_keys(&rest);
            let _ = rank::sort(inc.cuttings());
            let dt = t0.elapsed();

            // **只重算一截**：後區不必整段重放，算到夠使用者看的長度
            // 就好，剩下的等他繼續往後選再算。
            //
            // `WINDOW` 取 24——足夠涵蓋畫面上看得到的幾段，而 24 鍵
            // 的重算成本跟打字時的一鍵是同一個量級。
            const WINDOW: usize = 24;
            let win: String = rest.chars().take(WINDOW).collect();
            let t1 = Instant::now();
            let inc2 = Incremental::from_keys(&win);
            let _ = rank::sort(inc2.cuttings());
            let dt_win = t1.elapsed();
            t.window.push(dt_win);
            all.window.push(dt_win);
            t.picks += 1;
            t.times.push(dt);
            all.picks += 1;
            all.times.push(dt);
            if verbose && dt > Duration::from_millis(4) && worst.len() < 200 {
                worst.push((
                    dt,
                    format!(
                        "  {:>7.1?}  在第 {pos} 鍵後選，後區 {} 鍵｜{}",
                        dt,
                        keys_n - pos,
                        &row.keys[..row.keys.len().min(46)]
                    ),
                ));
            }
        }
    }

    println!("「選了就凍」重算的耗時（判準：一幀 {FRAME_BUDGET:?}）\n");
    println!(
        "{:<16} {:>5} {:>6} │ {:>9} {:>9} │ {:>9} {:>9}",
        "節", "句數", "選幾次", "全後區·p99", "全後區·最慢", "只算24鍵·p99", "打字·p99"
    );
    println!("{}", "─".repeat(76));
    for (tag, t) in &by_tag {
        print_row(tag, t);
    }
    println!("{}", "─".repeat(76));
    print_row("總計", &all);

    let p99 = percentile(&mut all.times.clone(), 0.99);
    let worst_t = all.times.iter().max().copied().unwrap_or_default();
    let typing_p99 = percentile(&mut all.typing.clone(), 0.99);
    println!(
        "\n重算 {} 次：p99 {:.2?}、最慢 {:.2?}",
        all.picks, p99, worst_t
    );
    println!("對照：打字最後一鍵 p99 {typing_p99:.2?}");
    let over = all.times.iter().filter(|d| **d > FRAME_BUDGET).count();
    println!(
        "\n超過一幀的次數：{over} / {}（{:.2}%）",
        all.picks,
        100.0 * over as f64 / all.picks.max(1) as f64
    );
    if p99 < FRAME_BUDGET {
        println!("**p99 在預算內**——重算不會造成感覺得到的頓。");
    } else {
        println!("**p99 超出預算**——重算會頓，要想辦法。");
    }

    if verbose && !worst.is_empty() {
        worst.sort_by(|a, b| b.0.cmp(&a.0));
        println!("\n最慢的幾次重算：");
        for (_, s) in worst.iter().take(15) {
            println!("{s}");
        }
    }
}

fn print_row(name: &str, t: &Tally) {
    let mut times = t.times.clone();
    let mut typing = t.typing.clone();
    let mut window = t.window.clone();
    println!(
        "{:<16} {:>5} {:>6} │ {:>9.2?} {:>9.2?} │ {:>9.2?} {:>9.2?}",
        name,
        t.rows,
        t.picks,
        percentile(&mut times, 0.99),
        t.times.iter().max().copied().unwrap_or_default(),
        percentile(&mut window, 0.99),
        percentile(&mut typing, 0.99),
    );
}

fn percentile(v: &mut [Duration], p: f64) -> Duration {
    if v.is_empty() {
        return Duration::ZERO;
    }
    v.sort_unstable();
    let i = ((v.len() as f64 - 1.0) * p).round() as usize;
    v[i]
}
