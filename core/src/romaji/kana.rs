//! 羅馬字 → 平假名的轉換。
//!
//! # 為什麼需要
//!
//! 日文詞典存的是假名（`きんようび`），切法拿在手上的是羅馬字
//! （`kinnyoubi`）。要拿切出來的段去查詞典，中間得有這個轉換器。
//!
//! 選詞模組也會用到——要顯示「きんようび／金曜日」讓使用者挑。
//!
//! # 難的部分 Phase 1 已經做完了
//!
//! 切分（哪幾個字母算一個 mora）才是難題：拗音、促音、長音的優先
//! 順序都在 `super::split_moras` 裡處理好了。這裡只負責**查表**。
//!
//! ```text
//! kinnyoubi → split_moras → ["ki","nn","yo","u","bi"] → 逐格查表
//!                                                     → きんようび
//! ```
//!
//! # 資料來源
//!
//! `日文輸入單位一覽.md`（使用者已核對）抄成靜態表，171 條；另補 53 條
//! 一覽表沒列但引擎判合法的組合，共 224 條。
//! 抄而不是解析那份 .md，是因為它在 Obsidian 資料夾裡，不進版控——
//! 跨電腦會找不到。`bopomofo::keymap` 也是同樣的做法。
//!
//! **只做平假名**。片假名是選詞模組顯示時的事，查詞典只需要平假名
//! （`dictionary00.txt` 的讀音欄就是平假名）。

