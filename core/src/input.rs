//! 輸入層：**按鍵怎麼變成語言段落**。
//!
//! 這一層之下有兩套**完全不同**的輸入邏輯：
//!
//! ```text
//!   按鍵
//!    │
//!    ├─ Cascade（自動模式）── 累加式切法 → 排序 → 多種切法讓使用者選
//!    │                        不知道是哪個語言，所以每種可能都留著
//!    │
//!    └─ Bopomofo（鎖定注音）─ 四格音節緩衝 → 一種切法
//!                             已經確定是注音，改走新酷音那套互動
//!
//!   ↓ 兩者都產出 Vec<Vec<Segment>>
//!
//!   選字、詞庫修正、全半形…（`session` 的其餘部分，兩邊共用）
//! ```
//!
//! # 為什麼要分開
//!
//! 兩者的**互動模型不同**，不是同一套邏輯的兩種參數：
//!
//! | | 自動（Cascade） | 鎖定注音（Bopomofo） |
//! |---|---|---|
//! | 按鍵累積 | 字串追加 | 四格覆寫（打錯聲母直接改） |
//! | 組字區顯示 | 原始按鍵 `su3` | 注音符號 `ㄋㄧ` |
//! | 切法 | 多種，可選 | 只有一種 |
//! | 何時算完一個單位 | 不知道（使用者自己決定） | 按到聲調鍵 |
//!
//! 混在同一個函式裡用 `if` 分流的話，兩套規則會互相污染——這一層
//! 存在就是為了讓它們各走各的。
//!
//! # 什麼**不**在這裡
//!
//! 選字、詞庫修正、全半形、候選視窗那些都不在——那些對兩種模式
//! 是一樣的，留在 `session`。這一層只管「按鍵 → 段落」。

use crate::bopomofo::buffer::{KeyResult, Syllable};
use crate::cutpoint::{incremental::Incremental, normalize, punct, rank, Segment};
use crate::language::Language;

/// 一次按鍵之後，輸入層有什麼變化。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Changed {
    /// 段落重算了——選字格要跟著重建。
    Segments,
    /// 只有「正在打的東西」變了（例如注音音節還沒收尾），
    /// 段落沒動，選字格不必重建。
    PendingOnly,
    /// 什麼都沒變（例如空的時候按 Backspace）。
    Nothing,
}

/// 輸入模式。**兩種完全獨立的輸入邏輯**，見模組說明。
#[derive(Debug, Clone)]
pub enum Input {
    Cascade(Cascade),
    Bopomofo(BopomofoInput),
    Romaji(RomajiInput),
}

impl Default for Input {
    fn default() -> Self {
        Input::Cascade(Cascade::default())
    }
}

impl Input {
    /// 依鎖定的語言建一個輸入層。
    ///
    /// 只有鎖定注音走自己的路——日文與英文的鎖定沒有「覆寫」或
    /// 「音節收尾」的問題，用 `Cascade` 加上單語言切段就夠了。
    pub fn for_lock(lock: Option<Language>) -> Self {
        Self::with_engines(lock, crate::config::Engines::default())
    }

    /// 同上，但指定啟用哪些引擎。
    pub fn with_engines(lock: Option<Language>, engines: crate::config::Engines) -> Self {
        match lock {
            Some(Language::Bopomofo) => Input::Bopomofo(BopomofoInput::default()),
            Some(Language::Romaji) => Input::Romaji(RomajiInput::default()),
            // 自動與鎖定英文都走瀑布式——英文是 passthrough，
            // 打什麼顯示什麼，不需要自己的輸入邏輯
            _ => Input::Cascade(Cascade {
                engines,
                ..Default::default()
            }),
        }
    }

    /// 打一個鍵。
    pub fn push(&mut self, ch: char, lock: Option<Language>) -> Changed {
        match self {
            Input::Cascade(c) => c.push(ch, lock),
            Input::Bopomofo(b) => b.push(ch),
            Input::Romaji(r) => r.push(ch),
        }
    }

    /// 刪一個鍵。
    pub fn backspace(&mut self, lock: Option<Language>) -> Changed {
        match self {
            Input::Cascade(c) => c.backspace(lock),
            Input::Bopomofo(b) => b.backspace(),
            Input::Romaji(r) => r.backspace(),
        }
    }

    /// 目前算出來的切法（第一種是引擎最推薦的）。
    pub fn cuttings(&self) -> &[Vec<Segment>] {
        match self {
            Input::Cascade(c) => &c.cuttings,
            Input::Bopomofo(b) => std::slice::from_ref(&b.segments),
            Input::Romaji(r) => std::slice::from_ref(&r.segments),
        }
    }

