//! 英文詞典。
//!
//! **只在切點引擎的補救時用**——依 `語言辨識演算法(新).canvas` 的
//! 「切法模組」第三條規則：
//!
//! > 切法頭與尾段與相鄰語言不同時，查詢英文辭典作為補候選
//!
//! 英文本身不做合法性判斷（它是瀑布的最後一站，passthrough），
//! 詞典只用來救「英文單字被日文吃掉」的情況：
//!
//! ```text
//! check    →  日:che | 英:ck      ← che（ちぇ）是合法日文
//! meeting  →  日:meeti | 英:ng
//! ```
//!
//! 這兩個都在詞典裡，查到就把整段還原成英文。

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;

static DICT: OnceLock<HashSet<String>> = OnceLock::new();
/// 兩字母詞的出現次數。排序層要用來分辨 `go` 與 `cl`。
static SHORT_FREQ: OnceLock<HashMap<String, u64>> = OnceLock::new();
/// 詞 → 詞頻排名（1 = 最常用）。語言判定要用它擋日文的誤命中。
static RANK: OnceLock<HashMap<String, u32>> = OnceLock::new();

/// 載入詞典。多次呼叫只讀一次檔。
///
/// 詞典檔的格式是 `word 頻率`，這裡只取詞本身——**頻率不進切點引擎**。
/// 切點只需要「這是不是一個英文詞」的是非題；頻率是選詞模組的事。
pub fn load(data_dir: &Path) -> &'static HashSet<String> {
    let first = DICT.get().is_none();
    let out = DICT.get_or_init(|| {
        let path = data_dir.join("english").join("en_50k.txt");
        let mut set = HashSet::new();
        let mut short = HashMap::new();
        let mut rank = HashMap::new();
        if let Ok(content) = std::fs::read_to_string(&path) {
            for (i, line) in content.lines().enumerate() {
                let mut it = line.split_whitespace();
                let Some(word) = it.next() else { continue };
                let w = word.to_ascii_lowercase();
                // 頻率**不進切點引擎**（切點只問是非題），但排序層要用它
                // 分辨常用短詞與雜訊，所以只留短詞的。
                if w.chars().count() <= 2 {
                    if let Some(Ok(n)) = it.next().map(|n| n.parse::<u64>()) {
                        short.insert(w.clone(), n);
                    }
                }
                rank.entry(w.clone()).or_insert(i as u32 + 1);
                set.insert(w);
            }
        }
        // 領域包的英文詞。**接在這裡而不是選字層**——`claimed`／
        // `common_en` 認得之後，切點的 `fewer_passthrough` 就不再把它
        // 當殘渣罰。實測 `hololive5k4ek7` 從「ほぉぃゔぇ這個」變成
        // 「hololive這個」，那正是 §2.7「詞庫沒有的英文詞會排到很後面」
        // 的正解。
        //
        let _ = SHORT_FREQ.set(short);
        let _ = RANK.set(rank);
        set
    });
    // 載完才通知——切點引擎的快取拿版本號當有效期
    if first {
        crate::dict::bump_generation();
    }
    out
}

/// 查表用的小寫鍵。
///
/// **輸入通常已經是小寫**——按鍵串就是小寫字母——這時直接借用，不配置。
///
/// 這幾個查詢在切點引擎的熱路徑上：每個候選的每一對相鄰段落都要問，
/// 一次 `to_ascii_lowercase()` 的配置乘上幾百個候選就是毫秒級的差別。
/// 給 `stole_head` 加排名比較時 p99 從 10.9ms 變 12.6ms，就是這樣來的。
fn key(word: &str) -> std::borrow::Cow<'_, str> {
    if word.bytes().any(|b| b.is_ascii_uppercase()) {
        std::borrow::Cow::Owned(word.to_ascii_lowercase())
    } else {
        std::borrow::Cow::Borrowed(word)
    }
}

