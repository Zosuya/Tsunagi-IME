//! spike：韓文的組字過程會不會踩到「改寫」硬指標？
//!
//! # 要驗證什麼
//!
//! 漏斗有一條硬指標：**改寫伸到游標 3 格外的次數必須是 0**
//! （`check_all::rewrite_shape`）。它防的是「早就打完、看過的字突然變」。
//!
//! 韓文的組字**天生就在改前面的字**——「달」再打一個母音要拆成「다르」，
//! 那是韓文規則本身，不是引擎的判斷失誤。所以問題是：**這條規則會不會
//! 把韓文整個擋在門外？**
//!
//! 這支模擬두벌식的逐鍵組字，用**跟 `rewrite_shape` 一模一樣的判準**
//! （逐格對齊、倒數第 3 格以上才算）去量。
//!
//! # 為什麼要自己實作組字
//!
//! 韓文引擎還不存在，這支就是在做那個引擎的最小版本——只做「按鍵串
//! → 音節序列」，不碰詞庫、不碰選字。組不組得對可以肉眼驗證
//! （`dkssud` 要是 `안녕`）。
//!
//! # 用法
//!
//! ```text
//! cargo run --release -p ime-core --bin spike_hangul_rewrite
//! ```

/// 初聲 19 個，順序是 Unicode 的組合順序（不能改）。
const CHO: [&str; 19] = [
    "ㄱ", "ㄲ", "ㄴ", "ㄷ", "ㄸ", "ㄹ", "ㅁ", "ㅂ", "ㅃ", "ㅅ", "ㅆ", "ㅇ", "ㅈ", "ㅉ", "ㅊ", "ㅋ",
    "ㅌ", "ㅍ", "ㅎ",
];

/// 中聲 21 個。
const JUNG: [&str; 21] = [
    "ㅏ", "ㅐ", "ㅑ", "ㅒ", "ㅓ", "ㅔ", "ㅕ", "ㅖ", "ㅗ", "ㅘ", "ㅙ", "ㅚ", "ㅛ", "ㅜ", "ㅝ", "ㅞ",
    "ㅟ", "ㅠ", "ㅡ", "ㅢ", "ㅣ",
];

/// 終聲 28 個，第 0 個是「沒有終聲」。
const JONG: [&str; 28] = [
    "", "ㄱ", "ㄲ", "ㄳ", "ㄴ", "ㄵ", "ㄶ", "ㄷ", "ㄹ", "ㄺ", "ㄻ", "ㄼ", "ㄽ", "ㄾ", "ㄿ", "ㅀ",
    "ㅁ", "ㅂ", "ㅄ", "ㅅ", "ㅆ", "ㅇ", "ㅈ", "ㅊ", "ㅋ", "ㅌ", "ㅍ", "ㅎ",
];

/// 두벌식（Dubeolsik）：按鍵 → 字母。大寫是 Shift 的那排。
const KEYS: &[(char, &str)] = &[
    ('q', "ㅂ"),
    ('w', "ㅈ"),
    ('e', "ㄷ"),
    ('r', "ㄱ"),
    ('t', "ㅅ"),
    ('y', "ㅛ"),
    ('u', "ㅕ"),
    ('i', "ㅑ"),
    ('o', "ㅐ"),
    ('p', "ㅔ"),
    ('a', "ㅁ"),
    ('s', "ㄴ"),
    ('d', "ㅇ"),
    ('f', "ㄹ"),
    ('g', "ㅎ"),
    ('h', "ㅗ"),
    ('j', "ㅓ"),
    ('k', "ㅏ"),
    ('l', "ㅣ"),
    ('z', "ㅋ"),
    ('x', "ㅌ"),
    ('c', "ㅊ"),
    ('v', "ㅍ"),
    ('b', "ㅠ"),
    ('n', "ㅜ"),
    ('m', "ㅡ"),
    ('Q', "ㅃ"),
    ('W', "ㅉ"),
    ('E', "ㄸ"),
    ('R', "ㄲ"),
    ('T', "ㅆ"),
    ('O', "ㅒ"),
    ('P', "ㅖ"),
];

