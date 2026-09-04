//! 標點符號的切點判斷。
//!
//! 依據 `通用語言輸入法 篩選規則.canvas`：
//!
//! > 標點符號前後均視為切點
//! > 【-】接在合法日文字元後時【-】視為日文，後面為切點
//!
//! # 為什麼不能無條件當標點
//!
//! `,` `.` `;` `/` `-` 在注音鍵盤上是 ㄝㄡㄤㄥㄦ——**同一個按鍵的
//! 兩種身分**。無條件當標點會把注音切斷：
//!
//! ```text
//! 5;4cl4（帳號）→ 被 ; 切成 5|;|4|cl4
//! m/4（用）      → 被 / 切成 m|/|4
//! ```
//!
//! 判準是**這個字元能不能參與一個完整的注音音節**——含後面的聲調。
//! 只往前看會漏掉這件事：`5;4` 的 `;` 要看到後面的 `4` 才知道它是ㄤ。

use crate::bopomofo;
use crate::romaji;

/// 注音鍵盤上同時是注音符號的標點：`,` `.` `;` `/` `-`
const AMBIGUOUS: [char; 5] = [',', '.', ';', '/', '-'];

/// 這個鍵是不是「一鍵兩用」的那五個？
///
/// 大千配置上它們是 ㄝㄡㄤㄥㄦ，所以看到它不能直接當標點——鎖定注音
/// 的輸入層也要問這件事（見 `input::BopomofoInput` 的待決標點）。
pub fn is_ambiguous(c: char) -> bool {
    AMBIGUOUS.contains(&c)
}

/// 這個字元是不是標點（在這個位置）？
///
/// `keys` 是整串按鍵，`i` 是要判斷的位置。
pub fn is_punct(keys: &str, i: usize) -> bool {
    let chars: Vec<char> = keys.chars().collect();
    let Some(&c) = chars.get(i) else {
        return false;
    };

    // 空白和字母數字不是標點
    if c == ' ' || c.is_alphanumeric() {
        return false;
    }

    if !AMBIGUOUS.contains(&c) {
        // 純標點（`@` `!` `?` 這些注音鍵盤上沒有的）
        return true;
    }

    // `-` 接在合法日文字元後 → 長音，是日文的一部分
    if c == '-' && i > 0 {
        let lo = i.saturating_sub(8);
        for start in (lo..i).rev() {
            let prev: String = chars[start..i].iter().collect();
            if romaji::validity(&prev) == romaji::Validity::Valid {
                return false;
            }
        }
    }

    // 能不能參與一個完整的注音音節？
    //
    // 試各種起點與長度——**要往後看聲調**。`5;4` 的 `;` 單看是ㄤ，
    // 要看到 `4` 才知道 `5;4` 是合法音節（帳）。
    let lo = i.saturating_sub(4);
    for start in lo..=i {
        let hi = (i + 3).min(chars.len());
        for end in (i + 1)..=hi {
            let seg: String = chars[start..end].iter().collect();
            if bopomofo::syllable::check(&seg) == bopomofo::Validity::Valid {
                return false;
            }
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn punct_at(keys: &str, i: usize) -> bool {
        is_punct(keys, i)
    }

    #[test]
    fn 純標點() {
        assert!(punct_at("a@b", 1), "@");
        assert!(punct_at("hello!", 5), "!");
        assert!(punct_at("what?", 4), "?");
    }

    #[test]
    fn 注音相容的標點_在注音裡不算標點() {
        // `5;4cl4` = ㄓㄤˋㄎㄠˋ（帳號），`;` 是ㄤ
        assert!(!punct_at("5;4cl4", 1), "; 在 5;4 裡是ㄤ");
        // `m/4` = ㄩㄥˋ（用），`/` 是ㄥ
        assert!(!punct_at("m/4", 1), "/ 在 m/4 裡是ㄥ");
        // `2;404` = ㄉㄤˋㄢˋ（檔案）
        assert!(!punct_at("2;404", 1), "; 在檔案裡是ㄤ");
    }

    #[test]
    fn 注音後面的標點是標點() {
        // `su3cl3,` = 你好，
        assert!(punct_at("su3cl3,", 6), "你好後面的逗號");
        assert!(punct_at("su3cl3.", 6), "你好後面的句號");
    }

    #[test]
    fn 英文後面的標點是標點() {
        assert!(punct_at("hello,", 5));
        assert!(punct_at("hello.", 5));
    }

    #[test]
    fn 長音接在日文後不是標點() {
        // `ra-menn` = ラーメン，`-` 是長音
        assert!(!punct_at("ra-menn", 2), "ra 後面的 - 是長音");
        assert!(!punct_at("ko-hi-", 2), "ko 後面的 - 是長音");
    }

    #[test]
    fn 長音在開頭是標點() {
        // 前面沒有日文可延長
        assert!(punct_at("-abc", 0));
    }

    #[test]
    fn 連續標點() {
        for i in 5..8 {
            assert!(punct_at("hello...", i), "第 {i} 個字元");
        }
    }

    #[test]
    fn 空白不是標點() {
        assert!(!punct_at("a b", 1));
    }

    #[test]
    fn 字母數字不是標點() {
        assert!(!punct_at("abc", 1));
        assert!(!punct_at("a1c", 1));
    }
}
