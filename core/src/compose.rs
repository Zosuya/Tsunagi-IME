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
    /// 這個字是**模糊音修正**過的嗎？
    ///
    /// 跟 `picked` 一樣凍住（重選／語言模型／詞層／模糊音都不再動它），
    /// 但**不是**使用者的表態——`learn::record` 只認 `picked`，這個
    /// 旗標不能讓引擎猜的字被學進去（門檻見 `learn.rs` 對 `picked`
    /// 的檢查，這裡刻意不碰它）。
    ///
    /// 使用者手動選字（`pick()`）會蓋過它、改標 `picked = true`。
    pub fuzzy_fixed: bool,
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
    compose_all_with(segs, width, bounds, lock, &[])
}

/// `compose_all` 加上「這幾串按鍵不要展開成長輸出」。
///
/// # 為什麼需要這個參數
///
/// 長輸出是**多格併一格**的，併完就沒辦法逐字選字了（`ㄋㄧˇㄏㄠˇ`
/// 變成「今天天氣好」一整格，想把「你」改成「妳」做不到）。
/// 使用者在候選裡選回「原樣」時，那一格要**真的拆回多格**——
/// 靠的就是把那串按鍵放進這份清單再重算，見 `Session::unmerge_long`。
///
/// **段選單的預覽不傳**（`segmenu::seg_preview`）：那是在算「這一段
/// 選成某種語言會長什麼樣」，跟使用者對某一格的表態無關。
pub fn compose_all_with(
    segs: &[Segment],
    width: crate::width::Width,
    bounds: Option<&JpBounds>,
    lock: Option<Language>,
    no_expand: &[String],
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
                fuzzy_fixed: false,
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
                                fuzzy_fixed: false,
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
                        fuzzy_fixed: false,
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
                        fuzzy_fixed: false,
                    });
                } else if words.len() <= 1 {
                    // 只有一個詞（或轉不出來）就維持原本的一格。
                    //
                    // 先問 `best_kana_word`（學習層、擴充包、有把握的
                    // 首選，門檻是 `CONFIDENT_COST`）。查不到的話，
                    // **Viterbi 已經判定整段就是這一個詞**：直接信它，
                    // 不必再套一次全域門檻——那道門檻擋的是「多詞被
                    // 誤當一詞」（`どうしよう`→「同仕様」），但這裡
                    // Viterbi 本來就切成一個詞，沒有這個問題。單詞段跟
                    // 多詞段本來就該同一套標準，多詞段從來沒套過這道門。
                    //
                    // **只在表記含漢字時才採用**：片假名首選多半是同音
                    // 的外來語／人名（`あらら`→`アララ`），越過門檻反而
                    // 會把常見的語氣詞變成片假名。
                    let text = match crate::dict::best_kana_word(&kana) {
                        Some(w) => w.into_owned(),
                        None => words
                            .first()
                            .map(|w| w.surface.clone())
                            .filter(|t| t.chars().any(|c| !('\u{3040}'..='\u{30ff}').contains(&c)))
                            .unwrap_or_else(|| best_japanese(&s.keys, &kana)),
                    };
                    out.push(Slot {
                        keys: s.keys.clone(),
                        text,
                        lang: Language::Romaji,
                        selectable: true,
                        is_mark: false,
                        cands: None,
                        picked: false,
                        fuzzy_fixed: false,
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
                            fuzzy_fixed: false,
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
                    fuzzy_fixed: false,
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
    // 包裡的長輸出（`ㄨㄛˇㄑㄧㄥˇ` → 我請你喝一杯）先併成一格。
    //
    // **排在詞層之前**：包是使用者的明確表態，優先權高於詞庫統計，
    // 讓詞層先填字再拆掉重來沒有意義。合併後那格的 `cands` 已經算好
    // （包給的全部＋原樣），`candidates_for` 走「算好的優先」那條路，
    // 後面每一關都不會再碰它。
    merge_pack_long(&mut out, no_expand);
    let mut by_word = apply_word_context(&mut out);
    // 打錯的音用詞庫當證據反推回來（`ㄐㄧㄥㄊㄧㄢ` → 今天）。
    //
    // **排在詞層之後**：它的觸發條件就是「詞層查不到詞」，要等詞層先
    // 跑過才知道哪裡沒被修好。原鍵串查得到詞就完全不觸發，成本只在
    // miss 時付。見 `apply_fuzzy_tone` 與開發文件 §2.81。
    apply_fuzzy_tone(&mut out, &by_word);
    // **貪心搶字的補救**：詞層由左往右、命中就跳，左邊的詞會把右邊的
    // 字搶走（`想建立` → `想見`＋落單的「立」）。這一步把每一段連續
    // 中文用詞格維特比重切一次，**只在切得更完整時**才套用——其餘
    // 閘門為什麼都拿掉了，見 `recut_spans`。
    //
    // 重切換了詞界，`by_word` 要跟著換，後面的語言模型才看得到新的
    // 詞界（見 `recut_span` 的結尾）。
    recut_spans(&mut out, &mut by_word);
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
                fuzzy_fixed: false,
            }],
        );
        i += 1;
    }
}

