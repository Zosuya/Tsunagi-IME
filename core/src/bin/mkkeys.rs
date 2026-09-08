//! 把詞轉成按鍵序列，用來製作測資。
//!
//! # 為什麼一定要有這支
//!
//! 按鍵序列**不准手敲**。舊的 100 組測資就是手敲注音毀掉的——多音字
//! 判讀錯，而錯的期望會把計分器帶偏，修引擎去迎合錯的答案只會愈修愈壞
//! （見 `期望基準審核.md` §0 的第四種標記「測資錯」）。
//!
//! 所以規矩是：**我提供詞，工具查詞庫產按鍵，再立刻丟回引擎驗證**，
//! 驗證過的才准進測資。
//!
//! # 三種語言各自怎麼產
//!
//! | 語言 | 資料源 | 產法 |
//! |---|---|---|
//! | 中文 | `BPMFMappings.txt`（詞）＋ `BPMFBase.txt`（單字） | 查注音 → 音節換按鍵；一詞多讀音全試 |
//! | 日文 | mozc `dictionary0*.txt` | 表記反查平假名 → 假名換羅馬字 |
//! | 英文 | 無 | passthrough，打什麼是什麼，不需要工具 |
//!
//! 中文的音節→按鍵對照**不手寫**：`BPMFBase.txt` 第 2、4 欄本來就是
//! 「注音音節」與「按鍵」，開機時掃一遍建表（1417 條，實測詞表用到的
//! 音節 100% 涵蓋、無衝突）。舊版手抄 41 條注音符號對照表，那是多餘的
//! 出錯機會。
//!
//! # 驗證走的是完整那條路
//!
//! `Incremental::from_keys → rank::sort → compose → text_of`，跟
//! `check_compose` 同一條鏈。所以這支說「會過」，計分器就會過——
//! 兩邊不會各說各話。
//!
//! 用法：
//! ```text
//! cargo run --release -p ime-core --bin mkkeys -- 今天 天氣 很好
//! cargo run --release -p ime-core --bin mkkeys -- --ja 寿司 金曜日
//! cargo run --release -p ime-core --bin mkkeys -- --file 詞表.txt
//! ```
//! 加 `--tsv` 只印按鍵無誤的，而且是可直接貼進測資的三欄格式。
//!
//! # 三種記號
//!
//! | 記號 | 意思 | 進不進測資 |
//! |---|---|---|
//! | `OK` | 按鍵對，引擎現在就打得回原詞 | 進 |
//! | `△` | 按鍵對，但引擎現在排序輸了（`階`→「回」） | **進**，讓它紅著＝需求 |
//! | `✗` | 按鍵本身可疑，讀音挑錯了 | **不進**，要人工確認 |
//!
//! 舊版只有 `OK`／`✗` 兩種，把這兩件事混在一起——結果是查不到常用讀音時
//! 悄悄拿檔案裡的第一個（`台`→`うてな`），還印成跟「引擎做不到」一樣的
//! `✗`，人工復核時看不出差別。判準見 `ja_keys_sound`。
//!
//! # 句型模式（`--sent`）
//!
//! 一行一句、段用 `|` 分隔，工具逐段產鍵：
//!
//! ```text
//! 3|個|_|bug          →  3|3ek7| |bug        3 3ek7 bug
//! zh:今天|_|meeting   →  rup wu0 | |meeting  rup wu0  meeting
//! ```
//!
//! **中日共用漢字分不出來**（`寿司` 是日文、`壽司` 是中文），純漢字段
//! 要標 `zh:` 或 `ja:`；有假名、純 ASCII 的段自動判得出來。
//!
//! 為什麼需要這個模式：`--tsv` 的第二欄是拿**引擎自己的第一名切法**
//! 填的，要寫「引擎現在會答錯」的測資時那等於拿錯答案當期望。句型模式
//! 讓結構由人指定，第二欄反映的是需求不是現況。

