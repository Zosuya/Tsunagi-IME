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
/// `keys` 是整串按鍵，`i` 是要判斷的位置。手上已經有逐字元切好的
/// `&[char]` 就改叫 `is_punct_at`，省掉這裡的整串配置。
pub fn is_punct(keys: &str, i: usize) -> bool {
    let chars: Vec<char> = keys.chars().collect();
    is_punct_at(&chars, i)
}

/// `is_punct` 的切片版：**不配置整串**。
///
/// 切點引擎在熱路徑上（`to_segments` 每條切法的每個單字元段、`prune::keep`
/// 每個新範圍）問這件事，而按鍵串可以長到兩百鍵——每問一次就把整串
/// 收成一個 `Vec<char>`，光這個配置就佔掉判斷本身的大半成本。那兩處
/// 手上本來就有對齊好的 `chars`，直接給切片。
pub fn is_punct_at(chars: &[char], i: usize) -> bool {
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
            if bopomofo::syllable::check(&seg) == bopomofo::Validity::Valid
                && !EMPTY_SYLLABLES.contains(&seg.as_str())
            {
                return false;
            }
        }
    }

    true
}

/// **軟標點**：`is_punct` 判成注音、但其實也可能是標點的位置。
///
/// 一鍵兩用的 `,` `.` `;` `/` 後面接空白時，`is_punct` 只要找得到任何
/// 一個含它的合法音節就判成注音，於是「標點＋分隔符」這個合法解讀
/// 根本不在候選裡：
///
/// ```text
/// hello.␣world   → 只切得出 英:hello | 注:.␣ | 英:world     （hello歐world）
/// fine.␣thank    → 英:fin | 注:e.␣ | 英:thank               （fin溝thank，e.␣＝ㄍㄡ）
/// review,␣config → 英:review | 注:,␣ | 英:config           （reviewㄝconfig，w,␣＝ㄊㄝ）
/// ```
///
/// 軟標點讓**兩種解讀都生得出來**，交給排序決定——它不改 `is_punct`
/// 的判斷（凍結點、單字母規則等「一定是標點」的用途不受影響），只在
/// 「這個字元自成一段」時允許它當標點。`vu.␣e93`（修改）照樣是第一名，
/// 只是多了一個「vu. 改」的候選。哪些形狀該判成標點由排序決定，見
/// `rank::soft_punct_swallowed`（hello. world 是句點、trip歐洲 不是）。
///
/// 前一個字元是空白的不算（`␣.␣` 那種不是句尾標點）。
pub fn is_soft_punct(keys: &str, i: usize) -> bool {
    let chars: Vec<char> = keys.chars().collect();
    soft_punct_shape(&chars, i) && !is_punct_at(&chars, i)
}

/// 軟標點的**字面形狀**：`,` `.` `;` `/` 夾在「非空白」與「空白」之間。
///
/// 只看左右各一個字元，不試音節——**便宜到可以先問**。`is_soft_punct`
/// 是「形狀對＋`is_punct` 判成注音」，而切點引擎要的永遠是「標點或軟
/// 標點」（見 `is_mark_at`），這時形狀對就夠了，不必再試音節。
fn soft_punct_shape(chars: &[char], i: usize) -> bool {
    i > 0
        && matches!(chars.get(i), Some(',' | '.' | ';' | '/'))
        && chars.get(i + 1) == Some(&' ')
        && chars[i - 1] != ' '
}

/// 這個字元**自成一段時**算不算標點：`is_punct` 或 `is_soft_punct`。
///
/// 切點引擎兩處要問的都是這個聯集——`prune::keep` 放不放行單字元段、
/// `to_segments` 標不標 `is_mark`。
///
/// # 為什麼不照字面寫成 `is_punct || is_soft_punct`
///
/// 軟標點生成剛上的時候就是照字面寫的，每個單字元段都要：
///
/// 1. `is_punct` 把整串收成 `Vec<char>`、試最多 15 種音節（每種配置一個字串）
/// 2. 判成注音的話，`is_soft_punct` **再收一次整串、再跑一次 `is_punct`**
///
/// `to_segments` 每條切法的每個單字元段都問（每鍵上萬次）。可是
/// `P || (形狀 && !P)` 就是 `形狀 || P`——**形狀對就已經是答案**，音節
/// 根本不必試；形狀不對才問 `is_punct`，而那正是軟標點上線前本來就要
/// 付的成本。改成這樣（連同不再收整串）之後，整份測資逐鍵走
/// `Session::push` 合計少 0.66 秒（疊加版比 master 多出的 1.5 秒裡的
/// 四成），long 節 p99 10.9→10.4ms。
///
/// 疊加驗證（2026-09-23）的效能歸因說「段選單定案新增成本的七成在軟
/// 標點」，那是把軟標點**整個關掉**量的——連它多生的切法一起拿掉了。
/// 這裡只省判斷本身，定案量不出差別：定案多出來的是排序（候選多 6%、
/// 每個候選的計分多 9%），不在這裡。
pub fn is_mark_at(chars: &[char], i: usize) -> bool {
    soft_punct_shape(chars, i) || is_punct_at(chars, i)
}

