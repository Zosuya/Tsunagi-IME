//! 「前面的字突然改了」有多常發生？
//!
//! # 為什麼需要這支
//!
//! 新酷音的使用者回報過：「連續打出一句話以後，前面的字突然改了」。
//! 我們的引擎也會——`compose::apply_word_context` 的設計就是「打
//! 『擬郝』選了『你』，『郝』跟著變『好』」，那是**意圖**不是 bug。
//!
//! 問題在於使用者已經滿意前面的字時它不該再動。`picked` 旗標只擋
//! **手動選過**的字，引擎自動給對、使用者也滿意的字仍然會被改掉。
//!
//! 那位使用者說「打過一次以後就無法重現了」——所以第一件事是**能量**。
//! 這支就是那把尺。
//!
//! # 怎麼量
//!
//! 每一句逐鍵餵進去，比較的是**格子**不是整串文字。
//!
//! 上一步「除了最後一格以外」的那些格，涵蓋了一段按鍵前綴 P。使用者
//! 繼續打字只會往後加，P 那段按鍵**再也沒被碰過**——所以它們對應的字
//! 不該變。這一步把涵蓋同一段 P 的格子接起來比對，不一樣就記一次。
//!
//! **兩件事要分開算**：
//!
//! | | 意思 | 算不算問題 |
//! |---|---|---|
//! | 內容改寫 | 同一段按鍵、同樣的分段，**字卻換了**（`好 策`→`好 測`） | **算** |
//! | 邊界移動 | 分段變了，對不齊 | 不算——累加式切法每打一鍵都在重算，那是本質 |
//!
//! **量錯過兩次**：第一版比整串文字的前綴，98.3% 的句子都「有改寫」，
//! 因為 `cl` →「好」這種**打到一半的正常轉換**（聲調還沒按）也算了進去；
//! 第二版把邊界移動也算成改寫，75.1%，同樣不是使用者抱怨的東西。
//!
//! ```text
//! 打到 vup g4      → 心事
//! 打到 vup g4ru4   → 新世紀     ← 「心事」整個被改掉，記一次
//! ```
//!
//! # 用法
//!
//! ```text
//! cargo run --release -p ime-core --bin check_rewrite
//! cargo run --release -p ime-core --bin check_rewrite -- <包名>   # 帶一個包當「學到的詞」
//! ```
//!
//! 帶包名是為了 spike：領域包已經是可寫的那一層，拿它模擬「學習之後」
//! 的狀態，不必先把學習寫出來就能量到副作用。

//! # 總次數不是那個要看的數字（2026-09-02）
//!
//! 實測 112 次改寫**全部**發生在游標旁邊 1～2 格，那正是「打『擬郝』
//! 到『你好』」的意圖行為。使用者抱怨的「打了很久的字突然變了」
//! 是**另一回事**，判準是**觸及範圍**：離游標 3 格以上才是病。
//!
//! 所以這支除了總次數，還報改寫的形狀。**遠距改寫那一格必須是 0**
//! ——它變成非 0 代表 `apply_word_context` 開始動使用者早就看過、
//! 也早就接受的字。

use ime_core::session::Session;

/// 改寫的**形狀**：每次改寫離游標多遠、被改掉的字撐了幾鍵。
///
/// 判準比總次數嚴——要求前一步的每一格**按鍵都原封不動還在原位**，
/// 分段一變就整步跳過。總次數那邊只對齊尾端那一刀，中間的邊界動了
/// 也算進去，會把意圖行為跟邊界移動混在一起。
#[derive(Default)]
struct Shape {
    /// 離游標幾格（1 = 緊鄰游標那格）
    backs: Vec<usize>,
    /// 被改掉的字撐了幾鍵才被改
    ages: Vec<usize>,
}

