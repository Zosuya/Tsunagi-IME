//! 接續矩陣的載入成本：**整句轉換能不能做的前提**。
//!
//! 日文整句轉換（Viterbi）需要整份 2672×2672 矩陣。原始資料是 36MB
//! 的文字檔、714 萬行，解析它正是「文字版詞庫 705ms」問題的翻版——
//! 會破壞[切換輸入法 0ms](../../../開發文件.md)。
//!
//! 這支量三件事：文字版要多久、二進位版要多久、記憶體多了多少。
//!
//! 用法：`cargo run --release -p ime-core --bin bench_connection`

use std::time::Instant;

#[cfg(windows)]
fn resident_mb() -> Option<u64> {
    let out = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!("(Get-Process -Id {}).WorkingSet64", std::process::id()),
        ])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<u64>()
        .ok()
        .map(|b| b / 1024 / 1024)
}

#[cfg(not(windows))]
fn resident_mb() -> Option<u64> {
    None
}

fn main() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data");
    let before = resident_mb();
    // **只量二進位**：文字版會配置 36MB 字串加 14MB 向量，就算之後
    // 釋放掉，配置器不一定馬上把記憶體還給系統，量出來的數字會不乾淨。
    let bin_only = std::env::args().any(|a| a == "--bin-only");

    // ── 文字版：整份解析要多久 ──
    let txt = data.join("japanese").join("connection_single_column.txt");
    let t = Instant::now();
    let text_ms = match if bin_only {
        Err(std::io::Error::other("skip"))
    } else {
        std::fs::read_to_string(&txt)
    } {
        Ok(content) => {
            let mut lines = content.lines();
            let n: usize = lines
                .next()
                .and_then(|l| l.trim().parse().ok())
                .unwrap_or(0);
            let mut v: Vec<u16> = Vec::with_capacity(n * n);
            for line in lines.take(n * n) {
                v.push(line.trim().parse::<u16>().unwrap_or(u16::MAX));
            }
            let ms = t.elapsed();
            println!("文字版   解析 {} 格，{:?}", v.len(), ms);
            Some(ms)
        }
        Err(_) if bin_only => {
            println!("文字版   （--bin-only，跳過）");
            None
        }
        Err(_) => {
            println!("文字版   讀不到（跑 data/download.ps1）");
            None
        }
    };

    // ── 二進位版 ──
    let t = Instant::now();
    let conn = ime_core::dict::load_connection(&data);
    let bin_ms = t.elapsed();
    match conn {
        Some(c) => {
            println!("二進位版 載入 {}×{}，{:?}", c.size(), c.size(), bin_ms);
            // 隨手抽驗幾格，確認資料是活的
            println!(
                "         抽驗 cost(0,1)={} cost(1,0)={} cost(100,200)={}",
                c.cost(0, 1),
                c.cost(1, 0),
                c.cost(100, 200)
            );
            // 查詢成本：Viterbi 會查很多次
            let t = Instant::now();
            let mut acc = 0u64;
            for i in 0..1_000_000u32 {
                acc += c.cost((i % 2672) as u16, ((i * 7) % 2672) as u16) as u64;
            }
            println!(
                "         查 100 萬次 {:?}（總和 {acc}，防最佳化掉）",
                t.elapsed()
            );
        }
        None => println!("二進位版 讀不到——跑 gen_connection"),
    }

    if let (Some(t), Some(b)) = (text_ms, Some(bin_ms)) {
        println!(
            "\n快了 {:.0} 倍",
            t.as_secs_f64() / b.as_secs_f64().max(1e-9)
        );
    }
    if let (Some(a), Some(b)) = (before, resident_mb()) {
        println!(
            "常駐記憶體 {a} MB → {b} MB（淨增 {} MB）",
            b.saturating_sub(a)
        );
    }
}