use ime_core::compose;
use ime_core::cutpoint::incremental::Incremental;
use ime_core::cutpoint::{normalize, rank};
use std::collections::HashMap;

const TAB: char = '\u{9}';

/// 資料目錄。`CARGO_MANIFEST_DIR` 是 `core/`，資料在它的上一層。
fn data_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("core 的上層")
        .join("data")
}

// ───────────────────────── 中文 ─────────────────────────

/// 注音音節 → 按鍵。從 `BPMFBase.txt` 建，不手寫。
///
/// 格式是固定 5 欄：`字 注音 拼音 按鍵 編碼`，取第 2、4 欄。
/// 一聲的按鍵欄**不含結尾空白**（`書 ㄕㄨ shu gj`），補空白是
/// `syllable_keys` 的事。
fn build_syllable_map(dir: &std::path::Path) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Ok(content) = std::fs::read_to_string(dir.join("bopomofo").join("BPMFBase.txt")) else {
        eprintln!("找不到 BPMFBase.txt");
        return out;
    };
    for line in content.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() == 5 {
            out.entry(f[1].to_string())
                .or_insert_with(|| f[3].to_string());
        }
    }
    out
}

/// 單字 → 按鍵。同樣從 `BPMFBase.txt` 建，取第 1、4 欄。
///
/// **詞表只收「詞」**——`個`／`年`／`點` 這些常用單字在 `BPMFMappings`
/// 裡查不到（`銀行` 有、單獨的 `行` 沒有），逐字拼的時候一定要有這份
/// 退路。多音字取檔案裡的第一個讀音。
fn build_char_map(dir: &std::path::Path) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Ok(content) = std::fs::read_to_string(dir.join("bopomofo").join("BPMFBase.txt")) else {
        return out;
    };
    for line in content.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() == 5 {
            let key = if f[1].ends_with(['ˊ', 'ˇ', 'ˋ', '˙']) {
                f[3].to_string()
            } else {
                // 一聲要補結尾空白，跟 `syllable_keys` 同一條規則
                format!("{} ", f[3])
            };
            out.entry(f[0].to_string()).or_insert(key);
        }
    }
    out
}

/// 表記 → 這個詞的所有讀音（每個讀音是一串音節）。
///
/// **同一個詞可能有多行**（`一丁不識` 有 3 種讀音），全部收下來後面
/// 一一試——舊版只取第一行，可能挑到冷門讀音而產出打不出原詞的按鍵。
fn build_word_map(dir: &std::path::Path) -> HashMap<String, Vec<Vec<String>>> {
    let mut out: HashMap<String, Vec<Vec<String>>> = HashMap::new();
    let Ok(content) = std::fs::read_to_string(dir.join("bopomofo").join("BPMFMappings.txt")) else {
        eprintln!("找不到 BPMFMappings.txt");
        return out;
    };
    for line in content.lines() {
        let mut it = line.split_whitespace();
        let Some(word) = it.next() else { continue };
        let syls: Vec<String> = it.map(String::from).collect();
        if syls.is_empty() {
            continue;
        }
        let e = out.entry(word.to_string()).or_default();
        if !e.contains(&syls) {
            e.push(syls);
        }
    }
    out
}

/// 一串注音音節換成按鍵。
///
/// **一聲要補結尾空白**——`BPMFBase` 的按鍵欄不含它，而引擎靠聲調鍵
/// （`3467` 或空白）判斷音節結束。漏掉的話整串會黏成一個音節。
fn syllable_keys(syls: &[String], map: &HashMap<String, String>) -> Option<String> {
    let mut out = String::new();
    for s in syls {
        let k = map.get(s)?;
        out.push_str(k);
        // 沒有聲調符號 = 一聲，補空白
        if !s.ends_with(['ˊ', 'ˇ', 'ˋ', '˙']) {
            out.push(' ');
        }
    }
    Some(out)
}

