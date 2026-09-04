//! 把輸入法的**完整狀態**倒出來：組字區、切法選單、每一格的候選字。
//!
//! # 跟另外兩支的差別
//!
//! | 工具 | 看得到 | 互動 |
//! |---|---|---|
//! | `repl` | 只有切法與排序 | 要 |
//! | `try_ime` | 逐鍵重畫，最接近真實 | 要（raw mode，需要真終端機） |
//! | 這支 | 組字區＋切法＋**每一格的候選字** | **不要**，一行指令拿到全部 |
//!
//! 不互動是刻意的——遠端連線、CI、或是把結果貼給別人看的時候，
//! raw mode 的 TUI 用不了。
//!
//! # 用法
//!
//! ```text
//! cargo run --release -p ime-core --bin show_ime -- "su3cl3"
//! cargo run --release -p ime-core --bin show_ime -- -n 12 "vup g4ru4"
//! cargo run --release -p ime-core --bin show_ime -- -m 30 "youtubeao6g4"
//! cargo run --release -p ime-core --bin show_ime -- --step "su3cl3"
//! ```
//!
//! 注音的一聲是空白，所以按鍵串要用引號括起來。

use ime_core::compose;
use ime_core::session::Session;

/// 候選字預設列幾個。
const DEFAULT_N: usize = 10;

/// 切法選單預設列幾種。
///
/// 預設跟候選視窗一樣是 6，但**查排序問題時要能調大**——切法有生出來
/// 卻排在第 17 名，跟根本沒生出來，是兩種完全不同的病，看不到完整的
/// 選單就分不出來。
const DEFAULT_M: usize = 6;

fn dump(keys: &str, n: usize, m: usize) {
    let mut s = Session::new();
    for c in keys.chars() {
        s.push(c);
    }

    println!("━━━ {} ━━━", keys.replace(' ', "␣"));
    // `text()` 是組出來的文字；`composition_text()` 在自動模式**刻意**
    // 顯示原始按鍵（見 session::composition_text），兩個都印出來才看得
    // 出「宿主看到什麼」與「引擎組出什麼」的差別
    println!("  文字    {}", s.text());
    println!("  組字區  {}", s.composition_text());

    // 切法選單：使用者按 Tab 會看到的那份
    let menu = s.cutting_menu(m);
    if !menu.is_empty() {
        println!(
            "  切法    第 {}／共 {} 種",
            s.cutting_index() + 1,
            s.cutting_count()
        );
        for (i, m) in menu.iter().enumerate() {
            let mark = if i == s.cutting_index() { "▸" } else { " " };
            println!("          {mark} [{}] {m}", i + 1);
        }
    }

    // 每一格：選字時方向鍵移動的單位
    println!("  格子");
    for (i, slot) in s.slots().iter().enumerate() {
        let keys_shown = slot.keys.replace(' ', "␣");
        if !slot.selectable {
            println!(
                "    #{:<2} {:<10} {:<6} （不選字：{}）",
                i + 1,
                keys_shown,
                slot.text,
                if slot.lang == ime_core::language::Language::English {
                    "英文段"
                } else {
                    "標點或還在打"
                }
            );
            continue;
        }
        let cands = compose::candidates_for(slot);
        let head: Vec<&str> = cands.iter().take(n).map(String::as_str).collect();
        let more = if cands.len() > n {
            format!("…（共 {}）", cands.len())
        } else {
            String::new()
        };
        println!(
            "    #{:<2} {:<10} {:<6} {}{}",
            i + 1,
            keys_shown,
            slot.text,
            head.join(" "),
            more
        );
    }
    println!();
}

fn main() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data");
    // 領域包要**先於詞庫**載入——詞庫建表時會來拿，晚了就吃不進去。
    // 包名從設定檔讀（`[behavior] packs = [...]`）
    let mut n = DEFAULT_N;
    let mut m = DEFAULT_M;
    let mut step = false;
    let mut inputs: Vec<String> = Vec::new();
    // `--pack` 指定的包**取代**設定檔的清單，不是附加——測包的時候
    // 要能不受使用者現有設定干擾，也不必去改使用者的 config.toml
    let mut packs: Option<Vec<String>> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-n" => {
                n = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(DEFAULT_N)
            }
            "-m" => {
                m = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(DEFAULT_M)
            }
            "--step" => step = true,
            "--pack" => {
                if let Some(v) = args.next() {
                    packs.get_or_insert_with(Vec::new).push(v);
                }
            }
            _ => inputs.push(a),
        }
    }

    // 領域包要**先於詞庫**載入——詞庫建表時會來拿，晚了就吃不進去。
    // 沒給 `--pack` 就用設定檔的清單（`[behavior] packs = [...]`）
    let cfg = ime_core::config::Config::load(Some(&data));
    let enabled = packs.unwrap_or_else(|| cfg.behavior.packs.clone());
    ime_core::pack::load(&cfg.behavior.packs_dir, &enabled);
    ime_core::english::load(&data);
    ime_core::dict::load_bopomofo(&data);
    ime_core::dict::load_japanese(&data);
    // 整句轉換要用的接續矩陣。**這支要跟真正的引擎載一樣的東西**，
    // 不然量到的行為跟使用者看到的不一樣（這個坑踩過一次）。
    ime_core::dict::load_connection(&data);
    if inputs.is_empty() {
        eprintln!("用法：show_ime [-n 候選數] [-m 切法數] [--step] [--pack 包名] \"<按鍵串>\" …");
        eprintln!("注音的一聲是空白，按鍵串要用引號括起來。");
        return;
    }

    for keys in &inputs {
        if step {
            // 逐鍵展開——看得到「前面的字被後面改掉」那種行為
            let mut acc = String::new();
            for c in keys.chars() {
                acc.push(c);
                dump(&acc, n, m);
            }
        } else {
            dump(keys, n, m);
        }
    }
}
