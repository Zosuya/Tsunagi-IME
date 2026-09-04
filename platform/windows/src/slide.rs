//! 候選視窗反白的滑動動畫。
//!
//! 兩處會滑，各用一個型別：
//!
//! ```text
//! ┌────────────────┐
//! │▶ 你▒▒好世界    │  ← 預覽列，按左右鍵換格（SpanSlide）
//! ├────────────────┤
//! │ 1 你           │
//! │▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒│  ← 候選清單，按上下鍵換列（Slide）
//! │ 3 尼           │
//! └────────────────┘
//! ```
//!
//! 這裡只算「反白現在該畫在哪」，不碰任何 Win32——跟 `width_bar.rs`
//! 一樣拆出來是為了能單元測試，計時器與重畫留在 `candidate_window.rs`。
//! 兩者共用同一條 ease-out 曲線與時長，手感才一致。
//!
//! # 為什麼候選清單是（欄, 列）兩個座標
//!
//! 一直排時只有列在動；展開全部的多欄網格裡，反白從一欄底部
//! 移到下一欄頂部時兩個座標都變，兩個一起內插才會斜著滑過去，
//! 而不是先橫再直。
//!
//! # 為什麼預覽列是（左緣, 右緣）
//!
//! 每一格的字寬不同，滑動時**寬度也要跟著變**。詳見 `SpanSlide`。

use std::time::{Duration, Instant};

/// 從一格滑到另一格要多久。跟全半形提示列一樣，視覺節奏才一致。
const SLIDE: Duration = Duration::from_millis(110);

/// ease-out 曲線：一開始快、接近終點慢下來。
///
/// 等速移動看起來很機械，這條曲線比較像真的東西在動。
/// **候選清單與預覽列共用**，兩處的動作手感才一致。
fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t) * (1.0 - t)
}

/// 進度：這一輪跑了多久（0.0 到 1.0）。
fn progress_of(started: Instant) -> f32 {
    (started.elapsed().as_secs_f32() / SLIDE.as_secs_f32()).clamp(0.0, 1.0)
}

/// 網格上的一格：（欄, 列）。
pub type Cell = (usize, usize);

/// 一段橫向區間：（左緣, 右緣），像素。
pub type Span = (i32, i32);

/// 預覽列反白塊的滑動。
///
/// 跟 `Slide` 的差別是**寬度也要跟著變**：候選清單每一列等高，
/// 只要滑「第幾列」；但預覽列每一格的字寬不同（「你」跟 `su3`
/// 差很多），左右緣得各自內插，滑動時反白塊才會一邊移動一邊
/// 伸縮成下一格的寬度。
#[derive(Debug, Clone)]
pub struct SpanSlide {
    from: (f32, f32),
    to: Span,
    started: Instant,
}

impl SpanSlide {
    /// 開始一輪：反白塊從 `from` 滑到 `to`。
    ///
    /// 上一輪還在滑的話從目前位置接著走，理由同 `Slide::start`。
    pub fn start(prev: Option<&Self>, from: Span, to: Span) -> Self {
        let from = match prev {
            Some(p) if !p.done() => p.position(),
            _ => (from.0 as f32, from.1 as f32),
        };
        Self {
            from,
            to,
            started: Instant::now(),
        }
    }

    /// 反白塊現在的左右緣（像素）。
    pub fn position(&self) -> (f32, f32) {
        let e = ease_out(progress_of(self.started));
        let lerp = |a: f32, b: i32| a + (b as f32 - a) * e;
        (lerp(self.from.0, self.to.0), lerp(self.from.1, self.to.1))
    }

    /// 滑完了嗎？
    pub fn done(&self) -> bool {
        self.started.elapsed() >= SLIDE
    }

    /// 終點那一段。
    #[allow(dead_code)] // 目前只有測試用得到
    pub fn target(&self) -> Span {
        self.to
    }
}

/// 單一數值的滑動。**捲軸滑塊用**。
///
/// 跟 `Slide`、`SpanSlide` 共用同一條曲線與時長，手感才一致。
/// 存的是 0.0～1.0 的**比例**而不是像素——視窗寬度會變（候選字寬不同），
/// 存像素的話重畫時就對不上了。
#[derive(Debug, Clone)]
pub struct ValueSlide {
    from: f32,
    to: f32,
    started: Instant,
}