/// 兩個母音併成一個複合中聲（ㅗ＋ㅏ＝ㅘ）。
const JUNG_COMBO: &[(&str, &str, &str)] = &[
    ("ㅗ", "ㅏ", "ㅘ"),
    ("ㅗ", "ㅐ", "ㅙ"),
    ("ㅗ", "ㅣ", "ㅚ"),
    ("ㅜ", "ㅓ", "ㅝ"),
    ("ㅜ", "ㅔ", "ㅞ"),
    ("ㅜ", "ㅣ", "ㅟ"),
    ("ㅡ", "ㅣ", "ㅢ"),
];

/// 兩個子音併成一個複合終聲（ㄱ＋ㅅ＝ㄳ）。**拆得開**——下一鍵是
/// 母音時要把後半搬走（`앉` ＋ ㅏ → `안자`）。
const JONG_COMBO: &[(&str, &str, &str)] = &[
    ("ㄱ", "ㅅ", "ㄳ"),
    ("ㄴ", "ㅈ", "ㄵ"),
    ("ㄴ", "ㅎ", "ㄶ"),
    ("ㄹ", "ㄱ", "ㄺ"),
    ("ㄹ", "ㅁ", "ㄻ"),
    ("ㄹ", "ㅂ", "ㄼ"),
    ("ㄹ", "ㅅ", "ㄽ"),
    ("ㄹ", "ㅌ", "ㄾ"),
    ("ㄹ", "ㅍ", "ㄿ"),
    ("ㄹ", "ㅎ", "ㅀ"),
    ("ㅂ", "ㅅ", "ㅄ"),
];

/// 正在組的那個音節。空字串代表那一格還沒有東西。
#[derive(Default, Clone, PartialEq)]
struct Syl {
    cho: String,
    jung: String,
    jong: String,
}

impl Syl {
    fn is_empty(&self) -> bool {
        self.cho.is_empty() && self.jung.is_empty() && self.jong.is_empty()
    }

    /// 組成一個字。湊不齊完整音節時就把有的字母原樣列出來——
    /// 那正是使用者在組字區看到的（打 `ㄷ` 時畫面就是 `ㄷ`）。
    fn render(&self) -> String {
        if self.cho.is_empty() || self.jung.is_empty() {
            return format!("{}{}{}", self.cho, self.jung, self.jong);
        }
        let c = CHO.iter().position(|x| *x == self.cho).unwrap_or(0);
        let v = JUNG.iter().position(|x| *x == self.jung).unwrap_or(0);
        let t = JONG.iter().position(|x| *x == self.jong).unwrap_or(0);
        let code = 0xAC00 + (c * 21 + v) * 28 + t;
        char::from_u32(code as u32)
            .map(String::from)
            .unwrap_or_default()
    }
}

fn is_vowel(s: &str) -> bool {
    JUNG.contains(&s)
}

