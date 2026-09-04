//! 日文羅馬字的 mora 合法性驗證。
//!
//! 依據 `字元規則/通用日文合法規則.canvas` 與 `通用日文例外規則.canvas`，
//! 以及 `字元規則/日文按鍵合法表.md` 的表格。
//!
//! # 什麼是一個 mora
//!
//! **一個輸入單位**，不是一個假名字元——`kya` 是一個單位但輸出 きゃ
//! 兩個字元。長度只有 1、2、3 三種（`日文輸入單位一覽.md` 統計過，
//! 175 條 mora 沒有 4 字母的）。
//!
//! # offset 機制
//!
//! canvas 用 offset 處理促音：促音成立時 offset=1，後面的「區1／區2」
//! 整體往後移一格。這讓促音不必複製整棵子樹——`kka` 的 `ka` 部分走的
//! 是跟 `ka` 完全相同的路徑，只是起點往後推。

use super::keymap as km;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Validity {
    Valid,
    Invalid,
}

/// 驗證一個 mora（含可能的促音前綴）。
///
/// # canvas 的直譯
///
/// 第一字元依序問：長音 → 母音 → 清音 → 濁音 → 半濁音 → 外來語 → 小寫打法。
///
/// - **長音** `-`：在開頭一律非法（前面沒東西可延長）
/// - **母音**：單獨就是一個 mora（あいうえお）
/// - **清音／濁音／半濁音／外來語**：進第二字元的判斷，先看促音
/// - **小寫打法** `x`/`l`：後面只能接 母音／`y`／`t`／`w`／`k`
///
/// 促音的判斷在第二字元：「與第一字元相同嗎」。相同且不是 `n` 時
/// offset=1（促音成立，真正的內容從第三字元開始）；`nn` 是撥音不是促音。
pub fn check(keys: &str) -> Validity {
    let c: Vec<char> = keys.chars().collect();
    if c.is_empty() {
        return Validity::Invalid;
    }

    // ── 第一字元 ──
    if km::is_chouon(c[0]) {
        // 長音不能在開頭
        return Validity::Invalid;
    }
    if km::is_vowel(c[0]) {
        // 母音單獨成 mora；後面不能再有東西
        return if c.len() == 1 {
            Validity::Valid
        } else {
            Validity::Invalid
        };
    }
    if km::is_kogaki(c[0]) {
        return check_kogaki(&c);
    }
    if !km::is_consonant(c[0]) {
        return Validity::Invalid;
    }

    // ── 第二字元：先看促音 ──
    let Some(&second) = c.get(1) else {
        return Validity::Invalid;
    };
    let offset = if second == c[0] {
        if c[0] == 'n' {
            // `nn` 是撥音的明確寫法（ん），不是促音——撥音優先。
            return if c.len() == 2 {
                Validity::Valid
            } else {
                Validity::Invalid
            };
        }
        // 促音成立：真正的內容從第三字元開始
        1
    } else {
        0
    };

    check_body(&c, 1 + offset)
}

/// 小寫打法：`x`/`l` 後面能接什麼。
///
/// 這四種對應日文僅有的小寫假名：
/// - 母音 → ぁぃぅぇぉ
/// - `y` + `a/u/o` → ゃゅょ
/// - `t` + `u` → っ
/// - `w` + `a` → ゎ
/// - `k` + `a/e` → ゕゖ
fn check_kogaki(c: &[char]) -> Validity {
    let Some(&second) = c.get(1) else {
        return Validity::Invalid;
    };
    if km::is_vowel(second) {
        return ok_if(c.len() == 2);
    }
    let Some(&third) = c.get(2) else {
        return Validity::Invalid;
    };
    if c.len() != 3 {
        return Validity::Invalid;
    }
    match second {
        'y' => ok_if("auo".contains(third)),
        't' => ok_if(third == 'u'),
        'w' => ok_if(third == 'a'),
        'k' => ok_if("ae".contains(third)),
        _ => Validity::Invalid,
    }
}

