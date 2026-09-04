//! 累加式切法：候選是打字過程的產物，不是事後窮舉。
//!
//! # 為什麼不能窮舉
//!
//! **沒辦法知道使用者什麼時候要把字打完**。輸入法在每一鍵之後都得
//! 給得出候選，沒有「打完了，來算一次」這個時刻。
//!
//! 所以候選不是「對最終按鍵串窮舉」，而是**一路累加**：切法一旦成立
//! 就留著，新字元接在它後面延續。
//!
//! ```text
//! 打到 "o"    → 日:o
//! 打到 "ok"   → 英:ok                    （規則一把 o|k 黏起來）
//! 打到 "oka"  → 日:o|日:ka ＋ 英:ok|日:a  ← 兩條分支並存
//! ```
//!
//! `ok` 這條分支**一旦出現就留著**。每打一鍵重算整串的話，`ok` 在
//! 下一鍵就被丟掉了——那正是舊的窮舉做法的問題。
//!
//! # 每一鍵有兩條路
//!
//! 對每一種活著的切法，新字元可以：
//!
//! - **併入最後一段**——那一段變長
//! - **另起一段**——在新字元前面切一刀
//!
//! 兩條都展開，然後把「有段落非法」的丟掉。

use super::{prune, punct, Segment, SEPARATOR};
use crate::language::Language;
use crate::{bopomofo, romaji};

/// 一種切法：切點的位置（切在第 i 個字元之前）。
///
/// 用位置而不是存字串，是因為每打一鍵都要複製整個集合——存位置
/// 只是一串小整數，存字串則要複製所有內容。
pub type Cut = Vec<usize>;

/// 累加式切法引擎。
///
/// 每打一鍵呼叫 `push`，隨時可以用 `cuttings()` 取出目前的候選。
#[derive(Debug, Clone)]
pub struct Incremental {
    keys: String,
    chars: Vec<char>,
    /// 目前活著的切法。每一種都涵蓋到目前打到的位置。
    alive: Vec<Cut>,
    /// 段落驗證的結果，鍵是 `(起點, 終點)`。
    ///
    /// **每一鍵都要把所有活著的切法的所有段落重驗一次**——四百種切法
    /// 各三四段，就是上千次驗證，而同一個 `(起點, 終點)` 會在很多切法
    /// 裡重複出現。
    ///
    /// 用範圍當鍵而不是段落文字：一來省掉取字串的配置，二來**按鍵串
    /// 只會往後長，驗過的範圍內容永遠不會變**，所以這份快取跨按鍵
    /// 也一直有效，不必每鍵清空。
    ///
    /// `RefCell` 是因為 `cut_ok` 拿的是 `&self`（它被 `retain` 的閉包
    /// 呼叫，那時 `next` 借著可變參考）。
    checked: std::cell::RefCell<std::collections::HashMap<(u32, u32), Checked>>,
    /// 快取建立時的詞庫版本。
    ///
    /// **詞庫是背景載入的**，載好之前算出來的答案載好之後就不對了。
    /// 版本一變就把快取整份丟掉重算，見 `dict::GENERATION`。
    gen: std::cell::Cell<u64>,
}

/// 一個範圍驗過的結果。
#[derive(Debug, Clone, Copy)]
struct Checked {
    /// 這一段有引擎吃得下嗎
    lang_ok: bool,
    /// 丟棄規則放它過嗎。**只在 `end < n` 時才問**，所以可能還沒算過。
    keep: Option<bool>,
}

/// **`Default` 必須跟 `new()` 一樣**，不能用 `derive`。
///
/// `derive(Default)` 給的 `alive` 是空的 `Vec`，而累加式是靠「每種活著
/// 的切法各分兩條路」往前推的——空的清單分不出任何東西，之後打再多
/// 字都是零切法，預覽列永遠空白。
///
/// 這個 bug 的症狀是「重新載入 DLL 之後第一次輸入預組區空白，按一下
/// 倒退鍵就正常」：TSF 那層的 `State` 用 `derive(Default)` 建 `Session`，
/// 於是拿到一個死掉的引擎；而退格會走 `from_keys()` 整串重建，
/// 順手把引擎換成活的。
impl Default for Incremental {
    fn default() -> Self {
        Self::new()
    }
}

