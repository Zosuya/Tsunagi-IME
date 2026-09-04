//! 注音引擎：判斷一串按鍵在注音下合不合法。
//!
//! **這一層只回答「合不合法」，不查詞庫、不產生候選。**
//! 查詞庫是選詞模組的職責，跟合法性判斷是兩件事——舊架構把兩者混在
//! 一起，導致「這不可能是注音」與「這是罕見的注音詞」都表現為低分，
//! 切點正確率只有 40.8%。見開發文件 §2.6。
//!
//! 規格來源：
//! - `注音按鍵合法表及對照表.md`（鍵位，使用者已核對）
//! - `字元規則/通用注音合法規則.canvas`（音節結構）
//! - `字元規則/通用注音例外規則.canvas`（7 個可單獨成音節的聲母）

pub mod buffer;
pub mod keymap;
pub mod syllable;

pub use syllable::Validity;

/// 把一串按鍵切成音節。
///
/// 貪婪由長到短：從目前位置往後試最長 4 個字元（聲母+介音+韻母+聲調
/// 是注音音節的上限），第一個合法的就收下。
///
/// 切不完就回 `None`——依「沒打完 = 非法」的規格，這一層不接受殘餘。
pub fn split_syllables(keys: &str) -> Option<Vec<String>> {
    let chars: Vec<char> = keys.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let mut matched = None;
        // 由長到短：注音音節最長 4 個字元（ㄋㄧㄠˇ = sul3）。
        // 必須由長到短，否則 `su3` 會先被切成 `s`+`u3`——但 `s` 單獨
        // 不是合法音節，貪婪最長才切得對。
        for len in (1..=4.min(chars.len() - i)).rev() {
            let seg: String = chars[i..i + len].iter().collect();
            if syllable::check(&seg) == Validity::Valid {
                matched = Some((seg, len));
                break;
            }
        }
        match matched {
            Some((seg, len)) => {
                out.push(seg);
                i += len;
            }
            None => return None,
        }
    }

    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// 這串按鍵在注音下合不合法。
///
/// 整串必須切得成一連串完整的音節，不留殘餘。
pub fn validity(keys: &str) -> Validity {
    if keys.is_empty() {
        return Validity::Invalid;
    }
    match split_syllables(keys) {
        Some(_) => Validity::Valid,
        None => Validity::Invalid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid(keys: &str) -> bool {
        validity(keys) == Validity::Valid
    }

    #[test]
    fn 單音節() {
        assert!(valid("su3"), "你");
        assert!(valid("cl3"), "好");
    }

    #[test]
    fn 多音節() {
        assert_eq!(
            split_syllables("su3cl3"),
            Some(vec!["su3".into(), "cl3".into()]),
            "你好"
        );
        assert_eq!(
            split_syllables("rup wu0 "),
            Some(vec!["rup ".into(), "wu0 ".into()]),
            "今天"
        );
    }

    #[test]
    fn 一聲的空白留在音節裡() {
        // 「今天」的兩個空白都是一聲，不是分隔符——這是切點引擎最難的
        // 一段的根源，在這一層要先切對。
        let s = split_syllables("rup wu0 ").unwrap();
        assert_eq!(s.len(), 2);
        assert!(s[0].ends_with(' '));
        assert!(s[1].ends_with(' '));
    }

    #[test]
    fn 貪婪由長到短() {
        // `su3` 若由短到長會先切出 `s`——但 `s` 單獨不合法，
        // 所以這裡必須由長到短。
        assert_eq!(split_syllables("su3"), Some(vec!["su3".into()]));
    }

    #[test]
    fn 切不完就非法() {
        assert!(!valid("su3s"), "尾巴的 s 湊不出音節");
        assert!(!valid("hello"), "英文");
    }

    #[test]
    fn 空字串非法() {
        assert!(!valid(""));
        assert_eq!(split_syllables(""), None);
    }

    #[test]
    fn 四字詞() {
        // 「今天天氣」
        let s = split_syllables("rup wu0 wu0 fu4").unwrap();
        assert_eq!(s.len(), 4, "切出四個音節：{s:?}");
    }
}
