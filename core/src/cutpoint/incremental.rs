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

/// 切法數到這個量就凍一刀——接近 `ALIVE_LIMIT`（400）但還沒撞死。
///
/// **用切法數而不是長度當觸發**，因為只有真的要爆的句子才需要凍：純中文
/// 長句的切法數少（正解切點是 0），永遠碰不到這條線，行為完全不變。
const FREEZE_TRIGGER: usize = 300;

/// 凍結點離游標至少要幾個按鍵。
///
/// 實作後在 850 句上重掃（`ALIVE_LIMIT = 800`）：
///
/// | 安全距離 | 切點涵蓋(long) | 長句錯字率 | 總計 | `check_rewrite` |
/// |---|---|---|---|---|
/// | **8** | **33/60** | **8.5%** | **754** | 2 |
/// | 12 | 27/60 | 10.4% | 753 | 2 |
/// | 16 | 23/60 | 12.0% | 753 | 2 |
/// | 20 | 10/60 | 16.7% | 752 | 0 |
///
/// # 為什麼取 8，即使 `check_rewrite` 不是 0
///
/// 那 2 次長這樣：`buggood ej/ tk ,showyl3,hameninatta` 打到一半時，
/// 同一格在 `は` 與 `ha` 之間**來回跳兩次**（130 次改寫裡的 1.5%，
/// 而且集中在同一句）。那是**分區的固有抖動**——凍第二刀時未凍區的
/// 邊界變了，同一格在凍結前後分屬不同區、判斷自然不同。
///
/// **使用者裁定（2026-09-07）：「打字中中間變動沒關係，只要使用者
/// 輸入完畢是對的就沒問題」。** 這修正了 `check_rewrite` 的定位——
/// 它量的是**打字過程**，而過程中的抖動與「打完之後突然變」是兩回事。
/// 原本那個「倒數第 3 格以上必須是 0」的硬指標，本意是擋後者。
///
/// 代價換來的是長句從幾乎不能用變成堪用：切點涵蓋 10 → 33/60、
/// 錯字率 16.7% → 8.5%。
///
/// 為什麼近處仍然不能凍（8 已經是下限，4 會更差）：那裡的排序還在跟
/// 後文互動。`bugu␣` 獨立算會判成日文 `ぶぐ`，只有在整串脈絡下（後面
/// 接注音、兩段湊得成詞）才切成 `英:bug | 注:u␣`。
///
/// §2.51 的 spike **沒有量過 `check_rewrite`**，這是實作後才發現的。
///
/// 為什麼近處不能凍：那裡的排序還在跟後文互動。`bugu␣` 獨立算會判成日文
/// `ぶぐ`，只有在整串脈絡下（後面接注音、兩段湊得成詞）才切成
/// `英:bug | 注:u␣`。
const FREEZE_SAFE: usize = 8;

