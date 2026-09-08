//! **spike 工具（2026-09-07）**：把每句測資的候選切法連「組出來的文字」
//! 一起倒出來，給外部腳本重排。
//!
//! 為什麼要這支：要回答「中文 n-gram 能不能幫切法排序」，必須拿到帶語言
//! 標籤的候選**以及**每個候選組出來的文字——語言標籤決定哪一段是中文，
//! 文字才算得出 bigram 分數。`repl` 只印標籤不印文字，`check_compose`
//! 只看第一名。
//!
//! 輸出（TSV，給腳本讀）：
//! ```text
//! S <檔名> <文字期望> <按鍵期望> <按鍵序列>
//! C <名次> <段落與語言> <組出來的文字> <日文段的假名，以 / 分隔>
//! ```
//!
//! 用法：cargo run --release -p ime-core --bin spike_bigram_cut > out.tsv

use ime_core::compose;
use ime_core::cutpoint::incremental::Incremental;
use ime_core::cutpoint::{normalize, rank, Segment};

const TAB: char = '\t';
/// 每句最多倒幾個候選。中位候選數約 38，40 幾乎等於全部。
const TOP: usize = 40;

fn show(segs: &[Segment]) -> String {
    segs.iter()
        .map(|s| format!("{}:{}", s.lang.short(), s.keys.replace(' ', "␣")))
        .collect::<Vec<_>>()
        .join("|")
}

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

            // 跟 check_incremental 一樣：正規化後去重，名次算相異輸出
            let cands = rank::sort(Incremental::from_keys(keys).cuttings());
            let mut seen = std::collections::HashSet::new();
            let mut n = 0usize;
            for c in &cands {
                let norm = normalize(c);
                let sig = show(&norm);
                if !seen.insert(sig.clone()) {
                    continue;
                }
                let text = compose::text_of(&compose::compose(&norm));
                // 日文段的假名——要量「這串假名像不像日文」只能看假名，
                // 組出來的文字已經是漢字了
                let kana = norm
                    .iter()
                    .filter(|s| s.lang.short() == "日")
                    .map(|s| {
                        ime_core::romaji::kana::to_kana(&s.keys).unwrap_or_else(|| s.keys.clone())
                    })
                    .collect::<Vec<_>>()
                    .join("/");
                println!("C\t{n}\t{sig}\t{text}\t{kana}");
                n += 1;
                if n >= TOP {
                    break;
                }
            }
        }
    }
}
