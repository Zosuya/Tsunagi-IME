//! 測資的讀取與解析——**十支計分器共用這一份**。
//!
//! # 為什麼要有這個模組
//!
//! 從前每支計分器各自寫死一份檔名清單（`check_freeze` 21 個、
//! `check_compose` 15 個、`check_symbols` 7 個⋯⋯），而且**子集互不相同**。
//! 加一個新測資檔要改最多十處，漏一處那支就默默少量一組——
//! `mixed_kana_fragment` 只出現在 9 支裡，多半就是這樣漏掉的。
//!
//! `expected_text`／`expected_segs` 也各自複製了四份。標記規則要是改了
//! 而沒同步，計分器之間會各說各話——那比算錯更難發現。
//!
//! 這個模組把兩件事收成一份：**去哪裡讀**、**怎麼解析**。
//!
//! # 測資格式
//!
//! 一份 `core/testdata/測資.txt`，用節標題分段：
//!
//! ```text
//! ## en_vowel ｜ 主測：切點 ｜ 來源：人工
//! # 英文詞＋母音開頭的注音音節：email＋a93 → 英:em|日:aila|注:93
//! file|要 <TAB> file|ul4 <TAB> fileul4
//! ```
//!
//! - `## 標籤 ｜ 主測：… ｜ 來源：…` 開一節，之後的資料列都歸這個標籤
//! - `#` 是註解
//! - 資料列三欄，**用 TAB 分隔**（用空白分欄的行會被靜默跳過，
//!   舊 `mixed_holdout` 的「第3版」那行就是這樣壞掉的）
//!
//! 節標題的 `來源` 有三種，意義不同：
//!
//! | 來源 | 意思 | 能不能重產 |
//! |---|---|---|
//! | 人工 | 人寫的句子 | 不能，改了就是改測資 |
//! | 使用者實測 | 實際打字時回報的案例 | 不能 |
//! | 生成 | 工具產的（`gen_long.py` 那類） | 可以整節重產 |
//!
//! `保留集` 另外標記——它的規矩是「寫的時候不看引擎輸出」，
//! 產測資時不准拿引擎篩選。
//!
//! # 標記規則
//!
//! 四種，沒有第五種（見 `期望基準審核.md` §0）：
//!
//! | 標記 | 意思 |
//! |---|---|
//! | 照寫 `A\|B` | 正確：段數與每段文字都要一樣 |
//! | `~` 開頭 | 寬容-段：只比接起來的文字，切成幾段不管 |
//! | ` \|\| ` | 寬容-列舉：命中任一個就算對（前後**都要有空白**） |
//! | 重產 | 測資本身寫錯，用 `mkkeys` 重產 |
//!
//! # 怎麼引入
//!
//! `src/bin/` 底下每個 `.rs` 都是**獨立的 crate**，彼此 `use` 不到；
//! 放進 `common/` 子目錄 Cargo 才不會把它當成執行檔。各計分器這樣拿：
//!
//! ```ignore
//! #[path = "common/testdata.rs"]
//! mod testdata;
//! ```

#![allow(dead_code)] // 每支計分器只用得到其中一部分

use std::collections::BTreeMap;

pub const TAB: char = '\u{9}';

/// 寬容-列舉的分隔符。**前後都要有空白**——`|` 本身是段的分隔符，
/// 沒有空白的話分不出是「段」還是「選項」。
pub const ALT: &str = " || ";

/// 一列測資。
#[derive(Clone, Debug)]
pub struct Row {
    /// 這一列屬於哪一節（`en_vowel`、`holdout`⋯⋯）。統計按它分列。
    pub tag: String,
    /// 第一欄：文字期望。可能帶 `~` 或 `||`，用 `alts()` 取。
    pub want: String,
    /// 第二欄：按鍵期望。`|` 就是切點位置，逐字元比對即可。
    pub key_expect: String,
    /// 第三欄：使用者實際按的鍵。
    pub keys: String,
    /// 在原始檔的第幾行（1-based）——報錯要指得出位置。
    pub line_no: usize,
}

impl Row {
    /// 寬容-列舉的各個選項。沒有 `||` 的話就只有一個。
    pub fn alts(&self) -> Vec<&str> {
        self.want.split(ALT).collect()
    }

    /// 期望的各段按鍵。`_`（分隔符空白）在按鍵欄是空字串，濾掉。
    pub fn expected_segs(&self) -> Vec<String> {
        expected_segs(&self.key_expect)
    }

    /// 期望文字的各段（取寬容-列舉的第一個選項，濾掉 `_`）。
    ///
    /// **一定要先切 `||` 再切 `|`**。`debug|_|しごと || debug_仕事` 直接
    /// 用 `|` 切的話，`|| debug_仕事` 會被當成兩個段，段數一錯，後面拿
    /// 「第 n 段的文字」配「第 n 段的按鍵」就全部錯位——症狀是
    /// 「しごと」被配到按鍵 `debug`。這是收攏之前 `check_bopomofo`
    /// 與 `check_romaji` 各自都有的 bug。
    pub fn want_segs(&self) -> Vec<&str> {
        self.alts()
            .first()
            .unwrap_or(&self.want.as_str())
            .trim_start_matches('~')
            .split('|')
            .map(str::trim)
            .filter(|s| *s != "_")
            .collect()
    }

