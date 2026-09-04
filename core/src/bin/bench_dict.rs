//! 量三本詞庫各要多久載入。
//!
//! # 為什麼需要這支工具
//!
//! 詞庫載入是**切換輸入法時使用者唯一會等的東西**。曾經要等 1.2 秒，
//! 而且原因完全不直覺——不是 I/O 慢（讀 60MB 只要 58ms），是建雜湊表
//! 的成本。這種事憑感覺猜不出來，只能量。
//!
//! 目前的基準（同一台開發機，release build）：
//!
//! | 詞庫 | 時間 |
//! |---|---|
//! | 英文 | 約 10ms |
//! | 注音 | 約 120ms |
//! | 日文 | 約 700ms |
//!
//! 日文那本大幅超過其他兩本，因為 mozc 的十個詞典檔約 60MB、745965
//! 個讀音。要是哪天這個數字又跳回一秒以上，先來這裡量，別直接猜。
//!
//!     cargo run --release -p ime-core --bin bench_dict

use std::time::Instant;

fn main() {
    let data = std::path::Path::new("data");
    if !data.exists() {
        eprintln!("找不到 data/，請在專案根目錄執行");
        return;
    }

    let total = Instant::now();

    let t = Instant::now();
    ime_core::english::load(data);
    println!("英文  {:>5}ms", t.elapsed().as_millis());

    let t = Instant::now();
    let bopomofo = ime_core::dict::load_bopomofo(data);
    println!(
        "注音  {:>5}ms   {} 條",
        t.elapsed().as_millis(),
        bopomofo.map(|d| d.word_count()).unwrap_or(0)
    );

    let t = Instant::now();
    let kana = ime_core::dict::load_japanese(data);
    let n = kana.map(|d| d.len()).unwrap_or(0);
    println!("日文  {:>5}ms   {n} 條", t.elapsed().as_millis());

    println!("合計  {:>5}ms", total.elapsed().as_millis());

    // `--hold`：載完之後停住不退出，讓外面量記憶體。
    //
    // **為什麼需要這個模式**：mmap 的價值在「多個宿主行程共用同一份
    // 實體記憶體」，而那要好幾個行程同時活著才量得出來。真的去開記事本、
    // 瀏覽器、Word 也可以，但那樣量到的還混著各家 App 自己的用量；
    // 同時跑幾個這支，量到的就只有詞庫。
    //
    // 量法（Windows）：`WorkingSet64` 會把共用頁重複算進每個行程，
    // 要看的是**私有工作集**——
    //
    //   Get-Counter '\Process(bench_dict*)\Working Set - Private'
    //
    // 兩者的差額就是共用的部分。
    if std::env::args().any(|a| a == "--hold") {
        // 摸過三本，確保頁面真的進了實體記憶體再讓人量
        let _ = ime_core::dict::word_for("su3cl3");
        let _ = ime_core::dict::best_kana_word("すし");
        let _ = ime_core::dict::connection();
        println!("\nPID {} — 載入完成，按 Enter 結束", std::process::id());
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
    }
}
