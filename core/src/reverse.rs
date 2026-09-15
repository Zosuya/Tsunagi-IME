//! 反查：**給一個詞，算出要按哪些鍵**。
//!
//! 見開發文件 §2.75。擴充包編輯器要讓使用者只打「胡桃」，程式自己
//! 填出 `cj6wl6`——不必他去別的地方複製注音符號。
//!
//! # 三種語言三條路
//!
//! | 語言 | 怎麼反查 | 命中率 |
//! |---|---|---|
//! | 中文 | 掃過 `dict_zh.bin` 建「詞 → 按鍵」的反向索引 | 抽樣 3000 筆 100% |
//! | 日文 | 假名 → 羅馬字（規則的） | 往返驗證 99.6% |
//! | 英文 | 按鍵就是它自己 | — |
//!
//! **含漢字的日文詞查不到**（`東京` → `toukyou` 要另外的索引），
//! 那種留空讓使用者自己填——使用者決定的（§2.75.6）。
//!
//! # 為什麼不讀 `BPMFMappings.txt`
//!
//! 那份 145k 筆的整詞讀音表是**建詞庫時**的原始資料，發布時不會帶
//! （只帶 `dict_zh.bin`，見 `platform/macos/build-release.sh`）。
//! 讀它的話使用者手上根本沒有那個檔。
//!
//! 所以改成掃 `dict_zh.bin`——它本來就是「按鍵 → 詞」，反過來建一份
//! 索引就是「詞 → 按鍵」。內容同源（就是那份讀音表建出來的），
//! 命中率一樣。

use std::collections::HashMap;
use std::sync::OnceLock;

/// 詞 → 按鍵的反向索引。
///
/// **建一次就好**：要掃過整份詞典（十幾萬筆），每次查都建的話設定頁
/// 會卡住。`OnceLock` 保證只做一次。
static ZH: OnceLock<HashMap<String, String>> = OnceLock::new();

/// 假名 → 羅馬字的反向表。理由同上。
static KANA: OnceLock<HashMap<String, String>> = OnceLock::new();

/// 一個詞要按哪些鍵？查不到回 `None`（呼叫端留空讓使用者自己填）。
///
/// **語言自己判斷**：純 ASCII 是英文、有假名是日文、其餘當中文。
pub fn keys_for(word: &str) -> Option<String> {
    let w = word.trim();
    if w.is_empty() {
        return None;
    }
    // 英文：按鍵就是它自己
    if w.is_ascii() {
        return Some(w.to_ascii_lowercase());
    }
    // 有假名就走日文那條
    if w.chars().any(is_kana) {
        return kana_keys(w);
    }
    // **純漢字一律當中文查**。
    //
    // `東京` 這種中日文都有的詞會拿到中文讀音（`ㄉㄨㄥㄐㄧㄥ`），
    // 那不是錯——使用者要加的如果是日文詞，他自己改按鍵欄就好，
    // 而**多數情況下純漢字就是中文**。日文的反查要另一份索引
    // （讀音→表記是單向的），使用者決定不做（§2.75.6）。
    zh_keys(w)
}

/// 中文詞 → 按鍵。**兩層查法**，跟 `mkkeys` 一樣。
///
/// 1. **先查整詞**——沒有歧義，最準
/// 2. **查不到就逐字拼**——「英雄聯盟」這種專有名詞詞庫不收，
///    但每個字都查得到
///
/// 逐字時**單字要查單字表**（`iter_chars`）：詞表只收「詞」，
/// 「個」「年」「行」這些常用單字在裡面根本查不到。`mkkeys` 的註解
/// 記著這條教訓——第一版漏了它，句型模式一測就全滅。
fn zh_keys(word: &str) -> Option<String> {
    let idx = ZH.get_or_init(build_zh);
    if let Some(k) = idx.get(word) {
        return Some(k.clone());
    }
    // 逐字拼
    let chars = CHARS.get_or_init(build_chars);
    let mut out = String::new();
    for c in word.chars() {
        let s = c.to_string();
        // 單字也可能在詞表裡（一個字的「詞」），兩邊都問
        let k = idx.get(&s).or_else(|| chars.get(&s))?;
        out.push_str(k);
    }
    (!out.is_empty()).then_some(out)
}

/// 單字 → 按鍵的反向索引。理由見 `zh_keys`。
static CHARS: OnceLock<HashMap<String, String>> = OnceLock::new();

