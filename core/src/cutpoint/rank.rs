//! 候選切法的排序。
//!
//! 累加式引擎生出的候選是「規則上站得住」的切法，但站得住的很多——
//! 中位 69 種。排序要從裡面挑出最可能是使用者本意的那一種。
//!
//! # 判準：被詞典認領的字元數
//!
//! 每一段去問三種詞典，認出來就把那一段的長度計入分數。
//!
//! ```text
//! su3cl3,（你好，）
//!   注:su3cl3 | 英:,        注音詞典認得「你好」→ 6 分
//!   英:s|英:u|英:3|…        沒有詞典認得        → 0 分
//! ```
//!
//! # 為什麼不是「命中幾個詞」或「命中幾種詞典」
//!
//! 都試過，都不行（2% 上下）。原因是**沒被認領的碎片不扣分**：
//!
//! ```text
//! 正解  注:su3cl3          1 個詞
//! 雜訊  英:s|英:u|英:3c|…  4 個「詞」  ← s、u 之類單字母都在英文詞典裡
//! ```
//!
//! 切得越碎，湊到的「詞」越多。改成算**字元數**之後碎片就有代價了：
//! 認領 6 個字元 vs 認領 0 個。
//!
//! 實測（440 句，一次性窮舉的舊架構）：
//!
//! | 判準 | 第一名正確率 |
//! |---|---|
//! | 命中越多「種」詞典 | 2.7% |
//! | 命中詞總數越多 | 2.3% |
//! | **詞典涵蓋字元數** | **68.0%** |

use super::Segment;
use crate::language::Language;
use crate::romaji;

