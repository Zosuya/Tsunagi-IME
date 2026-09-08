//! **spike 工具（2026-09-07）**：把每句測資的**格子與候選**倒出來，
//! 給外部腳本做整句維特比重排。
//!
//! 為什麼要這支：§2.53.5 的 85% 是**單點**量的（一個位置的正解對錯字），
//! 但實際上線是整句一起解，格子之間會互相牽動。要回答「整句維特比實際
//! 拿得到幾句」，必須拿到每一格的**全部候選**——`spike_bigram_cut` 只
//! 倒第一名的文字，`check_compose` 只看最後結果。
//!
//! 輸出（TSV）：
//! ```text
//! S <檔名> <文字期望> <按鍵期望> <按鍵序列>
//! G <格號> <語言> <可選字> <目前文字> <候選以\x1f分隔>
//! ```
//!
//! 用法：cargo run --release -p ime-core --bin spike_bigram_pick > picks.tsv

use ime_core::compose;
use ime_core::cutpoint::incremental::Incremental;
use ime_core::cutpoint::{normalize, rank};

const TAB: char = '\t';
/// 候選之間的分隔字元——候選本身可能含任何可見字元，用不可見的 US
const US: char = '\u{1f}';

fn main() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data");
    // 跟 check_compose 載一樣的東西，不然量到的行為跟使用者看到的不一樣
    ime_core::pack::set_bundled_dir(data.parent().map(|d| d.join("packs")));
    ime_core::pack::load(
        "__不存在的資料夾__",
        &[ime_core::pack::BUNDLED_SYMBOLS.to_string()],
    );
    ime_core::english::load(&data);
    ime_core::dict::load_bopomofo(&data);
    // 選字的中文 bigram——計分器一定要載，不然量到的是「關掉模型」的行為
    ime_core::lm::load(&data, ime_core::dict::char_freq_map(&data));
    ime_core::dict::load_japanese(&data);

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let files = [
        "mixed_daily",
        "mixed_otaku",
        "mixed_holdout",
        "mixed_trilingual",
        "mixed_japanese_verbs",
        "mixed_ja_en",
        "mixed_cutpoint",
        "mixed_kana_fragment",
        "mixed_en_bopomofo",
        "mixed_en_split",
        "mixed_en_vowel",
        "mixed_symbol",
        "mixed_long",
        "bopomofo_words",
        "bopomofo_sentences",
    ];

    for f in files {
        let Ok(content) = std::fs::read_to_string(dir.join(format!("{f}.txt"))) else {
            continue;
        };
        for line in content.lines() {
            let line = line.trim_end_matches(['\u{d}', '\u{a}']);
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let cols: Vec<&str> = line.split(TAB).collect();
            if cols.len() < 3 {
                continue;
            }
            let (want_col, key_expect, keys) = (cols[0], cols[1], cols[2]);
            println!("S\t{f}\t{want_col}\t{key_expect}\t{keys}");

            // 第一名的切法——跟 check_compose 走同一條路
            let cands = rank::sort(Incremental::from_keys(keys).cuttings());
            let Some(best) = cands.first() else { continue };
            let norm = normalize(best);
            let slots = compose::compose(&norm);
            for (i, s) in slots.iter().enumerate() {
                let cs = compose::candidates_for(s);
                let joined = cs.join(&US.to_string());
                println!(
                    "G\t{i}\t{}\t{}\t{}\t{}",
                    s.lang.short(),
                    if s.selectable { 1 } else { 0 },
                    s.text,
                    joined
                );
            }
        }
    }
}
