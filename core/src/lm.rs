//! 中文**字級 bigram 語言模型**：選字時看前後文。
//!
//! # 為什麼需要它
//!
//! 在這之前，選字是「以字為單位、用詞去修正」——同音字靠字頻（已按讀音
//! 打折，§2.16），詞層再回頭修。**沒有任何一步看前後文**，所以
//! 「他在／他再」「這份／這分」「要跟／要根」這一類永遠分不開：光看字
//! 本身兩邊字頻差不多，等於在丟硬幣。
//!
//! 這一份補的正是「誰接誰」的統計。§2.15 否決過的是拿**詞頻**猜分詞
//! （單一詞出現幾次，不含接續），**是不同的資料，不是同一件事再試一次**。
//!
//! # 資料來源與版面
//!
//! RIME 八股文 `zh-hant-t-essay-bgc.gram`（LGPL-3），字級 bigram、
//! 10.5 萬條、9.8MB。版面是 darts-clone 的 double array，唯讀、
//! mmap 友善，跟 `dict_bin` 同性質。
//!
//! ```text
//! 偏移 0   32 bytes  "Rime::Grammar/1.0" ＋補零
//! 偏移 32  u32       checksum（實測是 0，不檢查）
//! 偏移 36  u32       double array 的**單元數**（不是位元組數）
//! 偏移 40  u32       相對指標，指到 double array（相對於偏移 40 本身）
//! 偏移 44  單元數×4  darts-clone 影像
//! ```
//!
//! **值是 `int(ln(次數) × 10000)`**，還原時除以 10000。注意那是
//! **次數不是機率**，所以高頻字天生佔便宜——用的時候一律扣掉自身字頻，
//! 見 [`Lm::score`]。
//!
//! **鍵的編碼不是 UTF-8**，是八股文自己的一套（見 `encode_into`）。
//!
//! # 三個實測到的限制
//!
//! 1. **沒有單字（unigram），也沒有三字（trigram）**——就是純粹的兩字
//!    組表。所以沒辦法從檔案自己算條件機率，分母只能用我們自己的
//!    `char_freq.txt`。
//! 2. **鍵沒有詞邊界**，存的就是字元序列。這反而是好事：不必知道斷詞
//!    位置，拿任意兩個字去查就行。
//! 3. **語料是簡體轉繁體來的**，`為`／`線`／`眾`／`群` 這些**零命中**
//!    ——模型用的是 `爲`／`綫`／`衆`／`羣`。見 `ALIAS_GROUPS`。

use std::collections::HashMap;
use std::sync::OnceLock;

/// 檔頭到 double array 之間的固定欄位長度。
const HEADER: usize = 44;
/// 檔案識別字串，認不得就當作沒有這份資料。
const MAGIC: &[u8] = b"Rime::Grammar/1.0";

/// 我們的字形與模型的字形，**共存、取最大值**。
///
/// # 為什麼是共存而不是單向轉換
///
/// 第一版做成「查表前把 `為` 換成 `爲`」，但那張表**方向搞反會出事**
/// ——掃描量到 `說`(972 命中) 換成 `説`(0) 會把好好的資料換沒。改成
/// 兩種寫法都查、取分數高的之後，方向錯誤最壞只是白查一次。
///
/// 另一個理由是**有些字兩邊都有資料**（`裡` 530／`裏` 870，語料裡混用），
/// 單向換會丟掉另一邊那幾百筆證據。
///
/// # 這張表怎麼來的
///
/// **不是人腦列候選**——那樣會漏。做法是拿詞庫前 3000 個高頻字，各用
/// 500 個高頻字當左右鄰去數命中，撈出「幾乎查不到」的，再對每個候選
/// 字形雙向驗證。人腦版漏掉了 `眾`(1100)、`群`(586)、`床`、`啟` 四個
/// 常用字，又收了四條反向的錯誤。
///
/// **只收真正的異體字**（同一個詞、不同字形）。`譬/闢`、`嚀/寧`、
/// `楣/眉` 這種「不是異體只是碰巧另一個常見」的**不收**——共存會把
/// `叮嚀` 的分數跟 `叮寧` 混在一起，那是真的污染。
const ALIAS_GROUPS: &[&[char]] = &[
    &['為', '爲'],
    &['線', '綫'],
    &['眾', '衆'],
    &['群', '羣'],
    &['床', '牀'],
    &['啟', '啓'],
    &['抬', '擡'],
    &['晒', '曬'],
    &['痴', '癡'],
    &['偽', '僞'],
    &['麽', '麼'],
    &['汙', '污'],
    &['裡', '裏'],
    &['著', '着'],
    &['台', '臺'],
    &['儘', '盡'],
];

