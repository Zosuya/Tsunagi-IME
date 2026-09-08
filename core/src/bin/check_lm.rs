//! 語言模型的**讀取器驗證**：Rust 版算出來的分數要跟 spike 的 Python
//! 版一致。
//!
//! 為什麼要這支：`lm.rs` 是照 darts-clone 的位元運算手寫的，
//! **算錯不會當機、只會靜靜給出錯的分數**——那種缺陷在計分器上看起來
//! 只是「收益比預期少一點」，極難聯想到讀取器。所以先釘住幾個 spike
//! 量過的實際數值。
//!
//! 用法：cargo run --release -p ime-core --bin check_lm

fn main() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data");
    let freq = ime_core::dict::char_freq_map(&data);
    println!("字頻表 {} 字", freq.len());
    let Some(lm) = ime_core::lm::load(&data, freq) else {
        println!("✗ 載入失敗：找不到 data/bopomofo/zh_bigram.gram 或版面認不得");
        std::process::exit(1);
    };
    println!("✓ 載入成功\n");

    // spike 的 Python 版量到的關聯強度（ln(次數) − ln(命中字形字頻)）
    // 誤差容忍 0.01——Python 用 f64、我們用 f32
    let cases: &[(char, char, f32)] = &[
        ('煩', '你', 3.747),
        ('過', '你', 6.464),
        ('謝', '你', 6.666),
        ('剛', '才', 3.422),
        ('介', '面', 2.011),
    ];
    let mut bad = 0;
    println!("{:<12}{:>10}{:>10}", "字對", "Rust", "Python");
    for &(a, b, want) in cases {
        match lm.score(a, b) {
            Some(got) => {
                let ok = (got - want).abs() < 0.02;
                if !ok {
                    bad += 1;
                }
                println!(
                    "{a}{b}        {got:>10.3}{want:>10.3}  {}",
                    if ok { "✓" } else { "✗ 不一致" }
                );
            }
            None => {
                bad += 1;
                println!("{a}{b}        {:>10}{want:>10.3}  ✗ 查不到", "-");
            }
        }
    }

    // 異體字共存：`為` 我們的字形零命中，要靠 `爲` 借到資料
    println!("\n異體字共存（我們的字形在模型裡是零命中的）：");
    for (a, b) in [('因', '為'), ('上', '線'), ('大', '眾'), ('這', '裡')] {
        match lm.score(a, b) {
            Some(v) => println!("  {a}{b} = {v:.3} ✓"),
            None => {
                bad += 1;
                println!("  {a}{b} 查不到 ✗ 共存沒生效");
            }
        }
    }

    // 中國用語黑名單：這些要查不到（被主動擋掉）
    println!("\n中國用語黑名單（應該查不到）：");
    for (a, b) in [('剛', '纔'), ('界', '面')] {
        match lm.score(a, b) {
            None => println!("  {a}{b} 查不到 ✓"),
            Some(v) => {
                bad += 1;
                println!("  {a}{b} = {v:.3} ✗ 黑名單沒生效");
            }
        }
    }

    println!();
    if bad == 0 {
        println!("✓ 全部一致");
    } else {
        println!("✗ {bad} 項不一致");
        std::process::exit(1);
    }
}