/// 活著的切法上限。
///
/// 累加式每打一鍵，每種切法都分裂成兩條（併入最後一段／另起一段），
/// 所以是 2ⁿ 成長。`prune` 的丟棄規則砍掉大部分，但**日文長句砍不到**
/// ——日文沒空白也沒聲調鍵，幾乎每個位置都能合法切開。
///
/// 超過時保留**切點少的**——切得越碎越可能是雜訊。
///
/// # 為什麼是 400
///
/// 原本設 2000，那是沒量過的隨手數字。實測每一鍵都要對全部切法排序、
/// 正規化、去重，而排序的每個判準都要查詞典——2000 種時 48 鍵的日文
/// 長句單鍵要 741ms，遠超過一幀的 16ms 預算。
///
/// 掃描結果（570 句測資的切點涵蓋 / 前 3 名）：
///
/// | 上限 | 50 | 100 | 200 | 250 | 300 | **400** | 500 | 2000 |
/// |---|---|---|---|---|---|---|---|---|
/// | 涵蓋 | 88% | 94% | 99% | 99.8% | 99.8% | **100%** | 100% | 100% |
/// | 前 3 | 87% | 93% | 98% | 98% | 98% | **98.4%** | 98.4% | 98.4% |
///
/// 400 是「切點涵蓋還能維持 100%」的最低值——那是硬指標，不能妥協。
const ALIVE_LIMIT: usize = 400;

impl Incremental {
    pub fn new() -> Self {
        Self {
            keys: String::new(),
            chars: Vec::new(),
            // 空字串只有一種切法：什麼都沒切
            alive: vec![Vec::new()],
            checked: Default::default(),
            gen: std::cell::Cell::new(crate::dict::generation()),
        }
    }

    /// 從既有的按鍵串重建（等同逐鍵 `push`）。
    pub fn from_keys(keys: &str) -> Self {
        let mut s = Self::new();
        for c in keys.chars() {
            s.push(c);
        }
        s
    }

    /// 打一個鍵。
    pub fn push(&mut self, c: char) {
        self.keys.push(c);
        self.chars.push(c);
        let n = self.chars.len();

        // 每種活著的切法都有兩條路：併入最後一段、或另起一段
        let mut next: Vec<Cut> = Vec::with_capacity(self.alive.len() * 2);
        for cut in &self.alive {
            next.push(cut.clone());
            let mut split = cut.clone();
            split.push(n - 1);
            next.push(split);
        }

        next.retain(|cut| self.cut_ok(cut, n));
        next.sort_unstable();
        next.dedup();

        // 超過上限時保留切點少的
        if next.len() > ALIVE_LIMIT {
            next.sort_by_key(|c| c.len());
            next.truncate(ALIVE_LIMIT);
            next.sort_unstable();
        }
        self.alive = next;
    }

    /// 這種切法的每一段都站得住嗎？
    fn cut_ok(&self, cut: &Cut, n: usize) -> bool {
        // 詞庫在背景載完了的話，之前算的答案作廢
        let now = crate::dict::generation();
        if self.gen.get() != now {
            self.gen.set(now);
            self.checked.borrow_mut().clear();
        }
        let mut begin = 0usize;
        for &end in cut.iter().chain(std::iter::once(&n)) {
            if end <= begin || end > n {
                return false;
            }
            let key = (begin as u32, end as u32);
            let mut rec = self.checked.borrow().get(&key).copied();
            if rec.is_none() {
                let seg = super::slice(&self.keys, &self.chars, begin, end);
                let r = Checked {
                    lang_ok: lang_of(&seg).is_some(),
                    keep: None,
                };
                self.checked.borrow_mut().insert(key, r);
                rec = Some(r);
            }
            let mut rec = rec.expect("剛才補上了");
            if !rec.lang_ok {
                return false;
            }
            // **丟棄規則只套在已完成的段**。
            //
            // 最後一段還在打——它現在不是英文詞，不代表下一鍵不是
            // （`chec` → `check`）。在這裡判死的話逐字打根本走不下去，
            // 實測 `kinnyoubi` 會從 19 種活著掉到只剩 1 種。
            if end < n {
                let keep = match rec.keep {
                    Some(k) => k,
                    None => {
                        // **這個答案之後也不會變**：丟棄規則最多看到
                        // `chars[end]`，而 `end < n` 表示那個字元已經
                        // 定下來了，往後只會在它右邊追加。
                        let k = prune::keep(&self.keys, &self.chars, begin, end);
                        rec.keep = Some(k);
                        self.checked.borrow_mut().insert(key, rec);
                        k
                    }
                };
                if !keep {
                    return false;
                }
            }
            begin = end;
        }
        true
    }