/// 逐鍵組字。回傳每按一鍵之後**整串音節**的樣子。
///
/// # 韓文組字的核心規則
///
/// 每一鍵都要重新判斷「這個字母屬於現在這個音節，還是要開新的」，
/// 而且**母音來的時候可能要把上一個音節的終聲搶走**（종성 이동）。
/// 那正是「改前面的字」發生的地方。
fn compose(keys: &str) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    // 已經定案的音節 ＋ 正在組的那一個
    let mut done: Vec<Syl> = Vec::new();
    let mut cur = Syl::default();

    for ch in keys.chars() {
        let Some(letter) = KEYS.iter().find(|(k, _)| *k == ch).map(|(_, v)| *v) else {
            continue;
        };

        if is_vowel(letter) {
            // ── 母音 ──
            if cur.jung.is_empty() && !cur.cho.is_empty() {
                // 初聲有了、還沒中聲 → 直接填進去
                cur.jung = letter.to_string();
            } else if !cur.jung.is_empty() && cur.jong.is_empty() {
                // 已經是完整的「初＋中」，看能不能併成複合母音
                match JUNG_COMBO
                    .iter()
                    .find(|(a, b, _)| *a == cur.jung && *b == letter)
                {
                    Some((_, _, combo)) => cur.jung = combo.to_string(),
                    None => {
                        // 併不了 → 這個母音自己開新音節（無初聲，用 ㅇ）
                        done.push(std::mem::take(&mut cur));
                        cur.cho = "ㅇ".into();
                        cur.jung = letter.to_string();
                    }
                }
            } else if !cur.jong.is_empty() {
                // **종성 이동**：終聲被搶走當下一個音節的初聲
                //
                // 這就是「改前面的字」——畫面上已經成形的「달」
                // 會變成「다」，同時多出一個「르」。
                let moved = match JONG_COMBO.iter().find(|(_, _, c)| *c == cur.jong) {
                    // 複合終聲只搬後半（`앉` ＋ ㅏ → `안자`）
                    Some((a, b, _)) => {
                        cur.jong = a.to_string();
                        b.to_string()
                    }
                    None => std::mem::take(&mut cur.jong),
                };
                done.push(std::mem::take(&mut cur));
                cur.cho = moved;
                cur.jung = letter.to_string();
            } else {
                // 空的 → 無初聲的音節
                cur.cho = "ㅇ".into();
                cur.jung = letter.to_string();
            }
        } else {
            // ── 子音 ──
            if cur.cho.is_empty() {
                cur.cho = letter.to_string();
            } else if cur.jung.is_empty() {
                // 初聲後面又來子音 → 前一個自己成立，這個開新的
                done.push(std::mem::take(&mut cur));
                cur.cho = letter.to_string();
            } else if cur.jong.is_empty() {
                // 有初有中 → 當終聲。**不是每個子音都能當終聲**
                if JONG.contains(&letter) {
                    cur.jong = letter.to_string();
                } else {
                    done.push(std::mem::take(&mut cur));
                    cur.cho = letter.to_string();
                }
            } else {
                // 終聲已經有了 → 試複合終聲，不行就開新音節
                match JONG_COMBO
                    .iter()
                    .find(|(a, b, _)| *a == cur.jong && *b == letter)
                {
                    Some((_, _, combo)) => cur.jong = combo.to_string(),
                    None => {
                        done.push(std::mem::take(&mut cur));
                        cur.cho = letter.to_string();
                    }
                }
            }
        }

        // 這一鍵之後畫面上的樣子
        let mut snap: Vec<String> = done.iter().map(Syl::render).collect();
        if !cur.is_empty() {
            snap.push(cur.render());
        }
        out.push(snap);
    }
    out
}

/// 用 `check_all::rewrite_shape` 的判準算「改寫伸到游標 3 格外」的次數。
///
/// 判準照抄，一個字都沒改：
/// - **只看已定案的格**（最後一格是正在打的，不算）
/// - **逐格嚴格對齊**：前後兩次的「按鍵」要一樣才比得下去
/// - 距離：`now.len() - i >= 3` 才算「遠」
///
/// 韓文這裡的「一格」是一個音節，「按鍵」是組成它的字母序列。
fn count_far(snaps: &[Vec<String>]) -> (usize, Vec<String>) {
    let mut far = 0usize;
    let mut samples: Vec<String> = Vec::new();
    let mut prev: Vec<String> = Vec::new();

    for snap in snaps {
        // 已定案的格＝最後一格以外的全部
        let settled: Vec<String> = if snap.len() < 2 {
            Vec::new()
        } else {
            snap[..snap.len() - 1].to_vec()
        };
        // 對齊：格數只能增加，而且前面的格要還在
        //
        // **韓文這裡對不齊的情況正是종성 이동**：「달」變「다」，
        // 同一格的內容變了。`rewrite_shape` 是比「按鍵」對不對齊、
        // 比「文字」有沒有變；韓文的音節沒有獨立的按鍵欄位，所以
        // 這裡直接比文字——**比原判準更嚴格**，寧可高估。
        if prev.len() <= settled.len() {
            for (i, (p, q)) in prev.iter().zip(settled.iter()).enumerate() {
                if p != q && settled.len() - i >= 3 {
                    far += 1;
                    if samples.len() < 3 {
                        samples.push(format!("第{}格 {p}→{q}（共{}格）", i + 1, settled.len()));
                    }
                }
            }
        }
        prev = settled;
    }
    (far, samples)
}

