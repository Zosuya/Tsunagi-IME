//! 全半形切換的提示列與動畫。
//!
//! ```text
//! ┌────────────────────┐
//! │  自動 [半形] 全形   │  ← 反白條滑動，之後整列淡出
//! ├────────────────────┤
//! │ ▶ 你好             │
//! │ 1 你               │
//! └────────────────────┘
//! ```
//!
//! # 為什麼需要計時器
//!
//! 候選視窗**只在按鍵時重畫**——TSF 沒有「每一幀叫我一次」的機制，
//! `OnKeyDown` 處理完就沒人會再叫我們。動畫得自己開 `SetTimer`
//! 每幀重畫。
//!
//! 那個計時器跑在宿主行程（記事本、瀏覽器）的訊息迴圈裡，所以
//! **動畫要短**——滑動 150ms、停留 900ms、淡出 250ms，總共約
//! 1.3 秒之後計時器就停掉，不會一直佔著宿主。

use std::time::{Duration, Instant};

use ime_core::language::Language;
use ime_core::width::Width;

/// 反白條從一格滑到另一格要多久。
const SLIDE: Duration = Duration::from_millis(110);
/// 滑完之後停留多久才開始淡出。
const HOLD: Duration = Duration::from_millis(500);
/// 淡出要多久。
const FADE: Duration = Duration::from_millis(180);

/// 三個選項的順序。反白條在這三格之間移動。
pub const OPTIONS: [Width; 3] = [Width::Auto, Width::Half, Width::Full];

/// 標籤：單一個中文字。
///
/// `A`／`Ａ` 那種寬度對比在實際的小視窗裡看不出來——字太小，
/// 而且不是每套字型的全形拉丁字母都夠寬。單字中文反而清楚。
pub fn symbol(w: Width) -> &'static str {
    match w {
        Width::Auto => "自",
        Width::Half => "半",
        Width::Full => "全",
    }
}

/// 語言模式的四格：自動、注音、日文、英文。
///
/// 跟全半形共用同一個提示視窗與動畫，差別只有格數與標籤。
/// `None` 代表自動辨識（這個輸入法的預設與特色）。
pub const LANG_OPTIONS: [Option<Language>; 4] = [
    None,
    Some(Language::Bopomofo),
    Some(Language::Romaji),
    Some(Language::English),
];

/// 語言模式的標籤，跟全半形一樣用單字中文。
pub fn lang_symbol(l: Option<Language>) -> &'static str {
    match l {
        None => "自",
        Some(x) => x.short(),
    }
}

/// 依設定過濾出**實際會出現在提示列上的**語言模式。
///
/// 設定裡關掉的引擎連自動辨識都會跳過（見開發文件 §2.9），輪替時
/// 也跳過它——提示列當然不該再畫那一格，不然使用者會看到一個
/// 永遠輪不到的格子。
///
/// 「自動辨識」那格一定留著，它不是某個引擎，是預設狀態。
pub fn lang_options(engines: &ime_core::config::Engines) -> Vec<Option<Language>> {
    LANG_OPTIONS
        .iter()
        .copied()
        .filter(|o| match o {
            None => true,
            Some(l) => engines.enabled(*l),
        })
        .collect()
}

/// 某個語言模式排在第幾格。
///
/// **要傳過濾後的清單**——用固定的 `LANG_OPTIONS` 算的話，關掉中間
/// 某個語言時反白會滑到錯的格子。
pub fn lang_index_in(options: &[Option<Language>], l: Option<Language>) -> usize {
    options.iter().position(|&o| o == l).unwrap_or(0)
}

/// 動畫狀態。
#[derive(Debug, Clone)]
pub struct WidthBar {
    /// 從哪一格滑過來。**用小數**——連按兩次時要從中途的實際位置
    /// 接著走，取整數會讓反白跳一下。
    from: f32,
    /// 滑到哪一格
    to: usize,
    /// 這一輪動畫從什麼時候開始
    started: Instant,
    /// Shift 還按著嗎？
    ///
    /// 按著就**不淡出**——使用者可能還要再按空白切下一個模式，
    /// 這時候把提示收掉會讓他看不到自己切到哪。放開才開始倒數。
    held: bool,
}

impl WidthBar {
    /// 開始一輪動畫：反白從 `from` 滑到 `to`。
    ///
    /// 已經在動的話會從**目前的位置**接著滑，不是硬跳回起點——
    /// 使用者連按兩次 Shift+空白時才不會閃一下。
    /// 動畫本身跟「那幾格是什麼意思」無關——內部一直都是用索引在算。
    /// 全半形（自／半／全）與語言輪替（自／注／日／英）共用同一套動畫，
    /// 型別轉換留在呼叫端。
    pub fn start_at(prev: Option<&Self>, from: usize, to: usize) -> Self {
        // 上一輪還在滑的話，接著它目前的位置走
        let from_idx = match prev {
            Some(p) if p.progress() < 1.0 => p.visual_index(),
            _ => from as f32,
        };
        Self {
            from: from_idx,
            to,
            started: Instant::now(),
            held: true,
        }
    }

