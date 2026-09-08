//! 選詞模組：把切法變成文字。
//!
//! 切點引擎的職責到「哪一段是什麼語言」為止（`cutpoint`），這一層
//! 負責「那一段該顯示什麼字」。
//!
//! # 以字為單位，用詞去修正
//!
//! 使用者定的規則：
//!
//! > 以字為單位。如果使用者選第一個字之後，詞庫有符合的詞，可以改變
//! > 第二個字——例如打「擬郝」，使用者選了「你」，「郝」就自己變成「好」。
//!
//! 所以每個注音音節各自是一個選字位置，但**選過之後會回頭查詞**：
//!
//! ```text
//! su3cl3  →  [擬][郝]        ← 各自的字頻第一名
//!            使用者選「你」
//!         →  [你][好]        ← 詞庫有「你好」，第二個字跟著改
//! ```
//!
//! 這跟新注音的行為一致——選字會帶動後面，不必逐字選完。
//!
//! # 英文段幾乎不參與選字
//!
//! 英文段就是它自己（`check` 顯示 `check`），沒有同音字的問題，
//! 選字時直接跳過。**唯一的例外是日文詞典也收的那些**（`ii`→いい），
//! 理由見 `compose_with_bounds` 裡英文那一支的註解。

use crate::cutpoint::Segment;
use crate::language::Language;

/// 一個可以選字的位置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    /// 這一格的按鍵（一個注音音節，或整段英文／日文）。
    pub keys: String,
    /// 這一格目前顯示的文字。
    pub text: String,
    /// 這一格是哪個語言。
    pub lang: Language,
    /// 這一格能不能選字？英文段原則上不能（日文詞典也收的除外）。
    pub selectable: bool,
    /// 這一格是標點嗎？
    ///
    /// 標點格跟語言格的差別在**它不查詞庫**，但仍然可以有候選——
    /// `[` 能選 `「『【〔`。`lang` 分不出這件事（標點的 lang 是英文），
    /// 所以要獨立一欄。
    pub is_mark: bool,
    /// 這一格的候選是**算好的**嗎？
    ///
    /// 一般的格子靠 `keys` 去查（同音字、假名表記），但符號格
    /// （`\星\` 合併成的那一格）的名字在合併時就沒了——`keys` 是
    /// `u/␣\`，從那裡回推「星」要重跑一次選詞。算好放著最誠實。
    pub cands: Option<Vec<String>>,
    /// 這個字是**使用者手動選的**嗎？
    ///
    /// 手動選過的字不可以被詞庫或重算覆蓋掉——使用者已經表態了，
    /// 引擎再自作聰明改回去是最惱人的行為。見 `apply_word_context`。
    pub picked: bool,
}

/// 使用者手動調整過的**日文詞界**。
///
/// Viterbi 只能給「詞典查得到的」分法，遇到詞典沒收的專有名詞時
/// 再怎麼選字都拼不出來——所以要能自己把詞界拉開。
/// 見 `romaji::convert::convert_with`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JpBounds {
    /// 這是哪一段日文（用它的**按鍵**認）。
    ///
    /// 按鍵變了就代表使用者又打了字，那時這份調整已經過期——
    /// 跟 `Session::chosen_cut` 同一個道理。
    pub keys: String,
    /// 每個詞佔幾個假名
    pub lens: Vec<usize>,
}

/// 把一種切法變成可選字的格子。
///
/// 注音段會**再切成音節**——選字是以字為單位的。日文與英文段維持整段。
pub fn compose(segs: &[Segment]) -> Vec<Slot> {
    compose_with(segs, crate::width::Width::default())
}

/// 同上，但指定全半形模式。
///
/// 標點段會依模式轉換——`Auto` 看**前面那一段**的語言決定：
/// 中日文旁邊用全形（中文排版習慣），英文旁邊用半形（打程式碼時
/// 不能冒出全形符號）。
pub fn compose_with(segs: &[Segment], width: crate::width::Width) -> Vec<Slot> {
    compose_with_bounds(segs, width, None)
}

/// 同上，但可以指定**使用者手動調整過的日文詞界**。
pub fn compose_with_bounds(
    segs: &[Segment],
    width: crate::width::Width,
    bounds: Option<&JpBounds>,
) -> Vec<Slot> {
    compose_all(segs, width, bounds, None)
}

/// 全部的參數版本。`lock` 是使用者鎖定的語言（沒鎖就是 `None`）。
///
/// # 鎖定語言時標點跟著鎖走
///
/// 自動模式下標點看**這句話用什麼文字寫的**，句首沒有中日文可看時
/// 一律給半形（「打程式碼時不會突然冒出全形符號」，那個預設是對的）。
///
/// 使用者鎖定了語言之後就有依據了：鎖注音／日文時句首打句點應該是
/// `。` 而不是 `.`。鎖英文則維持半形——那個模式的語意本來就是
/// 「等同關掉輸入法」。
///
/// # 標點的全半形看整句，不看緊鄰的那一段
///
/// 全形標點屬於**句子的書寫系統**，不屬於相鄰的詞。中文文章裡夾用
/// 英文詞（「用 server 傳，很快」）時逗號仍然是全形——決定它的是
/// 這句話用中文寫，而不是它左邊剛好是英文。
///
/// 所以主導語言掃的是整句：句子裡出現過中日文就用該語言的標點，
/// 英文段沒有發言權（英文沒有自己的全形標點體系）。純英文的句子
/// 整句都掃不到中日文，維持半形，打程式碼的情境行為不變。
///
/// 中日並存時**先出現的那個**說了算——句子的書寫系統由它開頭定調，
/// 後面夾用的另一種語言是客人。
/// 這串段落**用什麼文字寫的**？決定標點要用哪一國的全形寫法。
///
/// # 為什麼看整句而不是看標點左邊那一段
///
/// 全形標點屬於句子的書寫系統，不屬於相鄰的詞。中文文章裡夾用英文詞
/// （「用 server 傳，很快」）時逗號仍然是全形——決定它的是這句話用
/// 中文寫，而不是它左邊剛好是英文。英文段因此沒有發言權（英文沒有
/// 自己的全形標點體系）；純英文的句子整句掃不到中日文，回 `None`
/// 維持半形，打程式碼的情境行為不變。
///
/// # 中日並存時看誰的段多
///
/// 中日只有逗號不同（`，` 對 `、`）。混語言長句即使開頭是日文，主體
/// 是中文時仍該用中文逗號——決定書寫系統的是**整句的重心**，不是誰
/// 先出現。平手時給中文：那是使用者更可能在寫的東西。
///
/// # `\名字\` 裡面的段不算
///
/// 符號名稱是查表的鍵，不是使用者寫下的句子內容。`e:\ㄒㄧㄥ\`（e:★）
/// 的「星」只是拿來查符號的，那句話本身是英文，冒號該維持半形。
fn dominant_cjk<'a>(segs: impl Iterator<Item = (bool, &'a String, Language)>) -> Option<Language> {
    let (mut zh, mut ja, mut in_symbol) = (0usize, 0usize, false);
    for (is_mark, keys, lang) in segs {
        if is_mark {
            if keys == "\\" {
                in_symbol = !in_symbol;
            }
            continue;
        }
        if in_symbol {
            continue;
        }
        match lang {
            Language::Bopomofo => zh += 1,
            Language::Romaji => ja += 1,
            Language::English => {}
        }
    }
    match (zh, ja) {
        (0, 0) => None,
        // 平手給中文，理由見上面
        (z, j) if z >= j => Some(Language::Bopomofo),
        _ => Some(Language::Romaji),
    }
}

