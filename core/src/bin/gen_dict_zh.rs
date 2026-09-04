//! 把注音的兩張表編成二進位版面（`data/bopomofo/dict_zh.bin`）。
//!
//! # 為什麼要這一步
//!
//! 文字版的注音常駐 23MB、建表峰值再多 21MB，而**資料本身只有 2.3MB**。
//! 常駐那部分是 13.2 萬個 `String` 鍵值各自一次堆配置；峰值那部分是
//! 字頻、詞頻、偏好表、讀音別字頻——那四份只有建表要用，決定完同音字
//! 的順序就沒事了。編成版面之後執行期兩者都不必付。
//!
//! **不跑這支也能用**：`load_bopomofo` 找不到 `.bin` 會現場從文字建同
//! 一份版面（同一個 `build_zh_layout`，不會有兩種結果）。
//!
//! 這是衍生資料，不進版控——跟 `connection.bin`、`dict_ja.bin` 一樣。
//!
//!     cargo run --release -p ime-core --bin gen_dict_zh

use std::time::Instant;

fn main() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data");

    let t = Instant::now();
    let Some(raw) = ime_core::dict::build_zh_layout(&data) else {
        eprintln!("讀不到 BPMFMappings.txt，先跑 data/download.ps1");
        std::process::exit(1);
    };
    let build_ms = t.elapsed().as_millis();

    let out = data.join("bopomofo").join("dict_zh.bin");
    if let Err(e) = ime_core::dict::write_data_file(&out, &raw) {
        eprintln!("寫不進 {}：{e}", out.display());
        std::process::exit(1);
    }

    // 立刻讀回來確認認得——寫出一個載不了的檔比沒有檔更糟
    let leaked: &'static [u8] = Box::leak(raw.into_boxed_slice());
    let Some(d) = ime_core::dict_bin_zh::ZhDict::new(leaked) else {
        eprintln!("產生出來的檔案自己認不得，格式有問題");
        std::process::exit(1);
    };

    println!("詞          {}", d.word_count());
    println!("輸出大小    {:.1} MB", leaked.len() as f64 / 1048576.0);
    println!("建構耗時    {build_ms}ms");
    println!("寫入        {}", out.display());
}