/// 這是不是一個英文詞？
///
/// 詞典沒載入時一律回 false——切點引擎要能在沒有詞庫的情況下運作，
/// 只是少了這一層補救。
pub fn is_word(word: &str) -> bool {
    let lower = key(word);
    // 領域包是**另一層**，不是併進詞典裡的。先問它——使用者自己加的詞
    // 應該贏過統計，而且這樣改設定就能換掉，不必重建詞典。
    // 沒啟用包的話 `is_empty()` 直接短路，一次查詢都不多做。
    if crate::pack::any() && crate::pack::index().en_has(lower.as_ref()) {
        return true;
    }
    // 學到的切詞也算「是個英文詞」。
    //
    // **只掛 `rank` 不夠**：`footer`、`widget` 這些根本不在 en_50k 裡，
    // `rank` 回 `None`、`is_word` 回 false，於是 `claimed` 不認、切法
    // 排不上去。量出來過——只改 `rank` 的話 720 句只多對 1 句。
    if crate::learn::cut_any()
        && crate::learn::cutting().lang_of(word) == Some(crate::language::Language::English)
    {
        return true;
    }
    match DICT.get() {
        Some(d) => d.contains(lower.as_ref()) || agent_noun(d, lower.as_ref()),
        None => false,
    }
}

/// 「動詞＋er」的行為者名詞：`sort`→`sorter`、`log`→`logger`、
/// `link`→`linker`。
///
/// # 為什麼要有
///
/// en_50k 是字幕語料，這一類技術詞多半沒收。詞典沒收的詞在切法裡
/// 沒辦法整段活下來，於是「英文詞＋一個中文字」會被切成英文碎片加假名：
///
/// ```text
/// logger要（loggerul4）  →  英:log | 日:geru | 注:l4
/// sorter要（sorterul4）  →  英:sort | 日:eru | 注:l4
/// ```
///
/// 做法跟日文的 `romaji::inflect` 一樣：**還原成字根再查**，不補資料。
/// `rank` 對這些詞仍回 `None`——它們不在語料裡，沒有排名可給。
///
/// # 三道門，各擋一種誤判
///
/// 日文動詞的「え段＋る」（食べる、閉める、上げる）拼出來就是 `…er…`，
/// 這條還原最怕把它們吃掉，前兩道門都是為了這個：
///
/// 1. **字根要在前 5000 名**（跟 `is_top_word` 同一條線）：`shim`（第
///    20180 名）＋er 會把 `shimerarenakatta`（閉められなかった）切成
///    shimer｜arenakatta
/// 2. **字根的長度**：sort＋er 型至少 4 個字母、queue＋r 型至少 5 個。
///    `age`（第 756 名）＋r 會吃掉 `agerareta`（上げられた）；`tab`＋er
///    會把 `taberarenakatta`（食べられなかった）切成 taber｜arenakatta
///    ——這個是還沒有第一道門時量到的，現在 `tab`（第 7715 名）兩道門
///    都擋
/// 3. **字根要是動詞**：詞典收了它的 -ing 或 -ed 形（sorting、logged、
///    mapping、linked、footing）。行為者名詞是從動詞來的，`about`、
///    `there` 不是動詞。沒有這道門的話前 5000 名的字根能湊出 8570 個
///    「詞」，絕大多數是 `abouter`、`therer`、`wentter` 這種；加上之後剩
///    714 個（accesser、plotter、splitter…），要救的全都還在。
///
/// 第三道門直接拿「去掉 er 的那串」接 -ing／-ed：這三種詞尾共用同一套
/// 拼寫變化（字尾的 e 拿掉、短母音後的子音重複），`logg`＋ing、
/// `queu`＋ing 就是 logging、queuing，不必另外還原。
fn agent_noun(d: &HashSet<String>, w: &str) -> bool {
    let Some(stem) = w.strip_suffix("er") else {
        return false;
    };
    let top = |root: &str| {
        d.contains(root)
            && RANK
                .get()
                .and_then(|m| m.get(root))
                .is_some_and(|r| *r <= TOP_WORD_RANK)
    };
    // sort＋er、link＋er、foot＋er
    let plain = stem.len() >= 4 && top(stem);
    // queue＋r、parse＋r：去掉最後那個 r 就是字根（`stem`＋e）
    let e_root = &w[..w.len() - 1];
    let silent_e = e_root.len() >= 5 && top(e_root);
    // log＋g＋er、map＋p＋er：短母音後的子音重複
    let s = stem.as_bytes();
    let doubled = s.len() >= 4
        && s[s.len() - 1] == s[s.len() - 2]
        && s[s.len() - 1].is_ascii_alphabetic()
        && !b"aeiou".contains(&s[s.len() - 1])
        && top(&stem[..stem.len() - 1]);
    if !(plain || silent_e || doubled) {
        return false;
    }
    // 字根要是動詞（見上面第三道門）。放在最後——走得到這裡的很少，
    // 值得為它配置字串
    d.contains(&format!("{stem}ing")) || d.contains(&format!("{stem}ed"))
}

