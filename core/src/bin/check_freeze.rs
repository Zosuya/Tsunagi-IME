//! 切點事後會不會被推翻？——凍結安不安全的那把尺。
//!
//! # 為什麼需要這支
//!
//! 混語言長句撞 `ALIVE_LIMIT`（400）之後，被丟掉的正好是正解——因為
//! 截斷時保留的是**切點少**的切法，而混語言正解切點最多。實測
//! 「英文＋中文交錯三次、41 鍵」開始，正解連候選都進不去。
//!
//! 提案的解法是**分區**：打到某個長度之後，把前面依當下的切法凍住，
//! 後面另起一區重新累加，兩區各自能用 Tab 選切法。
//!
//! 但舊架構為此栽過（死路清單那條「凍結已確定段落」）：判準是
//! 「含聲調鍵就凍」，`rup␣`（今）打完就凍，詞格再也看不到整串是
//! 「今天」，出來是「金天」。**音節收尾不代表它不會跟後面的字組成詞。**
//!
//! 所以動手之前要先回答一個問題：**引擎現在算出來的切點，會不會被
//! 後面打的字推翻？** 會的話，凍結就是把錯的邊界定死。
//!
//! # 怎麼量
//!
//! 每一句逐鍵餵進去，每一步取**第一名切法**的切點集合，跟打完整串的
//! 最終答案比。凍結的語意是「位置 `p = k - D` 以前的切點定死，`p`
//! 以後照樣重算」——所以**只比 `[0, p]` 這個區間**。
//!
//! ```text
//! 打到第 20 鍵，門檻 D=8 → 凍結點 p=12
//!   當下 [0,12] 內的切點  {4, 9}
//!   最終 [0,12] 內的切點  {4, 9}   → 一致，凍在這裡是安全的
//! ```
//!
//! **只比凍結區是關鍵**。純注音長句的最終答案是「整串一段」（切點集合
//! 是空的），但打到一半聲調還沒按時，引擎會暫時把它切成別的語言
//! （`ji3` ＋ `英:a`），一按聲調就併回去。拿「最靠左的分歧」當指標的話，
//! 這種暫時分歧會量出接近整串長度的距離，看起來像大災難——實際上那些
//! 分歧全都在游標旁邊，凍結點根本碰不到。第一版就是這樣量錯的。
//!
//! # 為什麼比「最終答案」而不是比正解
//!
//! 這支量的是**穩定性**不是正確性：問的是「凍結會不會改變引擎最後
//! 給的答案」。引擎本來就答錯的句子（撞 `ALIVE_LIMIT` 那些）不該算
//! 進凍結的帳上——那是另一個病，`check_incremental` 在管。
//!
//! # 用法
//!
//! ```text
//! cargo run --release -p ime-core --bin check_freeze
//! ```

use ime_core::cutpoint::{incremental::Incremental, normalize, rank, Segment};

#[path = "common/testdata.rs"]
mod testdata;

/// 要掃的門檻：凍結點離游標幾個按鍵。
const THRESHOLDS: [usize; 10] = [4, 6, 8, 10, 12, 16, 20, 24, 30, 40];

/// 一種切法的切點位置集合。
///
/// 段的長度累加就是切點——最後一段的結尾不算（那是字串結尾，不是切點）。
fn cuts_of(segs: &[Segment]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut acc = 0usize;
    for s in &segs[..segs.len().saturating_sub(1)] {
        acc += s.keys.chars().count();
        out.push(acc);
    }
    out
}

/// 這一步的第一名切法的切點。
///
/// **一定要先 `normalize`**。原始切法裡有大量「同語言內部」的切點
/// （`注:ji3 | 注:cjo4` 這種），它們不影響輸出——CLAUDE.md 記的
/// 「輸出等價，比對前要 normalize」就是這件事。拿原始切點來量的話，
/// 這些不影響任何東西的雜訊會被算成「凍錯」：第一版 D=4 量出 43.9%
/// 的句子有問題，絕大多數是假的。凍結要凍的也是 normalize 之後的
/// 邊界——那才是使用者看得到的分段。
fn best_cuts(inc: &Incremental) -> Vec<usize> {
    let cands = rank::sort(inc.cuttings());
    cands
        .first()
        .map(|c| cuts_of(&normalize(c)))
        .unwrap_or_default()
}

