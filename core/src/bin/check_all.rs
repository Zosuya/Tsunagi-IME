//! 漏斗計分：每一句從頭到尾走一遍，記下它**倒在哪一關**。
//!
//! # 為什麼要這支
//!
//! 從前十支計分器各量一段，數字散在十份報表裡。想知道「這次改動到底
//! 是好是壞」得自己把十個百分比在腦子裡合起來——而它們的分母還不一樣
//! （段／句／字／每百鍵）。
//!
//! 但**不能把各關的分數加總**。三個理由：
//!
//! 1. **各關不獨立，是漏斗**。切不出正解的句子，排序必掛、選字必掛，
//!    加總等於同一個錯扣三次
//! 2. **單位不同**。合法性算「段」、排序算「句」、錯字率算「字」、
//!    改寫算「每百鍵次數」——相加沒有意義
//! 3. **總分會把問題藏起來**。修 `en_bopomofo` 的錯法，排序會改走
//!    `en_split` 的錯法（測資檔頭記載過）——**一種錯換成另一種**，
//!    總分不動但行為變了
//!
//! 所以這支的做法是：每句照順序過關，**倒在哪一關就記在哪一格，
//! 而且只記一次**。最後活到底的句數就是那個單一數字，同時看得到
//! 每一層掉了幾句。
//!
//! # 關卡順序
//!
//! ```text
//! ⓪ 資料稽核    三欄自己矛盾嗎          ← 閘門，要先是 0
//! ① 合法性      注音／日文段判得合法嗎   （Phase 1 三個單語引擎）
//! ② 切法生成    正解切得出來嗎           ← 閘門，生不出來排序再好也沒用
//! ③ 切法排序    切法第一名對嗎           （產品實際的累加式行為）
//! ④ 選字        使用者看到的字對嗎
//! ⑤ 逐鍵穩定    前面對的字會不會被後面的鍵改掉
//! ```
//!
//! 前兩個閘門（⓪②）不是 0 就**警告但繼續**——它們代表尺本身有問題，
//! 後面的數字要打折看。
//!
//! `check_freeze`（凍結機制）與 `check_lm`（讀取器實作）不進漏斗：
//! 前者量的是機制本身、後者是驗實作跟 Python 版一致，性質不同。
//!
//! 用法：
//! ```text
//! cargo run --release -p ime-core --bin check_all
//! cargo run --release -p ime-core --bin check_all -- --save     # 存基準
//! cargo run --release -p ime-core --bin check_all -- --tag en_vowel  # 只看一節
//! cargo run --release -p ime-core --bin check_all -- --all     # 每一關的例子全部印出來
//! ```

use ime_core::compose;
use ime_core::cutpoint::incremental::Incremental;
use ime_core::cutpoint::{normalize, rank};
use ime_core::session::Session;
use std::collections::BTreeMap;

#[path = "common/testdata.rs"]
mod testdata;

use testdata::Row;

/// 一句倒在哪一關。順序就是漏斗的順序。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Stage {
    /// 三欄自己矛盾（按鍵欄與文字欄對不上）
    BadData,
    /// 段落判不合法
    Invalid,
    /// 正解切不出來——候選池裡根本沒有
    NotGenerated,
    /// 切得出來但排序不是第一名
    Misranked,
    /// 切對了但字錯
    WrongText,
    /// 從頭到尾都對
    Pass,
}

impl Stage {
    fn label(self) -> &'static str {
        match self {
            Stage::BadData => "⓪ 資料矛盾",
            Stage::Invalid => "① 判不合法",
            Stage::NotGenerated => "② 切不出來",
            Stage::Misranked => "③ 排序非第一",
            Stage::WrongText => "④ 字錯",
            Stage::Pass => "✓ 全對",
        }
    }
}

/// 一節的統計：每一關掉了幾句。
#[derive(Default, Clone)]
struct Tally {
    total: usize,
    by_stage: BTreeMap<i8, usize>,
}

impl Tally {
    fn add(&mut self, s: Stage) {
        self.total += 1;
        *self.by_stage.entry(s as i8).or_insert(0) += 1;
    }
    fn get(&self, s: Stage) -> usize {
        self.by_stage.get(&(s as i8)).copied().unwrap_or(0)
    }
    fn pass_rate(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        self.get(Stage::Pass) as f64 / self.total as f64 * 100.0
    }
}

