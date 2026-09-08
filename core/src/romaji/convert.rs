//! 假名 → 漢字的**整句轉換**：分詞與選字一起做。
//!
//! 設計依據是[開發文件 §2.23]，這裡記實作要點。
//!
//! # 為什麼分詞和選字不能分開
//!
//! 分法決定字，字又反過來決定分法：
//!
//! ```text
//! ごはん   → 一個詞「ご飯」
//! ご|はん  → 兩個詞「五|半」
//! ```
//!
//! 所以要在**所有「分詞 × 選字」的組合**裡一次找出總成本最低的路。
//! 那是一條 DP：`best[j]` ＝ 前 `j` 個假名的最小總成本。
//!
//! # 成本從哪來
//!
//! mozc 的資料本來就有：`讀音 ⟨左id⟩ ⟨右id⟩ ⟨詞成本⟩ 表記`。
//! 一條路的成本是
//!
//! ```text
//! Σ 詞成本 ＋ Σ 接續矩陣[前一個詞的 rid][這個詞的 lid]
//! ```
//!
//! 那個矩陣（2672×2672）**就是文法結構**——「名詞→を→動詞」便宜、
//! 「助詞→助詞」貴。句首與句尾用 id 0（BOS/EOS）。
//!
//! # 這不是被否決過的那個維特比
//!
//! 開發文件 §2.15 說「維特比 DP ＋ 長度先驗被實測否決」——**那是中文
//! 的實驗**，用語料詞頻當成本，單字累積量太大、沒有分詞壓力。日文用的
//! 是 mozc 訓練好的成本模型，不是同一件事。
//!
//! [開發文件 §2.23]: ../../../開發文件.md

use crate::dict;

/// 一個詞：讀音那一段、選中的表記。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    /// 這個詞佔的假名
    pub kana: String,
    /// 選中的表記（查不到詞典時就是假名本身）
    pub surface: String,
}

/// 一個詞最長幾個假名。
///
/// 沒有上限的話 DP 是 O(n²) 次查表；日文的詞很少超過這個長度，
/// 而上限讓長句的成本回到接近線性。
const MAX_WORD_KANA: usize = 16;

/// 查不到詞典的那一段，每個假名要罰多少。
///
/// **要夠貴，但不能貴到無限**：太便宜的話引擎會傾向「整句都當成
/// 查不到」（那樣接續成本是 0）；無限的話遇到詞典沒收的專有名詞
/// 就整句轉不出來。
const UNKNOWN_PER_KANA: u32 = 8000;

/// BOS／EOS 的 id。mozc 的慣例是 0。
const BOS_EOS: u16 = 0;

/// DP 的一格。
#[derive(Clone)]
struct Node {
    cost: u32,
    /// 從哪一個位置接過來
    from: usize,
    /// 從 `best[from]` 的第幾個節點接過來——**回溯要跟著同一條路**
    from_k: usize,
    /// 這一段選的表記
    surface: String,
    /// 這一段的右 id——下一段要用它算接續
    rid: u16,
}

/// 一個位置最多留幾個節點（beam）。
///
/// 留一批是為了保住「中途比較貴、但接得下去」的路，但同一個位置的
/// 候選可能有幾十個（`なけれ` 就有 7 個），全留會讓 DP 從 O(n·len)
/// 變成 O(n·len·beam)。
///
/// # 為什麼是 8
///
/// | BEAM | 超長句耗時 | 漏斗 | 改寫 |
/// |---|---|---|---|
/// | 4 | 353.8 µs | 1246 | 208 |
/// | **8** | **380.1 µs** | **1245** | **197** |
/// | 16 | 389.7 µs | 1245 | 191 |
///
/// **耗時對這個值幾乎不敏感**（4→16 只差 10%）——同 rid 去重之後實際
/// 留下來的節點本來就沒幾個，很少撞到上限。所以取捨不在效能，在品質：
/// 4 多賺 1 句但改寫更多，16 改寫最少卻沒多賺。8 是「再大也不會更好」
/// 的最小值。
const BEAM: usize = 8;

/// 把節點放進某個位置的候選堆，**同 rid 只留最便宜的**，並限制 beam。
///
/// 同 rid 的節點對後面完全等價（接續成本只看 rid），留多個是白費。
fn push_node(slot: &mut Vec<Node>, node: Node) {
    if let Some(old) = slot.iter_mut().find(|x| x.rid == node.rid) {
        if node.cost < old.cost {
            *old = node;
        }
        return;
    }
    slot.push(node);
    if slot.len() > BEAM {
        // 最貴的那個出局
        if let Some(worst) = slot
            .iter()
            .enumerate()
            .max_by_key(|(_, x)| x.cost)
            .map(|(i, _)| i)
        {
            slot.swap_remove(worst);
        }
    }
}