/// 促音剝掉之後的本體：從 `i` 這個位置開始。
///
/// canvas 的「區1／區2」對應這裡的 `c[i]` 與 `c[i+1]`。
fn check_body(c: &[char], i: usize) -> Validity {
    let first = c[0];
    let Some(&x) = c.get(i) else {
        return Validity::Invalid;
    };

    // 母音 → mora 完成
    if km::is_vowel(x) {
        return ok_if(c.len() == i + 1);
    }

    // 撥音：單獨的 n 或 nn
    // （`n` 當第一字元、第二字元不是母音時，它就是撥音）
    if first == 'n' && i == 1 {
        // `n` + 非母音 → 撥音，但後面不能再有東西
        // （`nn` 已在 check() 處理）
        return Validity::Invalid;
    }

    // 拗音 `y`：第一字元必須是子音／濁音／半濁音／小寫，且不能是 w
    if x == 'y' {
        if first == 'w' {
            return Validity::Invalid;
        }
        let Some(&v) = c.get(i + 1) else {
            return Validity::Invalid;
        };
        return ok_if(km::is_vowel(v) && c.len() == i + 2);
    }

    // 特 sh/ch/th/dh/who：`h` 在第二位
    if x == 'h' {
        let Some(&v) = c.get(i + 1) else {
            return Validity::Invalid;
        };
        if "sctd".contains(first) {
            return ok_if(km::is_vowel(v) && c.len() == i + 2);
        }
        if first == 'w' {
            // `who` = うぉ，只有 o
            return ok_if(v == 'o' && c.len() == i + 2);
        }
        return Validity::Invalid;
    }

    // 特 tsu：`s` 在第二位，第一字元必須是 `t`
    if x == 's' {
        if first != 't' {
            return Validity::Invalid;
        }
        let Some(&v) = c.get(i + 1) else {
            return Validity::Invalid;
        };
        return ok_if(km::is_vowel(v) && c.len() == i + 2);
    }

    // 特 w：`w` 在第二位，第一字元必須是 `t` 或 `d`
    if x == 'w' {
        if !"td".contains(first) {
            return Validity::Invalid;
        }
        let Some(&v) = c.get(i + 1) else {
            return Validity::Invalid;
        };
        return ok_if("auo".contains(v) && c.len() == i + 2);
    }

    Validity::Invalid
}

fn ok_if(b: bool) -> Validity {
    if b {
        Validity::Valid
    } else {
        Validity::Invalid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(k: &str) -> bool {
        check(k) == Validity::Valid
    }

    #[test]
    fn 母音單獨成_mora() {
        for k in ["a", "i", "u", "e", "o"] {
            assert!(v(k), "{k:?}");
        }
    }

    #[test]
    fn 子音加母音() {
        for k in ["ka", "shi", "tsu", "na", "hi", "mo", "yu", "ra", "wa"] {
            assert!(v(k), "{k:?}");
        }
    }

    #[test]
    fn 濁音半濁音外來語() {
        for k in ["ga", "za", "da", "ba", "ja", "pa", "va"] {
            assert!(v(k), "{k:?}");
        }
    }

    #[test]
    fn 拗音() {
        for k in [
            "kya", "kyu", "kyo", "sha", "cha", "gya", "bya", "pya", "jya",
        ] {
            assert!(v(k), "{k:?}");
        }
    }

    #[test]
    fn 特殊拼法() {
        // 特 sh/ch
        for k in ["sha", "shi", "shu", "she", "sho", "cha", "chi", "cho"] {
            assert!(v(k), "{k:?}");
        }
        // 特 th/dh（外來語音 てぃ / でぃ）
        for k in ["thi", "tha", "dhi", "dha"] {
            assert!(v(k), "{k:?}");
        }
        // 特 who
        assert!(v("who"), "うぉ");
        // 特 tsu
        for k in ["tsu", "tsa", "tsi", "tse", "tso"] {
            assert!(v(k), "{k:?}");
        }
        // 特 w
        for k in ["twu", "dwu"] {
            assert!(v(k), "{k:?}");
        }
    }

    #[test]
    fn 撥音用_nn() {
        assert!(v("nn"), "ん");
        // 單獨一個 n 不合法——依「沒打完 = 非法」的規格
        assert!(!v("n"), "單獨的 n 還沒打完");
    }

    #[test]
    fn 促音是子音重複() {
        for k in ["kka", "tta", "ssa", "ppa", "ggo", "ssha", "ttsu"] {
            assert!(v(k), "{k:?}");
        }
    }

    #[test]
    fn 母音重複不是促音() {
        // `ii` 是い＋い兩個 mora，不是促音——這裡只驗一個 mora，所以非法。
        assert!(!v("ii"));
        assert!(!v("aa"));
    }

    #[test]
    fn 小寫打法() {
        for k in ["xa", "xi", "la", "lo"] {
            assert!(v(k), "{k:?} 小寫母音");
        }
        for k in ["xya", "xyu", "xyo", "lya"] {
            assert!(v(k), "{k:?} 小寫拗音");
        }
        assert!(v("xtu"), "っ");
        assert!(v("ltu"), "っ");
        assert!(v("xwa"), "ゎ");
        assert!(v("xka"), "ゕ");
        assert!(v("xke"), "ゖ");
    }

    #[test]
    fn 長音不能在開頭() {
        assert!(!v("-"));
        assert!(!v("-a"));
    }

    #[test]
    fn 不合法的子音串() {
        for k in [
            "kh", "mh", "nh", "ph", "bh", "gh", "kw", "sw", "nw", "kt", "st", "nt",
        ] {
            assert!(!v(k), "{k:?} 不該合法");
        }
    }

    #[test]
    fn q_一律非法() {
        for k in ["q", "qa", "qu"] {
            assert!(!v(k), "{k:?}");
        }
    }

    #[test]
    fn 空字串非法() {
        assert!(!v(""));
    }

    #[test]
    fn 拗音的第三位必須是母音() {
        assert!(!v("kyk"), "kyk 第三位不是母音");
        assert!(!v("ky"), "ky 還沒打完");
    }

    #[test]
    fn w_不接拗音() {
        // わ 沒有拗音形式
        assert!(!v("wya"), "わ 不接拗音");
    }
}
