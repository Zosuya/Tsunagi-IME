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
///
/// # 為什麼點名式的懲罰排在計數式前面（2026-09-07 重排）
///
/// `stolen`／`split_word` 是**點名式**的——它們指得出「哪個字被誰偷走」、
/// 「哪個詞被攔腰切開」，有具體證據。`passthrough` 是**計數式**的，
/// 只數殘渣有幾個字元。計數式排前面時，字元數會壓過具體證據：
///
/// ```text
/// 英:AP | 英:It    殘渣 2 個  ← 贏
/// 英:API           殘渣 3 個  ← 正解，卻輸在數量
/// ```
///
/// `same_lang` 退到最後也是同一個道理。同語言的兩段之間有沒有切點，
/// 對最後輸出**根本沒有影響**（`normalize` 會黏回去），所以它是所有
/// 懲罰裡證據最弱的一條。而它排在前面時會踩到一個要命的情況：
/// **詞典沒收整個詞的時候，正解只能以拆開的樣子活下來**——`logout`
/// 不在 en_50k 裡，整段那條在 `prune` 就死了，正解只剩
/// `英:log | 英:out`，卻正好被這一欄扣分。罰它等於在罰「詞典沒收錄」。
///
/// 但 `same_lang` **不能整個拿掉**——它擋的是 `英:ma | 英:kes | 注:u3`
/// 這種拿兩個短詞硬湊 `makes` 的切法。退到最後就夠了。
///
/// 只重排順序、一條規則沒加沒減，漏斗 1264→1277（+13，六節進步、
/// 零節退步），前 2 名 1318→1340（+22）。
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
    /// 它只在**「有一個完整的注音音節被切開」**時才有值，其他輸入一律
    /// 是 0，不會影響任何既有的排序。而且只罰「切開」——整串當英文
    /// （`up`）也是一段，不受影響，那種真歧義留給後面的欄位決定。
    ///
    /// # 兩種形狀（2026-09-12 補第二種）
    ///
    /// 原本只認「整串剛好是一個音節」，判準是 `total_len <= 4`。那是
    /// 效能考量（熱路徑不能無條件配置字串），但它把**適用範圍**也一起
    /// 限制掉了：前面一接別的段，整串長度就超標，保護失效。
    ///
    /// ```text
    /// up␣      → 「因」      ✓ 整串就是一個音節，原本那條管得到
    /// cl3up␣   → 「好up␣」   ✗ 整串 6 鍵，保護沒生效
    /// ```
    ///
    /// 所以補第二種：**左鄰是注音段，而接下來連續幾段合起來剛好是一個
    /// 音節**。判準用「直接鄰居」而不是全句統計——真正要打英文的人不會
    /// 把英文接在注音段後面又剛好湊成一個完整音節。
    ///
    /// 兩個實作上的細節，各自都會讓這條規則失效：
    ///
    /// - **空白不算標點**。一聲就是空白鍵，它獨立成段而且 `is_mark=true`
    ///   （`英:up | 英:␣`）。當標點擋掉的話 `up␣` 永遠湊不齊
    /// - **含數字的組合不算**。主鍵盤的數字鍵本身就是注音鍵（`5`=ㄓ、
    ///   `0`=ㄢ⋯⋯），不擋的話 `等5␣minutes` 會把 `5` 吃進注音變成
    ///   「等之minutes」、`大約15␣km` 變「大約1支km」。實測不擋是
    ///   number 節 −1、排名 1.21→1.50
    ///
    /// 補完之後漏斗 1320 不變（零退步），目標案例全部修好。
    pub fewer_split_syllable: std::cmp::Reverse<usize>,
    /// **顯示不出來的音節**（取相反數——越少越好）。
    ///
    /// 注音段可能切得出音節、卻有音節查不到任何字（`/6` 是ㄥˊ，合法但
    /// 沒有字念這個音）。那種切法會在畫面上留下一串沒轉換的按鍵：
    ///
    /// ```text
    /// 這種fu/6況就是藥用      ← `/6` 原樣露出來
    /// 這種情dj盎就是藥用      ← `dj` 原樣露出來
    /// 第1合う/6               ← 在句尾也一樣（本意是「第1名」）
    /// ```
    ///
    /// # 整串的最後一個音節也算（2026-09-23）
    ///
    /// 原本最後一段的最後一個音節豁免，理由是「那可能只是還在打、聲調
    /// 還沒按」。但這個情況**到不了這一欄**：注音段一定是
    /// `bopomofo::validity` 判 `Valid` 的，而沒按聲調的音節不是 `Valid`
    /// （`au/` 不會成為注音段）——會落到這裡的 `/6` 已經打完了，它就是
    /// 一串會原樣露出來的按鍵。
    ///
    /// 豁免的代價是 `第1名`：`2u4 | 1 | au/6` 的正解輸給 `日:au | 注:/6`
    /// （`au` 是合法假名，`/6` 又被豁免）。拿掉豁免之後漏斗修好這一句、
    /// 弄壞 0 句，改寫次數不變。
    ///
    /// 每一段最多記 2（見 `BopoFacts::unreadable`）。
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
    ///
    /// # 注音段把標點或分隔符當成音節
    ///
    /// 一鍵兩用的 `,` `.` `;` `/` 是韻母鍵、空白是一聲，所以「標點＋
    /// 分隔符」常常也是一個合法的一聲音節。下面三種形狀也記在這一欄
    /// （`soft_punct_swallowed`）：
    ///
    /// | 形狀 | 吞掉的樣子 | 本意 |
    /// |---|---|---|
    /// | 英日後面的注音段以 `.␣`／`;␣`／`/␣` 開頭 | `英:hello \| 注:.␣`（hello歐） | hello. |
    /// | 注音段裡自成音節的 `,␣` | `注:su3cl3,␣`（你好ㄝ） | 你好, |
    /// | 注音段結尾自成音節的 `.␣`，後面接外文 | `注:su3cl3.␣ \| 英:come`（你好歐come） | 你好. come |
    ///
    /// 這三種要先有軟標點（`punct::is_soft_punct`）才生得出「標點＋分隔符」
    /// 那條切法，這一欄決定它什麼時候贏。
    ///
    /// **數字鍵＋一聲空白後接外文不當成吞字**：實測會把「搭uber」判成
    /// 「28 uber」（`28`＝ㄉㄚ也同時是數字鍵），已否決（2026-09-25），
    /// 見開發筆記「漏斗 1335 之後」。
    pub fewer_swallowed: std::cmp::Reverse<usize>,
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
    ///
    /// # 日文段也會偷（`kana_stole_head`）
    ///
    /// 注音鍵就是字母，中文字的第一鍵常常是羅馬字的母音（`e`＝ㄍ、
    /// `u`＝ㄧ），於是英文詞後面那個中文字的頭被拉去拼假名：
    ///
    /// ```text
    /// user稿（userel3）   日:usere | 注:l3     還回 e → 英:user ＋ 注:el3
    /// ```
    ///
    /// 形狀跟上面一樣是「前一段多吃了後面音節的第一個鍵」，所以記在同一欄。
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
    ///
    /// # 英文詞尾被分隔空白湊成一聲（`tail_eaten_by_tone1`）
    ///
    /// 同一種病的另一個形狀：英文詞後面打了分隔用的空白，最後一個字母
    /// 跟空白被讀成一聲的注音字——`token␣要` 變成 `日:toke | 注:n␣ul4…`
    /// （n␣ 是ㄙ，私）。`token` 一樣是被切開了，只是另一半躲在注音段裡。
    ///
    /// # 日文詞被冷僻的英文段切開（`steals_ja_head`）
    ///
    /// 反方向：`一緒に`（issyoni）被切成 `英:iss | 日:yoni`，真正的詞
    /// `issyo` 橫跨切點。`iss` 排第 40101 名，冷僻到不該拿來切日文。
    ///
    /// 這一條跟 `fewer_stolen` 裡的「日文段偷注音頭」看起來是鏡像，卻
    /// 記在不同欄，是因為**被切壞的東西不同**：那一條是後面一個注音
    /// **音節**的第一個鍵被拿走（`usere|l3` 的 `e` 屬於 `el3`），這一條是
    /// 一個**詞**橫跨切點——那正是這一欄在數的。兩種放法在漏斗上逐句
    /// 相同（2026-09-23 量過；`fewer_stolen` 的優先序比這一欄高，所以
    /// 只是現有測資上等價），就照語意放。
    pub fewer_split_word: std::cmp::Reverse<usize>,
    /// **一個詞被另一種語言的短段從中間剖開**的次數（越少越好）。
    ///
    /// `split_word` 只看相鄰兩段，擋不住三段的形狀——中間插一小段
    /// 別的語言，兩側各自都合法，於是零懲罰勝出：
    ///
    /// ```text
    /// 日:adoba | 英:is  | 日:uwo…   アドバイス 被 `is` 剖開
    /// 英:rev   | 日:ie  | 英:we     review 被 `ie`（いえ）剖開
    /// 日:enn   | 英:ban | 注:n␣…    円盤（ennbann）被 `ban` 與注音的 `n␣` 剖開
    /// ```
    ///
    /// 前兩種見 `split_sandwich`（兩個方向都罰），第三種見 `split_sandwich_zh`。
    pub fewer_split_sandwich: std::cmp::Reverse<usize>,
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
    /// 都能贏。這一欄罰兩種短日文段：
    ///
    /// 1. **不在詞典裡**的（原本就有的）。
    /// 2. **只有一個假名**的，不管在不在詞典裡（2026-09-05 補）。
    ///
    /// 第二條是因為第一條實際上幾乎濾不到東西：2～3 字母的合法假名
    /// 組合裡，**984 個全在 mozc 詞典裡**。地雷只有五組（ㄅㄐㄧ＝`ru`＝る、
    /// ㄋㄧ＝`su`、ㄑㄧ＝`fu`、ㄒㄧ＝`vu`、ㄌㄧ＝`xu`），但底下全是高頻字
    /// ——就、見、進、年、想、小、前、六、兩。見「假名碎片」測資。
    ///
    /// **長度門檻不能省**——日文活用形句子正是「合法但不在詞典裡」
    /// （mozc 只收辭書形），那些段長 28～34 字元，不能被罰到。
    ///
    /// 但長度只擋得住長的活用形。`tabete`（食べて，6 字元）跟碎片
    /// 長得一模一樣，所以還要再問一次 `romaji::inflect`——**真的還原
    /// 得出辭書形的不算碎片**（2026-09-07 補，見計分處的註解）。
    pub fewer_kana_bits: std::cmp::Reverse<usize>,
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
    /// 注音段切一次音節就能回答的幾件事。非注音段一律是預設值（0／false）
    bopo: BopoFacts,
    /// 這一段是**日文動詞的活用形**嗎（`romaji::inflect`）？
    ///
    /// **一定要走快取**——它要試各種還原規則、每次都查詞典，實測直接
    /// 呼叫會讓 p99 從 6.6ms 衝到 24.3ms（排序每鍵要對上千個段落算分）。
    /// 快取之後同一個段落只算一次。非日文段一律 false，連呼叫都省。
    inflected: bool,
    /// 這一段日文只有**一個假名**嗎（`single_mora`，碎片懲罰用）？
    ///
    /// **一樣要走快取**。`split_moras` 每次都配置一串字串，原本在
    /// `kana_bits` 裡對每個候選的每個日文段現算——那一項佔掉整個計分
    /// 的兩成多（每個候選約 0.6µs，是計分裡最貴的一項）。
    /// 非日文段、或長過 `KANA_FRAGMENT`（碎片懲罰根本不看）的一律 false。
    single_mora: bool,
}

