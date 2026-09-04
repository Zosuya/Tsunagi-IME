//! ime-core: 平台無關的核心引擎。
//!
//! # 架構：合法性優先
//!
//! ```text
//! Phase 1  合法性判斷（yes/no，不碰詞庫）   ← 目前在這
//! Phase 2  切點引擎（依合法性瀑布切段）
//! 後續     選詞模組（查詞庫、決定顯示什麼）  ← 獨立於前兩者
//! ```
//!
//! 關鍵是**把「合不合法」跟「該顯示什麼」分開**。舊架構讓引擎同時
//! 回答這兩件事，合法性因此被壓成分數的一部分，再跟詞頻混在一起加權，
//! 於是分不出「這不可能是日文」與「這是罕見的日文詞」——兩者都是低分。
//! 同一份測資，舊架構切點正確率 40.8%，分開之後 95.3%。見開發文件 §2.6。
//!
//! 設計依據是 canvas 規格檔（見 CLAUDE.md 的「規格檔」一節），
//! 那些 canvas 每個判斷節點恰好兩條出邊，可以當程式直接跑。

// 設定檔的讀寫。掛在 `config` feature 後面，
// 讓純演算法的工具能 --no-default-features 維持零依賴。
pub mod bopomofo;
pub mod command;
pub mod compose;
#[cfg(feature = "config")]
pub mod config;
pub mod cutpoint;
pub mod dict;
pub mod dict_bin;
pub mod dict_bin_zh;
pub mod english;
pub mod input;
pub mod language;
pub mod learn;
pub mod pack;
pub mod render;
pub mod romaji;
pub mod sanitize;
pub mod session;
#[cfg(feature = "config")]
pub mod theme_preset;
pub mod width;

/// 一個候選字/詞。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub text: String,
    pub label: &'static str,
}

/// 把**有啟用的**詞庫一次讀完。
///
/// 詞庫是惰性載入（`OnceLock`），第一次查詢才讀檔——那在 TSF 裡代表
/// 第一次打字會卡住。啟用輸入法時就呼叫這個把它讀完。
///
/// # 為什麼要看 `engines`
///
/// 三本詞庫的成本天差地遠（實測）：
///
/// | 詞庫 | 載入時間 |
/// |---|---|
/// | 英文 | 8 ms |
/// | 注音 | 122 ms |
/// | 日文 | **1086 ms** |
///
/// 日文那批檔案將近 90 MB，佔了總時間的九成。沒開日文引擎的話，那
/// 一秒是**純粹白等**——載完一本永遠不會被查的詞庫。切換輸入法要等
/// 一秒的問題就是這樣來的。
///
/// 英文不看設定，因為它是瀑布的最後一站，永遠會用到（而且只要 8ms）。
///
/// 引擎是**後來才打開**的話這裡不會補載，但惰性載入還在，第一次查詢
/// 會自己讀進來；下次啟用輸入法時就會被這裡預載了。
pub fn preload(data_dir: &std::path::Path, engines: config::Engines) {
    english::load(data_dir);
    if engines.bopomofo {
        dict::load_bopomofo(data_dir);
    }
    if engines.romaji {
        dict::load_japanese(data_dir);
        // 整句轉換要用的完整接續矩陣。**載不到不影響**——那時
        // `convert` 的接續成本一律當 0，退化成「只看詞成本」，
        // 仍然比整段不轉好。見 `romaji::convert`。
        dict::load_connection(data_dir);
    }
}
