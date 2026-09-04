//! 使用者設定：行為與外觀。
//!
//! 規格見[開發文件 §4](../../開發文件.md)（外觀主題）與 §1.6（行為項目）。
//!
//! # 為什麼在 core 而不是 platform
//!
//! 設定的**內容**跟平台無關——「選字按 Enter 要做什麼」在 macOS 上
//! 也是同一個問題。只有「怎麼把顏色交給繪圖 API」才是平台的事，
//! 那留在 `platform/windows/src/theme.rs`。
//!
//! # 缺欄位補預設
//!
//! 每個欄位都是選填（`#[serde(default)]`）。使用者只想改一個顏色時，
//! 設定檔就只要寫那一行。整份解析失敗就退回全預設——
//! **輸入法不能因為配色檔有錯就打不了字**。
//!
//! # 讀取順序
//!
//! ```text
//! 1. %APPDATA%\tsunagi-ime\config.toml   ← 找到就用
//! 2. <專案>\data\config.toml                ← 退而求其次
//! 3. 程式內建的預設值                        ← 最後防線
//! ```

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 選字時按 Enter 要做什麼。
///
/// 新注音是「選下一個字」，微軟注音是「退出」。兩派都有人習慣，
/// 所以做成設定而不是替使用者決定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EnterInSelect {
    /// 選中反白的字，然後移到下一格（新注音式）
    #[default]
    Next,
    /// 選中反白的字，然後離開選字模式
    Exit,
}

/// 行為設定。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Behavior {
    /// 選字時按 Enter 的行為
    pub enter_in_select: EnterInSelect,
    /// 最後一個字選完要不要直接送出（退出組字）。
    ///
    /// `false` 只離開選字狀態，字還在組字區，要再按一次 Enter 才送出。
    pub commit_on_last: bool,
    /// 標點的全半形。Shift+空白可以隨時切，這裡是**開機預設**。
    pub width: crate::width::Width,
    /// 啟用哪些語言引擎。
    ///
    /// # 關掉之後會怎樣
    ///
    /// **連自動辨識都跳過**——不只是鎖定輪替時略過那個模式，
    /// 瀑布式判斷也不會再問那個引擎。不打日文的人關掉之後，
    /// `sushi` 就穩定判成英文，不會忽然變成「すし」。
    ///
    /// 英文**不能關**：它是瀑布的最後一站（passthrough），
    /// 關掉的話有些按鍵組合會沒有任何語言接得住。
    pub engines: Engines,
    /// 鎖定語言時，倒退鍵要不要把反白那一整格刪掉？
    ///
    /// 關掉的話回到原本的行為（刪掉尾端的一個音節）。
    ///
    /// **只在鎖定模式生效**：自動模式的一格未必對應一個字——日文的
    /// 一格可能是整句（`sushiwotabemasu` 就只有一格），一下刪光太兇；
    /// 而且刪完會重新斷句，剩下的字可能被切成完全不同的樣子。
    #[serde(default = "default_true")]
    pub backspace_whole_cell: bool,
    /// 啟用哪些領域包（詞表），依檔名（不含 `.txt`）。
    ///
    /// **清單順序就是優先序**——同一個讀音在兩個包裡都有時，前面的贏。
    ///
    /// 包放在 `%APPDATA%\tsunagi-ime\packs\`，見 `crate::pack`。
    #[serde(default)]
    pub packs: Vec<String>,
    /// 包放在哪個資料夾。**空字串代表用預設位置**
    /// （`%APPDATA%\tsunagi-ime\packs\`）。
    ///
    /// 開放自訂是因為有人會想把包放在同步資料夾裡（多台電腦共用），
    /// 或是跟其他設定檔放在一起。見 `crate::pack::resolved_dir`。
    #[serde(default)]
    pub packs_dir: String,
    /// 鎖定注音時，`,` `.` `;` `/` `-` 這五個鍵怎麼處理。
    #[serde(default)]
    pub lock_punct: LockPunct,
    /// 鎖定注音時，`Ctrl+標點鍵` 要不要被輸入法接走？
    ///
    /// 接走的話可以明講「我要標點」，代價是那個組合在**鎖定注音時**
    /// 到不了宿主——`Ctrl+-`（瀏覽器縮小）與 `Ctrl+/`（編輯器註解）
    /// 會失效。會用到那兩個的人可以關掉。
    #[serde(default = "default_true")]
    pub ctrl_punct: bool,
}

