//! **spike 工具（2026-09-07）**：把每句測資的**原始**候選切法（排序前
//! 不去重、不正規化）連 `Score` 的 12 個欄位一起倒出來，給外部腳本
//! 做「學權重」的實驗——`rank.rs` 現在是字典序（前一個欄位分得出高下
//! 就不看後面），要回答的是「換成加權和會不會更好」。
//!
//! 輸出（TSV）：
//! ```text
//! S <檔名> <文字期望> <按鍵期望> <按鍵序列>
//! C <名次> <12 個特徵，逗號分隔> <正規化後的段落與語言> <組出來的文字>
//! ```
//! 特徵順序＝`Score` 欄位順序（就是字典序的優先順序）。`fewer_*` 印的是
//! 原始計數（越小越好），`covered`／`dict_chars` 越大越好，`has_dict_word`
//! 是 0／1。名次是現行排序給的位置（0 = 第一名）。
//!
//! 用法：cargo run --release -p ime-core --bin spike_rank_features > out.tsv

use ime_core::compose;
use ime_core::cutpoint::incremental::Incremental;
use ime_core::cutpoint::{normalize, rank, Segment};

const TAB: char = '\t';
/// 每句最多倒幾個原始候選（原始候選比相異輸出多，放寬一點）
const TOP: usize = 200;

fn show(segs: &[Segment]) -> String {
    segs.iter()
        .map(|s| format!("{}:{}", s.lang.short(), s.keys.replace(' ', "␣")))
        .collect::<Vec<_>>()
        .join("|")
}

fn features(s: &rank::Score) -> String {
    [
        s.fewer_split_syllable.0,
        s.fewer_unreadable.0,
        s.fewer_swallowed.0,
        s.fewer_same_lang.0,
        s.fewer_passthrough.0,
        s.fewer_stolen.0,
        s.fewer_split_word.0,
        s.fewer_kana_bits.0,
        s.covered,
        usize::from(s.has_dict_word),
        s.fewer_segments.0,
        s.dict_chars,
    ]
    .iter()
    .map(|v| v.to_string())
    .collect::<Vec<_>>()
    .join(",")
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

            // 跟 sort 看到的一樣：原始候選、現行排序的順序
            let cands = rank::sort(Incremental::from_keys(keys).cuttings());
            for (n, c) in cands.iter().take(TOP).enumerate() {
                let sc = rank::score(c);
                let norm = normalize(c);
                let text = compose::text_of(&compose::compose(&norm));
                println!("C\t{n}\t{}\t{}\t{text}", features(&sc), show(&norm));
            }
        }
    }
}