pub fn compose_all(
    segs: &[Segment],
    width: crate::width::Width,
    bounds: Option<&JpBounds>,
    lock: Option<Language>,
) -> Vec<Slot> {
    let mut out = Vec::new();
    // 這個標點該用什麼文字的寫法？理由見 `dominant_cjk`。
    //
    // **只看它前面的段，不看後面**——往後看的話，句子後半打出中文
    // 會回頭把前面已經上畫面的半形標點改成全形，使用者會看到游標
    // 很遠的地方字在跳。已經定案的標點不該再變，所以每個標點用
    // 「打到它為止」的統計決定，後面再打什麼都不動它。
    //
    // 代價是句首的標點（`hello，電車`）拿不到脈絡、維持半形——那要
    // 往前看才救得到，而回頭改寫的代價更高。
    for (i, s) in segs.iter().enumerate() {
        if s.is_mark {
            let prev_lang = dominant_cjk(segs[..i].iter().map(|x| (x.is_mark, &x.keys, x.lang)));
            // **鎖定的語言優先**：鎖住的時候每一段本來就都是那個語言，
            // 而它還多涵蓋了「句首、前面沒東西」的情況
            let lang = lock.or(prev_lang);
            let converted: String = s
                .keys
                .chars()
                .map(|c| crate::width::convert(c, width, lang))
                .collect();
            // **`;//` 的分號當成冒號**——那是漏按 Shift 打錯的網址。
            //
            // `:` 與 `;` 是同一個實體鍵（差在 Shift），而 `https://`
            // 是極高頻的輸入，漏按很自然。`;//` 這個組合在中文、日文、
            // 英文、程式碼裡都沒有意義，所以**不是歧義，是純粹的打錯**
            // ——沒有歧義就不該讓使用者再選一次去確認雙方都知道的事。
            //
            // 判準要求後面**兩格都是 `/`**：`;/` 兩個字元還可能是別的
            // 東西（分號後面接路徑），兩條斜線才是網址的樣子。
            let converted = if s.keys == ";" && next_two_slashes(segs, i) {
                ":".to_string()
            } else {
                converted
            };
            // **有候選才開放選字**。沒有變體的符號（`@`、`#`）維持
            // 不可選——那些格子按方向鍵移過去卻叫不出東西只是干擾。
            let variants = crate::width::variants(&s.keys, lang);
            let selectable = !variants.is_empty();
            // **選過 `LEARNED` 次的寫法變成預設**。
            //
            // 跟選字走同一條路（鍵是按鍵、值是文字），所以「記住你慣用
            // 的括號樣式」不必另外寫機制。
            //
            // **只接受候選清單裡的值**——學習檔是使用者可以手改的，
            // 而這一格的文字會直接進文件。收窄成「表裡有的那幾個」，
            // 比信任檔案內容安全。
            let text = crate::learn::any()
                .then(|| crate::learn::index().best(&s.keys).map(str::to_string))
                .flatten()
                .filter(|t| variants.iter().any(|v| v.to_string() == *t))
                .unwrap_or(converted);
            out.push(Slot {
                keys: s.keys.clone(),
                text,
                lang: s.lang,
                selectable,
                is_mark: true,
                cands: None,
                picked: false,
            });
            continue;
        }
        match s.lang {
            Language::Bopomofo => {
                // 注音再切成音節，每個音節一格
                match crate::bopomofo::split_syllables(&s.keys) {
                    Some(syllables) => {
                        for syl in syllables {
                            let text = best_char(&syl);
                            out.push(Slot {
                                keys: syl,
                                text,
                                lang: Language::Bopomofo,
                                selectable: true,
                                is_mark: false,
                                cands: None,
                                picked: false,
                            });
                        }
                    }
                    // 切不出音節（還在打）——整段當一格，顯示原始按鍵
                    None => out.push(Slot {
                        keys: s.keys.clone(),
                        text: s.keys.clone(),
                        lang: Language::Bopomofo,
                        selectable: false,
                        is_mark: false,
                        cands: None,
                        picked: false,
                    }),
                }
            }
            Language::Romaji => {
                let kana = crate::romaji::kana::to_kana(&s.keys).unwrap_or_else(|| s.keys.clone());
                // **一段日文再切成詞**——跟注音段切成音節同一個道理。
                //
                // 語言邊界是切點引擎的事（這一段是日文），詞的邊界是
                // 日文引擎的事（這段裡有幾個詞）。兩個維度分開，見
                // 開發文件 §2.23。
                // 使用者調整過這一段的詞界就照他的來，否則交給 Viterbi
                let words = match bounds {
                    Some(b) if b.keys == s.keys => {
                        crate::romaji::convert::convert_with(&kana, &b.lens)
                    }
                    _ => crate::romaji::convert::convert(&kana),
                };
                // **整段就是一個動詞的活用形時，不該被切成詞**。
                //
                // mozc 只收辭書形，所以活用形查不到，Viterbi 只好硬湊：
                // `よんだ` 被切成「4」＋「だ」、`いそいでいる` 切成
                // 「急いで」＋「イル」。那不是分詞問題——整串本來就是
                // 一個詞（読んだ／急いでいる），只是詞典裡沒有。
                //
                // `inflect::漢字表記` 組得出來就代表它是活用形，直接用
                // 組出來的結果，不進分詞。
                let 活用 = crate::romaji::inflect::漢字表記(&s.keys);
                if let Some(w) = 活用 {
                    out.push(Slot {
                        keys: s.keys.clone(),
                        text: w,
                        lang: Language::Romaji,
                        selectable: true,
                        is_mark: false,
                        cands: None,
                        picked: false,
                    });
                } else if words.len() <= 1 {
                    // 只有一個詞（或轉不出來）就維持原本的一格
                    out.push(Slot {
                        keys: s.keys.clone(),
                        text: best_japanese(&s.keys, &kana),
                        lang: Language::Romaji,
                        selectable: true,
                        is_mark: false,
                        cands: None,
                        picked: false,
                    });
                } else {
                    // **按鍵怎麼分配**：格子的 `keys` 要接得回原字串
                    // （`check_rewrite`、`delete_marked_slot`、學習都靠
                    // 這個性質）。分詞是照假名做的，而羅馬字跟假名
                    // **不是等比**——所以用 mora 的真實對應換算，
                    // 見 `kana::mora_spans`。
                    let spans = crate::romaji::kana::mora_spans(&s.keys).unwrap_or_default();
                    let keys: Vec<char> = s.keys.chars().collect();
                    let mut used = 0usize; // 用掉幾個按鍵
                    let mut si = 0usize; // 走到第幾個 mora
                    for (i, w) in words.iter().enumerate() {
                        let want = w.kana.chars().count();
                        let take = if i + 1 == words.len() || spans.is_empty() {
                            // 最後一格吃掉剩下的——保證接得回去
                            keys.len().saturating_sub(used)
                        } else {
                            // 吃 mora 直到假名數湊滿。詞界如果落在 mora
                            // 中間（`きゃ` 是一個 mora 兩個假名）就多吃
                            // 那一個——邊界差一點好過按鍵對不回去。
                            let mut k = 0usize;
                            let mut kana = 0usize;
                            while si < spans.len() && kana < want {
                                k += spans[si].0;
                                kana += spans[si].1;
                                si += 1;
                            }
                            k.min(keys.len().saturating_sub(used))
                        };
                        let part: String = keys[used..used + take].iter().collect();
                        used += take;
                        out.push(Slot {
                            keys: part,
                            text: w.surface.clone(),
                            lang: Language::Romaji,
                            selectable: true,
                            is_mark: false,
                            cands: None,
                            picked: false,
                        });
                    }
                }
            }
            // 英文就是它自己，沒有同音字的問題。
            //
            // **一個例外：日文詞典也收的英文段**（`ii`→いい、`mou`→もう）。
            // 語言判斷是一票定生死的（`lang_of`：夠常用的英文詞不讓給
            // 日文，否則 `you`／`the`／`time` 全會變假名），使用者沒有
            // 第二意見可表達——切法選單裡也不會有把它當日文的那一種。
            // 所以這一格開放選字，把日文候選補進清單。
            // **預設仍然是英文**，只有使用者表態過（學習到 `LEARNED`
            // 次）才會換掉，三支計分器量的第一名因此不動。
            Language::English => {
                let jp = crate::dict::is_japanese_word(&s.keys);
                let text = jp
                    .then(|| {
                        crate::learn::any()
                            .then(|| crate::learn::index().best(&s.keys).map(str::to_string))
                            .flatten()
                    })
                    .flatten()
                    .unwrap_or_else(|| s.keys.clone());
                out.push(Slot {
                    keys: s.keys.clone(),
                    text,
                    lang: Language::English,
                    selectable: jp,
                    is_mark: false,
                    cands: None,
                    picked: false,
                });
            }
        }
    }
    // 用詞庫修一次——單看每個字的字頻常常是錯的。
    //
    // **一定要排在 `merge_symbols` 之前**：符號名字比對的是「組出來的
    // 文字」，而詞庫修正之前每一格還是逐字的字頻第一名。`\音樂\` 那時
    // 是「因月」（ㄧㄣ 的第一名是「因」、ㄩㄝˋ 是「月」），查不到符號，
    // 而畫面上早已顯示成「音樂」——症狀是「字明明對卻不會變成符號」。
    // 它只碰連續的注音格，跟下面兩個合併的對象不重疊，提前是安全的。
    let by_word = apply_word_context(&mut out);
    // 再用中文字級 bigram 看前後文重算一次。**一定要排在
    // `apply_word_context` 之後**——詞層是強證據（查得到的詞就是詞），
    // 語言模型是統計傾向，讓統計覆蓋詞層會把 `這個`、`需求` 這種本來
    // 就對的字改壞。這裡只處理詞層管不到的位置（詞庫沒收的組合、
    // 單字連著單字）。
    apply_lm(&mut out, &by_word);
    // 數字後面的量詞。**排在語言模型之後**——「數字接量詞」是中文的
    // 結構而不是統計傾向，而且 bigram 在這個位置沒有資料可用（數字
    // 不是漢字）。
    apply_number_units(&mut out);
    // `\名字\` 換成符號。**要在標點合併之前**——連續的 `\` 不該先被
    // 當成標點組合吃掉
    merge_symbols(&mut out);
    // 連續的標點可能是一個組合（`...` → `…`）
    merge_mark_runs(&mut out, lock);
    out
}

/// 後面緊接著兩個 `/` 嗎？——`;//` 判定用，見呼叫處。
fn next_two_slashes(segs: &[Segment], i: usize) -> bool {
    segs.get(i + 1).is_some_and(|s| s.keys == "/") && segs.get(i + 2).is_some_and(|s| s.keys == "/")
}

/// 把 `\名字\` 換成符號。
///
/// # 名字查兩次：先文字、再按鍵
///
/// **中文只能用文字**——按鍵是 `\vu/␣\` 那種沒人記得住的東西，所以
/// 注音組出的「星」就是名字本身。
///
/// **日文只能用按鍵**。羅馬字要先過語言判斷、再過整句轉換，組出來的
/// 東西不可預期：`tougou` 變成漢字「統合」、`sekibun` 前半被判成中文
/// 成了「席bun」、`mugen` 尾巴的 `n` 遇到收尾的 `\` 還沒收成「ん」。
/// 2026-09-05 實測 29 個日文名字有 12 個這樣叫不出來，**連按 Tab 都
/// 沒有那個選項**——切法分支裡根本沒生出「整段當日文」那條。
///
/// 按鍵原文不經過任何判斷，打什麼就是什麼，所以日文名字在表裡列羅馬字
/// （`積分,sekibun,integral`）。英文名字兩條路都會中（它本來就是
/// passthrough），順序上先文字後按鍵，中文那條的行為完全不變。
///
/// # 收尾的 `\` 一打完就換掉
///
/// 使用者裁決（2026-09-04）：不必再選一次。所以預設文字是**第一個
/// 符號**，字面原樣放在候選的最後當退路。
///
/// **代價**：路徑裡剛好有同名資料夾時會被換掉（`C:\check\`）。判斷過
/// 值不值得——符號名撞到資料夾名的機率低，而且候選裡有原樣可以救。
///
/// # 為什麼要 `cands`
///
/// 合併之後這一格的 `keys` 是 `\vu/␣\`，名字（星）沒了。要從 keys 回推
/// 得重跑一次選詞，所以候選在這裡算好、存進 `Slot::cands`。
fn merge_symbols(slots: &mut Vec<Slot>) {
    let mut i = 0;
    while i < slots.len() {
        if !(slots[i].is_mark && slots[i].keys == "\\") {
            i += 1;
            continue;
        }
        // 往後找收尾的 `\`
        let Some(end) = (i + 1..slots.len()).find(|&j| slots[j].is_mark && slots[j].keys == "\\")
        else {
            // 沒有收尾就不是符號，維持現狀
            i += 1;
            continue;
        };
        if end == i + 1 {
            // `\` 中間沒東西
            i = end;
            continue;
        }
        let name: String = slots[i + 1..end].iter().map(|s| s.text.as_str()).collect();
        let mut syms = crate::symbol::lookup(&name);
        // 文字查不到就用**按鍵原文**再查一次，日文名字靠這條（見上面的說明）
        if syms.is_empty() {
            let typed: String = slots[i + 1..end].iter().map(|s| s.keys.as_str()).collect();
            syms = crate::symbol::lookup(&typed);
        }
        if syms.is_empty() {
            // 查不到就當普通的反斜線。**從收尾那個重新找起**——
            // `\a\b\` 的第二個 `\` 可能是下一組的開頭
            i = end;
            continue;
        }
        let keys: String = slots[i..=end].iter().map(|s| s.keys.as_str()).collect();
        let literal: String = slots[i..=end].iter().map(|s| s.text.as_str()).collect();
        // 候選：符號在前，字面原樣在最後當退路
        let mut cands = syms.clone();
        cands.push(literal);
        // 選過 `LEARNED` 次的那個變預設，跟選字同一條路
        let text = crate::learn::any()
            .then(|| crate::learn::index().best(&keys).map(str::to_string))
            .flatten()
            .filter(|t| cands.contains(t))
            .unwrap_or_else(|| syms[0].clone());
        slots.splice(
            i..=end,
            [Slot {
                keys,
                text,
                lang: Language::English,
                selectable: true,
                is_mark: true,
                cands: Some(cands),
                picked: false,
            }],
        );
        i += 1;
    }
}