/// 鎖定注音時，一鍵兩用的那五個鍵怎麼處理。
///
/// 它們在大千配置上是 ㄝㄡㄤㄥㄦ。**單看那一下判斷不出意圖**——
/// 四個都能自成音節（欸、歐、昂、二），所以要多看一鍵。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LockPunct {
    /// 自動判斷：接了聲調就是注音，否則構不成字，當標點。
    #[default]
    Auto,
    /// 一律當注音符號。要打標點就切回自動模式。
    Symbol,
}

/// `serde(default)` 要的預設值——這個開關預設是開的。
fn default_true() -> bool {
    true
}

/// 啟用哪些語言引擎。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Engines {
    pub bopomofo: bool,
    pub romaji: bool,
}

impl Default for Engines {
    fn default() -> Self {
        // 預設全開——這個輸入法的賣點就是三語言自動辨識
        Self {
            bopomofo: true,
            romaji: true,
        }
    }
}

impl Engines {
    /// 開關這個語言。**英文動不了**——它是瀑布的最後一站，關掉之後
    /// 什麼都打不出來。回傳切換後的狀態。
    pub fn toggle(&mut self, lang: crate::language::Language) -> bool {
        use crate::language::Language;
        match lang {
            Language::Bopomofo => {
                self.bopomofo = !self.bopomofo;
                self.bopomofo
            }
            Language::Romaji => {
                self.romaji = !self.romaji;
                self.romaji
            }
            Language::English => true,
        }
    }

    /// 這個語言啟用了嗎？英文永遠是 `true`（瀑布的最後一站）。
    pub fn enabled(&self, lang: crate::language::Language) -> bool {
        use crate::language::Language;
        match lang {
            Language::Bopomofo => self.bopomofo,
            Language::Romaji => self.romaji,
            Language::English => true,
        }
    }
}

impl Default for Behavior {
    fn default() -> Self {
        Self {
            enter_in_select: EnterInSelect::Next,
            commit_on_last: false,
            width: crate::width::Width::Auto,
            engines: Engines::default(),
            backspace_whole_cell: true,
            packs: Vec::new(),
            packs_dir: String::new(),
            lock_punct: LockPunct::default(),
            ctrl_punct: true,
        }
    }
}

/// 顏色角色。**列的是用途，不是顏色**——深色模式就是同一組角色換一組值。
///
/// 值是 `"#RRGGBB"` 字串。平台層負責轉成自己要的格式
/// （Windows 的 `COLORREF` 是 BGR 順序，寫設定檔的人會搞錯，
/// 所以轉換不能讓使用者做）。
/// `PartialEq` 是給設定頁反查主題名用的——拿目前的配色比對已知主題，
/// 完全相同就顯示那個主題名，動過任何一格就顯示「自訂」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Colors {
    /// 整個 popup 的底（漸層的**上緣**色）
    pub window_bg: String,
    /// 視窗底的漸層**下緣**色。
    ///
    /// 空字串或跟 `window_bg` 相同就是純色——**預設是空的**，
    /// 不設定的人看不出任何差別。
    pub window_bg2: String,
    /// 候選字
    pub text: String,
    /// 候選字前的編號。**跟候選字是不同角色**——層次感就來自這個差異
    pub index: String,
    /// 反白那一列的底
    pub highlight_bg: String,
    /// 反白那一列的字**與編號**。
    ///
    /// 原本文字與編號分兩個角色，但反白狀態下兩者同色才像新注音，
    /// 分開只是多一個要調的東西。使用者決定合併。
    pub highlight_text: String,
    /// 預覽列的文字**與標記符號**（▶、【】那些）。
    ///
    /// 標記本來就是預覽列的一部分，分開沒有意義。使用者決定合併。
    pub preview_text: String,
    /// 預覽列的底（漸層的**上緣**色）
    pub preview_bg: String,
    /// 預覽列漸層的**下緣**色。空＝純色。
    pub preview_bg2: String,
    /// 預覽列與候選清單之間的線
    pub separator: String,
}