    /// 目前活著的候選，切點少的排前面。
    pub fn cuttings(&self) -> Vec<Vec<Segment>> {
        let mut out: Vec<&Cut> = self.alive.iter().collect();
        out.sort_by_key(|c| (c.len(), (*c).clone()));
        out.iter().map(|c| self.to_segments(c)).collect()
    }

    /// 目前有幾種活著的切法。
    pub fn len(&self) -> usize {
        self.alive.len()
    }

    pub fn is_empty(&self) -> bool {
        self.alive.is_empty()
    }

    /// 目前的按鍵串。
    pub fn keys(&self) -> &str {
        &self.keys
    }

    /// 目前所有切法用到的切點位置。
    pub fn cut_positions(&self) -> std::collections::BTreeSet<usize> {
        self.alive.iter().flatten().copied().collect()
    }

    fn to_segments(&self, cut: &Cut) -> Vec<Segment> {
        let n = self.chars.len();
        let mut out = Vec::new();
        let mut begin = 0usize;
        for &end in cut.iter().chain(std::iter::once(&n)) {
            let seg: String = self.chars[begin..end].iter().collect();
            let lang = lang_of(&seg).unwrap_or(Language::English);
            let is_mark =
                seg == SEPARATOR || (end == begin + 1 && punct::is_punct(&self.keys, begin));
            out.push(Segment {
                keys: seg,
                is_mark,
                lang,
            });
            begin = end;
        }
        out
    }
}

/// 這一段歸哪個引擎？依瀑布順序：注音 → 日文 → 英文。
///
/// 回 `None` 代表三個引擎都不收，這條切法就死了。
///
/// # 「有詞典收錄就以那個詞典為優先」
///
/// 瀑布順序讓日文永遠贏過英文，但日文的合法範圍很大——`file` 拼得成
/// ふぃぇ、`live` 拼得成 ぃゔぇ。那些其實是英文詞。
///
/// 所以合法日文還要再問一句：**只有英文詞典收它、日文詞典沒收**的話，
/// 就判英文。兩邊都收（`sushi`）或都沒收（活用形）維持日文優先。
fn lang_of(seg: &str) -> Option<Language> {
    if bopomofo::validity(seg) == bopomofo::Validity::Valid {
        return Some(Language::Bopomofo);
    }
    if romaji::validity(seg) == romaji::Validity::Valid {
        // 只有英文詞典收它 → 判英文
        if seg.chars().count() >= 2
            && crate::english::is_word(seg)
            && !crate::dict::is_japanese_word(seg)
        {
            return Some(Language::English);
        }
        // **兩邊都收時，很常用的英文詞也判英文**。
        //
        // 日文詞典 74 萬條，`you`（よう）、`the`（てぇ）、`time`（ちめ）
        // 這些都查得到，於是瀑布順序讓它們全變日文——英文前 5000 名裡
        // 有 145 個中招。排名夠前的就不該讓給日文。
        if crate::english::is_top_word(seg) {
            return Some(Language::English);
        }
        return Some(Language::Romaji);
    }
    // 英文是最後一站（passthrough）。
    //
    // **標點也走這裡**——不再有「標點前後均為切點」的特別規則。
    // 那條規則會把 `g;4`（ㄕㄤˋ）攔腰砍斷：累加式一次只長一個字元，
    // 中途必然經過 `g;`，而 `;` 被判成標點就讓整條分支死掉。
    // 交給引擎自己表態：`g;4` 注音認得，`hello,` 的逗號注音不認、
    // 落到英文自成一段。
    if !seg.is_empty() && seg.chars().all(|c| !c.is_control()) {
        return Some(Language::English);
    }
    None
}

