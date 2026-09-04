//! 切點引擎：把一串按鍵切成語言段。
//!
//! 依據 `通用語言輸入法 篩選規則.canvas`。
//!
//! # 流程
//!
//! ```text
//! 字元編號 → 字列組成（合法就設斷點、記錄引擎）
//!          → 詞列組成（同引擎視為同一詞列）
//!          → 標點與空白的切點判斷
//!          → 切法模組的三條特殊規則
//! ```

pub mod candidates;
pub mod incremental;
pub mod merge;
pub mod prune;
pub mod punct;
pub mod rank;
pub mod space;

use crate::language::Language;
use crate::{bopomofo, romaji};

impl Language {
    /// 給除錯輸出用的單字標記。
    pub fn short(self) -> &'static str {
        match self {
            Language::Bopomofo => "注",
            Language::Romaji => "日",
            Language::English => "英",
        }
    }
}

/// 一個語言段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// 這一段的按鍵（含被吸收的一聲空白）。
    pub keys: String,
    /// 這一段是不是標點或分隔符？
    ///
    /// 標點與分隔符**一律自成一段、不參與任何合併**（canvas：標點符號
    /// 前後均視為切點）。沒有這個標記的話，`hello,` 的逗號會因為
    /// 「語言同樣是 English」而被併進 `hello` 那段。
    pub is_mark: bool,
    /// 這一段是哪個引擎給的。
    ///
    /// canvas 的「字列組成」要求「記錄此合法組合是甚麼引擎輸出的」——
    /// 「詞列組成」要靠它判斷「前一段跟自己是不是同一個引擎」。
    pub lang: Language,
}

/// 分隔符空白自成一段，用這個標記。
pub const SEPARATOR: &str = " ";

/// 把相鄰同語言的段合併，得到「輸出等價」的正規形式。
///
/// # 為什麼需要
///
/// 切點引擎的職責是**切語言**，同一個語言內部切不切**不影響輸出**：
///
/// ```text
/// 注:ru04au04cl3t␣           見面好吃
/// 注:ru04au04 | 注:cl3t␣     切了一刀，但輸出一模一樣
/// ```
///
/// 比對切法時要先正規化，否則會把無害的切法判成錯的——實測 440 句
/// 裡有 30 句是這樣被誤判的（86.8% vs 93.6%）。
///
/// 標記段（標點、分隔符）一律自成一段，不參與合併。
pub fn normalize(segs: &[Segment]) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    for s in segs {
        match out.last_mut() {
            Some(last) if last.lang == s.lang && !last.is_mark && !s.is_mark => {
                last.keys.push_str(&s.keys);
            }
            _ => out.push(s.clone()),
        }
    }
    out
}

/// 取出 `chars[start..end]` 那一段，**能借就借不要配置**。
///
/// 這個切片在切點引擎裡每鍵要取幾千次（每個活著的切法的每一段），
/// 每次都 `collect::<String>()` 是很大一筆白花的配置。
///
/// **判斷純 ASCII 只要比長度**：UTF-8 底下位元組數等於字元數，就代表
/// 每個字元都只佔一個位元組，位元組位移剛好等於字元位移。輸入法的
/// 按鍵本來就全是 ASCII，這條路幾乎一定成立；不成立時退回配置，
/// 行為完全一樣。
pub(crate) fn slice<'a>(
    keys: &'a str,
    chars: &[char],
    start: usize,
    end: usize,
) -> std::borrow::Cow<'a, str> {
    if keys.len() == chars.len() {
        if let Some(s) = keys.get(start..end) {
            return std::borrow::Cow::Borrowed(s);
        }
    }
    std::borrow::Cow::Owned(chars[start..end].iter().collect())
}

/// `normalize(segs).len()`，但**不配置任何東西**。
///
/// 排序時每個候選都要問一次這個長度，而 `normalize` 會把每一段連
/// 字串一起複製再合併——複製出來的東西看一眼長度就丟掉。日文長句
/// 有四百個候選，每鍵就是幾千次白做的字串複製。
///
/// 合併規則跟 `normalize` 一模一樣，由測試綁在一起。
pub fn normalize_len(segs: &[Segment]) -> usize {
    let mut n = 0usize;
    // 目前這一組合併段的語言與「是不是標點」——後面那個決定它能不能
    // 再吃下一段，跟 `normalize` 看 `out.last_mut()` 是同一件事
    let mut group: Option<(Language, bool)> = None;
    for s in segs {
        let merges = matches!(group, Some((lang, mark)) if lang == s.lang && !mark && !s.is_mark);
        if !merges {
            n += 1;
            group = Some((s.lang, s.is_mark));
        }
    }
    n
}