/// 一種切法的分數。
///
/// 欄位順序就是比較的優先順序（`derive(Ord)` 依序比較）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Score {
    /// **整串就是一個注音音節時，把它切開的次數**（取相反數——越少越好）。
    ///
    /// # 為什麼單音節特別吃虧
    ///
    /// `covered` 對注音**只算多音節詞**（見 `bopomofo_claimed_chars`——
    /// 單音節一律查得到，算進去就失去鑑別力）。所以 `ru8`（ㄐㄧㄚ 家）
    /// 整串的 covered 是 0，而切成 `日:る | 注:8` 反而有分（`ru` 是合法
    /// 假名）。實測 150 個高頻單音節有 7 個被這樣搶走：
    ///
    /// ```text
    /// ㄌㄧㄠˇ 了 → ぅ襖     ㄐㄧㄚ 家 → る啊    ㄋㄧㄢˊ 年 → す雸
    /// ㄐㄧㄥ 經 → る鞥      ㄐㄧㄡˋ 就 → る噢
    /// ```
    ///
    /// # 為什麼可以擺在最前面
    ///
    /// 它**只在「整串剛好是一個合法注音音節」時才有值**，其他輸入一律
    /// 是 0，不會影響任何既有的排序。而且只罰「切開」——整串當英文
    /// （`up`）也是一段，不受影響，那種真歧義留給後面的欄位決定。
    pub fewer_split_syllable: std::cmp::Reverse<usize>,
    /// **顯示不出來的段落數**（取相反數——越少越好）。
    ///
    /// 有些切法會在句子中間留下一串沒轉換的按鍵：
    ///
    /// ```text
    /// 這種fu/6況就是藥用      ← `/6`（ㄥˊ）合法，但沒有字念這個音
    /// 這種情dj盎就是藥用
    /// ```
    ///
    /// 那不是「打到一半」——**最後一段不算**，還在打的尾巴本來就
    /// 還沒成形，罰它會把候選推向日文（那正是 `fewer_split_syllable`
    /// 在修的病）。這一欄只罰**夾在中間**、後面還有正常內容的。
    pub fewer_unreadable: std::cmp::Reverse<usize>,
    /// 把分隔符或標點吞進其他段的**次數**（取相反數——越少越好）。
    ///
    /// 標點與分隔符一律自成一段，被吞進去就是切錯：
    ///
    /// ```text
    /// 好吃|_|買   正 注:cl3t␣ | 英:␣ | 注:a93
    ///             實 注:cl3t␣ | 英:␣a93        ← 分隔符被吞
    /// hello|.     正 英:hello | 英:.
    ///             實 英:hello.                 ← 句點被吞
    /// ```
    ///
    /// 光靠 `clean_word` 不夠——那只讓它「不加分」，但吞掉之後段數
    /// 變少，在同分時反而勝出。這一欄直接罰它。
    pub fewer_swallowed: std::cmp::Reverse<usize>,
    /// 相鄰同語言的切點數（取相反數——越少越好）。
    ///
    /// 同一個語言的兩段之間不該有切點——那是同一個詞列，本來就該
    /// 連在一起。`注:su3 | 注:cl3` 是把「你好」硬拆成兩段。
    ///
    /// **最後一組例外**：最後一段可能是還在打的英文半成品
    /// （`英:check | 英:c` 打到一半），那一刀不算錯。
    ///
    /// 實測 440 句：第一名有 41 個含這種切點，正解只有 6 個。
    pub fewer_same_lang: std::cmp::Reverse<usize>,
    /// **英文 passthrough 的字元數**（取相反數——越少越好）。
    ///
    /// 英文是瀑布的最後一站，收任何字元，所以垃圾段完全不扣分：
    ///
    /// ```text
    /// 注:au/6wu0␣ | 英:ru42k6bringn03
    ///                ↑ 14 個字元糊成一團，但 covered 只算「認領了幾個」
    /// ```
    ///
    /// 前面撈到「明天」(8 分) 就贏了，後面接多少垃圾都免費。
    /// 這一欄給那些「不是英文詞的英文段」記上代價。
    pub fewer_passthrough: std::cmp::Reverse<usize>,
    /// **英文段偷走注音音節開頭**的次數（取相反數——越少越好）。
    ///
    /// 「英文詞 ＋ 一個字母 = 也是英文詞」的組合在 en_50k 裡有 1265 組
    /// （the→them/then/they、for→ford/fork/form、you→your…）。
    /// 於是英文段會多吃一個字元，而剩下的注音殘段往往仍然合法：
    ///
    /// ```text
    /// file ＋ d9␣（開）  →  英:filed | 注:9␣    ← 9␣ 是ㄡˉ，也合法
    /// the  ＋ yl3（早）  →  英:they  | 注:l3
    /// ```
    ///
    /// 判準是**把最後一個字元還給後面之後，兩邊是不是都更好**：
    /// 還回去之後英文段仍是詞、注音段仍合法，那就是偷來的。
    /// `was ＋ cl3` 不會誤觸發，因為 `wa` 不是英文詞。
    pub fewer_stolen: std::cmp::Reverse<usize>,
    /// **英文詞被切成兩半**的次數（取相反數——越少越好）。
    ///
    /// `fewer_stolen` 罰得到「英文段偷注音的頭」，但擋不住換個切法：
    ///
    /// ```text
    /// 英:filed | 注:9␣       stolen=1  被罰了
    /// 日:fi | 英:led | 注:9␣ stolen=0  改由這個勝出
    /// ```
    ///
    /// `fi` 是合法日文（ふぃ）、`led` 是英文詞，兩段各自都說得通，
    /// 但合起來 `filed` 才是真正的那個詞——它被攔腰切開了。
    ///
    /// 判準：相鄰兩段合起來是英文詞，而**至少一段不是英文段**。
    /// 兩段都是英文的話不算（`review|commit` 是兩個詞，不是一個詞
    /// 被切開）。
    pub fewer_split_word: std::cmp::Reverse<usize>,
    /// **短的日文假名碎片**的段數（取相反數——越少越好）。
    ///
    /// 英文有三條懲罰（passthrough／stolen／split_word），日文一條都
    /// 沒有——這是不對稱的來源。`claimed` 對日文放寬成「合法就算」，
    /// 於是任何兩個字母的合法假名都白拿分數：
    ///
    /// ```text
    /// 注:ji3rup␣wu0␣…（我今天…）  covered=0   ← 整段不是「一個詞」
    /// 注:ji3 | 日:ru | 注:p␣wu0␣… covered=2   ← ru 是る，合法
    /// ```
    ///
    /// 中文越長越不可能是單一詞條，`covered` 就越接近 0，任何假名碎片
    /// 都能贏。這一欄專罰「短、合法、但不在詞典裡」的日文段。
    ///
    /// **長度門檻不能省**——日文活用形句子正是「合法但不在詞典裡」
    /// （mozc 只收辭書形），那些段長 28～34 字元，不能被罰到。
    pub fewer_kana_bits: std::cmp::Reverse<usize>,
    /// 被詞典認領的字元數——越多越好。
    pub covered: usize,
    /// **完全沒有任何一段查得到詞典**的切法要排後面（取相反數）。
    ///
    /// A 類失分全是這個模式——`covered` 平手，但一邊有詞、一邊沒有：
    ///
    /// ```text
    /// 日:goodarimasu        cov=11  dict=0    整串合法但不是詞
    /// 英:good | 日:arimasu  cov=11  dict=11   兩段都是詞
    /// ```
    ///
    /// `covered` 對日文放寬成「合法就算」，所以整串吞下去也拿滿分。
    /// 這一欄補上：**至少要有一段真的在詞典裡**。
    ///
    /// 只判「有沒有」不判「有幾個」——判數量的話又會變成「切得越碎
    /// 越好」，那是本專案踩過三次的坑。
    pub has_dict_word: bool,
    /// 真的查到詞典的**字元數**——同樣 covered 時，查得到的優先。
    ///
    /// `covered` 對日文放寬成「合法就算」（活用形不在詞典裡），
    /// 這一欄補回精確度：`sushi` 查得到、`nnyoubi` 查不到。
    ///
    /// **算字元不算段數**——算段數的話「切得越碎、湊到的詞越多」，
    /// 實測第一名會從 78% 掉到 8.6%。這是這個專案第三次踩到同一個坑。
    /// 段數的相反數——段少的優先。
    ///
    /// 同分時的平手判準：`注:su3cl3` 勝過 `注:su3 | 注:cl3`，
    /// 因為後者把一個詞拆成兩個單字。
    pub fewer_segments: std::cmp::Reverse<usize>,
    /// 真的查到詞典的字元數（最低優先度的平手判準）。
    pub dict_chars: usize,
}

