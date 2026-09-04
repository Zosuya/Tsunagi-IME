//! 全形／半形轉換。
//!
//! # 三種模式
//!
//! | 模式 | 行為 |
//! |---|---|
//! | `Auto` | 前面是中日文就全形，是英文就半形 |
//! | `Half` | 一律半形 |
//! | `Full` | 一律全形 |
//!
//! `Auto` 是預設——中文排版習慣用全形標點，但夾雜英文或程式碼時
//! 半形才對。看**前面那一段**判斷，因為打到符號時後面還沒有字。
//!
//! # 為什麼轉換放在 core
//!
//! 「這個字元的全形版本是什麼」跟平台無關。Windows、macOS 都是
//! 同一張對照表。平台層只負責提供「現在是哪個模式」。

use crate::language::Language;

/// 全半形模式。
///
/// 序列化只在 `config` feature 下掛上——`core` 要能在不帶任何依賴的
/// 情況下編譯（純演算法工具用 `--no-default-features`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(
    feature = "config",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub enum Width {
    /// 前面是中日文就全形，是英文就半形
    #[default]
    Auto,
    /// 一律半形
    Half,
    /// 一律全形
    Full,
}

impl Width {
    /// Shift+Space 切換：三態輪流。
    pub fn next(self) -> Self {
        match self {
            Width::Auto => Width::Half,
            Width::Half => Width::Full,
            Width::Full => Width::Auto,
        }
    }

    /// 顯示用的名稱。
    pub fn label(self) -> &'static str {
        match self {
            Width::Auto => "自動",
            Width::Half => "半形",
            Width::Full => "全形",
        }
    }
}

/// 半形轉全形。
///
/// # 範圍
///
/// ASCII 的 `!`（0x21）到 `~`（0x7E）在 Unicode 有一段連續的全形
/// 對應（0xFF01 到 0xFF5E），差值固定是 0xFEE0——**標點、英文、
/// 數字全部包含在內**（使用者選的）。
///
/// 空白是唯一的例外：它的全形是 U+3000（表意空格），不在那段連續
/// 區間裡。
pub fn to_full(c: char) -> char {
    match c {
        ' ' => '\u{3000}',
        '!'..='~' => char::from_u32(c as u32 + 0xFEE0).unwrap_or(c),
        _ => c,
    }
}

/// 全形轉半形。`to_full` 的反向。
pub fn to_half(c: char) -> char {
    match c {
        '\u{3000}' => ' ',
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
        _ => c,
    }
}

/// 這個半形標點在中日文裡寫成什麼？
///
/// # 為什麼不能用 `to_full`
///
/// `to_full` 走的是 ASCII → 全形的固定偏移（0xFEE0），那給出的是
/// 「全形的英文標點」而不是「中日文標點」：`.` 會變 `．`（U+FF0E）
/// 而不是句號 `。`，`,` 會變 `，` 而中文對、日文錯（日文的讀點是
/// `、`）。中日文標點在 Unicode 的位置跟 ASCII 沒有對應關係，
/// 只能查表。
///
/// # 只收有共識的那幾個
///
/// 表外的一律維持半形。`@`、`#`、`%`、`$` 這些雖然都有全形字符，
/// 但中文排版並沒有「要用全形」的習慣——測資的 `你好@` 期望的就是
/// 半形（`su3cl3@`）。寧可少轉，多轉了使用者得自己刪掉重打。
///
/// 引號沒收：`「」` 要分開頭與結尾，得記狀態，不是這一層的事。
fn to_cjk_punct(c: char, lang: Language) -> Option<char> {
    let ja = lang == Language::Romaji;
    Some(match c {
        // 日文的讀點是 `、`，中文的逗號是 `，`——同一個鍵、兩種字
        ',' => {
            if ja {
                '\u{3001}'
            } else {
                '\u{FF0C}'
            }
        }
        // 句號兩邊一樣
        '.' => '\u{3002}',
        '?' => '\u{FF1F}',
        '!' => '\u{FF01}',
        ';' => '\u{FF1B}',
        ':' => '\u{FF1A}',
        '(' => '\u{FF08}',
        ')' => '\u{FF09}',
        _ => return None,
    })
}