/// 把連續的標點格合併成一格——如果那個組合有自己的寫法。
///
/// # 為什麼在這一層做，不動切點引擎
///
/// 切點引擎的標點格**一格就是一個字元**（`to_segments` 寫死
/// `end == begin + 1`）。要讓 `...` 成為一格，改切點引擎是一種做法，
/// 但那會動到三支計分器量的東西；在 `compose` 合併則完全碰不到切點。
///
/// 合併後 `keys` 仍然接得回原字串（`...` 三個字元變成一格的 `keys`），
/// 那是 `check_rewrite`、倒退鍵刪格、學習都靠的性質。
fn merge_mark_runs(slots: &mut Vec<Slot>, lock: Option<Language>) {
    let mut i = 0;
    while i < slots.len() {
        if !slots[i].is_mark {
            i += 1;
            continue;
        }
        // **只在中日文脈絡下合併**。
        //
        // `hello...` 不該變成 `hello…`——那跟「打程式碼時不會突然冒出
        // 全形符號」是同一條原則，而且英文的刪節號本來就常寫三個點。
        // 測資的 `hello|.|.|.` 期望的正是三個點。
        //
        // 判準跟標點的全半形轉換共用（見 `dominant_cjk`），同樣**只看
        // 前面**——已經合併定案的標點不該因為後面又打了字而拆開。
        let prev_lang = dominant_cjk(slots[..i].iter().map(|s| (s.is_mark, &s.keys, s.lang)));
        let lang = lock.or(prev_lang);
        if !matches!(lang, Some(Language::Bopomofo | Language::Romaji)) {
            i += 1;
            continue;
        }
        // 這一串連續的標點有多長
        let mut end = i;
        while end < slots.len() && slots[end].is_mark {
            end += 1;
        }
        // **從最長的開始試**——`......` 要優先於 `...`
        let mut merged = false;
        for stop in ((i + 2)..=end).rev() {
            let keys: String = slots[i..stop].iter().map(|s| s.keys.as_str()).collect();
            let variants = crate::width::variants(&keys, lock);
            if variants.is_empty() {
                continue;
            }
            let text = variants[0].to_string();
            slots.splice(
                i..stop,
                [Slot {
                    keys,
                    text,
                    lang: Language::English,
                    selectable: true,
                    is_mark: true,
                    cands: None,
                    picked: false,
                }],
            );
            i += 1;
            merged = true;
            break;
        }
        if !merged {
            i = end;
        }
    }
}

/// 這個注音音節最可能是哪個字？
///
/// 同音字依**字頻**排序，取第一名。`su3` 有 29 個同音字，
/// 字頻讓「你」排在「儗」「旎」前面。
fn best_char(syllable: &str) -> String {
    crate::dict::best_char_for(syllable)
        .map(str::to_string)
        .unwrap_or_else(|| syllable.to_string())
}

/// 用詞庫修正相鄰的注音格。
///
/// 單看每個字的字頻常常選錯——`su3cl3` 逐字選是「擬郝」，但那兩個字
/// 連在一起是「你好」。這裡從長到短掃過去，找得到詞就把整組字換掉。
///
/// **從長到短**是因為長詞的資訊量大：`5k4ek7` 是「這個」而不是
/// 「這」＋「個」各自的第一名。
/// `lm_pick_word` 裡「靜態詞頻名次」的權重。
///
/// 掃過 0 到 10：0～0.5 是 747、1 是 748、2 是 749、**2.5～3.5 是 750**、
/// 4 以上回落。取穩定區中間。
///
/// 太小的話**沒有左鄰的詞**會被詞內部那一對 bigram 帶走——`剛才`→`鋼材`、
/// `小明`→`曉明`、`計畫`→`計劃`，實測弄壞 5 句全是這型（單獨出現的詞
/// 只有一對字可看，那一對分不出高下）。太大則語言模型永遠推翻不了詞頻，
/// 等於沒接。
///
/// 兩端各有一個失敗模式。太小的話**沒有左鄰的詞**會被詞內部那一對
/// bigram 帶走——`剛才`→`鋼材`、`小明`→`曉明`、`計畫`→`計劃`，實測
/// 弄壞 5 句全是這型（單獨出現的詞只有一對字可看，而那一對分不出高下）。
/// 太大則語言模型永遠推翻不了詞頻，等於沒接。
const LM_PICK_W_RANK: f32 = 3.0;

/// 同讀音有好幾個詞時，用語言模型挑一個。
///
/// # 為什麼需要它
///
/// 詞層原本取 `words_for` 的第一個，而那個順序是**靜態詞頻**排的
/// ——它不知道這句話在講什麼。`救回來`／`就回來` 讀音完全相同，詞頻
/// 永遠讓「救回來」贏，於是「他今天就回來」打不出來。實測 790 句測資
/// 裡有 201 個位置存在這種競爭，**16 次挑錯**。
///
/// 這些候選**全部都是合法的詞**（`記得`／`寄的`、`曉得`／`小的`、
/// `深得`／`深的`），不是詞庫收錯——差別只在上下文。詞頻分不開，
/// 但「我不曉得」與「小的東西」前後文完全不同，bigram 分得出來。
///
/// # 分數怎麼算
///
/// 兩個部分相加：
///
/// - **詞內部的接續**：詞裡每一對相鄰的字算一次 bigram。`就回來` 的
///   `就回`＋`回來` 對上 `救回來` 的 `救回`＋`回來`——差別在第一對。
/// - **跟左鄰的接續**：詞的第一個字接前一格最後一個字。`他今天|就回來`
///   的 `天就` 對 `天救`，這一項常常是決定性的。
///
/// 不看右鄰是刻意的：**詞層是從長到短掃的**，右邊那格還沒定案
/// （可能被更長的詞吃掉），拿未定的東西當證據會不穩。左鄰已經處理完
/// 所以可靠。
///
/// # 沒有模型或分數相同時
///
/// 回 `None`，呼叫端退回原本的「取第一個」。這是刻意的保守——語言模型
/// 是加分項，它沒意見的時候不該改變既有行為。
fn lm_pick_word<'a>(
    words: &'a [std::borrow::Cow<'static, str>],
    left: Option<char>,
) -> Option<&'a str> {
    let lm = crate::lm::get()?;
    if words.len() < 2 {
        return None;
    }
    let score_of = |rank: usize, w: &str| -> f32 {
        // **靜態詞頻的名次先驗**。`words_for` 已經依詞頻排好，那份順序
        // 大多數時候是對的——語言模型只該在證據夠強時推翻它。
        //
        // 少了這一項會壞在**沒有左鄰的詞**上：`剛才`／`鋼材`、`小明`／
        // `曉明`、`計畫`／`計劃` 單獨出現時只剩詞內部一對 bigram 可看，
        // 而那一對分不出高下（甚至指向錯的）。實測弄壞 5 句全是這型。
        let mut s = -LM_PICK_W_RANK * rank as f32;
        let cs: Vec<char> = w.chars().collect();
        // 詞內部：每一對相鄰的字
        for pair in cs.windows(2) {
            if is_han(pair[0]) && is_han(pair[1]) {
                s += lm.score(pair[0], pair[1]).unwrap_or(LM_MISS);
            }
        }
        // 跟左鄰的接續
        if let (Some(l), Some(&f)) = (left, cs.first()) {
            if is_han(l) && is_han(f) {
                s += lm.score(l, f).unwrap_or(LM_MISS);
            }
        }
        s
    };
    let mut best: Option<(&str, f32)> = None;
    for (rank, w) in words.iter().enumerate() {
        let s = score_of(rank, w);
        match best {
            Some((_, bs)) if s <= bs => {}
            _ => best = Some((w.as_ref(), s)),
        }
    }
    best.map(|(w, _)| w)
}

fn apply_word_context(slots: &mut [Slot]) -> Vec<(usize, usize)> {
    let n = slots.len();
    // 哪些格是詞層決定的？回報給 `apply_lm`——**詞是強證據**（查得到
    // 的詞就是那個詞），統計傾向不該覆蓋它。實測沒有這道保護時
    // 「各位」會被 bigram 改成「個為」。
    let mut by_word = vec![(0usize, 0usize); n];
    let mut i = 0;
    while i < n {
        if !slots[i].selectable || slots[i].lang != Language::Bopomofo {
            i += 1;
            continue;
        }
        // 從最長的連續注音段開始試
        let mut end = i;
        while end < n && slots[end].selectable && slots[end].lang == Language::Bopomofo {
            end += 1;
        }
        let mut matched = false;
        for stop in (i + 2..=end).rev() {
            let keys: String = slots[i..stop].iter().map(|s| s.keys.as_str()).collect();
            // **同讀音的詞不只一個**，挑跟「使用者已經選過的字」相容的
            // 那一個。「城市」與「程式」讀音相同，選了「程」就該挑到
            // 「程式」，「市」才會跟著變「式」——挑不到相容的就用第一個
            // （預設值），行為跟只有一個詞的時候一樣。
            let all = crate::dict::words_for(&keys);
            // **同讀音有幾個詞**？只有一個時詞層說了算（那是強證據）；
            // 有競爭者時它只是「第一個」，該讓語言模型從中挑
            // ——`救回來`／`就回來` 同鍵，詞層取第一個永遠是「救回來」。
            let n_words = all.len();
            // 字數要跟格數對得上（才填得回去），而且不能跟使用者
            // 手動選過的字衝突
            let fits = |w: &str| {
                let cs: Vec<char> = w.chars().collect();
                cs.len() == stop - i
                    && (i..stop).all(|j| {
                        !slots[j].picked || slots[j].text.chars().eq(std::iter::once(cs[j - i]))
                    })
            };
            let usable: Vec<_> = all.into_iter().filter(|w| fits(w)).collect();
            // **同讀音有好幾個詞時讓語言模型挑**，而不是取靜態詞頻的
            // 第一個——`救回來`／`就回來` 讀音相同，詞頻永遠讓前者贏。
            // 挑不出來（沒有模型、或分數相同）就退回第一個。
            let left = i.checked_sub(1).and_then(|j| slots[j].text.chars().last());
            let chosen = lm_pick_word(&usable, left)
                .map(|w| w.to_string())
                .or_else(|| usable.first().map(|w| w.to_string()));
            if let Some(word) = chosen {
                for (k, c) in word.chars().enumerate() {
                    // **手動選過的字不覆蓋**——使用者已經表態了
                    if !slots[i + k].picked {
                        slots[i + k].text = c.to_string();
                    }
                    by_word[i + k] = (word.chars().count(), n_words);
                }
                i = stop;
                matched = true;
                break;
            }
        }
        if !matched {
            i += 1;
        }
    }
    by_word
}

