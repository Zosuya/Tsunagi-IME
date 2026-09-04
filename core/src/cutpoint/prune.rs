//! 候選切法的丟棄規則。
//!
//! 切法的組合數是指數成長的——「金曜日までに提出しなければ」不設限
//! 有 **171 億**種切法。全留不可能，隨便砍又會砍掉正解。
//!
//! 這裡的兩條規則專門砍「不可能是答案」的切法。兩條都是**寧可留錯、
//! 不可殺對**：440 句測資的正解全數存活。
//!
//! | 規則 | 候選數中位 | 正解存活 |
//! |---|---|---|
//! | 不丟棄 | 4,480 | 440/440 |
//! | ＋單字母規則 | 195 | 440/440 |
//! | ＋英文詞規則 | **48** | **440/440** |
//!
//! 兩條要**一起用**才有效——單字母 `a`、`i`、`s` 本來就在英文詞典裡，
//! 英文詞規則擋不住它們；而單字母規則擋不住 `rchhel` 這種長碎片。
//!
//! 英文詞規則有一個例外：**邊界上的短縮寫**（`yt`、`fb`）。縮寫按定義
//! 不在詞典裡，被規則二殺掉的話「yt ＋ 中文」這種切法根本不會生成，
//! Tab 也救不回來。見 `boundary_acronym`。

use super::punct;
use crate::{bopomofo, romaji};

/// 這一段可以留著嗎？
///
/// `keys` 是完整按鍵串，`(start, end)` 是這一段在其中的字元範圍。
/// 需要完整的串是因為兩條規則都要**看前後文**——前後是不是邊界、
/// 自己是不是最後一段。
pub fn keep(keys: &str, chars: &[char], start: usize, end: usize) -> bool {
    if end > chars.len() || start >= end {
        return false;
    }
    let seg = super::slice(keys, chars, start, end);

    // 標點自成一段，不受這兩條規則管
    if end == start + 1 && punct::is_punct(keys, start) {
        return true;
    }
    // 分隔符同理
    if seg == super::SEPARATOR {
        return true;
    }

    single_letter_ok(keys, chars, start, end) && english_word_ok(chars, start, end, &seg)
}

/// 規則一：單字母段，前後皆為邊界時保留，否則丟棄。
///
/// # 為什麼要丟
///
/// 英文是瀑布的最後一站（passthrough），一次吃一個字元，所以任何
/// 字串都能被切成一堆單字母。`su3cl3`（你好）會生出
/// `s|u|3|c|l|3` 這種純粹是殘渣的切法。
///
/// # 為什麼不能全丟
///
/// `a`、`I` 是英文最常用的兩個詞。全丟的話 `a␣banana`、`I␣am`
/// 都切不出來——正解的第一段就是單字母。
///
/// # 邊界怎麼判
///
/// 前後是空白、標點或字串端，就是一個獨立的詞；否則是切碎的殘渣。
///
/// ```text
/// a␣banana  的 a → 前面是開頭、後面是空白  → 保留
/// a@b.com   的 a → 後面是標點              → 保留
/// su3cl3    切出的 s → 前後都是字母        → 丟棄
/// ```
fn single_letter_ok(keys: &str, chars: &[char], start: usize, end: usize) -> bool {
    // 整串就這麼短的話沒得挑，留著
    if chars.len() <= 1 || end != start + 1 {
        return true;
    }
    let left = start == 0 || chars[start - 1] == ' ' || punct::is_punct(keys, start - 1);
    let right = end >= chars.len() || chars[end] == ' ' || punct::is_punct(keys, end);
    left && right
}

/// 規則二：只有英文吃得下的段，必須是英文詞典裡的詞。
///
/// # 「只有英文吃得下」是關鍵
///
/// 注音或日文認領得了的段不受此限——`sushi` 是合法日文（すし），
/// 就算它剛好也是英文詞，也不必查詞典。這條只管**瀑布掉到最後一站**
/// 的那些段，也就是三個引擎裡只有英文肯收的。
///
/// # 最後一段例外
///
/// 使用者還在打字，最後一段本來就是半成品。打到 `chec` 的時候
/// 它還不是詞，但下一鍵就是了——這時候判死，逐字打根本走不下去。
///
/// ```text
/// dennwaconfig  的 config → 是詞           → 保留
///               的 onfi   → 不是詞、非結尾 → 丟棄
/// chec（還在打）→ 不是詞，但是最後一段     → 保留
/// ```
///
/// 詞庫沒載入時一律放行——切點引擎要能在沒有詞庫的情況下運作，
/// 只是少了這層收斂。
fn english_word_ok(chars: &[char], start: usize, end: usize, seg: &str) -> bool {
    if !crate::english::is_loaded() {
        return true;
    }
    // 注音或日文認領得了 → 不歸這條管
    if bopomofo::validity(seg) == bopomofo::Validity::Valid
        || romaji::validity(seg) == romaji::Validity::Valid
    {
        return true;
    }
    // 邊界上的短縮寫（yt、fb、ytb…）詞典收不到，但使用者天天打
    if boundary_acronym(chars, start, end, seg) {
        return true;
    }
    // 最後一段還在打，允許是半成品
    if end >= chars.len() {
        return true;
    }
    crate::english::is_word(seg.trim())
}