/// 中文詞的所有候選按鍵串（一讀音一串）。查不到回空。
fn zh_keys(
    word: &str,
    words: &HashMap<String, Vec<Vec<String>>>,
    syl: &HashMap<String, String>,
    chars: &HashMap<String, String>,
) -> Vec<String> {
    // 先查整詞
    if let Some(readings) = words.get(word) {
        let v: Vec<String> = readings
            .iter()
            .filter_map(|r| syllable_keys(r, syl))
            .collect();
        if !v.is_empty() {
            return v;
        }
    }
    // 整詞查不到就逐字拼。
    //
    // **單字要查 `BPMFBase`（`chars`）而不是詞表**——詞表只收「詞」，
    // 「個」「年」「點」這些常用單字在裡面根本查不到（`行` 也是：
    // `銀行` 有、單獨的 `行` 沒有）。第一版就是漏了這條，句型模式一測
    // 就全滅。多音字取第一個讀音，要別的讀音就整詞寫進句型裡。
    let mut acc: Vec<String> = Vec::new();
    for ch in word.chars() {
        let s = ch.to_string();
        let k = words
            .get(&s)
            .and_then(|r| r.first())
            .and_then(|r| syllable_keys(r, syl))
            .or_else(|| chars.get(&s).cloned());
        let Some(k) = k else {
            return Vec::new();
        };
        acc.push(k);
    }
    if acc.is_empty() {
        Vec::new()
    } else {
        vec![acc.concat()]
    }
}

// ───────────────────────── 日文 ─────────────────────────

/// 表記 → 讀音（平假名），**按 mozc 的 cost 由小到大排**。
///
/// 辭書格式：`讀音<TAB>左id<TAB>右id<TAB>cost<TAB>表記`。
/// `check_testdata` 有一份類似的表，但那份只收含漢字的表記；測資也要
/// 純假名詞（`ラーメン`／`ください`），所以這裡不濾。
///
/// # 為什麼一定要按 cost 排
///
/// mozc 的行序不代表常用度。單漢字量詞尤其毒：`台` 的三種讀音裡
/// `うてな`（植物學的花萼）排在檔案前面，`杯` 的 `さかずき`、`階` 的
/// `きざはし` 也一樣。這些讀音**打不回原詞**，於是 `pick_readable`
/// 掉進「都不行就拿第一個」的退路，把最冷門的那個寫進測資——
/// 「2 台」變成「2うてな」就是這麼來的。
///
/// cost 是 mozc 自己算的成本，越小越常用。實測 `台`／`杯`／`階`／
/// `頭`／`匹`／`人`／`本`／`羽` 排序後第一名全是正確的量詞讀音，
/// 冷門讀音全被推到後面。
fn build_ja_map(dir: &std::path::Path) -> HashMap<String, Vec<String>> {
    // 先收 (讀音, 最小 cost)——同一個讀音可能有多行（不同詞性），取最小的
    let mut acc: HashMap<String, Vec<(String, i64)>> = HashMap::new();
    for i in 0..10 {
        let path = dir.join("japanese").join(format!("dictionary{i:02}.txt"));
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        for line in content.lines() {
            let f: Vec<&str> = line.split(TAB).collect();
            let (Some(reading), Some(surface)) = (f.first(), f.get(4)) else {
                continue;
            };
            // cost 欄壞掉就當最大，排到最後而不是整行丟掉
            let cost = f
                .get(3)
                .and_then(|c| c.parse::<i64>().ok())
                .unwrap_or(i64::MAX);
            let e = acc.entry((*surface).to_string()).or_default();
            match e.iter_mut().find(|(r, _)| r == reading) {
                Some((_, c)) => *c = (*c).min(cost),
                None => e.push(((*reading).to_string(), cost)),
            }
        }
    }
    acc.into_iter()
        .map(|(surface, mut rs)| {
            // cost 相同時按讀音排，結果才不會因為 HashMap 的順序而漂動
            rs.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
            (surface, rs.into_iter().map(|(r, _)| r).collect())
        })
        .collect()
}

