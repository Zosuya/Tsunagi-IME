//! **診斷工具**：給一組按鍵，把每個候選切法的 12 個 `Score` 欄位攤開，
//! 並標出「期望的切法」排第幾、輸在哪一欄。
//!
//! 用法：cargo run --release -p ime-core --bin dbg_rank -- <按鍵> [期望切法]
//!   期望切法用 `|` 分隔按鍵段，例如 `logout|yl3`。
//!   給了期望就只印第一名與期望那兩列，並指出第一個分出高下的欄位。

use ime_core::compose;
use ime_core::cutpoint::incremental::Incremental;
use ime_core::cutpoint::{normalize, rank, Segment};

/// 欄位名稱，順序＝`Score` 的比較順序
const NAMES: [&str; 12] = [
    "split_syl",
    "unread",
    "swallow",
    "same_lang",
    "passthru",
    "stolen",
    "split_word",
    "kana_bits",
    "covered",
    "has_dict",
    "fewer_seg",
    "dict_chars",
];

fn vals(s: &rank::Score) -> [usize; 12] {
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
}

/// 欄位是「越大越好」嗎（covered／has_dict／dict_chars 是，其餘的是 Reverse）
fn bigger_better(i: usize) -> bool {
    matches!(i, 8 | 9 | 11)
}

fn show(segs: &[Segment]) -> String {
    segs.iter()
        .map(|s| format!("{}:{}", s.lang.short(), s.keys.replace(' ', "␣")))
        .collect::<Vec<_>>()
        .join("|")
}

fn line(rank_no: usize, c: &[Segment]) -> String {
    let sc = rank::score(c);
    let v = vals(&sc);
    let nums = v
        .iter()
        .zip(NAMES)
        .map(|(x, n)| format!("{n}={x}"))
        .collect::<Vec<_>>()
        .join(" ");
    let norm = normalize(c);
    let text = compose::text_of(&compose::compose(&norm));
    // 計分看的是**合併前**的段落，顯示看的是合併後——兩個都印，不然對不上
    format!(
        "  #{rank_no:<3} {text}\n       原 {}\n       正 {}\n       {nums}",
        show(c),
        show(&norm)
    )
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

    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(keys) = args.first() else {
        eprintln!("用法：dbg_rank <按鍵> [期望切法，用 | 分隔]");
        std::process::exit(1);
    };
    let want: Option<Vec<String>> = args
        .get(1)
        .map(|w| w.split('|').map(|s| s.replace('␣', " ")).collect());

    let cands = rank::sort(Incremental::from_keys(keys).cuttings());
    println!("=== {keys} （{} 種候選）===", cands.len());

    let Some(want) = want else {
        for (n, c) in cands.iter().take(10).enumerate() {
            println!("{}", line(n + 1, c));
        }
        return;
    };

    let pos = cands.iter().position(|c| {
        normalize(c)
            .iter()
            .map(|s| s.keys.clone())
            .collect::<Vec<_>>()
            == want
    });
    println!("  期望 {want:?}");
    println!("{}", line(1, &cands[0]));
    match pos {
        None => println!("  ！期望的切法根本沒被生出來"),
        Some(i) => {
            println!("{}", line(i + 1, &cands[i]));
            // 第一個分出高下的欄位就是輸掉的原因
            let (a, b) = (vals(&rank::score(&cands[0])), vals(&rank::score(&cands[i])));
            for k in 0..12 {
                if a[k] != b[k] {
                    let win = if bigger_better(k) {
                        a[k] > b[k]
                    } else {
                        a[k] < b[k]
                    };
                    println!(
                        "\n  → 輸在第 {} 欄 `{}`：第一名 {} vs 期望 {}（第一名{}）",
                        k + 1,
                        NAMES[k],
                        a[k],
                        b[k],
                        if win {
                            "勝"
                        } else {
                            "敗，但前面已分勝負？"
                        }
                    );
                    break;
                }
            }
        }
    }
}
