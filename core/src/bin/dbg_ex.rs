//! 稽核：`iru`／`eru` 結尾的**五段**動詞（例外表該收的那批），
//! 現在的活用形組得對嗎？
//!
//! 判準：五段的過去形是促音便（帰った），一段是直接接（変えた）。
//! 拿兩種按鍵各問一次，看組出來的是不是那個動詞。
use ime_core::romaji::inflect;

/// 日文教學裡標準的「例外動詞」清單（iru/eru 結尾但屬五段）。
/// 每筆：(辭書形羅馬字, 漢字, 促音便活用形的按鍵)
const 例外: &[(&str, &str, &str)] = &[
    ("kaeru", "帰る", "kaetta"),
    ("hasiru", "走る", "hasitta"),
    ("hairu", "入る", "haitta"),
    ("kiru", "切る", "kitta"),
    ("siru", "知る", "sitta"),
    ("iru", "要る", "itta"),
    ("suberu", "滑る", "subetta"),
    ("keru", "蹴る", "ketta"),
    ("neru", "練る", "netta"),
    ("heru", "減る", "hetta"),
    ("meiru", "滅入る", "meitta"),
    ("kagiru", "限る", "kagitta"),
    ("tiru", "散る", "titta"),
    ("maziru", "混じる", "mazitta"),
    ("mairu", "参る", "maitta"),
    ("nigiru", "握る", "nigitta"),
    ("syaberu", "喋る", "syabetta"),
    ("hineru", "捻る", "hinetta"),
    ("kudaru", "下る", "kudatta"),
    ("azakeru", "嘲る", "azaketta"),
    ("yogiru", "過る", "yogitta"),
    ("hoteru", "火照る", "hotetta"),
    // 表裡沒有、但同樣是有名的例外——看現在處理得如何
    ("kaziru", "齧る", "kazitta"),
    ("nezziru", "捻じる", "nezzitta"),
    ("hasyaru", "はしゃる", "hasyatta"),
    ("kutinaru", "朽ちる", "kutinatta"),
    ("suberikomu", "滑り込む", "suberikonnda"),
    ("iru2", "煎る", "itta"),
    ("teru", "照る", "tetta"),
    ("neziru", "捩る", "nezitta"),
    ("hoziru", "穿る", "hozitta"),
    ("kubiru", "縊る", "kubitta"),
    ("iziru", "弄る", "izitta"),
    ("kutugaeru", "覆る", "kutugaetta"),
    ("yomigaeru", "蘇る", "yomigaetta"),
    ("hirugaeru", "翻る", "hirugaetta"),
    ("aseru", "焦る", "asetta"),
    ("kosuru", "擦る", "kosutta"),
    ("nonosiru", "罵る", "nonositta"),
    ("kagayaru", "耀る", "kagayatta"),
];

fn main() {
    let d = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data");
    ime_core::dict::load_japanese(&d);
    let (mut ok, mut bad, mut 無詞典) = (0, 0, 0);
    for (辭書, 漢字, 促音鍵) in 例外 {
        let 在詞典 = ime_core::dict::is_japanese_word(辭書);
        let 候選 = inflect::漢字候選(促音鍵);
        // 語幹＝漢字去掉尾端假名
        let 幹: String = {
            let n = 漢字
                .chars()
                .rev()
                .take_while(|c| ('\u{3040}'..='\u{30ff}').contains(c))
                .count();
            漢字.chars().take(漢字.chars().count() - n).collect()
        };
        let 中 = 候選.iter().any(|w| w.starts_with(&幹));
        if !在詞典 {
            無詞典 += 1;
            println!("  ? {辭書:<12} {漢字:<6} 詞典沒收這個辭書形");
        } else if 中 {
            ok += 1;
            println!("  ✓ {辭書:<12} {漢字:<6} {促音鍵:<12} → {:?}", 候選.first());
        } else {
            bad += 1;
            println!(
                "  ✗ {辭書:<12} {漢字:<6} {促音鍵:<12} 候選裡沒有「{幹}」：{:?}",
                候選.iter().take(4).collect::<Vec<_>>()
            );
        }
    }
    println!("\n  ✓{ok}  ✗{bad}  ?{無詞典}（共 {} 筆）", 例外.len());
}
