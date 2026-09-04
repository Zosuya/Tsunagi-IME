//! 智慧學習：把使用者的選擇記下來，下次優先給。
//!
//! 設計依據是[開發文件 §2.22]，這裡只記實作上的要點。
//!
//! # 它是分層的第三層
//!
//! 系統詞庫（唯讀）→ 領域包（唯讀、可換）→ **個人學習（可寫）**。
//! 查詢入口早就是「先問可換的那一層」（見 `crate::pack`），這一層接在
//! 同一個位置、排在包前面——包是整批引進的通用詞，學習是這個人自己
//! 打出來的。
//!
//! # 記什麼：包含改字的所有 2～4 字子段，加上整段
//!
//! 使用者改過某一格之後送出，那一格前後的字構成什麼詞我們**並不知道**
//! （知道的話就不會選錯了）。所以不猜詞界，把所有可能的子段都記下來，
//! 讓重複次數自己把答案篩出來：
//!
//! ```text
//! 打「了新世紀真」，改了「世」
//!   長度2  新世 世紀
//!   長度3  了新世 新世紀 世紀真
//!   長度4  …（含「世」的四字窗）
//!   整段   了新世紀真（≦6 才記）
//! ```
//!
//! **真的詞會在不同上下文重複出現，雜訊子段不會**——「新世紀」下次
//! 出現在「進入新世紀」又 +1，「了新世」永遠停在 1。
//!
//! # 為什麼查詢是門檻而不是連續的指數
//!
//! 規劃定的曲線是「權重隨次數指數成長」。那在**有排序空間**的地方才
//! 成立（單字候選是一串可以重排的清單）；但 `word_for` 是**查表**，
//! 回傳單一結果、沒有分數可以混合。指數在這裡塌縮成一個門檻：
//! `k^N ≥ 統計分` 等價於 `N ≥ log_k(統計分)`，也就是 `LEARNED`。
//!
//! 所以這一版的門檻**就是指數曲線在查表情境下的樣子**，不是換了設計。
//! 單字候選那條（權重真的參與排序）留到下一刀。
//!
//! [開發文件 §2.22]: ../../../開發文件.md

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

/// 次數到幾算「學會」。
///
/// 見模組說明「為什麼查詢是門檻」。**1 是候選、≧2 是學會**——候選只是
/// 記著，不影響輸出；學會的才凌駕統計。這個數字之後要用 spike 調。
pub const LEARNED: u32 = 2;

/// 學習權重的**指數底數**：使用者選過 `N` 次，權重是 `k^N`。
///
/// # 為什麼是指數
///
/// 兩條既定規則會打架——「使用者的選擇無條件贏過統計」與「不要一次
/// 就跳到最前面」。指數一個機制同時滿足：選 1 次只是輕微加權，選 N 次
/// `k^N` 累積到某個點自然壓過統計。**不必訂「幾次才算」那種武斷門檻**。
///
/// **8 定案**（使用者裁決，2026-09-02）：實際用起來體感是對的，
/// 不再花一輪 spike 去掃。體感是：常用字選一次翻不動（差距常常是幾十
/// 上百倍），罕見字選三四次會贏（8³=512、8⁴=4096）。
/// 見開發文件 §2.22.5.1、§2.22.6.2。
pub const GROWTH: u64 = 8;

/// 記到幾個字為止。
///
/// 跟引擎查詞的 `MAX_WORD` 對齊——查詞迴圈本來就最多看 6 個音節，
/// 學更長的也查不到。
const MAX_LEN: usize = 6;

/// 子段最長記到幾個字（整段另計）。
const WINDOW: usize = 4;

/// 檔案最多幾條。超過就淘汰「從沒重複過」的候選。
const MAX_ENTRIES: usize = 20_000;

/// 一條學習記錄。
#[derive(Debug, Clone)]
pub struct Entry {
    /// 使用者送出的文字
    pub text: String,
    /// 選過幾次
    pub count: u32,
    /// 第幾個被記下來的——淘汰時先丟舊的
    pub seq: u64,
}

/// 學到的東西。**鍵是按鍵串**，值是競爭中的候選（依次數由大到小）。
///
/// 同一串按鍵可能有好幾個答案在競爭（`ㄓㄜˋㄅㄨˋㄏㄠˇ` 可以是
/// 「這部好」也可以是「這不好」），所以是清單不是單一值——次數多的
/// 那個贏，而次數會隨著使用者繼續打字互相追過。
#[derive(Default, Debug, Clone)]
pub struct Index {
    map: HashMap<String, Vec<Entry>>,
    next_seq: u64,
}