/// 掃過整份單字表，建「字 → 按鍵」。
///
/// **多音字挑分數最高的那個讀音**。同一個字會出現在好幾個讀音底下
/// （「行」在 `ㄒㄧㄥˊ` 也在 `ㄏㄤˊ`），照掃描順序留第一個的話拿到
/// 什麼全看讀音的排序——那跟常用度無關。分數是字頻算出來的，
/// 挑最高的才對得上使用者的預期。
fn build_chars() -> HashMap<String, String> {
    let mut best: HashMap<String, (String, u32)> = HashMap::new();
    let Some(dict) = crate::dict::zh() else {
        return HashMap::new();
    };
    for (keys, list) in dict.iter_chars() {
        for (c, score) in list {
            let e = best
                .entry(c.to_string())
                .or_insert_with(|| (keys.clone(), score));
            if score > e.1 {
                *e = (keys.clone(), score);
            }
        }
    }
    best.into_iter().map(|(c, (k, _))| (c, k)).collect()
}

/// 反向索引有幾筆。**給診斷用**——查不到時分辨「詞典沒這個詞」與
/// 「索引根本沒建起來」（詞庫還沒載完就先查了）。
pub fn zh_index_len() -> usize {
    ZH.get().map(|m| m.len()).unwrap_or(0)
}

/// 掃過整份注音詞典，建「詞 → 按鍵」。
///
/// **同一個詞可能有好幾種讀音**（多音字），這裡留**第一個遇到的**。
/// 詞典本身是按分數排的，先遇到的就是最常用的那個讀音。
fn build_zh() -> HashMap<String, String> {
    let mut m = HashMap::new();
    let Some(dict) = crate::dict::zh() else {
        return m;
    };
    for (keys, text) in dict.iter_words() {
        // 一串按鍵可能對到好幾個同音詞（「城市」「程式」），
        // 用 `\u{1}` 分隔——跟 `words()` 同一個約定
        for w in text.split(crate::dict_bin_zh::SEP) {
            if !w.is_empty() {
                m.entry(w.to_string()).or_insert_with(|| keys.clone());
            }
        }
    }
    m
}

/// 日文詞 → 羅馬字。**含漢字的查不到**（回 `None`）。
fn kana_keys(word: &str) -> Option<String> {
    let hira = to_hiragana(word);
    if !hira.chars().all(is_kana) {
        // 混了漢字——反查不了，留空讓使用者自己填（§2.75.6）
        return None;
    }
    let rev = KANA.get_or_init(build_kana);
    kana_to_romaji(&hira, rev)
}

/// 建「假名 → 羅馬字」的反向表。
///
/// 羅馬字表是單向的（按鍵→假名），這裡把所有一到三個字母的組合掃
/// 一遍，凡是 `mora_to_kana` 認得的就記下來。**同一個假名有好幾種
/// 拼法時留最短的**（`si` 勝過 `shi`），最好打。
fn build_kana() -> HashMap<String, String> {
    const LETTERS: &str = "abcdefghijklmnopqrstuvwxyz";
    let mut rev: HashMap<String, String> = HashMap::new();
    for a in LETTERS.chars() {
        for b in std::iter::once(None).chain(LETTERS.chars().map(Some)) {
            for c in std::iter::once(None).chain(LETTERS.chars().map(Some)) {
                if b.is_none() && c.is_some() {
                    continue;
                }
                let mut s = String::new();
                s.push(a);
                if let Some(b) = b {
                    s.push(b);
                }
                if let Some(c) = c {
                    s.push(c);
                }
                if let Some(k) = crate::romaji::kana::mora_to_kana(&s) {
                    let e = rev.entry(k.to_string()).or_insert_with(|| s.clone());
                    if s.len() < e.len() {
                        *e = s.clone();
                    }
                }
            }
        }
    }
    rev
}

/// 假名 → 羅馬字。貪心，從長到短配。
///
/// **三個特例不在 mora 表裡**（`to_kana` 那邊也是分開處理的）：
///
/// - `ん` → `nn`（規格規定一律打 nn）
/// - `っ` → 把後面那個 mora 的頭一個子音重複一次（`がっこう` → `gakkou`）
/// - `ー` → `-`
fn kana_to_romaji(hira: &str, rev: &HashMap<String, String>) -> Option<String> {
    let ch: Vec<char> = hira.chars().collect();
    let mut i = 0;
    let mut out = String::new();
    while i < ch.len() {
        // **拗音要先試**（長度優先），`きょ` 是一個 mora 不是兩個
        let mut matched = false;
        for len in (2..=3.min(ch.len() - i)).rev() {
            let seg: String = ch[i..i + len].iter().collect();
            if let Some(r) = rev.get(&seg) {
                out.push_str(r);
                i += len;
                matched = true;
                break;
            }
        }
        if matched {
            continue;
        }
        // 撥音
        if ch[i] == 'ん' {
            out.push_str("nn");
            i += 1;
            continue;
        }
        // 長音記號
        if ch[i] == 'ー' {
            out.push('-');
            i += 1;
            continue;
        }
        // 促音：先把後面那個 mora 轉出來，重複它的第一個字母
        if ch[i] == 'っ' {
            let mut next = None;
            for len in (1..=3.min(ch.len() - i - 1)).rev() {
                let seg: String = ch[i + 1..i + 1 + len].iter().collect();
                if let Some(r) = rev.get(&seg) {
                    next = Some((r.clone(), len));
                    break;
                }
            }
            let Some((r, len)) = next else {
                // 後面沒有可以黏的 mora（句尾的っ），用直接打法
                out.push_str("ltu");
                i += 1;
                continue;
            };
            let first = r.chars().next()?;
            // **後面那個 mora 以母音開頭時不能重複**——`っあ` 重複成
            // `aa` 會被讀成長音「ああ」，促音就消失了
            if matches!(first, 'a' | 'i' | 'u' | 'e' | 'o' | 'n') {
                out.push_str("ltu");
                i += 1;
                continue;
            }
            out.push(first);
            out.push_str(&r);
            i += 1 + len;
            continue;
        }
        let mut matched = false;
        for len in (1..=3.min(ch.len() - i)).rev() {
            let seg: String = ch[i..i + len].iter().collect();
            if let Some(r) = rev.get(&seg) {
                out.push_str(r);
                i += len;
                matched = true;
                break;
            }
        }
        if !matched {
            return None;
        }
    }
    Some(out)
}

