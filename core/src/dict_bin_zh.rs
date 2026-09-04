//! 注音詞庫的**二進位版面**。用 `dict_bin` 的索引，酬載形狀不同。
//!
//! # 為什麼注音也要做
//!
//! 日文那本改完之後量各詞庫的佔用，注音出乎意料地貴：
//!
//! | | 峰值 | 常駐 |
//! |---|---|---|
//! | 英文 | 9 MB | 8 MB |
//! | ＋注音 | 52 MB | **31 MB** |
//!
//! 常駐 23 MB、建表峰值再多 21 MB，而**資料本身只有 2.3 MB**。跟日文
//! 同一個病：13.2 萬個 `String` 鍵、13.2 萬個 `String` 值，各自一次堆
//! 配置加 24 位元組標頭。
//!
//! 峰值那 21 MB 是另一回事——`word_freq`（19 萬詞）、`char_freq`、
//! `reading_share` 這些是**建表才要的**。改成讀 `.bin` 之後執行期根本
//! 不會碰它們，峰值一起省掉。
//!
//! # 版面
//!
//! ```text
//! 檔頭      magic(8) ver(u16) 保留(u16)
//!           n_word(u32) n_char_key(u32) n_char(u32)
//!           詞索引位移/鍵長 字索引位移/鍵長 酬載位移(u32 × 6)
//! 詞索引    dict_bin 的索引：酬載範圍 = 詞在 blob 裡的位元組範圍
//! 字索引    同上：酬載範圍 = 這個讀音的同音字在字陣列裡的範圍
//! 字陣列    n_char × 9 bytes   位移(u32) 長度(u8) 分數(u32)
//! 文字 blob 詞與同音字的文字，全部貼著排
//! ```
//!
//! # 兩張表的酬載形狀為什麼不同
//!
//! 詞表是「一把鍵**幾個**詞」——同讀音的詞用 `SEP` 接在同一段裡，範圍
//! 直接當成文字 blob 的位元組區間，不必再存長度。**版面因此沒有改**，
//! 變的只是那一段的內容。
//!
//! 為什麼要能存多個：「城市」與「程式」讀音完全相同（ㄔㄥˊㄕˋ），
//! 舊的「一鍵一詞」讓分數低的那個在建表時就被丟掉——實測**丟掉了
//! 13,175 個詞**（8% 的按鍵串有兩個以上的詞，最擠的 `u4g4` 有 14 個）。
//! 詞層查不到「程式」，選字時選了「程」，「市」也就無從變成「式」。同音字表是「一個讀音好幾個字，各帶一個分數」——**分數不能
//! 丟**，單字學習要拿它跟 `k^N` 相乘，丟了就變成 libchewing 那條「選一
//! 次就跳第一」的曲線（那條在開發文件 §2.22 被否決過）。

use crate::dict_bin::{build_index, IndexRef};

const MAGIC: &[u8; 8] = b"TSNGZH01";
// 2：加了輕聲詞的本調別名（`build_zh_layout`）。**版面沒變，內容變了**
// ——舊的 `.bin` 讀得動但少了那 233 條，`ek4` 還是會出「這各」。
// `.bin` 是衍生檔又不進版控，別人 pull 下來手上是舊的，症狀又只是
// 「某些字打不出來」，極難聯想到要重跑 `gen_dict_zh`。版本加一之後
// 舊檔會被拒收、自動退回從文字重建，不必靠人記得。
// 3：同讀音的詞改成全部存下來（`SEP` 分隔）。版面沒變、內容變了，
// 舊檔讀得動但每個鍵只有一個詞，症狀是「選了第一個字，後面不跟著改」。
const VERSION: u16 = 3;

/// 同一個讀音的多個詞之間的分隔符。
///
/// 用 ASCII 的 Unit Separator——它不可能出現在中文詞裡，而且
/// `sanitize` 本來就擋控制字元，萬一漏出去也進不了文件。
pub const SEP: char = '\u{1f}';

/// 一個同音字在檔案裡佔幾個位元組：位移(4) 長度(1) 分數(4)
const CHAR_SIZE: usize = 9;

/// 檔頭：magic(8) ver(2) 保留(2) ＋ 三個數量(12) ＋ 六個位移／長度(24)
const HEADER: usize = 48;

/// 文字長度用 u8 存，超過就丟掉那一筆。理由同 `dict_bin::MAX_LEN`——
/// 寧可少一筆，也不要 `as u8` 靜默截斷讓後面的位移全部錯開。
const MAX_LEN: usize = 255;

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn get_u32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn get_u16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