/// 把一串假名轉成詞。**分詞與選字一起決定**。
///
/// **沒有接續矩陣就不分詞**，回傳單一一個「原樣」的詞——那是原本的
/// 行為（整段假名）。理由見函式內的說明：沒有矩陣時這個成本模型
/// 是壞的，寧可不轉也不要轉錯。
pub fn convert(kana: &str) -> Vec<Word> {
    let chars: Vec<char> = kana.chars().collect();
    let n = chars.len();
    if n == 0 {
        return Vec::new();
    }
    // 每個字元的位元組位移，切子字串時用——**不要每次都 collect**
    let mut offs = Vec::with_capacity(n + 1);
    let mut acc = 0usize;
    offs.push(0);
    for c in &chars {
        acc += c.len_utf8();
        offs.push(acc);
    }

    // **沒有矩陣就不要分詞**。
    //
    // 原本這裡寫著「載不到不影響，退化成只看詞成本，仍然比整段不轉
    // 好」——**那句話是錯的，實測打臉**：mozc 讓高頻詞的詞成本趨近零
    // （`目` 是 12、`に` 是 0），沒有接續成本擋著的話，DP 會把 `あにめ`
    // 切成 `あに`＋`目` 這種垃圾。整段不轉都比那個好。
    let Some(conn) = dict::connection() else {
        return vec![Word {
            kana: kana.to_string(),
            surface: kana.to_string(),
        }];
    };
    // **每個位置留一批節點，不是只留最便宜的那一個**。
    //
    // 接續成本是 `cost(前一段的 rid, 這一段的 lid)`，所以「走到位置 i
    // 最便宜的那條路」不足以決定後面——rid 不同，後面的接續成本就不同。
    // 只留一個的話會這樣：
    //
    // ```text
    // なければ 單獨   → なければ            ✓（假名整串最便宜）
    // しなければ      → し | 泣けれ | ば    ✗
    // ```
    //
    // 「泣けれ」在 `なけれ` 這個位置比「なけれ」便宜（動詞詞成本低），
    // 於是整串假名那條路在中途就被丟掉，後面再也接不回來。留一批就
    // 保得住——`ば` 接在助動詞後面很便宜、接在動詞仮定形後面很貴，
    // 到終點自然分得出高下。
    let mut best: Vec<Vec<Node>> = vec![Vec::new(); n + 1];
    best[0].push(Node {
        cost: 0,
        from: 0,
        from_k: 0,
        surface: String::new(),
        rid: BOS_EOS,
    });

    for i in 0..n {
        if best[i].is_empty() {
            continue;
        }
        let prevs = best[i].clone();
        for len in 1..=MAX_WORD_KANA.min(n - i) {
            let j = i + len;
            let part = &kana[offs[i]..offs[j]];
            let cands = dict::cands_for_kana(part);

            // 詞典查得到：每個候選 × 每個前驅各試一次
            for c in cands.iter() {
                let Some((k, cost)) = prevs
                    .iter()
                    .enumerate()
                    .map(|(k, p)| (k, p.cost + conn.cost(p.rid, c.lid) as u32 + c.cost as u32))
                    .min_by_key(|(_, cost)| *cost)
                else {
                    continue;
                };
                push_node(
                    &mut best[j],
                    Node {
                        cost,
                        from: i,
                        from_k: k,
                        surface: c.surface.to_string(),
                        rid: c.rid,
                    },
                );
            }

            // 查不到就當「原樣的一段」。**只試長度 1**——多長的未知段
            // 都可以由一串長度 1 疊出來，試每種長度是白花的。
            if len == 1 {
                let (k, cost) = prevs
                    .iter()
                    .enumerate()
                    .map(|(k, p)| (k, p.cost + UNKNOWN_PER_KANA))
                    .min_by_key(|(_, cost)| *cost)
                    .unwrap_or((0, UNKNOWN_PER_KANA));
                push_node(
                    &mut best[j],
                    Node {
                        cost,
                        from: i,
                        from_k: k,
                        surface: part.to_string(),
                        // 未知詞的 id 不知道，用 BOS/EOS——它對任何東西
                        // 的接續成本都是中性的
                        rid: BOS_EOS,
                    },
                );
            }
        }
    }

    // **句尾接續在這裡才算**——終點有一批節點，加上各自的 EOS 成本
    // 才知道誰真的最好。這是留一批的另一半：`ば` 結尾接得漂亮的那條
    // 路，要到這一步才贏得了中途比較便宜的那條。
    let Some(k_end) = (0..best[n].len()).min_by_key(|&k| {
        let node = &best[n][k];
        node.cost + conn.cost(node.rid, BOS_EOS) as u32
    }) else {
        return vec![Word {
            kana: kana.to_string(),
            surface: kana.to_string(),
        }];
    };

    // 回溯——**跟著 `from_k` 走同一條路**
    let mut out: Vec<Word> = Vec::new();
    let mut j = n;
    let mut k = k_end;
    while j > 0 {
        let node = best[j][k].clone();
        let i = node.from;
        out.push(Word {
            kana: kana[offs[i]..offs[j]].to_string(),
            surface: node.surface,
        });
        if i == j {
            break;
        }
        j = i;
        k = node.from_k;
    }
    out.reverse();
    // **相鄰的未知段併回去**：`ぷ` `ろ` `ぐ` 三個各自一格沒有意義，
    // 使用者看到的應該是一整段沒轉換的假名
    merge_unknown(out)
}