/// 這個位置可以當凍結點嗎？——**保守版**的字面邊界。
///
/// # 三版都實測過，這是第三版
///
/// **第一版**用 `space::tone_suffix_start` 判空白。它要求注音尾巴至少切
/// 得出**兩個**音節（為了擋 `notebook` 的 `k` 被當成 ㄜˉ），所以
/// `…sushiu␣`（一）這種單音節過不了門檻，聲調空白被誤判成邊界，
/// `u␣vu84`（一下）被攔腰切開——**舊架構「今天→金天」的同型錯誤**
/// （§4.37）。60 句長測資有 18 句踩到。
///
/// **第二版**改用引擎當下第一名切法的段邊界。長測資改善很大，但純日文
/// 長句被弄壞：`mottohayakushuppatsushiteokeba` 中途的第一名是
/// `英:mott | 日:ohayaku…`，那個「語言切換點」本身就是誤判，凍下去就
/// 定死了。**純日文沒有任何真正的邊界**，不該有東西可凍。
///
/// **這一版**回到字面邊界，但空白的判準改成**寧可少凍**：只要尾端存在
/// **任何**可能待收尾的注音音節就不當邊界。`notebook␣` 的 `k` 因此也被
/// 當成聲調而不切——**少一個凍結點不會壞事，凍錯才會**。
///
/// 這一條就是跟死路清單（§4.37「凍結已確定段落」）劃清界線的地方：舊的
/// 判準是「含聲調鍵就凍」會凍在詞的中間；這裡只認標點與**確定不是聲調**
/// 的空白，**不猜**。
fn can_freeze_here(keys: &str, i: usize, before: &str) -> bool {
    let Some(c) = keys.chars().nth(i) else {
        return false;
    };
    if c == ' ' {
        // 這個空白是注音的一聲嗎？是的話不能當邊界。
        //
        // **要用 `tone_suffix_start` 而不是自己滑動起點問 `is_tone`。**
        // 第一版寫成「從每個起點都試一遍，任何一個命中就不凍」，看起來
        // 更保守，實際上是**繞過了 `is_tone` 的前提**——它的文件寫明
        // 「呼叫端要先讓英文吃掉前面」，單獨拿英文詞的尾巴去問會誤中：
        // `boring` 的 `g`、`email` 的 `l`、`commit` 的 `t` 全部回 true。
        //
        // 後果是**英文詞後面的空白永遠凍不了**，長句在第 17 鍵就撞滿
        // 上限而第一刀拖到第 39 鍵才凍。
        //
        // `tone_suffix_start` 自己有那道守門（前面有別的語言時，要求
        // 注音尾巴至少切得出兩個音節），那正是擋 `notebook` 的 `k` 用的。
        if super::space::tone_suffix_start(before).is_some() {
            return false;
        }
        // **單音節的一聲要另外問**。`tone_suffix_start` 在「前面有別的
        // 語言」時要求注音尾巴至少切得出**兩個**音節（那是擋 `notebook`
        // 的 `k` 用的），所以 `sushiu␣`（寿司一）這種單音節過不了門檻，
        // 空白被誤判成邊界、`u␣vu84`（一下）被攔腰切開——**正是 §2.51.13
        // 第一版踩過的坑**，也是舊架構「今天→金天」的同型錯誤。
        //
        // **已知限制：單音節的一聲漏得掉。**`tone_suffix_start` 在
        // 「前面有別的語言」時要求注音尾巴至少切得出**兩個**音節（那是
        // 擋 `notebook` 的 `k` 被當成 ㄜˉ 用的），所以 `sushiu␣`（寿司一）
        // 這種單音節過不了門檻，空白被誤判成邊界、`u␣vu84`（一下）被攔腰
        // 切開，`寿司一下notebook` 的正解因此掉到第 4 名。
        //
        // 試過兩種補法都失敗：問最後一個字元的話 `email` 的 `l` 也會中
        // （回到上面那個問題）；問「前一個字元是不是注音鍵」的話——**26 個
        // 字母全是注音鍵**，分不出來。
        //
        // 這是同一個歧義的兩面：`u␣`（一）與 `l␣` 在字面上無法區分，要靠
        // **前文的語言**判斷，而那正是 `is_tone` 的前提要求呼叫端先做的事。
        // `can_freeze_here` 拿不到那個資訊，要解得先讓它看得到當下的切法。
        return !before.is_empty();
    }
    punct::is_punct(keys, i)
}

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
    /// **已經凍結的前區**（分區，§2.51）。
    ///
    /// 長的混語言句子會撞爆 `ALIVE_LIMIT`，而截斷策略保留「切點少」的
    /// ——混語言的正解恰恰是切點最多的那一批，所以爆掉的瞬間第一個被丟
    /// 的就是它。實測 100 鍵長句 **0/60 正確、錯字率 90.9%**，連正解都
    /// 生不出來。
    ///
    /// 解法是在高信心邊界把前面定死，後面另起一區從頭累加——**正解不必
    /// 在一個已經爆掉的池子裡競爭**。放寬上限沒有用（實測要 8000 才夠，
    /// 效能上不可行），見 §2.51.9。
    ///
    /// `keys` 仍然是**完整的按鍵串**（凍結是內部的事，外面看不到），
    /// `chars`／`alive` 只涵蓋未凍結的部分。
    frozen: Vec<Segment>,
    /// **對外的完整按鍵串**（含已凍結的前區）。
    ///
    /// 跟 `keys` 的差別：`keys` 只涵蓋未凍結的後區，而且**必須跟 `chars`
    /// 逐字元對齊**——`cut_ok`／`prune::keep` 拿 `chars` 的索引去
    /// `slice(&keys, &chars, begin, end)` 取段落，兩者長度不一致的話
    /// 索引就對不上，取到錯的字串，所有分支被誤殺（實測後區 `alive`
    /// 卡在 10 不動，切點停在 8）。
    all_keys: String,
    /// 正在重放後區嗎？重放期間不再嘗試凍結。
    ///
    /// **這個旗標是必要的**：凍完之後把後區重新累加時，`frozen` 已經
    /// 非空，長句模式（凍過一次就每遇到邊界都凍）會生效，於是重放的
    /// 每一個字元都再凍一次，把後區的切法又削掉一截。
    ///
    /// 症狀：同一段 `download,go,cl3` 單獨 `from_keys` 的切點是
    /// `{2,3,4,6,7,8,9,11,12,14}`，凍結後重放只剩 `{2,3,4,6,7,8}`
    /// ——尾巴的切點全部消失，看起來像「長句切不出來」。
    replaying: bool,
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
    /// 段落屬於哪個語言，鍵一樣是 `(起點, 終點)`。
    ///
    /// **跟 `checked` 是不同的問題**：那個問「這段合不合法」（`cut_ok`
    /// 用），這個問「合法的話屬於誰」（`to_segments` 用）。兩者都要查
    /// 三本詞典，但呼叫點與時機不同。
    ///
    /// 為什麼值得快取：`to_segments` 對**每條切法的每一段**都問一次，
    /// 而 400 條切法裡段落高度重複——實測純日文 110 鍵是 1739 個段落
    /// 只有 92 個相異（**重複率 95%**）。加快取之後 `cuttings()` 從
    /// 2.57ms 降到 0.14ms。
    ///
    /// 有效性的依據跟 `checked` 同一條：**按鍵串只會往後長，範圍內容
    /// 永遠不變**。詞庫版本一變就跟著清掉。
    lang_cache: std::cell::RefCell<std::collections::HashMap<(u32, u32), Option<Language>>>,
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
///
/// # 2026-09-07 放寬到 800
///
/// 上面那張表是**沒有長句測資**時掃的（最長 35 鍵）。補了 `mixed_long`
/// （平均 100 鍵）之後才看得到 400 在長句上的代價：切點涵蓋只有 7/60。
///
/// 買得起是因為 §2.51.11.1 的快取讓每鍵成本降了 59%（p99 11.1 → 4.6ms），
/// 800 的最慢一鍵是 7.1ms，仍在 16ms 預算內（400 是 4.7ms）。
///
/// 為什麼停在 800 而不是更大——收益遞減，而成本是線性的：
///
/// | 上限 | 切點涵蓋(long) | 長句錯字率 | 最慢一鍵 |
/// |---|---|---|---|
/// | 400 | 28/60 | 9.9% | 4.5ms |
/// | **800** | **33/60** | **8.5%** | **6.9ms** |
/// | 1600 | 36/60 | 8.4% | 11.2ms |
/// | 2400 | — | — | 14.8ms ← 只剩 1.2ms 餘裕 |
///
/// 1600 多 3 句卻要多付 4.3ms；長度是變數，餘裕要留給更長的句子。
const ALIVE_LIMIT: usize = 800;