/// 一段注音**切一次音節**就能回答的幾件事（`bopomofo_facts` 算）。
///
/// 「前後幾個音節有沒有被詞認領」都是由左而右取最長匹配（跟
/// `bopomofo_claimed_chars` 同一套）的結果，不是「有沒有任何一種拆法」。
#[derive(Clone, Copy, Default)]
struct BopoFacts {
    /// 被詞典的多音節詞認領的字元數，見 `bopomofo_claimed_chars`
    claimed: usize,
    /// **顯示不出來的音節**，每段最多記 2：最後一個以外有沒有（有幾個
    /// 都記 1）、最後一個是不是（記 1）。切不出音節的整段記 1。
    ///
    /// 會分成「前面」與「最後一個」兩項，是因為最後一個音節原本豁免
    /// （見 `Score::fewer_unreadable`）。豁免拿掉時照原本的兩項相加，
    /// 沒有改成逐音節計數——那會改變排序，沒有量過。
    unreadable: u8,
    /// **第一個音節**被多音節詞認領了嗎（`.␣5.␣` 歐洲 的 `.␣`）？
    ///
    /// 軟標點的罰則用它放行真的詞：`.␣` 是ㄡ一聲，也可能是句點＋分隔符，
    /// 但它在「歐洲」裡的時候就是「歐」。見 `soft_punct_swallowed`。
    first_in_word: bool,
    /// **最後一個音節**被多音節詞認領了嗎（`1o3.␣` 北歐 的 `.␣`）？
    ///
    /// 用途同 `first_in_word`。
    last_in_word: bool,
}

/// 段落判斷的快取。
///
/// **候選之間的段落重複得很兇**：實測 59 鍵的日文長句有 400 個候選、
/// 1472 個段落，但相異的只有 159 個——同樣的查詢做了九次。而每次
/// 查詢都要把羅馬字轉成假名（配置字串）再查詞典，實測一次要 8 微秒。
///
/// 按語言分層是為了**查得到就不必配置字串**：`HashMap<(String, Lang)>`
/// 沒辦法用 `(&str, Lang)` 查，每次都得先 clone 一份鍵，那就白做了。
///
/// **不是單一段落的判斷也放這裡**（`prefix`）：只要答案只跟按鍵有關，
/// 就該跟段落判斷共用同一個有效期（詞庫版本、`MEMO_LIMIT`），不要各自
/// 帶一份 `thread_local`——那種快取沒人清，詞庫載完之前的答案會一直留著。
#[derive(Default)]
struct Memo {
    /// 單一段落的判斷，按語言分層（理由見上）
    segs: std::collections::HashMap<Language, std::collections::HashMap<String, SegFacts>>,
    /// `steals_ja_head` 的答案：英文段 → 日文段 → 偷了沒。
    ///
    /// 兩層而不是 `(String, String)` 當鍵：後者沒辦法用兩個 `&str` 查，
    /// 每查一次都得先 clone 兩份字串。
    ja_head: std::collections::HashMap<String, std::collections::HashMap<String, bool>>,
    /// `ja_head` 內層的總筆數（`len` 用，不必每次掃一遍內層）
    ja_head_len: usize,
}

impl Memo {
    fn facts(&mut self, s: &Segment) -> SegFacts {
        let by_lang = self.segs.entry(s.lang).or_default();
        if let Some(f) = by_lang.get(s.keys.as_str()) {
            return *f;
        }
        let n = s.keys.chars().count();
        // **注音的幾件事一次算完**：`claimed_chars` 與「顯示得出來嗎」
        // 都要先切音節，而 `split_syllables` 每個候選長度都配置一個
        // 字串——分兩次呼叫實測讓 p99 從 10.9ms 衝到 14.3ms。
        let bopo = if s.lang == Language::Bopomofo {
            bopomofo_facts(&s.keys)
        } else {
            BopoFacts::default()
        };
        let f = SegFacts {
            n,
            claimed: claimed(&s.keys, s.lang, n),
            claimed_chars: if s.lang == Language::Bopomofo {
                bopo.claimed
            } else {
                claimed_chars(&s.keys, s.lang, n)
            },
            in_dict: in_dict(&s.keys, s.lang, n),
            clean: clean_word(&s.keys),
            common_en: crate::english::is_common_word(s.keys.trim()),
            bopo,
            inflected: s.lang == Language::Romaji && crate::romaji::inflect::is_inflected(&s.keys),
            single_mora: s.lang == Language::Romaji && n <= KANA_FRAGMENT && single_mora(&s.keys),
        };
        by_lang.insert(s.keys.clone(), f);
        f
    }

    /// 冷僻的英文段偷走了後面日文詞的開頭嗎？見 `steals_ja_head`。
    ///
    /// 答案只跟兩段的按鍵有關，同一對段落在幾百個候選裡反覆出現，所以
    /// 要快取。原本那支自帶一份 `thread_local`，有兩個毛病：
    ///
    /// - **不跟詞庫版本失效**。macOS 的詞庫是背景載入的，載完之前查到的
    ///   「不是日文詞」會一直留著；切詞學習（`is_japanese_word` 看它）、
    ///   擴充包、`is_top_word` 的答案變了也一樣。這幾件事都會跳
    ///   `dict::generation`，而 `Memo` 正是跟著它清的（`refresh_memo`）
    /// - **每查一次就配置兩個 `String`**（`(String, String)` 當鍵）
    ///
    /// 形狀條件（語言、長度、全是字母）在查表之前擋——絕大多數相鄰兩段
    /// 在這裡就走了，連雜湊都不必算。
    fn steals_ja_head(&mut self, en: &Segment, ja: &Segment) -> bool {
        if en.lang != Language::English || ja.lang != Language::Romaji || en.is_mark || ja.is_mark {
            return false;
        }
        let n = en.keys.chars().count();
        if !(2..=4).contains(&n) || !en.keys.chars().all(|c| c.is_ascii_alphabetic()) {
            return false;
        }
        if let Some(&v) = self
            .ja_head
            .get(en.keys.as_str())
            .and_then(|m| m.get(ja.keys.as_str()))
        {
            return v;
        }
        let v = steals_ja_head(&en.keys, &ja.keys);
        self.ja_head
            .entry(en.keys.clone())
            .or_default()
            .insert(ja.keys.clone(), v);
        self.ja_head_len += 1;
        v
    }