    /// Shift 放開了——開始倒數淡出。
    ///
    /// 把計時歸零到「滑動已結束」的位置，這樣放開的當下就進入
    /// 停留期，不會因為按住太久而立刻跳到淡出。
    pub fn release(&mut self) {
        if !self.held {
            return;
        }
        self.held = false;
        // 滑動若還沒跑完就讓它跑完；跑完了就從停留期起算
        let elapsed = self.started.elapsed();
        if elapsed > SLIDE {
            self.started = Instant::now() - SLIDE;
        }
    }

    /// 滑動進度，0.0 到 1.0。
    fn progress(&self) -> f32 {
        let t = self.started.elapsed().as_secs_f32() / SLIDE.as_secs_f32();
        t.clamp(0.0, 1.0)
    }

    /// 反白條現在畫在第幾格（可以是小數，例如 1.37）。
    ///
    /// 用 ease-out 曲線：一開始快、接近終點慢下來。等速移動看起來
    /// 很機械，這條曲線比較像真的東西在動。
    pub fn visual_index(&self) -> f32 {
        let t = self.progress();
        let eased = 1.0 - (1.0 - t) * (1.0 - t);
        self.from + (self.to as f32 - self.from) * eased
    }

    /// 整列的不透明度，0.0 到 1.0。停留期滿之後開始淡出。
    ///
    /// Shift 按著時**永遠是 1.0**——不淡出。
    pub fn opacity(&self) -> f32 {
        if self.held {
            return 1.0;
        }
        let elapsed = self.started.elapsed();
        let fade_start = SLIDE + HOLD;
        if elapsed < fade_start {
            return 1.0;
        }
        let t = (elapsed - fade_start).as_secs_f32() / FADE.as_secs_f32();
        (1.0 - t).clamp(0.0, 1.0)
    }

    /// 動畫跑完了嗎？跑完就可以停掉計時器、不畫這一列。
    ///
    /// Shift 按著就**永遠不算完**——使用者還在操作。
    pub fn done(&self) -> bool {
        !self.held && self.started.elapsed() >= SLIDE + HOLD + FADE
    }

    /// 反白落在哪一格（畫底色用，取整數）。
    pub fn target(&self) -> usize {
        self.to
    }
}