/// mora → 平假名。
///
/// 表外的撥音／促音／長音不在這裡，見 `to_kana`。
#[rustfmt::skip]
const MORA_TABLE: &[(&str, &str)] = &[
    // ── 1 字母：母音（5）──
    ("a","あ"), ("i","い"), ("u","う"), ("e","え"), ("o","お"),

    // ── 2 字母：清音 か〜わ行（43）──
    ("ha","は"), ("he","へ"), ("hi","ひ"), ("ho","ほ"), ("hu","ふ"),
    ("ka","か"), ("ke","け"), ("ki","き"), ("ko","こ"), ("ku","く"),
    ("ma","ま"), ("me","め"), ("mi","み"), ("mo","も"), ("mu","む"),
    ("na","な"), ("ne","ね"), ("ni","に"), ("no","の"), ("nu","ぬ"),
    ("ra","ら"), ("re","れ"), ("ri","り"), ("ro","ろ"), ("ru","る"),
    ("sa","さ"), ("se","せ"), ("si","し"), ("so","そ"), ("su","す"),
    ("ta","た"), ("te","て"), ("ti","ち"), ("to","と"), ("tu","つ"),
    ("wa","わ"), ("we","うぇ"), ("wi","うぃ"), ("wo","を"),
    ("ya","や"), ("ye","いぇ"), ("yo","よ"), ("yu","ゆ"),

    // ── 2 字母：濁音 が〜ば行 ＋ j（25）──
    ("ba","ば"), ("be","べ"), ("bi","び"), ("bo","ぼ"), ("bu","ぶ"),
    ("da","だ"), ("de","で"), ("di","ぢ"), ("do","ど"), ("du","づ"),
    ("ga","が"), ("ge","げ"), ("gi","ぎ"), ("go","ご"), ("gu","ぐ"),
    ("ja","じゃ"), ("je","じぇ"), ("ji","じ"), ("jo","じょ"), ("ju","じゅ"),
    ("za","ざ"), ("ze","ぜ"), ("zi","じ"), ("zo","ぞ"), ("zu","ず"),

    // ── 2 字母：半濁音 ぱ行（5）──
    ("pa","ぱ"), ("pe","ぺ"), ("pi","ぴ"), ("po","ぽ"), ("pu","ぷ"),

    // ── 2 字母：外來語 f v（10）──
    ("fa","ふぁ"), ("fe","ふぇ"), ("fi","ふぃ"), ("fo","ふぉ"), ("fu","ふ"),
    ("va","ゔぁ"), ("ve","ゔぇ"), ("vi","ゔぃ"), ("vo","ゔぉ"), ("vu","ゔ"),

    // ── 2 字母：小寫打法 x l（10）──
    ("la","ぁ"), ("le","ぇ"), ("li","ぃ"), ("lo","ぉ"), ("lu","ぅ"),
    ("xa","ぁ"), ("xe","ぇ"), ("xi","ぃ"), ("xo","ぉ"), ("xu","ぅ"),

    // ── 3 字母：拗音（第2位 = y）（45）──
    ("bya","びゃ"), ("byo","びょ"), ("byu","びゅ"), ("cyi","ちぃ"), ("dyu","でゅ"),
    ("fyu","ふゅ"), ("gya","ぎゃ"), ("gyo","ぎょ"), ("gyu","ぎゅ"), ("hya","ひゃ"),
    ("hyo","ひょ"), ("hyu","ひゅ"), ("jya","じゃ"), ("jyo","じょ"), ("jyu","じゅ"),
    ("kya","きゃ"), ("kyo","きょ"), ("kyu","きゅ"),
    ("mya","みゃ"), ("myo","みょ"), ("myu","みゅ"), ("nya","にゃ"),
    ("nyo","にょ"), ("nyu","にゅ"), ("pya","ぴゃ"), ("pyo","ぴょ"), ("pyu","ぴゅ"),
    ("rya","りゃ"), ("ryo","りょ"), ("ryu","りゅ"), ("sya","しゃ"), ("syo","しょ"),
    ("syu","しゅ"), ("tya","ちゃ"), ("tyo","ちょ"), ("tyu","ちゅ"),
    ("zya","じゃ"), ("zyo","じょ"), ("zyu","じゅ"),

    // ── 3 字母：特 sh / ch（10）──
    ("cha","ちゃ"), ("che","ちぇ"), ("chi","ち"), ("cho","ちょ"), ("chu","ちゅ"),
    ("sha","しゃ"), ("she","しぇ"), ("shi","し"), ("sho","しょ"), ("shu","しゅ"),

    // ── 3 字母：特 th / dh（10）──
    ("dha","でゃ"), ("dhe","でぇ"), ("dhi","でぃ"), ("dho","でょ"), ("dhu","でゅ"),
    ("tha","てゃ"), ("the","てぇ"), ("thi","てぃ"), ("tho","てょ"), ("thu","てゅ"),

    // ── 3 字母：特 who（1）──
    ("who","うぉ"),

    // ── 3 字母：特 ts（5）──
    ("tsa","つぁ"), ("tse","つぇ"), ("tsi","つぃ"), ("tso","つぉ"), ("tsu","つ"),

    // ── 3 字母：特 w（第2位 = w）──
    //
    // 一覽表的 `twa` 那格寫的是片假名「トァ」，同一列其他格都是平假名。
    // 那看起來是原表的筆誤，這裡統一成平假名「とぁ」。
    ("twa","とぁ"), ("twi","とぃ"), ("twu","とぅ"), ("twe","とぇ"), ("two","とぉ"),
    ("dwa","どぁ"), ("dwu","どぅ"), ("dwo","どぉ"),
    // ── 補：拗音接 e / i（26）──
    //
    // 一覽表只列了拗音的 a/o/u，但引擎判 e/i 也合法。
    // 對應方式跟表上的 `cyi`（ちぃ）一致：i 段假名 ＋ 小寫母音。
    ("bye","びぇ"), ("byi","びぃ"),
    ("cya","ちゃ"), ("cye","ちぇ"), ("cyo","ちょ"), ("cyu","ちゅ"),
    ("dya","ぢゃ"), ("dye","ぢぇ"), ("dyi","ぢぃ"), ("dyo","ぢょ"),
    ("fya","ふゃ"), ("fye","ふぇ"), ("fyi","ふぃ"), ("fyo","ふょ"),
    ("gye","ぎぇ"), ("gyi","ぎぃ"),
    ("hye","ひぇ"), ("hyi","ひぃ"),
    ("jye","じぇ"), ("jyi","じぃ"),
    ("kye","きぇ"), ("kyi","きぃ"),
    ("mye","みぇ"), ("myi","みぃ"),
    ("pye","ぴぇ"), ("pyi","ぴぃ"),
    ("rye","りぇ"), ("ryi","りぃ"),
    ("sye","しぇ"), ("syi","しぃ"),
    ("tye","ちぇ"), ("tyi","ちぃ"),
    ("vya","ゔゃ"), ("vye","ゔぇ"), ("vyi","ゔぃ"), ("vyo","ゔょ"), ("vyu","ゔゅ"),
    ("zye","じぇ"), ("zyi","じぃ"),

    // ── 補：小寫打法（14）──
    //
    // `x` 與 `l` 是「強制輸出小寫」的打法，走 `mora::check_kogaki`
    // 那條分支。一覽表只列了 `xa`～`xu`（小寫母音），沒把接
    // y／t／w／k 的組合列完，但引擎判它們合法。
    //
    // 對應直接照 `check_kogaki` 的文件——那裡已經寫明這四種對應
    // 日文僅有的小寫假名：
    //
    // ```text
    // 母音        → ぁぃぅぇぉ
    // y + a/u/o   → ゃゅょ
    // t + u       → っ        促音的直接打法，不必重複子音
    // w + a       → ゎ
    // k + a/e     → ヵヶ      見下
    // ```
    //
    // 只有 `lka`/`lke` 這格用**片假名**（ヵ U+30F5、ヶ U+30F6），
    // 其餘全表都是平假名。平假名雖然有 ゕゖ（U+3095、U+3096），
    // 但實務上不用——mozc 詞典 74 萬條裡含 ゕゖ 的是 0 條，
    // 含 ヵヶ 的才有（「三ヶ月」這種寫法）。用平假名會查不到詞典。
    ("lya","ゃ"), ("lyo","ょ"), ("lyu","ゅ"),
    ("xya","ゃ"), ("xyo","ょ"), ("xyu","ゅ"),
    ("ltu","っ"), ("xtu","っ"),
    ("lwa","ゎ"), ("xwa","ゎ"),
    ("lka","ヵ"), ("lke","ヶ"), ("xka","ヵ"), ("xke","ヶ"),
];