/// **合法但沒有任何字在用的音節**——遇到這些不要讓標點讓位。
///
/// # 為什麼需要這張表
///
/// `su3cl3,`（你好，）的逗號判得出是標點，但**後面多一個空白就不行**
/// ——`,␣` 剛好是「ㄝ一聲」這個合法音節，於是「能參與完整音節」那條
/// 規則成立，逗號被吞進注音段。而「你好，」後面接空白是極常見的打法。
///
/// 掃過五個一鍵兩用的鍵（`,` `.` `;` `/` `-`）× 五個聲調的所有合法
/// 音節，只有 `,␣` 的候選清單裡**沒有任何真正的字**——唯一的候選是
/// 注音符號 `ㄝ` 本身（那是 §2.18「注音符號直出」放進去的）。
///
/// 其餘的都有字：`.␣` 是ㄡ（歐、鷗）、`;␣` 是ㄤ（骯、腌）、
/// `/␣` 是ㄥ（鞥）、`-␣` 是ㄦ（兒），都不能收進來。
///
/// # 為什麼寫成常數而不是查詞典
///
/// `punct` 是切點引擎的底層，目前只依賴 `bopomofo`，**不碰詞典**
/// ——那是刻意的分層（詞典是背景載入的，載完之前判斷會不一致）。
/// 這張表只有一筆，寫死比引進依賴划算。
const EMPTY_SYLLABLES: &[&str] = &[", "];

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

    /// **後面接空白也要判得出是標點**。
    ///
    /// `su3cl3,` 判得出來，但多一個空白就不行——`,␣` 剛好是「ㄝ一聲」
    /// 這個合法音節，「能參與完整音節」那條規則就成立了。而 ㄝ 一聲
    /// 沒有任何字在用（唯一候選是注音符號本身），不該讓標點讓位。
    #[test]
    fn 逗號後面接空白仍是標點() {
        assert!(punct_at("su3cl3, ", 6), "你好，␣");
        assert!(punct_at("su3cl3, hi", 6), "你好，␣hi");
        // 其餘四個一鍵兩用的鍵**有字在用**，不可以一起放行
        assert!(!punct_at(".  ", 0), ".␣ 是ㄡ（歐）");
        assert!(!punct_at(";  ", 0), ";␣ 是ㄤ（骯）");
        assert!(!punct_at("-  ", 0), "-␣ 是ㄦ（兒）");
    }

    #[test]
    fn 軟標點_後接空白的一鍵兩用標點() {
        // `hello.␣` 的 `.` 可以是ㄡ（歐）也可以是句點
        assert!(is_soft_punct("hello. world", 5));
        assert!(is_soft_punct("su3cl3. ", 6), "你好。／你好歐 都合法");
        // `.` 跟左邊組得成音節的也算（fine. 的 e.␣＝ㄍㄡ、review, 的 w,␣＝ㄊㄝ）
        assert!(is_soft_punct("fine. ", 4));
        assert!(is_soft_punct("review, ", 6));
        // 後面不是空白 → 不是這種歧義
        assert!(!is_soft_punct("hello.3", 5));
        // 本來就是標點的不必再問（`,␣` 是空音節，`is_punct` 直接判標點）
        assert!(!is_soft_punct("hello, ", 5));
        // 前一個字元是空白的不算
        assert!(!is_soft_punct("a . b", 2));
    }

    /// **`is_mark_at` 是效能改寫，答案必須跟字面的聯集一模一樣**。
    ///
    /// 它先看軟標點的形狀、形狀對就不試音節（理由見那支的說明）。這條
    /// 守的是那個代數：`is_punct || is_soft_punct` 等於 `形狀 || is_punct`。
    /// 哪天有人改了形狀的條件，切點引擎會靜靜地多生或少生「句點＋分隔符」
    /// 那種切法——只有漏斗看得到。
    ///
    /// 對照組**照抄快路上線前的寫法**（每次收整串、先問 `is_punct` 再問
    /// 軟標點），不共用 `soft_punct_shape`——共用的話兩邊一起改錯也照樣過。
    /// 按鍵串取整份測資的第三欄（不自己編），每一個位置都比一次。
    #[test]
    fn 標點或軟標點_快路跟字面的聯集一致() {
        fn literal(keys: &str, i: usize) -> bool {
            let chars: Vec<char> = keys.chars().collect();
            let soft = chars
                .get(i)
                .is_some_and(|&c| matches!(c, ',' | '.' | ';' | '/'))
                && chars.get(i + 1) == Some(&' ')
                && i > 0
                && chars[i - 1] != ' '
                && !is_punct(keys, i);
            is_punct(keys, i) || soft
        }
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/測資.txt");
        let text = std::fs::read_to_string(&path).expect("讀得到測資");
        let mut keys: Vec<&str> = text
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| l.trim_end_matches('\r').split('\t').nth(2))
            .collect();
        assert!(keys.len() > 1000, "測資讀不到幾列，路徑或格式變了？");
        // 形狀的邊界：開頭、結尾、前面是空白、連續標點
        keys.extend([". ", "a.", "a. ", " . ", "a.. ", "hello, ", "-  "]);
        let mut shape_hits = 0;
        for k in keys {
            let chars: Vec<char> = k.chars().collect();
            for i in 0..=chars.len() {
                let want = literal(k, i);
                assert_eq!(is_mark_at(&chars, i), want, "{k:?} 第 {i} 個字元");
                assert_eq!(
                    is_soft_punct(k, i),
                    want && !is_punct(k, i),
                    "{k:?} 第 {i} 個字元"
                );
                shape_hits += usize::from(soft_punct_shape(&chars, i));
            }
        }
        assert!(
            shape_hits > 0,
            "測資裡應該有 `hello.␣` 那種形狀，不然這條等於沒測"
        );
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
