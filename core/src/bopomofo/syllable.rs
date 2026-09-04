//! 注音音節的合法性驗證。
//!
//! 依據 `字元規則/通用注音合法規則.canvas` 與 `通用注音例外規則.canvas`。
//! 那兩份 canvas 是可執行的規格——每個判斷節點恰好兩條出邊（綠=是、
//! 紅=否），這裡是它們的直譯。

use super::keymap::{self, Role};

/// 一個音節的驗證結果。
///
/// **只有兩種，沒有 `Partial`**——使用者定的規格是「沒打完 = 非法」。
/// 打到一半時往下一條語言問即可，顯示什麼交給之後的「偏好語言」功能。
/// 少了中間態，判斷樹只有兩個出口，規則大幅簡化。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Validity {
    Valid,
    Invalid,
}

/// 可以單獨成一個音節的 7 個聲母。
///
/// ㄓㄔㄕㄖㄗㄘㄙ 這幾個捲舌音／舌尖音自己就是一個字（「知」=ㄓ、
/// 「資」=ㄗ），其餘 14 個聲母必須接介音或韻母（「ㄅ」單獨不成字）。
///
/// 這是聲韻學的固定規則，不是啟發式猜測——`BPMFMappings.txt` 裡確實
/// 有 `ㄓ`/`ㄔ`/`ㄗ` 這種單符號音節，但沒有 `ㄅ` 這種。
const STANDALONE_INITIALS: [char; 7] = ['ㄓ', 'ㄔ', 'ㄕ', 'ㄖ', 'ㄗ', 'ㄘ', 'ㄙ'];

/// 驗證一個音節的按鍵序列。
///
/// # 合法規則（canvas 的直譯）
///
/// 依序嘗試匹配四種角色，**順序固定、各最多一個**：
///
/// ```text
/// 聲母(0~1) → 介音(0~1) → 韻母(0~1) → 聲調(1)
/// ```
///
/// canvas 上每個判斷節點的「是」分支往下走一層（推進字元指標），
/// 「否」分支往右走（同一個字元問下一個角色）。走到底必須是聲調——
/// 八條路徑全部以「是聲調嗎?」收尾。
///
/// 唯一提前結束的是「聲母否、介音否、韻母否」，那代表第一個字元
/// 不是任何注音符號，直接非法。
///
/// # 為什麼聲調是必要的
///
/// **注音一律以聲調鍵收尾**。一聲在書寫時不標符號，但輸入時要按空白鍵。
/// 這條規則讓「有沒有聲調鍵」成為區分注音與英日的強信號——英文單字與
/// 日文羅馬字永遠不會在序列裡出現聲調鍵。
pub fn check(keys: &str) -> Validity {
    let chars: Vec<char> = keys.chars().collect();
    if chars.is_empty() {
        return Validity::Invalid;
    }

    let mut idx = 0usize;
    let mut initial: Option<char> = None;
    let mut has_medial = false;
    let mut has_final = false;

    // 聲母（0~1）
    if let Some(&c) = chars.first() {
        if keymap::role_of(c) == Some(Role::Initial) {
            initial = keymap::symbol_of(c);
            idx = 1;
        }
    }
    // 介音（0~1）
    if let Some(&c) = chars.get(idx) {
        if keymap::role_of(c) == Some(Role::Medial) {
            has_medial = true;
            idx += 1;
        }
    }
    // 韻母（0~1）
    if let Some(&c) = chars.get(idx) {
        if keymap::role_of(c) == Some(Role::Final) {
            has_final = true;
            idx += 1;
        }
    }
    // 聲調（必要）
    match chars.get(idx) {
        Some(&c) if keymap::role_of(c) == Some(Role::Tone) => idx += 1,
        _ => return Validity::Invalid,
    }

    // 掃完標準順序後還有剩，代表出現了順序外的符號
    // （聲母後又一個聲母、聲調後還有東西……），結構不可能合法。
    if idx != chars.len() {
        return Validity::Invalid;
    }

    // 例外規則：第一字元是聲母、第二字元是聲調時（也就是這個音節
    // 只有「聲母＋聲調」），那個聲母必須是能單獨成音節的 7 個之一。
    if initial.is_some() && !has_medial && !has_final {
        let ok = initial
            .map(|c| STANDALONE_INITIALS.contains(&c))
            .unwrap_or(false);
        return if ok {
            Validity::Valid
        } else {
            Validity::Invalid
        };
    }

    // 沒有聲母也沒有主體（介音／韻母）——只有一個聲調鍵，不成音節。
    if initial.is_none() && !has_medial && !has_final {
        return Validity::Invalid;
    }

    Validity::Valid
}