impl Default for Colors {
    /// 模仿 Windows 11 的淺色配色。
    fn default() -> Self {
        Self {
            window_bg: "#FBFBFB".into(),
            // 空＝純色。想要漸層才填第二個顏色。
            window_bg2: String::new(),
            text: "#1A1A1A".into(),
            index: "#909090".into(),
            highlight_bg: "#0078D4".into(),
            highlight_text: "#FFFFFF".into(),
            preview_text: "#0060A8".into(),
            preview_bg: "#F2F7FB".into(),
            preview_bg2: String::new(),
            separator: "#E4E4E4".into(),
        }
    }
}

/// 字型。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Font {
    /// 字族。**空字串代表跟隨系統 UI 字型**——寫死字型名在沒裝
    /// 那套字的機器上會退化成很醜的預設字型
    pub family: String,
}

/// 整體尺寸。
///
/// # 為什麼只有一個縮放而不是一堆數值
///
/// 行高、內距、圓角、字級各自能調的話有六七個控制項，但實際上
/// 使用者要的只是「整個候選視窗大一點／小一點」。分開調反而容易
/// 調出比例失衡的樣子。
///
/// 所以只留一個百分比，全部等比放大——**版面比例由設計決定，
/// 使用者決定的是大小**。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Metrics {
    /// 整體縮放百分比。50/75/100/125/150/175/200。
    pub scale_percent: i32,
    /// 反白條的樣式。
    pub highlight_style: HighlightStyle,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            scale_percent: 100,
            highlight_style: HighlightStyle::default(),
        }
    }
}

/// 反白條長什麼樣。
///
/// 三種都是實測比較過才留下的（見開發文件 §4.21）——
/// `bin/demo_glass.rs` 那個示範程式把七八種並排畫出來，
/// 動起來看過之後選了這三個。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HighlightStyle {
    /// 實心色塊——最清楚，預設
    #[default]
    Solid,
    /// 色塊 + 上緣白色高光帶。玻璃有厚度，上半部會反光
    Sheen,
    /// **只有高光與亮邊**，底色全透明。
    ///
    /// 「選中」的訊號完全靠那道光——最輕，但也最不明顯。
    /// 這個模式下反白的字**維持原色**（沒有深底，白字會消失）。
    SheenOnly,
}

impl HighlightStyle {
    /// 這個樣式下，反白那一列的字要不要換成反白色？
    ///
    /// `SheenOnly` 沒有深色底，白字會看不見，所以維持原色。
    pub fn recolors_text(self) -> bool {
        !matches!(self, HighlightStyle::SheenOnly)
    }
}

/// 允許的縮放檔位。設定頁的下拉選單也用這一份。
pub const SCALE_STEPS: [i32; 7] = [50, 75, 100, 125, 150, 175, 200];

/// 一整份設定。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// 顯示設定頁的「除錯」分頁嗎？
    ///
    /// 放在最外層而不是 `[behavior]`——它不是輸入法的行為，
    /// 是設定頁自己的事。預設關閉，平常使用者不需要看到。
    pub debug: bool,
    pub behavior: Behavior,
    pub colors: Colors,
    pub font: Font,
    pub metrics: Metrics,
    pub background: Background,
}