/// 這是不是一個**夠常用**的英文詞？
///
/// # 為什麼短詞要看頻率
///
/// en_50k 收了大量兩三個字母的冷僻縮寫，它們讓「把字串切碎」變得
/// 有利可圖——`cl3`（好，ㄎㄠˇ）會被切成 `英:cl` ＋ `英:3`，因為
/// `cl` 在詞典裡。實測「大家好」就敗在這裡。
///
/// 但不能一律排除短詞——`go`、`ok`、`it` 都是真的常用字。
/// 頻率把兩者分得很開：
///
/// ```text
/// go 273 萬   ok 38 萬   it 1363 萬     ← 真的常用
/// cl 377      vu 1205    oka 347        ← 雜訊
/// ```
///
/// 四個字母以上不受此限——那個長度的巧合命中很少。
pub fn is_common_word(word: &str) -> bool {
    // 包裡的詞一律算常用——使用者特地列進來就是要它被認得，
    // 不該再拿短詞的頻率門檻去擋（那道門檻是為了濾詞典裡的雜訊）。
    if crate::pack::any() && crate::pack::index().en_has(key(word).as_ref()) {
        return true;
    }
    if !is_word(word) {
        return false;
    }
    // **只管兩個字母的**。
    //
    // 三字母的技術詞頻率跟雜訊重疊，分不開：
    //   tab 3719   bug 12385   git 1635      ← 要保留
    //   cl 377     vu 1205     oka 347       ← 要排除
    // 用頻率擋三字母會誤傷 tab/bug/git，實測輸出正確從 94.1% 掉到 93.2%。
    //
    // 兩字母就乾淨得多——常用的都在 30 萬以上，雜訊在 1.5 萬以下。
    if word.chars().count() > 2 {
        return true;
    }
    SHORT_FREQ
        .get()
        .and_then(|m| m.get(key(word).as_ref()))
        .is_some_and(|n| *n >= SHORT_WORD_MIN_FREQ)
}

/// 兩字母詞的頻率門檻。
///
/// 常用的都在 30 萬以上（`go` 273 萬、`ok` 38 萬、`it` 1363 萬），
/// 雜訊在 1.5 萬以下（`cl` 377、`vu` 1205、`bi` 2020），中間是空的，
/// 所以這個數字取在區間內都一樣（實測 2 萬～50 萬結果相同）。
pub const SHORT_WORD_MIN_FREQ: u64 = 100_000;

/// 這個詞的詞頻排名（1 = 最常用）。查不到回 `None`。
pub fn rank(word: &str) -> Option<u32> {
    let lower = key(word);
    // 包裡的詞排最前面——領域詞是使用者明講要的，不該輸給語料裡
    // 剛好比較常見的雜訊。`is_top_word` 也靠這條，所以包裡的英文詞
    // 不會在跟日文段相鄰時被搶走。
    if crate::pack::any() && crate::pack::index().en_has(lower.as_ref()) {
        return Some(0);
    }
    // 學到的切詞排在包前面——包是整批引進的通用詞，這是這個人自己
    // 按 Tab 表態過的。見開發文件 §2.26。
    if crate::learn::cut_any()
        && crate::learn::cutting().lang_of(word) == Some(crate::language::Language::English)
    {
        return Some(0);
    }
    RANK.get().and_then(|m| m.get(lower.as_ref())).copied()
}