/// 這一段**顯示得出來嗎**？
///
/// 注音段可能切得出音節、卻有音節查不到任何字（`/6` 是ㄥˊ，
/// 合法的打字中途狀態，但沒有字念這個音）。那種段落在畫面上就是
/// 一串沒轉換的按鍵：
///
/// ```text
/// 這種fu/6況就是藥用      ← `/6` 原樣露出來
/// 這種情dj盎就是藥用      ← `dj` 原樣露出來
/// ```
///
/// **這不是「打到一半」**——那時整串的最後一段本來就還沒成形，
/// 而這裡講的是**夾在中間**、後面還有正常內容的段落。
///
/// 「短到不可能是一句日文」的字元數。
///
/// 日文段落**合法但不在詞典裡**有兩種可能：一是活用形句子（mozc 只收
/// 辭書形，`teishutsushinakereba` 查不到），那些長 28～34 字元；二是
/// 從中文串裡切出來的假名碎片（`ru`／`au`／`su`），那些只有 2～3 個。
/// 這個門檻把兩者分開——只罰後者。
///
/// 12 是實測掃出來的：8～24 之間結果都在 618～621 之間（720 句），
/// 峰值在 12；超過 20 之後活用形句子開始被誤罰。不是刀鋒上的調參。
const KANA_FRAGMENT: usize = 12;

/// 「長到不可能是一個詞」的字元數。
///
/// 日文活用形句子都很長（28～34 字元），而被誤吞的英文詞組合很短
/// （`sushitime` 9、`onegaidata` 10）。實測 12～24 之間結果完全相同。
const LONG_SENTENCE: usize = 16;

/// 算一種切法的分數。
/// 一個段落算分時要問的幾件事。
///
/// **全都只跟 `(keys, lang)` 有關**——同一個段落在不同候選裡問幾次，
/// 答案都一樣。
#[derive(Clone, Copy)]
struct SegFacts {
    /// 字元數
    n: usize,
    claimed: bool,
    /// 這一段有幾個字元算「被認可」。
    ///
    /// 非注音的維持舊行為（整段認可就是全部、否則 0）。注音改成
    /// **組合式**——見 `bopomofo_claimed_chars`。
    claimed_chars: usize,
    in_dict: bool,
    clean: bool,
    /// 是常見英文詞嗎（前後空白已去掉）
    common_en: bool,
    /// 這一段**除了最後一個音節以外**，有顯示不出來的嗎？見 `renderable`。
    bad_head: bool,
    /// 這一段的**最後一個音節**顯示不出來嗎？
    ///
    /// 跟 `bad_head` 分開，是因為整串的最後一個音節可能只是**還在打**
    /// （聲調還沒按），那不該罰。
    bad_last: bool,
}

/// 段落判斷的快取。
///
/// **候選之間的段落重複得很兇**：實測 59 鍵的日文長句有 400 個候選、
/// 1472 個段落，但相異的只有 159 個——同樣的查詢做了九次。而每次
/// 查詢都要把羅馬字轉成假名（配置字串）再查詞典，實測一次要 8 微秒。
///
/// 按語言分層是為了**查得到就不必配置字串**：`HashMap<(String, Lang)>`
/// 沒辦法用 `(&str, Lang)` 查，每次都得先 clone 一份鍵，那就白做了。
#[derive(Default)]
struct Memo(std::collections::HashMap<Language, std::collections::HashMap<String, SegFacts>>);

impl Memo {
    fn facts(&mut self, s: &Segment) -> SegFacts {
        let by_lang = self.0.entry(s.lang).or_default();
        if let Some(f) = by_lang.get(s.keys.as_str()) {
            return *f;
        }
        let n = s.keys.chars().count();
        // **注音的三件事一次算完**：`claimed_chars` 與「顯示得出來嗎」
        // 都要先切音節，而 `split_syllables` 每個候選長度都配置一個
        // 字串——分兩次呼叫實測讓 p99 從 10.9ms 衝到 14.3ms。
        let (bopo_claimed, bad_head, bad_last) = if s.lang == Language::Bopomofo {
            bopomofo_facts(&s.keys)
        } else {
            (0, false, false)
        };
        let f = SegFacts {
            n,
            claimed: claimed(&s.keys, s.lang, n),
            claimed_chars: if s.lang == Language::Bopomofo {
                bopo_claimed
            } else {
                claimed_chars(&s.keys, s.lang, n)
            },
            in_dict: in_dict(&s.keys, s.lang, n),
            clean: clean_word(&s.keys),
            common_en: crate::english::is_common_word(s.keys.trim()),
            bad_head,
            bad_last,
        };
        by_lang.insert(s.keys.clone(), f);
        f
    }

    fn len(&self) -> usize {
        self.0.values().map(|m| m.len()).sum()
    }
}

/// 快取放到幾筆就整個丟掉重來。
///
/// 沒有上限的話，一直打字會讓它無限長大。整個丟掉而不是逐筆淘汰是
/// 因為**丟掉的代價很低**——重算幾百筆而已，而 LRU 那套的簿記成本
/// 反而可能比省下來的多。
const MEMO_LIMIT: usize = 20_000;

thread_local! {
    /// **跨按鍵共用的段落判斷快取**。
    ///
    /// 每一鍵都要替四百個候選算分，而使用者多打一個字時，前面那些
    /// 段落跟上一鍵幾乎一模一樣——每鍵重建快取等於把同樣的詞典查詢
    /// 重做一遍。實測日文長句：排序從 950ms 掉到剩零頭。
    ///
    /// 快取的內容是**純函式的結果**（給定按鍵與語言，答案永遠一樣），
    /// 所以留著跨按鍵用是安全的，不必跟著輸入清空。
    static MEMO: std::cell::RefCell<Memo> = std::cell::RefCell::new(Memo::default());
    /// 這份快取建立時的詞庫版本，理由同 `Incremental::gen`。
    static MEMO_GEN: std::cell::Cell<u64> = const { std::cell::Cell::new(u64::MAX) };
}