/// 上一步「除了最後一格」的那些格：(按鍵, 文字)
fn settled_slots(s: &Session) -> Vec<(String, String)> {
    let slots = s.slots();
    if slots.len() < 2 {
        return Vec::new();
    }
    slots[..slots.len() - 1]
        .iter()
        .map(|x| (x.keys.clone(), x.text.clone()))
        .collect()
}

/// 一句話量出來的東西。
struct Row {
    /// 按了幾鍵
    keys: usize,
    /// 幾次**內容改寫**（同範圍同分段，字不同）
    rewrites: usize,
    /// 幾次分段邊界移動（不算問題，只是報出來當背景）
    moved: usize,
    /// 第一次改寫的樣子（給人看的）
    sample: Option<(String, String)>,
}

/// 上一步「除了最後一格」的那些格：回傳 (涵蓋的按鍵長度, 接起來的文字)。
fn settled(s: &Session) -> (usize, String) {
    let slots = s.slots();
    if slots.len() < 2 {
        return (0, String::new());
    }
    let keep = &slots[..slots.len() - 1];
    (
        keep.iter().map(|x| x.keys.len()).sum(),
        keep.iter().map(|x| x.text.as_str()).collect(),
    )
}

/// 這一步涵蓋前 `n` 個按鍵的那些格，接起來的文字。
///
/// 對不齊（分段邊界移動了）就回 `None`——那本身就是一種改寫。
fn same_range(s: &Session, n: usize) -> Option<String> {
    let mut acc = 0usize;
    let mut out = String::new();
    for slot in s.slots() {
        if acc == n {
            return Some(out);
        }
        acc += slot.keys.len();
        if acc > n {
            return None;
        }
        out.push_str(&slot.text);
    }
    (acc == n).then_some(out)
}

fn measure(keys: &str, shape: &mut Shape) -> Row {
    let mut s = Session::new();
    let mut rewrites = 0usize;
    let mut moved = 0usize;
    let mut sample = None;
    let mut n = 0usize;
    let mut prev: Option<(usize, String)> = None;
    // 形狀那條走自己的嚴格對齊，跟上面的總次數各算各的
    let mut prev_slots: Vec<(String, String)> = Vec::new();
    // 每一格從第幾鍵起維持現在的文字
    let mut since: Vec<usize> = Vec::new();
    for c in keys.chars() {
        s.push(c);
        n += 1;
        let now_slots = settled_slots(&s);
        // 前一步的每一格都要按鍵相同且還在原位，否則這步是邊界移動
        let aligned = prev_slots.len() <= now_slots.len()
            && prev_slots
                .iter()
                .zip(now_slots.iter())
                .all(|(a, b)| a.0 == b.0);
        if aligned {
            for (i, (p, q)) in prev_slots.iter().zip(now_slots.iter()).enumerate() {
                if p.1 != q.1 {
                    shape.ages.push(n - since[i]);
                    shape.backs.push(now_slots.len() - i);
                }
            }
        }
        since = now_slots
            .iter()
            .enumerate()
            .map(|(i, (k, t))| {
                let held = aligned
                    && i < prev_slots.len()
                    && &prev_slots[i].0 == k
                    && &prev_slots[i].1 == t;
                if held {
                    since[i]
                } else {
                    n
                }
            })
            .collect();
        prev_slots = now_slots;
        if let Some((len, was)) = &prev {
            if *len > 0 {
                match same_range(&s, *len) {
                    // 對不齊＝分段邊界移動，不算問題
                    None => moved += 1,
                    Some(now) if now != *was => {
                        rewrites += 1;
                        if sample.is_none() {
                            sample = Some((was.clone(), now));
                        }
                    }
                    Some(_) => {}
                }
            }
        }
        prev = Some(settled(&s));
    }
    Row {
        keys: n,
        rewrites,
        moved,
        sample,
    }
}

/// 佔比，分母為 0 時回 0（不要在報表裡印 NaN）
fn pct(c: usize, total: usize) -> f64 {
    if total == 0 {
        return 0.0;
    }
    100.0 * c as f64 / total as f64
}