impl ValueSlide {
    /// 開始一輪：從目前位置滑到 `to`。
    ///
    /// 上一輪還在滑就從它現在的位置接著走——拖捲軸時每動一下都會
    /// 呼叫這裡，硬跳回起點會抖。
    pub fn start(prev: Option<&Self>, from: f32, to: f32) -> Self {
        let from = match prev {
            Some(p) if !p.done() => p.value(),
            _ => from,
        };
        Self {
            from,
            to,
            started: Instant::now(),
        }
    }

    /// 現在的值。
    pub fn value(&self) -> f32 {
        let e = ease_out(progress_of(self.started));
        self.from + (self.to - self.from) * e
    }

    /// 滑完了嗎？
    pub fn done(&self) -> bool {
        self.started.elapsed() >= SLIDE
    }

    /// 終點。
    pub fn target(&self) -> f32 {
        self.to
    }
}

/// 一輪滑動。
#[derive(Debug, Clone)]
pub struct Slide {
    /// 起點。**用小數**——連按時要從中途的實際位置接著走，
    /// 取整數會讓反白跳一下。
    from: (f32, f32),
    /// 終點。
    to: Cell,
    /// 這一輪什麼時候開始。
    started: Instant,
}

impl Slide {
    /// 開始一輪：反白從 `from` 滑到 `to`。
    ///
    /// 上一輪還在滑的話，從**它目前的位置**接著走，不是硬跳回
    /// `from`——使用者快速連按空白時才不會閃。
    pub fn start(prev: Option<&Self>, from: Cell, to: Cell) -> Self {
        let from = match prev {
            Some(p) if !p.done() => p.position(),
            _ => (from.0 as f32, from.1 as f32),
        };
        Self {
            from,
            to,
            started: Instant::now(),
        }
    }

    /// 進度，0.0 到 1.0。
    fn progress(&self) -> f32 {
        progress_of(self.started)
    }

    /// 反白條現在畫在哪（欄, 列），可以是小數。
    pub fn position(&self) -> (f32, f32) {
        let e = ease_out(self.progress());
        let lerp = |a: f32, b: usize| a + (b as f32 - a) * e;
        (lerp(self.from.0, self.to.0), lerp(self.from.1, self.to.1))
    }

    /// 滑完了嗎？滑完就可以停掉計時器，之後照一般方式畫在終點。
    pub fn done(&self) -> bool {
        self.started.elapsed() >= SLIDE
    }

