//! 把 mozc 的接續矩陣轉成二進位 `data/japanese/connection.bin`。
//!
//! # 為什麼要轉
//!
//! `connection_single_column.txt` 是 **36MB 的文字檔**，2672×2672 =
//! 714 萬行、每行一個成本值。日文整句轉換（Viterbi）需要**整份**矩陣，
//! 而解析 714 萬行正是「文字版詞庫 705ms」問題的翻版——會破壞
//! [切換輸入法 0ms](../../../開發文件.md)。
//!
//! 轉成二進位之後是 **13.6MB**（`u16` × 714 萬），載入時整塊讀進來、
//! 零解析。
//!
//! # 為什麼不是把整份塞進程式
//!
//! 它是**衍生資料**：跟 `char_freq_by_reading.txt` 一樣，由
//! `data/download.ps1` 在各自的機器上產生，不進版控。上游資料換了
//! 重跑就好。
//!
//! # 格式
//!
//! ```text
//! magic  4 bytes  "TSCM"
//! ver    u16      1
//! n      u16      矩陣邊長（id 數）
//! data   u16 × n×n   little-endian，索引 rid * n + lid
//! ```
//!
//! **little-endian 是刻意的**：三個目標平台（Windows／macOS／Linux）
//! 都是 LE，不必為了理論上的可攜性去付位元組序轉換的成本。
//!
//! # 用法
//!
//! ```text
//! cargo run --release -p ime-core --bin gen_connection
//! ```

use std::path::Path;

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data")
        .join("japanese");
    let src = dir.join("connection_single_column.txt");
    let dst = dir.join("connection.bin");

    let t = std::time::Instant::now();
    let content = std::fs::read_to_string(&src).expect("讀不到 connection_single_column.txt");
    let mut lines = content.lines();
    let n: usize = lines
        .next()
        .expect("空檔案")
        .trim()
        .parse()
        .expect("第一行該是矩陣邊長");
    assert!(n > 0 && n <= u16::MAX as usize, "邊長 {n} 不合理");

    // 索引 rid * n + lid，跟 `dict::load_connection_edges` 同一套
    let mut data: Vec<u16> = Vec::with_capacity(n * n);
    let mut bad = 0usize;
    for line in lines.by_ref().take(n * n) {
        match line.trim().parse::<u16>() {
            Ok(v) => data.push(v),
            Err(_) => {
                // 解析不了就當「接不起來」（成本拉到最高），
                // 而不是整份放棄——一行壞掉不該毀掉整個矩陣
                bad += 1;
                data.push(u16::MAX);
            }
        }
    }
    assert_eq!(data.len(), n * n, "行數不足：只讀到 {}", data.len());

    let mut out: Vec<u8> = Vec::with_capacity(8 + data.len() * 2);
    out.extend_from_slice(b"TSCM");
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(n as u16).to_le_bytes());
    for v in &data {
        out.extend_from_slice(&v.to_le_bytes());
    }
    // 原子寫入：這個檔會被 mmap，就地覆寫會動到正在打字的行程
    // 已經映射的位元組。見 `dict::write_data_file`
    ime_core::dict::write_data_file(&dst, &out).expect("寫不出 connection.bin");

    println!("矩陣邊長      {n}");
    println!("格數          {}", data.len());
    println!("輸出大小      {:.1} MB", out.len() as f64 / 1024.0 / 1024.0);
    if bad > 0 {
        println!("解析不了的行  {bad}（已當成接不起來）");
    }
    println!("耗時          {:?}", t.elapsed());
    println!("寫入          {}", dst.display());
}
