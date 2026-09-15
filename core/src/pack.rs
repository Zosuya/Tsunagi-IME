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

use std::collections::HashMap;
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

/// 有哪些包可以啟用？回傳檔名（不含副檔名），依名稱排序。
///
/// 給設定頁列清單用。**只看檔案存不存在，不解析內容**——列清單要快。
///
/// **`.txt` 與 `.bin` 都算**：官方包是二進位的（§2.83），只認 `.txt`
/// 的話它們在設定頁完全看不到，使用者無從勾選。同名的兩種只列一次
/// ——`find_pack` 決定實際載哪一個。
pub fn available(custom: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for d in dirs(custom) {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for name in entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .is_some_and(|x| x == "txt" || x == "bin")
            })
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

/// 指進 `Pool` 的一段字。**`Index` 內部用，不對外**。
///
/// 為什麼不存 `String`：見 `Pool` 的說明。`u32` 夠用——單一包的
/// 文字總量遠小於 4GB（台語 9 萬筆才 695KB）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct Str {
    off: u32,
    len: u32,
}

/// 指進 `Index::tw_flat` 的一段。理由同 `Str`——避免每組各配一個 `Vec`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Span {
    at: u32,
    len: u32,
}

/// 所有字串黏成一塊的池子。
///
/// # 為什麼要有它
///
/// 2026-09-15 量到台語包 **1.65MB 的檔案在記憶體裡變成 30.8MB**
/// （45 倍）。原因不是資料量——那 82373 個詞的純文字只有 695KB——
/// 而是每個 `String` 的固定成本：header 24 bytes，而平均詞長只有
/// 8.4 bytes，**內容佔不到兩成**。再加上雙向索引讓同一個詞被獨立
/// 配置好幾次（每個詞平均存 3.7 次）。
///
/// 池子把所有詞黏成一塊連續記憶體，索引只存 `(off, len)`：
/// **配置次數從 18 萬降到個位數**。
///
/// # 為什麼用 `(off, len)` 不用 `&'static str`
///
/// `&'static str`（`Box::leak` 一次）查詢端完全不用改，但官方包之後
/// 要走 mmap 二進位，那時檔案裡存的本來就是 `(off, len)`——用整數對
/// 是**一步到位**，池子換成映射的位元組時查詢端不必再動一次。
///
/// 見 [[擴充包吃掉 30MB：字串池與官方包二進位化]]。
#[derive(Default, Debug)]
struct Pool {
    buf: String,
    /// 去重用：同一個詞只進池子一次。建完索引就丟掉（查詢不需要）。
    seen: HashMap<String, Str>,
}

impl Pool {
    /// 把一個詞放進池子，回傳指標。**同樣的詞回同一個指標**。
    fn put(&mut self, s: &str) -> Str {
        if let Some(&at) = self.seen.get(s) {
            return at;
        }
        let at = Str {
            off: self.buf.len() as u32,
            len: s.len() as u32,
        };
        self.buf.push_str(s);
        self.seen.insert(s.to_string(), at);
        at
    }

    /// 切出那段字。
    #[inline]
    fn get(&self, at: Str) -> &str {
        let a = at.off as usize;
        &self.buf[a..a + at.len as usize]
    }

    /// 建完索引之後丟掉去重表——它跟池子一樣大，留著等於白付一份。
    fn shrink(&mut self) {
        self.seen = HashMap::new();
        self.buf.shrink_to_fit();
    }
}

/// 一層「鍵 → 值」的排序表。**六層共用同一個形狀**。
///
/// # 為什麼六層都改成這個
///
/// 原本只有 `tw` 是排序表（它最大，先被逼著改），其他五層還是
/// `HashMap<String, …>`。但官方包要 mmap 二進位化，而 `HashMap` **沒
/// 辦法在映射的位元組上直接查**——載入時得重建一份到堆上，那正是
/// mmap 想避免的事（共用的頁沒人碰，私有的那份照樣每個宿主一份）。
///
/// 排序表沒有這個問題：一塊排序好的 `(鍵指標, 值範圍)` 加一塊池子，
/// 二分搜尋只讀不寫。實測 spike 五個行程私有 27.7MB → 3.5MB。
///
/// # 值為什麼是範圍而不是單一指標
///
/// 六層的值形狀不同：`en` 沒有值、`ja`／`zh`／`zh_long` 是一個字串、
/// `sym`／`tw` 是一組。**用「範圍」一種形狀通吃**——單值就是長度 1 的
/// 範圍，多了一次間接但省掉六套程式碼與六套二進位版面。
///
/// 範圍指進 `Index::flat`（全部層共用一條），避免每組各配一個 `Vec`：
/// 台語 82373 個 `Vec` 光 header 就 8MB（實測）。
#[derive(Default, Debug)]
struct Table {
    /// 依鍵的**字面**排序（不是指標順序），查詢走二分搜尋。
    rows: Vec<(Str, Span)>,
}

impl Table {
    /// 二分搜尋那個鍵，回傳值的範圍。
    ///
    /// **比的是字面不是指標**——指標的順序是進池子的順序，跟字典序無關。
    #[inline]
    fn find(&self, pool: &Pool, key: &str) -> Option<Span> {
        self.rows
            .binary_search_by(|(k, _)| pool.get(*k).cmp(key))
            .ok()
            .map(|i| self.rows[i].1)
    }

    /// 有這個鍵嗎？**不取值**——`en` 那層只需要這個。
    #[inline]
    fn has(&self, pool: &Pool, key: &str) -> bool {
        self.rows
            .binary_search_by(|(k, _)| pool.get(*k).cmp(key))
            .is_ok()
    }

    fn len(&self) -> usize {
        self.rows.len()
    }

    fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// 建表用的暫存：先收集再排序。**只在 `build_index` 活著**。
///
/// 收集階段需要「同一個鍵出現第二次要找得到」，那是雜湊的工作；
/// 但查詢階段不需要雜湊。兩個階段用兩種結構，建完就丟。
#[derive(Default)]
struct Builder {
    /// 鍵 → 值清單。**保留插入順序**——`sym` 要「同名接起來」而且
    /// 順序就是包的啟用順序。
    rows: HashMap<Str, Vec<Str>>,
    /// 插入順序，`rows` 的 `HashMap` 不保序，但排序前要先有穩定的來源
    order: Vec<Str>,
}

impl Builder {
    /// 收一筆。**同一個鍵重複收就接在後面**（去重）。
    fn push(&mut self, key: Str, val: Str) {
        let slot = match self.rows.entry(key) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(e) => {
                self.order.push(key);
                e.insert(Vec::new())
            }
        };
        if !slot.contains(&val) {
            slot.push(val);
        }
    }

    /// 收一筆「只有鍵沒有值」（`en` 那層）。
    fn push_key(&mut self, key: Str) {
        if let std::collections::hash_map::Entry::Vacant(e) = self.rows.entry(key) {
            self.order.push(key);
            e.insert(Vec::new());
        }
    }

    /// 收一筆「第一個贏」的（`ja`／`zh`／`zh_long`：同鍵時前面的包優先）。
    fn push_first(&mut self, key: Str, val: Str) {
        if let std::collections::hash_map::Entry::Vacant(e) = self.rows.entry(key) {
            e.insert(vec![val]);
            self.order.push(key);
        }
    }