/// 候選視窗的背景圖。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Background {
    /// 圖檔路徑（PNG／JPG／BMP）。**空字串 = 不用圖**，回到純色背景。
    ///
    /// 相對路徑以 `data/` 為基準；絕對路徑照用。
    pub image: String,
    /// 圖片透出來多少：`0.0` = 完全看不到（純色），`1.0` = 只有圖。
    ///
    /// **不是直接畫圖就好**——圖片背景最容易毀掉的是可讀性：深色圖上的
    /// 深色字、亮處的白字都會消失。所以圖之上還會蓋一層原本的漸層底色，
    /// 這個值控制那層蓋多重（實際的遮罩不透明度是 `1.0 - strength`）。
    pub strength: f32,
    /// 文字描邊的濃度：`0.0` = 不描邊，`1.0` = 最濃。
    ///
    /// 在文字四周描一圈深色細邊，**背景再花也讀得到**。
    ///
    /// 為什麼是描邊而不是陰影：陰影只擋一個方向，字的另一側照樣會融進
    /// 背景；描邊四面都擋得住。
    ///
    /// 放在 `[background]` 底下是因為這裡最需要它（也最容易被找到），
    /// 但它**不依賴背景圖**——純色背景想要描邊也可以開。
    pub text_outline: f32,
}

impl Default for Background {
    fn default() -> Self {
        Self {
            image: String::new(),
            // 沒設圖時這個值用不到；設了圖的話從六成開始——
            // 看得出圖，但配色仍然壓得住對比
            strength: 0.6,
            // 預設不描邊：純色背景本來就夠清楚，描了反而糊
            text_outline: 0.0,
        }
    }
}

impl Background {
    /// 圖片有效嗎？路徑空的就是沒設。
    pub fn enabled(&self) -> bool {
        !self.image.trim().is_empty()
    }

    /// 蓋在圖片上的那層底色要多不透明。
    pub fn overlay_alpha(&self) -> f32 {
        (1.0 - self.strength).clamp(0.0, 1.0)
    }

    /// 要不要描邊。濃度太低就當作沒開——省下每個字多畫幾次的成本。
    pub fn outlined(&self) -> bool {
        self.text_outline > 0.01
    }

    /// 描邊的實際不透明度（夾在合法範圍內）。
    pub fn outline_alpha(&self) -> f32 {
        self.text_outline.clamp(0.0, 1.0)
    }
    /// 背景圖的**實際檔案路徑**。沒設、找不到檔案就回 `None`。
    ///
    /// # 為什麼放在 core
    ///
    /// 「設定值怎麼解讀」跟平台無關，而**輸入法與設定頁都要問這件事**
    /// ——一邊拿去畫候選視窗、一邊拿去畫預覽。兩邊各寫一份的話會走鐘，
    /// 而且已經走鐘過：一份有 `trim()` 一份沒有，設定值是空白字元時
    /// 兩邊的答案不一樣。
    ///
    /// `base` 是專案的 `data/`——相對路徑以它為準。呼叫端各自知道自己
    /// 那邊怎麼找到那個目錄，所以由外面傳進來。
    ///
    /// **會 `trim()`**，跟 `enabled()` 的規則一致（只有空白也算沒設）。
    pub fn resolve_image(&self, base: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
        resolve_image_path(&self.image, base)
    }
}

impl Config {
    /// 從 TOML 字串解析。壞掉就回預設。
    ///
    /// **不回傳錯誤是刻意的**——呼叫端唯一能做的事就是用預設值，
    /// 那不如在這裡做掉。設定頁要檢查語法的話另外呼叫 `parse`。
    pub fn from_toml_or_default(s: &str) -> Self {
        Self::parse(s).unwrap_or_default()
    }

    /// 從 TOML 字串解析，錯誤照實回報。設定頁用這個顯示錯誤訊息。
    pub fn parse(s: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(s)
    }