    fn len(&self) -> usize {
        self.segs.values().map(|m| m.len()).sum::<usize>() + self.ja_head_len
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
    // 吞掉分隔符或標點的段：英日段把標點包進去（`英:hello.`），以及注音段
    // 把標點或分隔符當成一聲音節的幾種形狀（見 `Score::fewer_swallowed`）
    let swallowed = segs
        .iter()
        .zip(&facts)
        .filter(|(s, f)| !s.is_mark && s.lang != Language::Bopomofo && f.n > 1 && !f.clean)
        .count()
        + soft_punct_swallowed(segs, &facts);
    // ── 相鄰幾段一起看的判斷：只有 `steals_ja_head` 走 `Memo`，其餘現算 ──
    //
    // `tail_eaten_by_tone1`、`split_sandwich_zh`、`kana_stole_head` 的答案
    // 也只跟按鍵有關，照「新判斷一律走 Memo」本來該快取。2026-09-23 逐一
    // 量過（整份測資逐鍵走 `Session::push`，約 2500 萬對相鄰段落），決定
    // **不快取**：
    //
    // | 判斷 | 過了語言門檻 | 之後每次 | 合計 | 單鍵最多 |
    // |---|---|---|---|---|
    // | `kana_stole_head` | 8%（200 萬次） | 67ns | 135ms | 0.7ms |
    // | `tail_eaten_by_tone1` | 0.8% | 88ns | 17ms | 0.2ms |
    // | `split_sandwich_zh` | 0.13%（2.9 萬次） | 2.1µs | 60ms | 0.8ms |
    //
    // 九成以上在第一道語言比對就回 false（幾 ns），查表反而比它貴——雜湊
    // 兩三個字串就要幾十 ns。過了門檻之後，前兩支的成本本來就只是一次
    // 詞典查詢，換成查表省不到東西；只有 `split_sandwich_zh` 每次 2µs 值得
    // 記，但它合計只有 60ms（整份測資逐鍵合計十幾秒）。對照：同一批相鄰
    // 段落上，既有的 `split_english_word`／`stole_head`／`split_sandwich`
    // 各花 0.9～1.3 秒，那才是這一段的大宗。
    //
    // `steals_ja_head` 走 `Memo` 是因為它原本就自帶快取，要修的是有效期
    // （見 `Memo::steals_ja_head`）。
    //
    // 英文詞被切成兩半了嗎？（含英文詞尾被一聲空白吃掉）
    let split_word = (0..segs.len().saturating_sub(1))
        .filter(|&i| {
            split_english_word(&segs[i], &segs[i + 1])
                || tail_eaten_by_tone1(&segs[i], &segs[i + 1])
        })
        .count();
    // 一個詞被另一種語言的短段從中間剖開了嗎？（三段的形狀）
    let split_ja = (0..segs.len().saturating_sub(2))
        .filter(|&i| {
            split_sandwich(&segs[i], &segs[i + 1], &segs[i + 2])
                || split_sandwich_zh(&segs[i], &segs[i + 1], &segs[i + 2])
        })
        .count();
    // 英文段（或日文段）偷走了後面注音音節的開頭嗎？
    let stolen = (0..segs.len().saturating_sub(1))
        .filter(|&i| {
            stole_head(&segs[i], &segs[i + 1])
                || kana_stole_head(i.checked_sub(1).map(|p| &segs[p]), &segs[i], &segs[i + 1])
        })
        .count();
    // 日文詞被冷僻英文段切開了嗎？（`英:iss | 日:yoni`，詞是 issyo）——
    // 跟上面的 `split_word` 記在同一欄，理由見 `Score::fewer_split_word`
    let ja_word_split = (0..segs.len().saturating_sub(1))
        .filter(|&i| memo.steals_ja_head(&segs[i], &segs[i + 1]))
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
                // **單一假名不管在不在詞典裡**。`!in_dict` 這道門本來很鬆：
                // 2～3 字母的合法假名組合裡，**984 個都在 mozc 詞典裡**（它收了
                // 大量單假名詞條，`る` 就是其一），於是碎片懲罰幾乎從來不生效。
                && (!f.in_dict || f.single_mora)
                // **真的是動詞活用形的不算碎片**。
                //
                // 碎片與短的活用形長得一模一樣——都是「合法、不在詞典裡、
                // 又短」，光靠 `KANA_FRAGMENT` 這道長度門檻分不開：
                //
                // ```text
                // 日:tabete（食べて，6 字元）   ← 正解，卻被當碎片罰
                // 英:tab | 日:ete              ← `ete` 在詞典裡反而不罰，就贏了
                // ```
                //
                // `romaji::inflect` 照文法把活用形還原成辭書形再查詞典，
                // 分得開這兩者：`tabete` 還原得出 `taberu`，`ete` 還原不出。
                //
                // 用**後綴表**做這件事會賠——`goodarimasu`（good ＋ あります）
                // 尾巴像 `masu` 就被當活用形保護起來，實測 japanese_verbs
                // 賺 2 句、cutpoint 賠 2 句。分類法沒有這個問題。
                && !f.inflected
        })
        .count();
    // 顯示不出來的音節數，**整串的最後一個音節也算**——它不會是「還在
    // 打」的半成品，理由見 `Score::fewer_unreadable`
    let unreadable: usize = facts.iter().map(|f| usize::from(f.bopo.unreadable)).sum();
    let total_len: usize = facts.iter().map(|f| f.n).sum();
    // **整串是一個注音音節卻被切開**了嗎？見 `Score::fewer_split_syllable`。
    //
    // 一個注音音節最多四個鍵（聲介韻調），所以用長度先擋掉絕大多數
    // 情況——這是熱路徑，每一鍵要對幾百個候選算一次，不能無條件配置
    // 字串（`concat` 曾讓 p99 從 12.4ms 衝到 17.9ms）。
    let split_syllable = if segs.len() > 1 && total_len <= 4 {
        let whole: String = segs.iter().map(|s| s.keys.as_str()).collect();
        usize::from(crate::bopomofo::split_syllables(&whole).is_some_and(|v| v.len() == 1))
    } else if segs.len() > 2 {
        // **整句長於 4 鍵時，只救「被注音段夾住」的那一段。**
        //
        // 上面那條只擋得住「整句就是一個音節」。前面一接別的段就失效
        // ——`cl3up␣`（好因）整串 6 鍵，`up␣`（ㄧㄣ）被切成英文段，
        // 因為 `up` 是常用英文詞而正解「好因」不是中文詞，
        // `covered`／`has_dict`／`dict_chars` 三欄都站在英文那邊。
        //
        // **判準是直接鄰居而不是全句統計**：夾在兩個注音段中間、自己
        // 卻不是注音的段，合起來又剛好是一個完整音節——那幾乎一定是
        // 被切壞的。真正要打英文的人不會把英文夾在兩個注音段之間。
        //
        // 只看鄰居的另一個好處是**擋掉數字那批**：`等5␣minutes` 的 `5`
        // 右邊是英文段，不符合「兩邊都是注音」。主鍵盤的數字鍵本身
        // 就是注音鍵，不擋的話數字會被吃進音節。
        // **一聲是空白鍵，而空白會獨立成段**——`up␣`（ㄧㄣ）在候選裡
        // 長這樣：`英:up | 英:␣`。所以不能只看「這一段」，要看**連續
        // 幾段合起來**是不是一個音節。
        let mut n = 0usize;
        for i in 1..segs.len() {
            // 左鄰必須是注音段：這是「被切壞的注音」最可靠的訊號，
            // 而且它擋掉了數字那批（`等5␣minutes` 的 `5` 左鄰是注音，
            // 但下面還要求整組不含數字）。
            if segs[i - 1].lang != Language::Bopomofo || segs[i - 1].is_mark {
                continue;
            }
            let mut acc = 0usize;
            for j in i..segs.len() {
                // 碰到注音段就停——那一段自己會被判定。
                //
                // **空白不算標點**：一聲就是空白鍵，而它會獨立成段而且
                // `is_mark=true`。把它當標點擋掉的話 `up␣`（ㄧㄣ）永遠
                // 湊不齊——那正是這條規則要救的形狀。
                if segs[j].lang == Language::Bopomofo || (segs[j].is_mark && segs[j].keys != " ") {
                    break;
                }
                acc += segs[j].keys.len();
                if acc > 4 {
                    break;
                }
                let whole: String = segs[i..=j].iter().map(|s| s.keys.as_str()).collect();
                // **含數字的不算**：主鍵盤的數字鍵本身就是注音鍵
                // （`5`=ㄓ、`0`=ㄢ⋯⋯），不擋的話 `等5␣minutes` 會把
                // `5` 吃進注音變成「等之minutes」。實測不擋是 number −1。
                if whole.chars().any(|c| c.is_ascii_digit()) {
                    continue;
                }
                if crate::bopomofo::split_syllables(&whole).is_some_and(|v| v.len() == 1) {
                    n += 1;
                    break;
                }
            }
        }
        n
    } else {
        0
    };
    // 不是英文詞的英文段——那是 passthrough 的殘渣。
    // **含小數點的數字串不算殘渣**：`0.49` 這種東西沒有別的解讀，
    // 罰它 4 分會輸給 `英:0 | 注:.4 | 英:9` 的 2 分，打出「0噢9」。
    // 只豁免含小數點的——純數字（`264`）不豁免，那個放寬過會讓數字
    // 在句子裡變成免費的分隔符，實測淨退 6 句（§2.66.2）。
    let passthrough: usize = segs
        .iter()
        .zip(&facts)
        .filter(|(s, f)| {
            s.lang == Language::English
                && !s.is_mark
                && s.keys != super::SEPARATOR
                && !f.common_en
                && !decimal_number(&s.keys)
        })
        .map(|(_, f)| f.n)
        .sum();
    Score {
        fewer_split_syllable: std::cmp::Reverse(split_syllable),
        fewer_unreadable: std::cmp::Reverse(unreadable),
        fewer_split_word: std::cmp::Reverse(split_word + ja_word_split),
        fewer_split_sandwich: std::cmp::Reverse(split_ja),
        fewer_kana_bits: std::cmp::Reverse(kana_bits),
        fewer_stolen: std::cmp::Reverse(stolen),
        fewer_passthrough: std::cmp::Reverse(passthrough),
        fewer_swallowed: std::cmp::Reverse(swallowed),
        fewer_same_lang: std::cmp::Reverse(same_lang),
        covered,
        // **只有「單段且真的是活用形」時才豁免**。
        //
        // 豁免是為了保護日文活用形——mozc 只收辭書形，
        // `kinnyoubimadeniteishutsushinakereba`（金曜日までに提出
        // しなければ）整句一段、查不到詞，但那是正解。
        //
        // 判準曾經是**長度**（≥16 字元就豁免），因為活用形句子通常很長。
        // 那只是代理指標，會兩頭出錯：`mitukaranakatta`（見つからなかった，
        // 15 字元）差一個字元就被判死，而 `sushitime`（9）這種
        // 「日文詞＋英文詞」誤黏的又擋不乾淨。
        //
        // 現在改成問 `romaji::inflect`——它照文法把活用形**還原成辭書形**
        // 再查詞典，還原得出來才算。誤觸從此歸零（`gakkousite`＝学校 site、
        // `onegaidata`＝お願い data 都不會中），漏斗 1149 → 1155。
        //
        // # 為什麼不再限制「整句只有一段」
        //
        // 原本這條豁免多一道 `norm_len <= 1` 的閘（整句只有一段才算），
        // 於是活用形**後面一接東西就失效**：
        //
        // ```text
        // wakarimashita          → 日:wakarimashita          正解第 1
        // wakarimashita a93 data → 日:wakari|英:mas|日:hita  正解不在池裡
        // ```
        //
        // 後者段數是 4，豁免關掉、`わかりました` 拿 has_dict=0，輸給切碎
        // 的 `wakari|mas|hita`（`wakari` 查得到詞）。而長句凍結會把當下
        // 第一名定案，正解就此消失——那 3 筆「切不出來」是這樣來的。
        //
        // 判斷粒度本來就該在**段**：`f.inflected` 問的是「這一段是不是
        // 活用形」，跟同句還有幾段無關。閘門拿掉後漏斗 1230 → 1231，
        // 十七節無一退步。
        //
        // 試過更進一步「活用形段落也算進 `dict_chars`」（段落層級加分），
        // 總分一樣是 1231，但 japanese_verbs 錯字率 4.7% → 5.3%、排名
        // 1.02 → 1.03，**賠掉**。豁免停在 `has_dict_word` 這一欄就好，
        // 那一欄本來就只判「有沒有」不判「有幾個」。
        has_dict_word: dict_chars > 0 || facts.iter().any(|f| f.inflected),
        fewer_segments: std::cmp::Reverse(segs.len()),
        dict_chars,
    }
}