/// **單向後備**：我們有這個字、模型沒有，查詢時借用另一個字形的資料。
///
/// 跟 `ALIAS_GROUPS` 的差別是**不雙向**——`妳`／`牠` 是台灣特有用法，
/// 模型裡完全不存在（零命中），但反過來 `你`／`他` 不該去借 `妳`／`牠`
/// 的資料。
const FALLBACK: &[(char, char)] = &[('妳', '你'), ('牠', '他'), ('矽', '硅')];

/// **中國用語黑名單**：模型偏好、但台灣不用的字對，查到當作沒資料。
///
/// 語料是簡體來的，`界面`／`剛纔` 這類會壓過台灣的 `介面`／`剛才`。
/// 落到這裡的字對直接跳過，退回我們自己的字頻決定。
///
/// **只需要收「同讀音會競爭」的**——`軟體`／`軟件` 讀音不同，在候選
/// 清單裡本來就不會撞，收了也沒用。
const CN_PAIRS: &[(char, char)] = &[
    ('剛', '纔'),
    ('界', '面'),
    ('信', '息'),
    ('視', '頻'),
    ('質', '量'),
    ('激', '光'),
    ('鼠', '標'),
    ('內', '存'),
];

/// 八股文的鍵編碼（`gram_encoding.cc`）。**不是 UTF-8。**
///
/// - 碼位 < 0x80：直接一個位元組（0 例外，寫成 `0xE0`）
/// - **0x4000 到 0xA000**：兩個位元組 `[(u>>8)+0x40, u&0xFF]`；
///   低位元組剛好是 0 時寫成 `[0xE1, (u>>8)+0x40]`。**CJK 常用區整段
///   落在這裡**，所以絕大多數鍵是「每字兩個位元組」
/// - 其餘：`[0xE0|n]` 加 n 個 `0x80|七位元` 的續接位元組
fn encode_into(chars: &[char], out: &mut Vec<u8>) {
    out.clear();
    for &c in chars {
        let u = c as u32;
        if u < 0x80 {
            out.push(if u == 0 { 0xE0 } else { u as u8 });
        } else if (0x4000..0xA000).contains(&u) {
            let hi = ((u >> 8) + 0x40) as u8;
            let lo = (u & 0xFF) as u8;
            if lo == 0 {
                out.push(0xE1);
                out.push(hi);
            } else {
                out.push(hi);
                out.push(lo);
            }
        } else {
            // 續接式：先算要幾個 7 位元的組
            let mut n = 0u32;
            let mut v = u;
            while v > 0 {
                n += 1;
                v >>= 7;
            }
            out.push(0xE0 | n as u8);
            for i in (0..n).rev() {
                out.push(0x80 | ((u >> (i * 7)) & 0x7F) as u8);
            }
        }
    }
}

/// darts-clone 的 double array 查詢。
///
/// 單元解碼（每個單元 4 位元組的 `u32`）：
/// ```text
/// has_leaf = (unit >> 8) & 1
/// label    = unit & 0x800000FF   （葉節點 bit31 = 1，跟純位元組比不會誤中）
/// offset   = (unit >> 10) << ((unit & 0x200) >> 6)
/// value    = unit & 0x7FFFFFFF
/// ```
struct Darts {
    units: &'static [u8],
    n: usize,
}

impl Darts {
    #[inline]
    fn unit(&self, i: usize) -> u32 {
        let at = i * 4;
        u32::from_le_bytes([
            self.units[at],
            self.units[at + 1],
            self.units[at + 2],
            self.units[at + 3],
        ])
    }

    /// 走完整個鍵，命中葉節點才回值。
    fn exact(&self, key: &[u8]) -> Option<u32> {
        let mut node = 0usize;
        let mut unit = self.unit(node);
        for &b in key {
            let offset = ((unit >> 10) << ((unit & 0x200) >> 6)) as usize;
            node = node ^ offset ^ b as usize;
            if node >= self.n {
                return None;
            }
            unit = self.unit(node);
            if unit & 0x8000_00FF != b as u32 {
                return None;
            }
        }
        if (unit >> 8) & 1 == 0 {
            return None;
        }
        let offset = ((unit >> 10) << ((unit & 0x200) >> 6)) as usize;
        let leaf = node ^ offset;
        if leaf >= self.n {
            return None;
        }
        Some(self.unit(leaf) & 0x7FFF_FFFF)
    }
}

/// 中文字級 bigram 模型。
pub struct Lm {
    darts: Darts,
    /// 我們自己的字頻（`char_freq.txt`），兩個用途：扣掉自身字頻讓
    /// `ln(次數)` 變成關聯強度、以及當「台灣常用字優先」的先驗。
    freq: HashMap<char, u32>,
    /// 字形別名：一個字對應所有要一起查的寫法（含它自己）。
    alias: HashMap<char, Vec<char>>,
}