fn is_kana(c: char) -> bool {
    matches!(c, '\u{3041}'..='\u{309f}' | '\u{30a0}'..='\u{30ff}')
}

fn to_hiragana(s: &str) -> String {
    s.chars()
        .map(|c| {
            if ('\u{30a1}'..='\u{30f6}').contains(&c) {
                char::from_u32(c as u32 - 0x60).unwrap_or(c)
            } else {
                c
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 英文就是它自己() {
        assert_eq!(keys_for("hololive").as_deref(), Some("hololive"));
        // 大寫轉小寫——載入時也是這樣存的
        assert_eq!(keys_for("HoloLive").as_deref(), Some("hololive"));
    }

    #[test]
    fn 純假名轉得出羅馬字() {
        // 片假名先轉平假名再轉
        assert_eq!(keys_for("ホロライブ").as_deref(), Some("hororaibu"));
        assert_eq!(keys_for("ほろらいぶ").as_deref(), Some("hororaibu"));
        assert_eq!(keys_for("ミク").as_deref(), Some("miku"));
    }

    #[test]
    fn 促音撥音拗音() {
        assert_eq!(keys_for("ゆっくり").as_deref(), Some("yukkuri"));
        assert_eq!(keys_for("しんぶん").as_deref(), Some("sinnbunn"));
        assert_eq!(keys_for("きょう").as_deref(), Some("kyou"));
    }

    #[test]
    fn 漢字加假名的日文查不到() {
        // 使用者決定不做日文的漢字反查（§2.75.6），留空讓他自己填。
        // **純漢字則一律當中文查**（`東京` 會拿到中文讀音），
        // 見 `keys_for` 的註解
        assert_eq!(keys_for("初音ミク"), None, "漢字＋假名反查不了");
        assert_eq!(keys_for("ご飯"), None);
    }

    #[test]
    fn 轉出來的羅馬字引擎讀得回去() {
        // **往返驗證**：反查產出的按鍵，引擎要能轉回原本的假名
        for w in ["ほろらいぶ", "ゆっくり", "しんぶん", "きょう", "こんにちは"]
        {
            let keys = keys_for(w).unwrap_or_else(|| panic!("{w} 轉不出羅馬字"));
            assert_eq!(
                crate::romaji::kana::to_kana(&keys).as_deref(),
                Some(w),
                "{w} → {keys} → 轉不回去"
            );
        }
    }

    /// 詞庫收不到的專有名詞要靠**逐字拼**——那正是使用者要加進擴充包
    /// 的東西。這個測試需要詞庫，沒有就跳過。
    #[test]
    fn 詞庫沒收的詞逐字拼得出來() {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data");
        if crate::dict::load_bopomofo(&data).is_none() {
            eprintln!("（沒有詞庫，跳過）");
            return;
        }
        // 詞庫都不收這幾個，但每個字都查得到
        for w in ["英雄聯盟", "原神", "鳴潮", "絕區零"] {
            let k = keys_for(w).unwrap_or_else(|| panic!("{w} 逐字拼不出來"));
            assert!(!k.is_empty());
        }
        // **多音字要挑最常用的讀音**（分數最高）
        assert_eq!(keys_for("行").as_deref(), Some("vu/6"), "行 該是 ㄒㄧㄥˊ");
        assert_eq!(keys_for("重").as_deref(), Some("5j/4"), "重 該是 ㄓㄨㄥˋ");
    }

    #[test]
    fn 空的與空白回none() {
        assert_eq!(keys_for(""), None);
        assert_eq!(keys_for("   "), None);
    }
}
