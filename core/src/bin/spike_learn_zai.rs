//! **spike（2026-09-10）**：學習層修不修得掉 `在`／`再`？
//!
//! §2.61.2 判定這一組卡在詞庫沒有詞性，而 `spike_zaizai` 進一步量到
//! **bigram 對「再＋動詞」15 組全錯**——因為「在說」（進行式）在中文
//! 裡本來就合法，統計必然選常見的那個。
//!
//! 那是統計分不出**使用者的意圖**，不是資料錯誤。所以剩下的問題是：
//! **使用者選一次之後，下次會不會自動對？**
//!
//! 用法：cargo run --release -p ime-core --bin spike_learn_zai

use ime_core::compose;
use ime_core::cutpoint::incremental::Incremental;
use ime_core::cutpoint::{normalize, rank};

fn text_of(keys: &str) -> String {
    let cands = rank::sort(Incremental::from_keys(keys).cuttings());
    let Some(top1) = cands.first() else {
        return String::new();
    };
    compose::text_of(&compose::compose(&normalize(top1)))
}

/// 模擬「使用者把第 `idx` 格選成 `choice`，然後送出」。
fn pick_and_commit(keys: &str, idx: usize, choice: &str) -> usize {
    let cands = rank::sort(Incremental::from_keys(keys).cuttings());
    let Some(top1) = cands.first() else { return 0 };
    let mut slots = compose::compose(&normalize(top1));
    compose::pick(&mut slots, idx, choice);
    ime_core::learn::record(&slots)
}

fn main() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data");
    ime_core::pack::set_bundled_dir(data.parent().map(|d| d.join("packs")));
    ime_core::pack::load(
        "__不存在的資料夾__",
        &[ime_core::pack::BUNDLED_SYMBOLS.to_string()],
    );
    ime_core::english::load(&data);
    ime_core::dict::load_bopomofo(&data);
    ime_core::lm::load(&data, ime_core::dict::char_freq_map(&data));
    ime_core::dict::load_japanese(&data);
    // **從乾淨的學習層開始**，不然量到的是本機累積的結果
    ime_core::learn::clear();

    // (按鍵, 該選的字在第幾格, 使用者要的字, 期望的整句)
    let cases = [
        ("y94fm,4bp4", 0, "再", "再確認"),
        ("y94rm,62u/4", 0, "再", "再決定"),
        ("y94d9 g3", 0, "再", "再開始"),
        ("ji3y94gji ", 1, "再", "我再說"),
    ];

    println!("\n=== 學習層對 `在`／`再` 的效果 ===\n");
    for (keys, idx, choice, want) in cases {
        let before = text_of(keys);
        let n = pick_and_commit(keys, idx, choice);
        let after = text_of(keys);
        let ok = if after == want { "✓" } else { "✗" };
        println!("  {ok} [{keys}]");
        println!("      選之前：「{before}」");
        println!("      選一次（記了 {n} 條）");
        println!("      選之後：「{after}」  期望「{want}」");
    }

    // **關鍵問題：學到的東西會不會污染該用「在」的情況？**
    println!("\n=== 學過「再」之後，該用「在」的還對嗎？ ===\n");
    for (keys, want) in [
        ("y94w961o3", "在台北"),
        ("vu04y94", "現在"),
        ("ji3y94w961o3", "我在台北"),
    ] {
        let got = text_of(keys);
        let ok = if got == want { "✓" } else { "✗" };
        println!("  {ok} [{keys}] 得到「{got}」  期望「{want}」");
    }

    let (n, _) = ime_core::learn::stats(None);
    println!("\n學習層現在有 {n} 條");

    // **記了什麼鍵？** 查詢端問的是整段按鍵，記的是不是同一個？
    println!("\n=== 學習層對這幾個鍵記了什麼 ===");
    let idx = ime_core::learn::index();
    for k in [
        "y94",
        "y94fm,4bp4",
        "fm,4bp4",
        "y94d9 g3",
        "d9 g3",
        "y94gji ",
        "gji ",
        "ji3y94gji ",
    ] {
        println!(
            "  [{k}] best={:?}  在={} 再={}",
            idx.best(k),
            idx.count(k, "在"),
            idx.count(k, "再")
        );
    }

    // **選很多次會不會贏？** 學習權重是 `base × k^N`，而最終決定
    // 選字的是 `apply_lm`（bigram 維特比）——它看的是候選排序還是
    // 自己的分數？
    println!(
        "
=== 選 N 次之後「再確認」對了沒 ==="
    );
    for extra in 1..=8 {
        pick_and_commit("y94fm,4bp4", 0, "再");
        let got = text_of("y94fm,4bp4");
        let cnt = ime_core::learn::index().count("y94", "再");
        println!("  選了 {} 次（count={cnt}）→ 「{got}」", extra + 1);
        if got == "再確認" {
            println!("  ↑ 這一次開始對了");
            break;
        }
    }
}