/// 詞庫版本變了（背景載完了）或快取太大就整份丟掉。
fn refresh_memo(memo: &mut Memo) {
    let now = crate::dict::generation();
    let stale = MEMO_GEN.with(|g| {
        let changed = g.get() != now;
        g.set(now);
        changed
    });
    if stale || memo.len() > MEMO_LIMIT {
        *memo = Memo::default();
    }
}

pub fn score(segs: &[Segment]) -> Score {
    MEMO.with(|m| {
        let mut memo = m.borrow_mut();
        refresh_memo(&mut memo);
        score_with(&mut memo, segs)
    })
}

fn score_with(memo: &mut Memo, segs: &[Segment]) -> Score {
    let mut covered = 0usize;
    let mut dict_chars = 0usize;
    // 相鄰同語言的切點數，最後一組不算（那可能是還在打的半成品）
    let same_lang = (0..segs.len().saturating_sub(2))
        .filter(|&i| segs[i].lang == segs[i + 1].lang && !segs[i].is_mark && !segs[i + 1].is_mark)
        .count();
    // 每一段先把要問的都問完（重複的段落只查一次，見 `Memo`）
    let facts: Vec<SegFacts> = segs.iter().map(|s| memo.facts(s)).collect();
    // 吞掉分隔符或標點的段
    let swallowed = segs
        .iter()
        .zip(&facts)
        .filter(|(s, f)| !s.is_mark && s.lang != Language::Bopomofo && f.n > 1 && !f.clean)
        .count();
    // 英文詞被切成兩半了嗎？
    let split_word = (0..segs.len().saturating_sub(1))
        .filter(|&i| split_english_word(&segs[i], &segs[i + 1]))
        .count();
    // 英文段偷走了後面注音音節的開頭嗎？
    let stolen = (0..segs.len().saturating_sub(1))
        .filter(|&i| stole_head(&segs[i], &segs[i + 1]))
        .count();
    for (s, f) in segs.iter().zip(&facts) {
        // 標點與分隔符不參與計分——它們本來就自成一段，不是「詞」
        if s.is_mark {
            continue;
        }
        covered += f.claimed_chars;
        if f.in_dict {
            dict_chars += f.n;
        }
    }
    let kana_bits = segs
        .iter()
        .zip(&facts)
        .filter(|(s, f)| {
            s.lang == Language::Romaji
                && !s.is_mark
                && f.n <= KANA_FRAGMENT
                && f.claimed
                && !f.in_dict
        })
        .count();
    // 顯示不出來的音節數。**整串的最後一個音節不算**——那可能只是
    // 還在打（聲調還沒按）。見 `Score::fewer_unreadable`
    let last_i = segs.len().saturating_sub(1);
    let unreadable: usize = facts
        .iter()
        .enumerate()
        .map(|(i, f)| usize::from(f.bad_head) + usize::from(f.bad_last && i != last_i))
        .sum();
    let norm_len = super::normalize_len(segs);
    let total_len: usize = facts.iter().map(|f| f.n).sum();
    // **整串是一個注音音節卻被切開**了嗎？見 `Score::fewer_split_syllable`。
    //
    // 一個注音音節最多四個鍵（聲介韻調），所以用長度先擋掉絕大多數
    // 情況——這是熱路徑，每一鍵要對幾百個候選算一次，不能無條件配置
    // 字串（`concat` 曾讓 p99 從 12.4ms 衝到 17.9ms）。
    let split_syllable = if segs.len() > 1 && total_len <= 4 {
        let whole: String = segs.iter().map(|s| s.keys.as_str()).collect();
        usize::from(crate::bopomofo::split_syllables(&whole).is_some_and(|v| v.len() == 1))
    } else {
        0
    };
    // 不是英文詞的英文段——那是 passthrough 的殘渣
    let passthrough: usize = segs
        .iter()
        .zip(&facts)
        .filter(|(s, f)| {
            s.lang == Language::English && !s.is_mark && s.keys != super::SEPARATOR && !f.common_en
        })
        .map(|(_, f)| f.n)
        .sum();
    Score {
        fewer_split_syllable: std::cmp::Reverse(split_syllable),
        fewer_unreadable: std::cmp::Reverse(unreadable),
        fewer_split_word: std::cmp::Reverse(split_word),
        fewer_kana_bits: std::cmp::Reverse(kana_bits),
        fewer_stolen: std::cmp::Reverse(stolen),
        fewer_passthrough: std::cmp::Reverse(passthrough),
        fewer_swallowed: std::cmp::Reverse(swallowed),
        fewer_same_lang: std::cmp::Reverse(same_lang),
        covered,
        // **只有「單段且夠長」時才豁免**。
        //
        // 豁免是為了保護日文活用形——mozc 只收辭書形，
        // `kinnyoubimadeniteishutsushinakereba`（金曜日までに提出
        // しなければ，34 字元）整句一段、查不到詞，但那是正解。
        //
        // 但沒有長度限制的話，`sushitime`（9 字元）也被保護了——
        // 它該切成 `日:sushi | 英:time`。加上長度門檻之後
        // 輸出正確從 95.5% 跳到 97.3%，而且 12～24 字元之間
        // 結果完全相同，代表這個分界是真的。
        has_dict_word: dict_chars > 0 || (norm_len <= 1 && total_len >= LONG_SENTENCE),
        fewer_segments: std::cmp::Reverse(segs.len()),
        dict_chars,
    }
}