    /// 這一列屬於保留集嗎？保留集的分數才反映泛化能力。
    pub fn is_holdout(&self) -> bool {
        self.tag.contains("holdout")
    }
}

/// 一節的中繼資料。
#[derive(Clone, Debug, Default)]
pub struct Section {
    pub tag: String,
    /// 主要測哪一層（切點／選字／合法性⋯⋯）。只是給人看的，不影響計分。
    pub focus: String,
    /// 人工／使用者實測／生成。決定這一節能不能重產。
    pub source: String,
    /// 這一節不要被哪些計分器量。寫在節標題的 `不量：check_rewrite`。
    ///
    /// **預設是每支計分器吃全部**——排除是例外，而且寫在資料旁邊，
    /// 不再散在十支程式裡。
    pub skip: Vec<String>,
}

/// 測資檔的路徑。`CARGO_MANIFEST_DIR` 是 `core/`。
pub fn path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("testdata")
        .join("測資.txt")
}

/// 讀進全部測資。`who` 是呼叫端的名字（`check_compose`），
/// 節標題標了 `不量：check_compose` 的那幾節會被跳過。
pub fn load(who: &str) -> Vec<Row> {
    let (rows, _) = load_with_sections(who);
    rows
}

/// 連節資訊一起讀。報表要按節印說明時用得到。
pub fn load_with_sections(who: &str) -> (Vec<Row>, BTreeMap<String, Section>) {
    let p = path();
    let Ok(content) = std::fs::read_to_string(&p) else {
        eprintln!("讀不到測資：{}", p.display());
        return (Vec::new(), BTreeMap::new());
    };
    let mut rows = Vec::new();
    let mut sections: BTreeMap<String, Section> = BTreeMap::new();
    let mut cur = Section {
        tag: "未分節".to_string(),
        ..Default::default()
    };
    let mut skipping = false;

    for (i, line) in content.lines().enumerate() {
        // 只剝換行——**不能剝空白**，注音一聲的結尾空白是資料的一部分
        let line = line.trim_end_matches(['\u{d}', '\u{a}']);

        if let Some(head) = line.strip_prefix("## ") {
            cur = parse_section(head);
            skipping = cur.skip.iter().any(|s| s == who);
            sections.insert(cur.tag.clone(), cur.clone());
            continue;
        }
        if line.trim().is_empty() || line.starts_with('#') || skipping {
            continue;
        }
        let cols: Vec<&str> = line.split(TAB).collect();
        if cols.len() < 3 {
            continue;
        }
        rows.push(Row {
            tag: cur.tag.clone(),
            want: cols[0].to_string(),
            key_expect: cols[1].to_string(),
            keys: cols[2].to_string(),
            line_no: i + 1,
        });
    }
    (rows, sections)
}

/// 解析節標題：`en_vowel ｜ 主測：切點 ｜ 來源：人工 ｜ 不量：check_rewrite`
///
/// 全形直線是刻意的——測資裡的半形 `|` 是切點標記，用同一個符號當
/// 欄位分隔會分不開。
fn parse_section(head: &str) -> Section {
    let mut parts = head.split('｜').map(str::trim);
    let mut s = Section {
        tag: parts.next().unwrap_or("未分節").to_string(),
        ..Default::default()
    };
    for p in parts {
        if let Some(v) = p.strip_prefix("主測：") {
            s.focus = v.trim().to_string();
        } else if let Some(v) = p.strip_prefix("來源：") {
            s.source = v.trim().to_string();
        } else if let Some(v) = p.strip_prefix("不量：") {
            s.skip = v.split(['、', ',']).map(|x| x.trim().to_string()).collect();
        }
    }
    s
}

// ───────────────────── 期望欄的解析 ─────────────────────
//
// 從前這幾個函式在 check_compose、check_incremental、check_candidates、
// spike_partition 各有一份拷貝。收在這裡，改規則只要改一處。

/// 按鍵期望欄 → 各段按鍵。`check| |u vu84` → `["check", " ", "u vu84"]`
pub fn expected_segs(key_expect: &str) -> Vec<String> {
    key_expect
        .split('|')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// 文字期望欄 → 期望字串：`|` 只是分隔、`_` 代表空白。
pub fn expected_text(want: &str) -> String {
    want.replace('|', "").replace('_', " ")
}

/// 去掉全半形空白——寬容-段與錯字率都不把空白當字算。
pub fn strip_spaces(s: &str) -> String {
    s.chars()
        .filter(|c| *c != ' ' && *c != '\u{3000}')
        .collect()
}

/// 這個選項是寬容-段嗎？（`~` 開頭）
pub fn is_tolerant(alt: &str) -> bool {
    alt.starts_with('~')
}

/// 期望文字跟實際輸出對得上嗎？自動處理 `~` 與 `||`。
///
/// 回傳命中的那個選項——報表要拿它當基準（沒中就用第一個）。
pub fn match_text<'a>(row: &'a Row, got: &str) -> Option<&'a str> {
    row.alts().into_iter().find(|alt| {
        let tol = is_tolerant(alt);
        let e = expected_text(alt.trim_start_matches('~'));
        if tol {
            strip_spaces(&e) == strip_spaces(got)
        } else {
            e == got
        }
    })
}

