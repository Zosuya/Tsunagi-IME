//! 逐鍵量 `Session::push`——**TSF 每一鍵真正走的那條路**，含選字。
//!
//! # 為什麼 `bench_typing` 不夠
//!
//! `bench_typing` 量的是「切法＋排序＋去重」（`Incremental::push`＋
//! `rank::sort`），**不含選字**。可是 TSF 每一鍵走的是
//! `Session::push` → `refresh` → 組候選格 → `compose`（維特比、語言模型、
//! 重切），選字那一段它從來沒量過。
//!
//! 2026-09-23 第一次走這條路量，才知道 **master 自己就有超過 16ms 的鍵**
//! ——long 節 193 鍵那句的第 180 鍵要 16.4ms，而 `bench_typing` 同一批
//! 案例的最慢一鍵才 14.3ms，看起來還有餘裕。改選字（`compose`）的東西
//! 只看 `bench_typing` 會以為不花錢。
//!
//! # 量什麼
//!
//! - `bench_typing` 的 10 個案例（同一批按鍵，方便兩支對照）
//! - 整份測資（`測資.txt` 全部的節），逐鍵餵給一個新的 `Session`
//!
//! 每一輪開**新的執行緒**跑完全部句子——排序的 `Memo` 是 `thread_local`，
//! 新執行緒等於冷快取，每一輪的狀態才一樣。句子之間照順序跑、快取不清，
//! 跟實際打字時一樣（TSF 的執行緒一直活著）。
//!
//! # 每鍵取多輪最小值
//!
//! 機器上有別的東西在跑時，單輪的 p99 會被雜訊拉高好幾成（實測同一台
//! 機器背景有編譯時 p99 是安靜時的 1.7 倍）。**每一鍵跑 N 輪取最小值**
//! 把雜訊濾掉，比較兩個版本時才看得出真正的差。要看「使用者實際會碰到
//! 的最壞情況」就用 `--reps 1` 在安靜的機器上跑。
//!
//! # 用法
//!
//! ```text
//! cargo run --release -p ime-core --bin bench_session                 # 5 輪
//! cargo run --release -p ime-core --bin bench_session -- --reps 10
//! cargo run --release -p ime-core --bin bench_session -- --tag long   # 只量一節
//! cargo run --release -p ime-core --bin bench_session -- --dump 檔名   # 逐鍵倒出 TSV
//! ```
//!
//! `--dump` 每一列是「名稱 TAB 第幾鍵 TAB 總鍵數 TAB 各輪耗時…」，
//! 拿兩個版本的檔案逐鍵對照用。
#[path = "common/testdata.rs"]
mod testdata;

use ime_core::session::Session;
use std::collections::BTreeMap;
use std::time::Instant;

/// 一幀的預算。超過就感覺得到卡頓，理由見 `bench_typing`。
const FRAME_MS: f64 = 16.0;

/// 跟 `bench_typing` 同一批案例——兩支對照時才知道選字多花了多少。
const CASES: &[(&str, &str)] = &[
    ("短句（注音）", "su3cl3"),
    ("短句（英文）", "check"),
    ("中句（中英混）", "rup wu0 5p 2k7 meeting"),
    ("中句（英＋注音）", "middlewared9 "),
    ("長句（純英文 43 鍵）", "the quick brown fox jumps over the lazy dog"),
    (
        "長句（純日文 47 鍵）",
        "maikaikareanishigotowooshitsukerareteirukigasuru",
    ),
    ("長句（三語混合）", "ji3e; e; fm4supermarketa93xk7gyuunyuu"),
    (
        "極長（日文 71 鍵）",
        "maikaikareanishigotowooshitsukerareteirukigasurushikatanaiiitsumokoudesu",
    ),
    (
        "超長（日文 110 鍵）",
        "maikaikareanishigotowooshitsukerareteirukigasurushikatanaiiitsumokoudesukaishanishucchousaseraretemattakuyaruki",
    ),
    (
        "極長（三語 70 鍵）",
        "rup wu0 2k7meeting5p2k7fu4dj4boringji3e; e; fm4supermarketa93xk7gyuunyuu",
    ),
];

/// 一個要逐鍵餵的輸入。
#[derive(Clone)]
struct Item {
    /// 分組：`bench_typing` 的案例是 `cases`，測資是節名
    group: String,
    /// 印出來給人看的名字
    name: String,
    keys: String,
}