/// 這是不是**常用到不該讓給日文**的英文詞？
///
/// # 為什麼需要
///
/// 日文詞典有 74 萬條，幾乎任何三四個字母的組合都能拼成某個假名詞。
/// 「有詞典收錄就以那個詞典為優先」那條規則在**兩邊都收**時失效，
/// 退回瀑布順序（日文優先），於是這些會被判成日文：
///
/// ```text
/// you  → よう    the → てぇ    are → あれ
/// time → ちめ    take → たけ   game → がめ
/// ```
///
/// 實測英文前 5000 名裡有 145 個中招。它們只有在跟日文段相鄰時才
/// 出事（會被合併成一段），但真實使用一定會遇到。
pub fn is_top_word(word: &str) -> bool {
    rank(word).is_some_and(|r| r <= TOP_WORD_RANK)
}

/// 「常用到不該讓給日文」的排名門檻。
///
/// 掃描結果（輸出正確）：
///
/// | 門檻 | 200 | 500 | 2000 | 3000 | **5000** | 8000 | 20000 | 50000 |
/// |---|---|---|---|---|---|---|---|---|
/// | 分數 | 95.5 | 95.5 | 95.7 | 95.9 | **96.4** | 96.1 | 96.4 | 95.0 |
///
/// 5000 之後是一片高原（96.1～96.4）而不是尖峰，代表這個分界是真的
/// 不是硬湊的。取 5000 是高原的起點——再往上收益不增，卻會把更多
/// 冷僻英文詞從日文手上搶走。
pub const TOP_WORD_RANK: u32 = 5000;

/// 詞典載入了嗎？
pub fn is_loaded() -> bool {
    DICT.get().is_some_and(|d| !d.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data")
    }

    #[test]
    fn 載入詞典() {
        let d = load(&data_dir());
        if d.is_empty() {
            eprintln!("詞庫未下載，跳過（跑 data/download.ps1）");
            return;
        }
        assert!(d.len() > 40000, "en_50k 應該有五萬詞，實際 {}", d.len());
    }

    #[test]
    fn 查得到常見詞() {
        let d = load(&data_dir());
        if d.is_empty() {
            return;
        }
        for w in ["check", "meeting", "push", "update", "keyboard"] {
            assert!(is_word(w), "{w:?} 該在詞典裡");
        }
    }

    #[test]
    fn 大小寫不分() {
        let d = load(&data_dir());
        if d.is_empty() {
            return;
        }
        assert_eq!(is_word("Check"), is_word("check"));
    }

    #[test]
    fn 亂碼查不到() {
        let d = load(&data_dir());
        if d.is_empty() {
            return;
        }
        assert!(!is_word("zzxxqqww"));
    }

    /// **「動詞＋er」詞典沒收也要認得**，而且三道門各自守得住。
    ///
    /// 要救的詞取自測資 en_vowel／en_split（`logger|要`、`sorter|要`、
    /// `mapper|改`、`footer|早`、`linker|已`）。英文詞庫進版控，CI 上也跑得到。
    #[test]
    fn 動詞加er還原得回字根() {
        let d = load(&data_dir());
        if d.is_empty() {
            return;
        }
        for w in ["sorter", "logger", "mapper", "footer", "linker"] {
            assert!(!d.contains(w), "前提：{w} 不在 en_50k 裡，這條才測得到還原");
            assert!(is_word(w), "{w} 該還原得回字根");
            assert!(is_common_word(w), "{w} 夠長，該算常用");
            assert_eq!(rank(w), None, "還原出來的詞不在語料裡，沒有排名");
        }
        // 日文動詞的「え段＋る」不可以被吃掉（前兩道門擋的）
        assert!(
            d.contains("shim") && !is_top_word("shim"),
            "前提：shim 冷僻"
        );
        assert!(!is_word("shimer"), "會把 shimerarenakatta 切成 shimer｜…");
        assert!(!is_word("taber"), "會把 taberarenakatta 切成 taber｜…");
        // `age` 夠常用、也是動詞（aging），擋它的只有長度
        assert!(is_top_word("age") && d.contains("aging"), "前提");
        assert!(!is_word("ager"), "會吃掉 agerareta");
        // 第三道門：字根要是動詞。`about`、`there` 在前 5000 名、長度也夠，
        // 但詞典沒有 abouting／thering 這種形，它們不是動詞
        assert!(is_top_word("about") && is_top_word("there"), "前提");
        assert!(!is_word("abouter"), "about 不是動詞");
        assert!(!is_word("therer"), "there 不是動詞");
    }
}