/// 這一段該由注音引擎負責嗎？
///
/// **不能只看期望文字是不是漢字**——中日共用漢字，`生誕`／`明日`／`寿司`
/// 都是純漢字但用羅馬字打的日文。要一起看按鍵：注音的按鍵一定以聲調鍵
/// （`3467` 或空白）收尾，日文羅馬字永遠不會有。
fn is_bopomofo_seg(text: &str, keys: &str) -> bool {
    let all_han = !text.is_empty() && text.chars().all(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
    let tone_end = keys
        .chars()
        .last()
        .is_some_and(|c| matches!(c, ' ' | '3' | '4' | '6' | '7'));
    all_han && tone_end
}

/// ① 合法性：這一列的每個注音／日文段，單語引擎都判得合法嗎？
fn stage_valid(row: &Row) -> bool {
    let want_segs = row.want_segs();
    let key_segs = row.expected_segs();
    // 段數對不上就比不了——那是資料的問題，⓪ 那關會抓
    if want_segs.len() != key_segs.len() {
        return true;
    }
    for (w, k) in want_segs.iter().zip(key_segs.iter()) {
        if is_bopomofo_seg(w, k)
            && ime_core::bopomofo::validity(k) != ime_core::bopomofo::Validity::Valid
        {
            return false;
        }
    }
    true
}

/// 總計那一列在 `extra` 裡的鍵。用節名不可能長成的樣子。
const TOTAL: &str = "\u{0}總計";

/// 併進漏斗的三項統計（原本各自散在 `check_incremental`／
/// `check_rewrite`／`check_compose` 裡）。
#[derive(Default, Clone)]
struct Extra {
    /// 正解名次的總和與筆數——平均名次 = `rank_sum / rank_n`。
    ///
    /// 1.00 代表整節都排第一名；越大代表排序越差。切不出來的句子不算
    /// 進來（那是生成問題，名次沒有意義）。
    rank_sum: usize,
    rank_n: usize,
    /// 改寫伸到游標 3 格以外的次數。**這是硬指標，必須是 0**。
    ///
    /// 1～2 格是「正在打的那個詞」，改是對的；3 格以上代表動到使用者
    /// 早就看過、接受了的字。
    far: usize,
    /// **差一步**：沒通過，但正解就在第 2 名的句數。
    ///
    /// 兩種都算，因為使用者付的代價一樣是「按一次鍵」：
    ///
    /// - ③ 排序：正解的**切法**排第 2（按 Tab 換切法就拿得到）
    /// - ④ 字錯：每個錯的格，正解都在**候選**第 2 名（按選字鍵就有）
    ///
    /// **這一欄不影響通過與否**（2026-09-07 使用者裁決 A 案）。放寬
    /// 判定會讓這把尺換刻度、跟以前的數字不能比，而且從此看不出第一名
    /// 準不準——長句每個位置都要按一次鍵，跟完全不用按是兩種產品。
    /// 所以判定不動，只把「還差一步」的數量顯示出來。
    second: usize,
    /// 錯的字數與期望的總字數——錯字率 = `errs / chars`。
    errs: usize,
    chars: usize,
}

/// 依語言拆的錯字統計（原本在 `check_compose`）。
///
/// **只算切法正確的句子**——切法都錯了的話，「哪一段屬於哪個語言」
/// 本身就不可信，拿它歸戶只會把錯誤算到無辜的語言頭上。
#[derive(Default, Clone)]
struct LangStat {
    segs: usize,
    ok_segs: usize,
    chars: usize,
    errs: usize,
}

/// 字錯的句子**差一步**嗎？——每個錯的格，正解都在候選第 2 名。
///
/// 使用者付的代價是「在錯的那幾格各按一次選字鍵」。全部都在第 2 名
/// 才算，有任何一格排更後面就不算——那要按好幾次方向鍵找。
///
/// 段數對不上（切法不同）時回 `false`：那時「哪一格對應哪個字」本身
/// 就不可信，硬比只會得到假的樂觀數字。
fn near_miss_by_slot(want: &str, got_slots: &[compose::Slot]) -> bool {
    let want: Vec<char> = testdata::strip_spaces(want).chars().collect();
    // 顯示出來的字（去掉空白）要跟期望一樣長，才對得起來
    let shown: Vec<(usize, char)> = got_slots
        .iter()
        .enumerate()
        .flat_map(|(i, s)| s.text.chars().map(move |c| (i, c)))
        .filter(|(_, c)| *c != ' ' && *c != '\u{3000}')
        .collect();
    if shown.len() != want.len() || want.is_empty() {
        return false;
    }
    let mut wrong = 0usize;
    for (&w, &(si, g)) in want.iter().zip(shown.iter()) {
        if w == g {
            continue;
        }
        wrong += 1;
        // 這一格的候選裡，正解排第 2 嗎？
        let cands = compose::candidates_for(&got_slots[si]);
        let ok = cands
            .get(1)
            .is_some_and(|c| c.chars().eq(std::iter::once(w)));
        if !ok {
            return false;
        }
    }
    wrong > 0
}

/// 這一句對依語言的統計有什麼貢獻？
///
/// 回 `None` 代表切法不正確或段數對不上，這句不歸戶。
fn lang_breakdown(
    row: &Row,
    got_slots: &[compose::Slot],
    top1: &[ime_core::cutpoint::Segment],
    got: &str,
) -> Option<Vec<(&'static str, usize, usize, bool)>> {
    // **寬容-列舉要用命中的那個選項**——固定用第一個的話，
    // `アニメ || anime` 這種明明打對也會被算成錯（實測日文錯字率
    // 從 6.4% 灌水到 16.2%）。
    let binding = testdata::match_text(row, got)
        .map(|w| w.trim_start_matches('~').to_string())
        .unwrap_or_else(|| {
            row.alts()
                .first()
                .copied()
                .unwrap_or("")
                .trim_start_matches('~')
                .to_string()
        });
    let want = binding.as_str();
    let cut_ok = top1.iter().map(|s| s.keys.clone()).collect::<Vec<_>>() == row.expected_segs();
    let texts: Vec<&str> = want.split('|').collect();
    if !cut_ok || texts.len() != top1.len() {
        return None;
    }
    let mut out = Vec::new();
    let mut si = 0usize;
    for (seg, want_seg) in top1.iter().zip(texts.iter()) {
        // 一個段可能被切成好幾格（注音是一音節一格），按鍵接回原段
        // 長度為止就是這一段的格
        let mut acc = String::new();
        let mut got_seg = String::new();
        while si < got_slots.len() && acc.len() < seg.keys.len() {
            acc.push_str(&got_slots[si].keys);
            got_seg.push_str(&got_slots[si].text);
            si += 1;
        }
        let name = if seg.is_mark {
            "標點／空白"
        } else {
            match seg.lang {
                ime_core::language::Language::Bopomofo => "注音",
                ime_core::language::Language::Romaji => "日文",
                ime_core::language::Language::English => "英文",
            }
        };
        let we = testdata::strip_spaces(&want_seg.replace('_', " "));
        let ge = testdata::strip_spaces(&got_seg);
        out.push((
            name,
            we.chars().count(),
            testdata::edit_distance(&we, &ge),
            we == ge,
        ));
    }
    Some(out)
}

/// 一句話走完整條漏斗的結果。
///
/// 除了「倒在哪一關」，還帶三項**併進來的資訊**（原本各自散在
/// `check_incremental`／`check_rewrite`／`check_compose`）：排名、
/// 改寫的觸及範圍、錯了幾個字。
struct Outcome {
    stage: Stage,
    note: String,
    /// 正解在候選裡排第幾（1 起算）。切不出來時是 `None`。
    rank: Option<usize>,
    /// 改寫伸到游標 3 格外的**次數**。**這是硬指標，必須是 0**。
    rewrite_far: usize,
    /// 這句錯了幾個字（編輯距離），與期望的字數。
    errs: usize,
    chars: usize,
    /// 依語言的貢獻：`(語言, 字數, 錯字, 整段對不對)`。切法錯就是空的。
    lang: Vec<(&'static str, usize, usize, bool)>,
    /// **差一步**：沒通過，但正解就在第 2 名（按一次鍵就拿得到）。
    /// 判定不受影響，見 `Extra::second`。
    near_miss: bool,
}

/// 一句話走完整條漏斗，回傳它倒在哪一關。
fn run(row: &Row) -> Outcome {
    // ① 合法性
    let want_text = testdata::expected_text(
        row.alts()
            .first()
            .copied()
            .unwrap_or("")
            .trim_start_matches('~'),
    );
    let want_chars = testdata::strip_spaces(&want_text).chars().count();
    // **改寫要一律量，不管這句最後對不對**——它是打字過程的性質，
    // 跟結果無關。只在通過的句子量的話會漏掉（實測漏了 3 次超標）。
    let far_of = |keys: &str| rewrite_shape(keys).1;
    // **錯字率一律用實際的編輯距離**，跟 `check_compose` 對齊。
    // 早期版本對「切不出來」那些直接記成「整句都錯」，量出來的錯字率
    // 是 16.6%（實際 5.4%）——那高估了三倍。
    let fail = |stage: Stage, note: String, rank: Option<usize>, errs: usize| Outcome {
        stage,
        note,
        rank,
        rewrite_far: far_of(&row.keys),
        errs,
        chars: want_chars,
        lang: Vec::new(),
        // ①不合法、②切不出來都不是「差一步」——正解根本沒被生出來
        near_miss: false,
    };
    // **寬容-列舉要用命中的那個選項當基準**，跟 `check_compose` 一致
    // ——固定用第一個的話，`アニメ || anime` 這種明明打對也會被算成錯。
    let errs_of = |got: &str| {
        let want = testdata::match_text(row, got)
            .map(|w| testdata::expected_text(w.trim_start_matches('~')))
            .unwrap_or_else(|| want_text.clone());
        testdata::edit_distance(&testdata::strip_spaces(&want), &testdata::strip_spaces(got))
    };
    let top_text = || {
        let cands = rank::sort(Incremental::from_keys(&row.keys).cuttings());
        cands
            .first()
            .map(|c| compose::text_of(&compose::compose(&normalize(c))))
            .unwrap_or_default()
    };
    if !stage_valid(row) {
        let got = top_text();
        return fail(Stage::Invalid, String::new(), None, errs_of(&got));
    }

    let expected = row.expected_segs();
    let cands = rank::sort(Incremental::from_keys(&row.keys).cuttings());

    // 正規化後去重——名次算的是相異輸出的名次
    let mut seen = std::collections::HashSet::new();
    let uniq: Vec<Vec<ime_core::cutpoint::Segment>> = cands
        .iter()
        .map(|c| normalize(c))
        .filter(|c| {
            seen.insert(
                c.iter()
                    .map(|s| s.keys.clone())
                    .collect::<Vec<_>>()
                    .join("|"),
            )
        })
        .collect();

    let rank_of = uniq
        .iter()
        .position(|c| c.iter().map(|s| s.keys.clone()).collect::<Vec<_>>() == expected);

    // ② 切法生成
    let Some(r) = rank_of else {
        let got = uniq
            .first()
            .map(|c| compose::text_of(&compose::compose(c)))
            .unwrap_or_default();
        // **帶上診斷資訊**——只說「切不出來」的話還要另外跑 cutpool
        // 才知道缺哪裡。缺的切點位置＋第一名實際切成什麼，是往下追的
        // 起點。
        let want_cuts = {
            let mut set = std::collections::BTreeSet::new();
            let mut n = 0usize;
            let segs = row.expected_segs();
            for seg in &segs[..segs.len().saturating_sub(1)] {
                n += seg.chars().count();
                set.insert(n);
            }
            set
        };
        let pool = Incremental::from_keys(&row.keys).cut_positions();
        let miss: Vec<_> = want_cuts.difference(&pool).copied().collect();
        // 長句的分段列出來會超過一行，只留缺口附近那幾段
        let top = uniq
            .first()
            .map(|c| {
                let all: Vec<String> = c
                    .iter()
                    .map(|s| format!("{}:{}", s.lang.short(), s.keys.replace(' ', "␣")))
                    .collect();
                if all.len() <= 8 {
                    return all.join(" | ");
                }
                format!("{} …（共 {} 段）", all[..8].join(" | "), all.len())
            })
            .unwrap_or_default();
        let note = if miss.is_empty() {
            format!("實際「{top}」")
        } else {
            format!("缺切點 {miss:?}，實際「{top}」")
        };
        return fail(Stage::NotGenerated, note, None, errs_of(&got));
    };

    let top1 = uniq.first().cloned().unwrap_or_default();
    let slots = compose::compose(&top1);
    let got = compose::text_of(&slots);

    // ③ 排序：切法第一名不是正解
    //
    // 注意順序——**先看字對不對再判排序**。切法不同但字一樣的情況很常見
    // （純中文的段數怎麼切，審核表 A1 判為寬容-段），那種不該算排序錯。
    let text_ok = testdata::match_text(row, &got).is_some();
    let errs = errs_of(&got);
    if r != 0 && !text_ok {
        return Outcome {
            stage: Stage::Misranked,
            note: format!("第{}名，實際「{got}」", r + 1),
            rank: Some(r + 1),
            rewrite_far: far_of(&row.keys),
            errs,
            chars: want_chars,
            lang: Vec::new(),
            // 正解的切法排第 2——按 Tab 換切法就拿得到
            near_miss: r == 1,
        };
    }

    // ④ 選字
    if !text_ok {
        return Outcome {
            stage: Stage::WrongText,
            note: format!("期望「{want_text}」實際「{got}」"),
            rank: Some(r + 1),
            rewrite_far: far_of(&row.keys),
            errs,
            chars: want_chars,
            lang: lang_breakdown(row, &slots, &top1, &got).unwrap_or_default(),
            // 每個錯的格，正解都在候選第 2 名——各按一次選字鍵就好
            near_miss: near_miss_by_slot(&want_text, &slots),
        };
    }

    // ⑤ 逐鍵穩定性：打的過程中，已經對的字會不會被後面的鍵改掉
    //
    // **這關不擋人**。改寫是 `apply_word_context` 的設計意圖——打「擬郝」
    // 選了「你」，「郝」跟著變「好」正是要的行為（見 check_rewrite 的
    // 說明）。只有使用者已經滿意時才算問題，而測資看不出「使用者滿不
    // 滿意」。所以結果仍算通過，只是額外記一筆讓數字看得見。
    let (sample, far) = rewrite_shape(&row.keys);
    Outcome {
        stage: Stage::Pass,
        note: sample
            .map(|x| format!("（打字中曾改寫：{x}）"))
            .unwrap_or_default(),
        rank: Some(r + 1),
        rewrite_far: far,
        errs,
        chars: want_chars,
        lang: lang_breakdown(row, &slots, &top1, &got).unwrap_or_default(),
        // 已經通過了，不必「差一步」
        near_miss: false,
    }
}

/// 上一步「除了最後一格」的那些格：(涵蓋的按鍵長度, 接起來的文字)。
///
/// 使用者繼續打字只會往後加，這段按鍵**再也沒被碰過**——所以它們對應的
/// 字不該變。跟 `check_rewrite` 同一個判準。
fn settled(s: &Session) -> (usize, String) {
    let slots = s.slots();
    if slots.len() < 2 {
        return (0, String::new());
    }
    let keep = &slots[..slots.len() - 1];
    (
        keep.iter().map(|x| x.keys.len()).sum(),
        keep.iter().map(|x| x.text.as_str()).collect(),
    )
}

/// 上一步已定的那段按鍵，這一步對應到的文字是什麼？
///
/// 沿著格子累加按鍵長度，剛好湊滿 `plen` 就回傳那幾格的文字。湊不滿
/// （格子邊界移動了，跟上一步對不齊）回 `None`——那是分段移動，
/// 不算內容改寫。
fn text_covering(s: &Session, plen: usize) -> Option<String> {
    let mut acc = 0usize;
    let mut out = String::new();
    for sl in s.slots() {
        if acc == plen {
            return Some(out);
        }
        acc += sl.keys.len();
        out.push_str(&sl.text);
    }
    (acc == plen).then_some(out)
}

/// 逐鍵餵進去，看已定的那段文字有沒有被改寫。回傳第一次改寫的樣子。
///
/// 判準跟 `check_rewrite` 一致：只算**內容改寫**（同一段按鍵、格子邊界
/// 也對得齊，但字變了），分段邊界移動不算——那是引擎重新理解句子，
/// 是意圖不是 bug。
/// 上一步已定的那些格：`(按鍵, 文字)`。
///
/// 跟 `settled` 的差別是**逐格保留**——算「改寫伸到游標幾格外」需要
/// 知道是哪一格變了，把整段串成字串就看不出來了。
fn settled_slots(s: &Session) -> Vec<(String, String)> {
    let slots = s.slots();
    if slots.len() < 2 {
        return Vec::new();
    }
    slots[..slots.len() - 1]
        .iter()
        .map(|x| (x.keys.clone(), x.text.clone()))
        .collect()
}

/// 逐鍵餵進去，回傳「第一次改寫的樣子」與「**伸到游標 3 格外的次數**」。
///
/// # 為什麼要算距離
///
/// 改寫本身不是問題——`apply_word_context` 的設計意圖就是「打『擬郝』
/// 選了『你』，『郝』跟著變『好』」。判準是**觸及範圍**：
///
/// | 距離 | 意義 |
/// |---|---|
/// | 倒數第 1～2 格 | 正在打的那個詞，改是對的 |
/// | **倒數第 3 格以上** | 早就打完、看過的字突然變 ← 使用者會抱怨的 |
///
/// **只算內容改寫**（同一段按鍵、格子邊界也對得齊，但字變了），分段
/// 邊界移動不算——那是引擎重新理解句子，是意圖不是 bug。
fn rewrite_shape(keys: &str) -> (Option<String>, usize) {
    let mut s = Session::new();
    let mut sample: Option<String> = None;
    // **算次數不是最遠距離**——同一句可能有好幾次，跟 `check_rewrite`
    // 的統計方式對齊才比得出來
    let mut far = 0usize;
    let mut prev: Option<(usize, String)> = None;
    let mut prev_slots: Vec<(String, String)> = Vec::new();
    for c in keys.chars() {
        s.push(c);
        // 樣本用整段比對（跟原本一樣，訊息比較好讀）
        if sample.is_none() {
            if let Some((plen, ptext)) = &prev {
                if *plen > 0 {
                    if let Some(now) = text_covering(&s, *plen) {
                        if now != *ptext {
                            sample = Some(format!("「{ptext}」→「{now}」"));
                        }
                    }
                }
            }
        }
        // 距離用逐格嚴格對齊
        let now_slots = settled_slots(&s);
        let aligned = prev_slots.len() <= now_slots.len()
            && prev_slots
                .iter()
                .zip(now_slots.iter())
                .all(|(a, b)| a.0 == b.0);
        if aligned {
            for (i, (p, q)) in prev_slots.iter().zip(now_slots.iter()).enumerate() {
                if p.1 != q.1 && now_slots.len() - i >= 3 {
                    far += 1;
                }
            }
        }
        prev = Some(settled(&s));
        prev_slots = now_slots;
    }
    (sample, far)
}

/// 基準檔的位置——存在 testdata 旁邊，跟著測資一起進版控。
fn baseline_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("testdata")
        .join("基準.txt")
}

fn load_baseline() -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    let Ok(c) = std::fs::read_to_string(baseline_path()) else {
        return out;
    };
    for line in c.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let mut it = line.split('\t');
        if let (Some(k), Some(v)) = (it.next(), it.next()) {
            if let Ok(n) = v.trim().parse() {
                out.insert(k.to_string(), n);
            }
        }
    }
    out
}

