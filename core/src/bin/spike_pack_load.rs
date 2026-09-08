//! Spike：emoji 全量規模的包，載入要多久？
//!
//! **這是可行性測試，不是產品程式碼。**
//!
//! CLAUDE.md 記著「包用純文字發布，不要二進位」（2026-09-01），依據是
//! 1000 條 0.5ms／10000 條 2.5ms。但那次量的是**詞**（一列一個詞），
//! emoji 包是**一列多個名字＋多個符號**，解析成本不一樣，而且全量
//! 1800 組展開成別名之後列數會膨脹。這裡重量一次，確認那個結論在新的
//! 規模下還成不成立。
//!
//! 用法：cargo run --release -p ime-core --bin spike_pack_load -- <包資料夾> <包名>

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (dir, name) = match (args.get(1), args.get(2)) {
        (Some(d), Some(n)) => (d.clone(), n.clone()),
        _ => {
            eprintln!("用法：spike_pack_load <包資料夾> <包名>");
            return;
        }
    };

    let path = std::path::Path::new(&dir).join(format!("{name}.txt"));
    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    println!(
        "包：{}  ({:.1} KB)\n",
        path.display(),
        bytes as f64 / 1024.0
    );

    // 冷啟動（第一次讀，檔案不在 OS cache 裡的情況不好模擬，這裡量的是熱的）
    let enabled = vec![name.clone()];

    // 量十次取中位數——單次量測會被排程雜訊蓋過
    let mut times = Vec::new();
    for _ in 0..10 {
        let t = std::time::Instant::now();
        ime_core::pack::load(&dir, &enabled);
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let info = ime_core::pack::index();
    println!("載入結果：sym {} 個名字", info.sym.len());
    println!(
        "載入耗時：中位 {:.2} ms  最快 {:.2} ms  最慢 {:.2} ms",
        times[5], times[0], times[9]
    );
    println!("\n（對照：CLAUDE.md 記的系統詞庫文字版 74 萬條 = 705ms）");
}