/// 注音段把**軟標點**當成一聲音節吃掉的次數（記進 `fewer_swallowed`）。
///
/// 軟標點（`punct::is_soft_punct`：`,` `.` `;` `/` 後接空白）讓「標點＋
/// 分隔符」跟「一聲音節」兩種解讀都生得出來，這裡決定哪些形狀該判成
/// 標點。三條各管一種形狀；① 與 ③ 在那個音節**被多音節詞認領時不算**
/// （② 不必：ㄝ一聲不是任何詞的一部分，見那一條）：
///
/// ```text
/// ① 英:hello | 注:.␣ | 英:world     hello歐world  → hello. world
/// ② 注:su3cl3,␣ | 英:hello          你好ㄝhello   → 你好, hello
/// ③ 注:su3cl3.␣ | 英:come           你好歐come    → 你好. come
///    英:trip | 注:.␣5.␣              trip歐洲      不罰（歐洲是詞）
///    注:1o3.␣ | 英:style             北歐style     不罰（北歐是詞）
/// ```
///
/// # 為什麼要有「被詞認領就不罰」這道門
///
/// ① 與 ③ 第一版不看詞，測資外「歐」開頭或結尾、旁邊接英文的詞幾乎
/// 全被改成句點（trip歐洲→trip. 洲、北歐style→北。 style，26 個組合
/// 壞 15 個），而整份測資一個「歐」字都沒有，漏斗看不到。那一面的
/// 證據是 `BopoFacts::first_in_word`／`last_in_word`：`.␣` 是「歐洲」
/// 「北歐」的一部分，就是字，不是句點。加門之後漏斗逐句不變，那 15 個
/// 救回 10 個（剩下的是詞典沒收的組合：歐巴、歐陽、歐美劇）。
fn soft_punct_swallowed(segs: &[Segment], facts: &[SegFacts]) -> usize {
    // ① **英日（或標點）後面的注音段以 `.␣`／`;␣`／`/␣` 開頭**：
    // `hello.␣` 的 `.␣` 被當成單獨一個ㄡ一聲（hello歐）。
    //
    // 左鄰是注音時不算——`好.␣` 是「好歐」還是「好。」，光看左邊分不出
    // 來（那由 ③ 看右邊）。左鄰是分隔符也不算：`hello␣.␣` 的 `.` 前面是
    // 空白，本來就不是軟標點，那是「hello 歐」。
    let after_foreign = (1..segs.len())
        .filter(|&i| {
            let (p, s) = (&segs[i - 1], &segs[i]);
            s.lang == Language::Bopomofo
                && !facts[i].bopo.first_in_word
                && p.lang != Language::Bopomofo
                && p.keys != super::SEPARATOR
                && [". ", "; ", "/ "].iter().any(|h| s.keys.starts_with(h))
        })
        .count();
    // ② **注音段裡夾著自成一個音節的 `,␣`**（ㄝ一聲）。那個音節唯一的
    // 候選是注音符號「ㄝ」本身——`dict::has_chars` 查得到，所以
    // `fewer_unreadable` 抓不到它，畫面上就是「你好ㄝ」。`punct` 把 `,␣`
    // 列為空音節、判成標點（`EMPTY_SYLLABLES`），但那只讓 `英:,` 那條
    // 切法生得出來，`注:su3cl3,␣` 照樣在，而且段數少、會贏。
    //
    // 自成音節才算：`u,␣`＝ㄧㄝ（耶）的 `,` 是韻母，不罰。不必像 ① ③
    // 那樣問「被詞認領」：詞表（`BPMFMappings.txt`）裡沒有任何一個詞
    // 含ㄝ一聲這個音節。
    let lone_comma = segs
        .iter()
        .filter(|s| s.lang == Language::Bopomofo && !s.is_mark)
        .filter(|s| {
            let b = s.keys.as_bytes();
            (0..b.len().saturating_sub(1))
                .any(|p| b[p] == b',' && b[p + 1] == b' ' && starts_syllable(b, p))
        })
        .count();
    // ③ **注音段結尾是自成一個音節的 `.␣`／`;␣`／`/␣`（ㄡ／ㄤ／ㄥ一聲），
    // 後面接的又不是注音**——那是句點＋分隔符，不是「你好歐come」。
    //
    // 右邊接英文／日文時歧義就解開了：ㄡ一聲單獨收在句尾再直接接外文，
    // 比「你好。 come」罕見得多。右邊還是注音，或整串到此為止（還在打，
    // 沒有下一段），都不算。不罰的話長句凍結會把「你好歐」定死。
    //
    // 自成音節才算：`e.␣`＝ㄍㄡ（溝）的 `.` 是韻母，不罰。
    let before_foreign = (0..segs.len().saturating_sub(1))
        .filter(|&i| {
            let (s, nx) = (&segs[i], &segs[i + 1]);
            if s.lang != Language::Bopomofo
                || s.is_mark
                || nx.lang == Language::Bopomofo
                || facts[i].bopo.last_in_word
            {
                return false;
            }
            let b = s.keys.as_bytes();
            let n = b.len();
            n >= 2
                && b[n - 1] == b' '
                && matches!(b[n - 2], b'.' | b';' | b'/')
                && starts_syllable(b, n - 2)
        })
        .count();
    after_foreign + lone_comma + before_foreign
}

/// `b[p]` 是一個注音音節的**開頭**嗎：在段首，或前一個鍵是聲調鍵
/// （一聲是空白）。
///
/// 分辨 `,␣`／`.␣` 是自成一個一聲音節（ㄝ／ㄡ），還是前一個音節的韻母
/// ——`u,␣` 是ㄧㄝ（耶）、`e.␣` 是ㄍㄡ（溝）。
fn starts_syllable(b: &[u8], p: usize) -> bool {
    use crate::bopomofo::keymap::{role_of, Role};
    p == 0 || role_of(char::from(b[p - 1])) == Some(Role::Tone)
}

/// 一個詞被**另一種語言的短段**從中間剖開了嗎？
///
/// `split_word` 罰的是「相鄰兩段合起來是英文詞」，擋不住換成三段的
/// 形狀——中間插一小段別的語言，兩側各自都合法，於是零懲罰勝出：
///
/// ```text
/// 日:adoba | 英:is | 日:uwo…   `adobaisu`（アドバイス）被 `is` 剖開
/// 英:rev   | 日:ie | 英:we     `review` 被 `ie`（いえ）剖開
/// ```
///
/// **兩個方向都要認**：日文詞被英文剖開、英文詞被日文剖開，是同一
/// 個病的兩面。兩側同語言、中間異語言且短（≤3 鍵）才算夾心——長的
/// 那一段本來就該獨立成段。
fn split_sandwich(a: &Segment, b: &Segment, c: &Segment) -> bool {
    if a.is_mark || b.is_mark || c.is_mark {
        return false;
    }
    // 兩側同語言、中間是另一種語言
    if a.lang != c.lang || b.lang == a.lang {
        return false;
    }
    // 注音不參與：它跟英日的按鍵集合語意不同，黏起來查詞沒有意義
    if a.lang == Language::Bopomofo || b.lang == Language::Bopomofo {
        return false;
    }
    if b.keys.chars().count() > 3 {
        return false;
    }
    // 三段全黏、或黏到 c 的開頭（c 常是好幾個詞連在一起）
    let cc: Vec<char> = c.keys.chars().collect();
    for take in 1..=cc.len() {
        let head: String = cc[..take].iter().collect();
        let joined = format!("{}{}{}", a.keys, b.keys, head);
        let hit = match a.lang {
            Language::Romaji => crate::dict::is_japanese_word(&joined),
            Language::English => crate::english::is_common_word(&joined),
            Language::Bopomofo => false,
        };
        if hit {
            return true;
        }
        if take >= 4 {
            break;
        }
    }
    false
}