/// **維特比的寬度**：每一格只考慮前幾個候選。
///
/// 這是效能守門員，不是品質取捨。不限制的話最壞一句要 49,275 次字對
/// 查詢（`昨天的會議記錄已經寄給大家`），Rust 就算比 Python 快 30 倍
/// 也要 14ms，而按鍵預算只有 16ms、現況已經用掉 11.2ms。
///
/// **限到 5 個之後最壞降到 320 次（1/154），命中一句都沒少**（790 句
/// 實測 754 對 754）。很合理——正解幾乎都在前幾名，第 20 個同音字不
/// 可能是答案。3 個也是 754，留 5 是給罕見情況一點餘裕。
const LM_WIDTH: usize = 5;

/// bigram 分數的權重。
///
/// 掃描過 0.25 到 3.0，0.5 到 1.0 之間都落在 750 到 754，**不敏感**。
/// 太大會壓過詞層修正的結果，太小則吃不到收益。
const LM_W_BIGRAM: f32 = 0.5;

/// 候選名次的權重：清單裡越後面的字，先驗越差。
///
/// 候選本來就依「字頻 × 讀音佔比」排好，這個權重保留那份排序的話語權
/// ——語言模型只有在證據夠強時才該推翻它。
const LM_W_RANK: f32 = 1.0;

/// **台灣常用字先驗**：我們自己的字頻（教育部字頻表）的權重。
///
/// 這一條是「常用字優先」——語料是簡體轉繁體來的，對台灣用語的覆蓋偏弱，
/// 而教育部字頻正好代表台灣的用字習慣。實測它擋掉了 `剛纔`、`界面`
/// 這類偏好，也讓資料稀疏時安全地退回我們自己的排序。
const LM_W_OWN: f32 = 0.1;

/// 兩個字在模型裡查不到時的罰分。
///
/// 不能給 0——那等於「查不到」跟「關聯強度剛好是 0」沒有差別。給負值
/// 代表「沒有證據支持這兩個字連在一起」。掃描過 0 到 -2，差別很小。
const LM_MISS: f32 = -1.0;

/// 用**中文字級 bigram** 把整句的選字重算一次。
///
/// # 為什麼是整句，不是逐格挑
///
/// §2.53.5 量到「只看左鄰 77%、只看右鄰 71%、兩邊都看 85%」——
/// `將來再說` 的線索在右邊（`再說`），逐格往前看永遠分不開。而要
/// 「看兩邊」就必須在整句上找總分最高的路徑，因為每一格的最佳選擇
/// 取決於鄰居，鄰居又取決於它。這跟日文整句轉換是同一個結構（§2.23）。
///
/// # 只碰「可選字的中文單字格」
///
/// 英文段、日文段、標點、符號格全部固定不動——它們的正確性由別的機制
/// 決定（日文有自己的 Viterbi、英文是原文照抄），中文 bigram 對它們
/// 沒有話語權。**手動選過的字（`picked`）也不動**，跟
/// `apply_word_context` 同一條原則：使用者已經表態了。
///
/// # 分數的組成
///
/// 每一格的分數是「名次先驗 ＋ 台灣字頻先驗」，格與格之間再加
/// 「bigram 關聯強度」。四個權重的來歷各見它們自己的常數說明。
/// 語言模型能不能動這一格？`(詞長, 同讀音競爭者數)` 由
/// `apply_word_context` 回報，`(0, 0)` 代表詞層沒碰過。
///
/// # 為什麼詞層碰過就一律不動
///
/// **詞是強證據、統計是傾向**。掃描過四種放行策略，全部拿詞層的錯誤
/// 換語言模型的收益，換不划算：
///
/// | 策略 | 文字命中 | `各位` |
/// |---|---|---|
/// | **全保護（這個）** | **748** | **✓** |
/// | 有競爭者就放行 | 751 | ✗ 變「個為」 |
/// | 長詞且有競爭者才放行 | 749 | ✓ |
/// | 完全不保護 | 754 | ✗ |
///
/// 完全不保護多的那 6 句幾乎都是 §2.45 那個「同讀音存得下多個詞、
/// 詞層取第一個」的延伸（`救回來`／`就回來` 同鍵），**那是詞層自己
/// 該修的**——讓統計去繞過它會連 `各位` 這種詞層明明給對的也一起賠掉。
///
/// `個為` 的 bigram 是 12.6、`各位` 只有 7.7，而且那不是資料錯誤
/// ——「一個為了…」在語料裡真的比「各位」常見。單字 bigram 分不出
/// 「這兩個字是一個詞」與「這兩個字常常相鄰」，那正是詞層存在的理由。
fn lm_movable((len, _n): (usize, usize)) -> bool {
    len == 0
}

/// **數字後面的量詞／單位**。
///
/// # 為什麼需要這張表
///
/// 數字段跟中文之間沒有語言模型可用——bigram 的鍵是漢字對，而 `3` 不是
/// 漢字，`3` 與 `個` 之間沒有任何統計資料。於是「數字＋量詞」全部退回
/// 純字頻決定，而同音的非量詞字常常字頻更高：
///
/// ```text
/// 3個bug  → 3各bug     各 比 個 常用
/// 100元   → 100原      原 比 元 常用
/// 第2季   → 第2記      記 比 季 常用
/// 第3章   → 第3張      張 比 章 常用
/// ```
///
/// # 判準：數字的右鄰是單一個中文字時，優先挑量詞
///
/// 測資裡 78 個數字段有 **70 個右鄰是單一個中文字**，而那 16 個選錯的
/// 正解**全部**在這張表裡（涵蓋 16/16、漏 0）。正解幾乎都排第 2～4 名，
/// 不是排在很後面——這是「加一條判準就拿得到」的距離。
///
/// **只在右鄰是單字時生效**。`5|分鐘`、`50|公斤` 那種多字的右鄰本來就
/// 由詞層決定（`分鐘` 是詞），不需要也不該被這條插手。
const NUMBER_UNITS: &[char] = &[
    // 通用量詞
    '個', '位', '名', '件', '份', '樣', '種', '類', '組', '批', '對', '雙', // 時間
    '年', '月', '日', '天', '時', '點', '分', '秒', '週', '季', '期', '屆', '次', '回',
    // 貨幣與度量
    '元', '塊', '角', '斤', '兩', '克', '噸', '尺', '寸', '里', '坪', '畝',
    // 出版與編號
    '版', '章', '節', '頁', '段', '句', '字', '行', '列', '課', '篇', '卷', '冊', '本', '號',
    // 容器與器物
    '杯', '碗', '盤', '碟', '瓶', '罐', '包', '盒', '箱', '袋', '張', '片', '塊',
    // 建築與空間
    '層', '樓', '間', '棟', '戶', '室', '排', '格', '欄', // 人與動物
    '人', '口', '隻', '頭', '匹', '尾', '條', // 交通與器材
    '台', '輛', '架', '艘', '部', '具', '支', '把', '面', '塊', // 折扣與比例
    '折', '成', '倍', '級', '等', '階',
];

/// **序數（`第 N ○`）後面的單位**，比一般量詞優先。
///
/// `第3章` 的 `章` 與 `張` 同音，而 `張`（一張紙）也是量詞、字頻更高，
/// 所以一般量詞表會先挑到它。序數的脈絡下該是章節單位。
const ORDINAL_UNITS: &[char] = &[
    '章', '節', '課', '篇', '卷', '冊', '版', '題', '頁', '段', '句', '行', '列', '名', '位', '屆',
    '期', '季', '級', '等', '階', '層', '樓', '號', '次', '回', '年', '月', '日', '天', '週', '個',
    '件', '組', '批',
];

/// 數字後面那一格，優先挑量詞。
///
/// # 為什麼是獨立的一條，不併進語言模型
///
/// `apply_lm` 做的是「整句找最佳路徑」，而它的證據是**漢字對的 bigram**
/// ——數字不是漢字，那條路徑上根本沒有分數可算。這裡補的是語言模型
/// 涵蓋不到的那個位置。
///
/// # 為什麼放在 `apply_lm` 之後
///
/// 這一條的證據比統計強：「數字後面接量詞」是中文的結構，不是傾向。
/// 讓它有最後的話語權。
///
/// **手動選過的字（`picked`）不動**，跟其他所有改字的地方同一條原則。
fn apply_number_units(slots: &mut [Slot]) {
    for i in 1..slots.len() {
        // 前一格是純數字嗎？
        let prev_is_number =
            !slots[i - 1].text.is_empty() && slots[i - 1].text.chars().all(|c| c.is_ascii_digit());
        if !prev_is_number {
            continue;
        }
        let s = &slots[i];
        // 只碰「可選字、注音、單一個字、沒被手動選過」的格。
        //
        // **日文格不管，試過了**：`5sai`（5歳）、`4kai`（4階）的量詞確實
        // 在候選裡（`歳` 第 5、`階` 第 7），但日文的量詞多義比中文嚴重
        // ——`回` 也是量詞而且排第一，`側`／`代` 同理。中文能用「第」的
        // 脈絡把 `張`／`章` 分開，日文沒有對應的線索。實測放進來是
        // 1139 → 1138，救 `2台` 一句卻弄壞 `1番`。
        if !s.selectable || s.lang != Language::Bopomofo || s.picked || s.is_mark {
            continue;
        }
        if s.text.chars().count() != 1 {
            continue;
        }
        // **序數的脈絡優先**：`第 N ○` 的 ○ 是章節單位，不是一般量詞。
        //
        // 沒有這一條的話 `第3章` 會變 `第3張`——`張` 也在量詞表裡（一張紙）
        // 而且字頻更高，先命中。`第4題` 對 `第4提` 同理。
        let ordinal = i >= 2 && slots[i - 2].text == "第";
        let table: &[char] = if ordinal { ORDINAL_UNITS } else { NUMBER_UNITS };
        // 目前顯示的已經是（這個脈絡下的）量詞就不必動。
        //
        // **這道早退要在 `table` 決定之後**——放前面的話 `第3張` 會因為
        // `張` 在一般量詞表裡（一張紙）就直接跳過，序數表根本沒機會發言。
        if s.text.chars().next().is_some_and(|c| table.contains(&c)) {
            continue;
        }
        let pick = candidates_for(s)
            .into_iter()
            .find(|cand| {
                let mut it = cand.chars();
                matches!((it.next(), it.next()), (Some(c), None) if table.contains(&c))
            })
            // 序數表沒中就退回一般量詞表
            .or_else(|| {
                if !ordinal {
                    return None;
                }
                candidates_for(s).into_iter().find(|cand| {
                    let mut it = cand.chars();
                    matches!((it.next(), it.next()), (Some(c), None) if NUMBER_UNITS.contains(&c))
                })
            });
        if let Some(p) = pick {
            slots[i].text = p;
        }
    }
}