/// 某個全半形模式排在第幾格。
pub fn index_of(w: Width) -> usize {
    OPTIONS.iter().position(|&o| o == w).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 語言模式四格的順序固定() {
        // 自動排第一（那是預設），其餘照語言辨識瀑布的順序
        assert_eq!(lang_index_in(&LANG_OPTIONS, None), 0);
        assert_eq!(lang_index_in(&LANG_OPTIONS, Some(Language::Bopomofo)), 1);
        assert_eq!(lang_index_in(&LANG_OPTIONS, Some(Language::Romaji)), 2);
        assert_eq!(lang_index_in(&LANG_OPTIONS, Some(Language::English)), 3);
    }

    #[test]
    fn 語言標籤都是單字() {
        // 跟全半形的「自半全」同一個風格
        for o in LANG_OPTIONS {
            assert_eq!(lang_symbol(o).chars().count(), 1, "標籤要單字：{o:?}");
        }
    }

    #[test]
    fn 兩種提示共用同一套動畫() {
        // 全半形三格、語言四格，但動畫本身跟格數無關——
        // 同樣從第 0 格滑到第 2 格，走過的比例要一樣
        let mut a = WidthBar::start_at(None, 0, 2);
        let mut b = WidthBar::start_at(None, 0, 2);
        a.started = Instant::now() - SLIDE / 2;
        b.started = a.started;
        assert!((a.visual_index() - b.visual_index()).abs() < 0.001);
    }

    #[test]
    fn 三個選項的順序固定() {
        assert_eq!(OPTIONS, [Width::Auto, Width::Half, Width::Full]);
        assert_eq!(index_of(Width::Auto), 0);
        assert_eq!(index_of(Width::Full), 2);
    }

    #[test]
    fn 一開始在起點() {
        let b = WidthBar::start_at(None, index_of(Width::Auto), index_of(Width::Half));
        // 剛開始時反白還在第 0 格附近
        assert!(b.visual_index() < 0.5, "剛開始不該已經滑到終點");
        assert_eq!(b.target(), 1);
    }

    #[test]
    fn 滑完之後停在終點() {
        let mut b = WidthBar::start_at(None, index_of(Width::Auto), index_of(Width::Full));
        // 把時間往前撥到滑動結束
        b.started = Instant::now() - SLIDE;
        assert!((b.visual_index() - 2.0).abs() < 0.01, "該停在第 2 格");
    }

    #[test]
    fn 停留期間不淡出() {
        let mut b = WidthBar::start_at(None, index_of(Width::Auto), index_of(Width::Half));
        b.release();
        b.started = Instant::now() - SLIDE - Duration::from_millis(100);
        assert_eq!(b.opacity(), 1.0, "還在停留期，不該開始淡");
        assert!(!b.done());
    }

    #[test]
    fn 淡出之後就結束() {
        let mut b = WidthBar::start_at(None, index_of(Width::Auto), index_of(Width::Half));
        b.release();
        b.started = Instant::now() - SLIDE - HOLD - FADE;
        assert_eq!(b.opacity(), 0.0);
        assert!(b.done(), "跑完該停掉計時器");
    }

    #[test]
    fn shift_按著就不淡出() {
        // 使用者可能還要再按空白切下一個模式，這時收掉提示
        // 會讓他看不到自己切到哪
        let mut b = WidthBar::start_at(None, index_of(Width::Auto), index_of(Width::Half));
        b.started = Instant::now() - SLIDE - HOLD - FADE * 3;
        assert_eq!(b.opacity(), 1.0, "按著就該一直亮著");
        assert!(!b.done(), "按著就不算跑完");

        // 放開才開始倒數
        b.release();
        assert_eq!(b.opacity(), 1.0, "剛放開還在停留期");
        assert!(!b.done());
    }

    #[test]
    fn 放開之後從停留期起算() {
        // 按住很久才放開，不該一放開就跳到淡出結束
        let mut b = WidthBar::start_at(None, index_of(Width::Auto), index_of(Width::Half));
        b.started = Instant::now() - Duration::from_secs(10);
        b.release();
        assert!(!b.done(), "放開的當下不該立刻結束");
        assert_eq!(b.opacity(), 1.0);
    }

    #[test]
    fn 連按兩次從目前位置接著滑() {
        // 使用者連按 Shift+空白，反白不該閃回起點
        let mut first = WidthBar::start_at(None, index_of(Width::Auto), index_of(Width::Full));
        // 滑到一半
        first.started = Instant::now() - SLIDE / 2;
        let mid = first.visual_index();
        assert!(mid > 0.0 && mid < 2.0, "中途應該在兩格之間：{mid}");

        let second = WidthBar::start_at(Some(&first), index_of(Width::Full), index_of(Width::Auto));
        assert_eq!(second.target(), 0);
        // 起點是接續中途的位置，不是硬跳回第 2 格
        assert!(
            second.from < 2.0,
            "該從中途位置 {} 接著走，不是從終點",
            second.from
        );
    }

    /// 這幾個測試盯的是同一件事：**關掉的語言不該留下死格子**，
    /// 而且反白要滑到正確的位置。
    mod 停用語言 {
        use super::*;
        use ime_core::config::Engines;

        fn 引擎(bopomofo: bool, romaji: bool) -> Engines {
            Engines { bopomofo, romaji }
        }

        #[test]
        fn 全開時四格都在() {
            let o = lang_options(&引擎(true, true));
            assert_eq!(o.len(), 4);
            assert_eq!(o, LANG_OPTIONS.to_vec());
        }

        #[test]
        fn 關掉日文只剩三格() {
            let o = lang_options(&引擎(true, false));
            assert_eq!(
                o,
                vec![None, Some(Language::Bopomofo), Some(Language::English)]
            );
        }

        #[test]
        fn 關掉注音只剩三格() {
            let o = lang_options(&引擎(false, true));
            assert_eq!(
                o,
                vec![None, Some(Language::Romaji), Some(Language::English)]
            );
        }

        #[test]
        fn 兩個都關還有自動與英文() {
            // 英文永遠開著——它是瀑布的最後一站，關不掉
            let o = lang_options(&引擎(false, false));
            assert_eq!(o, vec![None, Some(Language::English)]);
        }

        #[test]
        fn 關掉中間那格時索引要跟著挪() {
            // 這是最容易錯的情況：日文關掉後，英文從第 3 格變成第 2 格。
            // 用固定的 LANG_OPTIONS 算的話反白會滑到空的位置。
            let o = lang_options(&引擎(true, false));
            assert_eq!(lang_index_in(&o, None), 0);
            assert_eq!(lang_index_in(&o, Some(Language::Bopomofo)), 1);
            assert_eq!(lang_index_in(&o, Some(Language::English)), 2);
        }

        #[test]
        fn 停用的語言問索引不會爆() {
            // 理論上不會發生（輪替時就跳過了），但設定剛改完的那一瞬間
            // 可能還鎖在已停用的語言上——回退到第 0 格，不要 panic
            let o = lang_options(&引擎(true, false));
            assert_eq!(lang_index_in(&o, Some(Language::Romaji)), 0);
        }

        #[test]
        fn 開關來回都對() {
            // 切換前後兩個方向都要驗（見 CLAUDE.md 的測試注意事項）
            let 關 = lang_options(&引擎(true, false));
            let 開 = lang_options(&引擎(true, true));
            assert_eq!(關.len(), 3);
            assert_eq!(開.len(), 4);
            // 關掉再打開，英文要從第 2 格回到第 3 格
            assert_eq!(lang_index_in(&關, Some(Language::English)), 2);
            assert_eq!(lang_index_in(&開, Some(Language::English)), 3);
        }
    }
}