/// 把按鍵序列轉成注音符號字串。表外字元回 `None`。
///
/// 只做轉換不做驗證——驗證是 `check` 的職責。
/// 這串按鍵**有沒有可能**補成一個真的存在的音節？
///
/// 跟 `check` 的差別有兩層：
///
/// | | `check` | `viable` |
/// |---|---|---|
/// | 問的是 | 這串**完整**按鍵合不合法 | 這個**半成品**還有沒有救 |
/// | 判準 | 結構（聲介韻調的順序） | 詞庫裡真的有這個字 |
///
/// # 為什麼結構合法還不夠
///
/// `check("zm3")`（ㄈㄩˇ）回 `Valid`——聲母加介音加聲調，結構挑不出
/// 毛病。但中文裡**沒有 ㄈㄩ 這個音**，查詞庫是 0 個字。
///
/// 結構寬鬆在自動辨識時是對的（寧可多留一個候選，也不要漏掉），
/// 但鎖定注音要**在按下去的當下就擋掉**非法組合（新酷音的行為），
/// 就得問「這真的是一個字嗎」——那只有詞庫答得出來。
///
/// # 怎麼判斷
///
/// 窮舉所有可能的補全（補介音、補韻母、補聲調），只要有一種在詞庫裡
/// 查得到字就算有救。組合數 3×13×5 ＝ 195 種，打一個字算一次，
/// 而詞庫查詢是 HashSet，成本可以接受。
///
/// **詞庫沒載入時一律回 `true`**——那時無從判斷，擋掉會讓使用者
/// 什麼都打不出來。
pub fn viable(keys: &str) -> bool {
    if keys.is_empty() {
        return true;
    }
    if !crate::dict::bopomofo_loaded() {
        return true;
    }
    const TONES: [&str; 5] = ["3", "4", "6", "7", " "];
    const MEDIAL_KEYS: [&str; 3] = ["u", "j", "m"];
    const FINAL_KEYS: [&str; 13] = [
        "8", "i", "k", ",", "9", "o", "l", ".", "0", "p", ";", "/", "-",
    ];
    let exists = |k: &str| !crate::dict::chars_for(k).is_empty();

    // 已經是完整音節（含聲調）就直接查
    if exists(keys) {
        return true;
    }
    for t in TONES {
        if exists(&format!("{keys}{t}")) {
            return true;
        }
        for f in FINAL_KEYS {
            if exists(&format!("{keys}{f}{t}")) {
                return true;
            }
        }
        for m in MEDIAL_KEYS {
            if exists(&format!("{keys}{m}{t}")) {
                return true;
            }
            for f in FINAL_KEYS {
                if exists(&format!("{keys}{m}{f}{t}")) {
                    return true;
                }
            }
        }
    }
    false
}