/// 這相鄰兩段，是不是一個英文詞被切成兩半？
///
/// 條件：合起來是英文詞，而且**不是兩段都是英文**——兩段都是英文的
/// 話那是兩個詞相接（`review|commit`），不是一個詞被切開。
fn split_english_word(a: &Segment, b: &Segment) -> bool {
    if a.is_mark || b.is_mark {
        return false;
    }
    if a.lang == Language::English && b.lang == Language::English {
        return false;
    }
    // 注音跟英文的按鍵集合不相交，黏起來查詞典沒有意義
    if a.lang == Language::Bopomofo || b.lang == Language::Bopomofo {
        return false;
    }
    let joined = format!("{}{}", a.keys, b.keys);
    if crate::english::is_common_word(&joined) {
        return true;
    }
    // **也看日文段的尾巴**——`日:wakarimashitafi | 英:led` 的
    // `fi` 藏在長日文段的結尾，整串 `wakarimashitafiled` 不是詞，
    // 但尾巴 `fi` ＋ `led` 是。長日文段常常是好幾個詞連在一起，
    // 詞典查不到整串，得往回找。
    if a.lang == Language::Romaji && a.keys.chars().count() > b.keys.chars().count() {
        let ac: Vec<char> = a.keys.chars().collect();
        for take in 1..=4.min(ac.len().saturating_sub(1)) {
            let tail: String = ac[ac.len() - take..].iter().collect();
            let joined = format!("{tail}{}", b.keys);
            if crate::english::is_common_word(&joined) {
                return true;
            }
        }
    }
    // **也試「後段少幾個字元」**——日文段常常多吃了注音的頭：
    //
    // ```text
    // 英:but | 日:tona | 注:93   but + tona 不是詞
    //                            but + ton  = button ✓（tona 多吃了 a）
    // 英:head | 日:ere | 注:93   head + er  = header ✓
    // 英:log | 日:geru | 注:l4   log + ger  = logger ✓
    // ```
    //
    // 注音音節以母音鍵開頭時（a93 買、e93 修、u3 已），前面英文詞的
    // 尾巴加上那個母音就變成合法日文，於是日文段把它吃走。
    let bc: Vec<char> = b.keys.chars().collect();
    for drop in 1..=2.min(bc.len().saturating_sub(1)) {
        let head: String = bc[..bc.len() - drop].iter().collect();
        let joined = format!("{}{head}", a.keys);
        if crate::english::is_common_word(&joined) {
            return true;
        }
    }
    false
}

/// `en` 這個英文段，是不是偷走了 `bo` 這個注音段的開頭？
///
/// 條件：把 `en` 的最後一個字元移給 `bo` 之後，
/// **英文段仍是詞、注音段仍合法**——那代表原本那個字元屬於注音。
fn stole_head(en: &Segment, bo: &Segment) -> bool {
    if en.lang != Language::English || bo.lang != Language::Bopomofo || en.is_mark || bo.is_mark {
        return false;
    }
    let mut head = en.keys.clone();
    let Some(c) = head.pop() else { return false };
    let moved = format!("{c}{}", bo.keys);
    if crate::bopomofo::validity(&moved) != crate::bopomofo::Validity::Valid {
        return false;
    }
    // 剩下的頭要嘛整個消失（led→l 偷了 e 之類的碎片），要嘛是**更常用
    // 的**英文詞。
    //
    // 「更常用」這個條件是後來補的，補之前這一條**會誤判正解**：
    //
    // ```text
    // 英:game | 注:ji3（我）   ← 正解，卻被判成「game 偷了 eji3 的 e」
    // ```
    //
    // 因為剩下的 `gam` 剛好也在 en_50k 裡（排 40216）。而 `stolen` 的
    // 優先度高過 `split_word`，正解就被壓到 `日:ga | 英:me | 注:ji3`
    // 後面去了。
    //
    // 加上排名比較之後，原本要抓的案例照樣抓得到——`file`(第 1194 名)
    // 比 `filed`(第 9613 名) 常用、`the`(第 1 名) 比 `they`(第 61 名)
    // 常用，那才是「英文段多吃了一個字元」的形狀；`gam` 比 `game`
    // 冷僻兩個數量級，代表 `game` 本來就是那個詞。
    if head.is_empty() {
        return true;
    }
    if !crate::english::is_common_word(&head) {
        return false;
    }
    match (crate::english::rank(&head), crate::english::rank(&en.keys)) {
        (Some(h), Some(f)) => h < f,
        // 剩下的是詞、原本整段不是 → 那更是偷來的
        (Some(_), None) => true,
        _ => false,
    }
}

/// 這一段乾淨嗎？——不含分隔符也不含標點。
///
/// # 分隔符
///
/// `英:␣update` 把分隔符吞進去了，但 `is_word` 會 trim，查起來跟
/// `update` 一樣命中——於是它跟正解同分，卻因為段數少而勝出。
///
/// # 標點
///
/// 同樣的問題：`英:hello.` 的句點被 trim 掉，`hello.` 就算命中，
/// 還比正解的 `英:hello | 英:.` 多賺一個字元。標點該自成一段。
///
/// 注音不套這條——一聲的空白在音節內部（`vm␣ul4`），而注音走的是
/// 完整按鍵串比對，不 trim。
fn clean_word(keys: &str) -> bool {
    // 英文詞裡的撇號（don't、it's）不算標點
    const APOSTROPHE: char = '\u{27}';
    // 日文的長音符號也不算——`fo-ku`（ふぉーく）整串是詞典裡的詞，
    // 把 `-` 當標點的話它拿 0 分，於是輸給 `日:fo | 英:-ku`。
    const CHOUON: char = '-';
    !keys
        .chars()
        .any(|c| c == ' ' || (!c.is_ascii_alphanumeric() && c != APOSTROPHE && c != CHOUON))
}