    /// 終點那一格。
    #[allow(dead_code)]
    pub fn target(&self) -> Cell {
        self.to
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 一開始在起點() {
        let s = Slide::start(None, (0, 1), (0, 4));
        let (_, row) = s.position();
        assert!(row < 1.5, "剛開始不該已經滑走：{row}");
        assert!(!s.done());
    }

    #[test]
    fn 滑完停在終點() {
        let mut s = Slide::start(None, (0, 1), (0, 4));
        s.started = Instant::now() - SLIDE;
        let (col, row) = s.position();
        assert!((row - 4.0).abs() < 0.01, "該停在第 4 列：{row}");
        assert!(col.abs() < 0.01);
        assert!(s.done(), "跑完該停掉計時器");
    }

    #[test]
    fn 跨欄時兩個座標一起動() {
        // 多欄網格：從第 0 欄底部滑到第 1 欄頂部
        let mut s = Slide::start(None, (0, 9), (1, 0));
        s.started = Instant::now() - SLIDE / 2;
        let (col, row) = s.position();
        assert!(col > 0.0 && col < 1.0, "欄該在途中：{col}");
        assert!(row > 0.0 && row < 9.0, "列該在途中：{row}");
    }

    #[test]
    fn 連按從目前位置接著滑() {
        let mut first = Slide::start(None, (0, 0), (0, 4));
        first.started = Instant::now() - SLIDE / 2;
        let (_, mid) = first.position();
        assert!(mid > 0.0 && mid < 4.0, "中途該在兩格之間：{mid}");

        // 還沒滑完就又按了一次，目標改成第 5 列
        let second = Slide::start(Some(&first), (0, 4), (0, 5));
        assert_eq!(second.target(), (0, 5));
        assert!(
            second.from.1 < 4.0,
            "該從中途位置 {} 接著走，不是從第 4 列",
            second.from.1
        );
    }

    #[test]
    fn 上一輪已結束就從指定起點走() {
        let mut first = Slide::start(None, (0, 0), (0, 4));
        first.started = Instant::now() - SLIDE * 2;
        let second = Slide::start(Some(&first), (0, 4), (0, 2));
        assert!((second.from.1 - 4.0).abs() < 0.01);
    }

    #[test]
    fn 預覽列反白塊的寬度會跟著變() {
        // 從一個窄的格子（10..30，寬 20）滑到寬的（30..90，寬 60）
        let mut s = SpanSlide::start(None, (10, 30), (30, 90));
        s.started = Instant::now() - SLIDE / 2;
        let (l, r) = s.position();
        let w = r - l;
        assert!(w > 20.0 && w < 60.0, "中途寬度該在兩者之間：{w}");
        assert!(l > 10.0 && l < 30.0, "左緣該在途中：{l}");
    }

    #[test]
    fn 預覽列滑完停在終點() {
        let mut s = SpanSlide::start(None, (10, 30), (30, 90));
        s.started = Instant::now() - SLIDE;
        let (l, r) = s.position();
        assert!((l - 30.0).abs() < 0.01, "左緣該到 30：{l}");
        assert!((r - 90.0).abs() < 0.01, "右緣該到 90：{r}");
        assert!(s.done());
        assert_eq!(s.target(), (30, 90));
    }

    #[test]
    fn 預覽列連按從目前位置接著滑() {
        let mut first = SpanSlide::start(None, (0, 20), (40, 60));
        first.started = Instant::now() - SLIDE / 2;
        let (mid_l, _) = first.position();
        assert!(mid_l > 0.0 && mid_l < 40.0, "中途該在兩格之間：{mid_l}");

        let second = SpanSlide::start(Some(&first), (40, 60), (60, 80));
        assert!(
            second.from.0 < 40.0,
            "該從中途位置 {} 接著走，不是從 40",
            second.from.0
        );
    }

    #[test]
    fn 兩種滑動共用同一條曲線() {
        // 手感要一致：同樣的進度下走過的比例要相同
        let mut cell = Slide::start(None, (0, 0), (0, 100));
        let mut span = SpanSlide::start(None, (0, 0), (100, 100));
        let t = SLIDE / 3;
        cell.started = Instant::now() - t;
        span.started = Instant::now() - t;
        let (_, cell_row) = cell.position();
        let (span_l, _) = span.position();
        assert!(
            (cell_row - span_l).abs() < 1.0,
            "兩者走過的比例該一致：{cell_row} vs {span_l}"
        );
    }

    #[test]
    fn 數值滑動從中途接著走() {
        let mut first = ValueSlide::start(None, 0.0, 1.0);
        first.started = Instant::now() - SLIDE / 2;
        let mid = first.value();
        assert!(mid > 0.0 && mid < 1.0, "中途該在兩端之間：{mid}");

        let second = ValueSlide::start(Some(&first), 0.0, 0.5);
        assert!(second.from > 0.0, "該從中途 {} 接著走", second.from);
        assert_eq!(second.target(), 0.5);
    }

    #[test]
    fn 數值滑動跟反白同一條曲線() {
        let mut cell = Slide::start(None, (0, 0), (0, 100));
        let mut val = ValueSlide::start(None, 0.0, 100.0);
        let t = SLIDE / 3;
        cell.started = Instant::now() - t;
        val.started = Instant::now() - t;
        let (_, cell_row) = cell.position();
        assert!(
            (cell_row - val.value()).abs() < 1.0,
            "走過的比例該一致：{cell_row} vs {}",
            val.value()
        );
    }

    #[test]
    fn 越接近終點越慢() {
        // ease-out：前半段走的距離要比後半段多
        let mut s = Slide::start(None, (0, 0), (0, 10));
        s.started = Instant::now() - SLIDE / 2;
        let (_, half) = s.position();
        assert!(half > 5.0, "前半段該走超過一半：{half}");
    }
}