/// 凍結區裡的切點對不對？
fn frozen_wrong(now: &[usize], fin: &[usize], k: usize, d: usize) -> bool {
    let p = k.saturating_sub(d);
    if p == 0 {
        return false;
    }
    let a: Vec<usize> = now.iter().copied().filter(|&x| x <= p).collect();
    let b: Vec<usize> = fin.iter().copied().filter(|&x| x <= p).collect();
    a != b
}

/// 一句量出來的東西。
struct Row {
    keys: usize,
    /// 每個門檻各凍錯了幾步
    wrong: [usize; THRESHOLDS.len()],
}

fn measure(keys: &str) -> Row {
    let chars: Vec<char> = keys.chars().collect();
    // 先跑完整串拿最終答案
    let mut inc = Incremental::new();
    for &c in &chars {
        inc.push(c);
    }
    let fin = best_cuts(&inc);

    // 再逐鍵重跑一次，每一步問「凍在游標後 D 鍵的話對不對」
    let mut inc = Incremental::new();
    let mut wrong = [0usize; THRESHOLDS.len()];
    for (i, &c) in chars.iter().enumerate() {
        inc.push(c);
        let k = i + 1;
        let now = best_cuts(&inc);
        for (j, &d) in THRESHOLDS.iter().enumerate() {
            if frozen_wrong(&now, &fin, k, d) {
                wrong[j] += 1;
            }
        }
    }
    Row {
        keys: chars.len(),
        wrong,
    }
}

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
    ime_core::english::load(&data);
    ime_core::dict::load_bopomofo(&data);
    // 選字的中文 bigram——計分器一定要載，不然量到的是「關掉模型」的行為
    ime_core::lm::load(&data, ime_core::dict::char_freq_map(&data));
    ime_core::dict::load_japanese(&data);
    // 沒有詞庫就不要出數字——這一支跟漏斗一樣，缺資料照樣跑得完
    testdata::require_dicts(&data);

    // 帶檔案路徑就只跑那些——用來餵測資以外的壓力句（`testdata/` 最長
    // 只有 50 鍵，而且沒有中英交錯三次以上的長句，跟當初定 `ALIVE_LIMIT`
    // 時的盲點是同一個）
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--bench") {
        bench_partition();
        return;
    }
    if args.iter().any(|a| a == "--ja") {
        ja_boundaries();
        return;
    }
    if args.iter().any(|a| a == "--segments") {
        let extra: Vec<String> = args
            .iter()
            .filter(|a| !a.starts_with("--"))
            .cloned()
            .collect();
        segment_lengths(&extra);
        return;
    }
    let extra: Vec<String> = args;

    let mut sentences = 0usize;
    let mut steps = 0usize;
    // 每個門檻：凍錯的步數、凍錯的句數
    let mut wrong_steps = [0usize; THRESHOLDS.len()];
    let mut wrong_sentences = [0usize; THRESHOLDS.len()];
    // D=12 時凍錯的句子，給人看
    let mut worst: Vec<(usize, usize, String)> = Vec::new();
    let rep = THRESHOLDS.iter().position(|&d| d == 12).unwrap();

    // 這支只要按鍵欄。預設走共用載入器；`extra` 那條路留給臨時的外部檔。
    let all_keys: Vec<String> = if extra.is_empty() {
        testdata::load("check_freeze")
            .into_iter()
            .map(|r| r.keys)
            .collect()
    } else {
        extra
            .iter()
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .flat_map(|c| {
                c.lines()
                    .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
                    .filter_map(|l| l.split(testdata::TAB).nth(2).map(String::from))
                    .collect::<Vec<_>>()
            })
            .collect()
    };

    {
        for keys in all_keys.iter().map(String::as_str) {
            if keys.trim().is_empty() {
                continue;
            }
            let r = measure(keys);
            sentences += 1;
            steps += r.keys;
            for j in 0..THRESHOLDS.len() {
                wrong_steps[j] += r.wrong[j];
                if r.wrong[j] > 0 {
                    wrong_sentences[j] += 1;
                }
            }
            if r.wrong[rep] > 0 {
                worst.push((r.wrong[rep], r.keys, keys.to_string()));
            }
        }
    }

    println!("=== 凍結安不安全：凍在游標後 D 鍵，前面的切點會不會被推翻 ===\n");
    println!("  句數      {sentences}");
    println!("  總步數    {steps}（每一鍵算一步）\n");
    println!("     D   凍錯步數    佔比     凍錯句數    佔比");
    for (j, &d) in THRESHOLDS.iter().enumerate() {
        println!(
            "    {d:>2}   {:>8}   {:>5.2}%   {:>8}   {:>5.1}%",
            wrong_steps[j],
            pct(wrong_steps[j], steps),
            wrong_sentences[j],
            pct(wrong_sentences[j], sentences)
        );
    }

    worst.sort_by_key(|x| std::cmp::Reverse(x.0));
    println!("\n=== D=12 時凍錯最多的 10 句 ===\n");
    for (w, k, keys) in worst.iter().take(10) {
        let show: String = keys.chars().take(48).collect();
        println!("    凍錯 {w:>3} 步（{k:>3} 鍵）  {show}");
    }
}

