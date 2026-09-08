//! 段選單的每次操作要花多久？（判準：一幀 16ms）
//!
//! # 為什麼不能用 `spike_seg_speed`
//!
//! 那支是**規劃階段**量的，它自己 `Incremental::from_keys` 從零重建
//! 後區——那正是實作走快路（`freeze_user_prefix`）要避開的成本。
//! 它的數字是「如果沒有快路會多慢」的歷史對照，不是現況。
//!
//! # 這支量什麼
//!
//! 走**真正的 `Session` API**，跟平台層同一條路：
//!
//! | 操作 | 熱在哪 |
//! |---|---|
//! | 開選單 | `seg_cands` 窮舉長度 × 語言 |
//! | 換段（←→）| 同上，每次都重算候選 |
//! | 換候選（↑↓）| `seg_preview_texts` 要重畫預覽 |
//! | 推邊界（Shift+←→）| 多一次合法性判斷 |
//! | **定案（Enter）** | 前區凍結＋後區重算，最貴的一個 |
//! | 台語斷詞 | `tw_words` 最大匹配，鎖定注音時每次畫選單都跑 |
//!
//! 用法：`cargo run --release -p ime-core --bin bench_segmenu`
#[path = "common/testdata.rs"]
mod testdata;

use ime_core::session::Session;
use std::time::Instant;

/// 一組耗時的統計。
struct Stat {
    name: &'static str,
    times: Vec<f64>,
}

impl Stat {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            times: Vec::new(),
        }
    }
    fn add(&mut self, ms: f64) {
        self.times.push(ms);
    }
    fn pct(&mut self, p: f64) -> f64 {
        if self.times.is_empty() {
            return 0.0;
        }
        self.times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let i = ((self.times.len() as f64 - 1.0) * p).round() as usize;
        self.times[i]
    }
    fn max(&self) -> f64 {
        self.times.iter().cloned().fold(0.0, f64::max)
    }
    fn over(&self) -> usize {
        self.times.iter().filter(|t| **t > 16.0).count()
    }
}

fn ms(f: impl FnOnce()) -> f64 {
    let t = Instant::now();
    f();
    t.elapsed().as_secs_f64() * 1000.0
}

fn main() {
    testdata::load_engine();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("packs");
    ime_core::pack::set_bundled_dir(Some(root));
    ime_core::pack::load("", &["台語".to_string()]);
    let tw_ready = ime_core::pack::any_tw();

    let cases = testdata::load("bench_segmenu");
    eprintln!(
        "測資 {} 句，台語包 {}",
        cases.len(),
        if tw_ready {
            "有"
        } else {
            "沒有（台語那段跳過）"
        }
    );

    let mut open = Stat::new("開選單（TAB）");
    let mut right = Stat::new("換段（→）");
    let mut down = Stat::new("換候選（↓）");
    let mut widen = Stat::new("推邊界（Shift+→）");
    let mut confirm = Stat::new("定案（Enter）");
    // 超時的案例，印出來給人看
    let mut slow: Vec<String> = Vec::new();

    for c in &cases {
        let keys = &c.keys;
        if keys.chars().count() < 4 {
            continue;
        }
        let mut s = Session::new();
        for ch in keys.chars() {
            s.push(ch);
        }
        open.add(ms(|| {
            s.seg_open();
            let _ = s.seg_cands();
        }));
        // 走過每一段：換段、換候選、推邊界各一次
        let n = s.seg_count().min(8);
        for _ in 0..n {
            right.add(ms(|| {
                s.seg_right();
                let _ = s.seg_cands();
            }));
            down.add(ms(|| {
                s.seg_next_cand();
                let _ = s.seg_preview_texts();
            }));
            widen.add(ms(|| {
                s.seg_widen();
                let _ = s.seg_cands();
            }));
        }
        // 定案：從第一段開始——**後區最長，最貴的情況**
        let mut s2 = Session::new();
        for ch in keys.chars() {
            s2.push(ch);
        }
        s2.seg_open();
        for _ in 0..s2.seg_count().min(4) {
            if s2.seg_done() {
                break;
            }
            let t = ms(|| {
                s2.seg_confirm_with(true);
                let _ = s2.seg_cands();
            });
            if t > 16.0 && slow.len() < 12 {
                slow.push(format!(
                    "  {t:>6.1}ms  [{}] {} 鍵  {}",
                    c.tag,
                    keys.chars().count(),
                    c.want
                ));
            }
            confirm.add(t);
        }
    }

    // ── 台語：鎖定注音，斷詞在每次畫選單時都跑 ──
    let mut tw_open = Stat::new("台語開選單（含斷詞）");
    let mut tw_next = Stat::new("台語跳詞（→）");
    let mut tw_conf = Stat::new("台語定案（Enter）");
    if tw_ready {
        for c in &cases {
            if c.keys.chars().count() < 4 {
                continue;
            }
            let mut s = Session::new();
            s.set_lock(Some(ime_core::language::Language::Bopomofo));
            for ch in c.keys.chars() {
                s.push(ch);
            }
            tw_open.add(ms(|| {
                s.seg_open();
                let _ = s.seg_cands();
            }));
            for _ in 0..4 {
                tw_next.add(ms(|| {
                    s.tw_next();
                    let _ = s.seg_cands();
                }));
            }
            for _ in 0..3 {
                if s.seg_done() {
                    break;
                }
                tw_conf.add(ms(|| {
                    s.seg_confirm_with(true);
                    let _ = s.seg_cands();
                }));
            }
        }
    }

    println!();
    println!("段選單每次操作的耗時（判準：一幀 16ms）");
    println!("────────────────────────────────────────────────────────────");
    println!(
        "{:<24} {:>6} {:>9} {:>9} {:>9} {:>8}",
        "操作", "次數", "p50", "p95", "p99", "最慢"
    );
    println!("────────────────────────────────────────────────────────────");
    let mut all_over = 0usize;
    let mut all_n = 0usize;
    for st in [
        &mut open,
        &mut right,
        &mut down,
        &mut widen,
        &mut confirm,
        &mut tw_open,
        &mut tw_next,
        &mut tw_conf,
    ] {
        if st.times.is_empty() {
            continue;
        }
        let n = st.times.len();
        let (p50, p95, p99, mx, over) = (
            st.pct(0.50),
            st.pct(0.95),
            st.pct(0.99),
            st.max(),
            st.over(),
        );
        all_over += over;
        all_n += n;
        let mark = if p99 > 16.0 { " ✗" } else { "" };
        println!(
            "{:<24} {n:>6} {p50:>8.2}ms {p95:>8.2}ms {p99:>8.2}ms {mx:>7.1}ms{mark}",
            st.name
        );
    }
    println!("────────────────────────────────────────────────────────────");
    if !slow.is_empty() {
        println!();
        println!("超過一幀的定案（最多列 12 筆）：");
        for l in &slow {
            println!("{l}");
        }
    }
    println!();
    if all_over == 0 {
        println!("  ✓ {all_n} 次操作全部在一幀（16ms）內");
    } else {
        println!(
            "  {} {all_over} / {all_n} 次超過一幀（{:.2}%）",
            if all_over * 1000 < all_n {
                "△"
            } else {
                "✗"
            },
            all_over as f64 * 100.0 / all_n as f64
        );
    }
}