/// 依模式與前文決定要不要轉全形。
///
/// `prev_lang` 是**前面那一段**的語言，`None` 代表前面沒東西
/// （句首）。句首沒有依據，當成半形——那是比較安全的預設，
/// 打程式碼時不會突然冒出全形符號。
///
/// # 空白在 `Auto` 下一律半形
///
/// 標點跟著前文走（中文旁邊用全形逗號），但**空白不是標點，是分隔**
/// ——`麻煩 review` 裡那一格的職責是隔開中文與英文，不是排版。中文
/// 旁邊給全形空白會變成 `麻煩　review`，兩邊的字距明顯不對。
/// 2026-09-01 使用者裁決：分隔空白一律半形。
///
/// `Full` 不受影響——那是使用者明講「全部都要全形」的模式，
/// 連空白也是他要的。
pub fn convert(c: char, mode: Width, prev_lang: Option<Language>) -> char {
    match mode {
        Width::Half => to_half(c),
        // **全形模式也要先查中日文標點表**。
        //
        // 原本直接 `to_full`，於是 `.` 變成 `．`（U+FF0E，全形的**英文**
        // 句點）而不是句號 `。`——這一個檔案上面就寫著 `to_full` 給的是
        // 英文標點，但 `Full` 那一支自己踩了進去。日文的 `,` 同理，
        // 給了 `，` 而正確的是讀點 `、`。
        //
        // 語言不明時當中文——選了全形模式的人幾乎都在打中日文，而中日
        // 兩者只有逗號不同，猜中文的期望損失最小。
        Width::Full => {
            let lang = cjk_lang(prev_lang).unwrap_or(Language::Bopomofo);
            to_cjk_punct(to_half(c), lang).unwrap_or_else(|| to_full(c))
        }
        // 空白是分隔不是標點，不跟著前文轉全形
        Width::Auto if c == ' ' || c == '\u{3000}' => ' ',
        // 中日文旁邊查中日文標點表，表外的維持半形
        Width::Auto => match cjk_lang(prev_lang) {
            Some(lang) => to_cjk_punct(to_half(c), lang).unwrap_or_else(|| to_half(c)),
            None => to_half(c),
        },
    }
}

/// 這個標點有哪些**可以選的寫法**？第一個是預設，其餘依常用度排。
///
/// # 為什麼標點需要候選
///
/// `[` 在中日文裡有一整排對應：`「『【〔［《〈`。使用者想要哪一種
/// 引擎猜不到，而**猜錯的代價是使用者得刪掉重打**——那正是選字要解決
/// 的問題，只是以前沒把它用在標點上。
///
/// 選過兩次會被學習層記住（鍵是按鍵、值是文字，跟選字同一條路），
/// 所以「記住你慣用的括號樣式」不必另外寫機制。
///
/// # 只給不用 Shift 的鍵
///
/// 使用者裁決（2026-09-04）：Shift 系列的符號是明確按出來的，意圖很
/// 清楚（打 `!` 就是要驚嘆號），不需要再選。
///
/// # 一鍵兩用不是問題
///
/// `, . ; / -` 在大千配置上是ㄝㄡㄤㄥㄦ，但**那個歧義在切點階段就解決
/// 完了**（`punct::is_punct` 會往後看聲調）。走到這裡代表引擎已經確定
/// 它是標點，再給候選不會跟注音打架。
///
/// # 回傳空的代表「這個符號沒有選的必要」
///
/// `@`、`#`、`$` 那些中文排版沒有全形習慣，給候選只是干擾。
pub fn variants(keys: &str, lang: Option<Language>) -> &'static [char] {
    // 多字元的組合另外查（`...` → `…`）
    if keys.chars().count() > 1 {
        return combo_variants(keys);
    }
    let Some(c) = keys.chars().next() else {
        return &[];
    };
    let ja = matches!(lang, Some(Language::Romaji));
    match c {
        '[' => &['「', '『', '【', '〔', '［', '《', '〈'],
        ']' => &['」', '』', '】', '〕', '］', '》', '〉'],
        '\'' => &['‘', '’', '『', '』'],
        // 中日的第一個不同——逗號是讀點還是逗號，跟 `to_cjk_punct` 一致
        ',' if ja => &['、', '，', '；'],
        ',' => &['，', '、', '；'],
        '.' => &['。', '…', '．', '‧'],
        ';' => &['；', '：'],
        // `？` 放進來，不按 Shift 也打得到問號
        '/' => &['/', '／', '？', '・'],
        '-' => &['-', '－', '—', '～', '‧'],
        _ => &[],
    }
}