    /// 排序、攤平進 `flat`，產出可查詢的表。
    fn finish(self, pool: &Pool, flat: &mut Vec<Str>) -> Table {
        let mut rows: Vec<(Str, Vec<Str>)> = self
            .order
            .into_iter()
            .filter_map(|k| self.rows.get(&k).map(|v| (k, v.clone())))
            .collect();
        rows.sort_by(|(a, _), (b, _)| pool.get(*a).cmp(pool.get(*b)));
        let mut out = Table {
            rows: Vec::with_capacity(rows.len()),
        };
        flat.reserve(rows.iter().map(|(_, v)| v.len()).sum::<usize>());
        for (k, vals) in rows {
            let span = Span {
                at: flat.len() as u32,
                len: vals.len() as u32,
            };
            flat.extend_from_slice(&vals);
            out.rows.push((k, span));
        }
        out
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
    /// 英文詞（已轉小寫）。**只有鍵沒有值**——問的是「認不認得這個詞」。
    en: Table,
    /// 假名 → 表記
    ja: Table,
    /// 按鍵 → 詞
    zh: Table,
    /// **按鍵 → 長輸出**（字數跟音節數對不上的那些）。
    ///
    /// 為什麼要獨立一層：`zh` 那層的詞是**逐格填字**的
    /// （`compose::apply_word_context` 一格填一個字），字數對不上根本
    /// 填不回去。這一層走另一條路——`compose::merge_pack_long` 把那幾格
    /// **併成一格**，跟 `merge_symbols` 同一個模式。
    ///
    /// 兩層分開而不是合併成一個的理由：詞層要挑「跟使用者選過的字相容」
    /// 的那一個，長輸出沒有逐字對應關係，那套邏輯整個不適用。
    zh_long: Table,
    /// **國字 → 同一個意思的所有說法**（華語＋台語）。只給段選單用。
    ///
    /// 跟這裡其他層有兩點不一樣，理由都見 `build_index`：
    ///
    /// - **鍵是國字不是按鍵**（按鍵有歧義，會撈到跨詞邊界的誤配）
    /// - **雙向**：查「沙發」跟查「膨椅」都拿到 `[沙發, 膨椅, 胖椅]`
    ///   ——它們是同一個東西的不同說法，地位相等
    ///
    /// 第一個元素固定是華語詞。
    tw: Table,
    /// 符號名 → 一組符號
    sym: Table,
    /// **六層的值全部攤平成一條**，各層的 `Span` 指進來。
    ///
    /// 為什麼共用一條而不是各層一條：`Vec` 本身有成本（24 bytes header
    /// ＋ 一次堆配置），台語 82373 組各自一個 `Vec` 光 header 就 8MB
    /// （實測）。攤平之後整個 `Index` 只剩「池子 ＋ 六張表 ＋ 這條」。
    ///
    /// **這正是二進位檔的版面**——dump 出去就是檔案，見 §2.83。
    flat: Vec<Str>,
    /// 六層共用的字串池。
    pool: Pool,
    /// **官方包**：mmap 進來的 `.bin`，每一份都是借用的位元組。
    ///
    /// # 為什麼另外掛一層，不併進上面那六張表
    ///
    /// 併進去就得把映射的內容抄一份到池子裡——那正是 mmap 要避免的事
    /// （抄完之後每個宿主行程各一份，共用的頁沒人碰）。分開放，查詢時
    /// 兩邊各問一次，官方包的位元組從頭到尾沒被複製過。
    ///
    /// **順序**：使用者的包（上面六層）先問，官方包後問。理由跟同名
    /// 檔案「使用者的優先」一致——使用者自己寫的東西不該被官方包蓋掉。
    bins: Vec<&'static crate::pack_bin::PackBin>,
}

/// 六層在 `.bin` 裡的編號。**寫檔與讀檔共用**，見 `pack_bin::LAYERS`。
mod layer {
    pub const EN: usize = 0;
    pub const JA: usize = 1;
    pub const ZH: usize = 2;
    pub const ZH_LONG: usize = 3;
    pub const TW: usize = 4;
    pub const SYM: usize = 5;
}

impl Index {
    /// 一條都沒有？
    pub fn is_empty(&self) -> bool {
        !self.has_words() && self.sym.is_empty() && self.bins.is_empty()
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
        !(self.en.is_empty() && self.ja.is_empty() && self.zh.is_empty() && self.zh_long.is_empty())
            || self.bins.iter().any(|b| {
                [layer::EN, layer::JA, layer::ZH, layer::ZH_LONG]
                    .iter()
                    .any(|&l| !b.is_empty(l))
            })
    }

    /// 切出一個範圍裡的字串。**六層的取值都走這裡**。
    #[inline]
    fn vals(&self, s: Span) -> impl Iterator<Item = &str> {
        self.flat[s.at as usize..(s.at + s.len) as usize]
            .iter()
            .map(|&p| self.pool.get(p))
    }

    /// 範圍裡的第一個。`ja`／`zh`／`zh_long` 是單值層，值永遠只有一個。
    #[inline]
    fn first(&self, s: Span) -> Option<&str> {
        (s.len > 0).then(|| self.pool.get(self.flat[s.at as usize]))
    }

    /// 把一層倒成 `(鍵, 值清單)`。**只給 `layers_for_bin` 產 `.bin` 用**。
    ///
    /// 這是唯一一處把池子裡的東西複製回 `String` 的地方——產檔是一次性
    /// 的離線工作，不在任何熱路徑上，為它多開一套零複製的門不划算。
    ///
    /// 順序照 `rows` 原樣搬，也就是**已經依鍵的字面排序好**的順序。
    fn dump(&self, t: &Table) -> Vec<(String, Vec<String>)> {
        t.rows
            .iter()
            .map(|&(k, span)| {
                (
                    self.pool.get(k).to_string(),
                    self.vals(span).map(str::to_string).collect(),
                )
            })
            .collect()
    }

