//! 候選切法的生成。
//!
//! 依 `語言辨識演算法(新).canvas` 的「切法模組」：
//!
//! > 僅有一種正解，**可實現組合皆須於候選中**
//!
//! `cut()` 只回傳一種切法（依規則挑出來的第一名）。這個模組窮舉
//! **所有可實現的切法**，讓「正解在不在候選裡」變成可回答的問題。
//!
//! # 為什麼這件事重要
//!
//! 它區分兩種完全不同的問題：
//!
//! | 現象 | 性質 | 怎麼解 |
//! |---|---|---|
//! | 正解不在候選裡 | **生成問題** | 要改切法的生成規則 |
//! | 正解在候選但排後面 | **排序問題** | 要改挑第一名的規則 |
//!
//! 兩者的修法完全不同，不先分清楚會白做工。

use super::{merge, prune, punct, space, Segment, SEPARATOR};
use crate::language::Language;
use crate::{bopomofo, romaji};

/// 一種完整的切法。
pub type Cutting = Vec<Segment>;

/// 每個區塊最多留幾種切法。
///
/// 區塊的切法數是指數成長的——17 個字元就有十萬種以上，全留會爆炸。
/// 因為展開是**照瀑布順序**（見 `block_cuttings`），砍掉的是離規則
/// 最遠的那些，所以這個數字不必大。
///
/// 實測（440 句測資）：30→88.9%、300→95.2%、600→95.9%、3000→96.6%
/// 之後就飽和。600 之後每多 0.7% 要多花一倍時間，取這裡當平衡點。
const BLOCK_LIMIT: usize = 600;

/// 窮舉一個區塊（不含空白與標點）的所有切法。
///
/// # 展開的順序照瀑布規則，不是照長度
///
/// 舊版是廣度優先（逐層展開每種長度）。那跟引擎的規則無關——
/// `cut()` 的字列組成是**貪婪＋瀑布**：每個位置先讓注音吃最長的
/// 合法段，吃不到問日文，都吃不到才落英文。
///
/// 逐層展開的後果：正解排在 110～140 名，而每個區塊只留 30 種，
/// 於是 44 句被誤判成「生成不出來」——其實 `cut()` 早就切對了，
/// 只是窮舉器沒把引擎自己走的那條路放進候選。
///
/// 現在改成**照瀑布順序展開**：每個分支記一個「偏離代價」，
/// 完全照規則走的代價是 0，排在最前面。代價小的先展開，所以
/// 就算砍到 30 種，砍掉的也是離規則最遠的那些。
fn block_cuttings(block: &[char], limit: usize) -> Vec<Cutting> {
    // 區塊內不含空白與標點，所以「邊界」只有區塊的頭尾——
    // 把區塊自己當成完整按鍵串傳給 `prune::keep` 即可。
    let block_str: String = block.iter().collect();
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;

    // heap 只用 (代價, 序號) 排序——`Segment` 不需要能比大小。
    // 序號讓同代價的分支照生成順序出來，結果才穩定。
    struct Branch {
        cost: usize,
        seq: usize,
        at: usize,
        cut: Cutting,
    }
    impl PartialEq for Branch {
        fn eq(&self, o: &Self) -> bool {
            (self.cost, self.seq) == (o.cost, o.seq)
        }
    }
    impl Eq for Branch {}
    impl Ord for Branch {
        fn cmp(&self, o: &Self) -> std::cmp::Ordering {
            (self.cost, self.seq).cmp(&(o.cost, o.seq))
        }
    }
    impl PartialOrd for Branch {
        fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(o))
        }
    }

    let mut heap: BinaryHeap<Reverse<Branch>> = BinaryHeap::new();
    let mut seq = 0usize;
    heap.push(Reverse(Branch {
        cost: 0,
        seq: 0,
        at: 0,
        cut: Vec::new(),
    }));

    let mut out: Vec<Cutting> = Vec::new();
    while let Some(Reverse(Branch {
        cost,
        at: i,
        cut: cur,
        ..
    })) = heap.pop()
    {
        if out.len() >= limit {
            break;
        }
        if i == block.len() {
            out.push(cur);
            continue;
        }

        // 這個位置照規則會怎麼吃？──注音最長 → 日文最長 → 英文一字元
        //
        // **英文只能一次吃一個字元**，跟 `cut()` 的 passthrough 一致。
        // 讓英文吃任意長度的話，`notebookru04au04...` 會生出
        // 「日:noteboo ＋ 英:kru04au04wakarimashita」這種一路吞到底的
        // 分支——那是 `cut()` 永遠不會走的路，卻佔滿了候選名額，
        // 把正解擠到第 19160 名。英文段是靠相鄰合併長回來的，不是
        // 在這裡一口吃出來的。
        //
        // 另外套 `prune::keep` 的兩條丟棄規則（單字母、英文須是詞），
        // 它們把候選數中位從 4480 砍到 48，而 440 句測資的正解全數存活。
        let mut options: Vec<(usize, Language)> = Vec::new();
        for len in (1..=(block.len() - i)).rev() {
            let seg: String = block[i..i + len].iter().collect();
            if !prune::keep(&block_str, block, i, i + len) {
                continue;
            }
            for lang in langs_of(&seg) {
                // 英文一次只吃一個字元（跟 `cut()` 的 passthrough 一致），
                // **但整段是英文詞的話可以一口吃下**。
                //
                // 少了這個例外，`ok沒問題` 的 `ok` 就永遠生不出來——
                // 它只能靠 `o`＋`k` 合併，而那兩個單字母被 `prune`
                // 判成殘渣砍掉了。兩條規則在這裡對撞。
                if lang == Language::English && len > 1 && !crate::english::is_word(&seg) {
                    continue;
                }
                options.push((len, lang));
            }
        }
        // 依瀑布排序：注音優先、同語言長的優先
        options.sort_by_key(|(len, lang)| {
            let rank = match lang {
                Language::Bopomofo => 0,
                Language::Romaji => 1,
                Language::English => 2,
            };
            (rank, Reverse(*len))
        });

        if options.is_empty() {
            // 什麼都吃不下 → 英文 passthrough，一次一個字元（沒有別條路，不加代價）
            let mut next = cur.clone();
            next.push(Segment {
                keys: block[i].to_string(),
                is_mark: false,
                lang: Language::English,
            });
            seq += 1;
            heap.push(Reverse(Branch {
                cost,
                seq,
                at: i + 1,
                cut: next,
            }));
            continue;
        }

        // 第 0 個選項是規則本身的選擇（代價 +0），其餘依偏離程度加代價
        for (n, (len, lang)) in options.into_iter().enumerate() {
            let seg: String = block[i..i + len].iter().collect();
            let mut next = cur.clone();
            next.push(Segment {
                keys: seg,
                is_mark: false,
                lang,
            });
            seq += 1;
            heap.push(Reverse(Branch {
                cost: cost + n,
                seq,
                at: i + len,
                cut: next,
            }));
        }
    }
    out
}