/// 照**使用者指定的詞界**切，每一段各自挑最好的表記。
///
/// # 為什麼需要它
///
/// Viterbi 只能給「詞典查得到的」分法。遇到詞典沒收的專有名詞
/// （`うさだぺこら`）時，它切出來的東西再怎麼選字都拼不出正確答案
/// ——**使用者得能自己把詞界拉開**，逐段選字組出來。
///
/// 那也是「第一次輸入一個引擎不認識的詞」的唯一途徑：拼一次、學起來，
/// 第二次就自動對了。見開發文件 §2.23.5.2。
///
/// `lens` 是每一段佔幾個假名。加起來超過或不足都會被截到剛好。
pub fn convert_with(kana: &str, lens: &[usize]) -> Vec<Word> {
    let chars: Vec<char> = kana.chars().collect();
    let mut out = Vec::with_capacity(lens.len());
    let mut at = 0usize;
    for &n in lens {
        if at >= chars.len() {
            break;
        }
        let n = n.max(1).min(chars.len() - at);
        let part: String = chars[at..at + n].iter().collect();
        at += n;
        // 這一段最好的表記。查不到就用假名本身——那正是使用者要的
        // 「先固定詞界、再逐段選字」的起點
        let surface = dict::best_kana_word(&part)
            .map(|w| w.into_owned())
            .or_else(|| {
                dict::cands_for_kana(&part)
                    .iter()
                    .next()
                    .map(|c| c.surface.to_string())
            })
            .unwrap_or_else(|| part.clone());
        out.push(Word {
            kana: part,
            surface,
        });
    }
    // 沒吃完的補成最後一段——**按鍵一定要接得回原字串**
    if at < chars.len() {
        let part: String = chars[at..].iter().collect();
        let surface = dict::best_kana_word(&part)
            .map(|w| w.into_owned())
            .unwrap_or_else(|| part.clone());
        out.push(Word {
            kana: part,
            surface,
        });
    }
    out
}

/// 相鄰的「原樣」段合併成一段。
fn merge_unknown(words: Vec<Word>) -> Vec<Word> {
    let mut out: Vec<Word> = Vec::with_capacity(words.len());
    for w in words {
        let raw = w.surface == w.kana;
        match out.last_mut() {
            Some(last) if raw && last.surface == last.kana => {
                last.kana.push_str(&w.kana);
                last.surface.push_str(&w.surface);
            }
            _ => out.push(w),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load() -> bool {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data");
        crate::preload(&data, crate::config::Engines::default());
        dict::load_connection(&data);
        dict::japanese_loaded()
    }

    fn text(kana: &str) -> String {
        convert(kana).iter().map(|w| w.surface.as_str()).collect()
    }

    /// **付属語不可以被切成自立語漢字詞**。
    ///
    /// 這些串單獨轉都是對的，接成長鏈才錯——每個位置只留一個節點時，
    /// 「泣けれ」在 `なけれ` 那一格比「なけれ」便宜，整串假名那條路
    /// 中途就被丟掉，後面的「ば」再也接不回來。留一批才保得住。
    #[test]
    fn 否定形不會被切成漢字詞() {
        if !load() {
            return;
        }
        assert_eq!(
            text("きんようびまでにていしゅつしなければ"),
            "金曜日までに提出師なければ",
            "なければ 不可以變成 泣けれ｜ば"
        );
        assert_eq!(
            text("いかなければならない"),
            "いかなければ鳴らない",
            "いか 不可以變成 以下、ない 不可以變成 無い"
        );
    }

    #[test]
    fn 空字串不會炸() {
        assert!(convert("").is_empty());
    }

    /// 詞典沒載入時退回原樣——功能降級但不會壞。
    #[test]
    fn 沒有詞典就原樣回傳() {
        if dict::japanese_loaded() {
            return; // 別的測試已經載了，這條驗不到
        }
        assert_eq!(text("あいうえお"), "あいうえお");
    }

    /// **整句要分得出詞**：ごはんをたべます → ご飯を食べます。
    /// 這正是使用者回報的那一句。
    #[test]
    fn 整句轉換() {
        if !load() {
            return;
        }
        let got = text("ごはんをたべます");
        assert!(
            got.contains('飯') && got.contains('食'),
            "該轉出漢字：{got}"
        );
    }

    /// 分詞要合理——不能把一個詞切碎。
    #[test]
    fn 分詞不切碎詞() {
        if !load() {
            return;
        }
        let ws = convert("ごはんをたべます");
        assert!(ws.len() >= 2, "整句該切成多個詞：{ws:?}");
        assert!(
            ws.iter().any(|w| w.kana == "ごはん"),
            "「ごはん」該是一個詞：{ws:?}"
        );
    }

    /// 詞典沒收的東西不該讓整句壞掉，而且相鄰的未知段要併成一段。
    #[test]
    fn 未知段併成一段() {
        if !load() {
            return;
        }
        let ws = convert("ずずずずず");
        let raw = ws.iter().filter(|w| w.surface == w.kana).count();
        assert!(raw <= 2, "未知的假名不該一個一格：{ws:?}");
    }
}
