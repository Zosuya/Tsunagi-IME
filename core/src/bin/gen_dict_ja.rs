//! 把 mozc 的十個文字詞典編成二進位版面（`data/japanese/dict_ja.bin`）。
//!
//! # 為什麼要這一步
//!
//! 文字版每次啟動要花 700ms 逐行切欄位、為 187.5 萬個字串各配一次
//! 記憶體，常駐 287MB。二進位版面讀進來就是能查的樣子——載入 12ms、
//! 常駐剩下零頭。見 `dict_bin` 的模組說明。
//!
//! **不跑這支也能用**：`load_japanese` 找不到 `.bin` 會現場從文字建同
//! 一份版面（走的是同一個 `build_kana_layout`，不會有兩種結果），只是
//! 每次啟動都要等那 700ms。跑一次就省下來了。
//!
//! 這是衍生資料，不進版控——跟 `connection.bin` 一樣，換機器重跑即可。
//!
//! **但安裝版沒有這條退路**：安裝程式只帶 `.bin`、不帶文字詞典，舊版面
//! 被包出去就是整個沒有日文詞典。所以建置與打包腳本一律跑 `--if-stale`
//! （版面改了才重產），打包另外用 `--check` 守門，見 `common/gen_flags.rs`。
//!
//!     cargo run --release -p ime-core --bin gen_dict_ja
//!     cargo run --release -p ime-core --bin gen_dict_ja -- --if-stale
//!     cargo run --release -p ime-core --bin gen_dict_ja -- --check

use std::time::Instant;

#[path = "common/gen_flags.rs"]
mod gen_flags;

fn main() {
    let mode = gen_flags::mode("gen_dict_ja");
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data");
    let out = data.join("japanese").join("dict_ja.bin");
    gen_flags::gate("gen_dict_ja", mode, &out, ime_core::dict_bin::is_current);

    // 原料不齊就不產：執行期的 `build_kana_layout` 缺原料會默默退化（那是
    // 退路，該退），但這裡產的是要被包出去、被 `--if-stale` 當成「已是
    // 最新」的成品——檔頭是對的，之後沒有任何關卡看得出它少了詞性
    let missing = ime_core::dict::missing_kana_sources(&data);
    if !missing.is_empty() {
        eprintln!("原料不齊，不產 dict_ja.bin（產出來會默默少東西）：");
        for p in &missing {
            eprintln!("  缺 {}", p.display());
        }
        eprintln!("先跑 data/download.ps1");
        std::process::exit(1);
    }

    let t = Instant::now();
    let Some(raw) = ime_core::dict::build_kana_layout(&data) else {
        eprintln!("讀不到 dictionary00.txt ～ dictionary09.txt");
        std::process::exit(1);
    };
    let build_ms = t.elapsed().as_millis();

    if let Err(e) = ime_core::dict::write_data_file(&out, &raw) {
        eprintln!("寫不進 {}：{e}", out.display());
        std::process::exit(1);
    }

    // 立刻讀回來確認認得——寫出一個載不了的檔比沒有檔更糟
    let leaked: &'static [u8] = Box::leak(raw.into_boxed_slice());
    let Some(d) = ime_core::dict_bin::KanaDict::new(leaked) else {
        eprintln!("產生出來的檔案自己認不得，格式有問題");
        std::process::exit(1);
    };

    println!("讀音        {}", d.len());
    println!("輸出大小    {:.1} MB", leaked.len() as f64 / 1048576.0);
    println!("建構耗時    {build_ms}ms");
    println!("寫入        {}", out.display());
}