/// 縮寫放行的長度上限。
///
/// 3 是實測選的：2、3、4 對三支計分器**一個數字都沒動**，差別只在
/// 候選數中位（36 → 38／39／43）。常見縮寫幾乎都在三個字母以內
/// （yt、fb、ig、tw、ytb、nba），4 多付 7 個候選卻只多收 asap 那類，
/// 所以停在 3。
const ACRONYM_MAX: usize = 3;

/// 規則二的例外：**邊界上的短縮寫**。
///
/// # 為什麼需要這個例外
///
/// 規則二要求「只有英文吃得下的段必須是詞典裡的詞」，但縮寫按定義
/// 就不在詞典裡。`yt`、`fb` 不在 `en_50k`（`ig`、`gg`、`tv`、`pc` 反而
/// 在——收不收純屬語料的偶然），於是：
///
/// ```text
/// ytru.4u.3jp4wu6xk7（yt 舊有問題了）
///   → yt 不是詞、又不是最後一段 → 這個切法在生成階段就被丟掉
///   → 切法選單裡**完全沒有**「yt ＋ 中文」這個選項，Tab 也救不回來
/// ```
///
/// 那違反 Phase 2 的硬指標「切點涵蓋 100%，正解一定要在候選裡」，
/// 也違反本模組自己的原則「寧可留錯、不可殺對」。
///
/// # 判準
///
/// 這其實是**規則一的推廣**——規則一就是「邊界上的短段放行」，只是
/// 上限訂在 1（`a`、`I`）。這裡把上限拉到 `ACRONYM_MAX`，並多要求
/// 整段都是字母（縮寫不含數字，這一條擋掉 `su3` 那種注音殘渣）。
///
/// **左邊界的判準是「左邊那個字元不是英文字母」**，意思就是「我沒有
/// 把一個英文字砍成兩半」。
///
/// 第一版寫成「開頭／空白／標點」，**漏掉了最常見的情況**——縮寫夾在
/// 中文句子中間：
///
/// ```text
/// ytru.4u.3jp4wu6      yt 在開頭        → 過
/// 5k4ek7 ytu.3jp4wu6   yt 在空白後      → 過
/// 5k4ek7ytu.3jp4wu6    yt 接在「這個」後 → 漏掉了
/// ```
///
/// 注音音節一定以聲調鍵結尾（一聲是空白，其餘是 `3`／`4`／`6`／`7`），
/// 所以「中文後面」看到的是數字，不是空白也不是標點。改成「不是字母」
/// 之後三種情況一起涵蓋，而 `configg6ru0` 的 `onf`（左邊是 `c`）仍然
/// 擋得住——那正是這條界線要擋的東西。
///
/// **只看左邊界，不看右邊界**——跟規則一不同。累加式切法是「分支
/// 一旦死掉就不會復活」，`yt` 這條在打到第 3 鍵（`ytr`）時就得活下來，
/// 那時右邊只有一個還在打的字元，看不出任何東西。同理，「看下一段是
/// 什麼語言」那類做法在時序上根本來不及，不要再試。
///
/// # 順帶救到的
///
/// 短前綴 ＋ 已知詞會被 `merge` 黏回去，所以詞典沒收的長詞也跟著通了：
/// `vtuber` = `vt`（這條放行）＋ `uber`（本來就是詞）。
///
/// # 代價
///
/// 對**沒被解決**的案例，第一名會變醜——`vtuberru.4` 原本第一名是
/// 乾淨的英文原樣，現在是「vt宇部っる噢」，正解掉到第 3。**打得出來
/// 了，但要按 Tab。** 這是排序層的事，不是這裡的事。
fn boundary_acronym(chars: &[char], start: usize, end: usize, seg: &str) -> bool {
    end - start <= ACRONYM_MAX
        && (start == 0 || !chars[start - 1].is_ascii_alphabetic())
        && seg.chars().all(|c| c.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keep_seg(keys: &str, start: usize, end: usize) -> bool {
        let chars: Vec<char> = keys.chars().collect();
        keep(keys, &chars, start, end)
    }

    fn load() -> bool {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data");
        !crate::english::load(&data).is_empty()
    }

    #[test]
    fn 單字母_邊界上的保留() {
        // "a banana" 的 a：開頭 + 後面是空白
        assert!(keep_seg("a banana", 0, 1), "a␣banana 的 a 是一個詞");
        // "I am" 的 I
        assert!(keep_seg("I am", 0, 1), "I␣am 的 I 是一個詞");
    }

    #[test]
    fn 單字母_標點旁邊的保留() {
        // a@b.com 的 a（後面是 @）與 b（前後都是標點）
        assert!(keep_seg("a@b.com", 0, 1), "a 後面是標點");
        assert!(keep_seg("a@b.com", 2, 3), "b 前後都是標點");
    }

    #[test]
    fn 單字母_詞中間的丟棄() {
        // su3cl3（你好）被切成 s|u|3|... 的殘渣
        assert!(!keep_seg("su3cl3", 0, 1), "s 前後都是字母，是殘渣");
        assert!(!keep_seg("su3cl3", 1, 2), "u 同理");
    }

    #[test]
    fn 單字母_整串就一個字元時保留() {
        assert!(keep_seg("a", 0, 1), "整串就這麼短，沒得挑");
    }

    #[test]
    fn 英文段_是詞的保留() {
        if !load() {
            eprintln!("詞庫未下載，跳過（跑 data/download.ps1）");
            return;
        }
        // dennwaconfig 的 config（位置 6..12，是結尾）
        assert!(keep_seg("dennwaconfig", 6, 12), "config 是詞");
        // 非結尾的情況：configg6ru0 的 config
        assert!(keep_seg("configg6ru0", 0, 6), "config 是詞");
    }

    #[test]
    fn 英文段_不是詞的丟棄() {
        if !load() {
            return;
        }
        // configg6ru0 裡切出 onfi（非結尾、不是詞、三引擎只有英文收）
        assert!(!keep_seg("configg6ru0", 1, 5), "onfi 不是詞，該丟");
    }

    #[test]
    fn 縮寫_邊界上的短英文段保留() {
        if !load() {
            return;
        }
        // 使用者實際打不出來的那句：yt 舊有問題了
        assert!(
            keep_seg("ytru.4u.3jp4wu6xk7", 0, 2),
            "yt 是開頭的短縮寫，詞典沒收也要留"
        );
        // 空白後面同理
        assert!(keep_seg("check fbru.4", 6, 8), "空白後面也是邊界");
    }

    #[test]
    fn 縮寫_超過上限的丟棄() {
        if !load() {
            return;
        }
        // vtub 四個字母，超過 ACRONYM_MAX
        assert!(!keep_seg("vtuberru.4", 0, 4), "vtub 太長，不是縮寫");
    }

    /// **縮寫夾在中文句子中間**——最常見的情況，第一版漏掉了。
    ///
    /// 注音音節以聲調鍵結尾（`ek7` 的 `7`），不是空白也不是標點，
    /// 所以舊的「開頭／空白／標點」判準認不出這是邊界。
    #[test]
    fn 縮寫_接在中文後面也放行() {
        if !load() {
            return;
        }
        // 這個yt有問題：yt 在 6..8，左邊是「個」的輕聲鍵 7
        assert!(
            keep_seg("5k4ek7ytu.3jp4wu6", 6, 8),
            "yt 接在注音音節後面也是邊界"
        );
        // vtuber 靠 vt（縮寫）＋ uber（詞）＋ merge 黏回去
        assert!(
            keep_seg("s84ek7vtuberyji6", 6, 8),
            "vt 接在注音音節後面也是邊界"
        );
    }

    #[test]
    fn 縮寫_只放行左邊界上的() {
        if !load() {
            return;
        }
        // configg6ru0 裡的 onf：長度合格、也全是字母，但左邊是 c，
        // 是切碎的殘渣不是縮寫
        assert!(!keep_seg("configg6ru0", 1, 4), "onf 前面是字母，不是縮寫");
    }

    #[test]
    fn 縮寫_含數字的不算縮寫() {
        // 直接問規則本身——注音殘渣帶聲調數字，長度雖然合格也不放行。
        // 走 keep_seg 的話會先被注音合法性接走（su3 是ㄋㄧˇ），測不到這條。
        let chars: Vec<char> = "su3cl3".chars().collect();
        assert!(
            !boundary_acronym(&chars, 0, 3, "su3"),
            "su3 含數字，不是縮寫"
        );
    }

    #[test]
    fn 英文段_最後一段是半成品也保留() {
        if !load() {
            return;
        }
        // 打到 chec 的中間狀態
        assert!(keep_seg("chec", 0, 4), "還在打，不能判死");
    }

    #[test]
    fn 日文認領的段不受英文詞規則管() {
        if !load() {
            return;
        }
        // dennwaconfig 的 dennwa（でんわ）不是英文詞，但日文吃得下
        assert!(keep_seg("dennwaconfig", 0, 6), "dennwa 是合法日文");
    }

    #[test]
    fn 標點與分隔符不受管() {
        assert!(keep_seg("hello,world", 5, 6), "逗號自成一段");
        assert!(keep_seg("a banana", 1, 2), "分隔符空白");
    }
}