/// 撥音：`n` 或 `nn` → ん
const HATSUON: &str = "ん";
/// 促音：子音重複 → っ
const SOKUON: &str = "っ";
/// 長音：`-` → ー
const CHOUON: &str = "ー";

/// 一個 mora 對應的平假名。表外的回 `None`。
pub fn mora_to_kana(mora: &str) -> Option<&'static str> {
    MORA_TABLE.iter().find(|(k, _)| *k == mora).map(|(_, v)| *v)
}

/// 羅馬字轉平假名。不是合法日文就回 `None`。
///
/// # 表外的三種怎麼處理
///
/// | 類型 | 寫法 | 輸出 |
/// |---|---|---|
/// | 撥音 | `nn` | ん |
/// | 促音 | 子音重複 | っ ＋ 後面那個 mora |
/// | 長音 | `-` | ー |
///
/// 促音的形式是「重複的子音黏在 mora 前面」——`kko` 切出來就是
/// `kko` 一格，前面那個 `k` 是促音，剩下的 `ko` 才查表。
pub fn to_kana(keys: &str) -> Option<String> {
    let moras = super::split_moras(keys)?;
    let mut out = String::new();
    for m in &moras {
        out.push_str(&one(m)?);
    }
    Some(out)
}

/// **盡量轉**：轉得出假名的前綴轉掉，剩下的原樣留著。
///
/// 回傳 `(假名, 還沒轉的殘留字母)`。
///
/// # 為什麼需要這個
///
/// `to_kana` 是全有或全無——`sush` 這種打到一半的東西回 `None`。
/// 但鎖定日文時組字區要**邊打邊顯示**：打 `sus` 該看到「すs」，
/// 而不是整串退回羅馬字。
///
/// 這就是 mozc 的 `pending` 概念：`CharChunk` 把已確定的
/// （`conversion`）跟等後續輸入的（`pending`）分開存。差別是 mozc
/// 還有第三層 `ambiguous`（`n` 可能是「ん」也可能是「な」），
/// 本專案規定撥音一律打 `nn`，歧義不存在，所以只要兩層。
///
/// # 怎麼切
///
/// 從最長的前綴開始試，找到第一個轉得出假名的就收下，剩下的遞迴。
/// 殘留最多 3 個字母（`chy` 這種），所以從尾巴退最多 3 次就夠。
/// 每個 mora 佔幾個按鍵、產生幾個假名。
///
/// # 為什麼需要
///
/// 整句轉換是**照假名**分詞的（`ごはん|を|たべ|ます`），但格子的
/// `keys` 必須是**原始按鍵**——`check_rewrite`、整格刪除、學習全都靠
/// 「格子的按鍵接得回原字串」這個性質。
///
/// 而羅馬字跟假名**不是等比**：`すし` 兩個假名卻是五個字母（`sushi`），
/// `を` 一個假名是兩個字母（`wo`）。按比例分會切錯（實測 `sushi|wo|
/// tabe|masu` 被切成 `sush|iw|otab|emasu`），所以要拿真正的對應。
///
/// 回傳每個 mora 的 `(按鍵字元數, 假名字元數)`。轉不出來回 `None`。
pub fn mora_spans(keys: &str) -> Option<Vec<(usize, usize)>> {
    let moras = super::split_moras(keys)?;
    let mut out = Vec::with_capacity(moras.len());
    for m in &moras {
        let k = one(m)?;
        out.push((m.chars().count(), k.chars().count()));
    }
    Some(out)
}

