//! 帳號類字串真的會走到「調整切法」那條路嗎？
//!
//! 兩個問題分開量：
//!   A 第一名切法就是整段 → 使用者沒有理由按 Tab（安全）
//!   B 第一名切歪了 → 使用者會按 Tab 修，於是寫進 learned.txt（會洩漏）
fn main() {
    ime_core::preload(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data")
            .as_path(),
        ime_core::config::Engines::default(),
    );
    // 分成兩類看：純英數帳號 vs 含數字的敏感串
    let groups: [(&str, &[&str]); 2] = [
        (
            "帳號／密碼（英數）",
            // **一律用假帳號**——這支會進公開快照，而
            // `publish-snapshot.ps1` 的檢查會擋下真實身分字串。
            // 形狀要保留（短英數、英文單字、含年份的點號式），
            // 那才是這支 spike 在量的東西
            &[
                "cool5432",
                "sampleuser",
                "john.doe1990",
                "user12345678",
                "abc123def",
                "mypassword2024",
                "a1b2c3d4",
                "qwerty123",
                "admin2024",
                "s3cr3tp4ss",
            ],
        ),
        (
            "純數字／證號（高敏感）",
            &[
                "0912345678",
                "A123456789",
                "4532015112830366",
                "0223456789",
                "123456",
                "987654321",
            ],
        ),
    ];
    for (name, samples) in groups {
        println!("\n━━━ {name} ━━━");
        println!("{:<20} {:>5}  第一名切法", "按鍵", "切法");
        println!("{}", "─".repeat(68));
        let (mut one_seg, mut total) = (0, 0);
        for k in samples {
            let inp = ime_core::input::Input::from_keys(k, None);
            let cuts = inp.cuttings();
            let first = cuts.first();
            let shown = first
                .map(|c| {
                    c.iter()
                        .map(|s| format!("{:?}:{}", s.lang, s.keys))
                        .collect::<Vec<_>>()
                        .join("｜")
                })
                .unwrap_or_default();
            let one = first.is_some_and(|c| c.len() == 1);
            if one {
                one_seg += 1;
            }
            total += 1;
            println!(
                "{:<20} {:>5}  {}{}",
                k,
                cuts.len(),
                shown,
                if one { "" } else { "  ⚠ 切歪了" }
            );
        }
        println!("→ {one_seg}/{total} 第一名就是整段（不會被按 Tab）");
    }
}