/// 這一段可以是哪些語言？
///
/// **一段可能同時屬於多個語言**——`sushi` 既是合法日文（すし）也是
/// 英文詞。兩種都要進候選，由排序決定誰在前面。
fn langs_of(seg: &str) -> Vec<Language> {
    let mut out = Vec::new();
    if bopomofo::validity(seg) == bopomofo::Validity::Valid {
        out.push(Language::Bopomofo);
    }
    if romaji::validity(seg) == romaji::Validity::Valid {
        out.push(Language::Romaji);
    }
    // 英文：純字母就算（passthrough，不查詞典——查詞典是規則三的事）
    if !seg.is_empty() && seg.chars().all(|c| c.is_ascii_alphanumeric()) {
        out.push(Language::English);
    }
    out
}

/// 窮舉整串的所有切法。
///
/// # 空白的兩種可能都要窮舉
///
/// 空白可能是分隔符（自成一段），也可能是注音的一聲（併入前一段）。
/// **兩種都要進候選**——只窮舉一種的話，正解可能永遠不在裡面。
///
/// `limit` 是候選數上限，避免長句組合爆炸。
pub fn all_cuttings(keys: &str, limit: usize) -> Vec<Cutting> {
    let chars: Vec<char> = keys.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }

    // 把按鍵串拆成「區塊」與「標記」（空白／標點）。
    //
    // # 規則的順序：先收尾，再切點
    //
    // 「注音一律以聲調鍵收尾」在「空白鍵前後均為切點」**之前**。
    // 也就是說：空白先讓前面的注音音節收尾，收不了的才是分隔符。
    //
    // 順序反了會出事——`rup␣wu0␣wu0␣fu4...`（今天天氣不錯）若先照
    // 空白切，會變成 `rup | ␣ | wu0 | ␣ | ...`，而 `rup`、`wu0` 缺
    // 聲調都判非法，整串純注音反而湊不回來。
    //
    // 先收尾就沒這問題：每個空白都被前面的音節吃掉，整串是一個注音區塊。
    enum Piece {
        Block(Vec<char>),
        Space,
        Punct(char),
    }
    let mut pieces: Vec<Piece> = Vec::new();
    let mut cur: Vec<char> = Vec::new();
    for (n, &c) in chars.iter().enumerate() {
        if c == ' ' {
            // 這個空白能不能讓 `cur` 尾端的注音音節收尾？
            //
            // 用 `tone_suffix_start`：區塊前面可能還有別的語言
            // （`configg6ru0` 的 `config`），拿整塊問 `is_tone`
            // 一定是 false。收得了就把空白吃進區塊，不切。
            let cur_str: String = cur.iter().collect();
            if !cur.is_empty() && space::tone_suffix_start(&cur_str).is_some() {
                cur.push(' ');
                continue;
            }
            if !cur.is_empty() {
                pieces.push(Piece::Block(std::mem::take(&mut cur)));
            }
            pieces.push(Piece::Space);
        } else if punct::is_punct(keys, n) {
            if !cur.is_empty() {
                pieces.push(Piece::Block(std::mem::take(&mut cur)));
            }
            pieces.push(Piece::Punct(c));
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        pieces.push(Piece::Block(cur));
    }

    let mut combos: Vec<Cutting> = vec![Vec::new()];
    for p in &pieces {
        let mut next: Vec<Cutting> = Vec::new();
        match p {
            Piece::Block(b) => {
                let bc = block_cuttings(b, BLOCK_LIMIT);
                for c in &combos {
                    for one in &bc {
                        let mut m = c.clone();
                        // **相鄰的同語言段要合併**（詞列組成）。
                        //
                        // `rup␣wu0␣wu0␣fu4...`（今天天氣不錯）被空白拆成
                        // 四個區塊，第一個吸收空白變成注音段 `rup␣`，
                        // 但下一個區塊是獨立生成的——不合併的話，整串
                        // 注音永遠湊不回來，正解就不在候選裡。
                        for (idx, seg) in one.iter().enumerate() {
                            let can_merge = idx == 0
                                && !seg.is_mark
                                && m.last().is_some_and(|l| l.lang == seg.lang && !l.is_mark);
                            if can_merge {
                                m.last_mut().unwrap().keys.push_str(&seg.keys);
                            } else {
                                m.push(seg.clone());
                            }
                        }
                        next.push(m);
                        if next.len() >= limit {
                            break;
                        }
                    }
                    if next.len() >= limit {
                        break;
                    }
                }
            }
            Piece::Punct(ch) => {
                for c in &combos {
                    let mut m = c.clone();
                    m.push(Segment {
                        keys: ch.to_string(),
                        is_mark: true,
                        lang: Language::English,
                    });
                    next.push(m);
                }
            }
            Piece::Space => {
                // 兩種可能都展開
                for c in &combos {
                    // 可能一：分隔符，自成一段
                    let mut a = c.clone();
                    a.push(Segment {
                        keys: SEPARATOR.to_string(),
                        is_mark: true,
                        lang: Language::English,
                    });
                    next.push(a);

                    // 可能二：被前一段吸收當聲調
                    //
                    // `rup` + 空白 = ㄐㄧㄣˉ（今）。
                    //
                    // **不能要求前一段的 lang 是 Bopomofo**——`rup`
                    // 缺聲調，`langs_of` 會判它不是注音（那正是規格：
                    // 注音一律以聲調收尾），於是它的 lang 是 English。
                    // 要問的是 `space::is_tone`，它允許最後一段缺聲調。
                    if let Some(last) = c.last() {
                        if !last.is_mark && space::is_tone(&last.keys) {
                            let mut b = c.clone();
                            let seg = b.last_mut().unwrap();
                            seg.keys.push(' ');
                            // 吸收了聲調，這一段就是注音了
                            seg.lang = Language::Bopomofo;
                            next.push(b);
                        }
                    }
                    if next.len() >= limit {
                        break;
                    }
                }
            }
        }
        combos = next;
        if combos.is_empty() {
            break;
        }
    }

    // 每種切法都套用三條特殊規則，再去重
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for c in combos {
        let m = merge::apply(c);
        let key: Vec<(String, Language)> = m.iter().map(|s| (s.keys.clone(), s.lang)).collect();
        if seen.insert(key) {
            out.push(m);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contains(cands: &[Cutting], expect: &[&str]) -> bool {
        cands
            .iter()
            .any(|c| c.len() == expect.len() && c.iter().zip(expect).all(|(s, e)| s.keys == *e))
    }

    #[test]
    fn 候選裡有正解_純注音() {
        let c = all_cuttings("su3cl3", 500);
        assert!(contains(&c, &["su3cl3"]), "整段注音要在候選裡");
    }

    #[test]
    fn 候選裡有正解_英文接注音() {
        // check␣一下：期望 check | ␣ | u␣vu84
        let c = all_cuttings("check u vu84", 2000);
        assert!(
            contains(&c, &["check", " ", "u vu84"]),
            "候選數={}",
            c.len()
        );
    }

    #[test]
    fn 候選裡有正解_無空白的語言轉換() {
        // ok沒問題：期望 ok | ao6jp4wu6
        //
        // **要先載入詞庫**：`ok` 是英文詞才能一口吃下。沒詞庫時它只能
        // 靠 `o`＋`k` 合併，而那兩個單字母會被 `prune` 當殘渣砍掉。
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data");
        if crate::english::load(&data).is_empty() {
            eprintln!("詞庫未下載，跳過（跑 data/download.ps1）");
            return;
        }
        let c = all_cuttings("okao6jp4wu6", 3000);
        assert!(contains(&c, &["ok", "ao6jp4wu6"]), "候選數={}", c.len());
    }

    #[test]
    fn 空白的兩種可能都在候選裡() {
        // rup␣wu0␣（今天）：空白是一聲，不該切
        let c = all_cuttings("rup wu0 ", 500);
        assert!(contains(&c, &["rup wu0 "]), "一聲版本要在");
    }

    #[test]
    fn 一段可以同時是多個語言() {
        // sushi 既是日文（すし）也是英文詞
        let langs = langs_of("sushi");
        assert!(langs.contains(&Language::Romaji));
        assert!(langs.contains(&Language::English));
    }

    #[test]
    fn 空字串沒有候選() {
        assert!(all_cuttings("", 10).is_empty());
    }
}
