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
//! 見 `dict::encode_connection`。**格式只寫在 core 那一處**——載入端
//! 與這支共用，`--if-stale` 的判斷也是問 core（`connection_is_current`），
//! 這裡不另抄一份檔頭。
//!
//! # 用法
//!
//! ```text
//! cargo run --release -p ime-core --bin gen_connection
//! cargo run --release -p ime-core --bin gen_connection -- --if-stale   # 版面沒變就跳過
//! cargo run --release -p ime-core --bin gen_connection -- --check      # 只檢查，不是目前版面就回 1
//! ```

use std::path::Path;

#[path = "common/gen_flags.rs"]
mod gen_flags;

fn main() {
    let mode = gen_flags::mode("gen_connection");
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data")
        .join("japanese");
    let src = dir.join("connection_single_column.txt");
    let dst = dir.join("connection.bin");
    gen_flags::gate(
        "gen_connection",
        mode,
        &dst,
        ime_core::dict::connection_is_current,
    );

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

    let out = ime_core::dict::encode_connection(n as u16, &data);
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