pub fn to_symbols(keys: &str) -> Option<String> {
    keys.chars().map(keymap::symbol_of).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid(keys: &str) -> bool {
        check(keys) == Validity::Valid
    }

    #[test]
    fn 完整音節_聲母介音韻母聲調() {
        // ㄋㄧㄠˇ 鳥
        assert!(valid("sul3"));
        // ㄐㄧㄤˋ 醬
        assert!(valid("ru;4"));
    }

    #[test]
    fn 聲母加韻母加聲調() {
        // ㄇㄚˉ 媽
        assert!(valid("a8 "));
        // ㄏㄠˇ 好
        assert!(valid("cl3"));
    }

    #[test]
    fn 聲母加介音加聲調() {
        // ㄋㄧˇ 你
        assert!(valid("su3"));
        // ㄒㄩˊ 徐
        assert!(valid("vm6"));
    }

    #[test]
    fn 沒有聲母的音節() {
        // ㄧˉ 一
        assert!(valid("u "));
        // ㄞˋ 愛
        assert!(valid("94"));
        // ㄨㄛˇ 我
        assert!(valid("ji3"));
    }

    #[test]
    fn 一聲用空白鍵() {
        // ㄊㄧㄢˉ 天
        assert!(valid("wu0 "));
        // ㄐㄧㄣˉ 今
        assert!(valid("rup "));
    }

    #[test]
    fn 例外_七個聲母可以單獨成音節() {
        // 「知」=ㄓ、「吃」=ㄔ、「詩」=ㄕ、「日」=ㄖ、
        // 「資」=ㄗ、「詞」=ㄘ、「思」=ㄙ
        for keys in ["5 ", "t ", "g ", "b ", "y ", "h ", "n "] {
            assert!(valid(keys), "{keys:?} 應該合法");
        }
        // 帶其他聲調也一樣
        assert!(valid("54"), "ㄓˋ 至");
        assert!(valid("g6"), "ㄕˊ 十");
    }

    #[test]
    fn 例外_其餘十四個聲母單獨不成音節() {
        // ㄅ ㄆ ㄇ ㄈ ㄉ ㄊ ㄋ ㄌ ㄍ ㄎ ㄏ ㄐ ㄑ ㄒ
        for keys in [
            "1 ", "q ", "a ", "z ", "2 ", "w ", "s ", "x ", "e ", "d ", "c ", "r ", "f ", "v ",
        ] {
            assert!(!valid(keys), "{keys:?} 不該合法（聲母單獨不成字）");
        }
    }

    #[test]
    fn 沒有聲調就非法() {
        // 這是「注音一律以聲調鍵收尾」的直接後果。
        assert!(!valid("su"), "ㄋㄧ 缺聲調");
        assert!(!valid("cl"), "ㄏㄠ 缺聲調");
        assert!(!valid("5"), "ㄓ 缺聲調");
        assert!(!valid("wu0"), "ㄊㄧㄢ 缺聲調");
    }

    #[test]
    fn 順序錯了就非法() {
        assert!(!valid("8s "), "韻母在聲母前");
        assert!(!valid("us "), "介音在聲母前");
        assert!(!valid("3s "), "聲調在最前");
    }

    #[test]
    fn 同一種角色不能出現兩次() {
        assert!(!valid("ss8 "), "兩個聲母");
        assert!(!valid("suu "), "兩個介音");
        assert!(!valid("s88 "), "兩個韻母");
        assert!(!valid("su3 "), "兩個聲調");
    }

    #[test]
    fn 聲調之後不能有東西() {
        assert!(!valid("su3s"), "聲調後還有聲母");
        assert!(!valid("cl3 "), "聲調後還有聲調");
    }

    #[test]
    fn 表外字元一律非法() {
        assert!(!valid("@"), "@ 不在對照表裡");
        assert!(!valid("su3@"), "夾雜表外字元");
    }

    #[test]
    fn 空字串非法() {
        assert!(!valid(""));
    }

    #[test]
    fn 只有聲調非法() {
        // 單獨一個空白鍵（一聲）不成音節。
        assert!(!valid(" "));
        assert!(!valid("3"));
    }

    #[test]
    fn 英文單字幾乎都不是合法注音() {
        // 這是語言辨識的前提：注音必須以聲調鍵收尾，而英文沒有聲調鍵。
        for w in ["hello", "world", "check", "notebook", "javascript"] {
            assert!(!valid(w), "{w:?} 不該是合法注音");
        }
    }

    #[test]
    fn 轉成注音符號() {
        assert_eq!(to_symbols("su3"), Some("ㄋㄧˇ".to_string()));
        assert_eq!(to_symbols("wu0 "), Some("ㄊㄧㄢˉ".to_string()));
        assert_eq!(to_symbols("@"), None);
    }
}