fn apply_lm(slots: &mut [Slot], by_word: &[(usize, usize)]) {
    let Some(lm) = crate::lm::get() else { return };

    // 每一格的候選集合。不可動的格子只有一個選項（現在的字）
    let mut opts: Vec<Vec<char>> = Vec::with_capacity(slots.len());
    let mut any = false;
    for (idx, s) in slots.iter().enumerate() {
        // 只動「可選字、注音、單一個中文字、沒被手動選過」的格
        let movable = s.selectable
            && s.lang == Language::Bopomofo
            && !s.picked
            && !s.is_mark
            && lm_movable(by_word.get(idx).copied().unwrap_or((0, 0)))
            && s.text.chars().count() == 1;
        let cur: Vec<char> = s.text.chars().take(1).collect();
        if !movable {
            opts.push(cur);
            continue;
        }
        let mut v: Vec<char> = Vec::with_capacity(LM_WIDTH);
        for cand in candidates_for(s) {
            let mut it = cand.chars();
            let (Some(c), None) = (it.next(), it.next()) else {
                continue;
            };
            if !is_han(c) || v.contains(&c) {
                continue;
            }
            v.push(c);
            if v.len() >= LM_WIDTH {
                break;
            }
        }
        if v.len() > 1 {
            any = true;
        }
        opts.push(if v.is_empty() { cur } else { v });
    }
    // 沒有任何一格有得選就不必跑——中英日混打時這是常態
    if !any {
        return;
    }
    // **有格子完全沒候選就不能跑維特比**：那一列的 `score` 是空的，
    // 回溯時 `score[n-1][best]` 直接越界 panic。
    //
    // 段選單踩到過：使用者定案的段送進來時，某一格可能查不到任何字
    // （非法組合、或詞庫還沒載完）。這是 `compose` 本來就有的邊界情況，
    // **不管誰呼叫都不該 panic**，所以修在這裡而不是呼叫端。
    if opts.iter().any(|v| v.is_empty()) {
        return;
    }

    // 維特比：狀態是「這一格選了哪個候選」，記最佳前驅回溯
    let n = opts.len();
    let mut score: Vec<Vec<f32>> = Vec::with_capacity(n);
    let mut back: Vec<Vec<usize>> = Vec::with_capacity(n);
    for i in 0..n {
        let m = opts[i].len();
        let mut sc = vec![f32::NEG_INFINITY; m];
        let mut bk = vec![0usize; m];
        for (ci, &c) in opts[i].iter().enumerate() {
            let prior = -LM_W_RANK * ci as f32 + LM_W_OWN * lm.log_freq(c);
            if i == 0 {
                sc[ci] = prior;
                continue;
            }
            for (pi, &pc) in opts[i - 1].iter().enumerate() {
                let mut v = score[i - 1][pi] + prior;
                // 只有相鄰兩邊都是漢字時才有 bigram 可算
                if is_han(pc) && is_han(c) {
                    v += LM_W_BIGRAM * lm.score(pc, c).unwrap_or(LM_MISS);
                }
                if v > sc[ci] {
                    sc[ci] = v;
                    bk[ci] = pi;
                }
            }
        }
        score.push(sc);
        back.push(bk);
    }

    // 回溯最佳路徑
    let mut best = 0usize;
    for (i, &v) in score[n - 1].iter().enumerate() {
        if v > score[n - 1][best] {
            best = i;
        }
    }
    let mut path = vec![0usize; n];
    path[n - 1] = best;
    for i in (1..n).rev() {
        path[i - 1] = back[i][path[i]];
    }
    for (i, p) in path.iter().enumerate() {
        if opts[i].len() > 1 {
            slots[i].text = opts[i][*p].to_string();
        }
    }
}

/// 是不是中日韓統一表意文字（漢字）。
#[inline]
fn is_han(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
}

/// 使用者在第 `idx` 格選了 `choice` 這個字之後，重算後面的格子。
///
/// 這是使用者定的規則——「選了『你』，『郝』就自己變成『好』」。
/// 只往後修，不動前面已經選過的。
pub fn pick(slots: &mut [Slot], idx: usize, choice: &str) {
    if idx >= slots.len() {
        return;
    }
    slots[idx].text = choice.to_string();
    slots[idx].picked = true;
    if slots[idx].is_mark {
        pair_closing(slots, idx, choice);
        return;
    }
    // 從選中的那格開始往後找詞。`picked` 的格子不會被覆蓋，
    // 所以這裡不必再把使用者選的字寫回去一次。
    let _ = apply_word_context(&mut slots[idx..]);
}

/// 選了開括號之後，把配對的收括號一起改掉。
///
/// # 為什麼需要
///
/// 把 `[` 選成 `「` 之後，後面那個 `]` 還是 `]`——使用者得再選一次，
/// 而且**要記得自己剛才選了哪一種**。成對的東西本來就該一起變。
///
/// # 配對怎麼找
///
/// 往後找**第一個沒被巢狀吃掉的 `]`**。中間再遇到 `[` 就加一層深度，
/// 這樣 `[a[b]c]` 的外層才配得到外層。
///
/// **手動選過的不覆蓋**——跟 `apply_word_context` 同一條原則，使用者
/// 已經表態的地方引擎不再自作主張。
fn pair_closing(slots: &mut [Slot], idx: usize, choice: &str) {
    let Some(open_char) = choice.chars().next() else {
        return;
    };
    if choice.chars().count() != 1 {
        return;
    }
    let Some(close_char) = crate::width::closing_for(open_char) else {
        return;
    };
    // 用**按鍵**認配對，不是用文字——那一格顯示什麼還沒定，
    // 但它是使用者按的哪個鍵是確定的
    let open_key = slots[idx].keys.clone();
    let close_key = match open_key.as_str() {
        "[" => "]",
        _ => return,
    };
    let mut depth = 0usize;
    for slot in &mut slots[idx + 1..] {
        if !slot.is_mark {
            continue;
        }
        if slot.keys == open_key {
            depth += 1;
        } else if slot.keys == close_key {
            if depth == 0 {
                if !slot.picked {
                    slot.text = close_char.to_string();
                }
                return;
            }
            depth -= 1;
        }
    }
}

/// 日文的候選：**三種假名 + 詞庫的漢字轉換**。
///
/// 假名那三種不必查詞庫——它們是純粹的字形轉換，任何一段假名都
/// 一定有對應。日本的輸入法都提供這三個選項：
///
/// | | 例（`sushi`） | 什麼時候用 |
/// |---|---|---|
/// | 平假名 | すし | 和語、助詞 |
/// | 全形片假名 | スシ | 外來語、強調 |
/// | 半形片假名 | ｽｼ | 舊系統相容、表格 |
///
/// 排在最前面是因為它們**一定正確**——漢字轉換是猜的，假名不是。
/// 詞庫查到的漢字接在後面，重複的（詞庫剛好收了假名寫法）去掉。
fn romaji_candidates(kana: &str) -> Vec<String> {
    use crate::romaji::kana::{to_halfwidth_katakana, to_katakana};
    if kana.is_empty() {
        return Vec::new();
    }
    let dict = crate::dict::words_for_kana(kana);
    let mut out = Vec::new();
    // 詞典最佳排第一——那是預設顯示的字，清單第一個要跟它一致
    if let Some(best) = dict.first() {
        out.push(best.clone());
    }
    // 三種假名寫法緊接在後。它們**一定正確**（漢字轉換是猜的，假名不是），
    // 所以要讓使用者永遠一兩下就回得到假名
    for k in [
        kana.to_string(),
        to_katakana(kana),
        to_halfwidth_katakana(&to_katakana(kana)),
    ] {
        if !out.contains(&k) {
            out.push(k);
        }
    }
    for w in dict.iter().skip(1) {
        if !out.contains(w) {
            out.push(w.clone());
        }
    }
    out
}

/// 這串假名預設顯示什麼？
///
/// # 漢字優先（使用者 2026-08-31 裁決，見期望基準審核.md A6）
///
/// 查詞典取總成本最低的表記。**總成本含接續成本**，這是關鍵——只看
/// 詞成本的話「すし」會給「酸し」(4451) 而不是「寿司」(4520)，因為
/// 「酸し」是文語形容詞，便宜在詞本身、貴在句首接不上去。
///
/// 好處是**平假名該贏的時候會自己贏**：`ありがとう`、`おはよう` 查出來
/// 的第一名就是平假名，不必為它們另外維護例外表。
///
/// # 查不到就試著**組**出來
///
/// mozc 只收辭書形，所以活用形一律查不到，本來只能原樣顯示假名——
/// 打「おきた」出來就是「おきた」。但漢字推得出來：還原成辭書形
/// 「おきる」查到「起きる」，取語幹「起」接回語尾「きた」→「起きた」。
///
/// 見 `romaji::inflect::漢字表記`。組不出來才回到假名。
fn best_japanese(keys: &str, kana: &str) -> String {
    if let Some(w) = crate::dict::best_kana_word(kana) {
        return w.into_owned();
    }
    crate::romaji::inflect::漢字表記(keys).unwrap_or_else(|| kana.to_string())
}