static LM: OnceLock<Option<Lm>> = OnceLock::new();

impl Lm {
    /// 從 `.gram` 的位元組建模型。認不得版面就回 `None`（退回不用模型）。
    fn new(bytes: &'static [u8], freq: HashMap<char, u32>) -> Option<Lm> {
        if bytes.len() < HEADER || !bytes.starts_with(MAGIC) {
            return None;
        }
        let n = u32::from_le_bytes(bytes[36..40].try_into().ok()?) as usize;
        let rel = u32::from_le_bytes(bytes[40..44].try_into().ok()?) as usize;
        // 相對指標是相對於偏移 40 本身
        let at = 40usize.checked_add(rel)?;
        let end = at.checked_add(n.checked_mul(4)?)?;
        if end > bytes.len() {
            return None;
        }
        let mut alias: HashMap<char, Vec<char>> = HashMap::new();
        for grp in ALIAS_GROUPS {
            for &c in *grp {
                alias.entry(c).or_default().extend_from_slice(grp);
            }
        }
        for &(from, to) in FALLBACK {
            let e = alias.entry(from).or_insert_with(|| vec![from]);
            if !e.contains(&to) {
                e.push(to);
            }
        }
        Some(Lm {
            darts: Darts {
                units: &bytes[at..end],
                n,
            },
            freq,
            alias,
        })
    }

    /// 這個字要查哪些字形。沒有別名的字回它自己（不配置）。
    #[inline]
    fn forms<'a>(&'a self, c: &'a char) -> &'a [char] {
        match self.alias.get(c) {
            Some(v) => v.as_slice(),
            None => std::slice::from_ref(c),
        }
    }

    /// 我們自己的字頻取對數，當先驗與分母。
    #[inline]
    pub fn log_freq(&self, c: char) -> f32 {
        ((self.freq.get(&c).copied().unwrap_or(1) + 1) as f32).ln()
    }

    /// `a` 接 `b` 的**關聯強度**：`ln(次數) − ln(命中字形的字頻)`。
    ///
    /// # 為什麼要扣字頻
    ///
    /// 檔案存的是**次數不是條件機率**，`在` 比 `再` 常見兩個數量級，
    /// 所以 `看在` 的次數天生大於 `看再`。扣掉之後才是「這兩個字特別
    /// 愛黏在一起嗎」。實測 79% 到 82%。
    ///
    /// # 為什麼扣的是「命中字形」的字頻而不是原字的
    ///
    /// **這個坑實測踩到過**：共存讓罕見字形借用常用字形的次數，
    /// `謝→你` 與 `謝→妳` 拿到**完全一樣的原始次數**（15.01），但
    /// `妳` 字頻只有 54、`你` 有 4224——扣原字字頻的話 `妳` 分母小、
    /// 分數反而更高（11.0 對 6.7），於是「謝謝你」被改成「謝謝妳」。
    /// `纔` 字頻是 1，扣完直接飆到 14.3，「剛才」變「剛纔」。
    ///
    /// 改成扣命中字形的字頻之後，借來的次數也還回對應的分母，四句
    /// `妳` 全部修好。
    pub fn score(&self, a: char, b: char) -> Option<f32> {
        let mut buf = Vec::with_capacity(4);
        let mut best: Option<f32> = None;
        for &fa in self.forms(&a) {
            for &fb in self.forms(&b) {
                if CN_PAIRS.contains(&(fa, fb)) {
                    continue;
                }
                encode_into(&[fa, fb], &mut buf);
                if let Some(v) = self.darts.exact(&buf) {
                    let s = v as f32 / 10000.0 - self.log_freq(fb);
                    if best.is_none_or(|x| s > x) {
                        best = Some(s);
                    }
                }
            }
        }
        best
    }
}

/// 載入語言模型。找不到檔案或版面認不得就回 `None`，選字退回原本的
/// 行為（純字頻），**不是錯誤**——這份資料是加分項，不是必需品。
pub fn load(data_dir: &std::path::Path, freq: HashMap<char, u32>) -> Option<&'static Lm> {
    LM.get_or_init(|| {
        let path = data_dir.join("bopomofo").join("zh_bigram.gram");
        let bytes = crate::dict::map_file_pub(&path)?;
        Lm::new(bytes, freq)
    })
    .as_ref()
}

/// 取得已載入的模型。沒載入過回 `None`。
pub fn get() -> Option<&'static Lm> {
    LM.get().and_then(|o| o.as_ref())
}
