//! 台語包接進主輸入法的可行性驗證（§2.64.8 待決事項 1 的 (c) 選項）。
//!
//! # 這支在問什麼
//!
//! §2.64.9 量出資料可用，但那是**離線**的數字——證明得出按鍵、證明資料乾淨，
//! 沒有證明「台語詞真的會出現在候選裡」。這支把包真的載進引擎、真的送按鍵
//! 進去，看候選長什麼樣。
//!
//! 走的是既有的領域包那一層（§2.17），**一行引擎程式碼都沒改**。如果這支
//! 跑得出來，(c)「只當通譯的第四種語言」就不是需要重做架構的選項，而是
//! 「產一份包」而已。
//!
//! 用法：
//! ```text
//! cargo run --release -p ime-core --bin spike_taigi_pack -- <包所在資料夾> <包檔名>
//! ```

use ime_core::compose;
use ime_core::cutpoint::incremental::Incremental;
use ime_core::cutpoint::{normalize, rank};

fn data_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("core 的上層")
        .join("data")
}

/// 這串按鍵**第一格**的候選（第一名的切法之下）。
///
/// 選字是逐格進行的，多字詞由 `word_for`／`words_for` 整組換掉——所以
/// 台語詞會不會出現，看的就是第一格的候選清單裡有沒有它。
fn candidates(keys: &str, n: usize) -> Vec<String> {
    let cands = rank::sort(Incremental::from_keys(keys).cuttings());
    let segs = cands.first().map(|c| normalize(c)).unwrap_or_default();
    compose::compose(&segs)
        .first()
        .map(|slot| compose::candidates_for(slot).into_iter().take(n).collect())
        .unwrap_or_default()
}

/// 送這串按鍵，引擎第一名打出什麼。
fn text(keys: &str) -> String {
    let cands = rank::sort(Incremental::from_keys(keys).cuttings());
    let segs = cands.first().map(|c| normalize(c)).unwrap_or_default();
    compose::text_of(&compose::compose(&segs))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("用法: spike_taigi_pack <包資料夾> <包檔名>");
        std::process::exit(1);
    }
    let (dir_arg, pack_name) = (&args[0], &args[1]);

    let dir = data_dir();
    ime_core::pack::set_bundled_dir(dir.parent().map(|d| d.join("packs")));

    // ── 先不載台語包，記下「原本」的樣子 ──
    ime_core::pack::load(
        "__不存在的資料夾__",
        &[ime_core::pack::BUNDLED_SYMBOLS.to_string()],
    );
    ime_core::english::load(&dir);
    ime_core::dict::load_bopomofo(&dir);
    ime_core::lm::load(&dir, ime_core::dict::char_freq_map(&dir));
    ime_core::dict::load_japanese(&dir);

    // 幾個代表性的詞。**按鍵一律用 `mkkeys` 產**，不手敲——
    // 第一版手敲的八組全錯（CLAUDE.md 那條規矩不是說著玩的）
    let cases: &[(&str, &str)] = &[
        ("vu,4vu,4", "謝謝"),
        ("u3c.4", "以後"),
        ("vul3fu4", "小氣"),
        ("5;4q/6", "帳篷"),
        ("c93vul4", "海嘯"),
        ("a942; xl6", "麥當勞"),
        ("2j/ vu ", "東西"),
        ("qul4xu;4", "漂亮"),
    ];

    println!("═══ 載台語包之前 ═══");
    let mut before: Vec<Vec<String>> = Vec::new();
    for (keys, want) in cases {
        let c = candidates(keys, 8);
        println!("{want}（{keys}）→ {}", text(keys));
        println!("   候選: {}", c.join(" "));
        before.push(c);
    }

    // ── 載進台語包 ──
    let t0 = std::time::Instant::now();
    let n = ime_core::pack::load(dir_arg, std::slice::from_ref(pack_name));
    eprintln!("載入耗時 {:?}（{n} 條）", t0.elapsed());
    println!();
    println!("═══ 載入台語包：{n} 條 ═══");
    println!();
    for (i, (keys, want)) in cases.iter().enumerate() {
        let c = candidates(keys, 8);
        let changed = c != before[i];
        println!(
            "{want}（{keys}）→ {}  {}",
            text(keys),
            if changed {
                "★候選變了"
            } else {
                "（沒變）"
            }
        );
        println!("   候選: {}", c.join(" "));
        if changed {
            let new: Vec<&String> = c.iter().filter(|x| !before[i].contains(x)).collect();
            println!(
                "   新增: {}",
                new.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ")
            );
        }
    }
}