impl Index {
    pub fn len(&self) -> usize {
        self.map.values().map(|v| v.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// 這串按鍵學會了什麼？沒到 `LEARNED` 的不算。
    pub fn best(&self, keys: &str) -> Option<&str> {
        let v = self.map.get(keys)?;
        // 清單維持「次數由大到小」，第一個就是最強的
        let top = v.first()?;
        (top.count >= LEARNED).then_some(top.text.as_str())
    }

    /// 這串按鍵、這個文字，使用者選過幾次？沒有就是 0。
    ///
    /// **單字排序用它算權重**（`dict::weighted`）——那條路有分數可以
    /// 相乘，所以指數是真的連續的；`word_for` 那條是查表、只能用門檻。
    pub fn count(&self, keys: &str, text: &str) -> u32 {
        self.map
            .get(keys)
            .and_then(|v| v.iter().find(|e| e.text == text))
            .map(|e| e.count)
            .unwrap_or(0)
    }

    /// 記一次。同一組已經在裡面就 +1，並把它往前排。
    fn bump(&mut self, keys: &str, text: &str) {
        let v = self.map.entry(keys.to_string()).or_default();
        match v.iter().position(|e| e.text == text) {
            Some(i) => v[i].count += 1,
            None => {
                v.push(Entry {
                    text: text.to_string(),
                    count: 1,
                    seq: self.next_seq,
                });
                self.next_seq += 1;
            }
        }
        // **每次都重排**：清單很短（同一串鍵的競爭者通常一兩個），
        // 排好之後 `best` 只要看第一個
        v.sort_by(|a, b| b.count.cmp(&a.count).then(a.seq.cmp(&b.seq)));
    }

    /// 超過上限就淘汰。**只丟從沒重複過的候選**（次數 1），
    /// 學會的（≧`LEARNED`）永不自動淘汰——那是「不遺忘」的界線。
    fn evict(&mut self) {
        if self.len() <= MAX_ENTRIES {
            return;
        }
        let target = MAX_ENTRIES * 9 / 10;
        // 候選依 seq 由舊到新排，先丟最舊的
        let mut cands: Vec<(String, String, u64)> = self
            .map
            .iter()
            .flat_map(|(k, v)| {
                v.iter()
                    .filter(|e| e.count < LEARNED)
                    .map(move |e| (k.clone(), e.text.clone(), e.seq))
            })
            .collect();
        cands.sort_by_key(|(_, _, seq)| *seq);
        let mut n = self.len();
        for (k, text, _) in cands {
            if n <= target {
                break;
            }
            if let Some(v) = self.map.get_mut(&k) {
                v.retain(|e| e.text != text);
                n -= 1;
                if v.is_empty() {
                    self.map.remove(&k);
                }
            }
        }
    }
}

/// **可替換**的——跟領域包同一套（見 `pack::Index` 的說明）。
static INDEX: OnceLock<RwLock<Arc<Index>>> = OnceLock::new();
static HAS: AtomicBool = AtomicBool::new(false);

fn slot() -> &'static RwLock<Arc<Index>> {
    INDEX.get_or_init(|| RwLock::new(Arc::new(Index::default())))
}

/// 有學到東西嗎？**熱路徑的第一道關卡**，一次 relaxed 原子讀。
pub fn any() -> bool {
    HAS.load(Ordering::Relaxed)
}

/// 讀一個中毒也要讀得到的 `RwLock`。
///
/// # 為什麼不能用 `.map(..).unwrap_or_default()`
///
/// 那個寫法在中毒之後**永遠回空的**——學習層與領域包會在該行程剩下的
/// 壽命裡靜靜變成沒東西。不會 panic、不會當機，使用者只覺得「學過的詞
/// 突然都不見了」，而且重開才會好。
///
/// 中毒只代表「上一次有人 panic」，`RwLock` 裡的資料本身沒有壞——它是
/// `Arc<Index>`，讀取端只複製指標。拿回來繼續用，並清掉旗標。
///
/// 這是 2026-09-02 在平台層踩過同一個坑之後掃出來的，見
/// `text_service::lock_state`。
fn read_or_recover<T: Clone + Default>(lock: &std::sync::RwLock<T>) -> T {
    match lock.read() {
        Ok(g) => g.clone(),
        Err(poisoned) => {
            lock.clear_poison();
            poisoned.into_inner().clone()
        }
    }
}

/// 寫一個中毒也要寫得進去的 `RwLock`。
///
/// `if let Ok(..) = lock.write()` 在中毒之後會**靜靜什麼都不做**——
/// 學到的東西寫不進去，而且沒有任何跡象。
fn write_or_recover<T>(lock: &std::sync::RwLock<T>, f: impl FnOnce(&mut T)) {
    match lock.write() {
        Ok(mut g) => f(&mut g),
        Err(poisoned) => {
            lock.clear_poison();
            f(&mut poisoned.into_inner());
        }
    }
}

pub fn index() -> Arc<Index> {
    read_or_recover(slot())
}

fn set_index(new: Index) {
    let has = !new.is_empty();
    // **先建好再上鎖**：`build_cutting` 只讀傳進來的這份，不碰全域，
    // 免得「持著寫鎖又去拿讀鎖」自己鎖死自己
    let cut = build_cutting(&new);
    let has_cut = !cut.is_empty();
    write_or_recover(slot(), |g| *g = Arc::new(new));
    write_or_recover(cut_slot(), |g| *g = Arc::new(cut));
    HAS.store(has, Ordering::Relaxed);
    HAS_CUT.store(has_cut, Ordering::Relaxed);
    // **切點排序的分數快取要作廢**。
    //
    // `rank` 的 memo 用詞庫版本號當有效期，而學習會改變 `claimed`／
    // `is_top_word` 的答案——不作廢的話學到的東西要等重開才生效。
    // 這個坑量出來過：只開段落層級時 720 句只多對 1 句，開了整串層級
    // 卻多對 13 句，差別就是後者繞過分數、前者被舊分數擋住。
    crate::dict::bump_generation();
}

/// 學習檔放哪。跟設定檔、領域包同一個資料夾。
pub fn path(data_dir: Option<&Path>) -> Option<PathBuf> {
    let _ = data_dir;
    crate::config::user_dir().map(|d| d.join("learned.txt"))
}

/// 清空學到的東西。**只有計分器會叫它**——掃門檻時每一輪都要從零開始，
/// 不然上一輪學到的會污染下一輪。不寫檔，使用者的 `learned.txt` 不受影響。
pub fn clear() {
    set_index(Index::default());
}

/// 從檔案載入。格式是 `按鍵串 <TAB> 文字 <TAB> 次數`。
pub fn load(data_dir: Option<&Path>) -> usize {
    let idx = read_index(data_dir);
    let n = idx.len();
    set_index(idx);
    n
}

/// 學習檔現在有多少條。回傳 `(學會的, 還在觀察的)`。
///
/// 給設定頁顯示用——**不動全域索引**，設定頁是另一個行程，載進去也沒
/// 意義。判準跟 `Index::best` 一致：`count >= LEARNED` 才算學會。
pub fn stats(data_dir: Option<&Path>) -> (usize, usize) {
    let idx = read_index(data_dir);
    let mut learned = 0;
    let mut watching = 0;
    for e in idx.map.values().flatten() {
        if e.count >= LEARNED {
            learned += 1;
        } else {
            watching += 1;
        }
    }
    (learned, watching)
}

/// 讀檔並解析成索引。`load` 與 `stats` 共用——格式只寫一次。
fn read_index(data_dir: Option<&Path>) -> Index {
    let mut idx = Index::default();
    if let Some(p) = path(data_dir) {
        if let Ok(content) = std::fs::read_to_string(&p) {
            for line in content.lines() {
                if line.trim().is_empty() || line.starts_with('#') {
                    continue;
                }
                let mut f = line.split('\u{9}');
                let (Some(keys), Some(text), Some(n)) = (f.next(), f.next(), f.next()) else {
                    continue;
                };
                let Ok(count) = n.trim().parse::<u32>() else {
                    continue;
                };
                if keys.is_empty() || text.is_empty() || count == 0 {
                    continue;
                }
                // 這個檔案是同一位使用者寫的，但它跟包一樣是「原樣送進
                // 文件」的來源，同一道門
                if !crate::sanitize::is_safe_output(text) {
                    continue;
                }
                let seq = idx.next_seq;
                idx.next_seq += 1;
                idx.map.entry(keys.to_string()).or_default().push(Entry {
                    text: text.to_string(),
                    count,
                    seq,
                });
            }
        }
    }
    for v in idx.map.values_mut() {
        v.sort_by(|a, b| b.count.cmp(&a.count).then(a.seq.cmp(&b.seq)));
    }
    idx
}

/// 寫回檔案。**淘汰在這裡做**，不在熱路徑上。
pub fn save(data_dir: Option<&Path>) -> std::io::Result<()> {
    let Some(p) = path(data_dir) else {
        return Ok(());
    };
    let cur = index();
    let mut idx = Index {
        map: cur.map.clone(),
        next_seq: cur.next_seq,
    };
    idx.evict();

    let mut rows: Vec<(&String, &Entry)> = idx
        .map
        .iter()
        .flat_map(|(k, v)| v.iter().map(move |e| (k, e)))
        .collect();
    // 依 seq 輸出——**每行獨立、順序穩定**，日後想跨電腦合併時
    // 任何工具都拼得起來，diff 也看得懂
    rows.sort_by_key(|(_, e)| e.seq);

    let mut out = String::new();
    out.push_str("# 智慧學習記錄——引擎自動累積，可以手動刪行\n");
    out.push_str("# 格式：按鍵串 <TAB> 文字 <TAB> 選過幾次\n");
    out.push_str("# 次數 1 是「候選」（還沒生效），2 以上才會影響選字。\n");
    for (k, e) in rows {
        out.push_str(k);
        out.push('\u{9}');
        out.push_str(&e.text);
        out.push('\u{9}');
        out.push_str(&e.count.to_string());
        out.push('\n');
    }
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&p, out)?;
    set_index(idx);
    Ok(())
}