/// 取百分位數。`v` 要先排好序。
fn pct(v: &[f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v[((v.len() as f64 - 1.0) * p).round() as usize]
}

fn sorted(mut v: Vec<f64>) -> Vec<f64> {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

/// 跑一輪：每個輸入各開一個新的 `Session`，逐鍵計時（毫秒）。
fn run_once(items: &[Item]) -> Vec<Vec<f64>> {
    items
        .iter()
        .map(|it| {
            let mut s = Session::new();
            it.keys
                .chars()
                .map(|c| {
                    let t = Instant::now();
                    s.push(c);
                    t.elapsed().as_secs_f64() * 1000.0
                })
                .collect()
        })
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opt = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let reps: usize = opt("--reps")
        .and_then(|s| s.parse().ok())
        .unwrap_or(5)
        .max(1);
    let tag = opt("--tag");
    let dump = opt("--dump");

    testdata::load_engine();

    let mut items: Vec<Item> = Vec::new();
    if tag.is_none() {
        items.extend(CASES.iter().map(|(name, keys)| Item {
            group: "cases".into(),
            name: (*name).into(),
            keys: (*keys).into(),
        }));
    }
    items.extend(
        testdata::load("bench_session")
            .into_iter()
            .filter(|r| tag.as_ref().is_none_or(|t| &r.tag == t))
            .map(|r| Item {
                group: r.tag.clone(),
                name: format!("[{}] {}", r.tag, r.want),
                keys: r.keys,
            }),
    );
    let nkeys: usize = items.iter().map(|it| it.keys.chars().count()).sum();
    eprintln!("{} 個輸入、{nkeys} 鍵，跑 {reps} 輪", items.len());

    // 每一輪一條新的執行緒：`Memo` 是 thread_local，這樣每輪都從冷快取開始
    let runs: Vec<Vec<Vec<f64>>> = (0..reps)
        .map(|_| {
            let items = items.clone();
            std::thread::spawn(move || run_once(&items))
                .join()
                .expect("量測執行緒 panic")
        })
        .collect();

    if let Some(path) = &dump {
        let mut out = String::new();
        for (ii, it) in items.iter().enumerate() {
            let n = it.keys.chars().count();
            for ki in 0..n {
                out.push_str(&format!("{}\t{}\t{n}", it.name, ki + 1));
                for r in &runs {
                    out.push_str(&format!("\t{:.4}", r[ii][ki]));
                }
                out.push('\n');
            }
        }
        if let Err(e) = std::fs::write(path, out) {
            eprintln!("寫不進 {path}：{e}");
        }
    }

    // 每一鍵取各輪的最小值
    let best: Vec<Vec<f64>> = (0..items.len())
        .map(|ii| {
            (0..runs[0][ii].len())
                .map(|ki| runs.iter().map(|r| r[ii][ki]).fold(f64::MAX, f64::min))
                .collect()
        })
        .collect();

    println!("=== Session::push 逐鍵耗時（含選字，每鍵取 {reps} 輪最小）===");

    if tag.is_none() {
        println!("\n── bench_typing 的案例 ──");
        println!("  {:24} {:>5} {:>9}", "案例", "鍵數", "最慢一鍵");
        let mut sub = Vec::new();
        for (it, v) in items
            .iter()
            .zip(&best)
            .filter(|(it, _)| it.group == "cases")
        {
            let (at, mx) = v
                .iter()
                .copied()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap_or((0, 0.0));
            let mark = if mx > FRAME_MS { " ⚠" } else { "" };
            println!(
                "  {:24} {:>5} {mx:>7.2}ms（第 {} 鍵）{mark}",
                it.name,
                v.len(),
                at + 1
            );
            sub.extend_from_slice(v);
        }
        let sub = sorted(sub);
        println!(
            "  小計 {} 鍵  p50 {:.2}ms  p99 {:.2}ms  最慢 {:.2}ms",
            sub.len(),
            pct(&sub, 0.5),
            pct(&sub, 0.99),
            sub.last().copied().unwrap_or(0.0)
        );
    }

    let mut by_sec: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
    for (it, v) in items
        .iter()
        .zip(&best)
        .filter(|(it, _)| it.group != "cases")
    {
        by_sec
            .entry(it.group.as_str())
            .or_default()
            .extend_from_slice(v);
    }
    let rows: usize = items.iter().filter(|it| it.group != "cases").count();
    println!("\n── 測資逐鍵（{rows} 句）──");
    println!(
        "  {:<18} {:>7} {:>9} {:>9} {:>9} {:>6}",
        "節", "鍵數", "p50", "p99", "最慢", ">16ms"
    );
    let mut td = Vec::new();
    for (sec, v) in by_sec {
        td.extend_from_slice(&v);
        let over = v.iter().filter(|x| **x > FRAME_MS).count();
        let v = sorted(v);
        println!(
            "  {sec:<18} {:>7} {:>7.2}ms {:>7.2}ms {:>7.2}ms {over:>6}",
            v.len(),
            pct(&v, 0.5),
            pct(&v, 0.99),
            v.last().copied().unwrap_or(0.0)
        );
    }
    let over = td.iter().filter(|x| **x > FRAME_MS).count();
    let total: f64 = td.iter().sum();
    let td = sorted(td);
    println!(
        "  測資合計 {} 鍵  p50 {:.3}ms  p95 {:.2}ms  p99 {:.2}ms  最慢 {:.2}ms  總和 {total:.0}ms  >16ms {over} 鍵",
        td.len(),
        pct(&td, 0.5),
        pct(&td, 0.95),
        pct(&td, 0.99),
        td.last().copied().unwrap_or(0.0)
    );

    // 最慢的幾鍵：哪一句、第幾鍵
    let mut worst: Vec<(f64, usize, usize)> = best
        .iter()
        .enumerate()
        .flat_map(|(ii, v)| v.iter().enumerate().map(move |(ki, &x)| (x, ii, ki)))
        .collect();
    worst.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    println!("\n  最慢 10 鍵：");
    for &(x, ii, ki) in worst.iter().take(10) {
        let it = &items[ii];
        println!(
            "    {x:>7.2}ms  第 {}/{} 鍵  {}",
            ki + 1,
            it.keys.chars().count(),
            it.name
        );
    }
}