#[cfg(test)]
mod tests {
    /// 逐鍵打出來的結果，要跟一次整串建出來的一模一樣。
    ///
    /// **這是段落驗證快取的守門員**。那份快取用 `(起點, 終點)` 當鍵，
    /// 前提是「按鍵串只會往後長，驗過的範圍內容永遠不變」。哪天有人
    /// 加了會縮短或改寫按鍵串的方法，這個測試就會紅——因為逐鍵那條
    /// 路會拿到過期的答案，跟整串重建對不起來。
    #[test]
    fn 逐鍵與整串重建結果相同() {
        // **先把詞庫載完**。累加式引擎每一步都依當時的詞庫狀態淘汰
        // 切法，載到一半的話兩邊淘汰的時機不同，比起來當然不一樣。
        // 正式環境不會遇到——第一次按鍵那條路會擋著等詞庫載完。
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data");
        crate::preload(&data, crate::config::Engines::default());

        let cases = [
            "su3cl3",
            "check u vu84",
            "sushiwotabemasu",
            "rup wu0 wu0 fu4cp3cl3ul41j4",
            "hello world 2024",
            "kinnyoubimadeniteishutsushinakereba",
        ];
        for keys in cases {
            let mut inc = Incremental::new();
            for (i, c) in keys.chars().enumerate() {
                inc.push(c);
                let prefix: String = keys.chars().take(i + 1).collect();
                let fresh = Incremental::from_keys(&prefix);
                assert_eq!(
                    inc.cuttings(),
                    fresh.cuttings(),
                    "打到「{prefix}」時，逐鍵與整串重建的切法不一樣"
                );
            }
        }
    }

    use super::*;

    fn load() -> bool {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data");
        !crate::english::load(&data).is_empty()
    }

    fn show(segs: &[Segment]) -> String {
        segs.iter()
            .map(|s| format!("{}:{}", s.lang.short(), s.keys.replace(' ', "␣")))
            .collect::<Vec<_>>()
            .join(" | ")
    }

    fn has(inc: &Incremental, want: &str) -> bool {
        inc.cuttings().iter().any(|c| show(c) == want)
    }

    #[test]
    fn 純注音_大家好() {
        // 大 ㄉㄚˋ=284、家 ㄐㄧㄚ=ru8␣、好 ㄏㄠˇ=cl3
        let inc = Incremental::from_keys("284ru8 cl3");
        assert!(has(&inc, "注:284ru8␣cl3"), "整段注音要在：{:?}", inc.len());
        assert!(has(&inc, "注:284 | 注:ru8␣ | 注:cl3"), "大|家|好 也要在");
    }

    #[test]
    fn 分支一旦出現就留著() {
        if !load() {
            eprintln!("詞庫未下載，跳過（跑 data/download.ps1）");
            return;
        }
        // ok 在第 2 鍵成立，之後不能被丟掉
        let inc = Incremental::from_keys("okao6jp4wu6");
        assert!(has(&inc, "英:ok | 注:ao6jp4wu6"), "ok沒問題");
    }

    #[test]
    fn 最後一段還在打不判死() {
        if !load() {
            return;
        }
        // chec 還不是英文詞，但它是最後一段
        let inc = Incremental::from_keys("chec");
        assert!(!inc.is_empty(), "還在打，不能全滅");
        // 打完就是詞了
        let inc = Incremental::from_keys("check");
        assert!(has(&inc, "英:check"));
    }

    #[test]
    fn 日文長詞() {
        if !load() {
            return;
        }
        let inc = Incremental::from_keys("kinnyoubi");
        assert!(has(&inc, "日:kinnyoubi"), "金曜日");
    }

    #[test]
    fn 逐鍵與一次建立等價() {
        let a = Incremental::from_keys("su3cl3");
        let mut b = Incremental::new();
        for c in "su3cl3".chars() {
            b.push(c);
        }
        assert_eq!(a.len(), b.len());
    }

    #[test]
    fn 空字串() {
        let inc = Incremental::new();
        assert_eq!(inc.len(), 1, "空字串有一種切法：什麼都沒切");
        assert!(inc.keys().is_empty());
    }

    #[test]
    fn 標點自成一段() {
        let inc = Incremental::from_keys("su3cl3,");
        assert!(has(&inc, "注:su3cl3 | 英:,"), "你好，");
    }
}