    /// 已經收進段落的原始按鍵。
    pub fn keys(&self) -> &str {
        match self {
            Input::Cascade(c) => &c.keys,
            Input::Bopomofo(b) => &b.keys,
            Input::Romaji(r) => &r.keys,
        }
    }

    /// 還沒收尾的東西（鎖定注音時是正在打的音節符號）。
    pub fn pending(&self) -> String {
        match self {
            Input::Cascade(_) => String::new(),
            Input::Bopomofo(b) => b.syllable.symbols(),
            // 日文的 pending 是**還沒湊成 mora 的羅馬字**（`sush` 的 `sh`）
            Input::Romaji(r) => r.pending.clone(),
        }
    }

    /// **使用者明講這是標點**（Ctrl+鍵）。只有鎖定注音需要——其他模式
    /// 本來就打得出標點。回傳 `true` 代表接手了。
    pub fn push_punct(&mut self, ch: char) -> bool {
        match self {
            Input::Bopomofo(b) => {
                b.push_punct(ch);
                true
            }
            _ => false,
        }
    }

    /// 鎖定注音時標點鍵怎麼處理。換模式時要重設，見 `Session`。
    pub fn set_punct_mode(&mut self, mode: crate::config::LockPunct) {
        if let Input::Bopomofo(b) = self {
            b.punct_mode = mode;
        }
    }

    /// 一個字都還沒打？
    pub fn is_empty(&self) -> bool {
        self.keys().is_empty() && self.pending().is_empty()
    }

    /// 把還沒收尾的東西結算掉，回傳完整的按鍵串。
    ///
    /// 換模式時用——不然那些按鍵會卡在緩衝裡，看得到卻送不出去。
    pub fn drain_keys(&mut self) -> String {
        match self {
            Input::Cascade(c) => std::mem::take(&mut c.keys),
            Input::Romaji(r) => {
                let mut k = std::mem::take(&mut r.keys);
                k.push_str(&std::mem::take(&mut r.pending));
                k
            }
            Input::Bopomofo(b) => {
                let mut k = std::mem::take(&mut b.keys);
                k.push_str(&b.syllable.keys());
                b.syllable.clear();
                k
            }
        }
    }

    /// 從一串按鍵重建（換模式時用）。
    pub fn from_keys(keys: &str, lock: Option<Language>) -> Self {
        Self::from_keys_with(keys, lock, crate::config::Engines::default())
    }

    /// 同上，但指定啟用哪些引擎。
    pub fn from_keys_with(
        keys: &str,
        lock: Option<Language>,
        engines: crate::config::Engines,
    ) -> Self {
        let mut input = Self::with_engines(lock, engines);
        for c in keys.chars() {
            input.push(c, lock);
        }
        input
    }
}

/// **自動模式**的輸入：累加式切法 + 排序。
///
/// 不知道使用者要打哪個語言，所以每種可能的切法都留著，排序後
/// 這個字元**單獨打在開頭**時，該直接送進文件而不進組字區嗎？
///
/// # 為什麼要有這條路
///
/// `!` `@` `#` `$` 這種符號不可能是任何語言的開頭。讓它進組字區的話，
/// 使用者只是想打一個驚嘆號，卻要看著候選視窗彈出來、還得按 Enter
/// 才送得出去——多兩道手續換不到任何好處。
///
/// # 為什麼要排除注音鍵
///
/// **半形標點有一半是注音鍵**：`,` 是ㄝ、`.` 是ㄡ、`/` 是ㄥ、`;` 是ㄤ、
/// `-` 是ㄦ。一律放行的話那些音就打不出來了，所以要真的去查鍵位表，
/// 不能憑「看起來像標點」判斷。
///
/// # 為什麼只管開頭
///
/// 組字**中間**的標點是硬切點，要留在按鍵串裡——`hello,world` 的逗號
/// 參與切法判斷，抽掉的話切點引擎看到的就不是使用者打的東西了。
pub fn passthrough_alone(ch: char) -> bool {
    ch.is_ascii_punctuation() && crate::bopomofo::keymap::lookup(ch).is_none()
}

/// 讓使用者用切法選單挑。這是這個專案的核心設計，見開發文件 §2.7。
#[derive(Debug, Clone, Default)]
pub struct Cascade {
    keys: String,
    inc: Incremental,
    cuttings: Vec<Vec<Segment>>,
    /// 啟用哪些語言引擎（使用者可以在設定裡關掉不用的）。
    engines: crate::config::Engines,
}