/// 把按鍵串切成語言段。
///
/// # 字列組成
///
/// 從左到右，每個位置試著讓某個引擎吃掉最長的一段：
/// 注音 → 日文 → 英文（瀑布順序）。吃到就設斷點、記錄引擎。
///
/// # 空白的處理：先收尾，再切點
///
/// 「注音一律以聲調鍵收尾」在「空白鍵前後均為切點」**之前**——
/// 空白先讓前面的注音音節收尾，收不了的才是分隔符。見 `space::is_tone`。
pub fn cut(keys: &str) -> Vec<Segment> {
    // ── 階段1：切成區塊，但空白先讓注音音節收尾 ──
    //
    // 切開空白讓語言邊界變明確：區塊內沒有空白，引擎不會把
    // `check` 的尾巴 `ck ` 當成注音音節 ㄎㄜˉ 吃掉。
    //
    // **但要先問這個空白是不是聲調**。舊版一律先切，
    // `configg6ru0␣`（config時間）就被剝成區塊 `configg6ru0` ＋
    // 標記 `␣`；「時間」的 `ru0␣` 少了那個空白就缺聲調、判非法，
    // 於是退成 `注:g6` ＋ 日文搶走 `ru`，整段注音再也湊不回來。
    //
    // 收得了就把空白吃進區塊。`space::is_tone` 允許最後一段缺聲調，
    // 那正是要找的「待收尾音節」。
    let mut blocks: Vec<String> = Vec::new();
    let bytes: Vec<char> = keys.chars().collect();
    let mut cur = String::new();
    for (n, &c) in bytes.iter().enumerate() {
        if c == ' ' {
            // 用 `tone_suffix_start` 而不是 `is_tone`——區塊前面可能還有
            // 別的語言（`configg6ru0` 的 `config`），整塊問一定是 false。
            if !cur.is_empty() && space::tone_suffix_start(&cur).is_some() {
                cur.push(' '); // 空白是聲調，被音節吃掉，不切
                continue;
            }
            if !cur.is_empty() {
                blocks.push(std::mem::take(&mut cur));
            }
            blocks.push(" ".to_string());
        } else if punct::is_punct(keys, n) {
            // 標點：切開，並保留它自己成一塊（一律自成一段）
            if !cur.is_empty() {
                blocks.push(std::mem::take(&mut cur));
            }
            blocks.push(c.to_string());
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        blocks.push(cur);
    }

    // ── 階段2：區塊內各自切段 ──
    let mut out: Vec<Segment> = Vec::new();
    for block in blocks {
        // 標點一律自成一段（canvas：標點符號前後均視為切點）
        if block.chars().count() == 1 && block != " " {
            let c = block.chars().next().unwrap();
            if !c.is_alphanumeric() && punct::is_punct(&block, 0) {
                out.push(Segment {
                    keys: block.to_string(),
                    lang: Language::English,
                    is_mark: true,
                });
                continue;
            }
        }
        if block == " " {
            // 左邊那段要不要把這個空白收回去當聲調？
            let absorb = out
                .last()
                .map(|s| s.lang == Language::Bopomofo && space::is_tone(&s.keys))
                .unwrap_or(false);
            if absorb {
                out.last_mut().unwrap().keys.push(' ');
            } else {
                out.push(Segment {
                    keys: SEPARATOR.to_string(),
                    lang: Language::English,
                    is_mark: true,
                });
            }
            continue;
        }
        if block.is_empty() {
            continue;
        }
        cut_block(&block, &mut out);
    }

    // ── 切法模組的三條特殊規則 ──
    merge::apply(out)
}

/// 切一個不含空白的區塊。
///
/// 字列組成：從左到右，依瀑布順序（注音 → 日文 → 英文）讓引擎吃掉
/// 最長的一段。吃到就設斷點、記錄是哪個引擎。
fn cut_block(block: &str, out: &mut Vec<Segment>) {
    // **先判斷整個區塊是不是注音**。
    //
    // 「允許最後一段缺聲調」這個放寬只能用在整塊都是注音的情況——
    // 用在區塊中間切出來的碎片會出事：`check` 走貪婪會在位置 3 試到
    // `ck`，而 `ck␣` 剛好是 ㄎㄜˉ，於是 check 被切成 `che`+`ck`，
    // 後面的分隔符空白也被吃掉。
    if space::split_prefix_syllables(block).is_some() {
        push_or_merge(out, block.to_string(), Language::Bopomofo);
        return;
    }

    let chars: Vec<char> = block.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let mut taken = None;
        // 注音優先（嚴格：必須以聲調收尾）
        for len in (1..=(chars.len() - i)).rev() {
            let seg: String = chars[i..i + len].iter().collect();
            if bopomofo::validity(&seg) == bopomofo::Validity::Valid {
                taken = Some((seg, len, Language::Bopomofo));
                break;
            }
        }
        // 再問日文
        if taken.is_none() {
            for len in (1..=(chars.len() - i)).rev() {
                let seg: String = chars[i..i + len].iter().collect();
                if romaji::validity(&seg) == romaji::Validity::Valid {
                    taken = Some((seg, len, Language::Romaji));
                    break;
                }
            }
        }
        match taken {
            Some((seg, len, lang)) => {
                push_or_merge(out, seg, lang);
                i += len;
            }
            None => {
                // 兩個引擎都吃不下 → 英文 passthrough
                push_or_merge(out, chars[i].to_string(), Language::English);
                i += 1;
            }
        }
    }
}