/// 假名 → 羅馬字。反查 `mora_to_kana` 的表，取**最短**的拼法。
///
/// 表是多對一（`si`/`shi` 都轉成「し」），取最短是為了穩定：同一個
/// 假名每次都產出同一串按鍵，測資才不會因為重跑而漂動。
///
/// 這串 mora 清單是 `romaji::kana::MORA_TABLE` 的鍵——那張表沒有公開
/// 迭代器，只能逐一問。清單漏了哪一條不會產生錯的按鍵，只會讓那個
/// 假名查不到而放棄（`kana_keys` 回 `None`），是安全的失敗方向。
fn kana_to_mora(kana: &str) -> Option<&'static str> {
    #[rustfmt::skip]
    const MORAS: &[&str] = &[
        "a", "i", "u", "e", "o",
        "ka", "ki", "ku", "ke", "ko", "sa", "si", "su", "se", "so",
        "ta", "ti", "tu", "te", "to", "na", "ni", "nu", "ne", "no",
        "ha", "hi", "hu", "he", "ho", "ma", "mi", "mu", "me", "mo",
        "ya", "yu", "yo", "ra", "ri", "ru", "re", "ro", "wa", "wo",
        "ga", "gi", "gu", "ge", "go", "za", "zi", "zu", "ze", "zo",
        "da", "di", "du", "de", "do", "ba", "bi", "bu", "be", "bo",
        "pa", "pi", "pu", "pe", "po",
        "shi", "chi", "tsu", "fu", "ji",
        "kya", "kyu", "kyo", "sha", "shu", "sho", "sya", "syu", "syo",
        "cha", "chu", "cho", "tya", "tyu", "tyo", "nya", "nyu", "nyo",
        "hya", "hyu", "hyo", "mya", "myu", "myo", "rya", "ryu", "ryo",
        "gya", "gyu", "gyo", "ja", "ju", "jo", "jya", "jyu", "jyo",
        "zya", "zyu", "zyo", "bya", "byu", "byo", "pya", "pyu", "pyo",
        "fa", "fi", "fe", "fo", "va", "vi", "vu", "ve", "vo",
        "we", "wi", "ye", "je", "che", "she",
        "xa", "xi", "xu", "xe", "xo", "xya", "xyu", "xyo", "xtu", "xwa",
        "la", "li", "lu", "le", "lo",
    ];
    MORAS
        .iter()
        .filter(|m| ime_core::romaji::kana::mora_to_kana(m) == Some(kana))
        .min_by_key(|m| m.len())
        .copied()
}

/// 平假名 → 羅馬字按鍵。
///
/// **不建整張反轉表**——多對一的關係反過來查會有多個答案，挑錯就產出
/// 打不出原詞的按鍵。改成：照本專案的拼法規則直接拼，拼完丟回
/// `romaji::kana::to_kana` 驗證轉得回同一串假名，對不上就淘汰。
/// 驗證器是現成的，比手寫反轉表可靠。
///
/// 表外的三種（見 `romaji::kana` 的文件）：撥音一律 `nn`、促音是重複
/// 後一個 mora 的頭一個子音、長音是 `-`。
fn kana_keys(kana: &str) -> Option<String> {
    let out = spell(kana)?;
    // 驗證：拼出來的按鍵轉得回原來的假名嗎？
    match ime_core::romaji::kana::to_kana(&out) {
        Some(back) if back == kana => Some(out),
        _ => None,
    }
}

