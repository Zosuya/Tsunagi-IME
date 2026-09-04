//! 輸入法指令：打特定字串就能叫出功能。
//!
//! # 為什麼不用 `/config` 那種前綴
//!
//! 前綴會**搶走輸入**——在別的程式想打 `/config` 這串字就打不出來了。
//! 而且 `/` 在注音是ㄥ，打起來也怪。
//!
//! 所以改成：正常組字照跑，只是候選清單多一項「開啟設定」。要用就選它，
//! 不選就當普通文字送出。**不搶輸入，也不必記特殊前綴**。
//!
//! # 三種語言都能觸發
//!
//! 這個輸入法的核心精神是不必手動切換語言——你在打日文時突然想改設定，
//! 不該被迫先切成英文思維。所以三種打法都認：
//!
//! | 打什麼 | 語言 | 輸出 |
//! |---|---|---|
//! | `config` | 英文 | config |
//! | `gk42u/4` | 注音 | 設定 |
//! | `settei` | 日文 | せってい |
//!
//! # 兩條路指向同一件事
//!
//! - **候選清單**：排在最後一項，發現得到、不用記
//! - **上上下下**：熟了之後的快捷手勢，不必往下選
//!
//! 候選那一項本身就是提示——第一次打到 `config` 就知道有這回事。

/// 輸入法的指令。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// 開啟設定頁
    OpenSettings,
    /// 開關某個語言引擎。
    ///
    /// **是切換不是停用**：只能關不能開的話，關掉日文之後就得跑去
    /// 設定頁才回得來。切換的話再打一次同樣的詞就恢復。
    ///
    /// 而且日文關掉之後，英文的 `japanese` 仍然打得出來——不會把
    /// 自己鎖在門外。
    ToggleEngine(crate::language::Language),
}

impl Command {
    /// 候選清單裡顯示的文字。
    ///
    /// **切換類的指令要看目前狀態**——顯示「停用日文」還是「啟用日文」
    /// 差很多，不講清楚使用者不知道按下去會變成哪樣。
    pub fn label(self, engines: crate::config::Engines) -> String {
        match self {
            Command::OpenSettings => "⚙ 開啟設定".to_string(),
            Command::ToggleEngine(lang) => {
                let verb = if engines.enabled(lang) {
                    "停用"
                } else {
                    "啟用"
                };
                format!("⚙ {verb}{}", lang_name(lang))
            }
        }
    }
}

/// 指令文字裡怎麼稱呼這個語言。
fn lang_name(lang: crate::language::Language) -> &'static str {
    use crate::language::Language;
    match lang {
        Language::Bopomofo => "注音",
        Language::Romaji => "日文",
        Language::English => "英文",
    }
}

/// 觸發詞表：`(按鍵串, 指令)`。
///
/// 比對的是**按鍵串**而不是輸出文字——使用者實際打的是這些鍵，
/// 而且輸出文字會隨選字改變（「設定」可能被選成「設訂」）。
const TRIGGERS: &[(&str, Command)] = &[
    ("config", Command::OpenSettings),
    // 注音：ㄕㄜˋㄉㄧㄥˋ →「設定」
    ("gk42u/4", Command::OpenSettings),
    // 日文羅馬字：せってい →「設定」
    ("settei", Command::OpenSettings),
    // ── 開關語言引擎 ──
    //
    // **英文沒有對應的指令**：它是瀑布的最後一站，關掉之後什麼都
    // 打不出來，連要把它開回來的指令都打不了。
    (
        "japanese",
        Command::ToggleEngine(crate::language::Language::Romaji),
    ),
    // 注音：ㄖˋㄨㄣˊ →「日文」
    (
        "b4jp6",
        Command::ToggleEngine(crate::language::Language::Romaji),
    ),
    // 日文羅馬字：にほんご。
    //
    // **兩種打法都收**：「ん」在子音前很多人習慣打 `nn`，因為單一個
    // `n` 會被當成な行的開頭（`ni`、`na`…）。逼使用者記得這裡要打哪
    // 一種，等於在他已經想不起指令的時候再加一道門檻。
    (
        "nihongo",
        Command::ToggleEngine(crate::language::Language::Romaji),
    ),
    (
        "nihonngo",
        Command::ToggleEngine(crate::language::Language::Romaji),
    ),
    (
        "bopomofo",
        Command::ToggleEngine(crate::language::Language::Bopomofo),
    ),
    // 注音：ㄓㄨˋㄧㄣ →「注音」（一聲是空白，`match_keys` 會 trim 掉）
    (
        "5j4up",
        Command::ToggleEngine(crate::language::Language::Bopomofo),
    ),
];