/// 使用者送出了。把這一次的選擇記下來。
///
/// **只在有手動改過字時才記**——沒改就代表引擎本來就給對了，
/// 沒有新資訊。回傳記了幾條。
pub fn record(slots: &[crate::compose::Slot]) -> usize {
    if !slots.iter().any(|s| s.picked) {
        return 0;
    }
    let cur = index();
    let mut idx = Index {
        map: cur.map.clone(),
        next_seq: cur.next_seq,
    };
    let mut n = 0usize;

    // **一段＝連續、同語言的可選字格**。標點不可選字，天然是邊界。
    //
    // 語言也要當邊界：跨語言的子段（`k7ii` → `個いい`）沒有任何查詢
    // 端問得到它——`word_for` 拿到的是注音鍵、`best_kana_word` 拿到的
    // 是假名。記下來只會佔掉 `MAX_ENTRIES` 的額度。
    let mut start = 0usize;
    while start < slots.len() {
        if !slots[start].selectable {
            start += 1;
            continue;
        }
        let lang = slots[start].lang;
        // **標點自成一段**，不跟旁邊的語言段黏在一起。
        //
        // 標點的 `lang` 是英文，不分開的話 `check[` 會被當成同一段，
        // 學到「check「」這種沒有意義的子段。
        let mark = slots[start].is_mark;
        let mut end = start;
        while end < slots.len()
            && slots[end].selectable
            && slots[end].lang == lang
            && slots[end].is_mark == mark
        {
            end += 1;
        }
        n += record_run(&mut idx, &slots[start..end]);
        start = end;
    }

    if n > 0 {
        set_index(idx);
    }
    n
}