/// 這個開括號配哪個收括號？不是成對的符號回 `None`。
///
/// # 為什麼要這張表
///
/// 使用者把 `[` 選成 `「` 之後，後面那個 `]` 還是 `]`——得再選一次，
/// 而且**要記得自己剛才選了哪一種**。成對的東西本來就該一起變。
///
/// # 只做方括號
///
/// `'` 的開與收是同一個鍵，配不出來（`don't` 的撇號也走這裡）。
/// 引號那類要記狀態才分得出開閉，那正是 `to_cjk_punct` 當初不收引號
/// 的理由——這裡不重蹈覆轍。
pub fn closing_for(open: char) -> Option<char> {
    Some(match open {
        '「' => '」',
        '『' => '』',
        '【' => '】',
        '〔' => '〕',
        '［' => '］',
        '《' => '》',
        '〈' => '〉',
        '[' => ']',
        _ => return None,
    })
}

/// 多個標點連在一起時的寫法。`...` → `…` 是唯一一組。
fn combo_variants(keys: &str) -> &'static [char] {
    match keys {
        // 中文的刪節號正式寫法是兩個六點省略號（`……`），但那要兩格；
        // 這裡給一個，要兩個就打六個點
        "..." => &['…', '⋯'],
        "......" => &['…', '⋯'],
        _ => &[],
    }
}