impl Cascade {
    fn push(&mut self, ch: char, lock: Option<Language>) -> Changed {
        self.keys.push(ch);
        self.inc.push(ch);
        self.recompute(lock);
        Changed::Segments
    }

    fn backspace(&mut self, lock: Option<Language>) -> Changed {
        if self.keys.pop().is_none() {
            return Changed::Nothing;
        }
        // 累加式沒有退格——分支是一路累積的，沒有反向的走法。
        // 整串重建，成本可接受（按鍵串通常十幾個字元）。
        self.inc = Incremental::from_keys(&self.keys);
        self.recompute(lock);
        Changed::Segments
    }

    fn recompute(&mut self, lock: Option<Language>) {
        // 鎖定日文／英文時不做切點判斷——整串就是一段。
        // （鎖定注音走的是另一個型別，不會進到這裡。）
        if let Some(lang) = lock {
            self.cuttings = if self.keys.is_empty() {
                Vec::new()
            } else {
                vec![single_language_segments(&self.keys, lang)]
            };
            return;
        }
        let sorted = rank::sort(self.inc.cuttings());
        let mut seen = std::collections::HashSet::new();
        let engines = self.engines;
        self.cuttings = sorted
            .iter()
            .map(|c| normalize(c))
            // **停用的語言在出口過濾掉**。
            //
            // 不去動十幾處 `validity` 的呼叫點——那要嘛得把設定一路
            // 傳下去，要嘛得用全域狀態（測試會互相干擾）。切法的出口
            // 只有這一個地方，在這裡丟掉含停用語言的切法最單純。
            .filter(|c| c.iter().all(|s| engines.enabled(s.lang)))
            .filter(|c| {
                let key: Vec<_> = c.iter().map(|s| (s.keys.clone(), s.lang)).collect();
                seen.insert(key)
            })
            .collect();
        // **全被過濾光就退回英文**。
        //
        // 那代表這串按鍵只有停用的語言認得（例如關掉日文後打
        // `sushi`）。英文是 passthrough，永遠接得住。
        if self.cuttings.is_empty() && !self.keys.is_empty() {
            self.cuttings = vec![single_language_segments(&self.keys, Language::English)];
        }
        // **切詞學習的提名放在最後**：學到的位置是使用者在選單上看到的
        // 那一種，也就是正規化、去重之後的。見 `rank::promote_learned_cut`。
        self.cuttings = rank::promote_learned_cut(std::mem::take(&mut self.cuttings));
    }
}

/// **鎖定注音**的輸入：四格音節緩衝。
///
/// 互動照新酷音（libchewing）：同格覆寫、聲調收尾、組字區顯示注音
/// 符號。細節見 `bopomofo::buffer`。
#[derive(Debug, Clone, Default)]
pub struct BopomofoInput {
    /// 已經收尾的音節，接成一串按鍵。
    keys: String,
    /// 正在打的那個音節。
    syllable: Syllable,
    /// `keys` 切出來的段落。整串都是注音，只有標點自成一段。
    segments: Vec<Segment>,
    /// 標點鍵怎麼處理。見 `config::LockPunct`。
    punct_mode: crate::config::LockPunct,
    /// 上一個進按鍵串的標點是「使用者明講的」（Ctrl+鍵）嗎？
    ///
    /// 明講的就不要再被聲調取回去當注音——那是自動判斷的規則，
    /// 使用者已經表態了就不該再猜。
    forced_punct: bool,
}

impl BopomofoInput {
    /// # 一鍵兩用的那五個鍵
    ///
    /// `,` `.` `;` `/` `-` 在大千配置上是 ㄝㄡㄤㄥㄦ。單看那一下判斷不出
    /// 意圖——ㄝ、ㄡ、ㄤ、ㄦ 都能自成音節（欸、歐、昂、二），**要多看
    /// 一鍵**：接了聲調就是注音，否則構不成字，那就是標點。
    ///
    /// 判斷本身**不必自己寫**：`punct::is_punct` 已經在做這件事（自動模式
    /// 就是靠它，所以 `su3,` 打得出「你，」）。這裡只要讓那個鍵**進得了
    /// 按鍵串**——鎖定模式原本會先被音節緩衝吃掉，到不了那個判斷。
    ///
    /// 所以做法是「停在按鍵串裡」而不是「停在緩衝裡」：
    ///
    /// - 空緩衝按下那五個鍵 → 直接進按鍵串，`is_punct` 判成標點
    /// - 下一鍵是聲調 → 把它從按鍵串取回緩衝，當注音重打一次
    ///
    /// **停在緩衝裡是錯的**：那時直接按 Enter 送出，`text()` 只看已收尾的
    /// 按鍵，那個標點就憑空消失了。
    fn push(&mut self, ch: char) -> Changed {
        use crate::bopomofo::keymap::{role_of, Role};
        let auto = self.punct_mode == crate::config::LockPunct::Auto;

        // **先讀再清**：下面要用它擋掉「取回當注音」，
        // 先清掉的話檢查時永遠是 false
        let was_forced = std::mem::take(&mut self.forced_punct);
        if auto && self.syllable.is_empty() {
            // 空緩衝按下一鍵兩用的鍵：先當標點放進按鍵串
            if crate::cutpoint::punct::is_ambiguous(ch) {
                self.keys.push(ch);
                self.recompute();
                return Changed::Segments;
            }
            // 按了聲調，而按鍵串尾端剛好是待決的那個鍵 → 它其實是注音
            if role_of(ch) == Some(Role::Tone) && !was_forced {
                if let Some(last) = self.keys.chars().last() {
                    if crate::cutpoint::punct::is_ambiguous(last) {
                        self.keys.pop();
                        let _ = self.syllable.key_press(last);
                    }
                }
            }
        }
        self.push_key(ch)
    }