/// 把擴充包裡的**長輸出**條目合併成一格（`ㄨㄛˇㄑㄧㄥˇ` → 我請你喝一杯）。
///
/// # 為什麼要獨立一條路，不能併進詞層
///
/// 詞層（`apply_word_context`）是**逐格填字**的——`slots[i+k].text = c`，
/// 一格一個字。字數跟音節數對不上就填不回去，這是物理限制不是規定。
/// 所以長輸出只能**多格併一格**，跟 `merge_symbols` 同一個模式。
///
/// 以前這種條目在 `pack::build_index` 整條丟掉，於是包裡寫了也沒作用
/// （死資料）。放寬之後它們進 `Index::zh_long` 走這裡。
///
/// # 為什麼排在詞層之前
///
/// 包是使用者的**明確表態**（「這串按鍵我就是要這個東西」），跟
/// `Slot::picked` 同一條原則，優先權高於詞庫統計。排在後面的話詞層
/// 已經把那幾格填成別的字，還要再拆掉重來。
///
/// 同理它也擋在 `apply_fuzzy_tone`／`apply_lm` 前面——合併後的那格
/// `selectable: false`，後面每一關都只看 `selectable` 的注音格，自然
/// 不會再碰它。
///
/// # 最長優先
///
/// 跟詞層一樣從最長的連續注音段開始試。`ㄨㄛˇㄑㄧㄥˇ` 與
/// `ㄨㄛˇ` 都在包裡時，打完兩個音節該出長的那個。
///
/// # 為什麼要 `cands`
///
/// 合併之後這一格的 `keys` 是整串按鍵，原本逐格的注音沒了。使用者
/// 想退回原樣得有退路——候選裡放「長輸出」與「字面原樣」兩個，跟
/// `merge_symbols` 把 `literal` 放最後是同一個道理。
fn merge_pack_long(slots: &mut Vec<Slot>, no_expand: &[String]) {
    // **沒設定長輸出的人一次都不掃**——這是每次組字都會走到的路。
    if !crate::pack::any_zh_long() {
        return;
    }
    // 關掉自動展開時**完全不併**——打完維持原樣，使用者按選字鍵才
    // 看得到長輸出（見 `config::Behavior::auto_expand_long`）。
    //
    // 放在 `any_zh_long()` 之後：沒有長輸出的人本來就不會走到這裡，
    // 多一次原子讀沒有意義。
    if !auto_expand_long() {
        return;
    }
    let mut i = 0;
    while i < slots.len() {
        if !slots[i].selectable || slots[i].lang != Language::Bopomofo {
            i += 1;
            continue;
        }
        // 這一段連續的注音格到哪裡為止
        let mut end = i;
        while end < slots.len() && slots[end].selectable && slots[end].lang == Language::Bopomofo {
            end += 1;
        }
        let mut matched = false;
        // 從最長的試起
        for stop in (i + 1..=end).rev() {
            let keys: String = slots[i..stop].iter().map(|s| s.keys.as_str()).collect();
            // **使用者選回原樣的那一串不要再併**——他剛表態過，
            // 併回去等於把他的選擇吃掉（見 `Session::unmerge_long`）
            if no_expand.contains(&keys) {
                continue;
            }
            let Some(long) = crate::pack::zh_long(&keys) else {
                continue;
            };
            // **使用者手動選過的字不覆蓋**——他已經表態了，跟詞層同一條原則
            if slots[i..stop].iter().any(|s| s.picked) {
                continue;
            }
            // **候選 = 包給的全部 ＋ 引擎原本會出的字**（2026-09-20）。
            //
            // 包可以有多筆同按鍵的長輸出，全部列出來；最後接上「原樣」
            // 當退路——**被包換掉的格子要選得回去**，這是使用者要的。
            // 去重：包的第一筆常常就等於引擎原本的字，重複列出來只是
            // 讓人以為壞了。
            let literal: String = slots[i..stop].iter().map(|s| s.text.as_str()).collect();
            let mut cands = crate::pack::zh_long_all(&keys);
            if cands.is_empty() {
                cands.push(long.clone());
            }
            if !cands.contains(&literal) {
                cands.push(literal);
            }
            slots.splice(
                i..stop,
                [Slot {
                    keys,
                    text: long,
                    lang: Language::Bopomofo,
                    // **可以選字，但候選是算好的那份**（`cands`）。
                    //
                    // 這一格的內容是整串文字而不是一個字的同音字，
                    // 落到詞庫查詢會拿到不知所云的候選——`candidates_for`
                    // 的「算好的優先」那條路擋住了，`cands` 一定不是空的
                    // （上面保證至少有 `long` 一筆）。
                    selectable: true,
                    is_mark: false,
                    cands: Some(cands),
                    picked: false,
                    fuzzy_fixed: false,
                }],
            );
            i += 1;
            matched = true;
            break;
        }
        if !matched {
            i = end.max(i + 1);
        }
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
                    fuzzy_fixed: false,
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

/// 單字邊的代價：把詞拆成單字要付出的分數。
///
/// 不付代價的話拆開反而總分高（`下午`→`下五`、`伺服器`→`四氟氣`）
/// ——中文的詞是有邊界的實體，把詞拆開該付代價。§2.60.2 實測到的，
/// 這一輪掃過 2～12 完全不敏感，取中間值。
const RECUT_SINGLE_COST: f32 = 6.0;

/// 詞頻名次的權重。
///
/// **這是個窄峰**：掃過 0.5～8，3.0 是唯一的 +2，兩側都掉
/// （1.0 → −1、5.0 → −3）。所以它不是可以隨手調的旋鈕，
/// **改它之前先跑漏斗**。
const RECUT_W_RANK: f32 = 3.0;

/// 擴充包裡的詞在重切時加多少分。
///
/// **大到一定贏**：包是使用者的明確表態，不該被 bigram 推翻
/// （理由見 `pack::zh_word`）。bigram 的分數是對數機率，單項大約在
/// −1 到 −20 之間，一個詞最多幾對，所以 1000 綽綽有餘——
/// 不必精算，只要「無論如何都壓過統計」。
const PACK_BONUS: f32 = 1000.0;

// **「只在剛收完一個字時才重切」那道閘門拿掉了。**
//
// 原本的設計（§2.74.4）是用「最後一格是不是成字的中文格」判斷使用者
// 打完了沒，只在那一刻重算。判準本身很準（88.3%），但**接進產品碼之後
// 反而製造擺盪**：多打一個還沒成字的按鍵時重切整個不跑，畫面就掉回
// 貪心的結果，下一鍵成字又跑一次——`在`↔`載` 每一鍵來回，一句貢獻
// 20 次遠處改寫。
//
// **擺盪的來源是「跑／不跑」交替，不是重算本身。** 改成一律跑之後
// 改寫從 26 降到 5。反直覺但合理：穩定的規則比聰明的規則重要。
//
// 防止前文跳動因此完全交給 `more_complete`（切得更完整才套用）。
// 後來連「只碰最後兩段」的窗口也拿掉了，理由見 `recut_spans`。

/// 這一段被切成哪些詞？**貪心版**，用來當重切的比較基準。
///
/// 規則跟 `apply_word_context` 一樣：由左往右、最長先、命中就跳。
/// 只回傳「切成哪些詞」，不動 slots。
fn greedy_words(slots: &[Slot], lo: usize, hi: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = lo;
    while i < hi {
        let mut matched = false;
        for stop in ((i + 2)..=hi).rev() {
            let keys: String = slots[i..stop].iter().map(|s| s.keys.as_str()).collect();
            if let Some(w) = crate::dict::words_for(&keys)
                .into_iter()
                .find(|w| w.chars().count() == stop - i)
            {
                out.push(w.to_string());
                i = stop;
                matched = true;
                break;
            }
        }
        if !matched {
            out.push(slots[i].text.clone());
            i += 1;
        }
    }
    out
}

/// 落單的單字有幾個、最長的詞幾個字。
fn word_shape(words: &[String]) -> (usize, usize) {
    let mut singles = 0usize;
    let mut longest = 0usize;
    for w in words {
        let n = w.chars().count();
        if n == 1 {
            singles += 1;
        }
        longest = longest.max(n);
    }
    (singles, longest)
}

/// **詞完整性閘門**：重切的結果比原本更完整嗎？
///
/// 使用者提的判準（§2.74.5）：判斷到打完了要重算時，也要檢查詞的
/// 完整性才可以做修正。兩邊都是詞的時候（`適合` vs `是合`）純粹是
/// 分數在打架，那種來回跳沒有收益、只有視覺干擾。
///
/// # 第一版判準寫錯了，記在這裡
///
/// 第一版比「落單字數」與「最長詞長度」，結果**把目標案例也擋掉**：
///
/// | 切法 | 形狀 |
/// |---|---|
/// | `想見`＋`立` | 一個雙字詞＋一個落單字 |
/// | `想`＋`建立` | 一個雙字詞＋一個落單字 |
///
/// **形狀完全一樣。** 差別在**哪一個字落單**——「想」單獨成詞很常見，
/// 「立」單獨出現在句尾很罕見。所以形狀分不出高下時，改比**落單字的
/// 字頻總和**：越高代表那種切法越自然。
///
/// 字頻直接問 `lm`——它建構時就收下了 `char_freq.txt`（當關聯強度的
/// 分母與台灣用字先驗），**不必再載第二份**。
fn more_complete(new: &[String], old: &[String]) -> bool {
    let (ns, nl) = word_shape(new);
    let (os, ol) = word_shape(old);
    // **不准把長詞拆短**。
    //
    // 少一個落單字不等於更好：`寄出去`＋`了`（落單 1、最長 3）被換成
    // `祭出`＋`去了`（落單 0、最長 2），落單數確實少了一個，但
    // 「祭出去了」是錯的。**長詞是強證據**——詞典收了「寄出去」這個
    // 三字詞，把它拆成兩個雙字詞需要比「落單少一個」更強的理由。
    if nl < ol {
        return false;
    }
    if ns < os || nl > ol {
        return true;
    }
    if ns != os {
        return false;
    }
    // **形狀一樣 → 比整句讀起來通不通順**（bigram 總分）。
    //
    // 這裡原本比「落單字的字頻總和」，用錯地方了：那個判準是為
    // 「同一個位置該讓誰落單」設計的，但兩種切法的落單字**根本不是
    // 同一批**，比總和沒有意義。實測擋掉了正解——
    //
    // ```text
    // 請勿｜必｜再騎｜現｜內｜完成   落單 必現內 = 23.57  ← 贏
    // 請｜務必｜再｜期限｜內｜完成   落單 請再內 = 21.72
    // ```
    //
    // 貪心的落單字剛好都是高頻字，於是「請勿必再騎現內完成」勝出。
    //
    // 改比**整條路的 bigram 總分**：同一句話的每一對相鄰字都算進去，
    // 衡量的是「這樣讀通不通」而不是「誰落單比較常見」。同一例
    // 43.66 對 34.27，差距很清楚。
    let Some(lm) = crate::lm::get() else {
        return false;
    };
    let path = |ws: &[String]| -> f32 {
        let joined: String = ws.concat();
        let cs: Vec<char> = joined.chars().collect();
        cs.windows(2)
            .map(|p| lm.score(p[0], p[1]).unwrap_or(LM_MISS))
            .sum()
    };
    // **不加「要贏多少分」的門檻**：掃過 0～8 都換不到東西
    // （0 → ⚠10、3 → ⚠11、8 → ⚠6 但漏斗掉 4 分）。
    //
    // 因為剩下的改寫**不是擺盪，是一次性的修正**：11 次裡有 6 次
    // 來自「這批貨物預計明天到達」那一句，而它最終是修對的
    // （打字中途從「慾記名天道」跳到正解，一次修好 3 個字就記 3 次）。
    // 加門檻只會連那種修正一起擋掉。
    path(new) > path(old)
}

/// 在一段連續注音格上用**詞格維特比**重新切詞。
///
/// 跟 `apply_word_context` 的貪心版差別只有一個：**整段一起解，
/// 所以每個詞的接續分數看得到左右兩邊**。貪心是由左往右、命中就跳，
/// 右邊還沒算到，因此左邊的詞會把右邊的字搶走——`想建立` 被切成
/// `想見`＋`立` 就是這樣來的（§2.74.1）。
fn lattice_words(slots: &[Slot], lo: usize, hi: usize) -> Option<Vec<String>> {
    let lm = crate::lm::get()?;
    let n = hi - lo;
    // **狀態是「走到第 i 格、最後一個字是什麼」**，不是只有「第 i 格」。
    //
    // 接續分數看的是前一個詞的最後一個字，所以只留每一格的最佳一條會把
    // 「現在稍輸、但接下去比較順」的路提早丟掉：
    //
    // - `那隻鳥`：走到第 2 格時 `那|支` 比 `那|隻` 好，只留一條的話 `隻`
    //   在看到「鳥」之前就被丟了，而 `隻鳥` 很常見、`支鳥` 根本查不到
    // - `目標|是|提升` 在第 6 格輸給 `目|標示`（差 0.85），後面 `是提`
    //   的接續分數就再也算不到了
    //
    // 每個（格, 末字）留一條才是真正的維特比。狀態數受每格的候選數
    // 限制（單字格最多 `LM_WIDTH` 個），實測逐鍵延遲量不出差別。
    // states[i] = [(總分, 從哪一格來, 前一個狀態的索引, 這個詞)]
    let mut states: Vec<Vec<(f32, usize, usize, String)>> = vec![Vec::new(); n + 1];
    states[0].push((0.0, 0, 0, String::new()));

    for end in 1..=n {
        for start in 0..end {
            if states[start].is_empty() {
                continue;
            }
            let len = end - start;
            let cands: Vec<String> = if len == 1 {
                candidates_for(&slots[lo + start])
                    .into_iter()
                    .filter(|w| w.chars().count() == 1 && w.chars().all(is_han))
                    .take(LM_WIDTH)
                    .collect()
            } else {
                let keys: String = slots[lo + start..lo + end]
                    .iter()
                    .map(|s| s.keys.as_str())
                    .collect();
                crate::dict::words_for(&keys)
                    .into_iter()
                    .filter(|w| w.chars().count() == len)
                    .map(|w| w.to_string())
                    .collect()
            };
            for (rank, w) in cands.iter().enumerate() {
                let cs: Vec<char> = w.chars().collect();
                // **手動選過的字不可以被覆蓋**——跟 `apply_word_context`
                // 同一條原則，使用者已經表態了
                let fits = (start..end).all(|j| {
                    !slots[lo + j].picked
                        || slots[lo + j]
                            .text
                            .chars()
                            .eq(std::iter::once(cs[j - start]))
                });
                if !fits {
                    continue;
                }
                let mut base = -RECUT_W_RANK * rank as f32;
                if len == 1 {
                    base -= RECUT_SINGLE_COST;
                }
                // **擴充包的詞一定贏**——它是使用者的明確表態，
                // 不該被 bigram 的統計推翻（理由見 `pack::zh_word`）。
                // 生造的專有名詞在 bigram 眼裡分數極低，不給這個加分
                // 的話包裡寫什麼都沒用（實測回報，§2.78）。
                //
                // **詞層跟重切兩處都要改**：只改詞層的話重切會再翻回去
                if len > 1 {
                    let keys: String = slots[lo + start..lo + end]
                        .iter()
                        .map(|s| s.keys.as_str())
                        .collect();
                    if crate::pack::zh_word(&keys).as_deref() == Some(w.as_str()) {
                        base += PACK_BONUS;
                    }
                }
                // 詞內部每一對相鄰的字
                for pair in cs.windows(2) {
                    if is_han(pair[0]) && is_han(pair[1]) {
                        base += lm.score(pair[0], pair[1]).unwrap_or(LM_MISS);
                    }
                }
                let last = *cs.last().unwrap();
                for pi in 0..states[start].len() {
                    let (prev_score, _, _, ref prev_word) = states[start][pi];
                    let mut s = prev_score + base;
                    // 跟左邊那個詞的接續
                    if let (Some(l), Some(&f)) = (prev_word.chars().last(), cs.first()) {
                        if is_han(l) && is_han(f) {
                            s += lm.score(l, f).unwrap_or(LM_MISS);
                        }
                    }
                    match states[end].iter().position(|st| st.3.ends_with(last)) {
                        Some(k) if states[end][k].0 >= s => {}
                        Some(k) => states[end][k] = (s, start, pi, w.clone()),
                        None => states[end].push((s, start, pi, w.clone())),
                    }
                }
            }
        }
    }
    let mut bi = 0usize;
    for (i, st) in states[n].iter().enumerate() {
        if st.0 > states[n][bi].0 {
            bi = i;
        }
    }
    states[n].get(bi)?;
    let mut out = Vec::new();
    let mut at = n;
    let mut idx = bi;
    while at > 0 {
        let (_, prev, pi, w) = states[at][idx].clone();
        out.push(w);
        at = prev;
        idx = pi;
    }
    out.reverse();
    Some(out)
}

/// **重新切詞**：修掉貪心搶字造成的錯（`想建立`→`想見立`）。
///
/// # 一律跑，靠閘門擋，不靠時機也不靠視野
///
/// 原本設計的閘門有三道**接進產品碼之後翻案**（§2.74.11），現在
/// 剩下的是：
///
/// 1. **套用：只在切得更完整時**（`more_complete`）——兩邊都是詞的
///    時候純粹是分數在打架，換來換去沒有收益
/// 2. **填回去時跳過 `picked`**（見 `recut_span`）——使用者表態過的
///    字不覆蓋
///
/// 拿掉的那三道與理由：
///
/// | 拿掉的 | 為什麼 |
/// |---|---|
/// | 只在剛收完一個字時跑（`just_settled`） | **它本身就是擺盪的來源**：不成字的按鍵不跑、畫面掉回貪心，下一鍵成字又跑，`在`↔`載` 每鍵來回。一律跑之後改寫 26 → 5 |
/// | 只重算離尾端 N 格（`RECUT_WINDOW`） | **比對的基準不是畫面上的東西**：窗口把前文擠出去之後，窗口內的貪心本來就對，閘門判定「不需要修」，可畫面上的錯是整句貪心造成的 |
/// | 只重切最後兩段（`RECUT_SPANS`） | **窗口本身在製造遠處改寫**：一段被推出窗口的那一鍵，重切的修正就掉回貪心的結果（`那隻鳥`→`那支鳥`、`清一下`→`青衣下`，打到後面第三段時才發生）。拿掉之後漏斗持平、改寫 `⚠` 11→10 |
///
/// 教訓寫在 §2.74.11：**穩定的規則比聰明的規則重要。**
///
/// # 為什麼現在可以每一段都切
///
/// §2.60 量過「掃全部段落讓改寫硬指標從 1 衝到 41」，那是
/// `just_settled` 還在的年代量的：跑／不跑交替，早就捲遠的段落跟著
/// 來回翻。拿掉它之後重切**只看這一段自己的按鍵**，按鍵沒變、結果
/// 就不變，所以打完的段落不會被翻案——被翻案的反而是被窗口推出去、
/// 從「重切過」掉回「貪心」的那一段。
///
/// 代價是**重切自己切錯的段落不會再「推出窗口就變回貪心」**。
/// 測資外的新句子量到一句：`這季的|anime…` 那一段重切的結果是
/// `這記得`（打完那三個字的當下就是，有窗口時也一樣），有窗口時
/// 它被推出去之後變回貪心的 `這季的`——最後的字對了，但那是一次
/// 遠處改寫；沒有窗口就一直是 `這記得`。錯在重切本身，不在範圍。
fn recut_spans(slots: &mut [Slot], by_word: &mut [(usize, usize)]) {
    let n = slots.len();
    // **每一段連續的中文都要重切，不是只有尾端那一段。**
    //
    // 只做尾端會漏掉「中間夾了空白或別的語言」的句子：
    // 「我想要建立一套 標準」的空格把它切成兩段，尾端那段只有
    // 「標準」（本來就對），而壞掉的「件立」在前一段——重切完全
    // 碰不到它。
    //
    // 前面的段落**已經被使用者看過**，但重切只看那一段自己的按鍵，
    // 按鍵沒變就切出一樣的結果，不會無故翻案（理由見上面的說明）。
    // 真正防止跳動的是 `more_complete`：切得更完整才套用。
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut at = 0usize;
    while at < n {
        if !slots[at].selectable || slots[at].lang != Language::Bopomofo {
            at += 1;
            continue;
        }
        let mut end = at;
        while end < n && slots[end].selectable && slots[end].lang == Language::Bopomofo {
            end += 1;
        }
        if end - at >= 2 {
            spans.push((at, end));
        }
        at = end;
    }
    // 只重切**最後 `RECUT_SPANS` 段**連續注音，不是全部——量測依據見
    // `RECUT_SPANS` 常數本身的說明。
    let n_spans = spans.len();
    let skip = n_spans.saturating_sub(RECUT_SPANS);
    for (lo, hi) in spans.into_iter().skip(skip) {
        recut_span(slots, lo, hi, by_word);
    }
}

/// `recut_spans`／`apply_fuzzy_tone` 只回頭處理**最後幾段**連續注音的
/// 範圍限制。
///
/// 曾經不設限（每一段都重切／模糊音修正），但那是在 `fuzzy_fixed`
/// 凍結機制之前的做法——凍結上線後，被推出這個範圍的段落不會再被
/// `recut_span`／`apply_fuzzy_tone` 重新推導，得靠 `Session::fuzzy_fixes`
/// 把上一輪修正過的字原樣填回去（`reapply_fuzzy_fixes`），所以範圍
/// 不必再設到整句。
///
/// 量測（測資 1464 句，基準 1381，`bad3` 是「三本／山本」那個
/// `fuzzy_span` 假陽性修好之後的基準）：
/// - `2`：漏斗持平，但**讓「那隻鳥」那個舊的窗口效應復發**（曾經修好
///   又壞掉），遠處改寫 ⚠10→12——太緊
/// - `3`：漏斗持平，⚠10→11
/// - `4`：漏斗 1381（跟不設限逐句相同、0 差異），⚠10→10（跟不設限
///   完全一致），而且測資外的新句子「這季的」不再被事後改寫錯
///   （不設限與 `4` 都需要靠 `fuzzy_fixed` 凍結才能不掉回錯字；`4`
///   額外把「這季的」也救回來）
///
/// 多段模糊音探針（29 組同音誤植詞、逐鍵打完接續句）：`2`／`3`／`4`／
/// 不設限全部 29/29 通過，凍結機制本身在哪個範圍都有效，差異只在
/// 「範圍外」的重切／改寫行為。**`4` 與不設限效果相同、但保留了未來
/// 想再收緊範圍的餘地**，因此定案為 `4`。
const RECUT_SPANS: usize = 4;

/// 在一段連續的中文格上重新切詞。範圍是 `[lo, n)`。
fn recut_span(slots: &mut [Slot], lo: usize, n: usize, by_word: &mut [(usize, usize)]) {
    // **整段一起重切，不設固定窗口。**
    //
    // 第一版限制「只看離尾端 N 格」，結果**修好的字會在下一鍵掉回去**：
    // 打完「我想要建立」重切修對了，再打「一」變成 6 格，窗口把「我」
    // 擠出去，重切只看得到 `想要建立一`——那一段貪心本來就是對的，
    // 閘門判定「不需要修」，可是畫面上的「件立」是**整句**貪心
    // （`我想`／`要件`／`立`）造成的。
    //
    // 病灶是**拿窗口內的貪心當基準，比對的卻不是實際顯示的東西**。
    // 連續注音段本來就有長度上限（超過就會被別的語言或標點打斷），
    // 而 §2.60.2 實測 lattice 的成本比想像中低（p99 8.4ms vs 8.3ms）
    // ——每一格的候選**詞**數遠小於候選**字**數。
    //
    // 防止「前文跳動」靠的是 `more_complete`（切得更完整才套用），
    // 不是靠縮小視野——**視野縮小反而是病因**，見 `recut_spans` 的
    // 說明表。
    let Some(new) = lattice_words(slots, lo, n) else {
        return;
    };
    let old = greedy_words(slots, lo, n);
    if !more_complete(&new, &old) {
        return;
    }
    // **整段填回去**，不是只填最後一個詞。
    //
    // 第一版只填最後一個詞（想防止來回跳），結果**修好的字保不住**：
    // 打完「我想要建立」是對的，再打「一」就變回「我想要件立一」
    // ——重切每次都從貪心的結果重算，而貪心每一鍵都會把「建立」重新
    // 算成「件立」，只填最後一個詞就等於把修正丟掉。
    //
    // 來回跳要靠**閘門**擋，不是靠縮小填寫範圍：`more_complete` 已經
    // 要求「切得更完整」，兩邊都是詞的擺盪（`紀`／`記`）本來就過不了
    // 那一關。
    for (i, c) in (lo..n).zip(new.iter().flat_map(|w| w.chars())) {
        // **手動選過的字不覆蓋**——使用者已經表態了。
        //
        // 這裡**刻意不擋 `fuzzy_fixed`**，跟 `apply_word_context`／
        // `apply_lm` 不一樣。理由：`recut_span` 是同一次 pipeline 裡跟
        // `apply_fuzzy_tone` 前後腳跑的重新推導（整段用 lattice 重算，
        // 不是照抄舊值），讓它照樣能贏才對得起「證據更強就採用」
        // （`more_complete`）這個閘門本身；擋住它反而會把 `apply_fuzzy_tone`
        // 的假陽性（單一鍵串的同音詞誤配，例如 `三本` 被模糊音誤配成
        // `山本`）凍死，沒有東西能再救回來。
        //
        // 跨按鍵的凍結（修正過的字不因為段落被推出 `RECUT_SPANS` 視野
        // 而掉回錯字）改在 `Session` 層做（`reapply_fuzzy_fixes`）：那是
        // 在**整條 pipeline 跑完之後**補回去，不影響這次 pipeline 內部
        // `recut_span` 能不能重新裁決。
        if !slots[i].picked {
            slots[i].text = c.to_string();
            // 這一格的字是 `recut_span` 重新裁決的，不再是
            // `apply_fuzzy_tone` 認可的那個字——旗標要跟著清掉，不然
            // 畫面上顯示的字跟凍結旗標所代表的內容對不上
            slots[i].fuzzy_fixed = false;
        }
    }
    // **重切換了詞界，`by_word` 要跟著換。**
    //
    // `apply_lm` 靠 `by_word` 判斷哪些格是詞層決定的（詞是強證據，
    // 統計不准動）。不跟著換的話它看到的還是**貪心的詞界**：重切拆出來
    // 的單字被當成詞鎖住，重切組出來的詞反而被當成單字放行，語言模型
    // 就把詞裡的字換掉——`被佔用` 重切成 `被｜佔用` 之後，貪心詞界沒
    // 更新，「被」被當成單字交給語言模型，換成了「備佔用」。
    //
    // 第二個值在詞層是「同讀音有幾個詞」，但現在沒有人讀它
    // （`lm_movable` 只看詞長），重切不去查詞典補這個數，填 1。
    let mut at = lo;
    for w in &new {
        let len = w.chars().count();
        for k in at..(at + len).min(n) {
            if k < by_word.len() {
                by_word[k] = if len > 1 { (len, 1) } else { (0, 0) };
            }
        }
        at += len;
    }
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
            // 手動選過、或模糊音修正凍結的字衝突
            let fits = |w: &str| {
                let cs: Vec<char> = w.chars().collect();
                cs.len() == stop - i
                    && (i..stop).all(|j| {
                        (!slots[j].picked && !slots[j].fuzzy_fixed)
                            || slots[j].text.chars().eq(std::iter::once(cs[j - i]))
                    })
            };
            let usable: Vec<_> = all.into_iter().filter(|w| fits(w)).collect();
            // **同讀音有好幾個詞時讓語言模型挑**，而不是取靜態詞頻的
            // 第一個——`救回來`／`就回來` 讀音相同，詞頻永遠讓前者贏。
            // 挑不出來（沒有模型、或分數相同）就退回第一個。
            let left = i.checked_sub(1).and_then(|j| slots[j].text.chars().last());
            // **擴充包的詞不讓語言模型挑掉**。
            //
            // 包是使用者的明確表態（「這串按鍵我就是要這個詞」），跟
            // `Slot::picked` 同一條原則。而生造的專有名詞在 bigram 眼裡
            // 分數極低，交給它挑一定輸給常見詞——包裡寫
            // `ㄊㄧㄢㄑㄧˋ → 屇鼜`，打出來還是「天氣」（實測回報，§2.78）。
            let from_pack = crate::pack::zh_word(&keys).filter(|w| fits(w));
            let chosen = from_pack
                .or_else(|| lm_pick_word(&usable, left).map(|w| w.to_string()))
                .or_else(|| usable.first().map(|w| w.to_string()));
            if let Some(word) = chosen {
                for (k, c) in word.chars().enumerate() {
                    // **手動選過、或模糊音凍結的字不覆蓋**
                    if !slots[i + k].picked && !slots[i + k].fuzzy_fixed {
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

/// 注音易混音的按鍵對照（大千配置，見 `bopomofo::keymap`）。
///
/// 每組都是**單字元替換**，所以「換一個音」就是換掉音節裡的一個字元。
///
/// 為什麼只有這四組（使用者裁決 2026-09-12）：原本列了七組，砍掉
/// `ㄢ/ㄤ`、`ㄈ/ㄏ`、`ㄋ/ㄌ`——**實際上不太有人搞混**，留著只是白白
/// 擴大誤傷面。實測砍完誤傷從 3 筆變 0 筆（開發文件 §2.81.11）。
/// 剩下的是「前後鼻音 ＋ 捲舌平舌」，後三組同一個成因（南部腔）。
const FUZZY_PAIRS: [(char, char); 4] = [
    ('p', '/'), // ㄣ / ㄥ  前後鼻音，台灣最常見
    ('5', 'y'), // ㄓ / ㄗ  捲舌／平舌
    ('t', 'h'), // ㄔ / ㄘ
    ('g', 'n'), // ㄕ / ㄙ
];

/// 模糊音修正的總開關，由平台層在套用設定時呼叫 `set_fuzzy_tone`。
///
/// **為什麼是全域旗標而不是參數**：`compose` 是純函式，四個公開入口
/// （`compose`／`compose_with`／`compose_with_bounds`／`compose_all`）
/// 都不收設定——加一層參數要動到所有呼叫端。而這個開關的語意跟
/// `learn::any()`／`pack::any()` 完全一樣：一個行程一份、設定變了才變。
/// 沿用同一套（`AtomicBool` + relaxed 讀）比開特例誠實。
///
/// 預設 `true`，跟 `config::Behavior::fuzzy_tone` 的預設一致——平台層
/// 還沒套用設定之前的行為要跟套用之後一樣，不然「設定頁沒動過卻跟
/// 實際行為不符」。
static FUZZY_TONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// 模糊音修正開著嗎？**熱路徑的第一道關卡**，一次 relaxed 原子讀。
fn fuzzy_enabled() -> bool {
    FUZZY_TONE.load(std::sync::atomic::Ordering::Relaxed)
}

/// 套用「注音打錯音自動修正」的設定。平台層讀完設定檔之後呼叫。
pub fn set_fuzzy_tone(on: bool) {
    FUZZY_TONE.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// 長輸出要不要打完就自動展開。**跟 `FUZZY_TONE` 同一個模式**——
/// `compose` 是純函式、四個入口都不收設定，而這個開關一個行程一份。
///
/// 預設 `true`，跟 `config::Behavior::auto_expand_long` 一致。
static AUTO_EXPAND_LONG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

fn auto_expand_long() -> bool {
    AUTO_EXPAND_LONG.load(std::sync::atomic::Ordering::Relaxed)
}

/// **測試用的序列化鎖**：凡是讀寫上面那兩個全域旗標的測試都要拿它。
///
/// 放在本體而不是某個 `mod tests` 裡——長輸出的測試散在 `compose` 與
/// `session` 兩個模組，各自一把擋不住對方（實際踩到過：症狀是隨機
/// 掛一條、每次不一樣）。
#[cfg(test)]
pub(crate) static GLOBAL_FLAGS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 套用「長輸出自動展開」的設定。平台層讀完設定檔之後呼叫。
pub fn set_auto_expand_long(on: bool) {
    AUTO_EXPAND_LONG.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// 打錯的音，用詞庫當證據反推回來。
///
/// 想打「今天」(`ru5wu0␣`) 卻打成 `ru/wu0␣`——「京天」詞庫裡沒有，
/// 把 `ㄥ` 換回 `ㄣ` 就查得到「今天」，那就是證據。
///
/// # 三道閘
///
/// 1. **原鍵串查得到詞就完全不觸發**。成本只在 miss 時付，而「心理／
///    行李」這種兩邊都成詞的永遠不會被動——那是對的，不觸發＝不會弄壞
/// 2. **一次只替換一個音節**。兩個以上是組合爆炸加誤判率
/// 3. **命中要唯一**。換第一個音節命中一個詞、換第二個又命中另一個，
///    兩個都不採用
///
/// # 範圍：每一段連續注音都做
///
/// 原本只碰最後兩段（跟重切共用一個常數），多段的句子裡**前面那段的
/// 修正會在它被推出去的那一鍵掉回錯字**——`新聞ok今天ok很好` 的「新聞」
/// 打成ㄒㄧㄥ，打到第三段時變回「星文」。跟重切拿掉窗口同一個道理
/// （見 `recut_spans`）：修正只看這一段自己的按鍵，按鍵沒變就不會翻，
/// 反而是窗口讓它翻。§2.74.6 也踩過同一型：固定窗口會讓比對的基準
/// 跟畫面不一致，修好的字在下一鍵掉回去。
///
/// `picked` 的格不動，跟其他所有改字的地方同一條原則。
fn apply_fuzzy_tone(slots: &mut [Slot], by_word: &[(usize, usize)]) {
    if !fuzzy_enabled() {
        return;
    }
    // 連續注音段的邊界，跟 `recut_spans` 的算法一致
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < slots.len() {
        if !slots[i].selectable || slots[i].lang != Language::Bopomofo {
            i += 1;
            continue;
        }
        let lo = i;
        while i < slots.len() && slots[i].selectable && slots[i].lang == Language::Bopomofo {
            i += 1;
        }
        if i - lo >= 2 {
            spans.push((lo, i));
        }
    }
    // 跟 `recut_spans` 用同一個 `RECUT_SPANS`、同一個理由——`fuzzy_fixed`
    // 記著的凍結會在 `Session::reapply_fuzzy_fixes` 補回被推出視野的
    // 段落，所以這裡限縮不會讓已修正的段落掉回錯字。範圍：只處理
    // **最後 `RECUT_SPANS` 段**連續注音，不是每一段都做。
    let n_spans = spans.len();
    let skip = n_spans.saturating_sub(RECUT_SPANS);
    for (lo, hi) in spans.into_iter().skip(skip) {
        fuzzy_span(slots, lo, hi, by_word);
    }
}

/// 在一段連續的中文格上做模糊音修正。範圍是 `[lo, hi)`。
///
/// # 只碰「詞層完全沒覆蓋到」的格
///
/// **這是這支最容易寫錯的地方，實測退步 4 句才抓出來。**
///
/// 直覺的寫法是「由左往右、每個位置查不到詞就做模糊展開」，但那會咬到
/// **跨詞邊界的切片**：「跟正式」的正確詞邊界是 `跟`＋`正式`，而 2 字窗
/// 「跟正」查不到詞、模糊展開卻撞得到「更正」——於是整句被改成
/// 「更正賜環境」。同類的還有 `帶傘出`→`帶閃出`、`他準備`→`他種備`、
/// `新辦公`→`興辦公`。
///
/// 所以要先讓**詞層**（`apply_word_context` 的貪心走法）把它能切的詞
/// 全部吃掉，模糊展開只在**剩下的、完全沒被任何詞覆蓋的格**上做。
/// 「跟正式」裡 `正式` 是詞、`跟` 是落單的單格，單格不做模糊展開
/// （一個音節沒有詞可以當證據，那正是 §2.81.5 說的「救不了單字」）。
fn fuzzy_span(slots: &mut [Slot], lo: usize, hi: usize, by_word: &[(usize, usize)]) {
    // 第一遍：標記哪些格被真正的詞覆蓋了。**兩種證據都算**：
    //
    // 1. `by_word`——`apply_word_context` 真正的判定（`words_for` 的
    //    候選集合交給 bigram／擴充包挑出來的），跟下游看到的是同一份
    //    事實。缺這個會漏掉「同讀音有競爭詞、bigram 已經選對」的格
    //    （例如 `words_for` 沒有唯一詞、字層各自填字頻第一名，但兩格
    //    合起來讀音就是一個詞）。
    // 2. **原鍵串本身查得到詞**（`word_for`，不做任何模糊變換）——
    //    這是本函式文件開頭「三道閘」的第一道：「原鍵串查得到詞就
    //    完全不觸發」。單靠 `by_word` 會漏掉這道閘：`apply_word_context`
    //    沒把它填進 `by_word`（詞層本身沒選中這個詞、字層各自填了
    //    字頻第一名），但 `fuzzy_fill` 的 `fuzzy_lookup` 拿同一段鍵去試
    //    「換一個音」，換出來的字串剛好也是個詞，於是誤觸發、蓋掉本來
    //    就對的字（`三本`／`山本`同讀音，`words_for` 給兩個詞、字層
    //    各自填字頻第一名成「三」「本」，`fuzzy_lookup` 拿原鍵去查照樣
    //    命中「山本」，把「三」蓋回「山」還凍住）。
    let mut covered = vec![false; hi - lo];
    for (k, c) in covered.iter_mut().enumerate() {
        if by_word.get(lo + k).is_some_and(|&(len, _)| len > 0) {
            *c = true;
        }
    }
    let mut i = lo;
    while i < hi {
        let max = 4.min(hi - i);
        let mut ate = 0;
        for n in (2..=max).rev() {
            let keys: String = slots[i..i + n].iter().map(|s| s.keys.as_str()).collect();
            if crate::dict::word_for(&keys).is_some() {
                ate = n;
                break;
            }
        }
        if ate > 0 {
            for k in 0..ate {
                covered[i - lo + k] = true;
            }
            i += ate;
        } else {
            i += 1;
        }
    }
    // 第二遍：在連續的「沒被覆蓋」區段上做模糊展開
    let mut i = lo;
    while i < hi {
        if covered[i - lo] {
            i += 1;
            continue;
        }
        // 這一段連續的空白有多長？
        let mut end = i;
        while end < hi && !covered[end - lo] {
            end += 1;
        }
        // 單格不做——一個音節沒有詞可以當證據（§2.81.5「救不了單字」）
        if end - i >= 2 {
            fuzzy_fill(slots, i, end);
        }
        i = end;
    }
}

/// 在一段「詞層完全沒覆蓋到」的格上試模糊展開。範圍是 `[lo, hi)`。
fn fuzzy_fill(slots: &mut [Slot], lo: usize, hi: usize) {
    let mut i = lo;
    while i < hi {
        let max = 4.min(hi - i);
        if max < 2 {
            break;
        }
        let mut done = 0;
        for n in (2..=max).rev() {
            let Some(word) = fuzzy_lookup(slots, i, n) else {
                continue;
            };
            let cs: Vec<char> = word.chars().collect();
            if cs.len() != n {
                continue;
            }
            // `picked` 的格不覆蓋；整組有任何一格被鎖住就整個不套用
            if (0..n).any(|k| slots[i + k].picked) {
                continue;
            }
            for (k, c) in cs.iter().enumerate() {
                slots[i + k].text = c.to_string();
                // 模糊音修正過的格凍住，跟 `picked` 同一條原則套用到
                // 重選／詞層／語言模型（但不進學習層，見
                // `Slot::fuzzy_fixed` 的說明）
                slots[i + k].fuzzy_fixed = true;
            }
            done = n;
            break;
        }
        i += if done > 0 { done } else { 1 };
    }
}

/// 這串按鍵**換一個易混的音**之後查得到詞嗎？
///
/// 給切點排序用（`rank::bopomofo_facts` 的 `covered`）。打錯音的注音段
/// 在詞庫裡查不到，`covered` 就是 0，於是**輸給把同一串當日文的切法**
/// ——`n;4au04`（上面，ㄕ 打成 ㄙ）被切成 `注:n;4|日:au|注:04`「喪合う案」，
/// 因為 `au` 是合法假名而且詞典查得到。
///
/// 只回答「有沒有」不回傳詞：這裡在熱路徑上，而排序只需要布林。
fn fuzzy_word_exists(keys: &str) -> bool {
    let Some(syls) = crate::bopomofo::split_syllables(keys) else {
        return false;
    };
    if syls.len() < 2 {
        return false;
    }
    for (si, syl) in syls.iter().enumerate() {
        for v in fuzzy_variants(syl) {
            let cand: String = syls
                .iter()
                .enumerate()
                .map(|(k, s)| if k == si { v.as_str() } else { s.as_str() })
                .collect();
            if crate::dict::is_bopomofo_word(&cand) {
                return true;
            }
        }
    }
    false
}

/// 給 `cutpoint::rank` 用的入口。開關關掉時直接回 `false`。
pub fn fuzzy_is_word(keys: &str) -> bool {
    fuzzy_enabled() && fuzzy_word_exists(keys)
}

/// 把 `[i, i+n)` 這幾格的按鍵做模糊展開，回傳**唯一**命中的詞。
///
/// 撞到兩個以上不同的詞就回 `None`——那時無法判斷使用者想打哪一個，
/// 猜錯的代價比不猜高。
fn fuzzy_lookup(slots: &[Slot], i: usize, n: usize) -> Option<String> {
    let syls: Vec<&str> = slots[i..i + n].iter().map(|s| s.keys.as_str()).collect();
    let mut hit: Option<String> = None;
    for (si, syl) in syls.iter().enumerate() {
        for v in fuzzy_variants(syl) {
            let keys: String = syls
                .iter()
                .enumerate()
                .map(|(k, s)| if k == si { v.as_str() } else { s })
                .collect();
            let Some(w) = crate::dict::word_for(&keys) else {
                continue;
            };
            match &hit {
                // 撞到第二個不同的詞 → 不唯一，整個放棄
                Some(prev) if *prev != *w => return None,
                Some(_) => {}
                None => hit = Some(w.to_string()),
            }
        }
    }
    hit
}

/// 一個音節的所有模糊變體（只換一個字元，不含自己）。
///
/// 只接受換完**仍是合法音節**的——`p`→`/` 可能把合法音節變成殘缺的
/// 東西，那種不必往下查詞庫。
fn fuzzy_variants(syl: &str) -> Vec<String> {
    let chars: Vec<char> = syl.chars().collect();
    let mut out = Vec::new();
    for (i, &c) in chars.iter().enumerate() {
        for &(a, b) in FUZZY_PAIRS.iter() {
            let to = if c == a {
                b
            } else if c == b {
                a
            } else {
                continue;
            };
            let mut v = chars.clone();
            v[i] = to;
            let v: String = v.iter().collect();
            if crate::bopomofo::validity(&v) == crate::bopomofo::syllable::Validity::Valid {
                out.push(v);
            }
        }
    }
    out
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
/// 太大會壓過詞層修正的結果，太小則吃不到收益。
///
/// # 掃過兩次，結論不一樣
///
/// 第一次（§2.56，790 句）掃 0.25 到 3.0，0.5 到 1.0 之間都落在 750 到
/// 754，當時判定不敏感、取 0.5。
///
/// 第二次（2026-09-23，1464 句）跟 `lm::MIN_FREQ`（bigram 分母的下限）
/// 一起掃，數字是對「0.5、沒有下限」的淨增減：
///
/// - 0.75 或 0.9，下限 20 或 50：**+4，弄壞 0**
/// - 0.6：+2
/// - 1.0，下限 50：+2（`讚`→`贊` 弄壞 2 句）
/// - 0.75，下限 100：+2（`上線`→`上限`）
/// - 0.75，不加下限：+1（`三本`→`三苯` 弄壞 3 句）
/// - 下限 50，權重維持 0.5：±0
///
/// **權重跟下限要一起動**：權重一調大，語言模型就更常推翻字頻，
/// 罕見字的 bigram 分數偏高（分母小）這個老毛病就冒出來，下限是
/// 擋它的。
///
/// # 這個數字的證據在測資內
///
/// 0.5 → 0.75 多過的句子全是那一輪點名要修的（`在哪`／`在那`、
/// `太舊`／`太就` 這類只差 0.1～0.3 分的），測資外另寫的 330 句中文
/// 是修 1 壞 1、淨 0。所以它只說得上「測資內在穩定區、測資外沒看到
/// 害處」，不是泛化的證據。要再調就拿更大批的測資外句子重掃。
///
/// 2026-09-25 又另寫 20 句（專挑靠上下文的同音字）再比一次：修 1（`帶筆電`，
/// 0.5 是 `代`）壞 1（`都快`，0.75 是 `都會`），又是淨 0。使用者裁決維持 0.75。
const LM_W_BIGRAM: f32 = 0.75;

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
        // **小數也算**（`1.69倍`、`2.5公斤`）：數字段本來就收得下小數點，
        // 右鄰的量詞跟整數一樣沒有 bigram 可用。小數點要夾在數字中間
        // ——`3.` 還在打、`.5` 不是數字的寫法。
        let t = &slots[i - 1].text;
        let prev_is_number = !t.is_empty()
            && t.chars().all(|c| c.is_ascii_digit() || c == '.')
            && t.chars().filter(|&c| c == '.').count() <= 1
            && t.starts_with(|c: char| c.is_ascii_digit())
            && t.ends_with(|c: char| c.is_ascii_digit());
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
        if !s.selectable || s.lang != Language::Bopomofo || s.picked || s.fuzzy_fixed || s.is_mark {
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
            && !s.fuzzy_fixed
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
    // 使用者手動選字蓋過模糊音凍結——選過了就是明確表態，比引擎猜的
    // 音更可信，而且 `picked` 已經接手凍結的效果。
    slots[idx].fuzzy_fixed = false;
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
pub(crate) mod tests {
    use super::*;
    use crate::cutpoint::incremental::Incremental;
    use crate::cutpoint::{normalize, rank};

    /// **`session::tests` 的長輸出那組也用這支**——它要測試包，
    /// 而外層 `session::tests::load()` 只載詞庫。
    pub(crate) fn load() -> bool {
        // ★ **真的只做一次** ★
        //
        // 下面那段註解一直寫著「載入只做一次」，但程式碼每次呼叫都重跑
        // `pack::load`——換索引的那一瞬間，別的測試看到的是半成品，
        // 症狀正是它自己警告的那個（隨機掛一條、每次不一樣）。
        // 2026-09-20 實際踩到，補上 `OnceLock` 讓註解成真。
        static ONCE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let ok = *ONCE.get_or_init(load_once);
        // ★ **每次確認索引還在** ★
        //
        // `OnceLock` 擋的是「重複做昂貴的載入」，但擋不住**別人把索引
        // 換掉**——2026-09-20 時 `pack` 有幾條測試自己 `load()` 換索引，
        // 換完之後這裡回 `true`（記得成功過）而索引其實是空的。
        // 判準是「**共用的那幾個包還在不在**」，不是「有沒有包」。
        //
        // **這道檢查只是最後一道防線，救不了半途的測試**：2026-09-23
        // 查到台語測試照樣隨機掛——通過這裡之後別人才換，而且換進來的
        // 包剛好也有長輸出（二進位端對端那條），這個檢查被騙過。
        // 所以現在的規則是：**測試裡只有這支會寫全域包索引**，而且
        // 每次寫的都是同一份。`pack` 自己的測試改用 `pack::build`
        // 建在手上查，`symbol` 的測試改走這支，換索引的收尾
        // （`reload_shared_packs`）也跟著刪了。
        if ok && !crate::pack::any_zh_long() {
            load_once();
        }
        ok
    }

    fn load_once() -> bool {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        crate::preload(&root.join("data"), crate::config::Engines::default());
        // 符號現在全在預載包裡（`packs/內建符號.txt`），不載就查不到。
        crate::pack::set_bundled_dir(Some(root.join("packs")));
        // **所有測試共用同一份包索引**。
        //
        // `cargo test` 預設並行，而包索引是全域的——某個測試自己換掉
        // 索引的話，別的測試會隨機看到它換的東西（CLAUDE.md §2.64.16：
        // 「測試隨機掛、每次不同那個 → 先想並行」）。修法是**載入只做
        // 一次**，不是把測試序列化。
        //
        // 使用者目錄指向 `testdata/packs`（進版控、內容釘死），
        // 不是本機的 `%APPDATA%`——不然結果會被使用者自己的包影響。
        let user_packs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/packs");
        crate::pack::load(
            user_packs.to_str().unwrap_or(""),
            &[
                crate::pack::BUNDLED_SYMBOLS.to_string(),
                // 釘住「包不被語言模型推翻」，見 `擴充包的詞不被語言模型挑掉`
                "lm_override".to_string(),
                // 釘住「兩個注音出一長串」，見 `包的長輸出併成一格`
                "zh_long".to_string(),
                // 釘住「同一串按鍵可以有多個輸出」，見 `包的同讀音多輸出`
                "zh_multi".to_string(),
                // 段選單的台語測試（`session::segmenu`）也吃這一份——
                // **索引只有一份，測試包就該只有一份**
                "測試台語".to_string(),
            ],
        );
        crate::dict::all_loaded()
    }

    /// 「會被全域旗標影響」的測試共用的序列化鎖。
    ///
    /// `cargo test` 預設並行，而 `set_fuzzy_tone` 動的是**全域**旗標
    /// ——關掉的那一瞬間，別的模糊音測試剛好在跑就會看到「沒修正」，
    /// 症狀是**隨機失敗、每次掛的不一樣**（CLAUDE.md §2.64.16 那個坑）。
    ///
    /// 判準是「`--test-threads=1` 跑得過就是它」。修法不是把測試全部
    /// 序列化，只把**真的共用那個狀態**的幾條收進同一把鎖。
    fn fuzzy_guard() -> std::sync::MutexGuard<'static, ()> {
        super::GLOBAL_FLAGS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
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

    /// 打錯的音用詞庫當證據反推回來（§2.81）。
    ///
    /// 案例都是實際跑過確認的，不是憑想像寫的——`ru/ wu0 ` 的正確鍵是
    /// `rup wu0 `（ㄐㄧㄣ），`ㄣ` 打成 `ㄥ` 就是 `p` → `/`。
    #[test]
    fn 模糊音修正打錯的音() {
        if !load() {
            return;
        }
        let _g = fuzzy_guard();
        for (typo, want) in [
            ("ru/ wu0 ", "今天"),
            ("vu/ jp6", "新聞"),
            ("n/ xup6", "森林"),
            ("e/ 1p3", "根本"),
        ] {
            assert_eq!(text_of(&slots_of(typo)), want, "{typo} 應該被修成{want}");
        }
    }

    /// **原鍵串查得到詞就完全不觸發**——第一道閘，也是效能的前提。
    ///
    /// 兩邊都成詞的組合打哪個就該出哪個，模糊音不可以插手。
    /// `5/ 2k7`（ㄓㄥ ㄉㄜ˙）查得到「爭得」，所以不會被修成「真的」
    /// ——**那是對的**，不觸發＝不會弄壞。
    #[test]
    fn 模糊音不碰本來就查得到的詞() {
        if !load() {
            return;
        }
        let _g = fuzzy_guard();
        // 「行李」與「心裡」讀音只差 ㄣ/ㄥ，兩邊都成詞
        assert_eq!(text_of(&slots_of("vu/6xu3")), "行李");
        assert_eq!(text_of(&slots_of("vup xu3")), "心裡");
        // 「真的」與「爭得」同理
        assert_eq!(text_of(&slots_of("5p 2k7")), "真的");
        assert_eq!(text_of(&slots_of("5/ 2k7")), "爭得");
    }

    /// 跨詞邊界的切片不可以被改——**這條擋的是實測退步 4 句的那個 bug**。
    ///
    /// 「跟正式」的詞邊界是 `跟`＋`正式`，而 2 字窗「跟正」查不到詞、
    /// 模糊展開卻撞得到「更正」，整句會變成「更正賜環境」。詞層先把
    /// `正式` 吃掉之後，剩下的第一格是落單的單格，單格不做模糊展開。
    ///
    /// 第一個字是「根」不是「跟」（`ep ` 一聲的字頻第一名），那是字頻
    /// 的事、跟模糊音無關——**這條測的是後兩個字沒被改成「更正」**。
    #[test]
    fn 模糊音不碰跨詞邊界的切片() {
        if !load() {
            return;
        }
        let _g = fuzzy_guard();
        let got = text_of(&slots_of("ep 5/4g4"));
        assert!(
            got.ends_with("正式"),
            "跨詞邊界被模糊音改掉了（該是 ?正式）：{got}"
        );
        assert!(
            !got.contains("更正"),
            "「跟正」被模糊展開撞成「更正」了：{got}"
        );
    }

    /// `fuzzy_span` 的 covered 判斷只看原鍵串（`word_for`）時的假陽性：
    /// 跟 `apply_word_context` 真正的判定（`words_for` 交給 bigram／
    /// 擴充包挑出來的）不是同一份事實，兩者對不上時模糊展開會誤觸發、
    /// 蓋掉已經選對的字。
    ///
    /// `ㄙㄢ ㄅㄣˇ`（`n0 1p3`）不是詞典收的詞，字層各自填字頻第一名
    /// 是「三」「本」（正確），但 `word_for` 拿原鍵去查查不到、
    /// `fuzzy_lookup` 試 ㄕ/ㄙ 這組易混音變成 `g0 1p3` 卻查得到
    /// 「山本」——舊的 covered 判斷只看 `word_for`，看不到這兩格其實
    /// 已經有更強的證據（這裡用手動填的 `by_word` 模擬「該格已經被
    /// `apply_word_context` 判定為詞的一部分」），於是照樣觸發模糊
    /// 展開、把「三」蓋成「山」。
    ///
    /// **驗收方式**：把 `fuzzy_span` 的 covered 判斷改回只看
    /// `word_for`（拿掉 `by_word` 那道證據），這條測試要紅。
    #[test]
    fn 模糊音不覆蓋已被詞層證據覆蓋的格() {
        if !load() {
            return;
        }
        let _g = fuzzy_guard();
        // 「山本」確實在詞典裡（`word_for("g0 1p3")` 查得到），
        // 這是模糊展開會誤觸發的真正原因
        assert_eq!(
            crate::dict::word_for("g0 1p3").as_deref(),
            Some("山本"),
            "前提：「山本」要在詞典裡，模糊展開才有東西可誤配"
        );
        // 「三本」本身不是詞典收的詞——字層只能各自填字頻第一名
        assert_eq!(
            crate::dict::word_for("n0 1p3"),
            None,
            "前提：「三本」不是詞，`word_for` 查不到"
        );
        // 手動組兩格，文字是字層字頻第一名會填的字（`apply_fuzzy_tone`
        // 只看 `keys`／`text`／`selectable`／`lang`，不必真的跑一次完整
        // pipeline）
        let mut slots = vec![
            Slot {
                keys: "n0 ".into(),
                text: "三".into(),
                lang: Language::Bopomofo,
                selectable: true,
                is_mark: false,
                cands: None,
                picked: false,
                fuzzy_fixed: false,
            },
            Slot {
                keys: "1p3".into(),
                text: "本".into(),
                lang: Language::Bopomofo,
                selectable: true,
                is_mark: false,
                cands: None,
                picked: false,
                fuzzy_fixed: false,
            },
        ];
        // 模擬 `apply_word_context` 已經判定這兩格是詞的一部分
        // （例如同讀音有競爭詞、bigram 已經選中「三」「本」這個組合）
        let by_word = vec![(2usize, 1usize), (2, 1)];
        apply_fuzzy_tone(&mut slots, &by_word);
        assert_eq!(
            slots[0].text,
            "三",
            "已被詞層證據覆蓋的格被模糊音誤配掉了（該留著「三」，\
             不該變成「山」）：{}",
            text_of(&slots)
        );
        assert_eq!(slots[1].text, "本");
        assert!(
            !slots[0].fuzzy_fixed && !slots[1].fuzzy_fixed,
            "沒有觸發模糊展開的格不該被標成 fuzzy_fixed"
        );
    }

    /// 手動選過的格不再被自動修正，**重打才解鎖**。
    ///
    /// 跟 `recut_span`／`apply_word_context`／`apply_lm` 同一條原則
    /// （`Slot::picked`）：使用者已經表態了，引擎不該自作聰明改回去。
    #[test]
    fn 模糊音不碰選過的格() {
        if !load() {
            return;
        }
        let _g = fuzzy_guard();
        // 沒選過 → 會被修正成「今天」
        let mut slots = slots_of("ru/ wu0 ");
        assert_eq!(text_of(&slots), "今天");

        // 使用者把第一格選成「京」（他真的想打「京」）
        let cands = crate::dict::chars_for(&slots[0].keys);
        let jing = cands
            .iter()
            .find(|c| *c == "京")
            .expect("ㄐㄧㄥ 應該有「京」");
        pick(&mut slots, 0, jing);
        assert!(slots[0].picked);
        // 再跑一次修正：選過的那格不可以被改回「今」
        // `by_word` 給全 0（沒有格被詞層覆蓋）——這裡在測 `picked`
        // 擋不擋得住，跟詞界判斷是兩件事
        let zero_by_word = vec![(0, 0); slots.len()];
        apply_fuzzy_tone(&mut slots, &zero_by_word);
        assert_eq!(
            slots[0].text, "京",
            "選過的格被模糊音改掉了——picked 沒有被尊重"
        );

        // 重打（重建 slots）→ 解鎖，又會被修正
        let fresh = slots_of("ru/ wu0 ");
        assert!(!fresh[0].picked, "重建的格不該帶著 picked");
        assert_eq!(text_of(&fresh), "今天", "重打之後應該回到自動修正");
    }

    /// 打錯音的注音段不可以輸給「把同一串當日文」的切法。
    ///
    /// `n;4au04`（上面，ㄕ 打成 ㄙ）原本被切成 `注:n;4|日:au|注:04`
    /// 「喪合う案」——因為打錯音之後詞庫查不到「上面」，`covered` 是 0，
    /// 而 `au` 是合法假名、詞典查得到，日文那條在 `covered`／`has_dict`／
    /// `dict_chars` 三欄全贏。修法是讓 `rank` 的 `covered` 也認模糊音
    /// （`fuzzy_is_word`）。
    #[test]
    fn 模糊音的段不輸給日文切法() {
        if !load() {
            return;
        }
        let _g = fuzzy_guard();
        assert_eq!(text_of(&slots_of("n;4au04")), "上面");
        assert_eq!(text_of(&slots_of("nji au/6")), "說明");
    }

    /// 總開關：關掉不修、開回來要能修（**切換要測兩個方向**）。
    #[test]
    fn 模糊音總開關兩個方向() {
        if !load() {
            return;
        }
        let _g = fuzzy_guard();
        set_fuzzy_tone(true);
        assert_eq!(text_of(&slots_of("ru/ wu0 ")), "今天", "開著的時候要修正");

        set_fuzzy_tone(false);
        let off = text_of(&slots_of("ru/ wu0 "));
        assert_ne!(off, "今天", "關掉之後不該還在修正");

        set_fuzzy_tone(true);
        assert_eq!(text_of(&slots_of("ru/ wu0 ")), "今天", "開回來之後要能修正");
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

    /// **常用英文詞、日文那邊只有冷僻讀音時，預設判英文**——但日文
    /// 讀法仍然要在候選裡（§2.24 的鏡像）。
    ///
    /// `youtube` 排名 10160，原本超過 `lang_of` 的「夠常用就不讓給日文」
    /// 門檻（5000），於是整段被判成日文（ようつべ 在 mozc 裡，但只是
    /// 冷僻詞條、不 confident）。拉高排名門檻救不了——`sushi` 排 7210、
    /// `karaoke` 排 9015，都比它前面，卻是有把握的日文讀音，不該被搶走。
    ///
    /// D-H3 把 `lang_of` 的判斷改看「日文那邊有沒有把握」
    /// （`is_confident_japanese`）而不是英文排名，`youtube` 因此變成
    /// 預設英文——這**推翻了 §2.24／§2.40.4「預設不變，英文只補在後面」
    /// 的決定**。使用者裁決保留（2026-09-25）：現在是「預設英文，
    /// ようつべ 仍在候選裡」，跟這篇測試原本鎖的方向相反。
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
        // **使用者裁決保留（2026-09-25）**：D-H3 之前這一格是 Romaji、
        // 英文補在候選裡；之後變成 English、日文讀法退到候選裡。兩種都
        // 合法（sushi、karaoke 這些有把握的日文讀音不受影響，見
        // incremental.rs 的 `常用英文詞遇上冷僻的日文讀音判英文` 測試）。
        assert_eq!(yt.lang, Language::English, "D-H3：預設英文");
        let cands = candidates_for(yt);
        assert_eq!(
            cands.first().map(String::as_str),
            Some("youtube"),
            "預設英文，第一名該是英文原文：{cands:?}"
        );
        assert!(
            cands.iter().any(|c| c == "ようつべ"),
            "日文讀法（ようつべ）仍要在候選裡，不能整個消失：{cands:?}"
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

    /// **B-H2b：單詞段裡 Viterbi 已經判定整段就是一個詞時，直接信它，
    /// 不必再套 `CONFIDENT_COST` 門檻**。
    ///
    /// 「単行本」在詞典裡是唯一表記，總成本落在 7400～8300（超過門檻），
    /// 但 Viterbi 對這一整段只切出這一個詞——多詞段從來不會套這道門，
    /// 單詞段沒理由標準不同。門檻本來是為了擋「多詞被誤當一詞」
    /// （`どうしよう`→「同仕様」，上面那條測試），這裡不是那個情況。
    ///
    /// **還原方式**：把 `words.first().map(...)` 那段改回一律呼叫
    /// `best_japanese`（也就是拿掉「Viterbi 選了就信它」那條路），
    /// 這條會紅（「単行本」維持假名 たんこうほん）。
    #[test]
    fn 單詞段含漢字就採用不套門檻() {
        if !load() {
            return;
        }
        assert_eq!(text_of(&slots_of("tannkouhonn")), "単行本");
    }

    /// **B-H2b 的另一半：越過門檻只在表記含漢字時才生效**——片假名
    /// 首選多半是同音的外來語／人名，越過門檻反而會把常見的語氣詞
    /// 變成片假名。
    ///
    /// **還原方式**：把 `.filter(|t| t.chars().any(...))` 那個漢字
    /// 過濾拿掉，這條會紅（`あらら`／`まあまあ` 變成片假名）。
    #[test]
    fn 單詞段片假名不越過門檻() {
        if !load() {
            return;
        }
        assert_eq!(
            text_of(&slots_of("arara")),
            "あらら",
            "片假名首選不該被硬轉"
        );
        assert_eq!(text_of(&slots_of("maamaa")), "まあまあ");
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

    /// 詞層由左往右貪心，左邊的詞會把右邊的字搶走。
    ///
    /// `想見`／`前件` 都是詞庫裡的真詞，命中就跳之後「建」被搶走、
    /// 「立」落單。單獨打「建立」本來就是對的——**病灶是前文**，
    /// **擴充包的詞不可以被語言模型挑掉**。
    ///
    /// 包是使用者的明確表態，跟 `Slot::picked` 同一條原則。生造的
    /// 專有名詞在 bigram 眼裡分數極低，不特別處理的話包裡寫什麼都
    /// 沒用——實測回報：包裡寫 `ㄊㄧㄢㄑㄧˋ → 屇鼜`，打出來還是
    /// 「天氣」（§2.78）。
    ///
    /// 包在 `core/testdata/packs/lm_override.txt`（進版控、內容釘死），
    /// 由共用的 `load()` 載進來——**測試自己不動全域狀態**，理由見
    /// `load()` 裡的長註解。
    #[test]
    fn 擴充包的詞不被語言模型挑掉() {
        if !load() {
            return;
        }
        // **兩字與三字各測一次**：詞層與維特比重切走的路不同，
        // 只改一處的話另一處會把它翻回去
        assert_eq!(text_of(&slots_of("wu0 fu4")), "屇鼜", "兩字：詞層被推翻");
        assert_eq!(
            text_of(&slots_of("wu0 fu61j4")),
            "酟耆㧫",
            "三字：重切被推翻"
        );
    }

    /// **包的長輸出併成一格**：兩個注音出一長串。
    ///
    /// 詞層是逐格填字的，字數對不上填不回去——所以長輸出走
    /// `merge_pack_long`，幾格併成一格（跟 `merge_symbols` 同一個模式）。
    ///
    /// 包在 `core/testdata/packs/zh_long.txt`（進版控、內容釘死），
    /// 由共用的 `load()` 載進來。
    #[test]
    fn 包的長輸出併成一格() {
        if !load() {
            return;
        }
        let _g = fuzzy_guard(); // 讀 AUTO_EXPAND_LONG，跟改它的那條共用鎖
                                // 兩個音節 → 六個字
        let slots = slots_of("ji3fu/3");
        assert_eq!(text_of(&slots), "我請你喝一杯");
        assert_eq!(slots.len(), 1, "六個字要在同一格裡，不是六格");

        // 一個音節 → 一長串。最短的輸入也要成立
        assert_eq!(text_of(&slots_of("sup6")), "您好，很高興認識您");
    }

    /// **同一串按鍵可以有多個輸出**（2026-09-20 使用者裁定）。
    ///
    /// 以前 `build_index` 用 `push_first`——第一條贏，同按鍵的後幾條
    /// **靜靜消失在檔案裡**（打開來看得到，實際上永遠不會生效）。
    /// 編輯器為此還要在新增時主動砍掉舊的那筆。
    ///
    /// 現在三條都留著：`zh_get` 仍然只回第一個（不動選字鍵的預設值），
    /// `zh_all` 回全部，選字時列得出來。
    ///
    /// 包在 `core/testdata/packs/zh_multi.txt`（進版控、內容釘死）。
    #[test]
    fn 包的同讀音多輸出() {
        if !load() {
            return;
        }
        // **按鍵從注音現算**，不寫死——手敲容易錯，而錯了只會得到
        // 空清單（看起來像功能壞掉，其實是測資打錯）
        let rev = crate::dict::reverse_keymap();
        let keys =
            crate::dict::symbols_to_keys("ㄙㄨㄛˇㄆㄧㄥˊ", &rev).expect("包裡那串注音要轉得出按鍵");
        let keys = keys.as_str();
        let all = crate::pack::index().zh_all(keys);
        assert_eq!(
            all,
            vec!["㪽玶", "㪽帡", "㪽軿"],
            "三條都要在，而且照包裡的順序"
        );
        // 預設值仍然是第一個——**不動選字鍵就不該變**
        assert_eq!(
            crate::pack::index().zh_get(keys),
            Some("㪽玶"),
            "zh_get 是預設值，只回第一個"
        );
        // 選字那條路（`dict::words_for`）要拿得到全部
        let words = crate::dict::words_for(keys);
        for w in ["㪽玶", "㪽帡", "㪽軿"] {
            assert!(
                words.iter().any(|x| x == w),
                "選字時要列得出 {w}：{words:?}"
            );
        }
    }

    /// **長輸出層也能多筆**，候選是「包給的全部 ＋ 原樣」。
    ///
    /// 實測回報的正是這一條：包裡三筆同按鍵，打字時只出得來第一個、
    /// 方向鍵切不過去。兩個原因疊在一起——`zh_long_get` 只取第一個，
    /// 而那一格 `selectable: false` 讓 `candidates_for` 第一行就回空。
    #[test]
    fn 長輸出多筆時全部列得出來() {
        if !load() {
            return;
        }
        let _g = fuzzy_guard(); // 讀 AUTO_EXPAND_LONG，跟改它的那條共用鎖
        let rev = crate::dict::reverse_keymap();
        let keys =
            crate::dict::symbols_to_keys("ㄆㄤˊㄗㄨㄟˋ", &rev).expect("那串注音要轉得出按鍵");
        let all = crate::pack::zh_long_all(&keys);
        assert_eq!(
            all,
            vec!["這是第一種長輸出", "這是第二種長輸出"],
            "兩筆都要在，照包裡的順序"
        );

        // 併格之後，選字層要拿得到「兩筆 ＋ 原樣」
        let slots = slots_of(&keys);
        assert!(slots[0].selectable, "要選得動");
        let got = candidates_for(&slots[0]);
        assert_eq!(got[0], "這是第一種長輸出", "第一個是預設值");
        assert!(
            got.iter().any(|x| x == "這是第二種長輸出"),
            "第二筆要列得出來：{got:?}"
        );
        assert_eq!(got.len(), 3, "兩筆長輸出 ＋ 一條原樣：{got:?}");
    }

    /// **長輸出不進學習層**（使用者裁定 2026-09-20）。
    ///
    /// 記進去之後學習層會把它釘成預設，而**關掉包也救不回來**——
    /// 污染已經寫進學習檔了。理由跟台語那條防線一樣。
    #[test]
    fn 長輸出不進學習層() {
        if !load() {
            return;
        }
        let _g = fuzzy_guard(); // 讀 AUTO_EXPAND_LONG，跟改它的那條共用鎖
        let mut slots = slots_of("ji3fu/3");
        assert_eq!(slots.len(), 1, "先併成一格");
        assert_eq!(slots[0].text, "我請你喝一杯");
        // 裝成使用者選過的樣子——`learn::record` 的門檻是有格子 `picked`
        slots[0].picked = true;
        assert_eq!(
            crate::learn::record(&slots),
            0,
            "長輸出那格該被跳過，一條都不記"
        );
    }

    /// **關掉自動展開就完全不併**（`auto_expand_long`，2026-09-20）。
    ///
    /// 單格與多格一視同仁——使用者裁定不區分，「自動展開」這件事的
    /// 語意是全有全無。
    #[test]
    fn 關掉自動展開就不併() {
        if !load() {
            return;
        }
        let _g = fuzzy_guard(); // 動全域旗標，跟模糊音那組共用一把鎖
        crate::compose::set_auto_expand_long(false);
        // 多格那條
        let many = slots_of("ji3fu/3");
        // 單格那條（`ㄋㄧㄣˊ` → 您好，很高興認識您）
        let one = slots_of("sup6");
        crate::compose::set_auto_expand_long(true);

        assert!(many.len() > 1, "關掉之後不該併成一格：{many:?}");
        assert_ne!(text_of(&many), "我請你喝一杯", "不該展開");
        assert_ne!(text_of(&one), "您好，很高興認識您", "單格也不該展開");
        // 開回來要照舊
        assert_eq!(text_of(&slots_of("ji3fu/3")), "我請你喝一杯", "開著照舊");
    }

    /// 長輸出那一格**要選得回原樣**（2026-09-20 使用者裁定）。
    ///
    /// 原本 `selectable: false`——理由是「一格的內容是整串文字而不是
    /// 一個字，落到詞庫查詢會拿到不知所云的候選」。那個顧慮仍然成立，
    /// 但擋法改成**保證 `cands` 算好**（`candidates_for` 的「算好的
    /// 優先」那條路先接住），而不是整格不給選——**被包換掉的格子要
    /// 選得回去**。
    #[test]
    fn 長輸出那格選得回原樣() {
        if !load() {
            return;
        }
        let _g = fuzzy_guard(); // 讀 AUTO_EXPAND_LONG，跟改它的那條共用鎖
        let slots = slots_of("ji3fu/3");
        assert!(slots[0].selectable, "要選得動，不然退不回原樣");
        let cands = slots[0].cands.as_ref().expect("候選要先算好");
        assert_eq!(cands[0], "我請你喝一杯", "第一個是包給的預設值");
        assert_eq!(cands.len(), 2, "這串只有一筆長輸出，加上原樣共兩條");
        assert_ne!(cands[1], cands[0], "第二條是原樣，不是重複的長輸出");
        // **選字層真的拿得到**——`candidates_for` 走「算好的優先」，
        // 不會落到詞庫查出不知所云的單字候選
        let got = candidates_for(&slots[0]);
        assert_eq!(got, *cands, "選字層拿到的就是算好的那份");
    }

    /// **最長優先**：同一組按鍵的前綴也在包裡時，長的那個要贏。
    ///
    /// `ㄍㄨㄥㄙ`（本公司）與 `ㄍㄨㄥㄙㄏㄤˊㄏㄠˋ`（一長串地址）都在
    /// 包裡，打完四個音節該出長的那個——跟詞層「從最長的連續段試起」
    /// 同一條原則。
    #[test]
    fn 長輸出最長優先() {
        if !load() {
            return;
        }
        let _g = fuzzy_guard(); // 讀 AUTO_EXPAND_LONG，跟改它的那條共用鎖
        assert_eq!(text_of(&slots_of("ej/ n ")), "本公司", "只打兩個音節");
        assert_eq!(
            text_of(&slots_of("ej/ n c;4cl4")),
            "台北市信義區信義路五段7號",
            "四個音節要吃掉前綴那條"
        );
    }

    /// 分流之後**一般的詞照舊**——長輸出層不該干擾詞層。
    ///
    /// 同一個包裡兩種條目都有，字數對得上的仍然走逐格填字那條路
    /// （所以是兩格、而且可以選字）。
    #[test]
    fn 長輸出不影響一般的詞() {
        if !load() {
            return;
        }
        let _g = fuzzy_guard(); // 讀 AUTO_EXPAND_LONG，跟改它的那條共用鎖
        let slots = slots_of("yl30 ");
        assert_eq!(text_of(&slots), "早安");
        assert_eq!(slots.len(), 2, "一般的詞仍然是一格一個字");
        assert!(slots[0].selectable, "一般的詞照舊可以選字");
    }

    /// 所以測試一定要帶前文，否則測不到東西（§2.74.1）。
    #[test]
    fn 貪心搶字_前文不該把詞拆掉() {
        if !load() {
            return;
        }
        for (keys, want) in [
            ("ru04xu4", "建立"),
            ("vu;3ru04xu4", "想建立"),
            ("fu06ru04xu4", "前建立"),
            // 前文越長越容易搶：「我想」吃掉「想要建立」的第一個字，
            // 連鎖成「我想｜要件｜立」
            ("ji3vu;3ul4ru04xu4", "我想要建立"),
            // 空格把句子切成兩段，壞掉的字在**前一段**
            // ——只重切尾端那段碰不到它（重切要涵蓋每一段的由來）
            ("ji3vu;3ul4ru04xu4u wl4 1ul 5jp3", "我想要建立一套 標準"),
            // 長詞不准被拆短：`寄出去`＋`了` 不該變成 `祭出`＋`去了`
            ("ru4tj fm4xk7", "寄出去了"),
            // §2.60.1 的原始案例：貪心的連鎖崩壞。閘門的最後一關
            // 原本比「落單字的字頻總和」，貪心的落單字（必現內）
            // 剛好都是高頻字，把正解擋掉了——改比整句 bigram 才過。
            //
            // **`再`／`在` 是已知的雙義**（§2.61.2 補量），靠學習層，
            // 所以期望寫「再」——這一筆守的是前面那四個字。
            ("fu/3j41u4y94fu6vu04so4j06t/6", "請務必再期限內完成"),
        ] {
            let got = text_of(&slots_of(keys));
            assert_eq!(got, want, "`{keys}` 應該是「{want}」");
        }
    }

    /// 逐鍵打的過程中，**已經定案的前綴不可以被翻案**。
    ///
    /// 這是重切最容易壞的地方，踩過三次不同的病因：
    ///
    /// 1. 只填最後一個詞 → 修好的字下一鍵掉回去
    /// 2. 用固定窗口 → 窗口把前文擠出去，比對的基準跟畫面不一致
    /// 3. 只在「剛收完一個字」時跑 → **跑／不跑交替本身就是擺盪**
    ///    （`在`↔`載` 每一鍵來回，一句 20 次遠處改寫）
    ///
    /// 4. 只重切最後兩段 → 被推出去的那段掉回貪心（見
    ///    `重切與模糊音涵蓋每一段`）
    ///
    /// 現在靠的是 `more_complete`（切得更完整才套用）。`紀`／`記` 兩個
    /// 都是詞、形狀也一樣，過不了那道閘門，所以不會擺盪。
    #[test]
    fn 重切不翻案已定案的前綴() {
        if !load() {
            return;
        }
        // 「昨天的會議記錄」——`紀`／`記` 兩個都合理，最會擺盪的一句
        let keys = "yji6wu0 2k7cjo4u4ru4xj4 ";
        let mut prev = String::new();
        let mut acc = String::new();
        for c in keys.chars() {
            acc.push(c);
            let now = text_of(&slots_of(&acc));
            // 只比「兩次都已經成字」的前綴，長度取短的那個減 2
            // （最後兩格是正在打的，本來就該變）
            let n = prev.chars().count().min(now.chars().count());
            if n >= 3 {
                let a: String = prev.chars().take(n - 2).collect();
                let b: String = now.chars().take(n - 2).collect();
                assert_eq!(a, b, "打到「{acc}」時前綴被翻案了");
            }
            prev = now;
        }
    }

    /// **A1：重切換了詞界，`by_word` 要跟著換**，語言模型才看得到新詞界。
    ///
    /// 貪心先切出「port｜備｜佔｜用」，`recut_span` 把它改成
    /// 「port｜被｜佔用」。如果 `by_word` 沒同步更新，它還記得貪心的
    /// 詞界：「被」被當成單字交給語言模型重挑，「佔用」反而因為貪心
    /// 詞層曾經給過 `(2,1)` 被鎖住不動——兩邊都錯，語言模型把「被」
    /// 換成同音字頻更高的「備」。
    ///
    /// **還原方式**：把 `recut_span` 結尾同步 `by_word` 的迴圈拿掉
    /// （改成 `let _ = by_word;`），這條會紅（「被佔用」變成「備佔用」）
    /// ——已經實際還原驗證過。
    #[test]
    fn 重切換詞界要同步_by_word() {
        if !load() {
            return;
        }
        assert_eq!(
            text_of(&slots_of("port 1o4504m/4")),
            "port 被佔用",
            "重切把「備佔用」改成「被佔用」之後，by_word 沒跟著換就會被語言模型翻回「備」"
        );
    }

    /// **A2：詞格維特比要留「每個末字各一條」，不能整格只留一條**。
    ///
    /// 「那隻鳥」在切到「那」那一格時，`那｜支`（詞頻較高）比
    /// `那｜隻` 分數高。如果每格只留最佳一條，`那｜隻` 這條路會在看到
    /// 「鳥」之前就被丟棄——即使加上「鳥」之後 `隻鳥` 的接續分數遠高於
    /// 查不到的 `支鳥`，也沒有機會翻盤。
    ///
    /// **還原方式**：把 `lattice_words` 的狀態從
    /// `Vec<Vec<(分數,起點,前一狀態,詞)>>`（每個末字一條）改回
    /// 「每格只留一個 `best`」，這條會紅（卡在「那支鳥」）。
    #[test]
    fn 詞格維特比每個末字各留一條路() {
        if !load() {
            return;
        }
        assert_eq!(
            text_of(&slots_of("ji3d04ru04s835 sul3")),
            "我看見那隻鳥",
            "只留一條路的話「那隻」會在看到「鳥」之前被「那支」擠掉"
        );
    }

    /// **A3：重切／模糊音要涵蓋每一段，不能只碰最後兩段**。
    ///
    /// 重切是逐段的純函數（一段的結果只看這一段的按鍵），所以拿掉窗口
    /// 之後前面已經修好的段落不會被翻案——真正會製造遠處改寫的是窗口
    /// 本身：一段被推出「只碰最後兩段」的範圍時，它會從「重切過」的
    /// 結果掉回貪心的結果。
    ///
    /// 這裡打「我看見那隻鳥」之後再接兩個以英文分隔的中文段
    /// （`respond`、`friday`），讓「那隻鳥」被推出兩段的範圍，確認
    /// 「隻」不會在後面幾段打完時掉回「支」。
    ///
    /// **還原方式**：把 `recut_spans`／`apply_fuzzy_tone` 的
    /// `for (lo, hi) in spans` 改回
    /// `for &(lo, hi) in spans.iter().rev().take(2).rev()`，這條會紅
    /// （「那隻鳥」會在打完 `friday` 之後掉回「那支鳥」）。
    #[test]
    fn 重切涵蓋每一段_不受窗口推擠影響() {
        if !load() {
            return;
        }
        let settled = "ji3d04ru04s835 sul3"; // 「我看見那隻鳥」剛好打完、已定案
                                             // 後面再接三個以英文分隔的中文段，第三段出現時「那隻鳥」那段
                                             // 應該已經被推出「只碰最後兩段」的窗口
        let rest = "respondsu3cl3fridayrup wu0 wu0 fu4";
        assert_eq!(text_of(&slots_of(settled)), "我看見那隻鳥", "前置條件");
        let mut acc = settled.to_string();
        for c in rest.chars() {
            acc.push(c);
            let now = text_of(&slots_of(&acc));
            let head: String = now.chars().take(6).collect();
            assert_eq!(
                head, "我看見那隻鳥",
                "打到「{acc}」時已經定案的「那隻鳥」不該被後面推出窗口而改變"
            );
        }
    }

    /// **A4a：語言模型權重調到 0.75，才壓得過字頻先驗**。
    ///
    /// 「在哪」與「在那」的 bigram 分數其實分得開，但權重 0.5 時會被
    /// 「那」字頻第一名的先驗蓋掉。這條測完整的選字流程（不是只比
    /// `Lm::score`），因為最終結果是 bigram、名次先驗、字頻先驗三者
    /// 加總後的結果，光看 bigram 分數看不出誰會贏。
    ///
    /// **還原方式**：把 `LM_W_BIGRAM` 改回 `0.5`，這條會紅（「在哪」
    /// 變成「在那」）——已經實際還原驗證過。
    #[test]
    fn 語言模型權重夠大才推翻字頻先驗() {
        if !load() {
            return;
        }
        // 週報 template 在哪
        assert_eq!(
            text_of(&slots_of("5. 1l4templatey94s83")),
            "週報template在哪",
            "「哪」的 bigram 分數贏「那」，但字頻先驗選「那」——\
             權重要夠大才翻得過來"
        );
    }

    /// **A4b：bigram 分母有下限 `ln(MIN_FREQ)`，權重調大後才不會被
    /// 罕見字的分數暴衝反咬**。
    ///
    /// 「苯」在教育部字頻表裡幾乎沒出現（分母很小），`三苯` 的關聯
    /// 強度公式（`次數/分母`）因此比常用字「本」的 `三本` 還高。
    /// 語言模型權重還是 0.5 時被字頻先驗壓得住，調到 0.75（A4a）就會
    /// 讓它冒出來，所以兩個常數要一起動。
    ///
    /// **還原方式**：把 `Lm::score` 的
    /// `self.log_freq(fb).max(MIN_FREQ.ln())` 改回
    /// `self.log_freq(fb)`（不設下限），這條會紅（「買了三本」變成
    /// 「買了三苯」）——已經實際還原驗證過。
    #[test]
    fn bigram分母下限擋住罕見字反咬() {
        if !load() {
            return;
        }
        // 買了 三 本
        assert_eq!(
            text_of(&slots_of("a93xk7n0 1p3")),
            "買了三本",
            "沒有下限的話「苯」的分母太小，分數會蓋過常用字「本」"
        );
    }

    /// **A5：量詞規則要認得小數，不是只認整數**。
    ///
    /// `apply_number_units` 判斷「前一格是不是數字」時，`1.69` 這種
    /// 帶小數點的寫法要算數字，右鄰的量詞（倍）才問得到這張表。
    ///
    /// **還原方式**：把 `prev_is_number` 的判準改回
    /// `t.chars().all(|c| c.is_ascii_digit())`（只認全數字），這條會紅
    /// （`1.69` 含小數點，判定不是數字，量詞表不會被問到，退回字頻挑到
    /// 「備」）。
    #[test]
    fn 小數後面也套用量詞規則() {
        if !load() {
            return;
        }
        assert_eq!(
            text_of(&slots_of("284m, 1.691o4")),
            "大約1.69倍",
            "1.69 含小數點，也該被量詞規則認出來"
        );
    }
}