/// 這一段有詞典認領嗎？
/// 這種切法裡，某個引擎**認可**了幾個字元。
///
/// 跟「這一段標成什麼語言」是兩回事——標成注音不代表那真的是詞。
/// `claimed` 才是引擎自己的認可：注音要查得到詞、日文要是合法的羅馬字
/// 串、英文要是常見詞。
///
/// 挑「某個語言的代表切法」時要看這個。看原始按鍵數的話會挑到「按鍵
/// 多但都不是詞」的垃圾切法——實測 `check u vu84` 的中文代表會變成
/// 「ちぇ喝一下」而不是「check 一下」。
pub fn covered_by(segs: &[Segment], lang: Language) -> usize {
    segs.iter()
        .filter(|s| !s.is_mark && s.lang == lang)
        // **用 `claimed_chars` 不是 `claimed`**。後者問「整段是不是一個
        // 詞」，一整句中文當然不是，於是**一個長中文段拿 0 分**——反而
        // 輸給「被切得很碎、但每小塊剛好是個詞」的切法。
        //
        // 實測症狀：`這種情況就要用切法的` 整段拿 0，而把 `這種` 單獨
        // 切出來的那一種拿 7，於是（中）代表變成
        // `這種fu/6況る噢藥用切法的`。
        .map(|s| claimed_chars(&s.keys, lang, s.keys.chars().count()))
        .sum()
}

/// 這一段有幾個字元算「被認可」。
///
/// # 為什麼注音要用組合式
///
/// `claimed` 問的是「整段是不是一個詞」。對日文與英文那是對的——
/// 它們的段落就是一個詞。但**注音段不是**：切點引擎切的是語言不是詞
/// （見 §2.7），一個注音段可以是一整句中文，那當然不會是單一詞條。
///
/// 後果是中文越長越必輸：
///
/// ```text
/// 注:ji3rup␣wu0␣…（我今天早上去公司開會）  整段是詞？否 → covered=0
/// 注:ji3 | 日:ru | 注:p␣wu0␣…              ru 是合法假名   → covered=2
/// ```
///
/// 任何兩個字母的合法假名都能贏過一整句中文。改成問「這一段有多少
/// 字元能被詞典交代」——用最長匹配拆成詞，加總命中的部分。
fn claimed_chars(keys: &str, lang: Language, n: usize) -> usize {
    if lang != Language::Bopomofo {
        return if claimed(keys, lang, n) { n } else { 0 };
    }
    bopomofo_claimed_chars(keys)
}

/// 一段注音裡，有幾個字元屬於詞典查得到的詞。
///
/// 由左而右取最長匹配——跟選詞層 `compose::apply_word_context` 同一套
/// 做法，兩邊看到的「這段能組出什麼」才會一致。
///
/// 只算**多音節詞**：單音節一律查得到（每個合法音節都有同音字），
/// 算進去的話 `covered` 就等於段長，失去鑑別力。
fn bopomofo_claimed_chars(keys: &str) -> usize {
    bopomofo_facts(keys).0
}

/// 一段注音的三件事，**切一次音節全部算完**。
///
/// 回傳 `(被詞典認領的字元數, 前面有顯示不出來的, 最後一個顯示不出來)`。
///
/// 後兩者分開，是因為**整串的最後一個音節可能只是還在打**（聲調還沒
/// 按），那不該罰；夾在中間的才是「這條切法會在畫面上留下一串沒轉換
/// 的按鍵」。見 `Score::fewer_unreadable`。
fn bopomofo_facts(keys: &str) -> (usize, bool, bool) {
    const MAX_WORD: usize = 6;
    let Some(syllables) = crate::bopomofo::split_syllables(keys) else {
        // 切不出音節＝整段原樣顯示
        return (0, true, false);
    };
    let last_bad = syllables.last().is_some_and(|s| !crate::dict::has_chars(s));
    let head_bad = syllables
        .iter()
        .rev()
        .skip(1)
        .any(|s| !crate::dict::has_chars(s));
    // **改切原字串，不接字串**——音節接起來就是原本的 keys，所以每個
    // 音節的起訖直接算得出來。每鍵要查幾百個候選，`concat()` 一次一個
    // 配置，實測 p99 會從 12.4ms 衝到 17.9ms（預算 16ms）。
    let mut offs = Vec::with_capacity(syllables.len() + 1);
    offs.push(0usize);
    let mut acc = 0usize;
    for syl in &syllables {
        acc += syl.len();
        offs.push(acc);
    }
    debug_assert_eq!(acc, keys.len(), "音節接起來該等於原字串");
    let n = syllables.len();
    let mut covered = 0usize;
    let mut i = 0;
    while i < n {
        let mut hit = 0usize;
        for len in (2..=MAX_WORD.min(n - i)).rev() {
            let part = &keys[offs[i]..offs[i + len]];
            if crate::dict::is_bopomofo_word(part) {
                covered += part.chars().count();
                hit = len;
                break;
            }
        }
        i += if hit > 0 { hit } else { 1 };
    }
    (covered, head_bad, last_bad)
}