    /// **使用者明講這是標點**（Ctrl+鍵）。不管設定怎麼設都照做。
    fn push_punct(&mut self, ch: char) -> Changed {
        // 正在打的音節先收尾，不然標點會插到它前面
        if !self.syllable.is_empty() {
            let k = self.syllable.keys();
            self.syllable.clear();
            self.keys.push_str(&k);
        }
        self.keys.push(ch);
        self.forced_punct = true;
        self.recompute();
        Changed::Segments
    }

    fn push_key(&mut self, ch: char) -> Changed {
        match self.syllable.key_press(ch) {
            KeyResult::Absorbed => Changed::PendingOnly,
            KeyResult::Committed => {
                let k = self.syllable.keys();
                self.syllable.clear();
                self.keys.push_str(&k);
                self.recompute();
                Changed::Segments
            }
            KeyResult::Rejected => {
                // **注音鍵被拒絕＝非法組合，整個丟棄**。
                //
                // 那是 `key_press` 擋下來的（例如 ㄈ 之後按 ㄩ），
                // 使用者手滑而已，不該把那個鍵塞進按鍵串——塞進去
                // 會變成 `mㄈ` 這種東西。
                if crate::bopomofo::keymap::is_bopomofo_key(ch) {
                    return Changed::Nothing;
                }
                // 真的不是注音鍵（標點）才進按鍵串
                self.keys.push(ch);
                self.recompute();
                Changed::Segments
            }
        }
    }

    fn backspace(&mut self) -> Changed {
        // 正在打的音節優先刪——那是使用者看到的東西
        if self.syllable.backspace() {
            return Changed::PendingOnly;
        }
        if self.keys.is_empty() {
            return Changed::Nothing;
        }
        // **刪已完成的音節時，整個音節退回編輯狀態**。
        //
        // 不能只 `keys.pop()` 砍一個字元——`su3` 砍成 `su` 之後不成
        // 音節，切段會產出爛段落，畫面就從「你」跳回「su」。
        //
        // 新酷音的行為是把那個音節放回緩衝，使用者可以接著改
        // （刪掉聲調就回到 `ㄋㄧ`，改個聲調重新收尾）。
        let last = self.pop_last_syllable();
        for c in last.chars() {
            self.syllable.key_press(c);
        }
        // 放回去之後刪掉最後一個符號，才是「退格」該有的效果
        self.syllable.backspace();
        self.recompute();
        Changed::Segments
    }

    /// 從已完成的按鍵串尾端取下最後一個音節。
    ///
    /// 音節一律以聲調鍵收尾（見 `bopomofo::syllable`），所以從尾端
    /// 往前找到**前一個聲調鍵**的下一位，中間那段就是最後一個音節。
    fn pop_last_syllable(&mut self) -> String {
        let chars: Vec<char> = self.keys.chars().collect();
        if chars.is_empty() {
            return String::new();
        }
        // 尾端那個聲調鍵不算（它是這個音節自己的收尾）
        let mut start = 0;
        for i in (0..chars.len().saturating_sub(1)).rev() {
            if matches!(
                crate::bopomofo::keymap::role_of(chars[i]),
                Some(crate::bopomofo::keymap::Role::Tone)
            ) {
                start = i + 1;
                break;
            }
        }
        let last: String = chars[start..].iter().collect();
        self.keys.truncate(self.keys.len() - last.len());
        last
    }

    fn recompute(&mut self) {
        self.segments = if self.keys.is_empty() {
            Vec::new()
        } else {
            single_language_segments(&self.keys, Language::Bopomofo)
        };
    }
}