pub fn to_kana_partial(keys: &str) -> (String, String) {
    if keys.is_empty() {
        return (String::new(), String::new());
    }
    let chars: Vec<char> = keys.chars().collect();
    // 從整串開始往回退，找最長的可轉前綴
    for end in (0..=chars.len()).rev() {
        let head: String = chars[..end].iter().collect();
        if head.is_empty() {
            break;
        }
        if let Some(kana) = to_kana(&head) {
            let rest: String = chars[end..].iter().collect();
            return (kana, rest);
        }
    }
    // 一個 mora 都湊不出來，整串都是殘留
    (String::new(), keys.to_string())
}

/// 一格 mora → 平假名（含表外的三種）。
fn one(mora: &str) -> Option<String> {
    if mora == "-" {
        return Some(CHOUON.to_string());
    }
    // 撥音：`nn`（規格是「一律打 nn」，但單獨的 n 也認）
    if mora == "nn" || mora == "n" {
        return Some(HATSUON.to_string());
    }
    if let Some(k) = mora_to_kana(mora) {
        return Some(k.to_string());
    }
    // 促音：頭兩個字元相同的子音，剝掉一個再查
    let c: Vec<char> = mora.chars().collect();
    if c.len() >= 3 && c[0] == c[1] && c[0] != 'n' {
        let rest: String = c[1..].iter().collect();
        let k = mora_to_kana(&rest)?;
        return Some(format!("{SOKUON}{k}"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 基本音節() {
        assert_eq!(to_kana("sushi").as_deref(), Some("すし"));
        assert_eq!(to_kana("konnnichiha").as_deref(), Some("こんにちは"));
        assert_eq!(to_kana("arigatou").as_deref(), Some("ありがとう"));
    }

    #[test]
    fn 撥音() {
        assert_eq!(
            to_kana("kinnyoubi").as_deref(),
            Some("きんようび"),
            "金曜日"
        );
        assert_eq!(to_kana("dennwa").as_deref(), Some("でんわ"), "電話");
    }

    #[test]
    fn 促音() {
        assert_eq!(to_kana("gakkou").as_deref(), Some("がっこう"), "学校");
        assert_eq!(to_kana("gannbatte").as_deref(), Some("がんばって"));
    }

    #[test]
    fn 長音() {
        assert_eq!(to_kana("ra-menn").as_deref(), Some("らーめん"));
    }

    #[test]
    fn 拗音是一個單位輸出兩個字元() {
        assert_eq!(to_kana("kya").as_deref(), Some("きゃ"));
        assert_eq!(to_kana("shashinn").as_deref(), Some("しゃしん"), "写真");
    }

    #[test]
    fn 非法日文回_none() {
        assert_eq!(to_kana("zzz"), None);
        assert_eq!(to_kana(""), None);
        // 撥音一律打 nn，單獨的 kin 是「沒打完」→ 非法
        assert_eq!(to_kana("kin"), None);
    }

    #[test]
    fn 表涵蓋所有合法_mora() {
        // split_moras 切得出來的每一格，這裡都要查得到
        for keys in [
            "sushi",
            "tomodachi",
            "wakarimashita",
            "daijoubu",
            "issyoni",
            "oishii",
            "shitsurei",
            "gozaimasu",
            "onegai",
            "yasumi",
        ] {
            assert!(to_kana(keys).is_some(), "{keys:?} 該轉得出來");
        }
    }

    #[test]
    fn 表沒有重複的鍵() {
        let mut seen = std::collections::HashSet::new();
        for (k, _) in MORA_TABLE {
            assert!(seen.insert(*k), "{k:?} 在表裡出現兩次");
        }
    }

    #[test]
    fn 表的長度() {
        // 一覽表抄下來 171 條，加上補的 53 條（一覽表沒列但引擎判合法
        // 的組合：拗音接 e/i、cy- 系列、x/l 的拗音與特殊小字）。
        assert_eq!(MORA_TABLE.len(), 224, "實際 {}", MORA_TABLE.len());
    }

    /// 引擎判為合法的單一 mora，這張表都查得到嗎？
    ///
    /// 查不到的話 `to_kana` 會回 `None`，那一段就無法拿去查日文詞典。
    /// 這個測試窮舉所有 1～3 字母的組合，是表與引擎的一致性保證。
    #[test]
    fn 表對得上引擎() {
        use crate::romaji::{mora, Validity};
        let letters = "abcdefghijklmnopqrstuvwxyz-";
        let mut missing = Vec::new();
        for a in letters.chars() {
            for b in letters.chars() {
                for c in letters.chars() {
                    for len in 1..=3 {
                        let s: String = match len {
                            1 => a.to_string(),
                            2 => format!("{a}{b}"),
                            _ => format!("{a}{b}{c}"),
                        };
                        // 要兩關都過才是真的合法（例外規則在 validity 裡）
                        if mora::check(&s) != Validity::Valid
                            || super::super::validity(&s) != Validity::Valid
                        {
                            continue;
                        }
                        if one(&s).is_none() {
                            missing.push(s);
                        }
                    }
                }
            }
        }
        missing.sort();
        missing.dedup();
        assert!(
            missing.is_empty(),
            "引擎說合法但表查不到的 {} 個：{:?}",
            missing.len(),
            missing
        );
    }
}

/// 平假名 → 全形片假名。
///
/// Unicode 把兩者排在**同樣的順序**，相差固定的 0x60
/// （ぁ U+3041 ↔ ァ U+30A1），所以整段位移就好，不必查表。
///
/// 不在那個區間的字元（長音「ー」、標點、殘留的羅馬字母）原樣留著。
pub fn to_katakana(hira: &str) -> String {
    hira.chars()
        .map(|c| {
            // ぁ..ゖ 這段才位移；ゝゞ 那些重複記號不動
            if ('\u{3041}'..='\u{3096}').contains(&c) {
                char::from_u32(c as u32 + 0x60).unwrap_or(c)
            } else {
                c
            }
        })
        .collect()
}

/// 全形片假名 → 半形片假名。
///
/// # 為什麼要查表
///
/// 半形片假名不是等寬對應：濁音「ガ」要拆成兩個字元「ｶ」＋「ﾞ」
/// （濁點自己佔一格），所以沒辦法像全形那樣整段位移。
///
/// 清音的部分照 U+FF66 起的順序排，濁音／半濁音則是「清音＋濁點」。
pub fn to_halfwidth_katakana(kata: &str) -> String {
    let mut out = String::new();
    for c in kata.chars() {
        match c {
            // 濁音：清音 + 濁點
            'ガ'..='ボ' | 'ヴ' if is_dakuten(c) => {
                out.push(base_of_dakuten(c));
                out.push('ﾞ');
                continue;
            }
            // 半濁音：清音 + 半濁點
            'パ' | 'ピ' | 'プ' | 'ペ' | 'ポ' => {
                out.push(match c {
                    'パ' => 'ﾊ',
                    'ピ' => 'ﾋ',
                    'プ' => 'ﾌ',
                    'ペ' => 'ﾍ',
                    _ => 'ﾎ',
                });
                out.push('ﾟ');
                continue;
            }
            _ => {}
        }
        out.push(halfwidth_one(c));
    }
    out
}

/// 這個片假名是濁音嗎？
fn is_dakuten(c: char) -> bool {
    matches!(
        c,
        'ガ' | 'ギ'
            | 'グ'
            | 'ゲ'
            | 'ゴ'
            | 'ザ'
            | 'ジ'
            | 'ズ'
            | 'ゼ'
            | 'ゾ'
            | 'ダ'
            | 'ヂ'
            | 'ヅ'
            | 'デ'
            | 'ド'
            | 'バ'
            | 'ビ'
            | 'ブ'
            | 'ベ'
            | 'ボ'
            | 'ヴ'
    )
}

/// 濁音對應的清音（半形要拆成清音＋濁點）。
fn base_of_dakuten(c: char) -> char {
    match c {
        'ガ' => 'ｶ',
        'ギ' => 'ｷ',
        'グ' => 'ｸ',
        'ゲ' => 'ｹ',
        'ゴ' => 'ｺ',
        'ザ' => 'ｻ',
        'ジ' => 'ｼ',
        'ズ' => 'ｽ',
        'ゼ' => 'ｾ',
        'ゾ' => 'ｿ',
        'ダ' => 'ﾀ',
        'ヂ' => 'ﾁ',
        'ヅ' => 'ﾂ',
        'デ' => 'ﾃ',
        'ド' => 'ﾄ',
        'バ' => 'ﾊ',
        'ビ' => 'ﾋ',
        'ブ' => 'ﾌ',
        'ベ' => 'ﾍ',
        'ボ' => 'ﾎ',
        'ヴ' => 'ｳ',
        _ => c,
    }
}

/// 單一個全形片假名 → 半形。查不到的原樣留著。
fn halfwidth_one(c: char) -> char {
    match c {
        'ア' => 'ｱ',
        'イ' => 'ｲ',
        'ウ' => 'ｳ',
        'エ' => 'ｴ',
        'オ' => 'ｵ',
        'カ' => 'ｶ',
        'キ' => 'ｷ',
        'ク' => 'ｸ',
        'ケ' => 'ｹ',
        'コ' => 'ｺ',
        'サ' => 'ｻ',
        'シ' => 'ｼ',
        'ス' => 'ｽ',
        'セ' => 'ｾ',
        'ソ' => 'ｿ',
        'タ' => 'ﾀ',
        'チ' => 'ﾁ',
        'ツ' => 'ﾂ',
        'テ' => 'ﾃ',
        'ト' => 'ﾄ',
        'ナ' => 'ﾅ',
        'ニ' => 'ﾆ',
        'ヌ' => 'ﾇ',
        'ネ' => 'ﾈ',
        'ノ' => 'ﾉ',
        'ハ' => 'ﾊ',
        'ヒ' => 'ﾋ',
        'フ' => 'ﾌ',
        'ヘ' => 'ﾍ',
        'ホ' => 'ﾎ',
        'マ' => 'ﾏ',
        'ミ' => 'ﾐ',
        'ム' => 'ﾑ',
        'メ' => 'ﾒ',
        'モ' => 'ﾓ',
        'ヤ' => 'ﾔ',
        'ユ' => 'ﾕ',
        'ヨ' => 'ﾖ',
        'ラ' => 'ﾗ',
        'リ' => 'ﾘ',
        'ル' => 'ﾙ',
        'レ' => 'ﾚ',
        'ロ' => 'ﾛ',
        'ワ' => 'ﾜ',
        'ヲ' => 'ｦ',
        'ン' => 'ﾝ',
        // 小字
        'ァ' => 'ｧ',
        'ィ' => 'ｨ',
        'ゥ' => 'ｩ',
        'ェ' => 'ｪ',
        'ォ' => 'ｫ',
        'ッ' => 'ｯ',
        'ャ' => 'ｬ',
        'ュ' => 'ｭ',
        'ョ' => 'ｮ',
        // 長音與標點
        'ー' => 'ｰ',
        '、' => '､',
        '。' => '｡',
        '「' => '｢',
        '」' => '｣',
        _ => c,
    }
}

#[cfg(test)]
mod katakana_tests {
    use super::*;

    #[test]
    fn 平假名轉全形片假名() {
        assert_eq!(to_katakana("すし"), "スシ");
        assert_eq!(to_katakana("ありがとう"), "アリガトウ");
        // 促音、拗音也要轉
        assert_eq!(to_katakana("がっこう"), "ガッコウ");
    }

    #[test]
    fn 長音與非假名原樣留著() {
        // 長音「ー」不在位移區間，殘留的羅馬字母也是
        assert_eq!(to_katakana("けーき"), "ケーキ");
        assert_eq!(to_katakana("すsh"), "スsh");
    }

    #[test]
    fn 全形轉半形片假名() {
        assert_eq!(to_halfwidth_katakana("スシ"), "ｽｼ");
        assert_eq!(to_halfwidth_katakana("ケーキ"), "ｹｰｷ");
    }

    #[test]
    fn 濁音半形要拆成兩個字元() {
        // **這是半形不能整段位移的理由**：濁點自己佔一格
        assert_eq!(to_halfwidth_katakana("ガ"), "ｶﾞ");
        assert_eq!(to_halfwidth_katakana("ガッコウ"), "ｶﾞｯｺｳ");
        assert_eq!(to_halfwidth_katakana("アリガトウ"), "ｱﾘｶﾞﾄｳ");
    }

    #[test]
    fn 半濁音也是兩個字元() {
        assert_eq!(to_halfwidth_katakana("パ"), "ﾊﾟ");
        assert_eq!(to_halfwidth_katakana("ピザ"), "ﾋﾟｻﾞ");
    }

    #[test]
    fn 盡量轉_轉得出的前綴轉掉() {
        assert_eq!(to_kana_partial("sush"), ("す".into(), "sh".into()));
        assert_eq!(to_kana_partial("sushi"), ("すし".into(), "".into()));
        // 一個 mora 都湊不出來
        assert_eq!(to_kana_partial("k"), ("".into(), "k".into()));
    }
}
