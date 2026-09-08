//! 領域包：使用者可以自己加入的詞表。
//!
//! # 一個包，三張表
//!
//! 使用者看到的是「一個領域包」，引擎內部把它分流到三個不同的鍵空間：
//!
//! | 語言 | 鍵 | 接到哪 | 效果 |
//! |---|---|---|---|
//! | `en` | 字母 | 英文詞典 | **不只選字，還影響切點**——`claimed`／`common_en` 認得之後，`fewer_passthrough` 就不再當它是殘渣 |
//! | `ja` | 假名讀音 | `KANA_BEST` | 補新詞，或讓已在 mozc 但卡在信心門檻的詞過關 |
//! | `zh` | 注音符號 | `WORDS`＋`CHARS` | 同 `priority.txt` 的注入與排序機制 |
//!
//! **三者無法互推**：`hololive`（字母）、`ほろらいぶ`（讀音）、
//! `ㄏㄨˊㄊㄠˊ`（注音）是三種不同的輸入，音譯不是轉換，得分開登記。
//! 但那是實作細節，使用者不該知道。
//!
//! # 檔案格式
//!
//! 一包一個檔，放 `%APPDATA%\tsunagi-ime\packs\*.txt`
//! （跟 `config.toml` 同層）。`#` 開頭是註解。
//!
//! ```text
//! # 語言 <TAB> 輸入 <TAB> 輸出
//! en   →  hololive
//! ja   →  ほろらいぶ   →  ホロライブ
//! zh   →  ㄏㄨˊㄊㄠˊ   →  胡桃
//! ```
//!
//! （上面的 `→` 是製表符 TAB。doc comment 裡不能直接寫 TAB。）
//!
//! - `en` 是 passthrough，第三欄可以省略（省略就等於輸入本身）
//! - `zh` 的鍵寫**注音符號**不是按鍵，跟 `priority.txt` 一致——手動
//!   維護時看得懂，載入時才轉成按鍵
//!
//! # 為什麼是「先載包、再載詞庫」而不是傳參數
//!
//! 三個詞庫載入函式的簽章只有 `data_dir`，而且它們被 bin、平台層、
//! 測試各處呼叫。多加一個參數要動所有呼叫點。改成**包先載好放進
//! 這裡的靜態**，詞庫載入時自己來拿——沒載過就是空的，行為跟以前
//! 完全一樣。

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

/// 合併之後的領域包內容。**清單順序就是優先序**，前面的先套用。
#[derive(Default, Debug)]
pub struct Packs {
    /// 英文詞（已轉小寫）
    pub en: Vec<String>,
    /// 假名讀音 → 表記
    pub ja: Vec<(String, String)>,
    /// 注音符號 → 詞。**還沒轉成按鍵**，見模組說明
    pub zh: Vec<(String, String)>,
    /// **華語國字** → 台語說法。結構跟 `zh` 一樣，但**是獨立的一層**，
    /// 而且**鍵是國字不是注音**（`zh` 那些鍵寫注音符號）。
    ///
    /// 段選單拿組字區選好的國字去查，歧義在選字階段已經解掉了。用注音
    /// 查會撈到跨詞邊界的誤配——「沙發謝謝」會冒出「發洩」，因為
    /// `ㄈㄚㄒㄧㄝˋ` 同時對得上「發謝」，而那兩個字沒在畫面上相鄰過。
    ///
    /// # 為什麼不能併進 `zh`
    ///
    /// `zh` 排在靜態詞庫前面（§2.17.5），查到就回傳——那是「我要加
    /// hololive 這個詞」要的語意。拿它裝台語詞的話，打「謝謝」會直接
    /// 變成「感恩」，**使用者失去打華語的能力**（實測，§2.64.10）。
    ///
    /// 台語要的是**多一個選擇**：預設仍然是華語詞，段選單裡才看得到
    /// 台語的說法。所以它只給段選單用，不進查詞的主路徑。
    pub tw: Vec<(String, String)>,
    /// 符號名 → 一組符號（`\星\` 那條路，見 `crate::symbol`）。
    ///
    /// **不分語言**——名字是「組出來的文字」，中文的「星」與日文的
    /// 「星」本來就是同一個字串。
    ///
    /// **每個符號是獨立的一筆，不是一整串**。2026-09-07 從 `String`
    /// 改過來：emoji 拆不了字元（`👨‍👩‍👧` 是五個字元組成的一個圖，
    /// 逐字元拆會得到一堆碎片），所以「怎麼拆」的知識收在 `parse`
    /// 那一層，後面每一關拿到的都已經是拆好的清單。
    pub sym: Vec<(String, Vec<String>)>,
}

impl Packs {
    /// 三種語言加起來幾條。
    pub fn len(&self) -> usize {
        self.en.len() + self.ja.len() + self.zh.len() + self.tw.len() + self.sym.len()
    }

    /// 一條都沒有？
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// 包**應該**放在哪個資料夾——不管它存不存在。
///
/// 設定裡指定了就用指定的，沒指定就是預設位置
/// `%APPDATA%\tsunagi-ime\packs\`（跟 `config.toml` 同一層）。
///
/// # 為什麼不再退回專案的 `data/packs`
///
/// 那個後備曾經存在（開發時方便），但它會**默默搶走**預設位置：
/// 專案資料夾裡剛好有 `data/packs` 時，設定頁就指向那裡，使用者把包
/// 放進 `%APPDATA%` 反而看不到。要在別的位置測就明講——把路徑填進
/// 設定裡，畫面上看得見，不會有「我明明放了怎麼沒出現」。
pub fn resolved_dir(custom: &str) -> Option<PathBuf> {
    let c = custom.trim();
    if !c.is_empty() {
        // 網路路徑不收，理由見 `config::is_remote_path`
        if crate::config::is_remote_path(c) {
            return None;
        }
        return Some(PathBuf::from(c));
    }
    crate::config::user_dir().map(|d| d.join("packs"))
}

/// 包**實際**在哪？資料夾不存在就是 `None`。
pub fn dir(custom: &str) -> Option<PathBuf> {
    resolved_dir(custom).filter(|p| p.is_dir())
}

/// 隨程式一起裝的那份包放在哪。
///
/// 平台層在啟動時設一次（DLL 旁邊的 `packs\`，開發環境是專案根的
/// `packs\`）。`core` 保持平台無關，不自己去猜執行檔在哪。
static BUNDLED: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();

fn bundled_slot() -> &'static RwLock<Option<PathBuf>> {
    BUNDLED.get_or_init(|| RwLock::new(None))
}