/// 分區對效能有沒有用？——`--bench` 模式。
///
/// 日文長句的每鍵耗時是**隨長度線性成長**的：`alive` 早就撞滿 400，
/// 但每條切法的**段數**還在長，而排序要對每一段查詞典。所以成本大約是
/// 「400 條 × 段數 × 查詢」，長度一長就線性爬。
///
/// 分區把有效長度壓在門檻以內，成本理論上會從「隨長度成長」變成
/// 「有上限」。這一段就是量那件事：同一句話，整串一路打 vs 每 `T` 鍵
/// 重開一個 `Incremental`（模擬凍結前區、只重算後區），比最慢一鍵。
///
/// 模擬是簡化的——真正的分區還要保留前區的切法給 Tab 選。但**效能的
/// 成本全在後區的長度**，這一點是等價的。
fn bench_partition() {
    use std::time::Instant;

    // 取自 bench_typing 的長句案例
    const CASES: [(&str, &str); 3] = [
        (
            "日文 47 鍵",
            "maikaikareanishigotowooshitsukerareteirukigasuru",
        ),
        (
            "日文 71 鍵",
            "maikaikareanishigotowooshitsukerareteirukigasurushikatanaiiitsumokoudesu",
        ),
        (
            "日文 110 鍵",
            "maikaikareanishigotowooshitsukerareteirukigasurushikatanaiiitsumokoudesukaishanishucchousaseraretemattakuyaruki",
        ),
    ];

    /// 逐鍵打完，回最慢一鍵的微秒數。`part` 是分區門檻（0 = 不分區）。
    fn worst_key_us(keys: &str, part: usize) -> u128 {
        let mut inc = Incremental::new();
        let mut since = 0usize;
        let mut worst = 0u128;
        for c in keys.chars() {
            // 到門檻就重開一個——等同「前面凍住，只重算後區」
            if part > 0 && since >= part {
                inc = Incremental::new();
                since = 0;
            }
            let t = Instant::now();
            inc.push(c);
            let _ = rank::sort(inc.cuttings());
            worst = worst.max(t.elapsed().as_micros());
            since += 1;
        }
        worst
    }

    println!("=== 分區對每鍵耗時的影響（最慢一鍵）===\n");
    println!("  案例              不分區    每 40 鍵一區    每 30 鍵一區");
    for (name, keys) in CASES {
        let a = worst_key_us(keys, 0);
        let b = worst_key_us(keys, 40);
        let c = worst_key_us(keys, 30);
        println!(
            "  {name:<16} {:>6.1}ms      {:>6.1}ms       {:>6.1}ms",
            a as f64 / 1000.0,
            b as f64 / 1000.0,
            c as f64 / 1000.0
        );
    }
    println!("\n  一幀預算 16ms。");
}

