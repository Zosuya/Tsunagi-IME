//! 符號輸入：`\名字\` 叫出一組符號。
//!
//! # 為什麼要獨立的命名空間
//!
//! ★♥→※ 這類符號**偶爾要用一次**，性質跟標點不同——標點是打字時真的
//! 會用到的（見 `width::variants`），符號則是「我知道有這個東西，但不
//! 知道怎麼打」。
//!
//! 把它們混進同音字清單會有兩個問題：正常打字時多出一堆干擾，而且
//! `star` 這個英文詞會再也打不出原文。所以**只有 `\` 開頭進得去**。
//!
//! # 為什麼用 `\名字\` 而不是 `\名字`
//!
//! 收尾的 `\` 就是名字的邊界，所以多字名字（`\星星\`）不必再決定
//! 「名字到哪裡結束」，夾在句子中間（`你好\星\好`）也完全清楚。
//! 慣例上跟 Discord／Slack 的 `:star:` 是同一件事。
//!
//! # 名字有兩條路：組出來的文字、按鍵原文
//!
//! 這樣三種語言共用同一份表——包裡一列就把三種名字列齊，指向同一組
//! 符號：
//!
//! ```text
//! sym  星,hoshi,star  ★☆✦✧⭐      ← 欄位其實是 Tab 分隔
//!
//! \vu/␣\   注音組出「星」──→ 用**文字**查中
//! \hoshi\  日文按鍵 hoshi ─→ 用**按鍵**查中
//! \star\   英文兩條都一樣 ─→ 都查得中
//! ```
//!
//! **日文名字列羅馬字不列假名**：羅馬字要先過語言判斷、再過整句轉換，
//! 組出來的文字不可預期（`tougou` 變漢字「統合」、`sekibun` 前半被判成
//! 中文），只有按鍵原文百分之百對得上。查詢的順序與實測見
//! `compose::merge_symbols` 的說明。
//!
//! # 預設永遠是原樣
//!
//! `\star\` 的第一個候選是 `\star\` 自己——**不按方向鍵就不會變**。
//! 這讓「表裡剛好有你的資料夾名」不再是安全問題：`C:\star\file`
//! 打出來仍然是 `C:\star\file`。
//!
//! # 表是自己列的
//!
//! 新酷音的 `symbols.dat` 是 LGPL 資料（開發文件 §2.31 標過需隔離），
//! **不能直接抄**。「有哪些常用符號」是事實不是創作，這份表自己列，
//! 內容自然會跟它重疊。

/// 符號的前綴與結尾，前後各一個。
///
/// 平台層要知道這件事——`\` 本身沒有標點變體，不特別放行的話會被
/// 「開頭的符號直接送出」那條規則吃掉，**符號永遠起不了頭**。
pub const PREFIX: char = '\\';

/// 這個字元是符號的前綴嗎？
pub fn is_prefix(c: char) -> bool {
    c == PREFIX
}

/// 這個名字有哪些符號？沒有就回空的。
///
/// # 全部都在包裡
///
/// 這裡原本有一份 `BUILTIN` 常數（24 組），2026-09-05 抽出去變成隨程式
/// 一起裝的預載包 `packs/內建符號.txt`。好處是使用者打開就看得到有哪些、
/// 可以複製一份來改、更新符號表不必重新編譯 DLL。
///
/// 所以查詢只剩一條路：包。使用者自己的包同名時蓋過預載的那份，那是在
/// `pack::dirs()` 的優先序裡決定的，不在這裡。
pub fn lookup(name: &str) -> Vec<String> {
    if name.is_empty() {
        return Vec::new();
    }
    // 一個符號都沒有時直接短路，不必拿讀鎖
    if !crate::pack::any_sym() {
        return Vec::new();
    }
    // 已經是拆好的清單——「怎麼拆」在 `pack::parse` 就決定了。
    // 這裡原本是 `s.chars()`，2026-09-07 改的：emoji 拆不了字元。
    crate::pack::index()
        .sym
        .get(name)
        .cloned()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 預載包在專案根的 `packs/`。使用者目錄指向不存在的路徑，
    /// 測試才不會被本機 `%APPDATA%` 裡的包影響。
    fn load() -> bool {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        crate::pack::set_bundled_dir(Some(root.join("packs")));
        crate::pack::load(
            "__測試用_不存在的資料夾__",
            &[crate::pack::BUNDLED_SYMBOLS.to_string()],
        );
        crate::pack::any_sym()
    }

    /// 預載包的原始內容，給「表本身對不對」那幾條用。
    fn rows() -> Vec<(String, String)> {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("packs")
            .join(format!("{}.txt", crate::pack::BUNDLED_SYMBOLS));
        let Ok(text) = std::fs::read_to_string(p) else {
            return Vec::new();
        };
        text.lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .filter_map(|l| {
                let mut f = l.split('\t');
                match (f.next()?, f.next()?, f.next()?) {
                    // 名字欄可能列了好幾個別名，跟 `pack::parse` 一樣展開
                    ("sym", k, v) => Some(
                        k.split(',')
                            .map(|n| (n.trim().to_string(), v.to_string()))
                            .collect::<Vec<_>>(),
                    ),
                    _ => None,
                }
            })
            .flatten()
            .collect()
    }

    #[test]
    fn 查得到預載包的名字() {
        assert!(load(), "預載包載不進來");
        let s = lookup("星");
        assert_eq!(s.first().map(String::as_str), Some("★"));
        // 中／日／英三個名字指向同一組
        assert_eq!(lookup("star"), s);
        // 日文名字在表裡是羅馬字（`compose` 用按鍵原文查它）
        assert_eq!(lookup("hoshi"), s);
    }

    #[test]
    fn 查不到的回空的() {
        load();
        assert!(lookup("這個名字不存在").is_empty());
        assert!(lookup("").is_empty());
        // 路徑裡常見的資料夾名不該誤中
        assert!(lookup("Users").is_empty());
        assert!(lookup("Program Files").is_empty());
    }

    /// 表裡不能有重複的名字——後面那個進不了索引，是靜默的錯。
    #[test]
    fn 名字不重複() {
        let rows = rows();
        assert!(rows.len() >= 40, "預載包只讀到 {} 列", rows.len());
        let mut seen = std::collections::HashSet::new();
        for (k, _) in &rows {
            assert!(seen.insert(k.clone()), "重複的名字：{k}");
        }
    }

    /// 送進文件的字串要過 `sanitize` 那一關，跟其他擴充包同一道門。
    ///
    /// **這條擋的是真的踩過的坑**：`♥️` 帶了 emoji variation selector
    /// （U+FE0F，不可見），寫在包裡會讓整行被 `parse` 跳過——而它跟前面
    /// 的 `♥` 看起來一模一樣，肉眼看不出少了什麼。
    #[test]
    fn 預載包的符號都是安全的輸出() {
        for (k, v) in rows() {
            assert!(crate::sanitize::is_safe_output(&v), "{k} 的符號不安全");
            assert!(!v.is_empty(), "{k} 沒有符號");
        }
    }
}
