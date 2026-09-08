use ime_core::cutpoint::{incremental::Incremental, rank, Segment};
fn show(c: &[Segment]) -> String {
    c.iter()
        .map(|s| format!("{}:{}", s.lang.short(), s.keys.replace(' ', "␣")))
        .collect::<Vec<_>>()
        .join(" | ")
}
fn main() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data");
    ime_core::english::load(&data);
    ime_core::dict::load_bopomofo(&data);
    ime_core::dict::load_japanese(&data);
    for (keys, want) in [
        ("fileshitsureicl3", vec!["file", "shitsurei", "cl3"]),
        ("wakarimashitafiled9 ", vec!["wakarimashita", "file", "d9 "]),
    ] {
        let cands = rank::sort(Incremental::from_keys(keys).cuttings());
        let r = cands
            .iter()
            .position(|c| c.iter().map(|s| s.keys.clone()).collect::<Vec<_>>() == want);
        println!("=== {keys} ===");
        println!("  正解排第 {:?}／{} 種", r.map(|x| x + 1), cands.len());
        println!("  第一名 {}", show(&cands[0]));
        if let Some(i) = r {
            println!(
                "  正解   {}  分數 {:?}",
                show(&cands[i]),
                rank::score(&cands[i])
            );
        }
        println!("  第一名分數 {:?}", rank::score(&cands[0]));
    }
}