fn main() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data");
    let packs: Vec<String> = std::env::args().skip(1).collect();
    if !packs.is_empty() {
        let dir = data.join("packs");
        let n = ime_core::pack::load(&dir.to_string_lossy(), &packs);
        println!("模擬「學到的詞」：{n} 條\n");
    }
    ime_core::english::load(&data);
    ime_core::dict::load_bopomofo(&data);
    ime_core::dict::load_japanese(&data);

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let files = [
        "mixed_cutpoint",
        "mixed_daily",
        "mixed_trilingual",
        "bopomofo_sentences",
        "bopomofo_words",
    ];
    let mut total_keys = 0usize;
    let mut total_rewrites = 0usize;
    let mut total_moved = 0usize;
    let mut sentences = 0usize;
    let mut hit = 0usize;
    let mut samples: Vec<(String, String, String)> = Vec::new();
    let mut shape = Shape::default();

    for f in files {
        let Ok(content) = std::fs::read_to_string(dir.join(format!("{f}.txt"))) else {
            continue;
        };
        for line in content.lines() {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            // 三欄格式：文字期望 <TAB> 按鍵期望 <TAB> 按鍵序列
            let Some(keys) = line.split('\u{9}').nth(2) else {
                continue;
            };
            if keys.trim().is_empty() {
                continue;
            }
            let r = measure(keys, &mut shape);
            sentences += 1;
            total_keys += r.keys;
            total_rewrites += r.rewrites;
            total_moved += r.moved;
            if r.rewrites > 0 {
                hit += 1;
                if samples.len() < 12 {
                    if let Some((a, b)) = r.sample {
                        samples.push((keys.to_string(), a, b));
                    }
                }
            }
        }
    }

    println!("=== 「前面的字突然改了」有多常發生 ===\n");
    println!("  句數            {sentences}");
    println!("  總按鍵          {total_keys}");
    println!(
        "  發生改寫的句子  {hit}（{:.1}%）",
        100.0 * hit as f64 / sentences.max(1) as f64
    );
    println!(
        "  改寫次數        {total_rewrites}（每百鍵 {:.2} 次）",
        100.0 * total_rewrites as f64 / total_keys.max(1) as f64
    );
    println!("  （分段邊界移動  {total_moved} 次，不算問題，只是背景值）");

    // ── 形狀：意圖內的修正 vs 真正的病 ──
    let far = shape.backs.iter().filter(|&&b| b >= 3).count();
    let n = shape.backs.len();
    println!("\n=== 改寫的形狀（嚴格對齊，共 {n} 次）===\n");
    println!("  觸及範圍（1 = 緊鄰游標那格）");
    for b in 1..=2usize {
        let c = shape.backs.iter().filter(|&&x| x == b).count();
        println!("    倒數第 {b} 格      {c:>4}（{:>5.1}%）", pct(c, n));
    }
    println!(
        "    倒數第 3 格以上  {far:>4}（{:>5.1}%）  ← **必須是 0**",
        pct(far, n)
    );
    println!("\n  被改掉的字撐了幾鍵");
    for a in 1..=3usize {
        let c = shape.ages.iter().filter(|&&x| x == a).count();
        println!("    {a} 鍵            {c:>4}（{:>5.1}%）", pct(c, n));
    }
    let old_n = shape.ages.iter().filter(|&&x| x >= 4).count();
    println!("    4 鍵以上        {old_n:>4}（{:>5.1}%）", pct(old_n, n));
    if far == 0 {
        println!("\n  ✓ 沒有遠距改寫——改寫全部是「湊成詞、修正前一格」的意圖行為");
    } else {
        println!("\n  ⚠ 有 {far} 次改寫伸到游標 3 格以外，那是使用者會抱怨的那種");
    }

    if !samples.is_empty() {
        println!("\n=== 實例 ===\n");
        for (k, a, b) in &samples {
            println!("  {k}\n     {a}  →  {b}");
        }
    }
}