fn claimed(keys: &str, lang: Language, n: usize) -> bool {
    // 含分隔符或標點的段不算命中，見 `clean_word`。
    if lang != Language::Bopomofo && !clean_word(keys) {
        return false;
    }
    match lang {
        Language::Bopomofo => crate::dict::is_bopomofo_word(keys),
        // **合法就算命中，查到詞典再加碼**（見 `score` 的 `dict_hits`）。
        //
        // 不能要求「一定要在詞典裡」——mozc 的詞典存的是辭書形，
        // 活用形不在裡面：`teishutsushinakereba`（提出しなければ）
        // 查不到。要求進詞典的話 mixed_japanese_verbs 會從 100% 掉到 0%。
        Language::Romaji => n >= 2 && romaji::validity(keys) == romaji::Validity::Valid,
        // **單字母不算命中英文詞**——a/i/s/u 之類都在詞典裡，
        // 不排除的話「切得越碎分越高」，正解永遠贏不了。
        Language::English => n >= 2 && crate::english::is_common_word(keys.trim()),
    }
}

/// 這一段真的在詞典裡嗎？（比 `claimed` 嚴格，日文也要查詞典）
fn in_dict(keys: &str, lang: Language, n: usize) -> bool {
    if lang != Language::Bopomofo && !clean_word(keys) {
        return false;
    }
    match lang {
        Language::Bopomofo => crate::dict::is_bopomofo_word(keys),
        Language::Romaji => crate::dict::is_japanese_word(keys),
        Language::English => n >= 2 && crate::english::is_common_word(keys.trim()),
    }
}

/// 把候選依分數排序，高分在前。
pub fn sort(cands: Vec<Vec<Segment>>) -> Vec<Vec<Segment>> {
    // **先算好分數再排**（decorate-sort-undecorate）。
    //
    // `sort_by_key` 每次比較都會重算 key，也就是 O(n log n) 次 `score()`
    // 而不是 n 次。而 `score()` 很貴——每一段都要查三本詞典。
    //
    // 實測日文 48 鍵：排序佔了每鍵耗時的 85.6%（2720ms／3176ms）。
    // **共用同一份快取**——不只這一批候選之間，連跨按鍵也共用
    let mut scored: Vec<(Score, Vec<Segment>)> = MEMO.with(|m| {
        let mut memo = m.borrow_mut();
        refresh_memo(&mut memo);
        cands
            .into_iter()
            .map(|c| (score_with(&mut memo, &c), c))
            .collect()
    });
    // clippy 會建議改用 `sort_by_key`——那正是這裡要避開的寫法，
    // 它每次比較都重算 key。分數已經算好在 tuple 裡了。
    #[allow(clippy::unnecessary_sort_by)]
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().map(|(_, c)| c).collect()
}