/// 把注音的兩張表組成版面。
///
/// `words` 是「按鍵串 → 詞」，`chars` 是「按鍵串 → (字, 分數) 依序」。
/// 兩者**都不必先排序**，這裡會排——前綴共用要求鍵有序。
///
/// 同音字的順序原封不動保留：呼叫端已經依字頻排過、也套過偏好表了，
/// 這裡再排一次會把那些工作洗掉。
pub fn build(words: Vec<(String, String)>, chars: Vec<(String, Vec<(String, u32)>)>) -> Vec<u8> {
    let mut words: Vec<(String, String)> = words
        .into_iter()
        .filter(|(k, v)| k.len() <= MAX_LEN && v.len() <= MAX_LEN)
        .collect();
    words.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    let mut chars: Vec<(String, Vec<(String, u32)>)> = chars
        .into_iter()
        .filter(|(k, _)| k.len() <= MAX_LEN)
        .map(|(k, v)| {
            (
                k,
                v.into_iter().filter(|(t, _)| t.len() <= MAX_LEN).collect(),
            )
        })
        .collect();
    chars.sort_unstable_by(|a, b| a.0.cmp(&b.0));

    let mut text: Vec<u8> = Vec::new();

    // ── 詞表：酬載範圍直接是文字 blob 的位元組區間 ──
    let wkeys: Vec<&str> = words.iter().map(|(k, _)| k.as_str()).collect();
    let wcounts: Vec<u32> = words.iter().map(|(_, v)| v.len() as u32).collect();
    let widx = build_index(&wkeys, &wcounts);
    for (_, v) in &words {
        text.extend_from_slice(v.as_bytes());
    }

    // ── 同音字表：酬載範圍是「字陣列」的索引區間 ──
    let ckeys: Vec<&str> = chars.iter().map(|(k, _)| k.as_str()).collect();
    let ccounts: Vec<u32> = chars.iter().map(|(_, v)| v.len() as u32).collect();
    let cidx = build_index(&ckeys, &ccounts);
    let mut char_bytes: Vec<u8> = Vec::new();
    let mut n_char = 0u32;
    for (_, v) in &chars {
        for (t, score) in v {
            put_u32(&mut char_bytes, text.len() as u32);
            char_bytes.push(t.len() as u8);
            put_u32(&mut char_bytes, *score);
            text.extend_from_slice(t.as_bytes());
            n_char += 1;
        }
    }

    // ── 組檔 ──
    let off_widx = HEADER;
    let off_cidx = off_widx + widx.len();
    let off_chars = off_cidx + cidx.len();
    let off_text = off_chars + char_bytes.len();

    let mut out = Vec::with_capacity(off_text + text.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    put_u32(&mut out, words.len() as u32);
    put_u32(&mut out, chars.len() as u32);
    put_u32(&mut out, n_char);
    put_u32(&mut out, off_widx as u32);
    put_u32(&mut out, widx.keys.len() as u32);
    put_u32(&mut out, off_cidx as u32);
    put_u32(&mut out, cidx.keys.len() as u32);
    put_u32(&mut out, off_chars as u32);
    put_u32(&mut out, off_text as u32);
    debug_assert_eq!(out.len(), HEADER);
    widx.write(&mut out);
    cidx.write(&mut out);
    out.extend_from_slice(&char_bytes);
    out.extend_from_slice(&text);
    out
}

/// 查詢用的門面。**只借用那塊 bytes，自己不持有任何字串。**
pub struct ZhDict {
    bytes: &'static [u8],
    words: IndexRef,
    chars: IndexRef,
    off_chars: usize,
    off_text: usize,
}

impl ZhDict {
    /// 認檔頭。版面對不上就回 `None`——呼叫端會退回從文字重建。
    pub fn new(bytes: &'static [u8]) -> Option<Self> {
        if bytes.len() < HEADER || &bytes[..8] != MAGIC || get_u16(bytes, 8) != VERSION {
            return None;
        }
        let n_word = get_u32(bytes, 12) as usize;
        let n_ckey = get_u32(bytes, 16) as usize;
        let n_char = get_u32(bytes, 20) as usize;
        let off_widx = get_u32(bytes, 24) as usize;
        let wkeys_len = get_u32(bytes, 28) as usize;
        let off_cidx = get_u32(bytes, 32) as usize;
        let ckeys_len = get_u32(bytes, 36) as usize;
        let off_chars = get_u32(bytes, 40) as usize;
        let off_text = get_u32(bytes, 44) as usize;
        // 字陣列的長度必須跟宣告的數量對得上，否則就是壞檔
        if off_chars + n_char * CHAR_SIZE != off_text || off_text > bytes.len() {
            return None;
        }
        Some(ZhDict {
            bytes,
            words: IndexRef::new(bytes, off_widx, n_word, wkeys_len)?,
            chars: IndexRef::new(bytes, off_cidx, n_ckey, ckeys_len)?,
            off_chars,
            off_text,
        })
    }

    pub fn word_count(&self) -> usize {
        self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// 這串按鍵對應的多字詞。
    pub fn word(&self, keys: &str) -> Option<&'static str> {
        self.words_raw(keys)?.split(SEP).next()
    }

    /// 這串按鍵的**所有**同讀音詞，best 在前。
    ///
    /// 「城市」與「程式」讀音相同，選字要挑得到第二個才有意義。
    pub fn words(&self, keys: &str) -> impl Iterator<Item = &'static str> {
        self.words_raw(keys).into_iter().flat_map(|s| s.split(SEP))
    }

    /// 原始那一段（含分隔符）。
    fn words_raw(&self, keys: &str) -> Option<&'static str> {
        let i = self.words.find(keys)?;
        let (s, e) = self.words.range(i);
        std::str::from_utf8(&self.bytes[self.off_text + s..self.off_text + e]).ok()
    }

    /// 這串按鍵是詞表裡的詞嗎？
    pub fn has_word(&self, keys: &str) -> bool {
        self.words.find(keys).is_some()
    }

    /// 這個讀音有同音字嗎？
    ///
    /// **不要用 `chars(k).next().is_some()` 代替**——切點排序只想知道
    /// 「有沒有」，那是熱路徑，不該為一個布林值把第一個字解出來。
    pub fn has_chars(&self, keys: &str) -> bool {
        self.chars.find(keys).is_some()
    }

    /// 這個讀音的同音字，依原本的順序（字頻＋偏好表）。
    ///
    /// **分數要一起給**——單字學習拿它跟 `k^N` 相乘。
    pub fn chars(&self, keys: &str) -> impl Iterator<Item = (&'static str, u32)> + '_ {
        let range = self.chars.find(keys).map(|i| self.chars.range(i));
        let (s, e) = range.unwrap_or((0, 0));
        (s..e).map(move |k| {
            let at = self.off_chars + k * CHAR_SIZE;
            let off = get_u32(self.bytes, at) as usize;
            let len = self.bytes[at + 4] as usize;
            let score = get_u32(self.bytes, at + 5);
            let start = self.off_text + off;
            let t = std::str::from_utf8(&self.bytes[start..start + len]).unwrap_or_default();
            (t, score)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn built() -> &'static [u8] {
        let words = vec![
            ("su3cl3".to_string(), "你好".to_string()),
            ("su3c.4".to_string(), "你後".to_string()),
            ("wu3".to_string(), "體".to_string()),
        ];
        let chars = vec![
            (
                "su3".to_string(),
                vec![("你".to_string(), 900), ("擬".to_string(), 10)],
            ),
            ("cl3".to_string(), vec![("好".to_string(), 800)]),
        ];
        Box::leak(build(words, chars).into_boxed_slice())
    }

    #[test]
    fn 詞查得到() {
        let d = ZhDict::new(built()).unwrap();
        assert_eq!(d.word("su3cl3"), Some("你好"));
        assert_eq!(d.word("su3c.4"), Some("你後"));
        assert_eq!(d.word("wu3"), Some("體"));
        assert_eq!(d.word("nothing"), None);
    }

    #[test]
    fn 同音字的順序與分數原封不動() {
        let d = ZhDict::new(built()).unwrap();
        let v: Vec<_> = d.chars("su3").collect();
        assert_eq!(v, vec![("你", 900), ("擬", 10)], "順序不能被重排");
        assert_eq!(d.chars("cl3").count(), 1);
        assert_eq!(d.chars("沒有這個讀音").count(), 0);
    }

    #[test]
    fn 詞與同音字是兩個鍵空間() {
        // `su3` 在同音字表裡、不在詞表裡；`su3cl3` 反過來
        let d = ZhDict::new(built()).unwrap();
        assert!(!d.has_word("su3"), "單字不算詞");
        assert!(d.has_word("su3cl3"));
        assert_eq!(d.chars("su3cl3").count(), 0);
    }

    #[test]
    fn 超長的文字要被丟掉而不是截斷() {
        let long = "字".repeat(200); // 600 位元組
        let words = vec![
            ("a ".to_string(), long.clone()),
            ("b ".to_string(), "好".to_string()),
        ];
        let d = ZhDict::new(Box::leak(build(words, vec![]).into_boxed_slice())).unwrap();
        assert_eq!(d.word_count(), 1);
        assert_eq!(d.word("b "), Some("好"), "其餘不受影響");
    }

    /// 詞表的雜湊表每一槽都填了東西——`IndexRef::find` 不能永遠轉下去。
    /// 測試資料只有三個詞，槽數是最小的 16。
    #[test]
    fn 每槽都填滿的壞檔不能卡死() {
        let mut bad = built().to_vec();
        let off_widx = get_u32(&bad, 24) as usize;
        for i in 0..16 {
            let at = off_widx + i * 4;
            bad[at..at + 4].copy_from_slice(&1u32.to_le_bytes());
        }
        let d = ZhDict::new(Box::leak(bad.into_boxed_slice())).unwrap();
        assert!(d.word("nothing").is_none(), "查不到就是查不到，不能轉不停");
    }

    #[test]
    fn 壞檔要被認出來() {
        assert!(ZhDict::new(b"not a dict at all........").is_none());
        let mut bad = built().to_vec();
        bad[8] = 0xff; // 版本號
        assert!(ZhDict::new(Box::leak(bad.into_boxed_slice())).is_none());
    }

    #[test]
    fn 空表也要能用() {
        let d = ZhDict::new(Box::leak(build(vec![], vec![]).into_boxed_slice())).unwrap();
        assert!(d.is_empty());
        assert_eq!(d.word("x"), None);
        assert_eq!(d.chars("x").count(), 0);
    }
}