/// 拼字本身，不含驗證（遞迴時不重複驗證，由 `kana_keys` 統一做）。
fn spell(kana: &str) -> Option<String> {
    let chars: Vec<char> = kana.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            'ん' => {
                out.push_str("nn");
                i += 1;
            }
            'ー' => {
                out.push('-');
                i += 1;
            }
            'っ' => {
                // 促音：後一個 mora 的頭一個子音重複一次。後面沒東西就放棄。
                let rest: String = chars[i + 1..].iter().collect();
                let next = spell(&rest)?;
                out.push(next.chars().next()?);
                out.push_str(&next);
                i = chars.len();
            }
            c => {
                // 先試兩個字元（拗音 きゃ、外來語 ふぁ），再試一個
                if i + 2 <= chars.len() {
                    let two: String = chars[i..i + 2].iter().collect();
                    if let Some(k) = kana_to_mora(&two) {
                        out.push_str(k);
                        i += 2;
                        continue;
                    }
                }
                out.push_str(kana_to_mora(&c.to_string())?);
                i += 1;
            }
        }
    }
    Some(out)
}

/// 片假名轉平假名——羅馬字拼的是讀音，不管寫成哪一種假名。
fn to_hira(s: &str) -> String {
    s.chars()
        .map(|c| {
            if ('\u{30A1}'..='\u{30F6}').contains(&c) {
                char::from_u32(c as u32 - 0x60).unwrap_or(c)
            } else {
                c
            }
        })
        .collect()
}

/// 日文詞的所有候選按鍵串（一讀音一串）。
fn ja_keys(word: &str, map: &HashMap<String, Vec<String>>) -> Vec<String> {
    // 詞本身就是假名的話直接拼，不必查辭書
    let all_kana = !word.is_empty()
        && word.chars().all(|c| {
            ('\u{3041}'..='\u{309F}').contains(&c) || ('\u{30A1}'..='\u{30FF}').contains(&c)
        });
    if all_kana {
        return kana_keys(&to_hira(word)).into_iter().collect();
    }
    map.get(word)
        .map(|rs| rs.iter().filter_map(|r| kana_keys(r)).collect())
        .unwrap_or_default()
}

// ───────────────────────── 句型模式 ─────────────────────────

/// 一段的語言。從段的文字自動判斷，判不出來才要人標。
#[derive(Clone, Copy, PartialEq, Debug)]
enum Lang {
    Zh,
    Ja,
    En,
    /// 標點與分隔符空白——原樣輸出，不查任何詞庫
    Mark,
}

/// 這一段是什麼語言？
///
/// 判準刻意保守，判不出來寧可回 `None` 讓人手標，也不要猜錯——猜錯會
/// 產出打不出原句的按鍵，而那正是這支工具要防的事。
///
/// **中日共用漢字是主要的坑**：`生誕`／`明日`／`寿司` 都是純漢字但要用
/// 羅馬字打。所以純漢字段一律回 `None`，要人用 `zh:`／`ja:` 前綴標明。
fn guess_lang(seg: &str) -> Option<Lang> {
    if seg == "_" || seg.chars().all(|c| !c.is_alphanumeric() && c != '_') {
        return Some(Lang::Mark);
    }
    let has_kana = seg
        .chars()
        .any(|c| ('\u{3041}'..='\u{309F}').contains(&c) || ('\u{30A1}'..='\u{30FF}').contains(&c));
    let has_han = seg.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c));
    let all_ascii = seg.is_ascii();
    match (has_kana, has_han, all_ascii) {
        // 有假名 → 日文（漢字混假名也是）
        (true, _, _) => Some(Lang::Ja),
        // 純 ASCII → 英文（數字也走這條，passthrough）
        (false, false, true) => Some(Lang::En),
        // **純漢字分不出中日**，要人標
        (false, true, _) => None,
        _ => None,
    }
}

/// 多個候選按鍵裡，挑打得出原詞的那個。
///
/// **這一段自己單獨打得出來嗎**——不是整句打不打得出來。整句會不會過是
/// 另一回事（number 那組整句本來就要紅），這裡只要確保按鍵是「這個詞的
/// 合理打法」。
///
/// 為什麼一定要挑：mozc 的多讀音沒有排序，`番` 的第一個是 `tugai`
/// （つがい）而不是 `bann`。取第一個會產出打不出原詞的按鍵——那正是
/// 這支工具存在的理由。
fn pick_readable(cands: Vec<String>, want: &str) -> Option<String> {
    if cands.is_empty() {
        return None;
    }
    // 都打不回原詞就拿第一個。**日文的第一個是 cost 最小的那個**
    // （`build_ja_map` 已排序），不是檔案裡碰巧排最前面的冷門讀音。
    cands
        .iter()
        .find(|k| compose_text(k) == want)
        .or_else(|| cands.first())
        .cloned()
}