    /// 序列化成 TOML。設定頁存檔用。
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }

    /// 依讀取順序找設定檔並載入。找不到就回預設。
    ///
    /// `data_dir` 是專案的 `data/`，可能是 `None`（例如測試環境）。
    pub fn load(data_dir: Option<&Path>) -> Self {
        match Self::find(data_dir) {
            Some(p) => std::fs::read_to_string(p)
                .map(|s| Self::from_toml_or_default(&s))
                .unwrap_or_default(),
            None => Self::default(),
        }
    }

    /// 設定檔在哪？依讀取順序找第一個存在的。
    pub fn find(data_dir: Option<&Path>) -> Option<PathBuf> {
        let user = user_config_path();
        if user.as_ref().is_some_and(|p| p.is_file()) {
            return user;
        }
        let proj = data_dir.map(|d| d.join("config.toml"));
        proj.filter(|p| p.is_file())
    }

    /// 設定頁存檔的目標：一律寫使用者目錄。
    ///
    /// **不寫專案的 `data/`**——那裡是內建預設，會被 git 管理，
    /// 使用者改的東西不該混進去。
    pub fn save_path() -> Option<PathBuf> {
        user_config_path()
    }

    /// 存檔。會自動建立所在資料夾。
    pub fn save(&self) -> std::io::Result<PathBuf> {
        let path = Self::save_path().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "找不到使用者設定目錄")
        })?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, self.to_toml())?;
        Ok(path)
    }
}

/// 把設定裡的圖片路徑解讀成實際檔案路徑。見 `Background::resolve_image`。
pub fn resolve_image_path(
    setting: &str,
    base: Option<&std::path::Path>,
) -> Option<std::path::PathBuf> {
    let raw = setting.trim();
    if raw.is_empty() {
        return None;
    }
    if is_remote_path(raw) {
        return None;
    }
    let p = std::path::Path::new(raw);
    let full = if p.is_absolute() {
        p.to_path_buf()
    } else {
        base?.join(p)
    };
    full.is_file().then_some(full)
}

/// 這是網路路徑嗎（`\\主機\共享`）？
///
/// 設定裡的路徑會被每一個宿主行程（含 UAC 提示框那種高權限的）週期性
/// 地 stat。指到一台外面的主機，等於每個行程都去對它做 SMB 認證、而且
/// 逾時會卡在按鍵那條執行緒上。輸入法用不到網路路徑，一律不收。
pub fn is_remote_path(s: &str) -> bool {
    let s = s.trim_start();
    s.starts_with("\\\\") || s.starts_with("//")
}

/// `%APPDATA%` 底下的資料夾名。設定檔與領域包都放這裡。
pub const APP_DIR: &str = "tsunagi-ime";

/// 改名前的資料夾名（2026-09-01 從舊專案名改過來）。
///
/// 只有 `migrate_app_dir` 會用到——搬過去之後就再也碰不到它。
const OLD_APP_DIR: &str = "通用語言輸入法";

/// 使用者的資料夾：`%APPDATA%\tsunagi-ime\`
///
/// 用環境變數而不是寫死路徑——使用者名稱、磁碟機代號在別台電腦
/// 不一定相同（見 CLAUDE.md 的跨電腦開發注意事項）。
///
/// **順便處理改名的搬家**，見 `migrate_app_dir`。設定與領域包都經過
/// 這一支，所以不管誰先被呼叫，搬家都只會發生一次。
pub fn user_dir() -> Option<PathBuf> {
    let base = PathBuf::from(std::env::var_os("APPDATA")?);
    migrate_app_dir(&base);
    Some(base.join(APP_DIR))
}