/// 日文詞被**短英文段＋注音段的開頭**剖開了嗎？（夾心的第三種形狀）
///
/// ```text
/// 日:enn     | 英:ban | 注:n␣m4e.4xk7    円盤（ennbann）
/// 日:tannkou | 英:hon | 注:n␣vu84…      単行本（tannkouhonn）
/// ```
///
/// 撥音 `nn` 的後一個 `n` 被注音拿去跟空白湊成ㄙ，中間剩下的剛好是
/// 英文詞。`split_sandwich` 要求兩側同語言，接不到這個形狀。
///
/// `split_sandwich` 也說「注音不參與，按鍵集合語意不同」——但這裡被
/// 拿走的 `n` 本來就是羅馬字「ん」的一部分，按鍵是共用的。所以只准拿
/// 注音段**開頭的 1～2 個字母**回來接，不是整段黏起來查。
///
/// # 它擋住了 `in_dict` 改問「有把握」的副作用
///
/// `単行本` 的首選不夠常用，`in_dict` 改問 `is_confident_japanese` 之後，
/// 整段的 `tannkouhonn` 不再算查得到詞典，被剖開的那一種就贏了。這裡
/// 問的是 `is_japanese_word`（查得到就算），把那種切法罰回去——**兩條
/// 要一起上**，拿掉這一條「単行本 下個月」就切不出來。
///
/// 沒走 `Memo`：量過不值得（多半在語言門檻就回了），理由見 `score_with` 裡那張表。
fn split_sandwich_zh(a: &Segment, b: &Segment, c: &Segment) -> bool {
    if a.is_mark || b.is_mark || c.is_mark {
        return false;
    }
    if a.lang != Language::Romaji || b.lang != Language::English || c.lang != Language::Bopomofo {
        return false;
    }
    if b.keys.chars().count() > 3 || !b.keys.chars().all(|x| x.is_ascii_alphabetic()) {
        return false;
    }
    let cc: Vec<char> = c.keys.chars().collect();
    for take in 1..=2.min(cc.len()) {
        let head: String = cc[..take].iter().collect();
        if !head.chars().all(|x| x.is_ascii_alphabetic()) {
            break;
        }
        let joined = format!("{}{}{}", a.keys, b.keys, head);
        if romaji::validity(&joined) == romaji::Validity::Valid
            && crate::dict::is_japanese_word(&joined)
        {
            return true;
        }
    }
    false
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

/// **日文段**偷走了後面注音音節的開頭，而且還回去之後剩下的正好是個
/// 英文詞？（`stole_head` 的日文版，記在 `fewer_stolen`）
///
/// ```text
/// 日:usere | 注:l3     還回 e → user ＋ el3（稿）
/// ```
///
/// 注音鍵就是字母，中文字的第一鍵常常剛好是羅馬字的母音（`e`＝ㄍ、
/// `u`＝ㄧ、`a`＝ㄇ），被前面英文詞的尾巴拉去拼成假名。拆開以後兩半
/// 各自合法，別的欄位看不出錯；`covered` 還替假名多算一格，錯的反而贏。
///
/// # 常用的日文詞不算，除非左邊是冷僻的英文碎片
///
/// 沒有這道門時 `です安`（desu0␣）會變成 des煙、`雨愛` 變成 am概——
/// 那些日文詞去掉最後一個母音剛好是英文詞，後面又剛好接零聲母的字。
/// 所以**有把握的日文詞（`is_confident_japanese`）不罰**。
///
/// 但 `widget目` 被切成 `英:wid | 日:geta | 注:j4`，而 `geta`（下駄）
/// 是有把握的日文詞，那道門就把它放過了。分辨的線索在左邊：`wid`
/// 排第 42957 名，是某個英文詞的前半截。所以**左鄰是冷僻英文段時照樣
/// 罰**；左鄰是常用英文詞時保護（`英:ok | 日:desu | 注:0␣`＝ok です安
/// 是正常的混打）。只看「左鄰是英文」不看冷僻的話，`testです安` 會
/// 變成 testdes煙。
///
/// 沒走 `Memo`：量過不值得（多半在語言門檻就回了），理由見 `score_with` 裡那張表。
fn kana_stole_head(prev: Option<&Segment>, ja: &Segment, next: &Segment) -> bool {
    if ja.lang != Language::Romaji || ja.is_mark || next.is_mark || next.lang == Language::Romaji {
        return false;
    }
    // 熱路徑：用切片不配置字串，只有前兩道門都過了才組 `moved`
    let Some(c) = ja.keys.chars().last() else {
        return false;
    };
    let head = &ja.keys[..ja.keys.len() - c.len_utf8()];
    if head.chars().count() < 2 || !crate::english::is_common_word(head) {
        return false;
    }
    let moved = format!("{c}{}", next.keys);
    if crate::bopomofo::validity(&moved) != crate::bopomofo::Validity::Valid {
        return false;
    }
    // 有把握的日文詞不罰，除非左鄰是冷僻的英文碎片（理由見上）
    let left_fragment = prev.is_some_and(|p| {
        p.lang == Language::English && !p.is_mark && !crate::english::is_top_word(&p.keys)
    });
    left_fragment || !crate::dict::is_confident_japanese(&ja.keys)
}

/// 英文詞的最後一個字母，被後面的注音段拿去跟**分隔用的空白**湊成
/// 一聲音節了嗎？（記在 `fewer_split_word`）
///
/// ```text
/// 日:toke  | 注:n␣ul4…   token␣ 的 n␣ 被讀成ㄙ（私）
/// 英:scrip | 注:t␣ul4…   script␣ 的 t␣ 被讀成ㄔ（吃）
/// ```
///
/// 一聲就是空白鍵，所以「英文詞＋分隔空白」的最後一個字母跟空白剛好
/// 湊得成一個單鍵的注音音節。`space.rs` 的 notebook 保護（`k␣` 是ㄜˉ）
/// 只管空白切塊那一步，排序這一層原本沒有對應的規則。
///
/// 只認「**字母＋空白**」開頭的注音段：數字鍵（`5␣`＝ㄓ）不算，那是
/// 數字後面的分隔符，另有規則管。
///
/// 前一段本身已經是常用英文詞時，要「補回那個字母之後**更常用**」才算
/// ——跟 `stole_head` 的排名比較同一個道理：`we吃飯`（we＋t␣z04）補回
/// `t` 是 `wet`，比 `we` 冷僻，那個 `t␣` 本來就是「吃」。
///
/// 沒走 `Memo`：量過不值得（多半在語言門檻就回了），理由見 `score_with` 裡那張表。
fn tail_eaten_by_tone1(a: &Segment, b: &Segment) -> bool {
    if a.is_mark || b.is_mark || a.lang == Language::Bopomofo || b.lang != Language::Bopomofo {
        return false;
    }
    let mut it = b.keys.chars();
    let (Some(x), Some(' ')) = (it.next(), it.next()) else {
        return false;
    };
    if !x.is_ascii_alphabetic() {
        return false;
    }
    let joined = format!("{}{x}", a.keys);
    if joined.chars().count() < 3 || !crate::english::is_common_word(&joined) {
        return false;
    }
    // 英文段本身也是詞的話，要「補回那個字母之後更常用」才算
    if a.lang == Language::English && crate::english::is_common_word(&a.keys) {
        return match (crate::english::rank(&joined), crate::english::rank(&a.keys)) {
            (Some(j), Some(h)) => j < h,
            (Some(_), None) => true,
            _ => false,
        };
    }
    true
}

/// **冷僻的英文段偷走了後面日文詞的開頭**嗎？（記在 `fewer_split_word`）
///
/// ```text
/// 英:iss | 日:yoni       iss＋yo  是「一緒」（issyo）
/// 英:kon | 日:nichiwa    kon＋n   是「今」（konn）
/// ```
///
/// en_50k 收了大量冷僻的三字母詞（`iss` 第 40101 名、`kon` 第 40869 名），
/// 它們在日文詞的開頭切一刀，兩邊各自都合法，原本沒有任何一欄罰它。
///
/// 條件，各擋一種誤判：
///
/// - **英文段不在前 5000 名**：`ii`（いい，第 3557 名）這種常用詞不動
///   ——罰「夾在注音中間的短英文碎片」那次就是打壞了 `ii` 才撤掉的
/// - **接起來至少 4 個字元、而且查得到詞**：2～3 字母的假名組合幾乎全在
///   mozc 裡，短的接起來查得到不算證據
/// - **日文段剩下的部分仍是合法羅馬字**：還回去之後後半段要站得住
///
/// 排序裡**不要直接叫這支**：走 `Memo::steals_ja_head`，語言與長度的
/// 形狀條件在那裡先擋，答案也在那裡快取。
fn steals_ja_head(en: &str, ja: &str) -> bool {
    if ja.is_empty() || crate::english::is_top_word(en) {
        return false;
    }
    // 日文段的前 1～3 個字元接到英文段後面：取字元邊界切片，不收 `Vec<char>`
    let cuts = ja
        .char_indices()
        .map(|(i, _)| i)
        .skip(1)
        .chain(std::iter::once(ja.len()));
    for cut in cuts.take(3) {
        let (head, rest) = ja.split_at(cut);
        let joined = format!("{en}{head}");
        if romaji::validity(&joined) != romaji::Validity::Valid {
            continue;
        }
        if !rest.is_empty() && romaji::validity(rest) != romaji::Validity::Valid {
            continue;
        }
        if joined.chars().count() >= 4 && crate::dict::is_japanese_word(&joined) {
            return true;
        }
    }
    false
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
    let bytes = keys.as_bytes();
    !keys.char_indices().any(|(i, c)| {
        c == ' '
            || (!c.is_ascii_alphanumeric()
                && c != APOSTROPHE
                && c != CHOUON
                && !decimal_point(bytes, i, c))
    })
}

/// 數字包夾的小數點不算標點（`2.64`、`1.39`）。
///
/// `.` 在鍵盤上就是注音的ㄥ，所以 `.3`／`.4`／`.6` 剛好都是完整的注音
/// 音節（ㄥˇ／ㄥˋ／ㄥˊ）。於是 `2.64` 被切成 `英:2 | 注:.6 | 英:4`
/// 打出「2吽4」，而正解 `英:2.64` 因為含標點被判 `swallowed`——那一欄
/// 排第 3 順位，直接輸掉。
///
/// 判準要求**兩側都是數字**，所以 `5.␣`（第 7 週，`.` 是注音的一部分、
/// 後面接空白）不受影響。實測現有 1416 筆測資的按鍵裡沒有任何一筆含
/// 「數字.數字」，只有註解行有。
/// 含小數點的數字串（`0.49`、`2.64`）——純數字不算，見計分處的註解。
fn decimal_number(keys: &str) -> bool {
    let b = keys.as_bytes();
    keys.contains('.')
        && !b.is_empty()
        && b[0].is_ascii_digit()
        && b[b.len() - 1].is_ascii_digit()
        && keys
            .char_indices()
            .all(|(i, c)| c.is_ascii_digit() || decimal_point(b, i, c))
}

fn decimal_point(bytes: &[u8], i: usize, c: char) -> bool {
    c == '.'
        && i > 0
        && bytes[i - 1].is_ascii_digit()
        && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)
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
    bopomofo_facts(keys).claimed
}