/// 一段的按鍵。`lang` 已知就照它產。
fn seg_keys(
    seg: &str,
    lang: Lang,
    zh_words: &HashMap<String, Vec<Vec<String>>>,
    syl: &HashMap<String, String>,
    zh_chars: &HashMap<String, String>,
    ja_map: &HashMap<String, Vec<String>>,
) -> Option<String> {
    match lang {
        // 分隔符空白自成一段；其他標點原樣（按鍵欄一律半形）
        Lang::Mark => Some(if seg == "_" {
            " ".to_string()
        } else {
            seg.to_string()
        }),
        // 英文與數字是 passthrough：打什麼出什麼
        Lang::En => Some(seg.to_string()),
        // **多讀音要挑打得出原詞的那個**，不能取第一個。mozc 的讀音沒有
        // 排序，`番` 的第一個是 `tugai`（つがい）而不是 `bann`——取第一個
        // 會產出打不出原詞的按鍵，正是這支工具要防的事。
        Lang::Zh => pick_readable(zh_keys(seg, zh_words, syl, zh_chars), seg),
        Lang::Ja => pick_readable(ja_keys(seg, ja_map), seg),
    }
}

/// 句型模式：一行一句，段用 `|` 分隔，純漢字段要標 `zh:` 或 `ja:`。
///
/// ```text
/// 3|個|_|bug        →  3|3ek7| |bug        3 3ek7 bug
/// zh:今天|_|meeting →  rup wu0 | |meeting  rup wu0  meeting
/// ```
///
/// # 為什麼要有這個模式
///
/// `--tsv` 的第二欄是拿**引擎自己的第一名切法**填的。對會過的句子沒差，
/// 但要寫「引擎現在會答錯」的測資時，那等於**拿錯答案當期望**——
/// 循環論證。這個模式讓結構由人指定、按鍵由工具產，第二欄反映的是
/// 需求不是現況。
///
/// 產完仍然會跑一次引擎印出**現在**打出什麼，只是那結果不進第二欄。
fn sentence_mode(
    lines: &[String],
    zh_words: &HashMap<String, Vec<Vec<String>>>,
    syl: &HashMap<String, String>,
    zh_chars: &HashMap<String, String>,
    ja_map: &HashMap<String, Vec<String>>,
    tsv: bool,
) {
    let mut ok = 0usize;
    for line in lines {
        let mut text_segs: Vec<String> = Vec::new();
        let mut key_segs: Vec<String> = Vec::new();
        let mut bad: Option<String> = None;

        for raw in line.split('|') {
            // `zh:` / `ja:` / `en:` 前綴強制指定語言
            let (lang, seg) = if let Some(s) = raw.strip_prefix("zh:") {
                (Some(Lang::Zh), s)
            } else if let Some(s) = raw.strip_prefix("ja:") {
                (Some(Lang::Ja), s)
            } else if let Some(s) = raw.strip_prefix("en:") {
                (Some(Lang::En), s)
            } else {
                (guess_lang(raw), raw)
            };
            let Some(lang) = lang else {
                bad = Some(format!("「{seg}」是中文還日文？請標 zh: 或 ja:"));
                break;
            };
            match seg_keys(seg, lang, zh_words, syl, zh_chars, ja_map) {
                Some(k) => {
                    text_segs.push(seg.to_string());
                    key_segs.push(k);
                }
                None => {
                    bad = Some(format!("「{seg}」查無讀音"));
                    break;
                }
            }
        }

        if let Some(msg) = bad {
            if !tsv {
                println!("?\t{line}\t【{msg}】");
            }
            continue;
        }

        let keys: String = key_segs.concat();
        let want = text_segs.join("|");
        let key_expect = key_segs.join("|");
        // 引擎現在打出什麼？**只印出來給人看，不進第二欄**
        let got = compose_text(&keys);
        let matched = got == testdata_text(&want);
        if matched {
            ok += 1;
        }
        if tsv {
            println!("{want}\t{key_expect}\t{keys}");
        } else {
            let mark = if matched { "OK" } else { "✗" };
            let note = if matched {
                String::new()
            } else {
                format!("  現在打出「{got}」")
            };
            println!("{mark}\t{want}\t{key_expect}\t{keys}{note}");
        }
    }
    if !tsv {
        eprintln!(
            "\n{ok}/{} 現在就打得出來（打不出來的仍會產測資）",
            lines.len()
        );
    }
}

