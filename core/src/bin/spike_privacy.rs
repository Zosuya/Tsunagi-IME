//! 隱私守門的兩個待決：**連續數字的門檻**，與**雜湊那條路**。
//!
//! 兩個都不能憑感覺——門檻訂太鬆漏掉驗證碼，訂太緊把正常句子擋掉。
//! 這支拿**真的測資**（1421 筆）量誤擋，不是拿想像的例子。
//!
//! ```text
//! cargo run --release -p ime-core --bin spike_privacy
//! ```

#[path = "common/testdata.rs"]
mod testdata;

/// 一串文字裡**最長的連續數字**有幾位。
fn max_digit_run(s: &str) -> usize {
    let (mut best, mut cur) = (0usize, 0usize);
    for c in s.chars() {
        if c.is_ascii_digit() {
            cur += 1;
            best = best.max(cur);
        } else {
            cur = 0;
        }
    }
    best
}

/// 敏感樣本：**這些一定要擋掉**。
const SENSITIVE: &[(&str, &str)] = &[
    ("手機", "0912345678"),
    ("市話", "0223456789"),
    ("身分證", "A123456789"),
    ("信用卡", "4532015112830366"),
    ("驗證碼6位", "123456"),
    ("驗證碼4位", "8274"),
    ("生日", "19900115"),
    ("郵遞區號+門牌", "10012"),
];

fn main() {
    let rows = testdata::load("spike_privacy");
    if rows.is_empty() {
        eprintln!("測資讀不到");
        std::process::exit(1);
    }

    println!("隱私守門：連續數字的門檻要訂多少？\n");
    println!("誤擋的判準：**正常測資的文字**被擋掉就是誤擋（那句話本來該學）\n");

    // ── 門檻掃描 ────────────────────────────────────────────
    println!(
        "{:<6} {:>10} {:>8}  擋掉的敏感樣本",
        "門檻", "誤擋句數", "誤擋率"
    );
    println!("{}", "─".repeat(72));

    for th in 3..=8usize {
        let hit: Vec<&testdata::Row> = rows
            .iter()
            .filter(|r| max_digit_run(&testdata::expected_text(&r.want)) >= th)
            .collect();
        let caught = SENSITIVE
            .iter()
            .filter(|(_, v)| max_digit_run(v) >= th)
            .count();
        println!(
            "{:<6} {:>10} {:>7.1}%  {}/{}",
            th,
            hit.len(),
            hit.len() as f64 * 100.0 / rows.len() as f64,
            caught,
            SENSITIVE.len()
        );
    }

    // ── 各門檻誤擋了什麼？──────────────────────────────────
    for th in [4usize, 5, 6] {
        let hit: Vec<&testdata::Row> = rows
            .iter()
            .filter(|r| max_digit_run(&testdata::expected_text(&r.want)) >= th)
            .collect();
        println!("\n─── 門檻 {th}：誤擋的 {} 句 ───", hit.len());
        for r in hit.iter().take(12) {
            println!("  [{}] {}", r.tag, testdata::expected_text(&r.want));
        }
        if hit.len() > 12 {
            println!("  …還有 {} 句", hit.len() - 12);
        }
    }

    // ── 敏感樣本逐條 ──────────────────────────────────────
    println!("\n─── 敏感樣本：各門檻擋不擋得住 ───");
    println!("{:<12} {:<20} {:>6}  門檻4/5/6", "類型", "內容", "最長串");
    println!("{}", "─".repeat(64));
    for (name, v) in SENSITIVE {
        let n = max_digit_run(v);
        println!(
            "{:<12} {:<20} {:>6}  {}  {}  {}",
            name,
            v,
            n,
            mark(n >= 4),
            mark(n >= 5),
            mark(n >= 6)
        );
    }
}

fn mark(b: bool) -> &'static str {
    if b {
        "擋"
    } else {
        "漏"
    }
}