/// 同上，但算**所有距離**的改寫（不只 3 格外）——看韓文到底改多少次。
fn count_all(snaps: &[Vec<String>]) -> usize {
    let mut n = 0usize;
    let mut prev: Vec<String> = Vec::new();
    for snap in snaps {
        let settled: Vec<String> = if snap.len() < 2 {
            Vec::new()
        } else {
            snap[..snap.len() - 1].to_vec()
        };
        if prev.len() <= settled.len() {
            n += prev
                .iter()
                .zip(settled.iter())
                .filter(|(p, q)| p != q)
                .count();
        }
        prev = settled;
    }
    n
}

const SAMPLES: &[(&str, &str)] = &[
    ("dkssud", "안녕"),
    ("dkssudgktpdy", "안녕하세요"),
    ("gksrnr", "한국"),
    ("ekfms", "다른"),
    ("rkatkgkqslek", "감사합니다"),
    ("dhsmfskfTlrkTdb", "오늘날씨가좋아요"),
    ("dkswkTdjdy", "앉았어요"),
    ("wjsghksjqjgh", "전화번호"),
    ("gkrrydpTjdhwlaadh", "학교에서옳지만오"),
    ("tkfkdgksek", "사랑한다"),
    ("dhfoszksdurdud", "오래간만이영"),
    ("rjaektlqslek", "검다십니다"),
];

fn main() {
    println!("韓文組字：會不會踩到「改寫伸到游標 3 格外」硬指標？\n");
    println!("判準照抄 check_all::rewrite_shape（逐格對齊、倒數第 3 格以上才算）\n");
    println!(
        "{:<20} {:<18} {:>6} {:>8}",
        "按鍵", "組出來", "總改寫", "3格外⚠"
    );
    println!("{}", "─".repeat(60));

    let mut total_far = 0usize;
    let mut total_all = 0usize;
    let mut all_samples: Vec<String> = Vec::new();

    for (keys, want) in SAMPLES {
        let snaps = compose(keys);
        let final_text = snaps.last().map(|s| s.concat()).unwrap_or_default();
        let (far, samples) = count_far(&snaps);
        let all = count_all(&snaps);
        total_far += far;
        total_all += all;
        if all_samples.len() < 6 {
            all_samples.extend(samples);
        }
        // 組錯了要看得出來——這支的組字是自己實作的，不能盲信
        let ok = if final_text == *want {
            ""
        } else {
            " ← 組字對不上"
        };
        println!(
            "{:<20} {:<18} {:>6} {:>8}{}",
            keys, final_text, all, far, ok
        );
    }

    println!("\n總計：改寫 {total_all} 次，其中 {total_far} 次伸到 3 格外");
    if total_far == 0 {
        println!("\n**硬指標過關**——韓文的改寫全都落在倒數 1～2 格。");
    } else {
        println!("\n**踩到硬指標**，例子：");
        for s in &all_samples {
            println!("  {s}");
        }
    }

    // 逐鍵印一句，看清楚改寫發生在哪
    println!("\n─── 逐鍵過程：ekfms（다른）───");
    for (i, snap) in compose("ekfms").iter().enumerate() {
        let settled = if snap.len() < 2 { 0 } else { snap.len() - 1 };
        println!(
            "  第{}鍵  {:<12} 已定案 {} 格",
            i + 1,
            snap.join(""),
            settled
        );
    }
}
