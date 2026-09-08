//! 診斷：問 `romaji::inflect` 某串羅馬字是不是活用形、還原成什麼辭書形。
//!
//! 用法：dbg_inflect <羅馬字>...      直接問幾個
//!       dbg_inflect --file <清單>    一行一個
use ime_core::romaji::inflect;

fn main() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data");
    ime_core::dict::load_japanese(&data);
    ime_core::dict::load_bopomofo(&data);
    ime_core::english::load(&data);
    eprintln!("詞典載入：{}", ime_core::dict::japanese_loaded());

    let args: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<String> = if args.first().map(String::as_str) == Some("--file") {
        std::fs::read_to_string(&args[1])
            .expect("讀不到清單")
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect()
    } else {
        args
    };
    let (mut hit, mut n) = (0usize, 0usize);
    for w in &words {
        n += 1;
        match inflect::辭書形(w) {
            Some(d) => {
                hit += 1;
                let same = if d == *w { "（辭書形本身）" } else { "" };
                println!("  ✓ {w:<20} → {d}{same}");
            }
            None => println!("  ✗ {w:<20} 不是活用形"),
        }
    }
    println!("\n  {hit}/{n} 認得出");
}
