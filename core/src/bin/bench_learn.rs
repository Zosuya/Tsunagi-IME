//! 學習到底有沒有用？收斂多快？**這是調 `GROWTH` 的那把尺。**
//!
//! # 怎麼模擬
//!
//! 一輪 ＝ 使用者把整批測資打過一次：
//!
//! 1. 逐鍵打完，比對引擎給的文字與期望
//! 2. 不一樣就**模擬使用者逐格改成正確的字**，並記進學習
//! 3. 下一輪再打一次同一批——這次引擎應該記得了
//!
//! 量的是每一輪的命中率。收斂太慢代表 `GROWTH` 太小，一輪就全中代表
//! 太大（那就是 libchewing 被抱怨的「選一次就跳第一」）。
//!
//! # 只模擬「選字錯」，不碰「切法錯」
//!
//! 引擎的字數跟期望對不上時代表**切法**就錯了，那是另一類問題
//! （見開發文件 §2.22.3 的兩類學習）。這支只處理字數對得上的，
//! 逐格比對、逐格修正。
//!
//! 用法：`cargo run --release -p ime-core --bin bench_learn`

use ime_core::session::Session;

/// 打幾輪。
const ROUNDS: usize = 4;

/// 讀測資：`文字期望 <TAB> 按鍵期望 <TAB> 按鍵序列`。
fn rows() -> Vec<(String, String)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    // **只用純中文那兩份**：混合語言的期望值有空白與寬容標記，
    // 逐格對齊會被那些格式差異卡住（第一版就是這樣，518 句裡只有
    // 12 句對得齊，等於沒量到東西）。選字學習的主場本來就是中文。
    let files = ["bopomofo_sentences", "bopomofo_words"];
    let mut out = Vec::new();
    for f in files {
        let Ok(content) = std::fs::read_to_string(dir.join(format!("{f}.txt"))) else {
            continue;
        };
        for line in content.lines() {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let mut it = line.split('\u{9}');
            let (Some(want), Some(_), Some(keys)) = (it.next(), it.next(), it.next()) else {
                continue;
            };
            // 期望欄的標記：`~` 是寬容空白、` || ` 是寬容列舉。
            // 這支只要一個明確的目標，取第一個選項、去掉標記
            let want = want.split(" || ").next().unwrap_or(want);
            let want = want.trim_start_matches('~').replace('_', " ");
            if keys.trim().is_empty() || want.is_empty() {
                continue;
            }
            out.push((want, keys.to_string()));
        }
    }
    out
}

/// 「前面的字突然改了」有多常發生。判準跟 `check_rewrite` 同一條：
/// **同一段按鍵、同樣的分段，字卻換了**。
///
/// 抽成函式是為了**前後各量一次**——只報學習後的數字沒有意義，
/// 沒有對照就不知道是變好還是變壞。
fn rewrite_rate(rows: &[(String, String)]) -> f64 {
    let mut rewrites = 0usize;
    let mut keys_total = 0usize;
    for (_, keys) in rows {
        let mut s = Session::new();
        let mut prev: (usize, String) = (0, String::new());
        for c in keys.chars() {
            s.push(c);
            keys_total += 1;
            if prev.0 > 0 {
                let mut acc = 0usize;
                let mut now = String::new();
                let mut ok = false;
                for sl in s.slots() {
                    if acc == prev.0 {
                        ok = true;
                        break;
                    }
                    acc += sl.keys.len();
                    if acc > prev.0 {
                        break;
                    }
                    now.push_str(&sl.text);
                }
                if (ok || acc == prev.0) && now != prev.1 {
                    rewrites += 1;
                }
            }
            let slots = s.slots();
            prev = if slots.len() < 2 {
                (0, String::new())
            } else {
                let keep = &slots[..slots.len() - 1];
                (
                    keep.iter().map(|x| x.keys.len()).sum(),
                    keep.iter().map(|x| x.text.as_str()).collect(),
                )
            };
        }
    }
    100.0 * rewrites as f64 / keys_total.max(1) as f64
}

fn typed(keys: &str) -> Session {
    let mut s = Session::new();
    for c in keys.chars() {
        s.push(c);
    }
    s
}

fn main() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data");
    ime_core::preload(&data, ime_core::config::Engines::default());
    let rows = rows();

    println!("=== 學習的收斂 ===\n");
    println!("  指數底數 GROWTH = {}", ime_core::learn::GROWTH);
    println!("  句數 {}\n", rows.len());
    // **學習前先量一次**，不然學習後的數字沒有對照
    let before_rw = rewrite_rate(&rows);
    println!("  輪次    命中          可修正    這輪記了幾條");

    for round in 1..=ROUNDS {
        let mut hit = 0usize;
        let mut fixable = 0usize;
        let mut learned = 0usize;

        for (want, keys) in &rows {
            let mut s = typed(keys);
            if s.text() == *want {
                hit += 1;
                continue;
            }
            // **字數對不上＝切法錯**，這支不處理
            let slots = s.slots().to_vec();
            let cur: String = slots.iter().map(|x| x.text.as_str()).collect();
            if cur.chars().count() != want.chars().count() {
                continue;
            }
            fixable += 1;

            // 逐格對齊：每格吃掉跟它現在一樣長的期望字元
            let wc: Vec<char> = want.chars().collect();
            let mut fixed = Vec::with_capacity(slots.len());
            let mut at = 0usize;
            for sl in &slots {
                let n = sl.text.chars().count();
                let text: String = wc[at..at + n].iter().collect();
                at += n;
                let changed = text != sl.text;
                fixed.push(ime_core::compose::Slot {
                    keys: sl.keys.clone(),
                    text,
                    lang: sl.lang,
                    selectable: sl.selectable,
                    // **只有真的改過的才算表態**——沒改的格子沒有新資訊
                    picked: changed && sl.selectable,
                });
            }
            learned += ime_core::learn::record(&fixed);
            let _ = &mut s;
        }

        println!(
            "  第 {round} 輪  {hit}/{} ({:.1}%)   {fixable}      {learned}",
            rows.len(),
            100.0 * hit as f64 / rows.len().max(1) as f64
        );
    }

    let idx = ime_core::learn::index();
    println!("\n  學習庫累積 {} 條", idx.len());

    // **學不會的那些是誰**——把它們印出來，不然只看到一個數字，
    // 不知道是「學習不夠好」還是「本來就不歸學習管」。
    let mut stuck = Vec::new();
    for (want, keys) in &rows {
        let s = typed(keys);
        if s.text() != *want {
            let cur: String = s.slots().iter().map(|x| x.text.as_str()).collect();
            let kind = if cur.chars().count() == want.chars().count() {
                "選字"
            } else {
                "切法"
            };
            stuck.push((kind, want.clone(), s.text()));
        }
    }
    if !stuck.is_empty() {
        println!(
            "
  學不會的 {} 句：",
            stuck.len()
        );
        for (kind, want, got) in &stuck {
            println!(
                "    [{kind}] 期 {want}
           實 {got}"
            );
        }
    }

    println!(
        "  改寫率 {:.2} → {:.2} 次/百鍵（學習前 → 學習後）",
        before_rw,
        rewrite_rate(&rows)
    );
}