/// 一個連續可選字段裡，把該記的子段都記下來。
fn record_run(idx: &mut Index, run: &[crate::compose::Slot]) -> usize {
    if !run.iter().any(|s| s.picked) {
        return 0;
    }
    let mut n = 0usize;
    // **每個改過的格子自己也記一條**。
    //
    // 日文一格是一整段（詞級），注音一格是一個字——兩者都要記，
    // 但用途不同：日文那條餵 `best_kana_word`（查表、用門檻），
    // 注音那條餵單字排序（有分數可相乘，指數是連續的）。
    for sl in run.iter() {
        if !sl.picked {
            continue;
        }
        if sl.lang == crate::language::Language::Bopomofo
            && !sl.keys.is_empty()
            && !sl.text.is_empty()
        {
            idx.bump(&sl.keys, &sl.text);
            n += 1;
        }
        if sl.lang == crate::language::Language::Romaji {
            // **鍵要用假名不是羅馬字**：查詢端 `best_kana_word` 拿到的
            // 是假名（`compose` 先轉過），鍵不一致就永遠查不到。
            if let Some(kana) = crate::romaji::kana::to_kana(&sl.keys) {
                if !kana.is_empty() && !sl.text.is_empty() {
                    idx.bump(&kana, &sl.text);
                    n += 1;
                }
            }
        }
        // 英文段本來不選字，會走到這裡的只有「日文詞典也收」的那一類
        // （`ii`→いい）。**鍵用英文按鍵原文**，不能用假名——用假名的話
        // 會跟真正的日文段共用同一個鍵，`you` 就可能被別處學到的
        // 「よう→用」改掉。英文段一定不是合法注音音節（`lang_of` 先問
        // 注音），所以跟注音那條也不會撞。
        if sl.lang == crate::language::Language::English
            && !sl.keys.is_empty()
            && !sl.text.is_empty()
        {
            idx.bump(&sl.keys, &sl.text);
            n += 1;
        }
    }

    let mut put = |idx: &mut Index, a: usize, b: usize| {
        let keys: String = run[a..b].iter().map(|s| s.keys.as_str()).collect();
        let text: String = run[a..b].iter().map(|s| s.text.as_str()).collect();
        if keys.is_empty() || text.is_empty() {
            return;
        }
        idx.bump(&keys, &text);
        n += 1;
    };

    // 含改字的 2～WINDOW 字子段
    for len in 2..=WINDOW.min(run.len()) {
        for a in 0..=run.len() - len {
            let b = a + len;
            if run[a..b].iter().any(|s| s.picked) {
                put(idx, a, b);
            }
        }
    }
    // 整段（超過 MAX_LEN 就不記——查詞也查不到）
    if run.len() > WINDOW && run.len() <= MAX_LEN {
        put(idx, 0, run.len());
    }
    n
}