/// 詞列組成：**檢查前個合法字列是否與自己相同，相同者視為同一詞列**。
///
/// 相鄰的同語言段合併成一段——`明天`+`記得` 都是注音引擎給的，
/// 中間沒有空白，就是同一個詞列。
fn push_or_merge(out: &mut Vec<Segment>, keys: String, lang: Language) {
    match out.last_mut() {
        Some(last) if last.lang == lang && !last.is_mark => {
            last.keys.push_str(&keys);
        }
        _ => out.push(Segment {
            keys,
            lang,
            is_mark: false,
        }),
    }
}

#[cfg(test)]
mod tests {
    /// `normalize_len` 必須永遠等於 `normalize().len()`。
    ///
    /// **兩份實作講同一條規則，一定要綁在一起**——不然改了合併規則
    /// 只改一邊，排序的分數會悄悄跑掉，而且不會有任何錯誤訊息。
    #[test]
    fn 不配置的長度跟正規化後一致() {
        let cases = [
            "su3cl3",
            "check u vu84",
            "sushi wo tabemasu",
            "rup wu0 wu0 fu4cp3cl3",
            "hello world 2024",
            "a",
            "",
            "3.5",
            "wu0 2024",
        ];
        for keys in cases {
            for c in crate::cutpoint::incremental::Incremental::from_keys(keys).cuttings() {
                assert_eq!(
                    normalize_len(&c),
                    normalize(&c).len(),
                    "「{keys}」的某個切法對不上：{c:?}"
                );
            }
        }
    }

    use super::*;

    fn show(keys: &str) -> String {
        cut(keys)
            .iter()
            .map(|s| format!("{}:{}", s.lang.short(), s.keys.replace(' ', "␣")))
            .collect::<Vec<_>>()
            .join(" | ")
    }

    #[test]
    fn 純注音() {
        assert_eq!(show("su3cl3"), "注:su3cl3", "你好");
        assert_eq!(show("rup wu0 "), "注:rup␣wu0␣", "今天");
    }

    #[test]
    fn 純日文() {
        assert_eq!(show("sushi"), "日:sushi");
        assert_eq!(show("arigatou"), "日:arigatou");
    }

    #[test]
    fn 同引擎合併成一段() {
        // 「明天」「記得」都是注音，中間沒空白 → 同一詞列
        assert_eq!(show("au/6wu0 ru42k6"), "注:au/6wu0␣ru42k6");
    }

    #[test]
    fn 分隔符空白自成一段() {
        let segs = cut("ru42k6 update");
        assert_eq!(segs[1].keys, SEPARATOR, "中間是分隔符");
    }

    #[test]
    fn 一聲空白留在段內() {
        // 「今天」的兩個空白都是一聲
        let segs = cut("rup wu0 ");
        assert_eq!(segs.len(), 1, "不該被切開：{segs:?}");
        assert_eq!(segs[0].keys, "rup wu0 ");
    }
}