/// **鎖定日文**的輸入：羅馬字邊打邊轉假名。
///
/// # 跟注音的差別
///
/// 沒有「四格覆寫」——羅馬字是線性序列，`ka` 的 `k` 跟 `a` 沒有
/// 「格」的概念，打錯只能刪。所以這裡是單純的字串追加。
///
/// 要做的只有一件事：**組字區顯示假名而不是羅馬字**。轉得出來的
/// 部分轉掉，剩下的殘留字母原樣留著——`sush` 顯示「すsh」。
///
/// 那就是 mozc 的 `pending` 概念。mozc 還有第三層 `ambiguous`
/// （`n` 可能是「ん」也可能是「な」），本專案規定撥音一律打 `nn`，
/// 歧義不存在，所以只要兩層。
#[derive(Debug, Clone, Default)]
pub struct RomajiInput {
    /// 已經轉成假名的那部分，存的是**原始羅馬字**。
    ///
    /// 存羅馬字而不是假名——後面查詞庫、選字都是照按鍵走的，
    /// 假名只是顯示用。
    keys: String,
    /// 還沒湊成一個 mora 的殘留字母（`sush` 的 `sh`）。
    pending: String,
    segments: Vec<Segment>,
}

impl RomajiInput {
    fn push(&mut self, ch: char) -> Changed {
        // **轉換要帶上前文**。
        //
        // 長音 `-` 單獨轉不出來（引擎不接受「開頭就是長音」，那確實
        // 不是日文），但接在音後面就有效：`ke-` → けー。所以拿
        // 「已完成的按鍵 + 殘留 + 新字元」整串去轉，再看轉掉多少。
        let mut buf = std::mem::take(&mut self.pending);
        buf.push(ch);
        let whole = format!("{}{}", self.keys, buf);
        let (_, rest) = crate::romaji::kana::to_kana_partial(&whole);
        if rest.len() >= buf.len() {
            // 這一輪什麼都沒轉掉，繼續累積
            self.pending = buf;
            return Changed::PendingOnly;
        }
        // 轉掉了一部分：`whole` 扣掉殘留就是已完成的按鍵
        self.keys = whole[..whole.len() - rest.len()].to_string();
        self.pending = rest;
        self.recompute();
        Changed::Segments
    }

    fn backspace(&mut self) -> Changed {
        // 殘留字母優先刪——那是使用者最後打的
        if self.pending.pop().is_some() {
            return Changed::PendingOnly;
        }
        if self.keys.pop().is_none() {
            return Changed::Nothing;
        }
        // **刪掉之後尾巴可能不成 mora**，要重新分配。
        //
        // 例如 `sushi` 刪掉 `i` 剩 `sush`——`sh` 該退回殘留，
        // 不然切段會拿到不成 mora 的東西。
        let all = std::mem::take(&mut self.keys);
        let (_, rest) = crate::romaji::kana::to_kana_partial(&all);
        let consumed = all.len() - rest.len();
        self.keys = all[..consumed].to_string();
        self.pending = rest;
        self.recompute();
        Changed::Segments
    }

    fn recompute(&mut self) {
        self.segments = if self.keys.is_empty() {
            Vec::new()
        } else {
            single_language_segments(&self.keys, Language::Romaji)
        };
    }
}