// ─────────────────────── 切詞學習（第二刀） ───────────────────────
//
// 訊號是**使用者按 Tab 換了切法**，跟第一刀（方向鍵改字）是兩個不同的
// 動作、修的是兩類不同的錯。設計依據見開發文件 §2.26。
//
// 記錄放在同一個 `learned.txt`，靠鍵的前綴分型別：
//
// ```text
// footer      footer   3     ← 選字（第一刀）
// 語:footer   en       3     ← 段落層級：這串按鍵是英文
// 切:footeru3 6        2     ← 整串層級：切點位置
// ```
//
// **前綴用中文字是刻意的**——按鍵串只會是鍵盤打得出來的 ASCII，
// 中文字永遠不會跟真的按鍵串撞。

use crate::language::Language;

/// 段落層級記錄的鍵前綴。
pub const LANG_PREFIX: &str = "語:";
/// 整串層級記錄的鍵前綴。
pub const CUT_PREFIX: &str = "切:";

/// 切詞要選過幾次才生效。
///
/// **跟選字分開、而且可調**（使用者定，2026-09-01）：選錯字只毀一格，
/// 切錯詞會讓整句重新斷句、前面的字跟著變，代價高得多。
/// 預設值由 `bench_cut_learn` 掃出來。
static LEARNED_CUT: AtomicU32 = AtomicU32::new(3);

/// 目前的切詞門檻。
pub fn learned_cut() -> u32 {
    LEARNED_CUT.load(Ordering::Relaxed)
}

/// 改切詞門檻。**只有計分器會叫它**——改完要重建衍生索引。
pub fn set_learned_cut(n: u32) {
    LEARNED_CUT.store(n.max(1), Ordering::Relaxed);
    let idx = index();
    let cut = build_cutting(&idx);
    let has_cut = !cut.is_empty();
    write_or_recover(cut_slot(), |g| *g = Arc::new(cut));
    HAS_CUT.store(has_cut, Ordering::Relaxed);
}

/// 切詞學到的東西——**從 `Index` 衍生出來的查詢用索引**。
///
/// 為什麼要衍生一份而不是每次去主索引撈：切點排序是熱路徑（每一鍵
/// 四百種切法、每段查三本詞典），主索引的值是「競爭中的候選清單」，
/// 每次都要挑第一個、比門檻、解析文字。先算好放這裡，查詢就只是一次
/// 雜湊命中。
#[derive(Default, Debug)]
pub struct Cutting {
    /// 段落層級：按鍵串 → 語言
    lang: HashMap<String, Language>,
    /// 整串層級：按鍵串 → 切點位置
    cut: HashMap<String, Vec<usize>>,
}

impl Cutting {
    /// 這串按鍵學過是哪個語言嗎？
    pub fn lang_of(&self, keys: &str) -> Option<Language> {
        self.lang.get(keys).copied()
    }

    /// 這整串按鍵學過怎麼切嗎？
    pub fn cut_of(&self, keys: &str) -> Option<&[usize]> {
        self.cut.get(keys).map(|v| v.as_slice())
    }

    pub fn len(&self) -> usize {
        self.lang.len() + self.cut.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lang.is_empty() && self.cut.is_empty()
    }
}

static CUTTING: OnceLock<RwLock<Arc<Cutting>>> = OnceLock::new();
static HAS_CUT: AtomicBool = AtomicBool::new(false);

fn cut_slot() -> &'static RwLock<Arc<Cutting>> {
    CUTTING.get_or_init(|| RwLock::new(Arc::new(Cutting::default())))
}

/// 有學到切詞嗎？**熱路徑的第一道關卡**，一次 relaxed 原子讀。
///
/// 沒學過切詞的人（包含所有只用選字學習的）一次雜湊查詢都不會多做，
/// 跟 `pack::any()` 同一招。
pub fn cut_any() -> bool {
    HAS_CUT.load(Ordering::Relaxed)
}

pub fn cutting() -> Arc<Cutting> {
    read_or_recover(cut_slot())
}

/// 語言在檔案裡寫成什麼。**用短碼不用中文**——跟領域包的 `zh`／`ja`／
/// `en` 一致，使用者手動編輯時看到的是同一套詞彙。
fn lang_code(l: Language) -> &'static str {
    match l {
        Language::Bopomofo => "zh",
        Language::Romaji => "ja",
        Language::English => "en",
    }
}

fn lang_from(code: &str) -> Option<Language> {
    match code {
        "zh" => Some(Language::Bopomofo),
        "ja" => Some(Language::Romaji),
        "en" => Some(Language::English),
        _ => None,
    }
}

/// 兩種粒度各自的開關。**只有計分器會關掉其中一邊**——兩種都做是
/// 使用者定的，但「各自貢獻多少」得能分開量才知道。
static USE_LANG: AtomicBool = AtomicBool::new(true);
static USE_WHOLE: AtomicBool = AtomicBool::new(true);