/// 使用者對**這一整串按鍵**表態過的話，把那一種切法提到第一名。
///
/// # 為什麼不在 `sort` 裡面做
///
/// `sort` 之後還有 `normalize`（把相鄰同語言的段合併）。學習記下來的
/// 是使用者在選單上看到的那一種，也就是**正規化之後**的位置——在
/// `sort` 裡比對會拿正規化前的位置去對正規化後的紀錄，只有剛好兩者
/// 相同的才命中。這個坑第一版踩到了：14 句可修的只修好 6 句。
///
/// 所以呼叫點在 `input.rs` 組出最終清單之後。
///
/// # 為什麼要有整串這一層
///
/// 段落層級（`語:footer` → 英文）會推廣到別的句子，但它分不出
/// `serve` 與 `server`——兩邊都是英文詞，「這是不是詞」問不出差別。
/// 整串層級不推廣，可是吃得下這種難例。見開發文件 §2.26.2。
///
/// **沒學過切詞就一次雜湊查詢都不做**（`cut_any` 那道原子旗標）。
pub fn promote_learned_cut(mut cands: Vec<Vec<Segment>>) -> Vec<Vec<Segment>> {
    if !crate::learn::cut_any() || cands.len() < 2 {
        return cands;
    }
    let keys: String = cands[0].iter().map(|s| s.keys.as_str()).collect();
    let learned = crate::learn::cutting();
    let Some(want) = learned.cut_of(&keys) else {
        return cands;
    };
    // 切法只存切點位置，這裡也用位置比對——語言是事後由 `lang_of`
    // 對每一段各問一次得出的，存不進來也不必存
    let positions = |c: &Vec<Segment>| -> Vec<usize> {
        let mut out = Vec::new();
        let mut at = 0usize;
        for s in c.iter() {
            if at > 0 {
                out.push(at);
            }
            at += s.keys.chars().count();
        }
        out
    };
    if let Some(i) = cands.iter().position(|c| positions(c) == want) {
        let hit = cands.remove(i);
        cands.insert(0, hit);
    }
    cands
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cutpoint::incremental::Incremental;

    fn load() -> bool {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data");
        crate::english::load(&data);
        crate::dict::load_bopomofo(&data).is_some_and(|d| !d.is_empty())
    }

    fn show(segs: &[Segment]) -> String {
        segs.iter()
            .map(|s| format!("{}:{}", s.lang.short(), s.keys.replace(' ', "␣")))
            .collect::<Vec<_>>()
            .join(" | ")
    }

    #[test]
    fn 長中文的認可字數是組合出來的() {
        if !load() {
            return;
        }
        // 整句不是「一個詞」，但裡面的詞要被算到
        let whole = "ji3rup wu0 yl3g;4fm4ej/ n d9 cjo4"; // 我今天早上去公司開會
        assert!(
            !crate::dict::is_bopomofo_word(whole),
            "整句本來就不會是單一詞條——這正是舊寫法給 0 分的原因"
        );
        assert!(
            bopomofo_claimed_chars(whole) > 0,
            "組合式該認得出裡面的「今天」「早上」「公司」"
        );
        // 兩個字的詞整段都算
        assert_eq!(bopomofo_claimed_chars("su3cl3"), 6, "你好 = 6 個按鍵");
        // 湊不出詞的就是 0
        assert_eq!(
            bopomofo_claimed_chars("su3"),
            0,
            "單音節不算——每個合法音節都有字"
        );
    }

    #[test]
    fn 短假名碎片會被罰而活用形不會() {
        if !load() {
            return;
        }
        let facts = |keys: &str| {
            let n = keys.chars().count();
            (
                claimed(keys, Language::Romaji, n),
                in_dict(keys, Language::Romaji, n),
                n,
            )
        };
        // ru（る）合法、而且 mozc 真的收了它——所以「不在詞典裡」抓不到它，
        // 這條規則靠的是長度
        let (c, _d, n) = facts("ru");
        assert!(c && n <= KANA_FRAGMENT, "ru 是短碎片");
        // 活用形句子：合法、不在詞典裡，但夠長，不該被罰
        let long = "teishutsushinakereba";
        let (c2, d2, n2) = facts(long);
        assert!(c2 && !d2, "活用形合法但不在 mozc 詞典裡");
        assert!(n2 > KANA_FRAGMENT, "活用形句子要在門檻之上（{n2} 字元）");
    }

    #[test]
    fn 純注音排第一() {
        if !load() {
            eprintln!("詞庫未下載，跳過（跑 data/download.ps1）");
            return;
        }
        // 你好 = su3cl3
        let cands = sort(Incremental::from_keys("su3cl3").cuttings());
        assert_eq!(show(&cands[0]), "注:su3cl3");
    }

    /// **`stole_head` 不可以誤判正解**。
    ///
    /// `game` ＋ `ji3`（我）：把 `e` 還給後面之後 `eji3` 是合法的ㄍㄨㄛˇ，
    /// 而剩下的 `gam` 剛好也在 en_50k 裡（排 40216）——舊的判準到這裡
    /// 就成立了，於是正解被當成「偷」而扣分，輸給 `日:ga | 英:me | 注:ji3`。
    ///
    /// 加上「剩下的要更常用」之後就分得開了：`gam` 比 `game`(第 456 名)
    /// 冷僻兩個數量級，代表 `game` 本來就是那個詞。
    #[test]
    fn 偷頭不可以誤判正解() {
        if !load() {
            return;
        }
        let cands = sort(Incremental::from_keys("gameji3").cuttings());
        assert_eq!(show(&cands[0]), "英:game | 注:ji3");
    }

    /// 原本要抓的「偷頭」照樣抓得到——`the` 比 `they` 常用。
    #[test]
    fn 偷頭仍然抓得到() {
        if !load() {
            return;
        }
        let a = Segment {
            keys: "they".into(),
            is_mark: false,
            lang: Language::English,
        };
        let b = Segment {
            keys: "l3".into(),
            is_mark: false,
            lang: Language::Bopomofo,
        };
        assert!(stole_head(&a, &b), "they 偷了 yl3（早）的 y");
        // 反例：game 沒有偷 eji3 的 e
        let a = Segment {
            keys: "game".into(),
            is_mark: false,
            lang: Language::English,
        };
        let b = Segment {
            keys: "ji3".into(),
            is_mark: false,
            lang: Language::Bopomofo,
        };
        assert!(!stole_head(&a, &b), "game 本來就是那個詞，不是偷來的");
    }

    #[test]
    fn 碎片分數低() {
        if !load() {
            return;
        }
        let whole = vec![Segment {
            keys: "su3cl3".into(),
            is_mark: false,
            lang: Language::Bopomofo,
        }];
        let bits: Vec<Segment> = "su3cl3"
            .chars()
            .map(|c| Segment {
                keys: c.to_string(),
                is_mark: false,
                lang: Language::English,
            })
            .collect();
        assert!(score(&whole) > score(&bits), "整段要勝過碎片");
    }

    #[test]
    fn 段少的贏() {
        if !load() {
            return;
        }
        // 同樣認領 0 個字元時，段少的優先。
        // 用查不到的字串，確保兩邊 covered 都是 0。
        let two = vec![
            Segment {
                keys: "zzx".into(),
                is_mark: false,
                lang: Language::English,
            },
            Segment {
                keys: "qqw".into(),
                is_mark: false,
                lang: Language::English,
            },
        ];
        let one = vec![Segment {
            keys: "zzxqqw".into(),
            is_mark: false,
            lang: Language::English,
        }];
        assert_eq!(score(&one).covered, 0);
        assert_eq!(score(&two).covered, 0);
        assert!(score(&one) > score(&two), "同分時段少的贏");
    }

    #[test]
    fn 標點不參與計分() {
        let with_mark = vec![
            Segment {
                keys: "su3cl3".into(),
                is_mark: false,
                lang: Language::Bopomofo,
            },
            Segment {
                keys: ",".into(),
                is_mark: true,
                lang: Language::English,
            },
        ];
        // 標點那一段不該加分也不該扣分
        assert_eq!(score(&with_mark).covered, score(&with_mark).covered);
    }
}