/// 這個語言要用中日文標點嗎？是的話回傳它，英文回 `None`。
fn cjk_lang(lang: Option<Language>) -> Option<Language> {
    match lang {
        Some(l @ (Language::Bopomofo | Language::Romaji)) => Some(l),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 標點英文數字都轉() {
        // 使用者選的：全形模式下全部都轉
        assert_eq!(to_full('!'), '！');
        assert_eq!(to_full(','), '，');
        assert_eq!(to_full('a'), 'ａ');
        assert_eq!(to_full('1'), '１');
        assert_eq!(to_full('@'), '＠');
    }

    /// **全形模式的句點是句號，不是全形英文句點**。
    ///
    /// `to_full('.')` 給的是 `．`（U+FF0E）——那是全形的**英文**句點。
    /// 中日文要的是 `。`。修之前 `Full` 那一支直接走 `to_full`，
    /// 所以全形模式下打句點會出 `．`。
    #[test]
    fn 全形模式要給中日文標點() {
        use Language::{Bopomofo, English, Romaji};
        let full = |c, lang| convert(c, Width::Full, lang);

        assert_eq!(full('.', Some(Bopomofo)), '。', "中文句號");
        assert_eq!(full('.', Some(Romaji)), '。', "日文句號");
        // 語言不明時當中文——選全形的人幾乎都在打中日文
        assert_eq!(full('.', None), '。');
        assert_eq!(full('.', Some(English)), '。');

        // 逗號是中日唯一不同的那個
        assert_eq!(full(',', Some(Bopomofo)), '，', "中文逗號");
        assert_eq!(full(',', Some(Romaji)), '、', "日文讀點");

        // 表外的仍照 to_full 走——那才是「全部都要全形」的語意
        assert_eq!(full('a', None), 'ａ');
        assert_eq!(full('1', None), '１');
        assert_eq!(full('@', None), '＠');
        assert_eq!(full(' ', None), '\u{3000}');
    }

    #[test]
    fn 空白是特例() {
        // 全形空白不在 0xFF01..0xFF5E 那段連續區間裡
        assert_eq!(to_full(' '), '\u{3000}');
        assert_eq!(to_half('\u{3000}'), ' ');
    }

    #[test]
    fn 轉換可以往返() {
        for c in "!@#$%^&*()abcXYZ123,.;:".chars() {
            assert_eq!(to_half(to_full(c)), c, "{c} 往返後該一樣");
        }
        assert_eq!(to_half(to_full(' ')), ' ');
    }

    #[test]
    fn 中日文旁邊自動轉全形() {
        assert_eq!(convert('!', Width::Auto, Some(Language::Bopomofo)), '！');
        assert_eq!(convert('!', Width::Auto, Some(Language::Romaji)), '！');
        // 英文旁邊保持半形——打程式碼時不能冒出全形
        assert_eq!(convert('!', Width::Auto, Some(Language::English)), '!');
    }

    #[test]
    fn 中日文標點查表而不是套全形偏移() {
        // 句號是 。 不是 ．（ASCII 偏移給的那個）
        assert_eq!(convert('.', Width::Auto, Some(Language::Bopomofo)), '。');
        assert_eq!(convert('.', Width::Auto, Some(Language::Romaji)), '。');
        // 逗號兩種語言不同字：中文 ，／日文 、
        assert_eq!(convert(',', Width::Auto, Some(Language::Bopomofo)), '，');
        assert_eq!(convert(',', Width::Auto, Some(Language::Romaji)), '、');
        // 英文旁邊兩個都維持半形
        assert_eq!(convert('.', Width::Auto, Some(Language::English)), '.');
        assert_eq!(convert(',', Width::Auto, Some(Language::English)), ',');
    }

    #[test]
    fn 表外的標點維持半形() {
        // 中文排版沒有「@ 要用全形」的習慣——測資的 你好@ 期望半形
        assert_eq!(convert('@', Width::Auto, Some(Language::Bopomofo)), '@');
        assert_eq!(convert('#', Width::Auto, Some(Language::Bopomofo)), '#');
        assert_eq!(convert('%', Width::Auto, Some(Language::Romaji)), '%');
        // Full 是使用者明講全部都要全形，表外的照樣轉
        assert_eq!(convert('@', Width::Full, Some(Language::Bopomofo)), '＠');
    }

    #[test]
    fn 空白在_auto_下一律半形() {
        // 空白是分隔不是標點——中文旁邊也不轉全形（使用者 2026-09-01 裁決）
        assert_eq!(convert(' ', Width::Auto, Some(Language::Bopomofo)), ' ');
        assert_eq!(convert(' ', Width::Auto, Some(Language::Romaji)), ' ');
        assert_eq!(convert(' ', Width::Auto, Some(Language::English)), ' ');
        assert_eq!(convert(' ', Width::Auto, None), ' ');
        // 已經是全形空白的也拉回半形
        assert_eq!(
            convert('\u{3000}', Width::Auto, Some(Language::Bopomofo)),
            ' '
        );
        // 同一個位置的標點仍然跟著前文轉——只有空白是特例
        assert_eq!(convert(',', Width::Auto, Some(Language::Bopomofo)), '，');
        // Full 是使用者明講要全形，空白照樣全形
        assert_eq!(
            convert(' ', Width::Full, Some(Language::Bopomofo)),
            '\u{3000}'
        );
    }

    #[test]
    fn 句首當半形() {
        // 前面沒東西時沒有依據，半形比較安全
        assert_eq!(convert('!', Width::Auto, None), '!');
    }

    #[test]
    fn 固定模式不看前文() {
        assert_eq!(convert('!', Width::Full, Some(Language::English)), '！');
        assert_eq!(convert('!', Width::Half, Some(Language::Bopomofo)), '!');
    }

    #[test]
    fn 三態輪流切換() {
        let m = Width::Auto;
        let m = m.next();
        assert_eq!(m, Width::Half);
        let m = m.next();
        assert_eq!(m, Width::Full);
        let m = m.next();
        assert_eq!(m, Width::Auto, "轉一圈回到原點");
    }

    #[test]
    fn 不認得的字元原樣不動() {
        // 中文本身沒有半形版本
        assert_eq!(to_full('你'), '你');
        assert_eq!(to_half('你'), '你');
    }
}
