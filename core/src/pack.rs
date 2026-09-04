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
    /// 符號名 → 一組符號（`\星\` 那條路，見 `crate::symbol`）。
    ///
    /// **不分語言**——名字是「組出來的文字」，中文的「星」與日文的
    /// 「星」本來就是同一個字串。
    pub sym: Vec<(String, String)>,
}

impl Packs {
    /// 三種語言加起來幾條。
    pub fn len(&self) -> usize {
        self.en.len() + self.ja.len() + self.zh.len() + self.sym.len()
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

/// 有哪些包可以啟用？回傳檔名（不含 `.txt`），依名稱排序。
///
/// 給設定頁列清單用。**只看檔案存不存在，不解析內容**——列清單要快。
pub fn available(custom: &str) -> Vec<String> {
    let Some(d) = dir(custom) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&d) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "txt"))
        .filter_map(|e| e.path().file_stem()?.to_str().map(str::to_string))
        .collect();
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
            // 符號：第二欄是名字、第三欄是一串符號（`星` → `★☆✦`）
            "sym" => {
                if let Some(o) = output {
                    into.sym.push((input.to_string(), o.to_string()));
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
    /// 符號名 → 一組符號
    pub sym: HashMap<String, String>,
}

impl Index {
    /// 一條都沒有？**熱路徑靠這個短路**——沒啟用包的人一次查詢都不多做。
    pub fn is_empty(&self) -> bool {
        self.en.is_empty() && self.ja.is_empty() && self.zh.is_empty() && self.sym.is_empty()
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

static HAS: AtomicBool = AtomicBool::new(false);

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
    let has = !new.is_empty();
    write_or_recover(slot(), |g| *g = Arc::new(new));
    HAS.store(has, Ordering::Relaxed);
    // **切點排序的分數快取要作廢**——包會改變 `claimed`／`is_top_word`
    // 的答案。啟動時載入沒差（那時還沒打字），但使用者中途換包就會
    // 拿到舊分數。跟 `learn::set_index` 同一個理由。
    crate::dict::bump_generation();
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
    for (name, syms) in &packs.sym {
        out.sym.entry(name.clone()).or_insert_with(|| syms.clone());
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
    let Some(d) = dir(custom) else {
        return out;
    };
    for name in enabled {
        // **擋掉路徑穿越**：包名來自設定檔，不該能指到別的資料夾
        if name.contains(['/', '\\', ':']) || name.contains("..") {
            continue;
        }
        let p = d.join(format!("{name}.txt"));
        if let Ok(content) = std::fs::read_to_string(&p) {
            parse(&content, &mut out);
        }
    }
    out
}

/// 這些包的最後修改時間（取最大值）。用來判斷「包被改過了要重載」。
///
/// **不能只看 `config.toml`**：使用者直接編輯包的內容不會動到設定檔，
/// 但那正是最常見的用法——加一個詞進去就想馬上能打。
pub fn stamp(custom: &str, enabled: &[String]) -> Option<std::time::SystemTime> {
    let d = dir(custom)?;
    enabled
        .iter()
        .filter_map(|n| {
            std::fs::metadata(d.join(format!("{n}.txt")))
                .ok()?
                .modified()
                .ok()
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
    /// 符號組數
    pub sym: usize,
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
        self.en + self.ja + self.zh + self.sym
    }
}

/// 讀一個包的完整資訊（檔頭的基本資料＋各語言條數）。
///
/// 給設定頁顯示用——`available()` 刻意不解析內容（列清單要快），
/// 要看內容的是這一支，一次只讀一個檔。
pub fn info(custom: &str, file: &str) -> Info {
    let mut packs = Packs::default();
    let meta = match dir(custom).map(|d| d.join(format!("{file}.txt"))) {
        Some(p) => match std::fs::read_to_string(&p) {
            Ok(content) => {
                parse(&content, &mut packs);
                parse_meta(&content)
            }
            Err(_) => Meta::default(),
        },
        None => Meta::default(),
    };
    Info {
        file: file.to_string(),
        meta,
        en: packs.en.len(),
        ja: packs.ja.len(),
        zh: packs.zh.len(),
        sym: packs.sym.len(),
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