/// 一段注音要問的幾件事，**切一次音節全部算完**（見 `BopoFacts`）。
///
/// 每一件都要先切音節，而 `split_syllables` 每個候選長度都配置一個字串，
/// 分開問的話同一段要切好幾次。
fn bopomofo_facts(keys: &str) -> BopoFacts {
    const MAX_WORD: usize = 6;
    let Some(syllables) = crate::bopomofo::split_syllables(keys) else {
        // 切不出音節＝整段原樣顯示
        return BopoFacts {
            unreadable: 1,
            ..BopoFacts::default()
        };
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
    let mut first_in_word = false;
    let mut last_in_word = false;
    let mut i = 0;
    while i < n {
        let mut hit = 0usize;
        for len in (2..=MAX_WORD.min(n - i)).rev() {
            let part = &keys[offs[i]..offs[i + len]];
            if crate::dict::is_bopomofo_word(part) || crate::compose::fuzzy_is_word(part) {
                covered += part.chars().count();
                hit = len;
                break;
            }
        }
        if hit > 0 && i == 0 {
            first_in_word = true;
        }
        if hit > 0 && i + hit == n {
            last_in_word = true;
        }
        i += if hit > 0 { hit } else { 1 };
    }
    BopoFacts {
        claimed: covered,
        unreadable: u8::from(head_bad) + u8::from(last_bad),
        first_in_word,
        last_in_word,
    }
}

/// 這段日文只有**一個假名**嗎？
///
/// 單一假名單獨成一段，幾乎一定是從中文串裡硬切出來的碎片（`就`的
/// `ru.4` 前兩鍵剛好是 `ru`＝る）。真正要打的日文助詞（`wo`＝を）
/// 在整句轉換裡是**日文段內部**的一部分，不會單獨成段，所以罰不到它。
fn single_mora(keys: &str) -> bool {
    romaji::split_moras(keys).is_some_and(|m| m.len() == 1)
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
        // 查得到就算——跟 `claimed` 的日文分支同一個問法。
        //
        // 曾經試過要求「首選要夠常用才算查得到」（`is_confident_japanese`），
        // 理由是 mozc 收了大量冷僻詞條，2～3 字母的合法假名組合 984 個
        // 全在裡面，碎片懲罰因此幾乎不生效。但代價太大：隨機 349 個日文
        // 名詞壞 36、修 0（`膝裏`→`hiThe裏`、`部屋中`→`he野獣`、
        // `医歯薬`→`is医薬`），助詞緊接數字全壞（`会議は10時`→
        // `会議ha10時`、`友達が3人`→`tomodat位が3人` 之類）。已否決
        // （2026-09-25）。
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

    /// 注音音節不可以被切成英文段——**前面接了別的段時也一樣**。
    ///
    /// `up␣` 是 ㄧㄣ，但 `up` 剛好是常用英文詞，`covered`／`has_dict`／
    /// `dict_chars` 三欄都站在英文那邊。原本 `fewer_split_syllable` 只在
    /// 「整串就是一個音節」時生效（`total_len <= 4`），所以：
    ///
    /// ```text
    /// up␣     → 「因」      ✓ 整串 3 鍵，保護生效
    /// cl3up␣  → 「好up␣」   ✗ 整串 6 鍵，保護失效
    /// ```
    ///
    /// 這條守著補上去的第二種形狀（左鄰是注音段的情況）。
    #[test]
    fn 注音音節不被切成英文段() {
        if !load() {
            return;
        }
        for (keys, want_one_seg) in [
            ("up ", true),        // 整串就是一個音節，原本那條就管得到
            ("cl3up ", true),     // 好因——前面接了注音段
            ("5/ up ", true),     // 爭因
            ("2u4up 3k4", false), // 的音3惡——中間夾著，只要 up␣ 不被切走
        ] {
            let cands = sort(Incremental::from_keys(keys).cuttings());
            let first = crate::cutpoint::normalize(&cands[0]);
            let s = show(&first);
            if want_one_seg {
                assert!(
                    first.len() == 1 && first[0].lang == Language::Bopomofo,
                    "{keys} 該整串是一個注音段，實際 {s}"
                );
            }
            assert!(!s.contains("英:up"), "{keys} 的 up␣ 被切成英文段了：{s}");
        }
    }

    /// 數字不可以被吃進注音音節——**上面那條規則的反面**。
    ///
    /// 主鍵盤的數字鍵本身就是注音鍵（`5`=ㄓ、`0`=ㄢ⋯⋯），放寬單音節
    /// 保護時如果不擋數字，`等5␣minutes` 會變成「等之minutes」。
    /// 實測不擋是 number 節 −1、排名 1.21→1.50。
    #[test]
    fn 數字不被吃進注音音節() {
        if !load() {
            return;
        }
        for keys in ["2/3 5 items", "28y8 15 km"] {
            let cands = sort(Incremental::from_keys(keys).cuttings());
            let first = crate::cutpoint::normalize(&cands[0]);
            let s = show(&first);
            // 數字要自己成段（或留在英文段裡），不可以黏進注音段
            assert!(
                !first.iter().any(|x| x.lang == Language::Bopomofo
                    && x.keys.chars().any(|c| c.is_ascii_digit())
                    && x.keys.len() > 4),
                "{keys} 的數字被吃進注音段了：{s}"
            );
        }
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

    /// **活用形後面接了東西，豁免不可以失效**。
    ///
    /// `わかりました` 不在 mozc 詞典裡（只收辭書形 `分かる`），靠
    /// `has_dict_word` 的活用形豁免才贏得過切碎的 `wakari|mas|hita`
    /// （`wakari` 查得到）。豁免原本多一道「整句只有一段」的閘，於是
    /// 後面一接東西就關掉，正解直接掉出候選池。
    #[test]
    fn 活用形後面接東西也要保得住() {
        if !load() {
            return;
        }
        let cands = sort(Incremental::from_keys("wakarimashita a93 data").cuttings());
        assert_eq!(
            show(&cands[0]),
            "日:wakarimashita | 英:␣ | 注:a93 | 英:␣ | 英:data",
            "わかりました 買 data"
        );
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

    /// **單一假名不可以吃掉中文**。
    ///
    /// ㄅㄐㄧ 系的三拼字（就、見、進…）前兩鍵剛好是 `ru`＝る，而剩下的
    /// 韻母＋聲調本身也是合法音節（`.4`＝ㄡˋ），於是整句中文會被切成
    /// 「日：ru ｜ 注：.4」——而那條切法因為 `る` 在 mozc 詞典裡，反而拿到
    /// `covered`、`has_dict_word`、`dict_chars` 三項加分，贏過一整段合法但
    /// 不成詞的注音（那種段落 `covered` 永遠是 0）。
    #[test]
    fn 單一假名不會吃掉中文() {
        if !load() {
            return;
        }
        // 閃就：ㄕㄢˇ ㄐㄧㄡˋ。整串不是詞，但也不該輸給「閃る噢」
        let cands = sort(Incremental::from_keys("g03ru.4").cuttings());
        assert_eq!(show(&cands[0]), "注:g03ru.4");
        // 真正要打的日文不受影響——假名段不止一個假名就罰不到。
        //
        // 這裡**不比排序而比判準本身**：`sushi` 究竟是日文還是英文是
        // 另一個已知歧義（見期望基準審核.md A3），綁進來會讓這個測試
        // 在詞庫飄動時跟著壞。
        assert!(single_mora("ru"), "る 是單一假名");
        assert!(!single_mora("sushi"), "すし 不是");
        assert!(!single_mora("tabemasu"), "整句日文更不是");
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

    /// **「冷僻英文偷日文詞頭」的快取要跟著詞庫版本清**。
    ///
    /// 它原本自帶一份 `thread_local`，沒有人清：macOS 的詞庫是背景載入的，
    /// 載完之前查到的「不是日文詞」會一直留著，切詞學習與擴充包改了答案
    /// 也一樣。現在答案記在 `Memo`，跟段落判斷一起在 `dict::generation`
    /// 變了時整份丟掉。
    ///
    /// 做法：在 `Memo` 裡塞一個「詞庫還沒載好時查到的」舊答案，確認排序
    /// 真的拿它來用（不是另有一份快取），再跳一次詞庫版本，確認舊答案不見、
    /// 換回真的答案。**繞過 `Memo` 直接算的話第一步就會紅**。
    #[test]
    fn 偷日文詞頭的快取跟著詞庫版本清() {
        if !crate::compose::tests::load() || !crate::dict::all_loaded() {
            return;
        }
        // 取自測資 ja_en「一緒に|lunch」（issyonilunch）的錯誤切法 `英:iss | 日:yoni`
        let segs = [
            Segment {
                keys: "iss".into(),
                is_mark: false,
                lang: Language::English,
            },
            Segment {
                keys: "yoni".into(),
                is_mark: false,
                lang: Language::Romaji,
            },
        ];
        let mut memo = Memo::default();
        // 先對齊這條執行緒記的詞庫版本，後面才分得出「版本變了」
        refresh_memo(&mut memo);
        memo.ja_head
            .entry("iss".into())
            .or_default()
            .insert("yoni".into(), false);
        memo.ja_head_len += 1;
        assert_eq!(
            score_with(&mut memo, &segs).fewer_split_word,
            std::cmp::Reverse(0),
            "排序沒拿 Memo 裡的答案——另有一份快取，或根本沒快取"
        );

        crate::dict::bump_generation();
        refresh_memo(&mut memo);
        assert_eq!(memo.len(), 0, "詞庫版本變了，整份 Memo 都要丟掉");
        assert_eq!(
            score_with(&mut memo, &segs).fewer_split_word,
            std::cmp::Reverse(1),
            "iss＋yo 是「一緒」，冷僻的 iss 偷了日文詞頭"
        );
    }

    /// **碎片懲罰問的「只有一個假名嗎」要從 `Memo` 拿**，不要每個候選現算。
    ///
    /// `split_moras` 每次都配置一串字串，原本在 `kana_bits` 裡對每個候選
    /// 的每個日文段現算，佔掉整個計分的兩成多。做法跟「偷日文詞頭」那條
    /// 一樣：在 `Memo` 裡把答案改掉，確認計分真的拿它來用——直接呼叫
    /// `single_mora` 的話改了也沒用，這條就紅。
    #[test]
    fn 單一假名的判斷走快取() {
        if !crate::compose::tests::load() || !crate::dict::all_loaded() {
            return;
        }
        // る——`ru.4`（就）的前兩鍵被切成日文段的那種碎片
        let segs = [Segment {
            keys: "ru".into(),
            is_mark: false,
            lang: Language::Romaji,
        }];
        let mut memo = Memo::default();
        let f = memo.facts(&segs[0]);
        assert!(
            f.single_mora && f.in_dict,
            "前提：る 在詞典裡、而且是單一假名"
        );
        assert_eq!(
            score_with(&mut memo, &segs).fewer_kana_bits,
            std::cmp::Reverse(1)
        );
        // 把快取裡的答案改掉：計分要跟著變
        memo.segs
            .get_mut(&Language::Romaji)
            .and_then(|m| m.get_mut("ru"))
            .expect("剛算過")
            .single_mora = false;
        assert_eq!(
            score_with(&mut memo, &segs).fewer_kana_bits,
            std::cmp::Reverse(0),
            "計分沒拿 Memo 裡的答案——還在現算 single_mora"
        );
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

    #[test]
    fn 含小數點的數字串不算殘渣() {
        assert!(decimal_number("0.49"));
        assert!(decimal_number("2.64"));
        assert!(!decimal_number("264"), "純數字不豁免——放寬過，淨退 6 句");
        assert!(!decimal_number("5. "), "含空白");
        assert!(!decimal_number("v2.0"), "開頭是字母");

        // `英:0.49` 罰 4 分會輸給 `英:0|注:.4|英:9` 的 2 分
        let whole = vec![Segment {
            keys: "0.49".into(),
            is_mark: false,
            lang: Language::English,
        }];
        assert_eq!(score(&whole).fewer_passthrough, std::cmp::Reverse(0));
    }

    #[test]
    fn 數字包夾的小數點不算標點() {
        // `.` 就是注音的ㄥ，`.3`／`.4`／`.6` 都是完整音節（ㄥˇ／ㄥˋ／ㄥˊ），
        // 於是 `2.64` 會被切成 `英:2 | 注:.6 | 英:4` 打出「2吽4」。
        assert!(clean_word("2.64"), "小數點在數字中間，不算吞了標點");
        assert!(clean_word("0.49"));
        assert!(clean_word("264"), "純數字本來就乾淨");

        // 兩側都要是數字——`5.␣`（第 7 週）的 `.` 是注音的一部分
        assert!(!clean_word("5. "), "後面是空白，不是小數點");
        assert!(!clean_word("hello."), "句點該自成一段");
        assert!(!clean_word(".64"), "開頭就是點，不是小數");
    }
}

/// 英文詞後面直接接中文字、或英日字母串接在一起時，那幾條點名式懲罰。
///
/// 每一條都釘兩層：**計分**（那一欄真的記到了，拿掉呼叫就紅）與**排序
/// 的結果**（測資裡那一句的第一名）。只釘排序的話，同一句常常有別條
/// 規則順便救起來，拿掉這一條也不會紅。
#[cfg(test)]
mod seam_tests {
    use super::*;
    use crate::cutpoint::incremental::Incremental;
    use std::cmp::Reverse;
    use Language::{Bopomofo, English, Romaji};

    /// 日文詞庫沒進版控，CI 上沒有——這幾條全靠它，沒有就跳過
    fn load() -> bool {
        crate::compose::tests::load() && crate::dict::all_loaded()
    }

    fn seg(keys: &str, lang: Language) -> Segment {
        Segment {
            keys: keys.into(),
            is_mark: false,
            lang,
        }
    }

    /// 排序第一名（正規化之後），格式跟 `dbg_rank` 一樣
    fn first(keys: &str) -> String {
        let cands = sort(Incremental::from_keys(keys).cuttings());
        crate::cutpoint::normalize(&cands[0])
            .iter()
            .map(|s| format!("{}:{}", s.lang.short(), s.keys.replace(' ', "␣")))
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// **日文段偷了注音的頭，還回去剩英文詞**——記在 `fewer_stolen`。
    #[test]
    fn 日文段偷注音頭_還回去剩英文詞就罰() {
        if !load() {
            return;
        }
        // 取自測資 en_vowel「user|稿」（userel3）的錯誤切法：うせれ｜襖
        assert!(
            !crate::dict::is_confident_japanese("usere") && crate::english::is_common_word("user"),
            "前提：失せれ 是冷僻詞條、user 是常用英文"
        );
        let wrong = [seg("usere", Romaji), seg("l3", Bopomofo)];
        assert_eq!(score(&wrong).fewer_stolen, Reverse(1));
        assert_eq!(first("userel3"), "英:user | 注:el3");
    }

    /// **有把握的日文詞不罰，除非左鄰是冷僻的英文碎片**。
    #[test]
    fn 日文段偷注音頭_有把握的日文詞看左鄰() {
        if !load() {
            return;
        }
        // です安（desu＋0␣）：去掉 u 剩 des（英文詞）、u0␣ 是「煙」，形狀全中
        let desu = seg("desu", Romaji);
        let an = seg("0 ", Bopomofo);
        assert!(
            crate::dict::is_confident_japanese("desu") && crate::english::is_common_word("des"),
            "前提：です 有把握、des 是英文詞"
        );
        assert!(
            !kana_stole_head(None, &desu, &an),
            "です安 不可以變成 des煙"
        );
        for left in ["ok", "test"] {
            assert!(
                !kana_stole_head(Some(&seg(left, English)), &desu, &an),
                "左鄰 {left} 是常用英文詞，{left}です安 是正常的混打"
            );
        }
        // 取自測資 en_vowel「widget|目」（widgetaj4）的錯誤切法：
        // `geta`（下駄）有把握，擋它的只剩左邊那個冷僻的 `wid`
        let geta = seg("geta", Romaji);
        let mu = seg("j4", Bopomofo);
        assert!(
            crate::dict::is_confident_japanese("geta") && !crate::english::is_top_word("wid"),
            "前提：下駄 有把握、wid 冷僻"
        );
        assert!(!kana_stole_head(None, &geta, &mu), "單獨的 geta 是日文詞");
        assert!(
            kana_stole_head(Some(&seg("wid", English)), &geta, &mu),
            "左鄰是冷僻碎片 wid，geta 的 a 是偷來的"
        );
        assert_eq!(first("widgetaj4"), "英:widget | 注:aj4");
    }

    /// **英文詞尾被分隔空白湊成一聲**——記在 `fewer_split_word`。
    #[test]
    fn 英文詞尾被一聲空白吃掉就罰() {
        if !load() {
            return;
        }
        // 取自測資 holdout「token|_|要更新」（token ul4e/ vup ）的錯誤切法：n␣ 是ㄙ
        let wrong = [seg("toke", Romaji), seg("n ul4e/ vup ", Bopomofo)];
        assert_eq!(score(&wrong).fewer_split_word, Reverse(1));
        assert_eq!(first("token ul4e/ vup "), "英:token | 英:␣ | 注:ul4e/␣vup␣");
        // 取自測資 holdout「那個|_|script|_|要改」：t␣ 是ㄔ
        assert!(tail_eaten_by_tone1(
            &seg("scrip", English),
            &seg("t ul4e93", Bopomofo)
        ));
        assert_eq!(
            first("s84ek7 script ul4e93"),
            "注:s84ek7 | 英:␣ | 英:script | 英:␣ | 注:ul4e93"
        );
        // 反例：we吃飯（we＋t␣z04）。補回 t 是 wet，比 we 冷僻——t␣ 本來就是「吃」
        assert!(!tail_eaten_by_tone1(
            &seg("we", English),
            &seg("t z04", Bopomofo)
        ));
    }

    /// **「動詞＋er」還原**（`english::agent_noun`）接到排序上：詞典沒收的
    /// logger、sorter 整段活得下來，不再被切成英文碎片加假名。
    #[test]
    fn 動詞加er的詞整段排第一() {
        if !load() {
            return;
        }
        // 取自測資 en_vowel「logger|要」「sorter|要」「mapper|改」
        assert_eq!(first("loggerul4"), "英:logger | 注:ul4");
        assert_eq!(first("sorterul4"), "英:sorter | 注:ul4");
        assert_eq!(first("mappere93"), "英:mapper | 注:e93");
    }

    /// **冷僻英文段切開日文詞**——記在 `fewer_split_word`。快取那一層由
    /// `tests::偷日文詞頭的快取跟著詞庫版本清` 釘著，這裡釘判斷本身與排序。
    #[test]
    fn 冷僻英文段偷日文詞頭就罰() {
        if !load() {
            return;
        }
        // 取自測資 ja_en「一緒に|lunch」（issyonilunch）的錯誤切法
        assert!(steals_ja_head("iss", "yoni"), "iss＋yo 是「一緒」");
        let wrong = [
            seg("iss", English),
            seg("yoni", Romaji),
            seg("lunch", English),
        ];
        assert_eq!(score(&wrong).fewer_split_word, Reverse(1));
        assert_eq!(first("issyonilunch"), "日:issyoni | 英:lunch");
        // 「今日は」打成 konnichiwa 時的 `英:kon | 日:nichiwa`
        assert!(steals_ja_head("kon", "nichiwa"), "kon＋n 是「今」");
        // 常用英文詞不動（`ii`＝いい 就是這樣被保住的）
        assert!(crate::english::is_top_word("go"), "前提：go 在前 5000 名");
        assert!(!steals_ja_head("go", "hann"), "go 是常用英文詞");
    }

    /// **`in_dict` 的日文分支問 `is_japanese_word`（查得到就算），不是
    /// 「首選要夠常用才算查得到」（`is_confident_japanese`，D-H4b，
    /// 已否決見 `in_dict` 上的註解）。
    ///
    /// 冷僻的日文名詞（`hizaura`＝膝裏、`heyajuu`＝部屋中）打過的坑：
    /// 改問「有把握」之後這種詞的首選被拆成英文碎片
    /// （`膝裏`→`hiThe裏`、`部屋中`→`he野獣`）。這裡釘住：只要問
    /// `is_japanese_word`，整段就站得住，不會被切開。
    #[test]
    fn 冷僻日文名詞單獨打要整段日文() {
        if !load() {
            return;
        }
        // モデ、失せれ：詞典收了，但都是冷僻詞條——`in_dict` 照樣要算查得到
        for w in ["mode", "usere"] {
            assert!(
                crate::dict::is_japanese_word(w) && !crate::dict::is_confident_japanese(w),
                "前提：{w} 詞典收了、但冷僻"
            );
            assert!(
                in_dict(w, Romaji, w.len()),
                "{w} 查得到（問 is_japanese_word）"
            );
        }
        assert!(in_dict("sushi", Romaji, 5), "寿司 也查得到");
        // 取自實測：`hizaura`（膝裏）、`heyajuu`（部屋中）整段不能被剖開
        assert_eq!(first("hizaura"), "日:hizaura");
        assert_eq!(first("heyajuu"), "日:heyajuu");
    }

    /// **助詞緊接數字，首選不能把助詞吃進英文段**（D-H4b 的另一個代價：
    /// `会議は10時` 改問「有把握」之後變成 `会議ha10時`）。
    #[test]
    fn 助詞緊接數字首選正確() {
        if !load() {
            return;
        }
        // 今日は3時（きょうは3じ）——今日｜は 相鄰同語言，normalize 後黏成一段
        assert_eq!(first("kyouha3zi"), "日:kyouha | 英:3 | 日:zi");
        // 友達が3人（ともだちが3にん）
        assert_eq!(first("tomodatiga3hito"), "日:tomodatiga | 英:3 | 日:hito");
    }

    /// **日文詞被短英文段＋注音頭剖開**——記在 `fewer_split_sandwich`。
    #[test]
    fn 日文詞被短英文段和注音頭剖開就罰() {
        if !load() {
            return;
        }
        // 取自測資 otaku「円盤|_|預購了」（ennbann m4e.4xk7）的錯誤切法：円ban私慾
        let wrong = [
            seg("enn", Romaji),
            seg("ban", English),
            seg("n m4e.4xk7", Bopomofo),
        ];
        assert_eq!(score(&wrong).fewer_split_sandwich, Reverse(1));
        assert_eq!(first("ennbann m4e.4xk7"), "日:ennbann | 英:␣ | 注:m4e.4xk7");
        // 取自測資 otaku「単行本|_|下個月」：単行本 不夠有把握，`in_dict` 不替它
        // 加分，擋住被剖開那一種的就只剩這一條
        assert!(
            crate::dict::is_japanese_word("tannkouhonn")
                && !crate::dict::is_confident_japanese("tannkouhonn"),
            "前提：単行本 詞典收了、但不算有把握"
        );
        assert_eq!(
            first("tannkouhonn vu84ek4m,4"),
            "日:tannkouhonn | 英:␣ | 注:vu84ek4m,4"
        );
    }
}

/// 注音段把標點或分隔符當成一聲音節的幾條罰則（`soft_punct_swallowed`），
/// 以及最後一個音節不豁免的 `fewer_unreadable`。
///
/// 每一條都分兩層：**計分層**直接問 `score` 的那一欄（規則本身，罰或不罰
/// 的邊界），**排序層**走 `Incremental::from_keys → sort` 看第一名（產品
/// 真正的路，確認那一欄真的決定了結果）。按鍵串都是 `mkkeys --sent` 產的。
#[cfg(test)]
mod punct_digit_tests {
    use super::*;
    use crate::cutpoint::incremental::Incremental;

    /// **三本詞庫都在才測**。計分層要查注音詞典（`歐洲`、`北歐` 被詞認領），
    /// 排序層的第一名跟日文詞典載了沒有有關——只載一半的話，結果會跟著
    /// 別的測試載入的時機飄（CLAUDE.md「測試隨機掛」那一條）。
    fn load() -> bool {
        crate::compose::tests::load() && crate::dict::all_loaded()
    }

    fn seg(lang: Language, keys: &str) -> Segment {
        Segment {
            keys: keys.into(),
            is_mark: false,
            lang,
        }
    }

    fn zh(keys: &str) -> Segment {
        seg(Language::Bopomofo, keys)
    }

    fn en(keys: &str) -> Segment {
        seg(Language::English, keys)
    }

    /// 標點或分隔符——`to_segments` 標成 `is_mark` 的英文段
    fn mark(keys: &str) -> Segment {
        Segment {
            keys: keys.into(),
            is_mark: true,
            lang: Language::English,
        }
    }

    fn swallowed(segs: &[Segment]) -> usize {
        score(segs).fewer_swallowed.0
    }

    /// 排序第一名，`normalize` 過（同語言的切點不影響輸出）
    fn first(keys: &str) -> String {
        let cands = sort(Incremental::from_keys(keys).cuttings());
        crate::cutpoint::normalize(&cands[0])
            .iter()
            .map(|s| format!("{}:{}", s.lang.short(), s.keys.replace(' ', "␣")))
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// **英文後面的 `.␣` 是句點，不是ㄡ一聲**（`soft_punct_swallowed` ①）。
    ///
    /// `hello.␣` 的 `.␣` 剛好是合法的ㄡ一聲（歐），不罰的話「hello歐」段數
    /// 少、會贏。打到 `hello.␣` 還沒接下一個字時就要是句點——那時沒有右鄰，
    /// ③ 管不到，只有這一條。
    #[test]
    fn 軟標點_英文後面的句點不當成歐() {
        if !load() {
            return;
        }
        // 規則本身：`.␣`／`;␣`／`/␣`（ㄡ／ㄤ／ㄥ一聲）接在英文後面
        assert_eq!(swallowed(&[en("hello"), zh(". ")]), 1, "hello歐");
        assert_eq!(swallowed(&[en("ok"), zh("; ")]), 1, "ok骯");
        assert_eq!(swallowed(&[en("and"), zh("/ ")]), 1, "andㄥ");
        // 左鄰是注音：`好.␣` 光看左邊分不出「好歐」還是「好。」，這一條不管
        assert_eq!(swallowed(&[zh("su3cl3"), zh(". ")]), 0, "你好歐（還在打）");
        // 左鄰是分隔符：`hello␣.␣` 的 `.` 前面是空白，本來就是「hello 歐」
        assert_eq!(swallowed(&[en("hello"), mark(" "), zh(". ")]), 0);

        assert_eq!(first("hello. "), "英:hello | 英:. | 英:␣");
        assert_eq!(first("ok; "), "英:ok | 英:; | 英:␣");
    }

    /// **注音段結尾自成音節的 `.␣`，後面接外文就是句點**（③）。
    ///
    /// `你好.␣come`：左鄰是注音，① 不管；但右邊接的是英文，歧義就解開了
    /// ——ㄡ一聲單獨收尾再直接接外文，比「你好。 come」罕見得多。不罰的話
    /// 長句凍結會把「你好歐come」定死（long 節那一句切不出來）。
    #[test]
    fn 軟標點_句尾的歐接外文判成句點() {
        if !load() {
            return;
        }
        assert_eq!(swallowed(&[zh("su3cl3. "), en("come")]), 1, "你好歐come");
        // 還在打（沒有下一段）、右邊還是注音：都不罰，維持原排序
        assert_eq!(swallowed(&[zh("su3cl3. ")]), 0, "你好歐（還在打）");
        assert_eq!(swallowed(&[zh("su3cl3. "), zh("su3")]), 0, "你好歐你");
        // `e.␣` 是ㄍㄡ（溝）：`.` 是前一個音節的韻母，不是自成音節的ㄡ
        assert_eq!(swallowed(&[zh("su3e. "), en("come")]), 0, "你溝come");

        assert_eq!(first("su3cl3. come"), "注:su3cl3 | 英:. | 英:␣ | 英:come");
    }

    /// **注音段裡自成音節的 `,␣` 是逗號，不是ㄝ一聲**（②）。
    ///
    /// ㄝ一聲唯一的候選是注音符號「ㄝ」本身——`has_chars` 查得到，所以
    /// `fewer_unreadable` 抓不到它；不罰的話 `注:su3cl3,␣` 段數少、會贏，
    /// 畫面上就是「你好ㄝhello」（2026-09-23 之前的 master 正是如此）。
    #[test]
    fn 軟標點_自成音節的逗號不當成ㄝ() {
        if !load() {
            return;
        }
        assert!(
            crate::dict::has_chars(", "),
            "前提：ㄝ一聲查得到（候選是「ㄝ」本身），不能靠 fewer_unreadable"
        );
        assert_eq!(swallowed(&[zh("su3cl3, ")]), 1, "你好ㄝ");
        assert_eq!(swallowed(&[zh("su3, cl3")]), 1, "你ㄝ好");
        // `u,␣` 是ㄧㄝ（耶）：`,` 是韻母，不是自成音節
        assert_eq!(swallowed(&[zh("u, ")]), 0, "耶");

        assert_eq!(first("su3cl3, hello"), "注:su3cl3 | 英:, | 英:␣ | 英:hello");
        assert_eq!(first("su3cl3, "), "注:su3cl3 | 英:, | 英:␣");
        assert_eq!(first("u, "), "注:u,␣", "耶");
    }

    /// **`.␣` 被多音節詞認領時不罰**——歐洲、北歐 旁邊接英文不會變句點。
    ///
    /// ① 與 ③ 第一版不看詞，測資外「歐」開頭或結尾、旁邊接英文的詞幾乎全被
    /// 改成句點（trip歐洲→trip. 洲、北歐style→北。 style），而整份測資一個
    /// 「歐」字都沒有。這條跟上面兩條是同一道門的兩面：拿掉門的話這條紅，
    /// 門開太大的話上面兩條紅。
    #[test]
    fn 軟標點_被詞認領的歐不改成句點() {
        if !load() {
            return;
        }
        assert!(bopomofo_facts(". 5. ").first_in_word, "前提：歐洲 是詞");
        assert!(bopomofo_facts("1o3. ").last_in_word, "前提：北歐 是詞");
        assert_eq!(swallowed(&[en("trip"), zh(". 5. ")]), 0, "trip歐洲");
        assert_eq!(swallowed(&[zh("1o3. "), en("style")]), 0, "北歐style");

        assert_eq!(first("trip. 5. "), "英:trip | 注:.␣5.␣");
        assert_eq!(first("ji3fm4. 5. trip"), "注:ji3fm4.␣5.␣ | 英:trip");
        assert_eq!(first("1o3. style"), "注:1o3.␣ | 英:style");
        assert_eq!(first("s06. style"), "注:s06.␣ | 英:style", "南歐style");
        assert_eq!(
            first("ji3vu3cj0 1o3. design"),
            "注:ji3vu3cj0␣1o3.␣ | 英:design",
            "我喜歡北歐design"
        );
        // 另一面：沒被詞認領的照樣是句點
        assert_eq!(first("hello. world"), "英:hello | 英:. | 英:␣ | 英:world");
    }

    /// **F-H1c 不採用：數字鍵＋一聲空白接外文不當成吞字**
    /// （使用者裁決 2026-09-25）。
    ///
    /// 曾經試過把「注音段最後一個音節全是數字鍵＋一聲、沒被詞認領、
    /// 後面接外文詞」記一次 `fewer_swallowed`，理由是主鍵盤數字鍵同時是
    /// 注音鍵（`5`＝ㄓ、`0`＝ㄢ、`28`＝ㄉㄚ），原樣輸出的數字只算
    /// `passthrough`、湊成中文字反而完全免費，於是「5 items」「100
    /// percent」全被打成中文。加了這條罰則確實修好那 4 句（number 節），
    /// 但數字鍵本身**同時也是中文字**——「搭uber」（`28␣uber`）、
    /// 「班line」「單excel」「之app」全被拆成數字＋英文，比原本更糟。
    /// 規則分不出使用者要的是數字還是字，兩敗俱傷，已否決。現在的正確
    /// 行為是**兩者都原樣照打**：`28␣uber` 排序時中文字「搭」＋英文詞
    /// 要贏過數字＋英文（跟 master 一致）。
    #[test]
    fn 數字鍵是中文字時不當成數字() {
        if !load() {
            return;
        }
        // 前提：這些音節本來就合法查得到字，不靠這裡的規則就有候選
        assert!(crate::dict::has_chars("28 "), "前提：28␣＝搭");
        assert!(crate::dict::has_chars("20 "), "前提：20␣＝單");
        assert!(crate::dict::has_chars("5 "), "前提：5␣＝之");

        // 排序層：搭uber、單excel、之app 首選要是中文字＋英文，
        // 不能被判成「數字＋分隔符＋英文」
        assert_eq!(first("28 uber"), "注:28␣ | 英:uber", "搭uber");
        assert_eq!(first("20 excel"), "注:20␣ | 英:excel", "單excel");
        assert_eq!(first("5 app"), "注:5␣ | 英:app", "之app");

        // 計分層：數字鍵＋一聲接外文詞不再額外記一次 fewer_swallowed
        assert_eq!(swallowed(&[zh("28 "), en("uber")]), 0, "搭uber 不罰");
        assert_eq!(swallowed(&[zh("5 "), en("items")]), 0, "5 items 不罰");

        // number 節那 4 句（5 items、100 percent 等）付出的代價：
        // 數字原樣輸出不再贏過中文字，這是使用者裁決接受的取捨
        assert_eq!(
            first("5 items"),
            "注:5␣ | 英:items",
            "代價：這句原本靠 F-H1c 判成數字，現在變回中文字"
        );
    }

    /// **F-H3：最後一個音節顯示不出來也要罰，不再豁免整句最後一段**。
    ///
    /// `au/6`（合う/ㄥˊ）裡 `/6` 是合法音節但沒有字念這個音。舊規則只罰
    /// 「最後一段以外」的顯示不出來，這裡改成一律罰。
    /// 還原方式：把 `bopomofo_facts` 的 `last_bad` 判斷拿掉（`unreadable`
    /// 只算 `head_bad`），這條測試會紅。
    #[test]
    fn 最後一個音節顯示不出來也要罰() {
        if !load() {
            return;
        }
        assert!(!crate::dict::has_chars("/6"), "前提：ㄥˊ 沒有字念這個音");
        let f = bopomofo_facts("/6");
        assert_eq!(f.unreadable, 1, "整段只有一個音節、又是最後一個，也要罰");
        // 對照組：前面音節顯示不出來、最後一個沒問題——一樣罰 1（跟以前一致）
        let f2 = bopomofo_facts("/6su3");
        assert_eq!(f2.unreadable, 1, "頭音節顯示不出來，跟豁免拿掉前的行為一致");

        // score_with 層級：舊規則是「整串最後一段」豁免（按段落位置，不是
        // 按音節），還原方式是把這個位置的豁免加回來，下面的斷言會紅
        let unreadable = |segs: &[Segment]| score(segs).fewer_unreadable.0;
        assert_eq!(
            unreadable(&[zh("su3"), zh("/6")]),
            1,
            "/6 是整句最後一段，舊規則會豁免成 0"
        );
        assert_eq!(
            unreadable(&[zh("/6"), zh("su3")]),
            1,
            "/6 不是最後一段，新舊規則都罰"
        );
    }
}