/// 只讓某一種粒度生效。計分器用。
pub fn set_cut_kinds(lang: bool, whole: bool) {
    USE_LANG.store(lang, Ordering::Relaxed);
    USE_WHOLE.store(whole, Ordering::Relaxed);
    set_learned_cut(learned_cut()); // 順便重建
}

/// 從主索引撈出切詞那兩類，建成查詢索引。只收**次數到門檻**的。
fn build_cutting(idx: &Index) -> Cutting {
    let mut out = Cutting::default();
    let need = learned_cut();
    let (use_lang, use_whole) = (
        USE_LANG.load(Ordering::Relaxed),
        USE_WHOLE.load(Ordering::Relaxed),
    );
    for (k, v) in &idx.map {
        let Some(top) = v.first() else { continue };
        if top.count < need {
            continue;
        }
        if let Some(keys) = k.strip_prefix(LANG_PREFIX) {
            if use_lang {
                if let Some(l) = lang_from(&top.text) {
                    out.lang.insert(keys.to_string(), l);
                }
            }
        } else if let Some(keys) = k.strip_prefix(CUT_PREFIX).filter(|_| use_whole) {
            let cut: Vec<usize> = top
                .text
                .split(',')
                .filter_map(|x| x.trim().parse().ok())
                .collect();
            out.cut.insert(keys.to_string(), cut);
        }
    }
    out
}

/// 塞 `n` 條假的切詞記錄。**只有 `bench_typing` 會叫它**——
/// 「學了東西之後會不會變慢」得先有東西才量得到。
pub fn seed_cuttings(n: usize) -> usize {
    let idx0 = index();
    let mut idx = Index {
        map: idx0.map.clone(),
        next_seq: idx0.next_seq,
    };
    for i in 0..n {
        let k = format!("{LANG_PREFIX}zzseed{i}");
        for _ in 0..learned_cut() {
            idx.bump(&k, "en");
        }
    }
    set_index(idx);
    n
}

/// 使用者按 Tab 換過切法之後送出了。把這一次的切詞選擇記下來。
///
/// `chosen` 是他挑的那一種，`default` 是引擎原本的第一名。
/// **只記兩者不同的段落**——一樣的部分沒有新資訊，記了只是佔額度。
///
/// 兩種粒度都記（使用者定）：段落層級會推廣到別的句子，整串層級吃得下
/// 段落層級分不出來的難例（`serve` 與 `server` 兩邊都是英文詞）。
/// 回傳記了幾條。
pub fn record_cutting(
    keys: &str,
    chosen: &[crate::cutpoint::Segment],
    default: &[crate::cutpoint::Segment],
) -> usize {
    let rows = cutting_records(keys, chosen, default);
    if rows.is_empty() {
        return 0;
    }
    let idx0 = index();
    let mut idx = Index {
        map: idx0.map.clone(),
        next_seq: idx0.next_seq,
    };
    for (k, v) in &rows {
        idx.bump(k, v);
    }
    set_index(idx);
    rows.len()
}

/// 這一次該記哪幾條？**抽成純函式**——它是這一刀唯一有邏輯的地方
/// （比對兩種切法、決定哪些段落算「使用者糾正過」），而全域索引
/// 沒辦法在測試裡安全地共用。
fn cutting_records(
    keys: &str,
    chosen: &[crate::cutpoint::Segment],
    default: &[crate::cutpoint::Segment],
) -> Vec<(String, String)> {
    if keys.is_empty() || chosen.is_empty() {
        return Vec::new();
    }
    // 一種切法裡每一段佔的字元範圍
    let spans = |segs: &[crate::cutpoint::Segment]| -> Vec<(usize, usize, Language, bool)> {
        let mut out = Vec::with_capacity(segs.len());
        let mut at = 0usize;
        for s in segs {
            let n = s.keys.chars().count();
            out.push((at, at + n, s.lang, s.is_mark));
            at += n;
        }
        out
    };
    let old: Vec<_> = spans(default);
    let cur = spans(chosen);
    // 兩種切法完全一樣就沒得學——使用者按了 Tab 又轉回來
    if old == cur {
        return Vec::new();
    }

    let mut out = Vec::new();
    for (seg, sp) in chosen.iter().zip(cur.iter()) {
        if sp.3 || seg.keys.is_empty() {
            continue; // 標點不學
        }
        if old.contains(sp) {
            continue; // 跟原本一樣，沒有新資訊
        }
        out.push((
            format!("{LANG_PREFIX}{}", seg.keys),
            lang_code(seg.lang).to_string(),
        ));
    }

    // 整串層級：記切點位置（不含 0，那是起點不是切點）
    let cuts: Vec<String> = cur
        .iter()
        .skip(1)
        .map(|(start, ..)| start.to_string())
        .collect();
    out.push((format!("{CUT_PREFIX}{keys}"), cuts.join(",")));
    out
}

#[cfg(test)]
mod poison_tests {
    use super::{read_or_recover, write_or_recover};
    use std::sync::RwLock;