    /// 問一遍官方包，第一個答得出來的贏。**使用者的包已經先問過了**。
    #[inline]
    fn ask_bins<T>(&self, f: impl Fn(&'static crate::pack_bin::PackBin) -> Option<T>) -> Option<T> {
        self.bins.iter().find_map(|b| f(b))
    }

    // ── en：認不認得這個英文詞 ──

    /// 認得這個英文詞嗎？**鍵要先轉小寫**（建表時就是小寫）。
    pub fn en_has(&self, word: &str) -> bool {
        self.en.has(&self.pool, word) || self.bins.iter().any(|b| b.has(layer::EN, word))
    }

    pub fn en_len(&self) -> usize {
        self.en.len() + self.bins.iter().map(|b| b.len(layer::EN)).sum::<usize>()
    }

    // ── ja／zh／zh_long：單值層 ──

    /// 這個假名的表記。
    pub fn ja_get(&self, kana: &str) -> Option<&str> {
        self.ja
            .find(&self.pool, kana)
            .and_then(|s| self.first(s))
            .or_else(|| self.ask_bins(|b| b.first(layer::JA, kana)))
    }

    pub fn ja_len(&self) -> usize {
        self.ja.len() + self.bins.iter().map(|b| b.len(layer::JA)).sum::<usize>()
    }

    /// 這串按鍵的詞。
    pub fn zh_get(&self, keys: &str) -> Option<&str> {
        self.zh
            .find(&self.pool, keys)
            .and_then(|s| self.first(s))
            .or_else(|| self.ask_bins(|b| b.first(layer::ZH, keys)))
    }

    /// 有這串按鍵嗎？**不取值**——切點只問「包收了這個詞沒有」。
    pub fn zh_has(&self, keys: &str) -> bool {
        self.zh.has(&self.pool, keys) || self.bins.iter().any(|b| b.has(layer::ZH, keys))
    }

    pub fn zh_len(&self) -> usize {
        self.zh.len() + self.bins.iter().map(|b| b.len(layer::ZH)).sum::<usize>()
    }

    /// 這串按鍵的長輸出（字數跟音節數對不上的那些）。
    pub fn zh_long_get(&self, keys: &str) -> Option<&str> {
        self.zh_long
            .find(&self.pool, keys)
            .and_then(|s| self.first(s))
            .or_else(|| self.ask_bins(|b| b.first(layer::ZH_LONG, keys)))
    }

    pub fn zh_long_len(&self) -> usize {
        self.zh_long.len()
            + self
                .bins
                .iter()
                .map(|b| b.len(layer::ZH_LONG))
                .sum::<usize>()
    }

    // ── sym：一個名字一組符號 ──

    /// 這個名字叫得出哪些符號？**沒有就回空的**。
    ///
    /// **同名時接起來**，跟文字包之間的合併同一個規則（2026-09-07
    /// 裁定）：使用者的排前面，官方包的接在後面，組內去重。
    pub fn sym_get(&self, name: &str) -> Vec<String> {
        let mut out: Vec<String> = match self.sym.find(&self.pool, name) {
            Some(s) => self.vals(s).map(str::to_string).collect(),
            None => Vec::new(),
        };
        for b in &self.bins {
            for s in b.all(layer::SYM, name) {
                if !out.iter().any(|x| x == s) {
                    out.push(s.to_string());
                }
            }
        }
        out
    }

    /// 符號名字有幾個。
    ///
    /// **兩邊都有的名字會被數兩次**——這個數字是給設定頁與稽核工具
    /// 顯示規模用的，不是精確的去重計數。要精確的話得把兩邊的鍵合併
    /// 走一遍，那是 O(n) 的事，不值得為了一行顯示付。
    pub fn sym_len(&self) -> usize {
        self.sym.len() + self.bins.iter().map(|b| b.len(layer::SYM)).sum::<usize>()
    }

    /// 符號一個都沒有？
    pub fn sym_is_empty(&self) -> bool {
        self.sym.is_empty() && self.bins.iter().all(|b| b.is_empty(layer::SYM))
    }

    /// 走訪所有符號名與它們的符號。**給稽核工具用**，不是熱路徑。
    ///
    /// **兩邊各走一遍，不合併**——同名的會出現兩次。稽核工具要看的
    /// 就是「每一份表各收了什麼」，合併反而蓋掉問題。
    pub fn sym_iter(&self) -> impl Iterator<Item = (&str, Vec<&str>)> {
        let user = self
            .sym
            .rows
            .iter()
            .map(|(k, s)| (self.pool.get(*k), self.vals(*s).collect::<Vec<_>>()));
        // `&'static str` 降級成跟使用者那邊同樣的期限，`chain` 才接得起來
        let official = self
            .bins
            .iter()
            .flat_map(|b| b.iter(layer::SYM))
            .map(|(k, v)| (k as &str, v.into_iter().map(|s| s as &str).collect()));
        user.chain(official)
    }

    // ── tw：台語雙向詞表 ──

    /// 這個國字詞有幾種說法？**沒有就回 0**。
    ///
    /// 段選單靠它判斷「這是不是一個值得選的詞」（只有一種說法＝沒有
    /// 別的講法可選）。獨立一支是為了**不必為了數數就配置一個 Vec**
    /// ——斷詞會從長到短一路試，多數會落空。
    pub fn tw_len(&self, word: &str) -> usize {
        match self.tw.find(&self.pool, word) {
            Some(s) => s.len as usize,
            // **不累加**：台語是「同一個意思的所有說法」，兩份表各有
            // 一組的話語意是衝突不是互補，先找到的那組贏（跟 `tw_says`
            // 一致，不然數量跟內容會對不起來）
            None => self
                .ask_bins(|b| {
                    let n = b.count(layer::TW, word);
                    (n > 0).then_some(n)
                })
                .unwrap_or(0),
        }
    }

    /// 這個國字詞的所有說法（含它自己）。**沒有就回空的**。
    ///
    /// 第一個元素固定是華語詞，見 `build_index`。
    pub fn tw_says(&self, word: &str) -> Vec<String> {
        match self.tw.find(&self.pool, word) {
            Some(s) => self.vals(s).map(str::to_string).collect(),
            None => self
                .ask_bins(|b| {
                    let v = b.all(layer::TW, word);
                    (!v.is_empty()).then(|| v.into_iter().map(str::to_string).collect())
                })
                .unwrap_or_default(),
        }
    }

    /// 台語詞表是空的嗎？
    pub fn tw_is_empty(&self) -> bool {
        self.tw.is_empty() && self.bins.iter().all(|b| b.is_empty(layer::TW))
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
    // **一律走查詢方法，不要直接摸欄位**——`new.sym`／`new.tw` 只是
    // 使用者文字包那幾張表，官方包（`.bin`）在 `bins` 裡，直接摸欄位
    // 看不到它們。
    //
    // 這個坑實際踩過：台語包改成 `.bin` 之後 `tw_says` 查得到資料
    // （它會問 `bins`），但 `HAS_TW` 是 false，而平台層靠這個旗標決定
    // 要不要開段選單——**按 TAB 完全沒反應**。查表、分派全對，錯在
    // 狀態沒設對，讀程式碼很難看出來（log 一看就知道）。
    let has = new.has_words();
    let has_sym = !new.sym_is_empty();
    let has_tw = !new.tw_is_empty();
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
///
/// **設定頁的擴充包編輯器也叫它**（`pack_editor`）：使用者在符號那欄
/// 打了一串東西，畫面要當場說「這會拆成幾個」——拆法的規則（空白分隔
/// 還是舊格式逐字元、`+` 展開成 ZWJ）只有這裡知道，複製一份到 UI 去
/// 的話兩邊一定會漂掉。
pub fn split_symbols(o: &str) -> Vec<String> {
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
    // 六層各自收集，最後一起排序攤平。**池子是共用的**——同一個字串
    // 在不同層出現（「早安」既是 `zh` 的值也可能是 `tw` 的鍵）只存一份。
    let mut b_en = Builder::default();
    let mut b_ja = Builder::default();
    let mut b_zh = Builder::default();
    let mut b_long = Builder::default();
    let mut b_tw = Builder::default();
    let mut b_sym = Builder::default();

    for w in &packs.en {
        let k = out.pool.put(w);
        b_en.push_key(k);
    }
    for (k, v) in &packs.ja {
        let (k, v) = (out.pool.put(k), out.pool.put(v));
        b_ja.push_first(k, v);
    }
    let rev = crate::dict::reverse_keymap();
    for (symbols, word) in &packs.zh {
        let Some(keys) = crate::dict::symbols_to_keys(symbols, &rev) else {
            continue;
        };
        // **字數跟音節數對得上的走詞層，對不上的走長輸出層。**
        //
        // 詞層是**逐格填字**（`compose::apply_word_context` 一格填一個
        // 字），字數對不上填不回去。以前這種條目整條丟掉，但那讓
        // 「打兩個注音出一長串」這個明確的需求做不到——使用者的期待是
        // 「打了設定的字串，出來的就是輸入的東西」。
        //
        // 改成分流之後它們走 `compose::merge_pack_long`：**幾格併成一格**，
        // 跟 `merge_symbols` 同一個模式。台語層早就不套這條限制了
        // （「他們→𪜶」兩字換一字），理由一樣是「整個換掉、不逐格填」。
        let Some(syllables) = crate::bopomofo::split_syllables(&keys) else {
            continue;
        };
        let (k, v) = (out.pool.put(&keys), out.pool.put(word));
        if word.chars().count() == syllables.len() {
            b_zh.push_first(k, v);
        } else {
            b_long.push_first(k, v);
        }
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
    // **每個詞只進池子一次**，key 與 value 都存指標——這是台語包從
    // 30.8MB 降下來的關鍵，見 `Pool` 與 `Table`。
    for (zh_word, group) in groups {
        let ptrs: Vec<Str> = group.iter().map(|w| out.pool.put(w)).collect();
        // key 是「華語詞」與「每個台語說法」——雙向，見上面的長註解
        for key in std::iter::once(&zh_word).chain(group.iter().skip(1)) {
            let k = out.pool.put(key);
            for &p in &ptrs {
                b_tw.push(k, p);
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
        let k = out.pool.put(name);
        for s in syms {
            let v = out.pool.put(s);
            // `Builder::push` 自己去重——兩份表都收了 ☀ 的話只留第一個
            b_sym.push(k, v);
        }
    }

    // 六層一起排序攤平。**順序固定**，之後 dump 成二進位檔時照這個順序。
    //
    // `finish` 要讀池子比字面，所以池子必須先填完——上面每一層都只是
    // `put` 進池子，沒有任何排序。
    out.en = b_en.finish(&out.pool, &mut out.flat);
    out.ja = b_ja.finish(&out.pool, &mut out.flat);
    out.zh = b_zh.finish(&out.pool, &mut out.flat);
    out.zh_long = b_long.finish(&out.pool, &mut out.flat);
    out.tw = b_tw.finish(&out.pool, &mut out.flat);
    out.sym = b_sym.finish(&out.pool, &mut out.flat);
    // 去重表跟池子一樣大，建完就丟
    out.pool.shrink();
    out.flat.shrink_to_fit();
    out
}

/// 六層的 `(鍵, 值清單)`，也就是 `pack_bin::write` 收的形狀。
pub type BinLayers = [Vec<(String, Vec<String>)>; crate::pack_bin::LAYERS];

/// `pack_bin::write` 要的兩份東西：檔頭的 `(名稱, 值)` 與六層。
pub type BinInput = (Vec<(String, String)>, BinLayers);

/// 把 `Meta` 攤成 `(名稱, 值)` 的清單，名稱用 `parse_meta` 認得的小寫鍵。
///
/// **沒填的欄位不寫**——`meta()` 查不到與查到空字串是兩回事，前者才是
/// 「這個包沒填」。`readonly` 是布林，只有 `true` 時才寫一筆，讀回來
/// 對得上 `parse_meta` 的「寫了 true 才算」。
fn meta_pairs(m: &Meta) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut put = |k: &str, v: &Option<String>| {
        if let Some(v) = v {
            out.push((k.to_string(), v.clone()));
        }
    };
    put("name", &m.name);
    put("version", &m.version);
    put("author", &m.author);
    put("description", &m.description);
    put("license", &m.license);
    put("updated", &m.updated);
    put("homepage", &m.homepage);
    if m.readonly {
        out.push(("readonly".to_string(), "true".to_string()));
    }
    out
}

/// 把幾份**文字包**建成索引，再把六層倒成 `pack_bin::write` 要的形狀。
///
/// # 為什麼要開這道門
///
/// `gen_pack_bin` 要產的東西**就是 `Index` 的內容**——注音符號轉按鍵、
/// 字數與音節數的分流、台語的雙向索引、符號的同名合併，這些邏輯全在
/// `build_index` 裡。在工具端重寫一份一定會跟這裡漂掉，而漂掉的症狀是
/// 「`.bin` 跟 `.txt` 查出不一樣的東西」，不會有任何編譯或測試錯誤。
/// 所以走原路：`read_pack` ＋ `parse` ＋ `build_index`，只在最後多倒一次。
///
/// # 為什麼收的是路徑不是包名
///
/// `find_pack` 的規則是「`.bin` 優先於同名的 `.txt`」——那是查詢端要的，
/// 但產檔端拿包名去找會**找到上一次自己產的 `.bin`**，變成拿產物當原料。
/// 收路徑就沒有這個歧義。
///
/// 回傳的六層**已經依鍵的字面排序好**（`Builder::finish` 就是排序攤平），
/// 正好是 `pack_bin::write` 的前提，倒出來不必再排一次。
///
/// # 檔頭取**第一份**包的
///
/// 檔頭是授權的一部分（台語包載明 CC BY-SA 4.0 要求的姓名標示），
/// 不能不帶。多份合併時把每份的檔頭疊起來沒有意義——`name`／`license`
/// 只能有一個值，疊起來反而會讓「這份 `.bin` 是什麼授權」變得沒有答案。
/// 合併本來就是「產一份新的包」，檔頭該由發布者決定，所以取第一份的。
pub fn layers_for_bin(paths: &[PathBuf]) -> Result<BinInput, (PathBuf, PackReadError)> {
    let mut packs = Packs::default();
    let mut meta = Vec::new();
    for (i, p) in paths.iter().enumerate() {
        match read_pack(p) {
            Ok(content) => {
                if i == 0 {
                    meta = meta_pairs(&parse_meta(&content));
                }
                parse(&content, &mut packs);
            }
            Err(e) => return Err((p.clone(), e)),
        }
    }
    let idx = build_index(&packs);
    // 順序跟 `build_index` 結尾的攤平順序、跟 `layer::*` 的編號一致
    Ok((
        meta,
        [
            idx.dump(&idx.en),
            idx.dump(&idx.ja),
            idx.dump(&idx.zh),
            idx.dump(&idx.zh_long),
            idx.dump(&idx.tw),
            idx.dump(&idx.sym),
        ],
    ))
}

/// 依 `enabled` 的順序載入這些包，回傳總條數。
///
/// **可以重複呼叫**——包是獨立的一層，換掉索引就換掉了，不必重建
/// 詞庫（詞庫是 `OnceLock`，本來就重建不了）。設定改了就再叫一次。
pub fn load(custom: &str, enabled: &[String]) -> usize {
    // **內容沒變就整個跳過**。
    //
    // # 為什麼需要這道閘門
    //
    // `load` 是整批重建（讀全部檔案 → 建索引 → 換掉），台語包那種
    // 9 萬筆的要 ~90ms，而且舊索引的配置還給配置器之後**不會還給
    // OS**。實測用相同清單重載五次多吃 8.6MB（字串池做完之後仍有
    // 1.9MB），數字忽大忽小是碎片的典型特徵。
    //
    // 而使用者在設定頁**每勾一次／取消一次包就觸發一次**——勾來勾去
    // 試幾次，記憶體就一路往上，還每次卡 90ms。
    //
    // 跳過是安全的：不換表就沒有「換的瞬間別人查到半套」的問題
    // （見 `session::segmenu` 測試裡那段關於原子性的說明）。
    let fp = fingerprint(custom, enabled);
    if let Some(n) = unchanged(&fp) {
        return n;
    }

    let (packs, bins) = read(custom, enabled);
    let mut index = build_index(&packs);
    index.bins = bins;
    let n = index.en_len() + index.ja_len() + index.zh_len();
    // 索引跟著換——查詢層只看索引，不看原始清單
    set_index(index);
    remember(fp, n);
    n
}

/// 上一次載入的指紋與結果。
static LAST: std::sync::Mutex<Option<(Vec<FileStamp>, usize)>> = std::sync::Mutex::new(None);

/// 一個包檔案的身分：路徑、大小、修改時間。
///
/// **不讀內容算雜湊**——台語包 1.65MB，每次勾選都重讀重算是白費；
/// 而且真正要防的是「使用者改了包卻沒生效」，`(大小, mtime)` 對
/// 這件事已經夠靈敏：編輯器存檔一定會動到 mtime。
///
/// 極端情況（同一秒內改成同樣大小）會漏判，代價是「改了沒生效，
/// 重開設定頁才會看到」——比起每次都重建 90ms＋漏記憶體，這個
/// 取捨划算。
type FileStamp = (PathBuf, u64, Option<std::time::SystemTime>);

/// 一個包在磁碟上的位置，以及它是哪一種。
enum Found {
    /// 官方包，`.bin`，走 mmap。
    Bin(PathBuf),
    /// 文字包，`.txt`。
    Txt(PathBuf),
}

impl Found {
    fn path(&self) -> &std::path::Path {
        match self {
            Found::Bin(p) | Found::Txt(p) => p,
        }
    }
}

/// 找一個包的檔案。**這是唯一的找檔規則**，`read` 與 `fingerprint`
/// 都用它——兩邊的規則一漂掉，指紋就會對不上實際載入的檔案，症狀是
/// 「改了包沒生效」而且完全沒有跡象。
///
/// 兩條規則疊在一起：
///
/// - **`.bin` 優先於同名的 `.txt`**：官方包走 mmap，跨行程共用
/// - **目錄的順序決定誰贏**（`dirs` 的第一個是使用者的）：同名時
///   使用者的那份蓋過預載的
///
/// 順序是「先在同一個目錄裡挑 `.bin` 還是 `.txt`，再換下一個目錄」
/// ——不是「先掃完所有目錄的 `.bin`」。這樣使用者放一份 `.txt` 就能
/// 蓋掉預載的 `.bin`，符合「使用者的優先」。
fn find_pack(dirs: &[PathBuf], name: &str) -> Option<Found> {
    // **擋掉路徑穿越**：包名來自設定檔，不該能指到別的資料夾
    if name.contains(['/', '\\', ':']) || name.contains("..") {
        return None;
    }
    for d in dirs {
        let bin = d.join(format!("{name}.bin"));
        if bin.is_file() {
            return Some(Found::Bin(bin));
        }
        let txt = d.join(format!("{name}.txt"));
        if txt.is_file() {
            return Some(Found::Txt(txt));
        }
    }
    None
}

/// 算出這次要載的東西的指紋。
///
/// **包含沒找到的檔案**（以 0 大小記）——不然使用者把包放進資料夾時
/// 會被誤判成「沒變」。
fn fingerprint(custom: &str, enabled: &[String]) -> Vec<FileStamp> {
    let ds = dirs(custom);
    let mut out = Vec::with_capacity(enabled.len());
    for name in enabled {
        let found = find_pack(&ds, name).and_then(|f| {
            let p = f.path().to_path_buf();
            std::fs::metadata(&p)
                .ok()
                .map(|m| (p, m.len(), m.modified().ok()))
        });
        out.push(found.unwrap_or_else(|| (PathBuf::from(name), 0, None)));
    }
    out
}

/// 跟上次一樣嗎？一樣就回上次的結果。
fn unchanged(fp: &[FileStamp]) -> Option<usize> {
    let g = LAST.lock().ok()?;
    let (last, n) = g.as_ref()?;
    (last.as_slice() == fp).then_some(*n)
}

/// 記下這次的指紋。
fn remember(fp: Vec<FileStamp>, n: usize) {
    if let Ok(mut g) = LAST.lock() {
        *g = Some((fp, n));
    }
}

/// 讀出這些包的原始內容（不建索引）。
///
/// 回傳兩份：文字包解析出來的原始清單，與**官方包映射進來的位元組**。
/// 後者不解析、不複製——那正是二進位化的意義（§2.83）。
fn read(custom: &str, enabled: &[String]) -> (Packs, Vec<&'static crate::pack_bin::PackBin>) {
    let mut out = Packs::default();
    let mut bins = Vec::new();
    let ds = dirs(custom);
    for name in enabled {
        match find_pack(&ds, name) {
            Some(Found::Bin(p)) => {
                if let Some(b) = map_pack_bin(&p) {
                    bins.push(b);
                }
            }
            Some(Found::Txt(p)) => {
                if let Ok(content) = read_pack(&p) {
                    parse(&content, &mut out);
                }
            }
            None => {}
        }
    }
    (out, bins)
}

/// 映射一份官方包。**認不得就當成沒有這個包**。
///
/// # 為什麼 leak
///
/// 映射要活著，切出去的 `&str` 才有效。這裡跟詞庫同一個道理——
/// 但**包是可以被換掉的**（使用者在設定頁取消勾選），所以 leak 的
/// 量不是固定的：每載入一個沒載過的官方包就多一份映射。
///
/// 實務上這不成問題：官方包的數量是個位數，而且映射的頁是唯讀的
/// 檔案頁，記憶體壓力下可以直接丟棄——留著的只是位址空間，不是
/// 實體記憶體。**反過來如果不 leak，就得在換表時追蹤誰還在用那些
/// 位元組，那是引用計數的工作，而換表的整個設計就是為了避開它。**
fn map_pack_bin(path: &std::path::Path) -> Option<&'static crate::pack_bin::PackBin> {
    let bytes = crate::dict::map_file_pub(path)?;
    let p = crate::pack_bin::PackBin::new(bytes)?;
    Some(Box::leak(Box::new(p)))
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
    /// `.bin` 認不得：版面版本不合，或檔案損毀（下載中斷、磁碟滿）。
    ///
    /// **獨立一種**，理由跟 `NotUtf8` 一樣——講真正的原因。使用者看到
    /// 「沒有詞」會去檢查內容，但官方包的內容他根本改不了，也看不懂。
    BadBin,
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
            // 跟 `read` 同一條規則（`.bin` 優先），找到第一個就是那一份
            let f = find_pack(&ds, n)?;
            std::fs::metadata(f.path()).ok()?.modified().ok()
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
    /// **這個包不給編**（設定頁的編輯器整包唯讀）。
    ///
    /// 檔頭寫 `# readonly: true`。內建包與官方語言包（台語那類）走
    /// 這條——它們是隨程式一起發布的，使用者改了下次更新就被覆蓋，
    /// 而批量刪除更是一按就整包沒了（實測回報）。
    ///
    /// # 為什麼放在檔案裡而不是靠檔名或位置判斷
    ///
    /// 靠位置（「在 bundled 資料夾就唯讀」）的話，使用者把它複製到
    /// 自己的資料夾就繞過去了，而**那正是他該做的事**（想改就另存
    /// 一份）。靠檔名清單則要在程式裡維護一份名單，多一個官方包就
    /// 要改程式。**寫在檔案裡的話，包自己說了算**——第三方要做唯讀
    /// 的包也用同一條路。
    pub readonly: bool,
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
        // 唯讀旗標：**只要寫了 `true` 就是唯讀**，其餘值當成沒寫。
        // 它不是字串欄位，先攔下來
        if k.trim().eq_ignore_ascii_case("readonly") {
            if v.eq_ignore_ascii_case("true") || v == "1" {
                m.readonly = true;
            }
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
    let ds = dirs(custom);
    // **兩個目錄都要找**——`dirs()` 排好優先序（使用者的贏），只看
    // `dir()` 的話預載包（內建符號／emoji）永遠讀不到檔頭。
    // 找法跟 `read` 共用 `find_pack`：`.bin` 優先，兩邊一漂掉，設定頁
    // 顯示的就不是實際載入的那一份
    match find_pack(&ds, file) {
        // 官方包：檔頭與條數都從二進位檔問，不必解析文字
        Some(Found::Bin(p)) => match map_pack_bin(&p) {
            Some(b) => Info {
                file: file.to_string(),
                meta: meta_from_bin(b),
                en: b.len(layer::EN),
                ja: b.len(layer::JA),
                // 長輸出也算進 zh——設定頁顯示的是「中文有幾條」，
                // 使用者不必知道內部分兩層
                zh: b.len(layer::ZH) + b.len(layer::ZH_LONG),
                sym: b.len(layer::SYM),
                tw: b.len(layer::TW),
                error: None,
            },
            // **認不得的 `.bin`**：版本不合或檔案損毀。跟 Big5 那個洞
            // 同一條原則——講真正的原因，不要跟「沒有詞」共用一句話
            None => Info {
                file: file.to_string(),
                meta: Meta::default(),
                en: 0,
                ja: 0,
                zh: 0,
                sym: 0,
                tw: 0,
                error: Some(PackReadError::BadBin),
            },
        },
        Some(Found::Txt(p)) => {
            let mut packs = Packs::default();
            let mut error = None;
            let meta = match read_pack(&p) {
                Ok(content) => {
                    parse(&content, &mut packs);
                    parse_meta(&content)
                }
                Err(e) => {
                    // **讀不起來的原因要留著**，不能跟「沒有詞」共用
                    // 一句話（§2.49.3）
                    error = Some(e);
                    Meta::default()
                }
            };
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
        None => Info {
            file: file.to_string(),
            meta: Meta::default(),
            en: 0,
            ja: 0,
            zh: 0,
            sym: 0,
            tw: 0,
            error: None,
        },
    }
}

/// 把二進位檔的檔頭轉成 `Meta`。
///
/// 欄位名跟文字檔頭的 `# name:` 那些**完全一致**——`gen_pack_bin`
/// 是從同一份 `parse_meta` 的結果寫出去的。
fn meta_from_bin(b: &'static crate::pack_bin::PackBin) -> Meta {
    Meta {
        name: b.meta("name").map(str::to_string),
        version: b.meta("version").map(str::to_string),
        author: b.meta("author").map(str::to_string),
        description: b.meta("description").map(str::to_string),
        license: b.meta("license").map(str::to_string),
        updated: b.meta("updated").map(str::to_string),
        homepage: b.meta("homepage").map(str::to_string),
        // **官方包一律唯讀**——編輯器改不了二進位檔，而且它是隨版本
        // 發布的，改了下次更新就被蓋掉。不看檔頭裡寫什麼
        readonly: true,
    }
}

// ─────────────────────────────────────────────────────────────
// 設定頁的編輯器要用的那幾支。見開發文件 §2.75。
//
// **檔案格式完全不動**——這裡只是「把同一份 .txt 讀成看得懂的形狀，
// 改完再照原樣寫回去」。`parse` 一行都沒改，既有的包不必轉檔。
// ─────────────────────────────────────────────────────────────

/// 一條詞在編輯器裡長什麼樣。
///
/// 跟 `Packs` 的差別：`Packs` 是**合併好的查詢用資料**（依語言分成
/// 幾個清單），這裡是**一個檔案的逐行內容**，順序跟檔案一致，
/// 而且原樣保留語言代號——編輯器不認得的種類（`sym`／`tw`）也照樣
/// 讀出來，存回去時原封不動送回去。
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// 第一欄的語言代號：`en`／`ja`／`zh`／`tw`／`sym`
    pub lang: String,
    /// 第二欄：按鍵、注音符號、假名讀音，或符號的名字
    pub input: String,
    /// 第三欄。`en` 可以省略（省略就等於輸入本身）
    pub output: String,
}

/// 一個包讀出來的完整內容：檔頭 ＋ 逐條的詞。
#[derive(Debug, Clone, Default)]
pub struct Editable {
    pub meta: Meta,
    pub entries: Vec<Entry>,
}

/// 把一個包讀成可編輯的形式。
///
/// **兩個目錄都找**（使用者的優先），跟 `info` 同一條規則。
pub fn read_editable(custom: &str, file: &str) -> Result<Editable, PackReadError> {
    let mut last = PackReadError::Missing;
    for d in dirs(custom) {
        let p = d.join(format!("{file}.txt"));
        if !p.exists() {
            continue;
        }
        match read_pack(&p) {
            Ok(content) => {
                return Ok(Editable {
                    meta: parse_meta(&content),
                    entries: parse_entries(&content),
                })
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// 逐行拆成 `Entry`。**跟 `parse` 同一套規則**，但不分流到語言清單，
/// 也不做安全性過濾——那是載入時的事，編輯器要看到檔案裡真正有什麼。
fn parse_entries(content: &str) -> Vec<Entry> {
    let mut out = Vec::new();
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
        out.push(Entry {
            lang: lang.trim().to_string(),
            input: input.to_string(),
            output: f.next().map(str::trim).unwrap_or("").to_string(),
        });
    }
    out
}

/// 把編輯好的內容寫回 `.txt`。
///
/// # 寫到哪
///
/// **一律寫使用者自己的資料夾**（`resolved_dir`），不寫隨程式一起裝
/// 的那份——後者更新程式時會被覆蓋，使用者的修改就默默消失了。
/// 所以改一個內建包等於「在自己的資料夾裡放一份同名的」，而
/// `dirs()` 的優先序讓那一份贏。
///
/// # 覆寫前先備份
///
/// 舊的存成 `.txt.bak`。程式寫壞的話至少救得回來——這是**覆寫既有
/// 檔案**，不是新增。
pub fn write_editable(custom: &str, file: &str, data: &Editable) -> std::io::Result<PathBuf> {
    let dir = resolved_dir(custom).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "找不到可以寫入的資料夾")
    })?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{file}.txt"));

    // 覆寫前先備份
    if path.exists() {
        let bak = dir.join(format!("{file}.txt.bak"));
        let _ = std::fs::copy(&path, &bak);
    }

    std::fs::write(&path, render(data))?;
    Ok(path)
}

/// 組出檔案的內容。**格式跟手寫的完全一樣**，別人拿記事本開也看得懂。
fn render(data: &Editable) -> String {
    let mut s = String::new();
    for (key, value) in [
        ("name", &data.meta.name),
        ("version", &data.meta.version),
        ("author", &data.meta.author),
        ("description", &data.meta.description),
        ("license", &data.meta.license),
        ("updated", &data.meta.updated),
        ("homepage", &data.meta.homepage),
    ] {
        if let Some(v) = value {
            let v = v.trim();
            if !v.is_empty() {
                s.push_str(&format!("# {key}: {v}\n"));
            }
        }
    }
    // **唯讀旗標要寫回去**，不然「另存一份」之後那份就不唯讀了
    if data.meta.readonly {
        s.push_str("# readonly: true\n");
    }
    if !s.is_empty() {
        s.push('\n');
    }
    for e in &data.entries {
        // **只有英文省得掉第三欄**：英文的輸出等於輸入時第三欄沒有意義，
        // 寫出來只是雜訊（手寫的包也不會寫）。
        //
        // **其餘語言一律寫三欄**，就算輸出跟讀音長得一樣也要寫。`parse`
        // 對 `ja`／`zh`／`tw`／`sym` 都要求第三欄，少一欄那條**靜靜消失**
        // ——純假名的日文詞（`すごい` 的讀音與表記相同）正是這樣掉的，
        // 設定頁顯示得好好的、打字時卻沒作用（§2.75.13 第二件）。
        let 省得掉 = e.output.is_empty() || (e.lang == "en" && e.output == e.input);
        if 省得掉 {
            s.push_str(&format!("{}\t{}\n", e.lang, e.input));
        } else {
            s.push_str(&format!("{}\t{}\t{}\n", e.lang, e.input, e.output));
        }
    }
    s
}

/// 這串按鍵在擴充包裡對應的中文詞（沒有就是 `None`）。
///
/// # 為什麼選詞層需要知道這件事
///
/// 擴充包是**使用者的明確表態**——「這串按鍵我就是要這個詞」。
/// 而選詞的詞層會拿語言模型（bigram）在同讀音的候選之間挑，生造的
/// 專有名詞在 bigram 眼裡分數極低，於是**包裡的詞被統計推翻**：
/// 包裡寫 `ㄊㄧㄢㄑㄧˋ → 屇鼜`，打出來還是「天氣」（實測回報）。
///
/// 跟 `Slot::picked`（手動選過的字不可覆蓋）是同一條原則，
/// 只是表態的方式不同：一個是當場選、一個是事先寫進包裡。
pub fn zh_word(keys: &str) -> Option<String> {
    if !any() {
        return None;
    }
    index().zh_get(keys).map(str::to_string)
}

/// 包裡的**長輸出**（字數跟音節數對不上的那些）。
///
/// 給 `compose::merge_pack_long` 用——它把那幾格併成一格。查詞層的
/// `zh_word` 不會回傳這些，兩層是分開的，理由見 `Index::zh_long`。
pub fn zh_long(keys: &str) -> Option<String> {
    if !any() {
        return None;
    }
    index().zh_long_get(keys).map(str::to_string)
}

/// 有沒有任何長輸出條目？**熱路徑靠這個短路**——沒設定長輸出的人
/// 不必在每次組字時多掃一遍組字區。
pub fn any_zh_long() -> bool {
    any() && index().zh_long_len() > 0
}

/// 注音符號 → 按鍵，**一聲自動補空白**。給設定頁的編輯器用。
///
/// 檔案裡的注音是連寫的、而且**一聲不標符號**（「今天」寫成
/// `ㄐㄧㄣㄊㄧㄢ`），逐字元轉的話每個一聲的空白都會掉，引擎就切不
/// 出正確的語言（實測回報：`rup wu0` 少了結尾的空白，後半被當成
/// 英文）。音節邊界的判斷交給 `dict::symbols_to_keys`。
pub fn bopomofo_to_keys(symbols: &str) -> Option<String> {
    crate::dict::symbols_to_keys(symbols, &crate::dict::reverse_keymap())
}

/// 按鍵 → 注音符號。**一聲的空白不寫出來**，跟檔案的慣例一致。
pub fn keys_to_bopomofo(keys: &str) -> String {
    keys.chars()
        .filter(|c| *c != ' ')
        .map(|c| {
            crate::bopomofo::keymap::symbol_of(c)
                .map(|s| s.to_string())
                .unwrap_or_else(|| c.to_string())
        })
        .collect()
}

/// 這個包是不是「隨程式一起裝的」？
///
/// 內建包**不能就地改**——更新程式時會被覆蓋。編輯器要據此提醒
/// 使用者「存下去等於在你自己的資料夾另存一份」。
pub fn is_bundled_only(custom: &str, file: &str) -> bool {
    let name = format!("{file}.txt");
    let in_user = resolved_dir(custom).is_some_and(|d| d.join(&name).exists());
    let in_bundled = bundled_dir().is_some_and(|d| d.join(&name).exists());
    in_bundled && !in_user
}

#[cfg(test)]
mod 編輯 {
    use super::*;

    fn sample() -> &'static str {
        "# name: 測試包\n# version: 2\n\nen\thololive\nzh\tㄏㄨˊㄊㄠˊ\t胡桃\nsym\t星\t★ ☆\n"
    }

    #[test]
    fn 讀得出檔頭與每一條() {
        let e = Editable {
            meta: parse_meta(sample()),
            entries: parse_entries(sample()),
        };
        assert_eq!(e.meta.name.as_deref(), Some("測試包"));
        assert_eq!(e.meta.version.as_deref(), Some("2"));
        assert_eq!(e.entries.len(), 3);
        assert_eq!(e.entries[1].lang, "zh");
        assert_eq!(e.entries[1].input, "ㄏㄨˊㄊㄠˊ");
        assert_eq!(e.entries[1].output, "胡桃");
    }

    #[test]
    fn 讀出來再寫回去內容不變() {
        // **這是編輯器最重要的保證**：使用者只改一個詞，其餘的
        // 每一條都要原封不動——包括編輯器還不認得的 `sym`
        let before = Editable {
            meta: parse_meta(sample()),
            entries: parse_entries(sample()),
        };
        let text = render(&before);
        let after = Editable {
            meta: parse_meta(&text),
            entries: parse_entries(&text),
        };
        assert_eq!(before.entries, after.entries);
        assert_eq!(before.meta.name, after.meta.name);
        assert_eq!(before.meta.version, after.meta.version);
    }

    #[test]
    fn 寫出來的東西載入層讀得懂() {
        // 寫回去的檔案要能被 `parse`（產品碼那條路）讀出同樣的東西
        let e = Editable {
            meta: parse_meta(sample()),
            entries: parse_entries(sample()),
        };
        let mut packs = Packs::default();
        parse(&render(&e), &mut packs);
        assert_eq!(packs.en, vec!["hololive"]);
        assert_eq!(packs.zh.len(), 1);
        assert_eq!(packs.zh[0].1, "胡桃");
        assert_eq!(packs.sym.len(), 1);
    }

    #[test]
    fn 英文省略第三欄() {
        let e = Editable {
            meta: Meta::default(),
            entries: vec![Entry {
                lang: "en".into(),
                input: "hololive".into(),
                output: "hololive".into(),
            }],
        };
        assert_eq!(render(&e), "en\thololive\n");
    }

    #[test]
    fn 沒有檔頭就不寫空行() {
        let e = Editable {
            meta: Meta::default(),
            entries: vec![Entry {
                lang: "en".into(),
                input: "a".into(),
                output: String::new(),
            }],
        };
        assert!(!render(&e).starts_with('\n'));
    }

    #[test]
    fn 注音轉按鍵一聲要補空白() {
        // **檔案裡的一聲不標符號**，但打字要按空白。逐字元轉的話
        // 每個一聲的空白都會掉，引擎就切不出正確的語言
        assert_eq!(
            bopomofo_to_keys("ㄐㄧㄣㄊㄧㄢ").as_deref(),
            Some("rup wu0 ")
        );
        assert_eq!(bopomofo_to_keys("ㄏㄨˊㄊㄠˊ").as_deref(), Some("cj6wl6"));
        assert_eq!(bopomofo_to_keys("ㄉㄨㄛㄕㄠˇ").as_deref(), Some("2ji gl3"));
    }

    #[test]
    fn 按鍵轉注音不寫一聲() {
        assert_eq!(keys_to_bopomofo("rup wu0 "), "ㄐㄧㄣㄊㄧㄢ");
        assert_eq!(keys_to_bopomofo("cj6wl6"), "ㄏㄨˊㄊㄠˊ");
    }

    #[test]
    fn 注音與按鍵轉一圈回得來() {
        for bopo in ["ㄐㄧㄣㄊㄧㄢ", "ㄏㄨˊㄊㄠˊ", "ㄉㄨㄛㄕㄠˇ", "ㄋㄧˇㄏㄠˇ"]
        {
            let keys = bopomofo_to_keys(bopo).expect("轉不出按鍵");
            assert_eq!(keys_to_bopomofo(&keys), bopo, "{bopo} 轉一圈回不來");
        }
    }

    /// **日文的鍵是假名，不是羅馬字**。
    ///
    /// `build_index` 拿第二欄當鍵去對 `to_kana` 的結果——存成羅馬字的話
    /// 整條查不到（實測回報：包裡寫 `ja asee game`，打 `asee` 出不來）。
    /// 設定頁的編輯器存檔時要轉，這個測試釘住那個假設。
    #[test]
    fn 日文的鍵是假名() {
        let text = "ja\tあせえ\tgame\n";
        let mut packs = Packs::default();
        parse(text, &mut packs);
        assert_eq!(packs.ja, vec![("あせえ".to_string(), "game".to_string())]);

        // 建索引之後鍵仍然是假名——查詢那一端就是拿假名去對
        let idx = build_index(&packs);
        assert_eq!(idx.ja_get("あせえ"), Some("game"));
        assert_eq!(idx.ja_get("asee"), None, "羅馬字不該是鍵");
    }

    /// **日文表記跟讀音一樣時，第三欄不能省**。
    ///
    /// `render` 原本一律「輸出等於輸入就寫兩欄」，那條只對英文成立
    /// ——`ja` 少了第三欄，`parse` 直接丟掉整條。純假名的詞
    /// （`すごい`、`ありがとう`）就是這樣存進去卻沒作用的
    /// （§2.75.13 第二件）。
    #[test]
    fn 日文表記跟讀音一樣時第三欄不能省() {
        let e = Editable {
            meta: Meta::default(),
            entries: vec![Entry {
                lang: "ja".into(),
                input: "すごい".into(),
                output: "すごい".into(),
            }],
        };
        let text = render(&e);
        assert_eq!(text, "ja\tすごい\tすごい\n");

        // 真正要守的是這一句：寫出來的檔案，載入層讀得到這一條
        let mut packs = Packs::default();
        parse(&text, &mut packs);
        assert_eq!(
            packs.ja,
            vec![("すごい".to_string(), "すごい".to_string())],
            "第三欄省掉的話這一條會在載入層靜靜消失"
        );
    }

    #[test]
    fn 唯讀旗標讀得出來也寫得回去() {
        let text = "# name: 內建符號\n# readonly: true\n\nsym\t星\t★\n";
        let m = parse_meta(text);
        assert!(m.readonly, "檔頭寫了 readonly 就該是唯讀");

        // **寫回去不能掉**——不然「另存一份」之後那份就不唯讀了
        let e = Editable {
            meta: m,
            entries: parse_entries(text),
        };
        assert!(render(&e).contains("# readonly: true"));
        assert!(parse_meta(&render(&e)).readonly);
    }

    #[test]
    fn 沒寫唯讀就不是唯讀() {
        assert!(!parse_meta("# name: 我的包\n\nen\ta\n").readonly);
        // 認不得的值當成沒寫——**只有明確寫 true 才唯讀**
        assert!(!parse_meta("# readonly: false\n").readonly);
        assert!(!parse_meta("# readonly: 隨便\n").readonly);
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
            idx.sym_get("音樂"),
            ["♪", "♫", "🎵", "🎶"].map(String::from).to_vec(),
            "兩邊要接起來、先載的排前面、重複的只留一份"
        );
    }

    /// **字數對得上走詞層，對不上走長輸出層**——兩層分流，都不丟掉。
    ///
    /// 以前對不上的整條丟掉，包裡寫了也沒作用（死資料）。
    #[test]
    fn 長輸出分流到獨立的一層() {
        let mut packs = Packs::default();
        // 兩個音節兩個字：對得上，走詞層
        parse("zh\tㄐㄧㄣㄊㄧㄢ\t今天\n", &mut packs);
        // 兩個音節六個字：對不上，走長輸出層
        parse("zh\tㄨㄛˇㄑㄧㄥˇ\t我請你喝一杯\n", &mut packs);
        let idx = build_index(&packs);

        let 今天 = bopomofo_to_keys("ㄐㄧㄣㄊㄧㄢ").unwrap();
        let 長 = bopomofo_to_keys("ㄨㄛˇㄑㄧㄥˇ").unwrap();

        assert_eq!(idx.zh_get(&今天), Some("今天"));
        assert!(!idx.zh_has(&長), "長輸出不可以混進詞層——詞層是逐格填字的");
        assert_eq!(
            idx.zh_long_get(&長),
            Some("我請你喝一杯"),
            "字數對不上的要進長輸出層，不是被丟掉"
        );
        assert!(
            !idx.zh_long_get(&今天).is_some(),
            "對得上的不該同時出現在長輸出層"
        );
    }

    /// 只有長輸出條目的包**不是空包**。
    ///
    /// `has_words()` 是熱路徑的短路閘門，漏算 `zh_long` 的話整個查詢
    /// 路徑被關掉，症狀是「設定頁顯示有條目，但打字完全沒作用」。
    #[test]
    fn 只有長輸出的包也算有詞() {
        let mut packs = Packs::default();
        parse("zh\tㄨㄛˇㄑㄧㄥˇ\t我請你喝一杯\n", &mut packs);
        let idx = build_index(&packs);
        assert!(idx.has_words(), "只有長輸出也要算有詞，不然熱路徑會短路掉");
        assert!(!idx.is_empty());
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
        assert!(index().en_has("zzpacktestword"));
        assert_eq!(index().ja_get("っっぱっく"), Some("パック試験"));

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

    /// **改了包一定要生效**——這是「內容沒變就不重建」那道閘門最危險
    /// 的失敗模式：判錯的話使用者改了包卻沒反應，**不會報錯、沒有任何
    /// 跡象**，只能靠重開設定頁才發現。
    ///
    /// 這條守的是 `fingerprint` 對「同一個檔案被改掉」夠不夠靈敏。
    ///
    /// **不跟別的測試共用全域索引**：`load` 寫的是全域狀態，所以用
    /// 獨一無二的包名與資料夾，並在最後把索引還原成空的。
    #[test]
    fn 改了包要重新載入() {
        let dir = std::env::temp_dir().join("tsunagi-pack-fingerprint-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("zz指紋測試.txt");
        let d = dir.to_str().unwrap_or("");
        let names = ["zz指紋測試".to_string()];

        std::fs::write(&path, "en\tzzfingerprintone\n").unwrap();
        load(d, &names);
        assert!(index().en_has("zzfingerprintone"), "第一次載入該讀得到");

        // **長度也要不一樣**：`fingerprint` 比的是 (大小, mtime)，
        // 而檔案系統的 mtime 解析度在某些平台只到秒——同一秒內改成
        // 相同長度是已知會漏判的極端情況（見 `FileStamp` 的說明），
        // 這條測的是一般情況
        std::fs::write(&path, "en\tzzfingerprinttwo_longer\n").unwrap();
        load(d, &names);
        assert!(
            index().en_has("zzfingerprinttwo_longer"),
            "改了內容要重新載入，不能被「沒變」擋掉"
        );
        assert!(
            !index().en_has("zzfingerprintone"),
            "舊的那條該消失——索引是整個換掉的"
        );

        let _ = std::fs::remove_file(&path);
        set_index(Index::default());
    }

    /// **`.bin` 走完整條路要跟 `.txt` 查出一樣的東西**。
    ///
    /// 這是官方包二進位化（§2.83 第三步）的端對端測試：產檔 → 找檔 →
    /// mmap → 查詢，六層都要對得上。
    ///
    /// # 為什麼一定要有這條
    ///
    /// 產檔端（`layers_for_bin`）與查詢端（`Index::ask_bins`）**是兩份
    /// 各自維護的程式碼**，中間隔著一個二進位版面。任何一邊算錯位移都
    /// 不會有編譯錯誤——症狀是「查出別的詞」或「查不到」，而使用者只會
    /// 覺得「這個包壞了」。
    ///
    /// **不跟別的測試共用全域索引**：用獨一無二的包名與資料夾，最後還原。
    #[test]
    fn 二進位包跟文字包查出一樣的東西() {
        let dir = std::env::temp_dir().join("tsunagi-packbin-e2e-test");
        let _ = std::fs::create_dir_all(&dir);
        let d = dir.to_str().unwrap_or("");
        let txt = dir.join("zz二進位測試.txt");
        let bin = dir.join("zz二進位測試.bin");
        let names = ["zz二進位測試".to_string()];

        // 六層都放東西——只測台語那層的話，別層的位移算錯看不出來
        std::fs::write(
            &txt,
            "# name: 二進位端對端\n\
             # license: CC0\n\
             en\tzzbinword\n\
             ja\tっっびん\tビン試験\n\
             zh\tㄗˋㄗˋㄅㄧㄣ\t資自賓\n\
             zh\tㄗˋㄅㄧㄣ\t這是一串很長的輸出\n\
             tw\t沙發\t膨椅\n\
             sym\tっっびんほし\t★ ☆\n",
        )
        .unwrap();

        // ① 先走文字檔，記下答案
        load(d, &names);
        let want_en = index().en_has("zzbinword");
        let want_ja = index().ja_get("っっびん").map(str::to_string);
        let want_zh = index().zh_get("j4j4e2u6").map(str::to_string);
        let want_tw = index().tw_says("膨椅");
        let want_sym = index().sym_get("っっびんほし");
        assert!(want_en, "文字檔這一關就該過");
        assert!(!want_tw.is_empty(), "台語雙向查得到");

        // ② 產 `.bin`
        let (meta, layers) = layers_for_bin(std::slice::from_ref(&txt)).expect("產得出來");
        let bytes = crate::pack_bin::write(&meta, &layers).expect("編得出版面");
        crate::dict::write_data_file(&bin, &bytes).expect("寫得出去");

        // ③ 再載一次——`find_pack` 該挑 `.bin`（同名時它優先）
        //
        // **指紋會發現檔案換了**：`.bin` 跟 `.txt` 的路徑與大小都不同
        load(d, &names);
        assert!(index().en_has("zzbinword"), "en 走 .bin 也要認得");
        assert_eq!(index().ja_get("っっびん").map(str::to_string), want_ja);
        assert_eq!(index().zh_get("j4j4e2u6").map(str::to_string), want_zh);
        assert_eq!(index().tw_says("膨椅"), want_tw, "台語雙向要一樣");
        assert_eq!(index().sym_get("っっびんほし"), want_sym);

        // ④ 檔頭也要跟著進來——授權是散布的必要條件
        let got = info(d, "zz二進位測試");
        assert_eq!(got.meta.name.as_deref(), Some("二進位端對端"));
        assert_eq!(got.meta.license.as_deref(), Some("CC0"));
        assert!(got.meta.readonly, "官方包一律唯讀");
        assert!(got.total() > 0, "條數要數得出來，不然設定頁不給勾");

        // ⑤ 設定頁列得出來——只認 `.txt` 的話官方包會整個看不見
        assert!(
            available(d).contains(&"zz二進位測試".to_string()),
            "`.bin` 也要出現在可啟用的清單裡"
        );

        // ⑥ **三個熱路徑旗標要跟著亮**。
        //
        // 這條守的是實際踩過的一個 bug：`set_index` 原本用
        // `!new.tw.is_empty()` 判斷，那只看使用者文字包那張表，官方包
        // 在 `bins` 裡它看不到。結果是 `tw_says` 查得到資料、但旗標是
        // false，而平台層靠旗標決定要不要開段選單——**按 TAB 完全沒
        // 反應**，而且查表分派全對，讀程式碼很難看出來。
        assert!(any(), "有詞就要亮 any()");
        assert!(any_tw(), "有台語就要亮 any_tw()——段選單靠它");
        assert!(any_sym(), "有符號就要亮 any_sym()");

        let _ = std::fs::remove_file(&txt);
        let _ = std::fs::remove_file(&bin);
        set_index(Index::default());
    }
}