/// 預載的符號包叫什麼（檔名，不含 `.txt`）。
///
/// 這份原本是 `symbol.rs` 裡的 `BUILTIN` 常數，2026-09-05 抽出來變成
/// 隨程式一起裝的包。**檔名就是身分**，改了會讓已啟用的設定失效。
///
/// 它出現在設定頁的清單裡、可以取消勾選（使用者裁決）——所以
/// `Config::default()` 把它放進 `packs`，新使用者預設就有。
pub const BUNDLED_SYMBOLS: &str = "內建符號";

/// 預載的 emoji 包叫什麼（2026-09-07 加）。
///
/// **跟符號包分開兩個檔案**是使用者裁定的：只想要單色符號的人可以
/// 把這個關掉，反之亦然。名字撞到時兩邊的符號會接起來（見
/// `build_index`），所以分檔案不會讓誰打不出東西。
pub const BUNDLED_EMOJI: &str = "內建emoji";

/// 設定預載包的位置。平台層啟動時叫一次。
pub fn set_bundled_dir(dir: Option<PathBuf>) {
    write_or_recover(bundled_slot(), |g| *g = dir);
}

/// 預載包**實際**在哪？沒設或資料夾不存在就是 `None`。
pub fn bundled_dir() -> Option<PathBuf> {
    read_or_recover(bundled_slot()).filter(|p| p.is_dir())
}

/// 找包要掃哪些資料夾，**依優先序**。
///
/// 使用者的目錄排在預載目錄前面——同名時使用者的贏，所以想改內建
/// 符號的人在自己的包裡放一個同名的就蓋過去了，不必去動 Program Files
/// 底下那份（那裡升級會被覆蓋）。
fn dirs(custom: &str) -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(d) = dir(custom) {
        v.push(d);
    }
    if let Some(b) = bundled_dir() {
        // 開發環境兩者可能指到同一個地方，別掃兩次
        if !v.contains(&b) {
            v.push(b);
        }
    }
    v
}

/// 有哪些包可以啟用？回傳檔名（不含 `.txt`），依名稱排序。
///
/// 給設定頁列清單用。**只看檔案存不存在，不解析內容**——列清單要快。
pub fn available(custom: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for d in dirs(custom) {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for name in entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "txt"))
            .filter_map(|e| e.path().file_stem()?.to_str().map(str::to_string))
        {
            // 同名只列一次——`dirs()` 排好優先序了，使用者的那份贏
            if !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out.sort();
    out
}