/// 這串按鍵對應到哪個指令？
///
/// **必須完全相同**才算——中間多一個字就不是指令了。
/// 不然打「設定檔」的過程中會一直跳出選項。
pub fn match_keys(keys: &str) -> Option<Command> {
    let k = keys.trim();
    TRIGGERS.iter().find(|(t, _)| *t == k).map(|(_, cmd)| *cmd)
}

/// 手勢的目標序列。
const TARGET: [Dir; 4] = [Dir::Up, Dir::Up, Dir::Down, Dir::Down];

/// 「上上下下」手勢的按鍵序列長度。
pub const GESTURE_LEN: usize = TARGET.len();

/// 手勢的一個方向。平台層把方向鍵翻譯成這個。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
}

/// 手勢偵測器：記住最近按的方向鍵，湊滿「上上下下」就觸發。
///
/// # 為什麼用方向鍵而不是別的組合
///
/// 打字時**不可能**剛好按出上上下下——那四下在選字模式下是有意義的
/// 操作（上下移動反白），但連續四下同樣的模式不會自然發生。
/// 誤觸機率實質為零。
#[derive(Debug, Default, Clone)]
pub struct Gesture {
    recent: Vec<Dir>,
}

impl Gesture {
    /// 按了一個方向鍵。湊滿「上上下下」回 `true`。
    pub fn push(&mut self, d: Dir) -> bool {
        self.recent.push(d);
        // 只留最近四下，前面的不重要
        if self.recent.len() > GESTURE_LEN {
            self.recent.remove(0);
        }
        self.recent == TARGET
    }

    /// 目前這串**還有可能**湊成手勢嗎？
    ///
    /// # 為什麼需要這個
    ///
    /// 平台層要靠它決定「這一下方向鍵先不要進選字」。
    /// 不然按第一下 ↑ 就進了選字模式，模式一變方向鍵的意義就變了，
    /// 手勢永遠收不到第二下——那正是第一版做壞的地方。
    ///
    /// 按了 ↑ 是有希望的（`↑↑↓↓` 的開頭），按 ↓ 就沒希望了，
    /// 那一下要當成普通的「進選字」。
    pub fn promising(&self) -> bool {
        !self.recent.is_empty() && TARGET.starts_with(&self.recent[..])
    }

    /// 清掉記錄。打了別的鍵之後要呼叫——手勢必須是**連續**的四下，
    /// 中間插了打字就不算。
    pub fn clear(&mut self) {
        self.recent.clear();
    }
}