/// 字元編輯距離（插入／刪除／替換各算 1）。錯字率要用。
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = if ca == cb {
                prev[j]
            } else {
                1 + prev[j].min(prev[j + 1]).min(cur[j])
            };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

// ───────────────────── 引擎的載入 ─────────────────────

/// 載入計分器要的全部詞庫層。
///
/// **每支計分器都要載一模一樣的東西**——少載任何一層，量到的行為就跟
/// 使用者看到的不一樣。抽包那次就是靠 `check_compose` 的數字動了才發現
/// 漏掉符號包。收在這裡就不會有人再漏。
pub fn load_engine() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("core 的上層")
        .join("data");
    // 符號全在預載包裡（`packs/內建符號.txt`），不載的話 `\名字\` 整個失效
    ime_core::pack::set_bundled_dir(data.parent().map(|d| d.join("packs")));
    ime_core::pack::load(
        "__不存在的資料夾__",
        &[ime_core::pack::BUNDLED_SYMBOLS.to_string()],
    );
    ime_core::english::load(&data);
    ime_core::dict::load_bopomofo(&data);
    // 選字的中文 bigram——計分器一定要載，不然量到的是「關掉模型」的行為
    ime_core::lm::load(&data, ime_core::dict::char_freq_map(&data));
    ime_core::dict::load_japanese(&data);
    require_dicts(&data);
}

/// 詞庫到底有沒有載進來。**沒有就大聲喊、直接結束。**
///
/// # 為什麼要有這一支
///
/// 詞庫原始檔不進版控（見 `data/download.ps1`），全新的機器上整個
/// `data/` 只有一個 `priority.txt`。這時候計分器**照樣跑得完、exit
/// code 還是 0**，吐出一張看起來完全正常的表——2026-09-08 在 Mac
/// 上第一次跑漏斗就是這樣：1421 句「通過 12.5%」（真實值 90.1%），
/// 從頭到尾一句警告都沒有。表格長得跟真的一模一樣，`japanese_verbs`
/// 87.8%、`symbol` 82.0% 甚至接近正常值，更容易讓人以為資料齊了。
///
/// `check_lm` 一直都做對了（找不到 `.gram` 就 `exit(1)`），這裡只是
/// 把同一個規矩套到其餘幾支。**寧可不出數字，也不要出假數字。**
pub fn require_dicts(data_dir: &std::path::Path) {
    let mut missing: Vec<&str> = Vec::new();
    if ime_core::english::load(data_dir).is_empty() {
        missing.push("英文詞庫  data/english/");
    }
    if ime_core::dict::load_bopomofo(data_dir).is_none_or(|d| d.word_count() == 0) {
        missing.push("注音詞庫  data/bopomofo/");
    }
    if ime_core::dict::load_japanese(data_dir).is_none_or(|d| d.is_empty()) {
        missing.push("日文詞庫  data/japanese/");
    }
    // bigram 沒載的話量到的是「關掉語言模型」的行為（見 load_engine 的註解），
    // 那跟詞庫整個不在一樣會讓數字失真。
    //
    // 用 `get` 不用 `load`：`char_freq_map` 每叫一次就重讀一次
    // `char_freq.txt`（沒有記憶化），拿它只為了問一句「載了沒」太貴。
    // 呼叫端一定已經 `load` 過了。
    if ime_core::lm::get().is_none() {
        missing.push("中文 bigram  data/bopomofo/zh_bigram.gram");
    }
    if missing.is_empty() {
        return;
    }
    eprintln!("✗ 詞庫沒載到，這一輪量出來的數字全部無效：");
    for m in &missing {
        eprintln!("    - {m}");
    }
    eprintln!();
    eprintln!("  詞庫原始檔不進版控，每台開發機要各自準備一次：");
    eprintln!("    1. data/download.ps1        下載原始檔（約 119MB，Mac 要先裝 pwsh 7）");
    eprintln!("    2. cargo run --release -p ime-core --bin gen_dict_zh");
    eprintln!("    3. cargo run --release -p ime-core --bin gen_dict_ja");
    std::process::exit(1);
}

/// `--pack <包名>`：帶著領域包量一次。
///
/// **包會影響切點**（`en` 那半餵 `is_top_word`、`zh` 那半餵 `claimed`），
/// 所以「學到的詞會不會讓分數退步」只能這樣量。不給就是不載任何包。
pub fn load_packs_from_args() -> Vec<String> {
    let packs: Vec<String> = std::env::args()
        .skip(1)
        .scan(false, |want, a| {
            let take = *want;
            *want = a == "--pack";
            Some(take.then_some(a))
        })
        .flatten()
        .collect();
    if !packs.is_empty() {
        let cfg = ime_core::config::Config::load(None);
        let n = ime_core::pack::load(&cfg.behavior.packs_dir, &packs);
        println!("  （載入領域包 {packs:?}，共 {n} 條）");
    }
    packs
}
