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
//! # `--revert`：反悔要幾次（§2.72）
//!
//! 上面那段只量**收斂速度**（幾輪之後命中），量不到 2026-09-09 回報的
//! 病：學會之後**再也翻不回來**。反悔那一段是專門量它的——
//! 先把 B 教到贏，再改選 A，量 A 要幾次才追得回來。
//!
//! **理想值是 1～2 次**，跟第一次學會的成本對稱；現況會隨著「教了幾次」
//! 一路變貴，那就是曲線後段沒有收斂。三條候選曲線各量一次。
//!
//! 用法：`cargo run --release -p ime-core --bin bench_learn`
//!       `cargo run --release -p ime-core --bin bench_learn -- --revert`

#[path = "common/testdata.rs"]
mod testdata;

use ime_core::session::Session;

/// 打幾輪。
const ROUNDS: usize = 4;

/// 讀測資：**中文選字的那兩節**。
///
/// 走共用的 `common/testdata.rs`，不再自己寫死檔名——測資 2026-09 合併
/// 成一份 `測資.txt` 時這支漏改了，`bopomofo_sentences.txt` 早就不在，
/// `rows()` 回空的，整支**印了一個月的 0**（沒有人看，因為 `k` 已經定案）。
///
/// **只取 `zh_words` 與 `zh_sentences`**：混合語言那幾節的期望值有空白
/// 與寬容標記，逐格對齊會被那些格式差異卡住（第一版就是這樣，518 句裡
/// 只有 12 句對得齊，等於沒量到東西）。選字學習的主場本來就是中文。
fn rows() -> Vec<(String, String)> {
    testdata::load("bench_learn")
        .into_iter()
        .filter(|r| r.tag == "zh_words" || r.tag == "zh_sentences")
        .filter_map(|r| {
            // 寬容-列舉取第一個選項，`~` 與 `|` 是標記不是內容
            let alt = r.alts().first().copied().unwrap_or(&r.want).to_string();
            let want = testdata::expected_text(alt.trim_start_matches('~'));
            (!want.is_empty() && !r.keys.trim().is_empty()).then_some((want, r.keys))
        })
        .collect()
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

    if std::env::args().any(|a| a == "--revert") {
        revert(&rows);
        return;
    }

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
                    is_mark: false,
                    cands: None,
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

// ────────────────────────────────────────────────────────────
// 反悔成本（§2.72）
// ────────────────────────────────────────────────────────────

/// 追不回來就停在這裡。**比任何合理值都大**——真的撞到上限就代表
/// 「學了就鎖死」，那正是要量的病。
const 反悔上限: u32 = 30;

/// 取樣幾組同音字對。夠多就穩，太多只是等。
const 取樣: usize = 400;

/// 記一次「使用者把這個音節改成這個字」。
///
/// **走真的 `learn::record`**，不自己戳索引——記法本身（一格一條、
/// 要不要扣分）正是受測的東西。記法用 `learn::set_demote` 切換。
fn 選一次(keys: &str, ch: &str) {
    let slot = ime_core::compose::Slot {
        keys: keys.to_string(),
        text: ch.to_string(),
        lang: ime_core::language::Language::Bopomofo,
        selectable: true,
        is_mark: false,
        cands: None,
        picked: true,
    };
    ime_core::learn::record(&[slot]);
}

/// 一直選同一個字，直到它變成第一名。回傳選了幾次；超過上限回 `None`。
fn 選到贏(keys: &str, ch: &str) -> Option<u32> {
    for n in 1..=反悔上限 {
        選一次(keys, ch);
        if ime_core::dict::best_char_for(keys) == Some(ch) {
            return Some(n);
        }
    }
    None
}

/// 找可以對打的同音字對：`(按鍵, 統計第一名, 挑戰者)`。
///
/// **在乾淨的學習狀態下取樣**——有學過的話 `chars_for` 的順序已經被
/// 動過，取出來的「第一名」就不是統計的第一名了。
fn 取樣同音字對(rows: &[(String, String)]) -> Vec<(String, String, String)> {
    ime_core::learn::clear();
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (_, keys) in rows {
        for sl in typed(keys).slots() {
            if sl.lang != ime_core::language::Language::Bopomofo
                || !sl.selectable
                || sl.keys.is_empty()
                || !seen.insert(sl.keys.clone())
            {
                continue;
            }
            // **A 要取引擎真的會輸出的那個字**，不是候選清單的第一個。
            // 兩者可能不一樣——清單的順序是「偏好表優先」，而引擎輸出的
            // 是分數最高的。拿錯的話「反悔」量到的是別的東西
            let Some(a) = ime_core::dict::best_char_for(&sl.keys) else {
                continue;
            };
            let cs = ime_core::dict::chars_for(&sl.keys);
            // 挑戰者取清單裡第一個不是 A 的；只有一個同音字就沒得對打
            if let Some(b) = cs.iter().find(|c| c.as_str() != a) {
                out.push((sl.keys.clone(), a.to_string(), b.clone()));
            }
            if out.len() >= 取樣 {
                return out;
            }
        }
    }
    out
}

/// 一條曲線在「教了 `n` 次」之後的反悔成本。
///
/// 回傳 `(平均, 最壞, 樣本數, 卡死幾組)`。**只算真的學會了的樣本**——
/// B 還沒贏的話「反悔」是 0 次，那不是成績是雜訊。
///
/// **卡死才是使用者回報的那件事**：撞到上限代表選 30 次都改不回來，
/// 平均值會被沒卡死的樣本稀釋掉，所以要分開報。
fn 反悔成本(pairs: &[(String, String, String)], n: u32) -> (f64, u32, usize, usize) {
    let (mut sum, mut worst, mut cnt, mut stuck) = (0u64, 0u32, 0usize, 0usize);
    for (keys, a, b) in pairs {
        ime_core::learn::clear();
        // 先把挑戰者 B 教 n 次
        for _ in 0..n {
            選一次(keys, b);
        }
        if ime_core::dict::best_char_for(keys) != Some(b.as_str()) {
            continue; // 教 n 次還沒贏，這一格還沒進入「學會」狀態
        }
        // 反悔：改選回原本的 A，要幾次？
        let cost = match 選到贏(keys, a) {
            Some(c) => c,
            None => {
                stuck += 1;
                if std::env::args().any(|x| x == "--why") {
                    eprintln!(
                        "    卡死 keys={keys} A={a} B={b} 現在第一名={:?} A的次數={} B的次數={}",
                        ime_core::dict::best_char_for(keys),
                        ime_core::learn::index().count(keys, a),
                        ime_core::learn::index().count(keys, b),
                    );
                }
                反悔上限
            }
        };
        sum += cost as u64;
        worst = worst.max(cost);
        cnt += 1;
    }
    (sum as f64 / cnt.max(1) as f64, worst, cnt, stuck)
}

/// 第一次學會要幾次（平均）與**學不會幾組**。
///
/// **反悔的對照組**——理想是兩者對稱。學不會那一欄是壓低封頂的代價：
/// 天花板一低，字頻差太多的字就永遠翻不上去（§2.72.3 的疑慮）。
fn 學會成本(pairs: &[(String, String, String)]) -> (f64, usize) {
    let (mut sum, mut cnt, mut never) = (0u64, 0usize, 0usize);
    for (keys, _, b) in pairs {
        ime_core::learn::clear();
        match 選到贏(keys, b) {
            Some(n) => {
                sum += n as u64;
                cnt += 1;
            }
            None => never += 1,
        }
    }
    (sum as f64 / cnt.max(1) as f64, never)
}

fn revert(rows: &[(String, String)]) {
    let pairs = 取樣同音字對(rows);
    println!("=== 反悔成本：學會之後還改得回來嗎（§2.72）===\n");
    println!(
        "  同音字對 {} 組，底數 GROWTH = {}",
        pairs.len(),
        ime_core::learn::GROWTH
    );
    println!("  「教 N 次」＝使用者選了挑戰者 N 次；欄位是**改回原字要選幾次**");
    println!("  理想：跟學會成本對稱，而且不隨 N 變貴。上限 {反悔上限} 次\n");

    let 記法們 = [
        ("只加分（現況）", false, 0u32),
        ("加減分（選了誰、別人扣 1）", true, 0),
        ("加減分＋次數封 4", true, 4),
    ];
    for (name, 減分, 上限) in 記法們 {
        ime_core::learn::set_demote(減分, 上限);
        println!("\n  ── 記法：{name} ──");
        一輪(&pairs);
    }
    ime_core::learn::set_curve(ime_core::learn::DEFAULT_CURVE);
    ime_core::learn::set_demote(true, 4);
    ime_core::learn::clear();

    println!("\n  「－」＝教這麼多次還沒學會，沒有反悔可言（曲線爬得慢，不是壞事）");
    println!("  「卡死」＝選滿 {反悔上限} 次仍改不回來的組數（取各 N 之中最多的那一欄）");
}

/// 一種記法之下，把所有曲線各量一遍。
fn 一輪(pairs: &[(String, String, String)]) {
    let 曲線們 = [
        ("Exp{cap:10} 現況", ime_core::learn::Curve::Exp { cap: 10 }),
        (
            "Exp{cap:3}  壓低封頂",
            ime_core::learn::Curve::Exp { cap: 3 },
        ),
        (
            "Exp{cap:4}  壓低封頂",
            ime_core::learn::Curve::Exp { cap: 4 },
        ),
        (
            "Saturate{4096} 飽和",
            ime_core::learn::Curve::Saturate { ceil: 4096 },
        ),
        (
            "Saturate{512}  飽和",
            ime_core::learn::Curve::Saturate { ceil: 512 },
        ),
        (
            "Rival{1050} 相對競爭",
            ime_core::learn::Curve::Rival { permille: 1050 },
        ),
    ];
    let ns = [1u32, 3, 5, 8, 12, 20, 30];

    print!("  曲線                    學會 學不會");
    for n in ns {
        print!("  教{n:>2}次");
    }
    println!("   最壞  卡死");

    for (name, c) in 曲線們 {
        ime_core::learn::set_curve(c);
        let (學會, 學不會) = 學會成本(pairs);
        print!("  {name:<22} {學會:>4.1} {學不會:>3}");
        let (mut worst, mut stuck_max, mut n_max) = (0u32, 0usize, 0usize);
        for n in ns {
            let (avg, w, cnt, stuck) = 反悔成本(pairs, n);
            worst = worst.max(w);
            if stuck > stuck_max {
                stuck_max = stuck;
            }
            n_max = n_max.max(cnt);
            if cnt == 0 {
                print!("     － ");
            } else {
                print!("  {avg:>5.1} ");
            }
        }
        println!("   {worst:>3}  {stuck_max:>3}/{n_max}");
    }
}