    fn poison_read(lock: &RwLock<u32>) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = lock.write().unwrap();
            panic!("裝的");
        }));
        assert!(lock.is_poisoned(), "前提：panic 應該讓鎖中毒");
    }

    /// 中毒之後**還讀得到原本的值**，不是回空的。
    ///
    /// # 這條在擋什麼
    ///
    /// 原本寫成 `.read().map(|g| g.clone()).unwrap_or_default()`——中毒
    /// 之後永遠回 `default()`，學習層與領域包會在該行程剩下的壽命裡靜靜
    /// 變成沒東西。不 panic、不當機，使用者只覺得「學過的詞突然都不見
    /// 了」，重開才會好。
    #[test]
    fn 中毒之後還讀得到值() {
        let lock = RwLock::new(42u32);
        poison_read(&lock);
        assert_eq!(read_or_recover(&lock), 42, "不能因為中毒就回預設值");
        assert!(!lock.is_poisoned(), "旗標要清掉，不然每次都走復原路徑");
    }

    /// 中毒之後**寫得進去**，不是靜靜消失。
    ///
    /// 原本寫成 `if let Ok(mut g) = lock.write() { .. }`——中毒之後那個
    /// 分支永遠不成立，學到的東西寫不進去而且沒有任何跡象。
    #[test]
    fn 中毒之後還寫得進去() {
        let lock = RwLock::new(0u32);
        poison_read(&lock);
        write_or_recover(&lock, |v| *v = 7);
        assert_eq!(read_or_recover(&lock), 7, "寫入不能靜靜消失");
    }

    /// 復原之後回到正常路徑——這是平台層踩過的那個坑。
    ///
    /// `into_inner()` 只把值拿回來，中毒旗標是**黏著的**。不清的話每次
    /// 存取都走復原路徑，在平台層那邊的症狀是「每打一個字就把前一個
    /// 吃掉」。見 `text_service::lock_state`。
    #[test]
    fn 復原之後不能還是中毒() {
        let lock = RwLock::new(1u32);
        poison_read(&lock);
        let _ = read_or_recover(&lock);
        assert!(lock.read().is_ok(), "下一次讀取要走正常路徑");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::Slot;
    use crate::language::Language;

    fn slot(keys: &str, text: &str, picked: bool) -> Slot {
        Slot {
            keys: keys.into(),
            text: text.into(),
            lang: Language::Bopomofo,
            selectable: true,
            is_mark: false,
            cands: None,
            picked,
        }
    }

    #[test]
    fn 沒改過字就不記() {
        let mut idx = Index::default();
        let run = [slot("a", "你", false), slot("b", "好", false)];
        assert_eq!(record_run(&mut idx, &run), 0);
    }

    /// 三字詞改中間：**必須有涵蓋整個詞的那一條**。
    ///
    /// 前後各一分開記（「新世」＋「世紀」）是不夠的——查詞是消耗式的
    /// 最長匹配，吃掉「新世」之後「紀」就落單了，兩條拼不回一個詞。
    #[test]
    fn 三字詞改中間有涵蓋整詞的條目() {
        let mut idx = Index::default();
        let run = [
            slot("k1", "新", false),
            slot("k2", "世", true),
            slot("k3", "紀", false),
        ];
        record_run(&mut idx, &run);
        assert!(
            idx.map.contains_key("k1k2k3"),
            "整個三字詞要有一條：{:?}",
            idx.map.keys().collect::<Vec<_>>()
        );
        assert!(idx.map.contains_key("k1k2"), "二字子段也要有");
        assert!(idx.map.contains_key("k2k3"));
    }

    /// **日文一格就是一整段**，所以單格也要記——而且鍵要用假名，
    /// 因為查詢端 `best_kana_word` 拿到的是假名不是羅馬字。
    #[test]
    fn 日文單格用假名當鍵() {
        let mut idx = Index::default();
        let run = [Slot {
            keys: "sushi".into(),
            text: "鮨".into(),
            lang: Language::Romaji,
            selectable: true,
            is_mark: false,
            cands: None,
            picked: true,
        }];
        record_run(&mut idx, &run);
        assert!(
            idx.map.contains_key("すし"),
            "鍵要是假名：{:?}",
            idx.map.keys().collect::<Vec<_>>()
        );
        assert!(
            !idx.map.contains_key("sushi"),
            "不能用羅馬字當鍵，那樣查不到"
        );
    }

    /// 注音的單格是**一個字**——單字學習要用它，所以也要記。
    ///
    /// 它跟日文那條的用途不同：日文餵查表（門檻），注音餵單字排序
    /// （有分數可相乘，指數是連續的）。
    /// 日文詞典也收的英文段（`ii`→いい）記在**英文按鍵原文**底下。
    ///
    /// 不能用假名當鍵：那樣會跟真正的日文段共用同一格，
    /// `you` 就可能被別處學到的「よう→用」改掉。
    fn seg(keys: &str, lang: Language) -> crate::cutpoint::Segment {
        crate::cutpoint::Segment {
            keys: keys.into(),
            is_mark: false,
            lang,
        }
    }

    /// 切詞：兩種粒度都要記，而且**只記跟原本不同的段落**。
    #[test]
    fn 切詞記兩種粒度() {
        let default = [
            seg("foote", Language::Romaji),
            seg("ru3", Language::Bopomofo),
        ];
        let chosen = [
            seg("footer", Language::English),
            seg("u3", Language::Bopomofo),
        ];
        let rows = cutting_records("footeru3", &chosen, &default);
        assert!(
            rows.contains(&("語:footer".into(), "en".into())),
            "{rows:?}"
        );
        assert!(
            rows.contains(&("切:footeru3".into(), "6".into())),
            "{rows:?}"
        );
    }

    /// 一樣的段落沒有新資訊，記了只是佔額度。
    #[test]
    fn 切詞不記沒變的段落() {
        let default = [seg("ab", Language::English), seg("u3", Language::Bopomofo)];
        let chosen = [seg("ab", Language::English), seg("u3", Language::Bopomofo)];
        // 完全一樣 → 什麼都不記
        assert!(cutting_records("abu3", &chosen, &default).is_empty());
    }

    /// 沒到門檻的不進查詢索引——「選一次不會突然改變輸出」在切詞也適用。
    #[test]
    fn 切詞要到門檻才生效() {
        let mut idx = Index::default();
        idx.bump("語:footer", "en");
        assert!(
            build_cutting(&idx).lang_of("footer").is_none(),
            "只選一次不該生效"
        );
        for _ in 1..learned_cut() {
            idx.bump("語:footer", "en");
        }
        assert_eq!(
            build_cutting(&idx).lang_of("footer"),
            Some(Language::English)
        );
    }

    #[test]
    fn 英文段用按鍵原文當鍵() {
        let mut idx = Index::default();
        let run = [Slot {
            keys: "ii".into(),
            text: "いい".into(),
            lang: Language::English,
            selectable: true,
            is_mark: false,
            cands: None,
            picked: true,
        }];
        record_run(&mut idx, &run);
        assert_eq!(idx.count("ii", "いい"), 1);
    }

    #[test]
    fn 注音單格也記() {
        let mut idx = Index::default();
        let run = [slot("k1", "你", true)];
        assert_eq!(record_run(&mut idx, &run), 1);
        assert_eq!(idx.count("k1", "你"), 1);
    }

    /// 次數查得到，才算得出 `k^N` 的權重。
    #[test]
    fn 次數查得到() {
        let mut idx = Index::default();
        assert_eq!(idx.count("k", "甲"), 0);
        idx.bump("k", "甲");
        idx.bump("k", "甲");
        assert_eq!(idx.count("k", "甲"), 2);
    }

    /// 不含改字的子段不記——那跟這次修正無關。
    #[test]
    fn 不含改字的子段不記() {
        let mut idx = Index::default();
        let run = [
            slot("k1", "了", false),
            slot("k2", "新", false),
            slot("k3", "世", true),
        ];
        record_run(&mut idx, &run);
        assert!(!idx.map.contains_key("k1k2"), "「了新」不含改字，不該記");
    }

    /// 次數沒到 `LEARNED` 之前不生效——這就是「選一次不會突然跳第一」。
    #[test]
    fn 候選要重複過才生效() {
        let mut idx = Index::default();
        idx.bump("k", "甲");
        assert_eq!(idx.best("k"), None, "只選過一次還是候選");
        idx.bump("k", "甲");
        assert_eq!(idx.best("k"), Some("甲"), "重複過才算學會");
    }

    /// **後來居上**：之前選甲兩次，後來都選乙，乙追過就換它。
    /// 「不遺忘」不等於「學錯就永遠錯」。
    #[test]
    fn 次數追過就換人() {
        let mut idx = Index::default();
        idx.bump("k", "甲");
        idx.bump("k", "甲");
        assert_eq!(idx.best("k"), Some("甲"));
        for _ in 0..3 {
            idx.bump("k", "乙");
        }
        assert_eq!(idx.best("k"), Some("乙"), "次數多的贏");
    }

    /// 淘汰只丟候選，學會的留著。
    #[test]
    fn 淘汰不動學會的() {
        let mut idx = Index::default();
        idx.bump("keep", "學會的");
        idx.bump("keep", "學會的");
        for i in 0..MAX_ENTRIES + 100 {
            idx.bump(&format!("c{i}"), "候選");
        }
        idx.evict();
        assert!(idx.len() <= MAX_ENTRIES, "要降到上限以下");
        assert_eq!(idx.best("keep"), Some("學會的"), "學會的不能被丟掉");
    }
}