/// 某一格的候選字，依字頻排序。
pub fn candidates_for(slot: &Slot) -> Vec<String> {
    if !slot.selectable {
        return Vec::new();
    }
    // 算好的優先——符號格的名字在合併時就沒了，回不去
    if let Some(c) = &slot.cands {
        let mut out = c.clone();
        if let Some(i) = out.iter().position(|x| *x == slot.text) {
            let cur = out.remove(i);
            out.insert(0, cur);
        }
        return out;
    }
    let mut out = if slot.is_mark {
        // **標點格不查詞庫**，它的候選是「同一個符號的不同寫法」。
        // 語言傳 `None`——那一格已經算好文字了，`variants` 只有逗號
        // 需要語言，而那個差別已經反映在 `slot.text` 上，下面「把目前
        // 的字提到最前面」會處理
        crate::width::variants(&slot.keys, None)
            .iter()
            .map(|c| c.to_string())
            .collect()
    } else {
        candidates_raw(slot)
    };
    // **清單的第一個永遠是這一格現在顯示的字。**
    //
    // 候選本身是依字頻排的，但那一格顯示的未必是字頻第一名——手動選過
    // （`picked`）或被詞層修正過都會不一樣：
    //
    // ```text
    // 選了「程」之後   #1 text=式   候選 是 事 市 世 士 示 式 …
    //                                              ↑ 排第 7
    // ```
    //
    // 反白進選字時停在第 0 個（`select.rs` 的 `cand_idx = 0`），清單第一個
    // 不是現在的字的話，**方向鍵一移就把剛修好的字弄丟**。
    //
    // 這條規則跟 §2.24 給英文段定的「第一個放英文原文，讓使用者永遠
    // 回得來」是同一條，只是推廣到所有語言。
    //
    // 純顯示層的調整——預設輸出走的是 `best_char` 與 `apply_word_context`，
    // 不經過這裡，所以三支計分器不會動。
    if let Some(i) = out.iter().position(|c| *c == slot.text) {
        let cur = out.remove(i);
        out.insert(0, cur);
    } else if !slot.text.is_empty() {
        // 詞層填進來的字未必在同音字清單裡（偏好表注入的詞就可能）
        out.insert(0, slot.text.clone());
    }
    out
}

/// 依字頻排好的候選，還沒把「現在顯示的字」提到前面。
fn candidates_raw(slot: &Slot) -> Vec<String> {
    match slot.lang {
        Language::Bopomofo => crate::dict::chars_for(&slot.keys),
        // **拿讀音去查，不是拿已經轉換過的文字**。
        //
        // 這裡踩過坑：原本傳的是 `slot.text`，那一格是漢字時
        // （`ご飯`）就查不到任何候選，只剩三種假名寫法；剛好停在
        // 假名時才查得到。整句轉換之前每格常常是假名，所以不明顯，
        // 分詞之後每格都是漢字，問題就整個浮出來。
        // **鏡像 §2.24**：日文段若英文詞典也收得到這串按鍵，把英文
        // 原文補在最後（`youtube`→ようつべ、`sushi`→すし）。
        //
        // 理由跟 `ii`→いい 那次一模一樣，只是方向相反。`lang_of` 的
        // 「夠常用的英文詞不讓給日文」門檻是排名 5000，而 `youtube`
        // 排 10160，於是整段被判成日文，切法選單裡也不會有把它當英文
        // 的那一種——使用者沒有第二意見可表達。
        //
        // **拉高那個門檻的路是死的**：`sushi` 排 7210、`karaoke` 排
        // 9015，都比 `youtube` 前面。門檻只要高到能救 `youtube`，
        // `sushi` 就會變成英文原樣而不是「すし」。英文詞頻排名分不開
        // 「英文詞」與「用羅馬字打的日文外來語」，這兩群完全交錯。
        //
        // **補在最後、不動前面**：預設輸出不變（第一名仍是日文），
        // 三支計分器因此一個數字都不動。選過 `LEARNED` 次之後
        // `best_kana_word` 才會換掉——日文格的學習鍵是假名，那條路
        // 本來就通，不必另外接。
        //
        // **用 `is_common_word` 不是 `is_word`**：`en_50k` 收了大量冷僻
        // 的兩字母詞，助詞格會因此多出雜訊（`wo`→を 那一格冒出「wo」）。
        // `is_common_word` 對兩字母有頻率門檻（≥10 萬），正好濾掉這批，
        // 三字母以上一律放行，所以 `youtube`／`sushi` 不受影響。
        Language::Romaji => {
            let kana =
                crate::romaji::kana::to_kana(&slot.keys).unwrap_or_else(|| slot.text.clone());
            let mut out = romaji_candidates(&kana);
            // **活用形組出來的漢字要進候選**。
            //
            // 詞典沒有活用形，所以 `romaji_candidates` 給不出「読んだ」，
            // 使用者選不回來。而同一個活用形常常對應好幾個動詞——
            // `よんだ` 可以是 読んだ／呼んだ／詠んだ——全部都要在清單裡，
            // 不然排序猜錯就沒救。
            //
            // 插在假名寫法後面：假名一定正確（漢字是猜的），要讓使用者
            // 一兩下就回得到假名，這是 `romaji_candidates` 的既有規矩。
            for (i, w) in crate::romaji::inflect::漢字候選(&slot.keys)
                .into_iter()
                .enumerate()
            {
                if out.contains(&w) {
                    continue;
                }
                // 第一個（最可能的）擺到最前面，其餘接在假名之後
                if i == 0 && !out.is_empty() {
                    out.insert(0, w);
                } else {
                    out.push(w);
                }
            }
            if crate::english::is_common_word(&slot.keys) && !out.contains(&slot.keys) {
                out.push(slot.keys.clone());
            }
            out
        }
        // 只有「日文詞典也收」的英文段走得到這裡（別的英文段
        // `selectable` 是 false，上面就回去了）。第一個放英文原文，
        // 讓使用者永遠回得來。
        Language::English => {
            let mut out = vec![slot.keys.clone()];
            if let Some(kana) = crate::romaji::kana::to_kana(&slot.keys) {
                for c in romaji_candidates(&kana) {
                    if !out.contains(&c) {
                        out.push(c);
                    }
                }
            }
            out
        }
    }
}