/// 舊資料夾改名成新的。**只在新的還不存在時做，而且一個行程只試一次。**
///
/// # 為什麼用 rename 而不是複製
///
/// 同一個磁碟上的改名是原子的：要嘛整個成功、要嘛什麼都沒發生，
/// 不會出現「複製到一半」的半套狀態。也不必處理「複製完要不要刪舊的」。
///
/// # 失敗了不要緊
///
/// 改名失敗（權限、檔案被鎖）就當作沒有設定檔，回到預設值——那是
/// 這個函式本來就有的行為。輸入法還是能打字，使用者重存一次設定就好，
/// **絕不能因為搬不動就當掉**。
///
/// 多個宿主行程同時開的話會有好幾個一起試，但 Windows 的 rename 在
/// 目標已存在時會失敗，所以只有第一個會成功，其餘的看到新的已經在了。
fn migrate_app_dir(base: &Path) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| rename_old_dir(base));
}

/// 真正的搬家動作。
///
/// 從 `migrate_app_dir` 抽出來是**為了能測**——那一支有 `Once`，
/// 一個行程只跑得了一次，測不到第二種情況。
fn rename_old_dir(base: &Path) {
    let new = base.join(APP_DIR);
    if new.exists() {
        return;
    }
    let old = base.join(OLD_APP_DIR);
    if old.is_dir() {
        let _ = std::fs::rename(&old, &new);
    }
}

/// `%APPDATA%\tsunagi-ime\config.toml`
fn user_config_path() -> Option<PathBuf> {
    Some(user_dir()?.join("config.toml"))
}

/// 設定檔的修改時間。用來判斷「改過了要重讀」。
///
/// 讀不到（檔案不存在、沒權限）回 `None`——那代表沒有設定檔，
/// 不是錯誤。
pub fn modified_at(data_dir: Option<&Path>) -> Option<std::time::SystemTime> {
    let p = Config::find(data_dir)?;
    std::fs::metadata(p).ok()?.modified().ok()
}

#[cfg(test)]
mod tests {
    /// 資料夾改名的搬家（2026-09-01 從舊專案名改成 `tsunagi-ime`）。
    ///
    /// 直接在暫存資料夾裡造出兩種狀況來測，不碰真的 `%APPDATA%`。
    mod 搬家 {
        use super::super::{rename_old_dir, APP_DIR, OLD_APP_DIR};

        /// 造一個乾淨的暫存 base，回傳路徑。用完自己刪。
        fn 暫存區(tag: &str) -> std::path::PathBuf {
            let p = std::env::temp_dir().join(format!("tsunagi_migrate_{tag}"));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            p
        }

        #[test]
        fn 舊的在新的不在_就搬過去() {
            let base = 暫存區("move");
            let old = base.join(OLD_APP_DIR);
            std::fs::create_dir_all(&old).unwrap();
            std::fs::write(old.join("config.toml"), "debug = true").unwrap();

            rename_old_dir(&base);

            let new = base.join(APP_DIR);
            assert!(new.is_dir(), "新資料夾要出現");
            assert!(!old.exists(), "舊的要不見");
            assert_eq!(
                std::fs::read_to_string(new.join("config.toml")).unwrap(),
                "debug = true",
                "內容要跟著過去"
            );
            let _ = std::fs::remove_dir_all(&base);
        }

        /// **新的已經在就不要動**——那代表使用者已經有新設定了，
        /// 蓋掉會把他現在在用的設定弄丟。
        #[test]
        fn 新的已經在_就不動() {
            let base = 暫存區("keep");
            let old = base.join(OLD_APP_DIR);
            let new = base.join(APP_DIR);
            std::fs::create_dir_all(&old).unwrap();
            std::fs::create_dir_all(&new).unwrap();
            std::fs::write(old.join("config.toml"), "舊的").unwrap();
            std::fs::write(new.join("config.toml"), "新的").unwrap();

            rename_old_dir(&base);

            assert_eq!(
                std::fs::read_to_string(new.join("config.toml")).unwrap(),
                "新的",
                "現在在用的設定不能被舊的蓋掉"
            );
            assert!(old.is_dir(), "舊的原封不動留著");
            let _ = std::fs::remove_dir_all(&base);
        }

        /// 兩個都不在（全新安裝）——什麼都不做，也不能當掉。
        #[test]
        fn 兩個都不在_什麼都不做() {
            let base = 暫存區("fresh");
            rename_old_dir(&base);
            assert!(!base.join(APP_DIR).exists());
            let _ = std::fs::remove_dir_all(&base);
        }
    }

