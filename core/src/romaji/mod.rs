//! 日文羅馬字引擎：判斷一串按鍵在日文下合不合法。
//!
//! 跟注音引擎一樣，**這一層只回答「合不合法」，不查詞庫、不產生候選**。
//!
//! 規格來源：
//! - `字元規則/日文按鍵合法表.md`（按鍵分類與附加條件）
//! - `字元規則/通用日文合法規則.canvas`（判斷樹，含促音的 offset 機制）
//! - `字元規則/通用日文例外規則.canvas`（`c`+母音、`wu`/`yi`/`yyyi`）

pub mod convert;
pub mod kana;
pub mod keymap;
pub mod mora;

pub use mora::Validity;

/// 例外規則：主表放行但組不出假名的組合。
///
/// 依 `通用日文例外規則.canvas`，兩條規則（含促音形式）：
///
/// 1. **`c` 不直接接母音**——か 是 `ka` 不是 `ca`；`c` 只出現在
///    `cha`（ちゃ）與 `cyi`（ちぃ）
/// 2. **`wu` / `yi` / `yyyi`**——う 只有 `u`、い 只有 `i`，沒有 w/y 打法
///
/// 促音形式（`cca`、`wwu`、`yyi`）用 offset 一併處理：剝掉促音之後
/// 再判斷一次。
fn is_exception(keys: &str, offset: usize) -> bool {
    let c: Vec<char> = keys.chars().collect();
    // 例外一：第(1+offset)字元是 c 且第(2+offset)字元是母音
    if let (Some(&a), Some(&b)) = (c.get(offset), c.get(offset + 1)) {
        if a == 'c' && keymap::is_vowel(b) {
            return true;
        }
    }
    // 例外二：從 offset 起算等於 wu / yi / yyyi
    let rest: String = c[offset.min(c.len())..].iter().collect();
    matches!(rest.as_str(), "wu" | "yi" | "yyyi") || matches!(keys, "wu" | "yi" | "yyyi")
}

/// 這個 mora 的促音偏移量：促音成立時是 1，否則 0。
fn offset_of(keys: &str) -> usize {
    let c: Vec<char> = keys.chars().collect();
    if c.len() >= 3 && c[0] == c[1] && c[0] != 'n' && keymap::is_consonant(c[0]) {
        1
    } else {
        0
    }
}

/// 驗證一個 mora：合法規則 → 例外規則。
///
/// 兩層串接的順序很重要——**例外表只在合法之後才跑**。主表判非法的
/// 東西，例外表永遠救不回來（那是使用者定的：例外只能「合法之中再
/// 篩掉」，不能「非法之中撈回來」）。
pub fn check_mora(keys: &str) -> Validity {
    if mora::check(keys) == Validity::Invalid {
        return Validity::Invalid;
    }
    if is_exception(keys, offset_of(keys)) {
        return Validity::Invalid;
    }
    Validity::Valid
}

/// 把一串按鍵切成 mora。
///
/// 貪婪由長到短（最長 4 個字元：促音＋3 字母 mora，例如 `ssha`）。
/// 必須由長到短，否則 `kyo` 會先被切成 `ky`+`o`——但那樣 `ky` 不合法。
///
/// 長音 `-` 特別處理：它不是獨立的 mora，而是「延長前一個假名」，
/// 所以只有在前面已經有 mora 時才能接。
///
/// 切不完就回 `None`——依「沒打完 = 非法」的規格。
pub fn split_moras(keys: &str) -> Option<Vec<String>> {
    let chars: Vec<char> = keys.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        // 長音：接在已有的 mora 後面
        if keymap::is_chouon(chars[i]) {
            if out.is_empty() {
                return None; // 開頭不能是長音
            }
            out.push("-".to_string());
            i += 1;
            continue;
        }

        let mut matched = None;
        for len in (1..=4.min(chars.len() - i)).rev() {
            let seg: String = chars[i..i + len].iter().collect();
            if check_mora(&seg) == Validity::Valid {
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

/// 這串按鍵在日文下合不合法。
pub fn validity(keys: &str) -> Validity {
    if keys.is_empty() {
        return Validity::Invalid;
    }
    match split_moras(keys) {
        Some(_) => Validity::Valid,
        None => Validity::Invalid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(k: &str) -> bool {
        validity(k) == Validity::Valid
    }

    #[test]
    fn 單一_mora() {
        for k in ["a", "ka", "kya", "tsu", "nn"] {
            assert!(v(k), "{k:?}");
        }
    }

    #[test]
    fn 多_mora() {
        assert_eq!(
            split_moras("arigatou"),
            Some(vec![
                "a".into(),
                "ri".into(),
                "ga".into(),
                "to".into(),
                "u".into()
            ])
        );
        assert_eq!(split_moras("sushi"), Some(vec!["su".into(), "shi".into()]));
    }

    #[test]
    fn 撥音要打_nn() {
        // 多語言輸入法的取捨：撥音一律打 nn，邊界才不依賴後文。
        assert!(v("konnnichiha"), "こんにちは");
        assert!(v("gannbatte"), "がんばって");
        assert!(v("dennwa"), "でんわ");
    }

    #[test]
    fn 促音() {
        assert!(v("gakkou"), "がっこう");
        assert!(v("itta"), "いった");
        assert!(v("issho"), "いっしょ");
    }

    #[test]
    fn 長音() {
        assert!(v("ra-menn"), "ラーメン");
        assert!(!v("-men"), "長音不能在開頭");
    }

    #[test]
    fn 例外_c不直接接母音() {
        for k in ["ca", "ce", "ci", "co", "cu"] {
            assert!(!v(k), "{k:?} 不該合法");
        }
        // 但 cha / cyi 合法
        assert!(v("cha"), "ちゃ");
    }

    #[test]
    fn 例外_wu與yi() {
        assert!(!v("wu"), "う 只有 u");
        assert!(!v("yi"), "い 只有 i");
    }

    #[test]
    fn 英文單字多半不是合法日文() {
        for w in ["javascript", "keyboard", "password"] {
            assert!(!v(w), "{w:?}");
        }
    }

    #[test]
    fn 空字串非法() {
        assert!(!v(""));
        assert_eq!(split_moras(""), None);
    }

    #[test]
    fn 貪婪由長到短() {
        // `kyo` 若由短到長會先切出 `ky`——那不合法。
        assert_eq!(split_moras("kyo"), Some(vec!["kyo".into()]));
        assert_eq!(split_moras("ssha"), Some(vec!["ssha".into()]));
    }
}