/// 高信心邊界能把句子切多碎？——`--segments` 模式。
///
/// 使用者的設計是「引擎用高信心邊界分段，Tab 的選單按段組織」。那麼
/// 一個關鍵問題是：**段夠不夠短，短到段內不會撞 `ALIVE_LIMIT`？**
///
/// 這裡只用兩種**不需要先有切法就判斷得出來**的邊界：
///
/// - **標點**（`punct::is_punct`，§4.37 實測凍在這裡永遠不會凍錯）
/// - **真分隔符空白**——注音的一聲也是空白（`rup␣wu0␣` 今天），所以
///   要先問 `space::tone_suffix_start`：收得了前面音節的尾就不是邊界。
///   這一條沒做對的話，「今天」會被切成「今」「天」，正好是舊架構
///   那個「金天」的病。
///
/// 語言切換點也算高信心邊界，但那要先有切法才知道，這裡不算——所以
/// 量出來的段長是**保守的上界**，實際會更短。
fn segment_lengths(extra: &[String]) {
    use ime_core::cutpoint::{punct, space};

    /// 用高信心邊界把按鍵串切段，回每段長度。
    fn split(keys: &str) -> Vec<usize> {
        let chars: Vec<char> = keys.chars().collect();
        let mut out = Vec::new();
        let mut cur = String::new();
        for (i, &c) in chars.iter().enumerate() {
            if c == ' ' {
                // 這個空白是聲調嗎？是的話不算邊界
                if !cur.is_empty() && space::tone_suffix_start(&cur).is_some() {
                    cur.push(' ');
                    continue;
                }
                if !cur.is_empty() {
                    out.push(cur.chars().count());
                    cur.clear();
                }
            } else if punct::is_punct(keys, i) {
                if !cur.is_empty() {
                    out.push(cur.chars().count());
                    cur.clear();
                }
            } else {
                cur.push(c);
            }
        }
        if !cur.is_empty() {
            out.push(cur.chars().count());
        }
        out
    }

    // 這支只要按鍵欄。預設走共用載入器；`extra` 那條路留給臨時的外部檔。
    let all_keys: Vec<String> = if extra.is_empty() {
        testdata::load("check_freeze")
            .into_iter()
            .map(|r| r.keys)
            .collect()
    } else {
        extra
            .iter()
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .flat_map(|c| {
                c.lines()
                    .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
                    .filter_map(|l| l.split(testdata::TAB).nth(2).map(String::from))
                    .collect::<Vec<_>>()
            })
            .collect()
    };

    let mut lens: Vec<usize> = Vec::new();
    let mut longest: Vec<(usize, String)> = Vec::new();
    {
        for keys in all_keys.iter().map(String::as_str) {
            if keys.trim().is_empty() {
                continue;
            }
            let parts = split(keys);
            if let Some(&m) = parts.iter().max() {
                longest.push((m, keys.to_string()));
            }
            lens.extend(parts);
        }
    }

    lens.sort_unstable();
    let n = lens.len();
    println!("=== 高信心邊界切出來的段有多長 ===\n");
    println!("  段數      {n}");
    if n == 0 {
        return;
    }
    println!(
        "  段長：中位 {}、p95 {}、最大 {}",
        lens[n / 2],
        lens[(n * 95 / 100).min(n - 1)],
        lens[n - 1]
    );
    // 19 鍵是實測撞滿 400 的長度（§2.51.9）
    let risky = lens.iter().filter(|&&x| x >= 19).count();
    println!(
        "\n  ≥19 鍵的段（實測這個長度會撞滿 400）  {risky}（{:.1}%）",
        pct(risky, n)
    );

    longest.sort_by_key(|x| std::cmp::Reverse(x.0));
    println!("\n=== 最長的段來自哪些句子 ===\n");
    for (m, keys) in longest.iter().take(10) {
        let show: String = keys.chars().take(52).collect();
        println!("    最長段 {m:>3} 鍵   {show}");
    }
}