fn save_baseline(per_tag: &BTreeMap<String, Tally>, total: &Tally) {
    let mut s = String::from("# check_all 的基準：每節通過幾句。用 --save 更新。\n");
    s.push_str(&format!("__總計\t{}\n", total.get(Stage::Pass)));
    for (tag, t) in per_tag {
        s.push_str(&format!("{tag}\t{}\n", t.get(Stage::Pass)));
    }
    if let Err(e) = std::fs::write(baseline_path(), s) {
        eprintln!("寫不進基準檔：{e}");
    } else {
        println!("\n已存基準：{}", baseline_path().display());
    }
}

fn main() {
    testdata::load_engine();
    testdata::load_packs_from_args();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let save = args.iter().any(|a| a == "--save");
    // 每一關的例子平常只留 8 筆，--all 印全部（診斷排序／字錯時要看全貌）
    let show_limit = if args.iter().any(|a| a == "--all") {
        usize::MAX
    } else {
        8
    };
    let only_tag = args
        .iter()
        .position(|a| a == "--tag")
        .and_then(|i| args.get(i + 1))
        .cloned();

    let (rows, sections) = testdata::load_with_sections("check_all");
    if rows.is_empty() {
        eprintln!("沒有測資可跑。");
        std::process::exit(1);
    }

    let mut per_tag: BTreeMap<String, Tally> = BTreeMap::new();
    let mut total = Tally::default();
    // 每一關留幾個例子給人看
    let mut samples: BTreeMap<i8, Vec<(String, String, String)>> = BTreeMap::new();
    // 「通過但打字中曾改寫」——不擋人，但要看得見
    let mut rewrote: BTreeMap<String, usize> = BTreeMap::new();
    let mut extra: BTreeMap<String, Extra> = BTreeMap::new();
    let mut per_lang: BTreeMap<&'static str, LangStat> = BTreeMap::new();
    let mut rewrote_total = 0usize;
    let mut rewrote_samples: Vec<(String, String, String)> = Vec::new();

    for row in &rows {
        if let Some(t) = &only_tag {
            if &row.tag != t {
                continue;
            }
        }
        let out = run(row);
        let (stage, note) = (out.stage, out.note);
        per_tag.entry(row.tag.clone()).or_default().add(stage);
        total.add(stage);
        // 併進來的三項：排名、改寫觸及範圍、錯字
        for t in [row.tag.clone(), TOTAL.to_string()] {
            let x = extra.entry(t).or_default();
            if let Some(r) = out.rank {
                x.rank_sum += r;
                x.rank_n += 1;
            }
            x.far += out.rewrite_far;
            x.errs += out.errs;
            x.chars += out.chars;
            x.second += usize::from(out.near_miss);
        }
        for (name, chars, errs, ok) in &out.lang {
            let e = per_lang.entry(name).or_default();
            e.segs += 1;
            e.chars += chars;
            e.errs += errs;
            if *ok {
                e.ok_segs += 1;
            }
        }
        if stage == Stage::Pass {
            if !note.is_empty() {
                *rewrote.entry(row.tag.clone()).or_insert(0) += 1;
                rewrote_total += 1;
                if rewrote_samples.len() < show_limit {
                    rewrote_samples.push((row.tag.clone(), row.want.clone(), note));
                }
            }
        } else {
            let e = samples.entry(stage as i8).or_default();
            if e.len() < show_limit {
                e.push((row.tag.clone(), row.want.clone(), note));
            }
        }
    }

    // ───── 報表：情境 × 階段 ─────
    println!("\n=== 漏斗計分：{} 句 ===\n", total.total);
    println!(
        "  {:<18} {:>4} │ {:>5} {:>5} {:>5} {:>5} │ {:>4} {:>6} {:>6} │ {:>5} {:>7} {:>6}",
        "節",
        "句數",
        "①不合法",
        "②切不出",
        "③排序",
        "④字錯",
        "✓通過",
        "通過率",
        "差一步",
        "排名",
        "改寫",
        "錯字率"
    );
    println!("  {}", "─".repeat(104));
    let row_fmt = |tag: &str, t: &Tally, rw: usize, x: &Extra| {
        // 平均名次：1.00 = 全部第一名
        let rank = if x.rank_n == 0 {
            "—".to_string()
        } else {
            format!("{:.2}", x.rank_sum as f64 / x.rank_n as f64)
        };
        // 改寫：曾改寫句數，超標的加 ⚠
        let rewrite = if x.far > 0 {
            format!("{rw}⚠{}", x.far)
        } else {
            rw.to_string()
        };
        let err_rate = if x.chars == 0 {
            0.0
        } else {
            x.errs as f64 / x.chars as f64 * 100.0
        };
        // 差一步：沒過，但正解就在第 2 名——按一次鍵就拿得到。
        // **不算進通過**，只是讓「還差多少」看得見。
        let second = if x.second == 0 {
            "—".to_string()
        } else {
            format!("+{}", x.second)
        };
        println!(
            "  {:<18} {:>4} │ {:>5} {:>5} {:>5} {:>5} │ {:>4} {:>5.1}% {:>6} │ {:>5} {:>7} {:>5.1}%",
            tag,
            t.total,
            t.get(Stage::Invalid),
            t.get(Stage::NotGenerated),
            t.get(Stage::Misranked),
            t.get(Stage::WrongText),
            t.get(Stage::Pass),
            t.pass_rate(),
            second,
            rank,
            rewrite,
            err_rate
        );
    };
    let empty = Extra::default();
    for (tag, t) in &per_tag {
        row_fmt(
            tag,
            t,
            rewrote.get(tag).copied().unwrap_or(0),
            extra.get(tag).unwrap_or(&empty),
        );
    }
    println!("  {}", "─".repeat(104));
    row_fmt(
        "總計",
        &total,
        rewrote_total,
        extra.get(TOTAL).unwrap_or(&empty),
    );

    // ───── 依語言拆的錯字率 ─────
    //
    // **只算切法正確的句子**——切法都錯了的話「哪一段屬於哪個語言」
    // 本身就不可信，拿它歸戶只會把錯誤算到無辜的語言頭上。
    if !per_lang.is_empty() {
        println!(
            "
  依語言（只算切法正確的段）"
        );
        println!(
            "    {:<10} {:>6} {:>10} {:>8} {:>8}",
            "", "段數", "整段正確", "字元", "錯字率"
        );
        for (name, e) in &per_lang {
            println!(
                "    {:<10} {:>6} {:>6}/{:<5} {:>7} {:>7.1}%",
                name,
                e.segs,
                e.ok_segs,
                e.segs,
                e.chars,
                if e.chars == 0 {
                    0.0
                } else {
                    e.errs as f64 / e.chars as f64 * 100.0
                }
            );
        }
    }

    // ───── 閘門警告 ─────
    let bad = total.get(Stage::BadData);
    let notgen = total.get(Stage::NotGenerated);
    if bad > 0 {
        println!("\n⚠ 有 {bad} 句資料自己矛盾——先跑 check_testdata 修測資，不然是拿錯的尺量引擎");
    }
    if notgen > 0 {
        println!("⚠ 有 {notgen} 句正解切不出來——這是生成問題，排序調得再好也救不了");
    }

    // ───── 每一關的例子 ─────
    for st in [
        Stage::Invalid,
        Stage::NotGenerated,
        Stage::Misranked,
        Stage::WrongText,
    ] {
        let Some(list) = samples.get(&(st as i8)) else {
            continue;
        };
        if list.is_empty() {
            continue;
        }
        println!("\n── {} ──", st.label());
        for (tag, want, note) in list {
            println!("  [{tag}] {want}  {note}");
        }
    }
    if !rewrote_samples.is_empty() {
        println!("\n── ⑤ 通過，但打字中曾改寫（{rewrote_total} 句，不算失敗）──");
        for (tag, want, note) in &rewrote_samples {
            println!("  [{tag}] {want}  {note}");
        }
    }

    // ───── 跟基準比 ─────
    let base = load_baseline();
    if !base.is_empty() && only_tag.is_none() {
        println!("\n=== 跟基準比 ===\n");
        let mut any = false;
        let cur_total = total.get(Stage::Pass);
        if let Some(b) = base.get("__總計") {
            let d = cur_total as i64 - *b as i64;
            println!(
                "  總計    {b} → {cur_total}  {}",
                if d == 0 {
                    "持平".to_string()
                } else {
                    format!("{d:+}")
                }
            );
            any = d != 0;
        }
        for (tag, t) in &per_tag {
            let Some(b) = base.get(tag) else {
                println!("  {tag:<20} （新增的節，基準裡沒有）");
                any = true;
                continue;
            };
            let cur = t.get(Stage::Pass);
            let d = cur as i64 - *b as i64;
            if d != 0 {
                println!("  {tag:<20} {b} → {cur}  {d:+}");
                any = true;
            }
        }
        if !any {
            println!("  （全部持平）");
        }
    } else if base.is_empty() {
        println!("\n（還沒有基準檔，跑 --save 存一份）");
    }

    // ───── 節的說明 ─────
    if only_tag.is_none() && !sections.is_empty() {
        println!("\n=== 各節 ===\n");
        for (tag, s) in &sections {
            let n = per_tag.get(tag).map(|t| t.total).unwrap_or(0);
            println!(
                "  {tag:<20} {n:>4} 句  主測：{:<8} 來源：{}",
                s.focus, s.source
            );
        }
    }

    if save {
        save_baseline(&per_tag, &total);
    }
}
