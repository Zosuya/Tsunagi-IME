//! 使用者可寫入的資料檔裡，「會被送進文件的文字」的守門員。
//!
//! 領域包與學習檔的輸出欄位原本是原樣進索引、原樣送出。包會流通，
//! 一個惡意包可以把常見讀音對到含雙向覆寫（U+202E）或零寬字元的字串
//! ——使用者看到的跟實際送進網址列、終端機的不一樣。這裡只擋**看不見
//! 卻會改變語意**的那批字元；同形字（西里爾 а 冒充拉丁 a）看得見，
//! 不在這裡處理。

/// 這段文字可以安心送進文件嗎？
///
/// 擋的是三類：控制字元（含換行，一條詞不該換行）、Unicode 雙向控制字元、
/// 零寬與其他不可見的格式字元。空字串算安全，由呼叫端另行決定要不要收。
pub fn is_safe_output(s: &str) -> bool {
    s.chars().all(|c| !is_invisible_or_control(c))
}

fn is_invisible_or_control(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061C}'  // 阿拉伯字母記號（雙向）
            | '\u{180E}' // 蒙古文母音分隔符
            | '\u{200B}'..='\u{200F}' // 零寬空白、零寬連接／不連接、LRM、RLM
            | '\u{2028}' | '\u{2029}' // 行／段分隔符
            | '\u{202A}'..='\u{202E}' // 雙向嵌入／覆寫
            | '\u{2060}'..='\u{2064}' // 詞連接符等不可見運算子
            | '\u{2066}'..='\u{2069}' // 雙向隔離
            | '\u{FEFF}' // BOM／零寬不換行空白
            | '\u{FFF9}'..='\u{FFFB}' // 行間註解錨
            | '\u{E0000}'..='\u{E007F}' // 標籤字元
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 一般文字都過() {
        for s in ["github.com", "ご飯", "你好", "Ünïcödé", "😀", ""] {
            assert!(is_safe_output(s), "{s:?}");
        }
    }

    #[test]
    fn 雙向覆寫擋下() {
        assert!(!is_safe_output("moc.buhtig\u{202E}"));
        assert!(!is_safe_output("a\u{2066}b"));
    }

    #[test]
    fn 零寬與控制字元擋下() {
        assert!(!is_safe_output("git\u{200B}hub"));
        assert!(!is_safe_output("a\u{FEFF}b"));
        assert!(!is_safe_output("a\nb"));
        assert!(!is_safe_output("a\u{7}b"));
        assert!(!is_safe_output("a\u{E0041}"));
    }
}