/// 單一語言時的切段：只把標點切出來自成一段。
///
/// 鎖定的語意是「接下來都打這個語言」，沒有「哪裡換語言」的問題。
/// 標點例外——它不屬於任何語言（新注音打逗號也是獨立的一個符號），
/// 而且 `compose` 那層靠 `is_mark` 決定不要拿它去查詞庫。
fn single_language_segments(keys: &str, lang: Language) -> Vec<Segment> {
    let chars: Vec<char> = keys.chars().collect();
    let mut out: Vec<Segment> = Vec::new();
    let mut buf = String::new();

    for (i, &c) in chars.iter().enumerate() {
        // **鎖定日文時 `-` 是長音不是標點**（`ke-ki` ＝ ケーキ）。
        //
        // `is_punct` 會依前文判斷，但那是為自動模式寫的；鎖定之後
        // 已經確定是日文，`-` 一律當長音。
        let dash_is_chouon = lang == Language::Romaji && c == '-';
        if !dash_is_chouon && punct::is_punct(keys, i) {
            if !buf.is_empty() {
                out.push(Segment {
                    keys: std::mem::take(&mut buf),
                    lang,
                    is_mark: false,
                });
            }
            out.push(Segment {
                keys: c.to_string(),
                lang,
                is_mark: true,
            });
        } else {
            buf.push(c);
        }
    }
    if !buf.is_empty() {
        out.push(Segment {
            keys: buf,
            lang,
            is_mark: false,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    /// 開頭的符號直接送出，不進組字區。
    mod 開頭符號 {
        use super::super::passthrough_alone;

        #[test]
        fn 常見符號直接送出() {
            for ch in [
                '!', '@', '#', '$', '%', '^', '&', '*', '(', ')', '"', '?', ':',
            ] {
                assert!(passthrough_alone(ch), "「{ch}」該直接送出");
            }
        }

        #[test]
        fn 注音鍵不能放行() {
            // **半形標點有一半是注音鍵**：ㄝㄡㄥㄤㄦ。
            // 放行的話那幾個音就打不出來了。
            for ch in [',', '.', '/', ';', '-'] {
                assert!(!passthrough_alone(ch), "「{ch}」是注音鍵，不能直接送出");
            }
        }

        #[test]
        fn 字母數字不受影響() {
            for ch in ['a', 'z', 'A', '0', '9'] {
                assert!(!passthrough_alone(ch), "「{ch}」要進組字區");
            }
        }

        #[test]
        fn 空白不算符號() {
            // 空白是注音的一聲，也是段落分隔符，絕對不能繞過組字
            assert!(!passthrough_alone(' '));
        }
    }

    use super::*;

    fn press(input: &mut Input, keys: &str, lock: Option<Language>) {
        for c in keys.chars() {
            input.push(c, lock);
        }
    }

    #[test]
    fn 兩種模式是不同型別() {
        // 結構上分開，不是同一個型別的兩種參數
        assert!(matches!(Input::for_lock(None), Input::Cascade(_)));
        assert!(matches!(
            Input::for_lock(Some(Language::Bopomofo)),
            Input::Bopomofo(_)
        ));
    }

    #[test]
    fn 三種語言三條路() {
        // 注音與日文各有自己的輸入邏輯；英文是 passthrough，
        // 打什麼顯示什麼，不需要自己的一條路
        assert!(matches!(
            Input::for_lock(Some(Language::Bopomofo)),
            Input::Bopomofo(_)
        ));
        assert!(matches!(
            Input::for_lock(Some(Language::Romaji)),
            Input::Romaji(_)
        ));
        assert!(matches!(
            Input::for_lock(Some(Language::English)),
            Input::Cascade(_)
        ));
    }

    #[test]
    fn 日文邊打邊轉假名() {
        // mozc 的 pending 概念：轉得出來的轉掉，剩的留著
        let lock = Some(Language::Romaji);
        let mut i = Input::for_lock(lock);
        i.push('s', lock);
        assert_eq!(i.pending(), "s", "還湊不出 mora");
        assert_eq!(i.keys(), "");
        i.push('u', lock);
        assert_eq!(i.keys(), "su", "湊出來了進按鍵串");
        assert_eq!(i.pending(), "");
    }

    #[test]
    fn 日文殘留字母留著等後續() {
        let lock = Some(Language::Romaji);
        let mut i = Input::for_lock(lock);
        press(&mut i, "sush", lock);
        assert_eq!(i.keys(), "su", "只有 su 轉得出假名");
        assert_eq!(i.pending(), "sh", "sh 等後續");
        i.push('i', lock);
        assert_eq!(i.keys(), "sushi");
        assert_eq!(i.pending(), "");
    }

    #[test]
    fn 日文促音要等後續才成立() {
        // kk 本身不成 mora，要接母音才是「っか」
        let lock = Some(Language::Romaji);
        let mut i = Input::for_lock(lock);
        press(&mut i, "kk", lock);
        assert_eq!(i.pending(), "kk");
        i.push('a', lock);
        assert_eq!(i.keys(), "kka");
    }

    #[test]
    fn 日文退格逐字元退回() {
        // 刪掉之後尾巴可能不成 mora，要退回殘留區
        let lock = Some(Language::Romaji);
        let mut i = Input::for_lock(lock);
        press(&mut i, "sushi", lock);
        i.backspace(lock);
        assert_eq!(i.keys(), "su", "sh 不成 mora，退回殘留");
        assert_eq!(i.pending(), "sh");
    }

    #[test]
    fn 日文長音不是標點() {
        // `ke-ki` ＝ ケーキ。`is_punct` 依前文判斷 `-`，那是為自動
        // 模式寫的；鎖定日文之後一律當長音。
        let lock = Some(Language::Romaji);
        let mut i = Input::for_lock(lock);
        press(&mut i, "ke-ki", lock);
        assert_eq!(i.keys(), "ke-ki");
        assert_eq!(i.pending(), "");
        let segs = &i.cuttings()[0];
        assert_eq!(segs.len(), 1, "不該被 `-` 切開：{segs:?}");
    }

    #[test]
    fn 轉換要帶上前文() {
        // **長音單獨轉不出來**（引擎不接受「開頭就是長音」），
        // 但接在音後面就有效。所以轉換時要拿整串去試，
        // 不能只看殘留的部分。
        let lock = Some(Language::Romaji);
        let mut i = Input::for_lock(lock);
        press(&mut i, "ke", lock);
        i.push('-', lock);
        assert_eq!(i.keys(), "ke-", "長音該接上去");
        assert_eq!(i.pending(), "");
    }

    #[test]
    fn 日文沒有覆寫這回事() {
        // 羅馬字是線性序列，跟注音的四格覆寫不同
        let lock = Some(Language::Romaji);
        let mut i = Input::for_lock(lock);
        press(&mut i, "ka", lock);
        press(&mut i, "ki", lock);
        assert_eq!(i.keys(), "kaki", "追加不覆寫");
    }

    #[test]
    fn 注音模式同格覆寫() {
        let mut i = Input::for_lock(Some(Language::Bopomofo));
        i.push('1', Some(Language::Bopomofo)); // ㄅ
        assert_eq!(i.pending(), "ㄅ");
        i.push('q', Some(Language::Bopomofo)); // ㄆ
        assert_eq!(i.pending(), "ㄆ", "同是聲母該覆寫");
    }

    #[test]
    fn 注音模式聲調收尾() {
        let lock = Some(Language::Bopomofo);
        let mut i = Input::for_lock(lock);
        press(&mut i, "su", lock);
        assert_eq!(i.keys(), "", "還沒收尾，按鍵串還是空的");
        i.push('3', lock);
        assert_eq!(i.keys(), "su3", "收尾後進按鍵串");
        assert_eq!(i.pending(), "");
    }

    #[test]
    fn 自動模式是字串追加不覆寫() {
        let mut i = Input::for_lock(None);
        press(&mut i, "1q", None);
        assert_eq!(i.keys(), "1q", "自動模式不該覆寫");
        assert_eq!(i.pending(), "", "自動模式沒有 pending 這回事");
    }

    #[test]
    fn 自動模式有多種切法() {
        let mut i = Input::for_lock(None);
        press(&mut i, "su3", None);
        assert!(!i.cuttings().is_empty());
    }

    #[test]
    fn 注音模式只有一種切法() {
        let lock = Some(Language::Bopomofo);
        let mut i = Input::for_lock(lock);
        press(&mut i, "su3", lock);
        assert_eq!(i.cuttings().len(), 1);
    }

    #[test]
    fn 換模式時未收尾的按鍵要結算() {
        let lock = Some(Language::Bopomofo);
        let mut i = Input::for_lock(lock);
        press(&mut i, "su", lock);
        assert_eq!(i.drain_keys(), "su", "緩衝裡的按鍵要接回來");
        assert!(i.is_empty());
    }

    #[test]
    fn 從按鍵串重建() {
        let lock = Some(Language::Bopomofo);
        let i = Input::from_keys("su3", lock);
        assert_eq!(i.keys(), "su3");
        assert_eq!(i.cuttings().len(), 1);
    }

    #[test]
    fn 注音鍵盤上沒有的標點自成一段() {
        // `!` 不在注音鍵盤上，一定是標點
        let lock = Some(Language::Bopomofo);
        let mut i = Input::for_lock(lock);
        press(&mut i, "su3!", lock);
        let segs = &i.cuttings()[0];
        assert!(segs.iter().any(|s| s.is_mark), "驚嘆號該自成一段：{segs:?}");
    }

    /// **這條規則被推翻過**（2026-09-01，使用者回報）。
    ///
    /// 原本寫的是「鎖定之後已經確定是注音，所以逗號就是ㄝ」，聽起來
    /// 合理，實際後果是**鎖定注音時標點完全打不出來**——`,` `.` `;` `/` `-`
    /// 五個最常用的標點鍵在大千配置上全是注音（ㄝㄡㄤㄥㄦ）。
    ///
    /// 正確的判準跟自動模式同一條：ㄝ 要成字一定得再按聲調，所以**多看
    /// 一鍵**就分得出來。做法是先當標點停在按鍵串裡，接了聲調再取回來。
    #[test]
    fn 一鍵兩用的鍵先當標點接了聲調才是注音() {
        let lock = Some(Language::Bopomofo);
        let mut i = Input::for_lock(lock);
        press(&mut i, ",", lock);
        assert_eq!(i.pending(), "", "逗號不進音節緩衝");
        assert_eq!(i.keys(), ",", "先停在按鍵串裡，Enter 送得出去");

        press(&mut i, "4", lock);
        assert_eq!(i.keys(), ",4", "接了聲調就取回來當注音（ㄝˋ）");
    }

    /// 這一組測試要查詞庫（`viable` 靠它判斷音節存不存在）。
    fn load_dict() -> bool {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data");
        crate::preload(&data, crate::config::Engines::default());
        crate::dict::bopomofo_loaded()
    }

    #[test]
    fn 非法組合按下去就擋掉() {
        if !load_dict() {
            return;
        }
        // ㄈ 配不了 ㄩ——中文沒這個音（詞庫查不到字）。
        // 新酷音的行為是直接不收那個鍵，緩衝維持 ㄈ。
        let lock = Some(Language::Bopomofo);
        let mut i = Input::for_lock(lock);
        i.push('z', lock); // ㄈ
        assert_eq!(i.pending(), "ㄈ");
        i.push('m', lock); // ㄩ — 非法
        assert_eq!(i.pending(), "ㄈ", "非法的介音不該收");
        assert_eq!(i.keys(), "", "被擋的鍵也不該跑進按鍵串");
    }

    #[test]
    fn 擋掉之後可以接著打合法的() {
        if !load_dict() {
            return;
        }
        // 使用者可能只是手滑，接著要打的是 ㄈㄥ
        let lock = Some(Language::Bopomofo);
        let mut i = Input::for_lock(lock);
        press(&mut i, "zm", lock); // ㄩ 被擋
        press(&mut i, "/", lock); // ㄥ
        assert_eq!(i.pending(), "ㄈㄥ");
    }

    #[test]
    fn 合法組合不受影響() {
        if !load_dict() {
            return;
        }
        let lock = Some(Language::Bopomofo);
        let mut i = Input::for_lock(lock);
        press(&mut i, "sul", lock); // ㄋㄧㄠ
        assert_eq!(i.pending(), "ㄋㄧㄠ");
    }

    #[test]
    fn 刪已完成的音節會退回編輯狀態() {
        // **這是實測發現的 bug**：原本只 `keys.pop()` 砍一個字元，
        // `su3` 變成 `su` 不成音節，畫面就從「你」跳回「su」——
        // 看起來像整個輸入法退回自動模式了。
        //
        // 正確行為（新酷音）：整個音節放回緩衝，可以接著改。
        let lock = Some(Language::Bopomofo);
        let mut i = Input::for_lock(lock);
        press(&mut i, "su3", lock);
        assert_eq!(i.keys(), "su3");
        i.backspace(lock);
        assert_eq!(i.keys(), "", "已完成的按鍵串要清掉");
        assert_eq!(i.pending(), "ㄋㄧ", "音節退回緩衝，刪掉聲調");
    }

    #[test]
    fn 刪音節只影響最後一個() {
        let lock = Some(Language::Bopomofo);
        let mut i = Input::for_lock(lock);
        press(&mut i, "su3cl3", lock);
        i.backspace(lock);
        assert_eq!(i.keys(), "su3", "前面的音節不動");
        assert_eq!(i.pending(), "ㄏㄠ");
    }

    #[test]
    fn 一聲的音節也能退回() {
        // 一聲是空白鍵收尾，`pop_last_syllable` 要認得
        let lock = Some(Language::Bopomofo);
        let mut i = Input::for_lock(lock);
        press(&mut i, "a8 ", lock);
        i.backspace(lock);
        assert_eq!(i.pending(), "ㄇㄚ");
    }

    #[test]
    fn 全部刪光之後仍在注音模式() {
        let lock = Some(Language::Bopomofo);
        let mut i = Input::for_lock(lock);
        press(&mut i, "su3", lock);
        for _ in 0..5 {
            i.backspace(lock);
        }
        assert!(i.is_empty());
        // 再打還是要同格覆寫
        i.push('1', lock);
        i.push('q', lock);
        assert_eq!(i.pending(), "ㄆ");
    }

    #[test]
    fn backspace_先刪未收尾的音節() {
        let lock = Some(Language::Bopomofo);
        let mut i = Input::for_lock(lock);
        press(&mut i, "su3su", lock);
        assert_eq!(i.keys(), "su3");
        assert_eq!(i.pending(), "ㄋㄧ");
        i.backspace(lock);
        assert_eq!(i.pending(), "ㄋ", "先刪正在打的");
        assert_eq!(i.keys(), "su3", "已收尾的不動");
    }
}