    /// 背景圖路徑的解讀。**兩邊共用這一份**，各寫一份會走鐘（曾經
    /// 一份有 trim 一份沒有）。
    mod 圖片路徑 {
        use super::*;
        use std::path::Path;

        #[test]
        fn 沒設就是沒有() {
            assert_eq!(resolve_image_path("", Some(Path::new("."))), None);
        }

        #[test]
        fn 只有空白也算沒設() {
            // 跟 `Background::enabled()` 同一條規則——兩邊不一致的話，
            // 會出現「設定頁說沒圖、輸入法卻去找一個叫空白的檔案」
            assert_eq!(resolve_image_path("   ", Some(Path::new("."))), None);
        }

        #[test]
        fn 找不到檔案就回無() {
            let r = resolve_image_path("這個檔案不存在.png", Some(Path::new(".")));
            assert_eq!(r, None);
        }

        #[test]
        fn 相對路徑沒有基準目錄時回無() {
            assert_eq!(resolve_image_path("bg.png", None), None);
        }

        /// 網路路徑一律不收——每個宿主行程都會去 stat 它，指到外面的
        /// 主機等於幫人做 SMB 認證外洩，而且逾時會卡住按鍵。
        #[test]
        fn 網路路徑不收() {
            assert!(is_remote_path("\\\\evil\\share\\bg.png"));
            assert!(is_remote_path("//evil/share/bg.png"));
            assert!(is_remote_path("  \\\\evil\\share"));
            assert!(!is_remote_path("C:\\Users\\me\\bg.png"));
            assert!(!is_remote_path("bg.png"));
            assert!(!is_remote_path("\\single"));
            assert_eq!(
                resolve_image_path("\\\\evil\\share\\bg.png", Some(Path::new("."))),
                None
            );
        }

        #[test]
        fn 真的存在的檔案找得到() {
            // 用專案裡一定有的檔案當標的
            let base = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
            let got = resolve_image_path("CLAUDE.md", Some(base));
            assert!(got.is_some(), "相對路徑該以基準目錄解讀");
            // 前後空白要被吃掉
            assert_eq!(got, resolve_image_path("  CLAUDE.md  ", Some(base)));
        }
    }

    /// 背景圖：濃度與遮罩是互補的，兩端要能真的到「純色」與「純圖」。
    mod 背景圖 {
        use super::super::Background;

        #[test]
        fn 沒設路徑就是沒啟用() {
            assert!(!Background::default().enabled());
            let mut b = Background {
                image: "   ".to_string(),
                ..Default::default()
            };
            assert!(!b.enabled(), "只有空白也算沒設");
            b.image = "bg.png".to_string();
            assert!(b.enabled());
        }

        #[test]
        fn 濃度與遮罩互補() {
            let mut b = Background {
                strength: 0.0,
                ..Default::default()
            };
            assert_eq!(b.overlay_alpha(), 1.0, "濃度 0 = 底色全蓋 = 看不到圖");
            b.strength = 1.0;
            assert_eq!(b.overlay_alpha(), 0.0, "濃度 1 = 不蓋 = 只有圖");
            b.strength = 0.25;
            assert!((b.overlay_alpha() - 0.75).abs() < 1e-6);
        }

        #[test]
        fn 描邊濃度太低就當作沒開() {
            let mut b = Background::default();
            assert!(!b.outlined(), "預設不描邊");
            b.text_outline = 0.005;
            assert!(!b.outlined(), "低到看不見就別浪費每個字多畫八次");
            b.text_outline = 0.5;
            assert!(b.outlined());
        }

        #[test]
        fn 描邊不依賴背景圖() {
            // 純色背景想描邊也可以——兩個設定各自獨立
            let b = Background {
                text_outline: 0.8,
                ..Default::default()
            };
            assert!(!b.enabled(), "沒有圖");
            assert!(b.outlined(), "但描邊照樣開得起來");
        }

        #[test]
        fn 設定檔被手改成離譜的值也不會算出負數() {
            let mut b = Background {
                strength: 5.0,
                ..Default::default()
            };
            assert_eq!(b.overlay_alpha(), 0.0);
            b.strength = -3.0;
            assert_eq!(b.overlay_alpha(), 1.0);
            b.text_outline = 9.0;
            assert_eq!(b.outline_alpha(), 1.0);
            b.text_outline = -1.0;
            assert_eq!(b.outline_alpha(), 0.0);
        }
    }