/// 把格子接成一串顯示文字。
pub fn text_of(slots: &[Slot]) -> String {
    slots.iter().map(|s| s.text.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cutpoint::incremental::Incremental;
    use crate::cutpoint::{normalize, rank};

    fn load() -> bool {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        crate::preload(&root.join("data"), crate::config::Engines::default());
        // 符號現在全在預載包裡（`packs/內建符號.txt`），不載就查不到。
        // 使用者目錄指向一個不存在的路徑，測試才不會被本機的包影響。
        crate::pack::set_bundled_dir(Some(root.join("packs")));
        crate::pack::load(
            "__測試用_不存在的資料夾__",
            &[crate::pack::BUNDLED_SYMBOLS.to_string()],
        );
        crate::dict::bopomofo_loaded()
    }

    /// 取第一名切法的格子
    fn slots_of(keys: &str) -> Vec<Slot> {
        let cands = rank::sort(Incremental::from_keys(keys).cuttings());
        compose(&normalize(&cands[0]))
    }

    #[test]
    fn 注音切成單字() {
        if !load() {
            eprintln!("詞庫未下載，跳過（跑 data/download.ps1）");
            return;
        }
        let slots = slots_of("su3cl3");
        assert_eq!(slots.len(), 2, "你好是兩個字：{slots:?}");
        assert!(slots.iter().all(|s| s.selectable));
    }

    #[test]
    fn 詞庫修正逐字選錯的結果() {
        if !load() {
            return;
        }
        // su3 的字頻第一名不是「你」，但「你好」是詞
        let slots = slots_of("su3cl3");
        assert_eq!(text_of(&slots), "你好");
    }

    /// 不同的東西各有各的名字，不必在一排符號裡數格子。
    ///
    /// 2026-09-05 重排：＋－×÷ 這種**根本是不同東西**的從群組裡拆出來
    /// 各給一列（群組那列留著當瀏覽入口），★☆✦✧ 那種**同一個東西的
    /// 不同寫法**才留在一起。這條守住拆出來的那些真的叫得到。
    #[test]
    fn 拆開的符號各叫各的() {
        if !load() {
            return;
        }
        for (keys, want) in [
            (r"\plus\", "＋"),
            (r"\times\", "×"),
            (r"\sun\", "☀"),
            (r"\yen\", "￥"),
            (r"\male\", "♂"),
            (r"\celsius\", "℃"),
            // 群組還在，第一個符號直接出來
            (r"\math\", "＋"),
            (r"\weather\", "☀"),
        ] {
            assert_eq!(text_of(&slots_of(keys)), want, "{keys} 叫不出符號");
        }
    }

    /// 日文名字靠**按鍵原文**這條路，不靠組出來的文字。
    ///
    /// **這幾個是實測選出來的**（2026-09-05 掃過 29 個日文名字）：
    /// `sekibun` 組出來是「席bun」（前半被判成中文）、`mugen` 是「無gen」
    /// （尾巴的 n 遇到收尾的 `\` 還沒收成ん）、`tougou` 直接轉成漢字
    /// 「統合」。三個都跟表裡的名字對不上，而且**連按 Tab 都沒有那個
    /// 選項**——切法裡根本沒生出「整段當日文」那條。
    ///
    /// 哪天有人把查詢改回只看文字，這條會紅。
    #[test]
    fn 日文名字用按鍵查得到() {
        if !load() {
            return;
        }
        for (keys, want) in [
            (r"\sekibun\", "∫"),
            (r"\mugen\", "∞"),
            (r"\tougou\", "＝"),
            (r"\hoshi\", "★"),
        ] {
            assert_eq!(text_of(&slots_of(keys)), want, "{keys} 叫不出符號");
        }
    }

    #[test]
    fn 符號名字看的是詞庫修正後的文字() {
        if !load() {
            return;
        }
        // ㄧㄣ 的字頻第一名是「因」、ㄩㄝˋ 是「月」，所以逐字組出來是
        // 「因月」——拿它查符號表當然查不到，而畫面上顯示的早已是詞庫
        // 修正過的「音樂」，症狀是「字明明對卻不會變成符號」。
        //
        // 這條守住 `apply_word_context` 排在 `merge_symbols` 前面。
        assert_eq!(text_of(&slots_of(r"\up m,4\")), "♪", "音樂 → ♪");
        // 同樣要靠詞庫才組得出來：ㄐㄧㄢˋㄊㄡˊ 逐字不是「箭頭」
        assert_eq!(text_of(&slots_of(r"\ru04w.6\")), "→", "箭頭 → →");
    }

    #[test]
    fn 符號的英文名與單字名不受影響() {
        if !load() {
            return;
        }
        // 英文段是 passthrough，文字永遠等於按鍵，本來就不受詞庫修正影響
        assert_eq!(text_of(&slots_of(r"\star\")), "★");
        // 單字名字逐字第一名就對，改順序之後也要照舊
        assert_eq!(text_of(&slots_of(r"\vu/ \")), "★", "星 → ★");
    }

    #[test]
    fn 英文段不參與選字() {
        if !load() {
            return;
        }
        let slots = slots_of("check");
        assert_eq!(slots.len(), 1);
        assert!(!slots[0].selectable);
        assert_eq!(slots[0].text, "check");
    }

    /// **標點也能選寫法**。
    #[test]
    fn 標點有候選() {
        if !load() {
            return;
        }
        // `[` → 中日文的各種括號，第一個是目前顯示的字
        let slots = slots_of("su3cl3[");
        let br = slots.last().expect("該有一格 [");
        assert!(br.is_mark);
        assert!(br.selectable, "有候選就要能選");
        let c = candidates_for(br);
        assert_eq!(c.first().map(String::as_str), Some("["), "第一個是現在的字");
        assert!(c.iter().any(|x| x == "「"), "要有中文引號：{c:?}");

        // 逗號看語言——日文旁邊第一個是讀點
        let ja = slots_of("sushi,");
        let comma = ja.last().unwrap();
        assert_eq!(comma.text, "、");
        assert_eq!(
            candidates_for(comma).first().map(String::as_str),
            Some("、")
        );

        // 沒有變體的符號維持不可選，不然按方向鍵移過去卻叫不出東西
        let at = slots_of("su3cl3@");
        assert!(!at.last().unwrap().selectable, "@ 沒有變體");
    }

    /// **`\名字\` 換成符號**，三種語言共用同一份表。
    #[test]
    fn 符號用反斜線包起來() {
        if !load() {
            return;
        }
        // 注音組出「星」
        let s = slots_of(r"\vu/ \");
        assert_eq!(text_of(&s), "★", "打完收尾的反斜線就換掉，不必再選");
        assert_eq!(s.len(), 1, "三格合併成一格");
        assert_eq!(s[0].keys, r"\vu/ \", "按鍵要接得回原字串");

        // 英文名指向同一組符號
        assert_eq!(text_of(&slots_of(r"\star\")), "★");
        // 夾在句子中間
        assert_eq!(text_of(&slots_of(r"su3cl3\vu/ \cl3")), "你好★好");

        // 候選：符號在前，字面原樣在最後當退路
        let c = candidates_for(&s[0]);
        assert_eq!(c.first().map(String::as_str), Some("★"));
        assert_eq!(c.last().map(String::as_str), Some(r"\星\"), "原樣是退路");
    }

    /// **查不到的名字不動**——路徑不能被弄壞。
    #[test]
    fn 不是符號的反斜線維持原樣() {
        if !load() {
            return;
        }
        let path = r"C:\Users\test";
        assert_eq!(text_of(&slots_of(path)), path);
        // 沒有收尾的反斜線
        assert_eq!(text_of(&slots_of(r"\star")), r"\star");
        // 中間沒東西
        assert_eq!(text_of(&slots_of(r"\\")), r"\\");
    }

    /// **選了開括號，配對的收括號要跟著變**。
    #[test]
    fn 括號成對連動() {
        if !load() {
            return;
        }
        // su3cl3[su3cl3] → 你好[你好]
        let mut s = slots_of("su3cl3[su3cl3]");
        let open = s.iter().position(|x| x.keys == "[").expect("該有 [");
        pick(&mut s, open, "「");
        assert_eq!(text_of(&s), "你好「你好」", "收括號要跟著變");

        // **手動選過的不覆蓋**
        let mut s = slots_of("su3cl3[su3cl3]");
        let close = s.iter().position(|x| x.keys == "]").unwrap();
        pick(&mut s, close, "】");
        let open = s.iter().position(|x| x.keys == "[").unwrap();
        pick(&mut s, open, "「");
        assert!(text_of(&s).ends_with('】'), "使用者選過的收括號不該被蓋掉");

        // **巢狀要配對到自己那一層**
        let mut s = slots_of("[su3[cl3]su3]");
        let outer = s.iter().position(|x| x.keys == "[").unwrap();
        pick(&mut s, outer, "「");
        let t = text_of(&s);
        assert!(t.starts_with('「') && t.ends_with('」'), "外層配外層：{t}");
    }

    /// **`...` 在中日文脈絡下合併成一格**，英文脈絡不動。
    #[test]
    fn 三個點合併成刪節號() {
        if !load() {
            return;
        }
        assert_eq!(text_of(&slots_of("su3cl3...")), "你好…");
        // **英文脈絡不合併**——跟「打程式碼不會冒出全形符號」同一條原則
        assert_eq!(text_of(&slots_of("hello...")), "hello...");
        // 兩個點不是組合，維持兩格
        assert_eq!(text_of(&slots_of("su3cl3..")), "你好。。");

        // 合併後 keys 仍然接得回原字串——check_rewrite、倒退鍵刪格、
        // 學習都靠這個性質
        let s = slots_of("su3cl3...");
        let joined: String = s.iter().map(|x| x.keys.as_str()).collect();
        assert_eq!(joined, "su3cl3...");
    }

    /// **標點的全半形看整句，不看緊鄰那一段**。
    ///
    /// 中文句子裡夾一個英文詞，逗號仍該是全形——決定它的是這句話用
    /// 中文寫，不是它左邊剛好是英文。見 `dominant_cjk`。
    #[test]
    fn 標點看整句不看緊鄰那段() {
        if !load() {
            return;
        }
        // 夾在中文句子裡的英文詞不該把標點拉成半形
        assert_eq!(text_of(&slots_of("tj06server,su3cl3")), "傳server，你好");
        // 純英文整句掃不到中日文，維持半形（打程式碼的情境不變）
        assert_eq!(text_of(&slots_of("server,client")), "server,client");
        // 中文緊鄰時本來就對，不能改壞
        assert_eq!(text_of(&slots_of("su3cl3,su3cl3")), "你好，你好");
    }

    /// **只往回看，不往前看**——已經上畫面的標點不因後面打的字而變。
    ///
    /// 往前看會讓「句子後半打出中文」回頭改前面的半形標點，使用者
    /// 看到游標很遠的地方字在跳（漏斗的改寫硬指標）。
    #[test]
    fn 標點只看前面不因後文改變() {
        if !load() {
            return;
        }
        // 逗號前面只有英文 → 半形；後面接了中文也不該回頭改它
        let 前段 = text_of(&slots_of("server,"));
        assert!(前段.ends_with(','), "前面沒有中日文時是半形：{前段}");
        let 全句 = text_of(&slots_of("server,su3cl3"));
        assert!(
            全句.starts_with("server,"),
            "後面打了中文也不回頭改前面的標點：{全句}"
        );
    }

    /// **`\名字\` 裡面的段不算脈絡**——符號名稱是查表的鍵，不是句子內容。
    #[test]
    fn 符號名稱不算句子的文字() {
        if !load() {
            return;
        }
        // `e:★` 的「星」只是拿來查符號的，那句話本身是英文 → 冒號半形
        let t = text_of(&slots_of("e:\\vu/ \\"));
        assert!(t.starts_with("e:"), "符號名稱不該讓冒號變全形：{t}");
    }

    /// **候選清單的第一個永遠是這一格現在顯示的字**。
    ///
    /// 反白進選字時停在第 0 個，清單第一個不是現在的字的話，方向鍵
    /// 一移就把手動選過／被詞層修正過的字弄丟。
    #[test]
    fn 候選第一個是目前顯示的字() {
        if !load() {
            return;
        }
        // 詞層修正過的：選了「程」之後第 2 格是「式」（字頻排第 7）
        let mut slots = slots_of("t/6g4");
        pick(&mut slots, 0, "程");
        assert_eq!(text_of(&slots), "程式");
        assert_eq!(
            candidates_for(&slots[0]).first().map(String::as_str),
            Some("程"),
            "手動選過的那格"
        );
        assert_eq!(
            candidates_for(&slots[1]).first().map(String::as_str),
            Some("式"),
            "被詞層修正過的那格"
        );
        // 其餘仍照字頻——「式」被抽走之後第二個是原本的第一名
        assert_eq!(
            candidates_for(&slots[1]).get(1).map(String::as_str),
            Some("是")
        );
    }

    /// **同讀音的詞要全部查得到，而且預設不變**。
    #[test]
    fn 同讀音的詞不只一個() {
        if !load() {
            return;
        }
        let ws = crate::dict::words_for("t/6g4");
        assert_eq!(
            ws.first().map(|w| w.as_ref()),
            Some("城市"),
            "第一個仍是最常用的——預設輸出不能變：{ws:?}"
        );
        assert!(
            ws.iter().any(|w| w.as_ref() == "程式"),
            "「程式」讀音相同，也要在清單裡：{ws:?}"
        );
        // `word_for` 只回第一個，那是「直接送出」要的預設值
        assert_eq!(crate::dict::word_for("t/6g4").as_deref(), Some("城市"));
    }

    /// **數字後面優先挑量詞**——那個位置語言模型幫不上忙。
    ///
    /// bigram 的鍵是漢字對，而數字不是漢字，`3` 與 `個` 之間沒有任何
    /// 統計資料，於是全部退回純字頻，而同音的非量詞字常常字頻更高
    /// （`各` 比 `個` 常用、`原` 比 `元` 常用）。
    #[test]
    fn 數字後面優先量詞() {
        if !load() {
            return;
        }
        // 3個：「各」字頻比「個」高
        assert_eq!(text_of(&slots_of("3ek4")), "3個");
        // 100元：「原」字頻比「元」高
        assert_eq!(text_of(&slots_of("100m06")), "100元");
        // **序數的脈絡優先**：第3章 的「章」與「張」同音，而「張」也是
        // 量詞（一張紙）、字頻更高
        assert_eq!(text_of(&slots_of("2u435; ")), "第3章");
    }

    /// **同讀音的詞靠上下文挑**，不是永遠取詞頻第一個。
    ///
    /// `救回來`／`就回來` 讀音完全相同，靜態詞頻永遠讓「救回來」贏。
    /// 接上語言模型之後，「他今天—」這個左鄰讓「就回來」勝出。
    ///
    /// **沒有語言模型時要退回原本的行為**（取第一個），所以這條測試
    /// 在模型載不到時直接跳過——那不是失敗，是設計。
    #[test]
    fn 同讀音的詞靠上下文挑() {
        if !load() {
            return;
        }
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data");
        if crate::lm::load(&data, crate::dict::char_freq_map(&data)).is_none() {
            return; // 沒有語言模型，這條沒有意義
        }
        // 「他今天就回來」——左鄰是「天」
        assert_eq!(text_of(&slots_of("w8 rup wu0 ru.4cjo6x96")), "他今天就回來");
        // 單獨打仍然是詞頻第一個（沒有上下文就不該推翻既有順序）
        assert_eq!(crate::dict::word_for("t/6g4").as_deref(), Some("城市"));
    }

    /// **輕聲詞用本調也要打得出來**。
    ///
    /// 語料標的是「這個字怎麼念」（輕聲），使用者打的是字典音。
    /// 沒有別名的話 `5k4ek4` 查不到「這個」，詞層一落空就整個詞逐字
    /// 重排——「各」在ㄍㄜˋ底下贏「個」五倍。
    #[test]
    fn 輕聲詞的本調別名() {
        if !load() {
            return;
        }
        // 使用者實際的打法：ㄓㄜˋㄍㄜˋ
        assert_eq!(text_of(&slots_of("5k4ek4")), "這個");
        assert_eq!(text_of(&slots_of("u6ek4")), "一個");
        assert_eq!(text_of(&slots_of("s84ek4")), "那個");
        // 輕聲那條路不能壞
        assert_eq!(text_of(&slots_of("5k4ek7")), "這個");
        // **別名只填空位**：「各位」的鍵上本來就有真詞，不該被蓋掉
        assert_eq!(text_of(&slots_of("ek4jo4")), "各位");
    }

    /// **壞掉的常常不是那個輕聲字，是被拖垮的鄰居**。
    ///
    /// 「子」在ㄗˇ底下本來就排第一，所以逐字重排時它自己是對的
    /// ——垮的是前面那格：ㄏㄞˊ 的第一名是「還」不是「孩」。
    /// 判準因此不能問「這個字排不排得到第一」，要問「整個詞對不對」。
    #[test]
    fn 輕聲詞的本調別名_壞的是鄰居() {
        if !load() {
            return;
        }
        // c96y3 = ㄏㄞˊㄗˇ，沒有別名的話是「還子」
        assert_eq!(text_of(&slots_of("c96y3")), "孩子");
        // gk6ai6 = ㄕㄜˊㄇㄛˊ（什麼的四種讀音之一，本調）
        assert_eq!(text_of(&slots_of("gk6ai6")), "什麼");
        // u 2j4y3 = ㄧ ㄉㄨˋㄗˇ，沒有別名的話是「一度子」
        assert_eq!(text_of(&slots_of("u 2j4y3")), "一肚子");
        // **別名不排擠真詞**：ㄕㄣˊㄇㄛˊ 上面本來就有「神魔」，
        // 「什麼」的別名不該蓋掉它
        assert_eq!(text_of(&slots_of("gp6ai6")), "神魔");
    }

    /// **一次只換一個音節**。
    ///
    /// 第一版寫成整串 `replace`，詞裡有兩個「個」而輕重不同時就生不出
    /// 逐位置的變體，實測漏掉 9 條。
    #[test]
    fn 輕聲詞的本調別名_同一個字輕重並存() {
        if !load() {
            return;
        }
        // 一個個 = ㄧ ㄍㄜ˙ ㄍㄜˋ，第二個「個」本來就是本調
        assert_eq!(text_of(&slots_of("u ek7ek4")), "一個個");
        assert_eq!(text_of(&slots_of("u6ek7ek4")), "一個個");
    }

    /// **英文詞典也收的日文段要補英文原文**（§2.24 的鏡像）。
    ///
    /// `youtube` 排名 10160，超過 `lang_of` 的「夠常用就不讓給日文」
    /// 門檻（5000），於是整段被判成日文（ようつべ 在 mozc 裡）。
    /// 拉高門檻救不了——`sushi` 排 7210、`karaoke` 排 9015，都比它前面。
    /// 這一格的候選是使用者打出英文原文的唯一出口。
    #[test]
    fn 日文段_英文詞典也收的補上英文原文() {
        if !load() || !crate::dict::japanese_loaded() {
            return;
        }
        let slots = slots_of("youtubeao6g4");
        let yt = slots
            .iter()
            .find(|s| s.keys == "youtube")
            .unwrap_or_else(|| panic!("該有一格 youtube：{slots:?}"));
        assert_eq!(yt.lang, Language::Romaji);
        let cands = candidates_for(yt);
        // **預設不變**——第一名仍是日文，英文只是補在後面
        assert_ne!(
            cands.first().map(String::as_str),
            Some("youtube"),
            "英文不該搶第一名：{cands:?}"
        );
        assert!(
            cands.iter().any(|c| c == "youtube"),
            "候選要有英文原文：{cands:?}"
        );
    }

    /// 冷僻的兩字母詞不算——助詞格不該冒出雜訊。
    ///
    /// `wo`（を）在 en_50k 裡，但頻率是雜訊等級。用 `is_word` 的話
    /// 每個助詞格都會多一個英文候選。
    #[test]
    fn 日文段_冷僻兩字母詞不補() {
        if !load() || !crate::dict::japanese_loaded() {
            return;
        }
        let slots = slots_of("sushiwotabemasu");
        let wo = slots
            .iter()
            .find(|s| s.keys == "wo")
            .unwrap_or_else(|| panic!("該有一格 wo：{slots:?}"));
        let cands = candidates_for(wo);
        assert!(
            !cands.iter().any(|c| c == "wo"),
            "wo 是雜訊，不該補進候選：{cands:?}"
        );
    }

    /// **日文詞典也收的英文段要能選字**。
    ///
    /// `ii` 是排名 3557 的英文詞，`lang_of` 的「夠常用就不讓給日文」
    /// 把它判成英文；語言是一票定生死的，切法選單裡也沒有把它當日文
    /// 的那一種。這一格的候選是使用者打出「いい」的唯一出口。
    #[test]
    fn 英文段_日文詞典也收的可以選字() {
        if !load() || !crate::dict::japanese_loaded() {
            return;
        }
        let slots = slots_of("rup wu0 g4ek7iiwu0 fu4");
        let ii = slots
            .iter()
            .find(|s| s.keys == "ii")
            .expect("該有一格 ii：{slots:?}");
        assert_eq!(ii.lang, Language::English);
        assert!(ii.selectable, "該可以選字");
        let cands = candidates_for(ii);
        assert_eq!(
            cands.first().map(String::as_str),
            Some("ii"),
            "第一個要是英文原文"
        );
        assert!(cands.iter().any(|c| c == "いい"), "候選要有いい：{cands:?}");
    }

    #[test]
    fn 日文段預設寫成漢字() {
        if !load() {
            return;
        }
        // 使用者裁決「預設漢字優先」（期望基準審核.md A6）。
        // 「寿司」贏過「酸し」靠的是接續成本——只看詞成本的話
        // 「酸し」(4451) 比「寿司」(4520) 便宜
        assert_eq!(text_of(&slots_of("sushi")), "寿司");
        // 外來語用片假名
        assert_eq!(text_of(&slots_of("anime")), "アニメ");
    }

    #[test]
    fn 沒把握的就維持假名() {
        if !load() {
            return;
        }
        // 「どうしよう」是「どう＋しよう」兩個詞，詞典裡沒有這個條目，
        // 但「同仕様」剛好有。那種假命中的總成本偏高，被門檻擋掉
        assert_eq!(text_of(&slots_of("doushiyou")), "どうしよう");
        // 平假名本來就該贏的也不會被硬轉
        assert_eq!(text_of(&slots_of("arigatou")), "ありがとう");
    }

    #[test]
    fn 選字會帶動後面() {
        if !load() {
            return;
        }
        let mut slots = slots_of("su3cl3");
        // 假設使用者把第一格改成別的字，後面應該跟著重算
        pick(&mut slots, 0, "妳");
        assert_eq!(slots[0].text, "妳", "使用者選的不能被改掉");
    }

    #[test]
    fn 空輸入() {
        assert!(compose(&[]).is_empty());
        assert_eq!(text_of(&[]), "");
    }
    #[test]
    fn 日文候選是漢字加三種假名() {
        if !load() {
            return;
        }
        let c = romaji_candidates("すし");
        // 第一個是預設顯示的字，要跟 compose 的輸出一致
        assert_eq!(c[0], "寿司", "詞典最佳排第一：{c:?}");
        // 三種假名緊接在後——它們一定正確，使用者要永遠一兩下回得到
        assert_eq!(&c[1..4], &["すし", "スシ", "ｽｼ"], "三種假名要接在後面");
    }

    #[test]
    fn 濁音的半形片假名() {
        if !load() {
            return;
        }
        let c = romaji_candidates("ありがとう");
        assert_eq!(c[1], "アリガトウ");
        assert_eq!(c[2], "ｱﾘｶﾞﾄｳ", "濁音要拆成清音＋濁點");
    }

    #[test]
    fn 假名候選不重複() {
        if !load() {
            return;
        }
        // 詞庫剛好收了假名寫法時不該出現兩次
        let c = romaji_candidates("こんにちは");
        let n = c.iter().filter(|x| *x == "こんにちは").count();
        assert_eq!(n, 1, "重複的要去掉：{c:?}");
    }

    /// `;//` 的分號當成冒號——漏按 Shift 打錯的網址。
    ///
    /// `:` 與 `;` 是同一個實體鍵，而 `;//` 這個組合在中文、日文、英文、
    /// 程式碼裡都沒有意義，所以不是歧義是打錯。
    #[test]
    fn 網址的分號當成冒號() {
        if !load() {
            return;
        }
        for keys in ["https;//google.com", "http;//a.com", "x;//y"] {
            let got = text_of(&slots_of(keys));
            assert!(
                got.contains("://"),
                "`{keys}` 的分號該當成冒號：得到「{got}」"
            );
            assert!(!got.contains(";//"), "不該還留著分號：「{got}」");
        }
    }

    #[test]
    fn 只有一條斜線不算網址() {
        if !load() {
            return;
        }
        // `;/` 還可能是別的東西（分號後面接路徑），兩條斜線才是網址
        for keys in ["a;/b", "a;b", "test;"] {
            let got = text_of(&slots_of(keys));
            assert!(!got.contains(':'), "`{keys}` 不該被當成網址：得到「{got}」");
        }
    }
}
