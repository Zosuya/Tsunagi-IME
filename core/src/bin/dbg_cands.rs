//! 診斷：一段按鍵在選字層的**候選清單**（使用者按選字鍵看到的東西）。
use ime_core::{
    compose,
    cutpoint::{incremental::Incremental, normalize, rank},
};
fn main() {
    let d = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data");
    ime_core::pack::set_bundled_dir(d.parent().map(|x| x.join("packs")));
    ime_core::pack::load("__無__", &[ime_core::pack::BUNDLED_SYMBOLS.to_string()]);
    ime_core::english::load(&d);
    ime_core::dict::load_bopomofo(&d);
    ime_core::lm::load(&d, ime_core::dict::char_freq_map(&d));
    ime_core::dict::load_japanese(&d);
    for keys in std::env::args().skip(1) {
        let cuts = rank::sort(Incremental::from_keys(&keys).cuttings());
        let slots = compose::compose(&normalize(&cuts[0]));
        println!("=== {keys} → 「{}」 ===", compose::text_of(&slots));
        for (i, s) in slots.iter().enumerate() {
            let c = compose::candidates_for(s);
            println!(
                "  格{i} keys={:<14} 顯示={:<10} 候選({})={:?}",
                s.keys,
                s.text,
                c.len(),
                c.iter().take(10).collect::<Vec<_>>()
            );
        }
    }
}