impl Incremental {
    pub fn new() -> Self {
        Self {
            keys: String::new(),
            chars: Vec::new(),
            frozen: Vec::new(),
            all_keys: String::new(),
            replaying: false,
            // 空字串只有一種切法：什麼都沒切
            alive: vec![Vec::new()],
            checked: Default::default(),
            lang_cache: Default::default(),
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
        self.all_keys.push(c);
        self.chars.push(c);
        let n = self.chars.len();

        // 反斜線是符號輸入的邊界（符號打法是把名字用兩個反斜線夾起來），
        // **兩邊都強制切一刀**。
        //
        // 不特別處理的話它就是個普通字元，跟著走「併入前一段」那條路
        // ——而英文段什麼字母串都收，所以打 noteq 那個名字時，第一名的
        // 切法是「英:反斜線 | 英:noteq＋反斜線」：收尾那個被吞進英文段
        // 裡了。符號比對要找的是**獨立成一段的**收尾反斜線，找不到就
        // 整個功能靜靜失效。
        //
        // 為什麼修在切點層而不是在符號那邊補救：這本來就是「哪裡該切」
        // 的問題，而且**不可預測**——notequal（詞典有這個詞）沒事、
        // noteq 中招，差別只在排序分數。使用者自己寫包用生造的名字就會
        // 踩到，而症狀是「名字明明寫了卻沒反應」，沒有任何錯誤訊息。
        //
        // 少掉一條分支，切法也變少，不花錢。
        let boundary =
            n > 1 && (c == crate::symbol::PREFIX || self.chars[n - 2] == crate::symbol::PREFIX);

        // 每種活著的切法都有兩條路：併入最後一段、或另起一段
        let mut next: Vec<Cut> = Vec::with_capacity(self.alive.len() * 2);
        for cut in &self.alive {
            if !boundary {
                next.push(cut.clone());
            }
            let mut split = cut.clone();
            split.push(n - 1);
            next.push(split);
        }

        next.retain(|cut| self.cut_ok(cut, n));
        next.sort_unstable();
        next.dedup();
        self.alive = next;

        // **先凍結再截斷**——順序反過來的話正解已經被砍掉了。
        //
        // 截斷保留「切點少」的，而混語言的正解恰恰是切點最多的那一批。
        // 原本的順序是「每一鍵先砍到 400、再看要不要凍」，等於每次凍結
        // 都是在一個已經丟掉正解的池子上動手——實測 92 鍵的句子凍了 3 刀
        // 卻仍然 0 分，切點涵蓋 7/60。
        //
        // 凍結會讓 `alive` 從幾百掉到幾十（前區定案、後區從頭累加），
        // 所以凍完往往就不必截斷了。
        self.maybe_freeze();

        // 凍完仍然超過上限才截斷
        if self.alive.len() > ALIVE_LIMIT {
            let mut cut = std::mem::take(&mut self.alive);
            cut.sort_by_key(|c| c.len());
            cut.truncate(ALIVE_LIMIT);
            cut.sort_unstable();
            self.alive = cut;
        }
    }

    /// 這種切法的每一段都站得住嗎？
    fn cut_ok(&self, cut: &Cut, n: usize) -> bool {
        // 詞庫在背景載完了的話，之前算的答案作廢
        let now = crate::dict::generation();
        if self.gen.get() != now {
            self.gen.set(now);
            self.checked.borrow_mut().clear();
            self.lang_cache.borrow_mut().clear();
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
        // **凍結的前區接在每一種切法前面**——外面看到的永遠是完整的
        // 分段，分區是引擎內部的事。
        if self.frozen.is_empty() {
            return out.iter().map(|c| self.to_segments(c)).collect();
        }
        out.iter()
            .map(|c| {
                let mut v = self.frozen.clone();
                v.extend(self.to_segments(c));
                v
            })
            .collect()
    }

    /// 切法數逼近上限時，把前面凍成定案。
    ///
    /// 往回找**離游標 `FREEZE_SAFE` 鍵以外、最靠近游標**的字面邊界。
    /// 找不到就不凍——純日文長句完全沒有字面邊界，那是刻意的：它不會
    /// 因為切法爆炸而選錯（正解切點是 0，永遠排在保留名單最前面），
    /// 只是效能沒解決，而那條線的解法是「別產生等價切法」（§2.51.11.1）。
    fn maybe_freeze(&mut self) {
        // **凍過一次就進入長句模式**：之後每遇到安全邊界就凍。
        //
        // 只看切法數的話會漏掉後半段——凍一刀之後 `alive` 掉到幾十，
        // 再也回不到門檻，於是 90 鍵的句子只凍第 23 鍵那一刀，後面
        // 60 幾鍵整段黏成一個英文段（實測錯字率只從 90.8% 降到 80.3%）。
        //
        // 「已經凍過」代表這句話確實長到會爆，那個判斷不會因為凍完
        // 分支變少就失效。
        if self.replaying {
            return;
        }
        if self.frozen.is_empty() && self.alive.len() < FREEZE_TRIGGER {
            return;
        }
        let n = self.chars.len();
        let pending: String = self.chars.iter().collect();
        let mut cut = 0usize;
        for i in (0..n.saturating_sub(FREEZE_SAFE)).rev() {
            let before: String = self.chars[..i].iter().collect();
            if can_freeze_here(&pending, i, &before) {
                cut = i + 1; // 邊界字元本身歸前區
                break;
            }
        }
        if cut == 0 {
            return;
        }
        let head: String = self.chars[..cut].iter().collect();
        let mut sub = Self::new();
        for c in head.chars() {
            sub.push(c);
        }
        let Some(best) = crate::cutpoint::rank::sort(sub.cuttings())
            .into_iter()
            .next()
        else {
            return;
        };
        // **前區存 normalize 之後的分段**——那是「這一段最終長什麼樣」
        // 的定案。不能存原始切法：同語言內部的切點不影響輸出，留著只是
        // 讓後面的比對多一堆等價形狀。
        self.frozen.extend(crate::cutpoint::normalize(&best));
        // 後區從頭重新累加。**遞迴是安全的**——`alive` 從 1 開始，
        // 重放這幾個字元到不了 `FREEZE_TRIGGER`，不會再凍一次。
        let rest: Vec<char> = self.chars[cut..].to_vec();
        // `keys` 交給 `push` 自己重新填——**不能先塞完整的再重放**，
        // 那會變成兩倍長。前區的按鍵在這裡補回去（它們已經不在後區的
        // `chars` 裡了，但 `keys` 對外的語意是完整按鍵串）。
        let frozen = std::mem::take(&mut self.frozen);
        // `all_keys` 要保住整串（對外的語意），`keys` 則跟著後區重來
        // ——它必須跟 `chars` 逐字元對齊，見那個欄位的說明。
        let all = std::mem::take(&mut self.all_keys);
        *self = Self::new();
        self.frozen = frozen;
        // 重放只補後區，`all_keys` 先扣掉將要被 `push` 加回去的部分
        self.all_keys = all.chars().take(all.chars().count() - rest.len()).collect();
        self.replaying = true;
        for c in rest {
            self.push(c);
        }
        self.replaying = false;
    }

    /// 目前有幾種活著的切法。
    pub fn len(&self) -> usize {
        self.alive.len()
    }

    pub fn is_empty(&self) -> bool {
        self.alive.is_empty()
    }

    /// 目前的按鍵串（**整串，含已凍結的前區**）。
    ///
    /// 回的是 `all_keys` 不是 `keys`——後者只涵蓋未凍結的後區，那是
    /// 內部用來跟 `chars` 對齊的，見那兩個欄位的說明。clippy 會抱怨
    /// 「getter 回錯欄位」，這裡是刻意的。
    #[allow(clippy::misnamed_getters)]
    pub fn keys(&self) -> &str {
        &self.all_keys
    }

    /// **使用者在段選單定案了開頭 `n` 個按鍵**——把它當成凍結區，
    /// 後區維持原狀。
    ///
    /// # 為什麼要有這個操作
    ///
    /// 段選單每定案一段就要重算後區。最直覺的做法是拿剩下的按鍵重開
    /// 一個 `Incremental`，但那會**連後區已經凍好的成果一起丟掉**：
    /// 實測 93 鍵的長句每定案一段要 40ms（一幀預算 16ms 的 2.5 倍），
    /// 而打完整句才 78ms——等於每選一段就重打半句。
    ///
    /// 這個操作只做兩件事：把 `frozen` 換成使用者定的，把 `alive` 裡
    /// 那 `n` 鍵切掉。**後區的分支、快取、凍結成果全部留著**。
    ///
    /// # 為什麼 `alive` 可以直接平移
    ///
    /// `alive` 存的是切點位置（相對於後區起點）。定案的那一段結尾
    /// 本來就是一個切點——不然它不會是「一段」——所以每一種活著的
    /// 切法都包含 `n` 這個位置，砍掉它以前的部分再平移就是新的後區。
    ///
    /// **不含 `n` 的切法要丟掉**：那些切法認為這裡不該切，跟使用者
    /// 的決定衝突。丟掉正是「使用者說了算」的意思。
    ///
    /// 回傳 `false` 代表 `n` 落在後區的中間、沒有任何切法在那裡切開
    /// ——呼叫端要退回「整個重建」那條路。
    pub fn freeze_user_prefix(&mut self, prefix: Vec<Segment>, n: usize) -> bool {
        let frozen_len: usize = self.frozen.iter().map(|s| s.keys.chars().count()).sum();

        // ── 情況一：使用者定的落在**引擎已經凍好的前區之內** ──
        //
        // 這是最常見的情況（長句打字時引擎早就凍了好幾刀）。後區完全
        // 不受影響——使用者改的那一段在更前面，`alive` 一個字都不必動。
        //
        // 只要把 `frozen` 的前 `n` 鍵換成使用者定的，後面的段留著。
        if n <= frozen_len {
            // 掃過凍結區，把段分成三類：`n` 以前的（丟掉，換成使用者
            // 定的）、跨過 `n` 的（拆開）、`n` 以後的（留著）
            let mut rest: Vec<Segment> = Vec::new();
            // 被拆開那一段的尾巴——要解凍丟回後區重新參與競爭
            let mut thawed: Option<String> = None;
            let mut pos = 0usize;
            for s in &self.frozen {
                let len = s.keys.chars().count();
                if pos >= n {
                    rest.push(s.clone());
                } else if pos + len > n {
                    // **使用者的切點落在引擎凍結段的中間**——他不同意
                    // 引擎在這裡的切法。
                    //
                    // 把那一段拆開：`n` 以前歸使用者（`prefix` 已經含了），
                    // `n` 以後**解凍**、丟回後區重新算。後面其他凍結段
                    // 不受影響。
                    //
                    // 實測 93 鍵長句約一半的定案會走到這裡——引擎凍得
                    // 很積極（每遇到邊界就凍），而使用者想改的往往正是
                    // 引擎凍錯的地方。整串重建要 40ms，只解凍一段是
                    // 「重放那一段的尾巴」，通常不到十個鍵。
                    thawed = Some(s.keys.chars().skip(n - pos).collect());
                }
                pos += len;
            }

            match thawed {
                // 正好落在段界上：換掉前面幾段就好，後區一個字都不動
                None => {
                    self.frozen = prefix;
                    self.frozen.extend(rest);
                    true
                }
                // 拆開了某一段：**只有那一段的尾巴要重算**。
                //
                // 後面其他凍結段不動——它們離使用者改的地方至少
                // `FREEZE_SAFE` 遠，那正是「凍在這裡是安全的」的意思
                // （見 `FREEZE_SAFE` 的說明：近處的排序才會跟後文互動）。
                //
                // 重算的範圍因此只有幾個鍵，不是整個後區。
                Some(tail) => {
                    let mut sub = Self::new();
                    for c in tail.chars() {
                        sub.push(c);
                    }
                    let Some(best) = crate::cutpoint::rank::sort(sub.cuttings())
                        .into_iter()
                        .next()
                    else {
                        // 尾巴自己算不出東西（極短或全是怪字元）——
                        // 退回整個重建，那條路一定走得通
                        return false;
                    };
                    self.frozen = prefix;
                    self.frozen.extend(crate::cutpoint::normalize(&best));
                    self.frozen.extend(rest);
                    true
                }
            }
        } else {
            // ── 情況三：使用者定的**超出引擎凍結區**，切在後區裡 ──
            //
            // 後區的每一種切法都要在這裡切開才留得住：不在這裡切的
            // 那些跟使用者的決定衝突，丟掉正是「使用者說了算」。
            let cut = n - frozen_len;
            if cut > self.chars.len() {
                return false;
            }
            let kept: Vec<Cut> = self
                .alive
                .iter()
                .filter(|c| cut == 0 || c.contains(&cut))
                .map(|c| c.iter().filter(|&&p| p > cut).map(|&p| p - cut).collect())
                .collect();
            if kept.is_empty() && cut > 0 {
                // 沒有任何切法在這裡切開——退回整個重建
                return false;
            }
            self.frozen = prefix;
            self.chars.drain(..cut);
            // `keys` 必須跟 `chars` 逐字元對齊（見那個欄位的說明）
            self.keys = self.keys.chars().skip(cut).collect();
            self.alive = kept;
            self.alive.sort_unstable();
            self.alive.dedup();
            // **段落驗證的快取要清掉**：它的鍵是 `(起點, 終點)`，而起點
            // 的基準剛剛平移了。不清的話會拿到別段的驗證結果。
            self.checked.borrow_mut().clear();
            self.lang_cache.borrow_mut().clear();
            true
        }
    }

    /// **把一段前區直接掛成凍結區**——給「整個重建」那條路收尾用。
    ///
    /// `freeze_user_prefix` 走不通時，呼叫端會拿剩下的按鍵重開一個
    /// `Incremental`。那個新引擎只認得後區，得把前區補回去，`keys()`
    /// 與 `cuttings()` 才會涵蓋整串——否則送出的文字會缺開頭。
    pub fn adopt_frozen(&mut self, prefix: Vec<Segment>) {
        let head: String = prefix.iter().map(|s| s.keys.as_str()).collect();
        self.all_keys = format!("{head}{}", self.all_keys);
        self.frozen = prefix;
    }

    /// 凍結區的分段。整串都凍住時（後區空了）它就是完整的分段。
    pub fn frozen_segments(&self) -> &[Segment] {
        &self.frozen
    }

    /// 目前所有切法用到的切點位置。
    pub fn cut_positions(&self) -> std::collections::BTreeSet<usize> {
        // **凍結的前區也要算進來**——`alive` 只涵蓋未凍結的後區，而位置
        // 是相對於後區起點的。分區之後前區的切點就在 `frozen` 的段邊界上，
        // 漏掉的話 `cutpool` 會以為那些切點生不出來（實測 118 句誤報）。
        let mut out = std::collections::BTreeSet::new();
        let mut base = 0usize;
        for seg in &self.frozen {
            base += seg.keys.chars().count();
            out.insert(base); // 每一段的結尾就是一個切點
        }
        // 前區與後區的交界本身也是切點，上面最後一次 insert 已經放進去了
        for &p in self.alive.iter().flatten() {
            out.insert(base + p);
        }
        out
    }

    fn to_segments(&self, cut: &Cut) -> Vec<Segment> {
        let n = self.chars.len();
        let mut out = Vec::new();
        let mut begin = 0usize;
        for &end in cut.iter().chain(std::iter::once(&n)) {
            let seg: String = self.chars[begin..end].iter().collect();
            let key = (begin as u32, end as u32);
            let cached = self.lang_cache.borrow().get(&key).copied();
            let lang = match cached {
                Some(l) => l,
                None => {
                    let l = lang_of(&seg);
                    self.lang_cache.borrow_mut().insert(key, l);
                    l
                }
            }
            .unwrap_or(Language::English);
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
    /// 反斜線一定自成一段，不會被英文段吞掉。
    ///
    /// **這條擋的是一個靜靜失效的坑**：符號打法是把名字用兩個反斜線
    /// 夾起來，而符號比對要找的是獨立成一段的收尾反斜線。放任反斜線
    /// 走一般的「併入前一段」那條路的話，詞典收不到的名字（`abcx`）
    /// 會被切成「英:反斜線 | 英:abcx＋反斜線」，符號整個叫不出來，
    /// 而且沒有任何錯誤訊息。
    ///
    /// `notequal` 這種詞典有的沒事、`noteq` 中招，差別只在排序分數
    /// ——所以不能靠「挑好名字」迴避。
    #[test]
    fn 反斜線一定自成一段() {
        // 英文段的丟棄規則要查詞典（`abcx` 不是詞），沒載的話整條切法
        // 會被殺光，測不到我們要測的東西
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data");
        crate::preload(&data, crate::config::Engines::default());
        if !crate::english::is_loaded() {
            eprintln!("詞庫未下載，跳過（跑 data/download.ps1）");
            return;
        }
        // 案例挑**詞典一定收得到**的名字：生造的名字（noteq）活不活得下來
        // 還要看英文詞典與引擎開關，那是 `prune` 的事，會讓這條測試隨環境
        // 變紅變綠。這裡只管一件事——反斜線不准黏在別的段上。
        for keys in [r"\star\", r"\check\", r"\vu/ \", r"a\b\c"] {
            let cuts = super::Incremental::from_keys(keys).cuttings();
            assert!(!cuts.is_empty(), "{keys} 一種切法都沒有");
            for cut in &cuts {
                for seg in cut {
                    if seg.keys.contains('\\') {
                        assert_eq!(seg.keys, "\\", "{keys} 的反斜線黏在別的段裡：{cut:?}");
                    }
                }
            }
        }
    }

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
