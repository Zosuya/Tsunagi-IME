//! spike：韓文按鍵串跟注音／日文撞不撞？
//!
//! # 要驗證什麼
//!
//! 評估韓文時我先入為主地說「두벌식佈局跟大千每顆鍵都衝突，所以自動
//! 判斷失效」——使用者指出那些韓文例子在注音裡根本不合法。到底撞不撞
//! 要問引擎，不是憑推論。
//!
//! 這支拿常見的韓文按鍵串（두벌식打出來的）去問三個現有引擎：注音收不
//! 收？日文收不收？收了的話切成什麼？
//!
//! # 用法
//!
//! ```text
//! cargo run --release -p ime-core --bin spike_hangul_clash
//! ```

use ime_core::input::Input;

#[path = "common/testdata.rs"]
mod testdata;

/// 두벌식（Dubeolsik）佈局：按鍵 → 韓文字母。
///
/// 標準的兩套式鍵盤，左手區是子音、右手區是母音。Shift 的那排
/// （ㅃㅉㄸㄲㅆ、ㅒㅖ）先不列——判斷「撞不撞」用不到。
const DUBEOLSIK: &[(char, &str)] = &[
    ('q', "ㅂ"),
    ('w', "ㅈ"),
    ('e', "ㄷ"),
    ('r', "ㄱ"),
    ('t', "ㅅ"),
    ('y', "ㅛ"),
    ('u', "ㅕ"),
    ('i', "ㅑ"),
    ('o', "ㅐ"),
    ('p', "ㅔ"),
    ('a', "ㅁ"),
    ('s', "ㄴ"),
    ('d', "ㅇ"),
    ('f', "ㄹ"),
    ('g', "ㅎ"),
    ('h', "ㅗ"),
    ('j', "ㅓ"),
    ('k', "ㅏ"),
    ('l', "ㅣ"),
    ('z', "ㅋ"),
    ('x', "ㅌ"),
    ('c', "ㅊ"),
    ('v', "ㅍ"),
    ('b', "ㅠ"),
    ('n', "ㅜ"),
    ('m', "ㅡ"),
];

/// 常見韓文詞的按鍵串（두벌식）與它該打出來的字。
const SAMPLES: &[(&str, &str)] = &[
    ("dkssud", "안녕"),
    ("gktpdy", "하세요"),
    ("dkssudgktpdy", "안녕하세요"),
    ("gksrnr", "한국"),
    ("ekfms", "다른"),
    ("tkfkd", "사랑"),
    ("rkatkgkqslek", "감사합니다"),
    ("dhsmf", "오늘"),
    ("skfTl", "날씨"),
    ("wpdy", "제요"),
    ("aksskt", "만났"),
    ("gkrry", "학교"),
    ("tjfua", "서렴"),
    ("dlfnrl", "일우기"),
    ("qkdqjq", "밥법"),
    ("wjsghk", "전화"),
    ("tlrtk", "식사"),
    ("dizy", "아캐"),
    ("rjaek", "검다"),
    ("ahfmrpTdj", "모르게썼"),
];

fn main() {
    testdata::load_engine();

    println!("韓文按鍵串在通譯引擎裡是什麼？\n");
    println!(
        "{:<16} {:<12} {:<7} {:<7}  {}",
        "按鍵", "韓文", "注音?", "日文?", "引擎第一名的切法"
    );
    println!("{}", "─".repeat(84));

    let mut zh_ok = 0usize;
    let mut ja_ok = 0usize;
    let mut all_en = 0usize;

    for (keys, hangul) in SAMPLES {
        let zh = ime_core::bopomofo::validity(keys) == ime_core::bopomofo::Validity::Valid;
        let ja = ime_core::romaji::validity(keys) == ime_core::romaji::Validity::Valid;
        zh_ok += usize::from(zh);
        ja_ok += usize::from(ja);

        let input = Input::from_keys(keys, None);
        let cut = input
            .cuttings()
            .first()
            .map(|c| {
                c.iter()
                    .map(|s| format!("{}:{}", mark(s.lang), s.keys))
                    .collect::<Vec<_>>()
                    .join("｜")
            })
            .unwrap_or_default();
        // 整串都判成英文＝「三個引擎都不認得」，那正是自動判斷分得出來
        // 韓文的情況
        let pure_en = input.cuttings().first().is_some_and(|c| {
            c.iter()
                .all(|s| s.lang == ime_core::language::Language::English)
        });
        all_en += usize::from(pure_en);

        println!(
            "{:<16} {:<12} {:<7} {:<7}  {}",
            keys,
            hangul,
            if zh { "合法" } else { "─" },
            if ja { "合法" } else { "─" },
            cut
        );
    }

    let n = SAMPLES.len();
    println!("\n{n} 串韓文按鍵：");
    println!("  整串是合法注音的  {zh_ok}");
    println!("  整串是合法日文的  {ja_ok}");
    println!("  切完整串都是英文  {all_en}（＝三個引擎都不認得，分得出來）");

    println!("\n─── 反向：注音／日文的按鍵串在韓文眼裡是什麼 ───\n");
    for keys in [
        "su3",
        "su3cl3",
        "rup wu0 ",
        "cl3t ",
        "sushi",
        "wakarimashita",
        "config",
    ] {
        println!("  {:<16} → {}", keys, to_hangul_letters(keys));
    }
}

/// 按鍵串照두벌식轉成韓文字母（不組音節，只看字母序列）。
///
/// 組不組得成音節是另一回事——這裡只要看「這串按鍵在韓文鍵盤上
/// 打出來像不像話」。
fn to_hangul_letters(keys: &str) -> String {
    keys.chars()
        .map(|c| {
            DUBEOLSIK
                .iter()
                .find(|(k, _)| *k == c)
                .map(|(_, v)| *v)
                .unwrap_or("·")
                .to_string()
        })
        .collect()
}

fn mark(l: ime_core::language::Language) -> &'static str {
    use ime_core::language::Language;
    match l {
        Language::Bopomofo => "注",
        Language::Romaji => "日",
        Language::English => "英",
    }
}