#[cfg(test)]
mod tests {
    /// 開關語言引擎的指令。
    mod 切換語言 {
        use super::super::*;
        use crate::config::Engines;
        use crate::language::Language;

        #[test]
        fn 三種打法都認得日文() {
            // `nihonngo` 是同一個詞的另一種羅馬字打法（ん 打成 nn）
            for keys in ["japanese", "b4jp6", "nihongo", "nihonngo"] {
                assert_eq!(
                    match_keys(keys),
                    Some(Command::ToggleEngine(Language::Romaji)),
                    "「{keys}」該觸發切換日文"
                );
            }
        }

        #[test]
        fn 注音也有兩種打法() {
            for keys in ["bopomofo", "5j4up"] {
                assert_eq!(
                    match_keys(keys),
                    Some(Command::ToggleEngine(Language::Bopomofo)),
                    "「{keys}」該觸發切換注音"
                );
            }
        }

        #[test]
        fn 英文沒有對應的指令() {
            // 英文是瀑布的最後一站，關掉之後連要開回來的指令都打不了
            let has_english = TRIGGERS
                .iter()
                .any(|(_, c)| *c == Command::ToggleEngine(Language::English));
            assert!(!has_english, "不該有停用英文的指令");
        }

        #[test]
        fn 標籤要講清楚按下去會變怎樣() {
            let mut e = Engines::default();
            let cmd = Command::ToggleEngine(Language::Romaji);
            assert!(e.enabled(Language::Romaji));
            assert!(cmd.label(e).contains("停用"), "開著的時候按下去是停用");
            e.toggle(Language::Romaji);
            assert!(cmd.label(e).contains("啟用"), "關掉之後按下去是啟用");
        }

        #[test]
        fn 切換是可逆的() {
            let mut e = Engines::default();
            assert!(!e.toggle(Language::Romaji), "第一次關掉");
            assert!(e.toggle(Language::Romaji), "再一次開回來");
        }

        #[test]
        fn 英文動不了() {
            let mut e = Engines::default();
            assert!(e.toggle(Language::English), "英文永遠是開的");
            assert!(e.enabled(Language::English));
        }

        #[test]
        fn 打一半不會誤觸() {
            // 「japan」「nihon」都還不是完整的觸發詞
            assert_eq!(match_keys("japan"), None);
            assert_eq!(match_keys("nihon"), None);
            assert_eq!(match_keys("japaneses"), None);
        }
    }

    use super::*;

    #[test]
    fn 三種語言都能觸發設定() {
        assert_eq!(match_keys("config"), Some(Command::OpenSettings));
        assert_eq!(match_keys("gk42u/4"), Some(Command::OpenSettings));
        assert_eq!(match_keys("settei"), Some(Command::OpenSettings));
    }

    #[test]
    fn 必須完全相同才算指令() {
        // 打「設定檔」的過程中不該一直跳出選項
        assert_eq!(match_keys("gk42u/4gj4"), None);
        assert_eq!(match_keys("configuration"), None);
        assert_eq!(match_keys("conf"), None, "打到一半不算");
        assert_eq!(match_keys(""), None);
    }

    #[test]
    fn 手勢要連續四下() {
        let mut g = Gesture::default();
        assert!(!g.push(Dir::Up));
        assert!(!g.push(Dir::Up));
        assert!(!g.push(Dir::Down));
        assert!(g.push(Dir::Down), "上上下下該觸發");
    }

    #[test]
    fn 有希望的前綴才算數() {
        let mut g = Gesture::default();
        // ↑ 是 ↑↑↓↓ 的開頭，有希望
        g.push(Dir::Up);
        assert!(g.promising());
        g.push(Dir::Up);
        assert!(g.promising());
        g.push(Dir::Down);
        assert!(g.promising());

        // 但一開始按 ↓ 就沒希望了，那一下該當成普通的進選字
        let mut g2 = Gesture::default();
        g2.push(Dir::Down);
        assert!(!g2.promising());
    }

    #[test]
    fn 順序不對不觸發() {
        let mut g = Gesture::default();
        for d in [Dir::Down, Dir::Down, Dir::Up, Dir::Up] {
            assert!(!g.push(d));
        }
    }

    #[test]
    fn 中間打字就重來() {
        let mut g = Gesture::default();
        g.push(Dir::Up);
        g.push(Dir::Up);
        g.clear(); // 打了別的鍵
        g.push(Dir::Down);
        assert!(!g.push(Dir::Down), "被打斷過就不該觸發");
    }

    #[test]
    fn 多按幾下還是認得() {
        let mut g = Gesture::default();
        // 前面亂按不影響，只看最近四下
        g.push(Dir::Down);
        g.push(Dir::Down);
        g.push(Dir::Up);
        g.push(Dir::Up);
        g.push(Dir::Down);
        assert!(g.push(Dir::Down), "最近四下是上上下下");
    }
}