/// 日文的分詞邊界穩不穩？——`--ja` 模式。
///
/// # 為什麼問這個
///
/// 純日文長句沒有任何**字面**邊界（沒空白、沒標點、沒語言切換），所以
/// §2.51.13 那套分區對它完全不動手，效能問題留著。
///
/// 但日文其實有另一種邊界：**mozc 的分詞**（§2.23 的整句轉換用 Viterbi
/// 在「分詞×選字」的所有組合裡找總成本最低的路）。`ご飯|を|食べます`
/// 那些縫就是詞邊界，而且是**日文內部**的，不會出現第二版那種
/// `英:mott | 日:ohayaku` 的語言誤判。
///
/// 能不能拿來當凍結點，取決於**它穩不穩**：打到一半算出來的分詞，跟
/// 打完之後算的是不是同一個。這支就是量那件事，方法跟切點那邊一樣
/// ——只比「凍結區 `[0, p]`」，`p = 目前假名數 − D`。
///
/// 單位是**假名**不是按鍵：羅馬字跟假名不等比（CLAUDE.md 記過的坑），
/// 這裡量的是分詞本身穩不穩，用假名當單位才乾淨。
fn ja_boundaries() {
    use ime_core::romaji::{convert, kana};

    /// 一串假名的分詞邊界（累積假名數，不含結尾）。
    fn bounds(k: &str) -> Vec<usize> {
        let words = convert::convert(k);
        let mut out = Vec::new();
        let mut acc = 0usize;
        for w in &words[..words.len().saturating_sub(1)] {
            acc += w.kana.chars().count();
            out.push(acc);
        }
        out
    }

    // 額外補幾句 bench_typing 用的超長日文——測資裡最長也才 35 鍵
    let extra_cases = [
        "maikaikareanishigotowooshitsukerareteirukigasuru",
        "maikaikareanishigotowooshitsukerareteirukigasurushikatanaiiitsumokoudesu",
        "maikaikareanishigotowooshitsukerareteirukigasurushikatanaiiitsumokoudesukaishanishucchousaseraretemattakuyaruki",
    ];

    let mut keys_list: Vec<String> = extra_cases.iter().map(|s| s.to_string()).collect();
    for row in testdata::load("check_freeze") {
        let keys = row.keys.as_str();
        // 只要純日文的句子：整串都是小寫字母，而且轉得成假名
        if keys.len() < 8 || !keys.chars().all(|c| c.is_ascii_lowercase()) {
            continue;
        }
        if kana::to_kana(keys).is_some() {
            keys_list.push(keys.to_string());
        }
    }

    const SWEEP: [usize; 7] = [1, 2, 3, 4, 6, 8, 12];
    let mut wrong = [0usize; SWEEP.len()];
    let mut wrong_sent = [0usize; SWEEP.len()];
    let mut steps = 0usize;

    for keys in &keys_list {
        let Some(fin_kana) = kana::to_kana(keys) else {
            continue;
        };
        let fin = bounds(&fin_kana);
        let mut hit = [false; SWEEP.len()];
        let chars: Vec<char> = keys.chars().collect();
        for k in 1..=chars.len() {
            let sofar: String = chars[..k].iter().collect();
            let (now_kana, _) = kana::to_kana_partial(&sofar);
            if now_kana.is_empty() {
                continue;
            }
            let now = bounds(&now_kana);
            let len = now_kana.chars().count();
            steps += 1;
            for (j, &d) in SWEEP.iter().enumerate() {
                let p = len.saturating_sub(d);
                if p == 0 {
                    continue;
                }
                let a: Vec<usize> = now.iter().copied().filter(|&x| x <= p).collect();
                let b: Vec<usize> = fin.iter().copied().filter(|&x| x <= p).collect();
                if a != b {
                    wrong[j] += 1;
                    hit[j] = true;
                }
            }
        }
        for (j, h) in hit.iter().enumerate() {
            if *h {
                wrong_sent[j] += 1;
            }
        }
    }

    println!("=== 日文分詞邊界穩不穩（單位：假名）===\n");
    println!("  純日文句 {} 句、{} 步\n", keys_list.len(), steps);
    println!("   安全距離   凍錯步數    佔比     凍錯句數    佔比");
    for (j, &d) in SWEEP.iter().enumerate() {
        println!(
            "   {d:>6}   {:>8}   {:>5.2}%   {:>8}   {:>5.1}%",
            wrong[j],
            pct(wrong[j], steps),
            wrong_sent[j],
            pct(wrong_sent[j], keys_list.len())
        );
    }
}