    use super::*;

    #[test]
    fn 預設值可以往返() {
        let c = Config::default();
        let s = c.to_toml();
        let back = Config::parse(&s).expect("自己吐的 TOML 要讀得回來");
        assert_eq!(back.colors.highlight_bg, c.colors.highlight_bg);
        assert_eq!(back.behavior.enter_in_select, c.behavior.enter_in_select);
    }

    #[test]
    fn 只寫一個欄位其餘補預設() {
        // 使用者只想改一個顏色，不該被迫複製一整份
        let c = Config::parse("[colors]\nhighlight_bg = \"#FF0000\"\n").unwrap();
        assert_eq!(c.colors.highlight_bg, "#FF0000");
        assert_eq!(
            c.colors.window_bg,
            Colors::default().window_bg,
            "沒寫的欄位要補預設"
        );
        assert_eq!(c.font.family, Font::default().family);
    }

    #[test]
    fn 壞掉的檔案退回預設() {
        // **輸入法不能因為配色檔有錯就打不了字**
        let c = Config::from_toml_or_default("這根本不是 TOML {{{");
        assert_eq!(c.colors.window_bg, Colors::default().window_bg);
    }

    #[test]
    fn 空檔案等於全預設() {
        let c = Config::parse("").unwrap();
        assert_eq!(c.metrics.scale_percent, 100);
        assert!(!c.debug, "除錯分頁預設關閉");
    }

    #[test]
    fn debug_是最外層的欄位() {
        let c = Config::parse("debug = true\n").unwrap();
        assert!(c.debug);
    }

    #[test]
    fn 縮放檔位涵蓋預設值() {
        assert!(SCALE_STEPS.contains(&Metrics::default().scale_percent));
        assert_eq!(SCALE_STEPS[0], 50);
        assert_eq!(SCALE_STEPS[SCALE_STEPS.len() - 1], 200);
    }

    #[test]
    fn 全半形是行為設定的一部分() {
        let c = Config::parse(
            "[behavior]
width = \"full\"
",
        )
        .unwrap();
        assert_eq!(c.behavior.width, crate::width::Width::Full);
        // 沒寫就是自動
        assert_eq!(
            Config::parse("").unwrap().behavior.width,
            crate::width::Width::Auto
        );
    }

    #[test]
    fn 行為設定讀得懂() {
        let c = Config::parse("[behavior]\nenter_in_select = \"exit\"\ncommit_on_last = true\n")
            .unwrap();
        assert_eq!(c.behavior.enter_in_select, EnterInSelect::Exit);
        assert!(c.behavior.commit_on_last);
    }

    #[test]
    fn 找不到檔案時回預設() {
        // 注意：`load` 會先找 %APPDATA%，那裡可能有使用者自己存的設定，
        // 所以這裡驗的是 `find` 找不到專案檔時不會硬撐，而不是 `load`
        // 的結果——後者本來就該讀到使用者的設定。
        let none = Config::find(Some(Path::new("這個路徑不存在")));
        if none.is_none() {
            assert_eq!(
                Config::load(Some(Path::new("這個路徑不存在"))).colors.text,
                Colors::default().text
            );
        }
    }
}
