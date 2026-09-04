//! 互動式切點引擎：在終端裡直接打字，**逐鍵**看結果。
//!
//! ## 為什麼需要它
//!
//! 以前驗證一個想法要：編譯 → 註冊（需提權）→ 切輸入法 → 開記事本
//! → 打字 → 切回來 → 關揉著 DLL 的行程（不然編譯會失敗）。
//! 這支工具把那整套縮成一行指令。
//!
//! ## 跟 `repl` 的差別
//!
//! `repl` 要按 Enter 才送出，這支是 raw mode **每打一鍵就重畫**——
//! 那才是輸入法真正的行為，也才看得出累加式切法的過程。
//!
//! ## 按鍵
//!
//! - 任何可打印字元：加入按鍵串（空白也算，它在注音是一聲）
//! - Backspace：刪一個；Ctrl+U：清空；Esc / Ctrl+C：離開
//!
//! 用法：`cargo run --release -p ime-core --features tui --bin try_ime`

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ime_core::cutpoint::incremental::Incremental;
use ime_core::cutpoint::{normalize, rank, Segment};
use std::io::Write;

/// 一種切法的顯示字串。
fn show(segs: &[Segment]) -> String {
    segs.iter()
        .map(|s| format!("{}:{}", s.lang.short(), s.keys.replace(' ', "␣")))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// raw mode 下 `\n` 不會回到行首，所以每行都要用 `\r\n` 收尾。
fn draw(keys: &str, inc: &Incremental) {
    print!("\x1b[2J\x1b[H");
    print!("─── 切點引擎  (Esc 離開，Ctrl+U 清空) ───\r\n\r\n");
    print!("  按鍵串：{}│\r\n\r\n", keys.replace(' ', "␣"));

    if keys.is_empty() {
        print!("  (開始打字… 例：su3cl3  sushi  check u vu84)\r\n");
        let _ = std::io::stdout().flush();
        return;
    }

    let cands = rank::sort(inc.cuttings());
    match cands.first() {
        Some(best) => print!("  切法 ▶ {}\r\n\r\n", show(&normalize(best))),
        None => print!("  (沒有合法切法)\r\n\r\n"),
    }

    // **顯示正規化後的切法並去重**。
    //
    // 同一個語言內部切不切不影響輸出，兩者送出的字一模一樣。
    // 不去重的話候選清單會被這種重複塞滿，看不出真正有幾種輸出。
    let mut seen = std::collections::HashSet::new();
    let uniq: Vec<Vec<Segment>> = cands
        .iter()
        .map(|c| normalize(c))
        .filter(|c| seen.insert(show(c)))
        .collect();

    print!(
        "  ── 相異輸出 {} 種（原始候選 {}），前 10 ──\r\n",
        uniq.len(),
        cands.len()
    );
    for (i, c) in uniq.iter().take(10).enumerate() {
        let mark = if i == 0 { "★" } else { " " };
        print!("   {mark}{:>2}. {}\r\n", i + 1, show(c));
    }
    let _ = std::io::stdout().flush();
}

fn main() -> std::io::Result<()> {
    println!("載入詞庫中…");
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data");
    ime_core::english::load(&data);
    ime_core::dict::load_bopomofo(&data);
    ime_core::dict::load_japanese(&data);

    // 帶引數時跑一次就結束（不進 raw mode），方便在沒有真終端的
    // 環境下驗證渲染邏輯，也方便寫進腳本。
    if let Some(arg) = std::env::args().nth(1) {
        let inc = Incremental::from_keys(&arg);
        draw(&arg, &inc);
        println!();
        return Ok(());
    }

    let mut keys = String::new();
    let mut inc = Incremental::new();
    enable_raw_mode()?;
    draw(&keys, &inc);

    loop {
        // 只接「按下」，不接「放開」。
        //
        // Windows 的 crossterm 會把 Press 與 Release **兩個事件都送上來**
        // （Unix 終端只有 Press）。若只比對鍵碼不看 `kind`，一個鍵就會
        // 被收兩次——實測打 `s` 會變成 `ss`。
        if let Event::Key(KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            ..
        }) = event::read()?
        {
            match code {
                KeyCode::Esc => break,
                KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => break,
                KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => {
                    keys.clear();
                    inc = Incremental::new();
                }
                KeyCode::Backspace => {
                    if keys.pop().is_some() {
                        // 累加式沒有退格，整串重建。
                        //
                        // 退一格就得丟掉所有分支重來——那些分支是一路
                        // 累積的，沒有「反向」的走法。重建的成本可接受：
                        // 按鍵串通常只有十幾個字元。
                        inc = Incremental::from_keys(&keys);
                    }
                }
                KeyCode::Char(c) => {
                    keys.push(c);
                    // **增量**：只算新增的那一鍵，跟產品行為一致。
                    inc.push(c);
                }
                _ => continue,
            }
            draw(&keys, &inc);
        }
    }

    disable_raw_mode()?;
    println!();
    Ok(())
}