/// 期望欄 → 期望字串：`|` 只是分隔、`_` 代表空白。
/// 跟 `common/testdata.rs` 的 `expected_text` 同一個規則。
fn testdata_text(want: &str) -> String {
    want.replace('|', "").replace('_', " ")
}

// ───────────────────────── 驗證 ─────────────────────────

/// 這串日文按鍵**本身**對不對——不管引擎打不打得回原詞。
///
/// # 為什麼要分這兩件事
///
/// 打不回原詞有兩種完全不同的原因，修法相反：
///
/// 1. **工具挑錯讀音**（`台` 挑到 `うてな`）——按鍵是錯的，
///    這種**不可以進測資**，錯的期望會把計分器帶偏。
/// 2. **讀音對但引擎排序輸了**（`階` 的 `kai` 打出「回」）——按鍵是對的，
///    這種**應該進測資讓它紅著**，那是需求不是退步。
///
/// 判準：按鍵轉回假名，看是不是這個表記在辭書裡的讀音之一。
/// 是 → 第 2 種；不是 → 第 1 種。純假名詞不必查辭書，轉得回自己就對。
fn ja_keys_sound(keys: &str, want: &str, map: &HashMap<String, Vec<String>>) -> bool {
    let Some(back) = ime_core::romaji::kana::to_kana(keys) else {
        return false;
    };
    let want_hira = to_hira(want);
    if back == want_hira {
        return true;
    }
    map.get(want).is_some_and(|rs| rs.contains(&back))
}

/// 切法第一名的段。`check_compose` 那條完整的鏈就是從這裡開始。
fn top_cut(keys: &str) -> Vec<ime_core::cutpoint::Segment> {
    let cands = rank::sort(Incremental::from_keys(keys).cuttings());
    cands.first().map(|c| normalize(c)).unwrap_or_default()
}