/// 解析一個包的內容。格式錯的列直接跳過，不讓一行打錯毀掉整包。
fn parse(content: &str, into: &mut Packs) {
    for line in content.lines() {
        let line = line.trim_end_matches(['\u{d}', '\u{a}']);
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let mut f = line.split('\u{9}');
        let (Some(lang), Some(input)) = (f.next(), f.next()) else {
            continue;
        };
        let input = input.trim();
        if input.is_empty() {
            continue;
        }
        // 第三欄省略時，輸出就是輸入本身（英文的 passthrough）
        let output = f.next().map(str::trim).filter(|s| !s.is_empty());
        // 會送進文件的字串不能藏看不見的字元（雙向覆寫、零寬字元）。
        // 英文的輸入本身就是輸出，所以也要看
        if !crate::sanitize::is_safe_output(input)
            || output.is_some_and(|o| !crate::sanitize::is_safe_output(o))
        {
            continue;
        }
        match lang.trim() {
            "en" => into.en.push(input.to_ascii_lowercase()),
            "ja" => {
                if let Some(o) = output {
                    into.ja.push((input.to_string(), o.to_string()));
                }
            }
            "zh" => {
                if let Some(o) = output {
                    into.zh.push((input.to_string(), o.to_string()));
                }
            }
            // 台語：**跟 `zh` 同樣的形狀，但獨立一層**——它是「多一個
            // 選擇」不是「取代」，只給段選單用。見 `Packs::tw`。
            "tw" => {
                if let Some(o) = output {
                    into.tw.push((input.to_string(), o.to_string()));
                }
            }
            // 符號：第二欄是名字、第三欄是一串符號（`星` → `★☆✦`）
            //
            // **名字可以用逗號列多個**（`星,ほし,star`），三種語言叫得出
            // 同一組符號。這裡就地展開成多筆，後面每一關（索引、計數、
            // 查詢）都當成一列一個名字，不必知道別名這回事。
            //
            // 為什麼不是一列一個名字：那樣符號串要整串複製好幾次，改一個
            // 符號得記得改每一份，兩邊會漂掉。
            //
            // 全形逗號也算——名字是打出來的中文，很容易順手打成全形，
            // 而符號名字裡不會有逗號。
            "sym" => {
                if let Some(o) = output {
                    // **符號之間用空白分隔**。2026-09-07 改的——原本靠
                    // 「一個字元就是一個符號」，那對 emoji 全面壞掉：
                    // `👨‍👩‍👧` 是五個字元（中間夾零寬連接符）組成的
                    // 一個圖，逐字元拆會得到三個人加兩個看不見的字元。
                    // 膚色（`👍🏽`＝手＋色票）、變體選擇符（`☀️`）也一樣。
                    //
                    // 判準用 DirectWrite 的「叢集」量過：那才是使用者
                    // 眼裡的一個字，而它跟字元數對不上（見開發文件 §2.51）。
                    let syms = split_symbols(o);
                    if syms.is_empty() {
                        continue;
                    }
                    for name in input.split([',', '，']) {
                        let name = name.trim();
                        if !name.is_empty() {
                            into.sym.push((name.to_string(), syms.clone()));
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// 查詢用的索引。
///
/// `Packs` 是「照設定順序載進來的原始清單」，保留順序是為了衝突時
/// 前面的贏；但**熱路徑不能拿 Vec 做線性掃描**，所以另外建一份雜湊表。
/// 建的時候後面的不覆蓋前面的，優先序就落實在這裡。
///
/// `zh` 的鍵在檔案裡是注音符號，這裡已經轉成按鍵——轉換只在建索引時
/// 做一次，查詢時不做。
#[derive(Default, Debug)]
pub struct Index {
    pub en: HashSet<String>,
    /// 假名 → 表記
    pub ja: HashMap<String, String>,
    /// 按鍵 → 詞
    pub zh: HashMap<String, String>,
    /// **國字 → 同一個意思的所有說法**（華語＋台語）。只給段選單用。
    ///
    /// 跟這裡其他層有兩點不一樣，理由都見 `build_index`：
    ///
    /// - **鍵是國字不是按鍵**（按鍵有歧義，會撈到跨詞邊界的誤配）
    /// - **雙向**：查「沙發」跟查「膨椅」都拿到 `[沙發, 膨椅, 胖椅]`
    ///   ——它們是同一個東西的不同說法，地位相等
    ///
    /// 第一個元素固定是華語詞。
    pub tw: HashMap<String, Vec<String>>,
    /// 符號名 → 一組符號
    pub sym: HashMap<String, Vec<String>>,
}

impl Index {
    /// 一條都沒有？
    pub fn is_empty(&self) -> bool {
        !self.has_words() && self.sym.is_empty()
    }

    /// 有沒有**詞**？**熱路徑靠這個短路**——沒啟用詞包的人一次查詢都不多做。
    ///
    /// # 為什麼符號不算
    ///
    /// `any()` 擋的是 `dict`／`english` 的查詞路徑，符號查詢不走那裡
    /// （只在組字區出現 `\` 時才問一次，不是熱路徑）。內建符號抽成預載包
    /// 之後**每個人都會啟用一個純符號的包**，把 `sym` 算進來的話這道閘門
    /// 就永遠是開的，等於所有人開始付查詞的成本。
    pub fn has_words(&self) -> bool {
        !(self.en.is_empty() && self.ja.is_empty() && self.zh.is_empty())
    }
}

/// **可替換**的——這是選這套設計的理由。使用者在設定頁改了啟用的包，
/// 換掉這張表就生效，不必重建詞庫（詞庫是 `OnceLock`，重建不了）。
/// 之後 Phase 4 的個人化學習也掛在同一個位置。
static INDEX: OnceLock<RwLock<Arc<Index>>> = OnceLock::new();

fn slot() -> &'static RwLock<Arc<Index>> {
    INDEX.get_or_init(|| RwLock::new(Arc::new(Index::default())))
}

/// 有沒有啟用任何包？
///
/// **熱路徑的第一道關卡**。沒啟用包時查詞完全不該多付成本，而
/// `index()` 要拿讀鎖＋複製一次 `Arc`——實測那個成本量得出來。
/// 這裡一次 relaxed 的原子讀就擋掉了，是絕大多數使用者的路徑。
pub fn any() -> bool {
    HAS.load(Ordering::Relaxed)
}

/// 有沒有符號？
///
/// **跟 `any()` 分開**：`any()` 擋的是查詞路徑，而內建符號抽成預載包
/// 之後每個人都會啟用一個純符號的包——共用同一道旗標的話，不是讓查詞
/// 的人白付成本，就是讓符號查詢被短路掉（第一版就是後者，符號整個失效）。
pub fn any_sym() -> bool {
    HAS_SYM.load(Ordering::Relaxed)
}

static HAS: AtomicBool = AtomicBool::new(false);
static HAS_SYM: AtomicBool = AtomicBool::new(false);

/// 有沒有台語詞？
///
/// **第三道旗標**，理由跟 `any_sym` 一樣：台語只給段選單用，不走查詞
/// 熱路徑。併進 `any()` 的話，只裝台語包的人每次查詞都白付一次雜湊。
pub fn any_tw() -> bool {
    HAS_TW.load(Ordering::Relaxed)
}

static HAS_TW: AtomicBool = AtomicBool::new(false);

/// 查詢用的索引。沒載過就是空的。
///
/// 回傳 `Arc` 而不是借用——鎖只在取的瞬間持有，查詢期間不擋住換表。
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

/// 換掉整張表。設定改了就呼叫這個。
pub fn set_index(new: Index) {
    let has = new.has_words();
    let has_sym = !new.sym.is_empty();
    let has_tw = !new.tw.is_empty();
    write_or_recover(slot(), |g| *g = Arc::new(new));
    HAS.store(has, Ordering::Relaxed);
    HAS_SYM.store(has_sym, Ordering::Relaxed);
    HAS_TW.store(has_tw, Ordering::Relaxed);
    // **切點排序的分數快取要作廢**——包會改變 `claimed`／`is_top_word`
    // 的答案。啟動時載入沒差（那時還沒打字），但使用者中途換包就會
    // 拿到舊分數。跟 `learn::set_index` 同一個理由。
    crate::dict::bump_generation();
}

/// 第三欄怎麼拆成一個一個符號。
///
/// # 兩種寫法都收
///
/// **新格式用空白分隔**（`★ ☆ ✦`），因為 emoji 拆不了字元：`👍🏽`
/// 是手加色票兩個字元一個圖，逐字元拆會得到兩個殘缺的東西。
///
/// **舊格式是連著寫的**（`★☆✦`），2026-09-07 之前的包都長這樣，
/// 靠「一個字元就是一個符號」。使用者手上已經有這種包，改格式不能
/// 讓它們靜靜壞掉——那會變成打 `\括號\` 一口氣送出十四個字元。
///
/// 判準是**有沒有空白**：有就照空白切（新格式，作者明確表達了邊界），
/// 沒有就逐字元拆（舊格式）。兩者對單字元符號的結果完全一樣，所以
/// 舊包不必改也是對的；要寫多字元符號才非得用新格式不可。
///
/// 之所以能這樣分，是因為**多字元符號在舊格式裡本來就寫不出來**
/// ——舊的拆法會把它拆碎。所以「沒有空白」必然是舊格式，不會誤判。
fn split_symbols(o: &str) -> Vec<String> {
    if o.split_whitespace().count() > 1 {
        // 新格式：作者用空白標出了邊界
        return o
            .split_whitespace()
            .filter(|s| !s.is_empty())
            .map(expand_zwj)
            .collect();
    }
    // 只有一段，可能是舊格式的一整串，也可能是新格式只放一個符號。
    // `+` 是我們自己的 ZWJ 標記，先展開再判斷。
    let one = expand_zwj(o.trim());
    if one.is_empty() {
        return Vec::new();
    }
    // 展開出 ZWJ 的必然是新格式的單一組合符號，不能再拆
    if one.contains('\u{200D}') {
        return vec![one];
    }
    // 舊格式：逐字元。但**變體選擇符與膚色要跟著前一個字元**——
    // 舊包裡不會有這些（那時的表都是 BMP 單字元符號），這裡順手處理，
    // 免得有人手寫一個 `☀️` 卻被拆成兩份。
    let mut out: Vec<String> = Vec::new();
    for c in one.chars() {
        let combining = matches!(c, '\u{FE0F}' | '\u{FE0E}' | '\u{1F3FB}'..='\u{1F3FF}');
        match out.last_mut() {
            Some(prev) if combining => prev.push(c),
            _ => out.push(c.to_string()),
        }
    }
    out
}

/// 把 `👩+💻` 這種寫法展開成真正的 ZWJ 組合（`👩\u{200D}💻`）。
///
/// # 為什麼不直接在檔案裡寫真的 ZWJ
///
/// `sanitize` 擋掉包裡所有零寬字元——那道防線擋的是惡意包拿不可見
/// 字元讓「看到的」和「送出的」不一樣，理由完全正當，**不該為了
/// emoji 拆掉它**。
///
/// 但 ZWJ 在 emoji 裡是**構圖零件**不是偽裝：`👩‍💻` 就是「👩 加 💻」。
/// 兩件事的差別在於「誰放的」——包裡寫的是可見的 `+`，不可見的字元
/// 由我們的程式產生。防線一行都不用動，檔案也仍然肉眼可審：
/// 純文字打開來看得到 `👩+💻`，沒有藏東西的餘地。
///
/// # 為什麼是 `+`
///
/// 全形＋（U+FF0B）才是符號表裡的加號（`\加\` 那組），半形 `+` 兩份
/// 表都沒用到，掃過確認的。語意上也最直觀。
///
/// 真的要在符號表裡放半形 `+` 的話寫 `++`（跳脫），這裡照樣處理。
fn expand_zwj(s: &str) -> String {
    if !s.contains('+') {
        // 絕大多數符號走這條，不必配置
        return s.to_string();
    }
    const ZWJ: char = '\u{200D}';
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '+' {
            out.push(c);
            continue;
        }
        // `++` 是「我真的要一個加號」
        if chars.peek() == Some(&'+') {
            chars.next();
            out.push('+');
        } else {
            out.push(ZWJ);
        }
    }
    out
}

fn build_index(packs: &Packs) -> Index {
    let mut out = Index::default();
    for w in &packs.en {
        out.en.insert(w.clone());
    }
    for (k, v) in &packs.ja {
        out.ja.entry(k.clone()).or_insert_with(|| v.clone());
    }
    let rev = crate::dict::reverse_keymap();
    for (symbols, word) in &packs.zh {
        let Some(keys) = crate::dict::symbols_to_keys(symbols, &rev) else {
            continue;
        };
        // **字數要跟音節數對得上**——選詞層是一格填一個字的
        // （見 `compose::apply_word_context`），對不上的填不進去，
        // 而且多半代表包裡打錯了。
        let Some(syllables) = crate::bopomofo::split_syllables(&keys) else {
            continue;
        };
        if word.chars().count() != syllables.len() {
            continue;
        }
        out.zh.entry(keys).or_insert_with(|| word.clone());
    }
    // 台語：**鍵是華語國字，不是按鍵**——跟其他層都不一樣。
    //
    // 段選單拿組字區已經選好的國字去查（見 `session::segmenu` 的斷詞）。
    // 用按鍵查會撈到跨詞邊界的誤配：`z8 vu,4` 同時是「發謝」與「發洩」，
    // 「沙發謝謝」就會冒出「發洩／出氣／消解」一整排——那兩個字在畫面上
    // 根本沒相鄰過。歧義在選字階段已經解掉了，不必再對一次按鍵。
    //
    // 也**不套「字數等於音節數」那條限制**：`zh` 要求對得上是因為選詞層
    // 一格填一個字，台語是整個詞換掉，「他們→𪜶」兩字換一字完全沒問題。
    //
    // **雙向**：華語詞與每個台語說法都當鍵，指向同一組。
    //
    // 「沙發」與「膨椅」是同一個東西的兩種說法，地位相等——就像選字裡
    // 「好」與「郝」是同一個音的兩個字。查「沙發」跟查「膨椅」都該拿到
    // `[沙發, 膨椅, 胖椅]` 整組。
    //
    // 單向（只從華語查）的話，選成台語之後畫面上那個詞就查不到了，
    // **選不回中文**——得另外記「原本是什麼」再拿按鍵重算，而那份紀錄
    // 又會讓斷詞、往下跳、走完判斷全部失準（實測連續踩了五個）。
    // 雙向之後這些全部不必處理：不管畫面上是哪一個，查出來都一樣。
    //
    // 第一個元素固定是**華語詞**——段選單的「現況」要列它，而且使用者
    // 期待選回中文是第一順位。
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for (zh_word, tw_word) in &packs.tw {
        let v = groups
            .entry(zh_word.clone())
            .or_insert_with(|| vec![zh_word.clone()]);
        if !v.contains(tw_word) {
            v.push(tw_word.clone());
        }
    }
    for (zh_word, group) in groups {
        for key in std::iter::once(&zh_word).chain(group.iter().skip(1)) {
            let v = out.tw.entry(key.clone()).or_default();
            for w in &group {
                if !v.contains(w) {
                    v.push(w.clone());
                }
            }
        }
    }
    // 符號**同名時接起來**，不是先到的贏（2026-09-07 使用者裁定）。
    //
    // 內建符號包的 `\音樂\` 是 ♪♫♬♩、emoji 包的 `\音樂\` 是 🎵🎶🎤，
    // 對使用者來說「音樂的符號」本來就是同一件事，不該因為技術上分兩
    // 個檔案就要記兩個名字。原本的「先到的贏」還讓結果取決於包的啟用
    // 順序——**那是不可預期的行為**。
    //
    // **跟「同檔名覆蓋」是兩回事**：使用者自己寫一份 `內建符號.txt`
    // 仍然整份取代預載的那份（在 `read` 的「找到第一個就停」決定），
    // 那是「我要改掉這份表」。這裡處理的是「兩份不同的表剛好都有這個
    // 名字」，語意相反，合併不能動到前者。
    //
    // 順序就是包的啟用順序（前面的排前面），組內去重——兩份表都收了
    // ☀ 的話只留第一個，不然候選裡會有看起來一模一樣的兩格。
    for (name, syms) in &packs.sym {
        let slot = out.sym.entry(name.clone()).or_default();
        for s in syms {
            if !slot.contains(s) {
                slot.push(s.clone());
            }
        }
    }
    out
}

/// 依 `enabled` 的順序載入這些包，回傳總條數。
///
/// **可以重複呼叫**——包是獨立的一層，換掉索引就換掉了，不必重建
/// 詞庫（詞庫是 `OnceLock`，本來就重建不了）。設定改了就再叫一次。
pub fn load(custom: &str, enabled: &[String]) -> usize {
    let packs = read(custom, enabled);
    let index = build_index(&packs);
    let n = index.en.len() + index.ja.len() + index.zh.len();
    // 索引跟著換——查詢層只看索引，不看原始清單
    set_index(index);
    n
}

/// 讀出這些包的原始內容（不建索引）。
fn read(custom: &str, enabled: &[String]) -> Packs {
    let mut out = Packs::default();
    let ds = dirs(custom);
    for name in enabled {
        // **擋掉路徑穿越**：包名來自設定檔，不該能指到別的資料夾
        if name.contains(['/', '\\', ':']) || name.contains("..") {
            continue;
        }
        // 找到第一個就停——同名時使用者的那份蓋過預載的
        for d in &ds {
            if let Ok(content) = read_pack(&d.join(format!("{name}.txt"))) {
                parse(&content, &mut out);
                break;
            }
        }
    }
    out
}

/// 讀一個包的檔案內容，**吃掉 BOM**。
///
/// # 為什麼要獨立一支
///
/// 兩個洞都只在「包從別處來」時才踩得到（自己寫的包不會有），而開
/// 擴充包 repo 之後那就是常態。實測見 §2.49.3。
///
/// **BOM**：`parse_meta` 的 `strip_prefix('#')` 對 `\u{feff}#…` 失敗，
/// 而它的語意是「遇到第一行資料就停」，於是**整段檔頭在第一行就被
/// 判定結束**——包能用，但設定頁只顯示檔名，看起來像作者偷懶沒填。
/// `U+FEFF` 不屬於 Unicode 的 White_Space，`trim()` 清不掉它。
///
/// 而 BOM 正是從網頁「另存新檔」或用舊工具編輯最容易長出來的東西。
fn read_pack(path: &std::path::Path) -> Result<String, PackReadError> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(s.strip_prefix('\u{feff}').unwrap_or(&s).to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
            // **不是 UTF-8**（記事本的「ANSI」＝Big5）。
            //
            // 原本這個錯被吞掉當成「沒有內容」，設定頁顯示「還沒有詞
            // （沒填，或格式不對）」——但格式完全正確，錯的是編碼。
            // 使用者照那句話去檢查格式**永遠查不出來**。
            Err(PackReadError::NotUtf8)
        }
        Err(_) => Err(PackReadError::Missing),
    }
}

/// 包讀不起來的原因。**要分得出來**——見 `read_pack`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackReadError {
    /// 檔案不在，或讀不到
    Missing,
    /// 檔案在，但不是 UTF-8（多半是用記事本存成「ANSI」＝Big5）
    NotUtf8,
}

/// 這些包的最後修改時間（取最大值）。用來判斷「包被改過了要重載」。
///
/// **不能只看 `config.toml`**：使用者直接編輯包的內容不會動到設定檔，
/// 但那正是最常見的用法——加一個詞進去就想馬上能打。
pub fn stamp(custom: &str, enabled: &[String]) -> Option<std::time::SystemTime> {
    let ds = dirs(custom);
    enabled
        .iter()
        .filter_map(|n| {
            // 跟 `read` 同一條規則：找到第一個就是那一份
            ds.iter().find_map(|d| {
                std::fs::metadata(d.join(format!("{n}.txt")))
                    .ok()?
                    .modified()
                    .ok()
            })
        })
        .max()
}

/// 包的基本資料，寫在檔案開頭的註解區塊裡。
///
/// # 為什麼用註解而不是另一種語法
///
/// **舊包沒有這一段也要照樣能用**。寫成 `#` 註解的話，不認得它的
/// 版本就當註解跳過，行為完全不變；換成別的語法（例如 `[meta]` 區塊）
/// 就得先判斷是不是舊格式。
///
/// # 為什麼只看開頭
///
/// 只認「檔案開頭那一段連續註解」，遇到第一行資料就停。包的內容裡
/// 出現 `# name:` 不會被誤讀成基本資料，掃描也不必讀完整個檔。
///
/// # 檔名才是身分
///
/// 設定檔存的是**檔名**，`name` 只影響顯示。這樣使用者改了顯示名稱
/// 不會讓已啟用的包突然失效。
#[derive(Default, Debug, Clone)]
pub struct Meta {
    pub name: Option<String>,
    pub version: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub updated: Option<String>,
    pub homepage: Option<String>,
}

/// 讀檔頭的基本資料。認不得的鍵**忽略**——之後加欄位，舊版讀到不會壞。
fn parse_meta(content: &str) -> Meta {
    let mut m = Meta::default();
    for line in content.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        // 第一行資料就停——後面出現的 `# name:` 是內容不是檔頭
        let Some(rest) = t.strip_prefix('#') else {
            break;
        };
        let Some((k, v)) = rest.split_once(':') else {
            continue;
        };
        let v = v.trim();
        if v.is_empty() {
            continue;
        }
        let slot = match k.trim().to_ascii_lowercase().as_str() {
            "name" => &mut m.name,
            "version" => &mut m.version,
            "author" => &mut m.author,
            "description" => &mut m.description,
            "license" => &mut m.license,
            "updated" => &mut m.updated,
            "homepage" => &mut m.homepage,
            _ => continue,
        };
        // 重複出現時**第一個贏**，跟包的優先序一致
        slot.get_or_insert_with(|| v.to_string());
    }
    m
}

/// 一個包的完整資訊：身分、基本資料、三種語言各幾條。
#[derive(Debug, Clone)]
pub struct Info {
    /// 檔名（不含 `.txt`）。**這才是身分**，設定檔存的是它。
    pub file: String,
    pub meta: Meta,
    pub en: usize,
    pub ja: usize,
    pub zh: usize,
    /// 符號名字數。**一列列了幾個別名就算幾個**——設定頁顯示的是
    /// 「叫得出來的名字有幾個」，跟其他語言的條數同一個語意。
    pub sym: usize,
    /// 台語條數。
    pub tw: usize,
    /// **讀不起來的原因**，`None` 代表讀成功了。
    ///
    /// 原本 `read_to_string` 失敗一律吞掉，設定頁就顯示「還沒有詞
    /// （沒填，或格式不對）」——但 Big5 存的包**格式完全正確**，錯的
    /// 是編碼。使用者照那句話去檢查格式永遠查不出來（§2.49.3）。
    pub error: Option<PackReadError>,
}

impl Info {
    /// 顯示用的名字：檔頭有寫就用它，沒寫就用檔名。
    pub fn title(&self) -> &str {
        self.meta.name.as_deref().unwrap_or(&self.file)
    }

    /// 這個包一共幾條。
    ///
    /// **符號也要算**——設定頁用這個數字判斷「空包不給勾」，
    /// 漏掉的話只放符號的包會顯示 0 條、勾選框被停用，
    /// 等於**符號包裝不進去**。
    pub fn total(&self) -> usize {
        self.en + self.ja + self.zh + self.tw + self.sym
    }
}

/// 讀一個包的完整資訊（檔頭的基本資料＋各語言條數）。
///
/// 給設定頁顯示用——`available()` 刻意不解析內容（列清單要快），
/// 要看內容的是這一支，一次只讀一個檔。
pub fn info(custom: &str, file: &str) -> Info {
    let mut packs = Packs::default();
    let mut error = None;
    // **兩個目錄都要找**——`dirs()` 排好優先序（使用者的贏），只看
    // `dir()` 的話預載包（內建符號／emoji）永遠讀不到檔頭
    let meta = dirs(custom)
        .iter()
        .find_map(|d| {
            let p = d.join(format!("{file}.txt"));
            if !p.exists() {
                return None;
            }
            match read_pack(&p) {
                Ok(content) => {
                    parse(&content, &mut packs);
                    Some(parse_meta(&content))
                }
                Err(e) => {
                    // **讀不起來的原因要留著**，不能跟「沒有詞」共用
                    // 一句話（§2.49.3）
                    error = Some(e);
                    Some(Meta::default())
                }
            }
        })
        .unwrap_or_default();
    Info {
        file: file.to_string(),
        meta,
        en: packs.en.len(),
        ja: packs.ja.len(),
        zh: packs.zh.len(),
        sym: packs.sym.len(),
        tw: packs.tw.len(),
        error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn testdata() -> String {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata")
            .join("packs")
            .to_string_lossy()
            .into_owned()
    }

    /// 一列可以列好幾個名字，指向同一組符號。
    ///
    /// **這是符號表能三語共用的關鍵**——寫成一列一個名字的話符號串要
    /// 整串複製好幾份，改一個符號會漏改。全形逗號也要收，名字是打出來
    /// 的中文，很容易順手打成全形。
    #[test]
    fn 符號名字可以列好幾個() {
        let mut packs = Packs::default();
        parse(
            "sym	星,ほし,star	★ ☆ ✦
sym	心，heart	♥ ♡
",
            &mut packs,
        );
        assert_eq!(packs.sym.len(), 5, "五個名字都要展開成獨立的一筆");
        for name in ["星", "ほし", "star"] {
            assert_eq!(
                packs
                    .sym
                    .iter()
                    .find(|(k, _)| k == name)
                    .map(|(_, v)| v.as_slice()),
                Some(["★", "☆", "✦"].map(String::from).as_slice()),
                "{name} 指不到那組符號"
            );
        }
        // 全形逗號
        assert!(packs.sym.iter().any(|(k, _)| k == "heart"));
    }

    /// **符號用空白分隔，不是逐字元拆**。
    ///
    /// 這條擋的是 emoji 被拆碎：`👨‍👩‍👧` 是五個字元（中間夾零寬連接
    /// 符）組成的一個圖，逐字元拆會得到三個人加兩個看不見的字元，
    /// 而候選視窗會把那五份都列出來。膚色與變體選擇符同理。
    #[test]
    fn 符號用空白分隔而不是逐字元拆() {
        let mut packs = Packs::default();
        parse("sym	表情,kaomoji,face	👍🏽 ☀️ 😂\n", &mut packs);
        let (_, syms) = &packs.sym[0];
        assert_eq!(syms.len(), 3, "三個符號，不是拆成一堆字元");
        assert_eq!(syms[0].chars().count(), 2, "膚色是手＋色票，兩個字元一個圖");
        assert_eq!(syms[1].chars().count(), 2, "變體選擇符要留著");
        assert_eq!(syms[2].chars().count(), 1, "單一碼位的照常");
    }

    /// **2026-09-07 之前的包（符號連著寫）要照樣能用**。
    ///
    /// 使用者手上已經有這種包。改格式不能讓它們靜靜壞掉——實際踩到
    /// 過：改完之後打 `\括號\` 一口氣送出十四個字元，而不是給一組
    /// 候選讓人挑。沒報錯、沒當機，就是行為變了。
    #[test]
    fn 舊格式連著寫的照樣拆得開() {
        let mut packs = Packs::default();
        parse("sym	括號,brackets	（）〔〕【】\n", &mut packs);
        let (_, syms) = &packs.sym[0];
        assert_eq!(syms.len(), 6, "沒有空白就逐字元拆");
        assert_eq!(syms[0], "（");
    }

    /// 只放一個符號時兩種格式長得一樣，不能誤判。
    #[test]
    fn 只有一個符號時不會被誤拆() {
        let mut packs = Packs::default();
        parse("sym	加,plus	＋\n", &mut packs);
        assert_eq!(packs.sym[0].1, vec!["＋"]);

        // 一個多字元的組合符號也是一段，但不該拆
        let mut p2 = Packs::default();
        parse("sym	工程師,dev	👩+💻\n", &mut p2);
        assert_eq!(p2.sym[0].1.len(), 1, "ZWJ 組合是一個符號");
    }

    /// 舊格式裡萬一有變體選擇符或膚色，要跟著前一個字元走。
    ///
    /// 舊包不會有（那時的表都是單字元符號），但手寫一個進去不該被
    /// 拆成兩份殘缺的東西。
    #[test]
    fn 舊格式的變體選擇符跟著前一個字() {
        let mut packs = Packs::default();
        parse("sym	太陽,sun	☀️🌞\n", &mut packs);
        let (_, syms) = &packs.sym[0];
        assert_eq!(syms.len(), 2, "☀️ 是一個符號不是兩個");
        assert_eq!(syms[0].chars().count(), 2, "太陽帶著變體選擇符");
    }

    /// **ZWJ 組合在檔案裡寫成 `+`，載入時才展開**（2026-09-07）。
    ///
    /// `👩‍💻` 是 👩 用零寬連接符黏上 💻，而 `sanitize` 擋掉包裡所有
    /// 零寬字元——那道防線擋的是惡意包拿不可見字元偽裝輸出，**不該
    /// 為了 emoji 拆掉**。寫成 `👩+💻` 兩邊都成立：檔案裡沒有不可見
    /// 字元（肉眼可審、防線不動），組合照樣打得出來。
    #[test]
    fn zwj寫成加號載入時展開() {
        let mut packs = Packs::default();
        parse("sym	工程師,puroguramaa,dev	👩+💻 👨+💻\n", &mut packs);
        let (_, syms) = &packs.sym[0];
        assert_eq!(syms.len(), 2);
        assert_eq!(syms[0], "👩\u{200D}💻", "+ 要展開成真正的 ZWJ");
        assert_eq!(syms[0].chars().count(), 3, "兩個 emoji 夾一個 ZWJ");
    }

    /// 三個以上串起來的也要（家庭那批）。
    #[test]
    fn 多段zwj組合() {
        let mut packs = Packs::default();
        parse("sym	家庭,kazoku,family	👨+👩+👧\n", &mut packs);
        assert_eq!(packs.sym[0].1[0], "👨\u{200D}👩\u{200D}👧");
    }

    /// 真的要放半形加號時寫 `++`。
    ///
    /// 符號表現在用的是全形＋（`\加\` 那組），但別人的包可能要放
    /// 半形的，得留一條路。
    #[test]
    fn 加號自己用兩個寫() {
        let mut packs = Packs::default();
        parse("sym	加號,plus	++\n", &mut packs);
        assert_eq!(packs.sym[0].1[0], "+", "++ 是一個字面上的加號");
    }

    /// 展開後**仍然不能有別的不可見字元**——防線在展開之前就檢查過。
    ///
    /// 這條擋的是「拿 emoji 當藉口把 sanitize 繞過去」：檔案裡直接
    /// 藏零寬字元照樣被擋，能產生 ZWJ 的只有我們自己的 `+` 展開。
    #[test]
    fn 檔案裡直接藏零寬字元照樣被擋() {
        let mut packs = Packs::default();
        parse("sym	假的,fake	a\u{200B}b\n", &mut packs);
        assert!(packs.sym.is_empty(), "零寬空白不是 ZWJ 展開，要照擋");
    }

    /// **不同包的同一個符號名要合併，不是先到的贏**（2026-09-07）。
    ///
    /// 內建符號包的 `\音樂\` 是 ♪♫，emoji 包的是 🎵🎶，對使用者來說
    /// 那本來就是同一件事。原本的「先到的贏」還讓結果取決於包的啟用
    /// 順序——**那是不可預期的行為**。
    ///
    /// 順序照包的啟用順序，重複的只留一份（兩份表都收 ☀ 的話候選裡
    /// 不該出現看起來一模一樣的兩格）。
    #[test]
    fn 不同包的同名符號要合併() {
        let mut packs = Packs::default();
        // 兩個包各有一組 `音樂`，第二個包還跟第一個重複一個符號
        parse("sym	音樂,music	♪ ♫\n", &mut packs);
        parse("sym	音樂,music	🎵 🎶 ♪\n", &mut packs);
        let idx = build_index(&packs);
        assert_eq!(
            idx.sym.get("音樂").map(Vec::as_slice),
            Some(["♪", "♫", "🎵", "🎶"].map(String::from).as_slice()),
            "兩邊要接起來、先載的排前面、重複的只留一份"
        );
    }

    /// 空的符號欄位不該產生一筆「沒有符號」的名字。
    ///
    /// 那會讓 `\名字\` 查得到卻換不出東西——`merge_symbols` 拿
    /// `syms[0]` 會 panic。
    #[test]
    fn 符號欄位是空白的就跳過() {
        let mut packs = Packs::default();
        parse("sym	空的,empty	   \n", &mut packs);
        assert!(packs.sym.is_empty(), "只有空白的符號欄要整列跳過");
    }

    /// 只放符號的包**不是空包**。
    ///
    /// `total()` 漏算 `sym` 的話設定頁會顯示「0 條」，而「空包不給勾」
    /// 那條規則會把勾選框停用——**純符號包整個裝不進去**。
    /// 兩個各自都對的規則撞在一起，症狀是功能不可用而不是報錯。
    #[test]
    fn 只放符號的包不是空包() {
        let i = info(&testdata(), "sym_only_pack");
        assert_eq!((i.en, i.ja, i.zh), (0, 0, 0), "這個包只有符號");
        assert_eq!(i.sym, 1);
        assert!(i.total() > 0, "純符號包被當成空包了，設定頁會不給勾");
    }

    /// **這是選這套設計的理由**：換設定就換索引，不必重建詞庫。
    ///
    /// 詞庫是 `OnceLock`，一個行程裡重建不了；包獨立成一層之後，
    /// 停用一個包只是把索引換掉，下一次查詢就看不到它了。
    #[test]
    fn 換設定就換索引() {
        let d = testdata();
        let n = load(&d, &["test_pack".to_string()]);
        assert_eq!(n, 3, "英日中各一條");
        assert!(any());
        assert!(index().en.contains("zzpacktestword"));
        assert_eq!(
            index().ja.get("っっぱっく").map(String::as_str),
            Some("パック試験")
        );

        // 停用——索引換成空的，熱路徑的旗標也跟著關
        let n = load(&d, &[]);
        assert_eq!(n, 0);
        assert!(!any());
        assert!(index().en.is_empty());
    }

    /// 包會流通：藏了雙向覆寫或零寬字元的條目，使用者看到的跟送進
    /// 文件的不一樣。整條丟掉，其餘不受影響。
    #[test]
    fn 藏了看不見字元的條目不收() {
        let mut p = Packs::default();
        parse(
            "en\tgithub\u{202E}\nen\tgood\nzh\tㄋㄧˇ\t你\u{200B}\nzh\tㄏㄠˇ\t好\nja\tあ\t亜\u{2066}\n",
            &mut p,
        );
        assert_eq!(p.en, vec!["good"]);
        assert_eq!(p.zh.len(), 1);
        assert_eq!(p.zh[0].1, "好");
        assert!(p.ja.is_empty());
    }

    #[test]
    fn 包的路徑不收網路路徑() {
        assert_eq!(resolved_dir("\\\\evil\\share\\packs"), None);
        assert!(resolved_dir("C:\\somewhere\\packs").is_some());
    }

    #[test]
    fn 讀得到檔頭的基本資料() {
        let m = parse_meta(
            "# name: Hololive 詞庫
# version: 1.2
# 這行只是註解
en	hololive
",
        );
        assert_eq!(m.name.as_deref(), Some("Hololive 詞庫"));
        assert_eq!(m.version.as_deref(), Some("1.2"));
        assert!(m.author.is_none());
    }

    /// **遇到第一行詞就停**——包的內容裡出現 `# name:` 是註解，
    /// 不該被當成這個包的名字。
    #[test]
    fn 檔頭只認開頭那一段() {
        let m = parse_meta(
            "# name: 真的名字
en	word
# name: 假的名字
",
        );
        assert_eq!(m.name.as_deref(), Some("真的名字"));
    }

    /// 認不得的鍵忽略——之後加欄位，舊版程式讀到不會壞。
    #[test]
    fn 未知的鍵忽略() {
        let m = parse_meta(
            "# name: 包
# 未來才有的欄位: 值
",
        );
        assert_eq!(m.name.as_deref(), Some("包"));
    }

    /// 沒有檔頭的舊包照樣能用，顯示名退回檔名。
    #[test]
    fn 沒有檔頭就用檔名() {
        let info = Info {
            file: "我的包".into(),
            meta: parse_meta(
                "en	word
",
            ),
            en: 1,
            ja: 0,
            sym: 0,
            zh: 0,
            tw: 0,
            error: None,
        };
        assert_eq!(info.title(), "我的包");
        assert_eq!(info.total(), 1);
    }

    /// 字數跟音節數對不上的中文條目要被擋掉——選詞層是一格填一個字，
    /// 對不上的填不進去，多半代表包裡打錯了。
    #[test]
    fn 中文條目字數要對得上() {
        let mut p = Packs::default();
        parse(
            "zh	ㄗˋㄗˋㄆㄞˋ	資自派
zh	ㄗˋㄗˋㄆㄞˋ	兩字
",
            &mut p,
        );
        assert_eq!(p.zh.len(), 2, "解析不管字數，兩條都收");
        let idx = build_index(&p);
        assert_eq!(idx.zh.len(), 1, "建索引時擋掉對不上的那條");
    }

    #[test]
    fn 解析三種語言() {
        let mut p = Packs::default();
        parse(
            "# 註解\n\
             en\thololive\n\
             ja\tほろらいぶ\tホロライブ\n\
             zh\tㄏㄨˊㄊㄠˊ\t胡桃\n",
            &mut p,
        );
        assert_eq!(p.en, ["hololive"]);
        assert_eq!(p.ja, [("ほろらいぶ".to_string(), "ホロライブ".to_string())]);
        assert_eq!(p.zh, [("ㄏㄨˊㄊㄠˊ".to_string(), "胡桃".to_string())]);
    }

    #[test]
    fn 英文可以省略第三欄() {
        let mut p = Packs::default();
        parse("en\tHoloLive\n", &mut p);
        // 詞典是小寫的，比對前要轉
        assert_eq!(p.en, ["hololive"]);
    }

    #[test]
    fn 中日文沒有第三欄就跳過() {
        // 沒有輸出的話不知道要顯示什麼，那一列無效
        let mut p = Packs::default();
        parse("ja\tほろらいぶ\nzh\tㄏㄨˊㄊㄠˊ\n", &mut p);
        assert!(p.ja.is_empty() && p.zh.is_empty());
    }

    #[test]
    fn 格式錯的列不會毀掉整包() {
        let mut p = Packs::default();
        parse("這行沒有分隔\nxx\t不認得的語言\n\nen\tok\n", &mut p);
        assert_eq!(p.en, ["ok"]);
    }
}

/// **包從別處來**才踩得到的兩個洞（§2.49.3）。
///
/// 自己寫的包不會有 BOM、也不會是 Big5——這兩個是「從網頁另存新檔」
/// 或「用記事本編輯」才長出來的。開擴充包 repo 之後那就是常態。
#[cfg(test)]
mod 從別處來的包 {
    use super::*;

    fn testdata() -> String {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata")
            .join("packs")
            .to_string_lossy()
            .into_owned()
    }

    /// **BOM 不能吃掉檔頭**。
    ///
    /// `parse_meta` 的 `strip_prefix('#')` 對 `\u{feff}#…` 失敗，而它
    /// 的語意是「遇到第一行資料就停」——於是整段檔頭在第一行就結束。
    /// 症狀是「包能用，但設定頁只顯示檔名」，看起來像作者偷懶沒填。
    #[test]
    fn bom_不會吃掉檔頭() {
        let info = info(&testdata(), "bom_pack");
        assert_eq!(
            info.meta.name.as_deref(),
            Some("BOM 測試包"),
            "檔頭被 BOM 吃掉了"
        );
        assert_eq!(info.meta.version.as_deref(), Some("1.2"));
        assert_eq!(info.meta.author.as_deref(), Some("測試"));
        assert!(info.en > 0, "詞也要讀得到");
        assert_eq!(info.error, None);
    }

    /// **Big5 要講「編碼不對」，不能跟「沒有詞」共用一句話**。
    ///
    /// 記事本的「ANSI」存出來就是 Big5。那種包**格式完全正確**，
    /// 使用者照「格式不對」那句話去檢查格式永遠查不出來。
    #[test]
    fn big5_要說得出真正的原因() {
        let info = info(&testdata(), "big5_pack");
        assert_eq!(
            info.error,
            Some(PackReadError::NotUtf8),
            "該指出是編碼問題，不是「沒有詞」"
        );
        assert_eq!(info.total(), 0, "讀不進來所以是 0 條");
    }

    /// 正常的 UTF-8 包不受影響。
    #[test]
    fn 正常的包照舊() {
        let info = info(&testdata(), "test_pack");
        assert_eq!(info.error, None);
        assert!(info.total() > 0);
    }
}