/// 這串按鍵打出來是什麼？
fn compose_text(keys: &str) -> String {
    compose::text_of(&compose::compose(&top_cut(keys)))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ja = args.iter().any(|a| a == "--ja");
    let tsv = args.iter().any(|a| a == "--tsv");
    let sent = args.iter().any(|a| a == "--sent");

    // --file 讀檔（一行一詞），否則吃命令列參數
    let mut words: Vec<String> = Vec::new();
    let mut it = args.iter().peekable();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--ja" | "--tsv" | "--sent" => {}
            "--file" => {
                if let Some(p) = it.next() {
                    match std::fs::read_to_string(p) {
                        Ok(c) => words.extend(
                            c.lines()
                                .map(str::trim)
                                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                                .map(String::from),
                        ),
                        Err(e) => eprintln!("讀不到 {p}：{e}"),
                    }
                }
            }
            _ => words.push(a.clone()),
        }
    }
    if words.is_empty() {
        eprintln!("用法：mkkeys [--ja] [--tsv] 詞1 詞2 ...");
        eprintln!("      mkkeys [--ja] [--tsv] --file 詞表.txt");
        eprintln!("      mkkeys --sent [--tsv] --file 句型.txt   （段用 | 分隔）");
        std::process::exit(1);
    }

    let dir = data_dir();
    // **載的東西必須跟 `check_compose` 一模一樣**，一項都不能少。
    // 少載任何一層，這支的「OK」跟計分器的結果就會各說各話——那比
    // 沒有這支工具更糟，因為它會產出看似驗證過的錯測資。
    ime_core::pack::set_bundled_dir(dir.parent().map(|d| d.join("packs")));
    ime_core::pack::load(
        "__不存在的資料夾__",
        &[ime_core::pack::BUNDLED_SYMBOLS.to_string()],
    );
    ime_core::english::load(&dir);
    ime_core::dict::load_bopomofo(&dir);
    ime_core::lm::load(&dir, ime_core::dict::char_freq_map(&dir));
    ime_core::dict::load_japanese(&dir);

    // 句型模式一句裡三種語言都可能出現，三份資料都要載
    let need_zh = sent || !ja;
    let need_ja = sent || ja;
    let syl = if need_zh {
        build_syllable_map(&dir)
    } else {
        HashMap::new()
    };
    let zh_words = if need_zh {
        build_word_map(&dir)
    } else {
        HashMap::new()
    };
    let zh_chars = if need_zh {
        build_char_map(&dir)
    } else {
        HashMap::new()
    };
    let ja_map = if need_ja {
        build_ja_map(&dir)
    } else {
        HashMap::new()
    };

    if sent {
        sentence_mode(&words, &zh_words, &syl, &zh_chars, &ja_map, tsv);
        return;
    }

    let mut ok_count = 0usize;
    let mut sound_count = 0usize;
    for want in &words {
        let cands = if ja {
            ja_keys(want, &ja_map)
        } else {
            zh_keys(want, &zh_words, &syl, &zh_chars)
        };
        if cands.is_empty() {
            println!("?\t{want}\t【查無讀音，需人工處理】");
            continue;
        }
        // 挑第一個打得出原詞的讀音；都打不出來就拿第一個當診斷樣本
        let hit = cands.iter().find(|k| compose_text(k) == *want);
        let keys = hit.unwrap_or(&cands[0]);
        let got = compose_text(keys);
        let matched = got == *want;
        // **打不回原詞時要分兩種**：按鍵本身對不對。對的話是引擎排序還沒
        // 做到，測資照收讓它紅著；錯的話是工具挑錯讀音，不准進測資。
        // 中文那條路沒有這個問題（注音音節是一對一的），只有日文要分。
        let sound = matched || (ja && ja_keys_sound(keys, want, &ja_map));
        if matched {
            ok_count += 1;
        }
        if sound {
            sound_count += 1;
        }
        if tsv {
            // 按鍵無誤就印——包含「引擎現在打錯」的，那是需求不是退步
            if sound {
                let segs: Vec<String> = top_cut(keys).into_iter().map(|s| s.keys).collect();
                println!("{want}\t{}\t{keys}", segs.join("|"));
            }
        } else {
            let mark = match (matched, sound) {
                (true, _) => "OK",
                // 按鍵對、引擎還打不出來——可以進測資
                (false, true) => "△",
                // 按鍵本身就錯——不可以進測資
                (false, false) => "✗",
            };
            let note = if matched {
                String::new()
            } else if sound {
                format!("  引擎現在打出「{got}」（按鍵無誤，可收）")
            } else {
                format!("  實際={got}【讀音可疑，需人工確認】")
            };
            let alt = if cands.len() > 1 {
                format!("  ({}種讀音)", cands.len())
            } else {
                String::new()
            };
            println!("{mark}\t{want}\t{keys}{note}{alt}");
        }
    }
    if !tsv {
        eprintln!(
            "\n{ok_count}/{} 打得出原詞；{sound_count}/{} 按鍵無誤（可進測資）",
            words.len(),
            words.len()
        );
    }
}
