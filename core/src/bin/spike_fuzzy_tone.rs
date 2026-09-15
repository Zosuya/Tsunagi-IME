//! **spike（2026-09-12）**：注音模糊音容錯——「弄壞 vs 修好」的比值量得出來嗎？
//!
//! §2.81.7 列了動手前要先量的三個數字，這支就是去量它們。判準是
//! §2.81.5 的機制：多音節子段查不到詞時，把七組易混音做**單音節**
//! 替換，撞到詞就當作證據，反推使用者打錯了哪個音。
//!
//! # 兩半
//!
//! 測資裡的鍵串**使用者都是打對的**，所以直接掃只量得到「弄壞」那一
//! 側（本來就對、卻被模糊展開改掉）。「修好」那一側要自己造：拿正確
//! 鍵串故意替換一個音，再看展開救不救得回來。兩半合起來才是比值。
//!
//! 用法：cargo run --release -p ime-core --bin spike_fuzzy_tone
//!       加 `--final-only` 只開 `ㄣ/ㄥ`（不含捲舌平舌那三組）

#[path = "common/testdata.rs"]
mod testdata;

use std::collections::{BTreeMap, BTreeSet};

/// 易混音的按鍵對照（大千配置，見 `bopomofo/keymap.rs`）。
/// 每組都是單字元替換，所以展開只是換掉音節裡的一個字元。
/// 第四欄：是不是韻母組（`--final-only` 只留這些）。
///
/// **使用者裁決（2026-09-12）**：原本列了七組，砍掉 `ㄢ/ㄤ`、`ㄈ/ㄏ`、
/// `ㄋ/ㄌ` 三組——**實際上不太有人搞混**，留著只是白白擴大誤傷面。
/// 剩下的四組正好是「前後鼻音 ＋ 捲舌平舌」，後三組同一個成因（南部腔
/// 不分捲舌），設定頁可以當一個群組一起開關。
const PAIRS: [(char, char, &str, bool); 4] = [
    ('p', '/', "ㄣ/ㄥ", true),  // 韻母：前後鼻音，台灣最常見
    ('5', 'y', "ㄓ/ㄗ", false), // 聲母：捲舌／平舌
    ('t', 'h', "ㄔ/ㄘ", false), // 聲母：捲舌／平舌
    ('g', 'n', "ㄕ/ㄙ", false), // 聲母：捲舌／平舌
];

/// 一個音節的所有模糊變體（只換一個字元，不含自己）。
///
/// 只接受換完仍是**合法音節**的——`p`→`/` 可能把合法音節變成殘缺的
/// 東西，那種不必往下查詞庫。
fn variants_of(syl: &str, only_final: bool) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = syl.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        for &(a, b, _, is_final) in PAIRS.iter() {
            if only_final && !is_final {
                continue;
            }
            let to = if c == a {
                b
            } else if c == b {
                a
            } else {
                continue;
            };
            let mut v = chars.clone();
            v[i] = to;
            let v: String = v.iter().collect();
            if ime_core::bopomofo::validity(&v) == ime_core::bopomofo::syllable::Validity::Valid {
                out.push(v);
            }
        }
    }
    out
}

/// 一個窗（數個音節）的所有模糊變體：**一次只替換一個音節**。
fn window_variants(syls: &[String], only_final: bool) -> Vec<String> {
    let mut out = Vec::new();
    for i in 0..syls.len() {
        for v in variants_of(&syls[i], only_final) {
            let mut w = syls.to_vec();
            w[i] = v;
            out.push(w.concat());
        }
    }
    out
}

/// 撞到的詞：(變體鍵串, 詞)。
fn hits(syls: &[String], only_final: bool) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for v in window_variants(syls, only_final) {
        if let Some(w) = ime_core::dict::word_for(&v) {
            out.push((v, w.to_string()));
        }
    }
    out
}

/// 一個詞有多常用——拿字頻當代理，取**最小值**而不是平均。
///
/// 理由：詞的罕見程度由最罕見的那個字決定（「稀飯」的「稀」不罕見，
/// 但「膩友」的「膩」就把整個詞拉下去）。平均會被一個常用字拉高，
/// 正好是這功能最怕的失敗模式（撞到冷門詞卻看起來分數不低）。
fn word_weight(w: &str, freq: &std::collections::HashMap<char, u32>) -> u32 {
    w.chars()
        .map(|c| freq.get(&c).copied().unwrap_or(0))
        .min()
        .unwrap_or(0)
}

/// 選字層實際會查的窗：從左到右**貪婪最長匹配**，回傳 `(起點, 長度)`。
///
/// 為什麼要這一步：無腦的 2～4 滑動窗會產生「希『望明』天」這種**跨詞
/// 邊界的切片**。左邊已經被「希望」整個吃掉，選字層永遠不會拿「望明」
/// 去查詞——把它算進誤傷等於憑空多算一筆。
///
/// 規則：位置 i 由長到短試，查得到詞就整段吃掉、跳到詞尾；查不到就
/// 前進一格，但**該位置仍然要回報一個窗**（那正是模糊展開的觸發點）。
fn greedy_windows(syls: &[String]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < syls.len() {
        let max = 4.min(syls.len() - i);
        let mut ate = 0;
        for n in (2..=max).rev() {
            if ime_core::dict::word_for(&syls[i..i + n].concat()).is_some() {
                out.push((i, n));
                ate = n;
                break;
            }
        }
        if ate > 0 {
            i += ate; // 整個詞吃掉，後面的切片不存在
        } else {
            // 查不到詞——這就是模糊展開的觸發點。回報這裡最長的窗，
            // 長度取到 4（或剩餘長度），跟選字層想吃多長一致
            if max >= 2 {
                out.push((i, max));
            }
            i += 1;
        }
    }
    out
}

#[derive(Default, Clone)]
struct Stat {
    /// 掃過的窗總數
    windows: usize,
    /// 原鍵串查不到詞的窗（觸發面）
    miss: usize,
    /// 其中模糊展開撞到詞的
    hit: usize,
    /// 撞到、而且**唯一**命中的
    hit_unique: usize,
    /// 唯一命中、而且撞到的詞跟正解不同 → 會被弄壞
    broke: usize,
    /// 唯一命中、撞到的詞剛好就是正解 → 詞庫沒收而已，改了反而對
    same: usize,
}

/// `--scan`：**現在的測資裡，有幾列的鍵串本身帶模糊音錯誤？**
///
/// 跟主流程的「造假錯字」不同——這裡問的是真實測資的現況。判準是
/// 一個窗**用原鍵打不出正解、模糊展開之後打得出來**，那就是一筆
/// 模糊音能修好的實例。
///
/// 為什麼預期會很少：測資的鍵串多半是 `mkkeys` 從正確讀音產生的，
/// 理論上不含打錯音。真有的話只會出現在「使用者實測」來源那幾節。
fn scan_testdata(rows: &[testdata::Row], only_final: bool) {
    println!("=== 掃描：現在的測資裡有幾列帶模糊音錯誤？ ===");
    let mut hit_rows = 0usize;
    let mut cases: Vec<(String, usize, String, String, String)> = Vec::new();
    let mut scanned = 0usize;

    for row in rows {
        let segs = row.expected_segs();
        let texts = row.want_segs();
        if segs.len() != texts.len() {
            continue;
        }
        let mut this_row = false;
        for (keys, text) in segs.iter().zip(texts.iter()) {
            let Some(syls) = ime_core::bopomofo::split_syllables(keys) else {
                continue;
            };
            let tchars: Vec<char> = text.chars().collect();
            if tchars.len() != syls.len() {
                continue;
            }
            scanned += 1;
            // **逐個音節**問：正解那個字，用實際按的鍵打得出來嗎？
            //
            // 這比「查整個詞」寬鬆得多——詞庫沒收的詞、或正解本身是
            // 罕用搭配，查詞都會漏掉，但逐字一定抓得到。
            for (si, syl) in syls.iter().enumerate() {
                let want = tchars[si];
                let can_type = |k: &str| {
                    ime_core::dict::chars_for(k)
                        .iter()
                        .any(|c| c.starts_with(want))
                };
                if can_type(syl) {
                    continue; // 這個鍵打得出正解，沒問題
                }
                // 打不出來——換個模糊音就打得出來嗎？
                for v in variants_of(syl, only_final) {
                    if can_type(&v) {
                        this_row = true;
                        if cases.len() < 40 {
                            cases.push((
                                row.tag.clone(),
                                row.line_no,
                                syl.clone(),
                                v,
                                want.to_string(),
                            ));
                        }
                        break;
                    }
                }
            }
        }
        if this_row {
            hit_rows += 1;
        }
    }

    println!("  掃過的注音段     {scanned}");
    println!("  帶模糊音錯誤的列 {hit_rows}");
    if cases.is_empty() {
        println!(
            "\n  一筆都沒有。**這是預期的**——測資的鍵串是 `mkkeys` 從正確讀音產生的，\n  \
             本來就不含打錯的音。模糊音容錯救不到現在的測資，它救的是真人打字。"
        );
    } else {
        println!("\n--- 實例（最多 40 筆）---");
        for (tag, line, orig, fixed, want) in &cases {
            println!("  [{tag}:{line}] {orig} → {fixed}  打得出「{want}」");
        }
    }
}

fn main() {
    testdata::load_engine();
    let rows = testdata::load("spike_fuzzy_tone");
    let only_final = std::env::args().any(|a| a == "--final-only");

    println!(
        "模式：{}\n",
        if only_final {
            "只開 ㄣ/ㄥ"
        } else {
            "四組全開（ㄣㄥ＋捲舌平舌三組）"
        }
    );

    if std::env::args().any(|a| a == "--scan") {
        scan_testdata(&rows, only_final);
        return;
    }

    let mut per_tag: BTreeMap<String, Stat> = BTreeMap::new();
    // 弄壞的實例，給人看的。最後兩欄是正解與撞到的詞的字頻權重，
    // 用來回答「字頻門檻擋不擋得住」
    let mut broke_cases: Vec<(String, String, String, String, u32, u32)> = Vec::new();
    let data_dir0 = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("core 的上層")
        .join("data");
    let cf = ime_core::dict::char_freq_map(&data_dir0);

    for row in &rows {
        let segs = row.expected_segs();
        let texts = row.want_segs();
        if segs.len() != texts.len() {
            continue; // 段數對不上就不拿它當標準答案
        }
        for (keys, text) in segs.iter().zip(texts.iter()) {
            // 只看純注音段：整段切得成音節，而且正解字數等於音節數
            let Some(syls) = ime_core::bopomofo::split_syllables(keys) else {
                continue;
            };
            let tchars: Vec<char> = text.chars().collect();
            if tchars.len() != syls.len() || syls.len() < 2 {
                continue;
            }
            // 選字層實際會查哪些窗：**貪婪最長匹配**，不是無腦掃全部位置。
            //
            // 這一步是整支 spike 最關鍵的修正。無腦滑動窗會掃到「希『望明』天」
            // 這種**跨詞邊界的切片**——左邊早被「希望」吃掉了，選字層根本
            // 不會拿它去查詞。不濾掉的話誤傷率會被大幅高估。
            let covered = greedy_windows(&syls);
            for &(i, n) in &covered {
                {
                    let win = &syls[i..i + n];
                    let want: String = tchars[i..i + n].iter().collect();
                    let st = per_tag.entry(row.tag.clone()).or_default();
                    st.windows += 1;
                    let orig = win.concat();
                    if ime_core::dict::word_for(&orig).is_some() {
                        continue; // 原鍵串查得到 → 完全不觸發，零影響
                    }
                    st.miss += 1;
                    let hs = hits(win, only_final);
                    if hs.is_empty() {
                        continue;
                    }
                    st.hit += 1;
                    // 唯一性：只有一個變體命中詞才可信（§2.81.5 第一道閘）
                    let uniq: BTreeSet<&String> = hs.iter().map(|(_, w)| w).collect();
                    if uniq.len() != 1 {
                        continue;
                    }
                    st.hit_unique += 1;
                    let got = hs[0].1.clone();
                    if got == want {
                        st.same += 1;
                    } else {
                        st.broke += 1;
                        if broke_cases.len() < 40 {
                            let (wf, gf) = (word_weight(&want, &cf), word_weight(&got, &cf));
                            broke_cases.push((row.tag.clone(), orig, want, got, wf, gf));
                        }
                    }
                }
            }
        }
    }

    let mut total = Stat::default();
    println!("=== 誤傷側：測資的鍵是對的，展開會改掉幾個？ ===");
    println!("節               窗數   查無詞  撞到詞  唯一命中 │ 正解相同  會被弄壞");
    for (tag, s) in &per_tag {
        println!(
            "{:<14} {:>6} {:>7} {:>7} {:>8} │ {:>8} {:>9}",
            tag, s.windows, s.miss, s.hit, s.hit_unique, s.same, s.broke
        );
        total.windows += s.windows;
        total.miss += s.miss;
        total.hit += s.hit;
        total.hit_unique += s.hit_unique;
        total.same += s.same;
        total.broke += s.broke;
    }
    let pct = |a: usize, b: usize| {
        if b == 0 {
            0.0
        } else {
            a as f64 * 100.0 / b as f64
        }
    };
    println!(
        "{:<14} {:>6} {:>7} {:>7} {:>8} │ {:>8} {:>9}",
        "總計", total.windows, total.miss, total.hit, total.hit_unique, total.same, total.broke
    );
    println!(
        "\n  觸發面：查不到詞的窗 {:.1}%（{}/{}）",
        pct(total.miss, total.windows),
        total.miss,
        total.windows
    );
    println!(
        "  撞詞率：查不到的裡面撞到詞 {:.1}%（{}/{}）",
        pct(total.hit, total.miss),
        total.hit,
        total.miss
    );
    println!(
        "  唯一命中後仍會弄壞：{}（佔全部窗 {:.2}%）",
        total.broke,
        pct(total.broke, total.windows)
    );

    if !broke_cases.is_empty() {
        println!("\n--- 會被弄壞的實例（最多 40 筆，括號是字頻權重）---");
        // 字頻門檻能不能擋住：撞到的詞比正解罕見的話，門檻就有用
        let mut guardable = 0;
        for (tag, keys, want, got, wf, gf) in &broke_cases {
            let mark = if gf < wf {
                guardable += 1;
                "門檻可擋"
            } else {
                "門檻擋不住"
            };
            println!("  [{tag}] {keys}  正解「{want}」({wf}) → 改成「{got}」({gf})  {mark}");
        }
        println!(
            "  → 列出的 {} 筆裡，撞到的詞比正解罕見的有 {guardable} 筆（字頻門檻擋得住這些）",
            broke_cases.len()
        );
    }

    // === 修復側：故意打錯一個音，展開救得回來嗎？ ===
    println!("\n=== 修復側：把正確鍵串故意打錯一個音，救得回來嗎？ ===");
    let (mut made, mut recovered, mut wrong_fix, mut no_hit, mut not_unique, mut still_word) =
        (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let mut wrong_cases: Vec<(String, String, String)> = Vec::new();
    // 「撞到但不唯一」那批，改用詞頻挑第一名的話對不對
    let (mut nu_top_right, mut nu_top_wrong) = (0usize, 0usize);
    let mut nu_cases: Vec<(String, String, String, usize)> = Vec::new();
    let data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("core 的上層")
        .join("data");
    let char_freq = ime_core::dict::char_freq_map(&data_dir);

    for row in &rows {
        let segs = row.expected_segs();
        let texts = row.want_segs();
        if segs.len() != texts.len() {
            continue;
        }
        for (keys, text) in segs.iter().zip(texts.iter()) {
            let Some(syls) = ime_core::bopomofo::split_syllables(keys) else {
                continue;
            };
            let tchars: Vec<char> = text.chars().collect();
            if tchars.len() != syls.len() || syls.len() < 2 {
                continue;
            }
            for &(i, n) in &greedy_windows(&syls) {
                {
                    let win = &syls[i..i + n];
                    let want: String = tchars[i..i + n].iter().collect();
                    // 只拿「原本查得到、而且查出來就是正解」的窗來造錯
                    // ——那才是使用者真的打算打出來的詞
                    let orig = win.concat();
                    let orig_word = ime_core::dict::word_for(&orig).map(|w| w.to_string());
                    if orig_word.as_deref() != Some(want.as_str()) {
                        continue;
                    }
                    // 造一個錯：替換其中一個音節
                    for typo in window_variants(win, only_final) {
                        made += 1;
                        // 打錯之後照樣查得到詞 → 根本不觸發（心理／行李那種）
                        if ime_core::dict::word_for(&typo).is_some() {
                            still_word += 1;
                            continue;
                        }
                        let Some(tsyls) = ime_core::bopomofo::split_syllables(&typo) else {
                            no_hit += 1;
                            continue;
                        };
                        let hs = hits(&tsyls, only_final);
                        if hs.is_empty() {
                            no_hit += 1;
                            continue;
                        }
                        let uniq: BTreeSet<&String> = hs.iter().map(|(_, w)| w).collect();
                        if uniq.len() != 1 {
                            not_unique += 1;
                            // 唯一性擋掉的這批，如果改用詞頻挑第一名呢？
                            // **這是放寬唯一性限制的代價與收益**——
                            // 唯一性本身是同義反覆的來源（錯字是從正解造的，
                            // 折返命中必然在集合裡），這一批才量得到真實排序。
                            let mut cands: Vec<&(String, String)> = hs.iter().collect();
                            cands.sort_by_key(|(_, w)| {
                                std::cmp::Reverse(word_weight(w, &char_freq))
                            });
                            if cands[0].1 == want {
                                nu_top_right += 1;
                            } else {
                                nu_top_wrong += 1;
                                if nu_cases.len() < 20 {
                                    nu_cases.push((
                                        typo.clone(),
                                        want.clone(),
                                        cands[0].1.clone(),
                                        uniq.len(),
                                    ));
                                }
                            }
                            continue;
                        }
                        if hs[0].1 == want {
                            recovered += 1;
                        } else {
                            wrong_fix += 1;
                            if wrong_cases.len() < 20 {
                                wrong_cases.push((typo, want.clone(), hs[0].1.clone()));
                            }
                        }
                    }
                }
            }
        }
    }

    println!("  造出的錯字案例       {made}");
    println!("  打錯後仍是別的詞     {still_word}（不觸發，救不到）");
    println!("  展開撞不到詞         {no_hit}");
    println!("  撞到但不唯一         {not_unique}");
    println!("  ✓ 救回正解           {recovered}");
    println!("  ✗ 救成別的詞         {wrong_fix}");
    println!("\n  【放寬唯一性】上面「不唯一」那 {not_unique} 筆改用詞頻挑第一名：");
    println!("    ✓ 挑中正解 {nu_top_right}　✗ 挑錯 {nu_top_wrong}");
    if nu_top_right + nu_top_wrong > 0 {
        println!(
            "    → 放寬的話這批救對 {:.1}%",
            nu_top_right as f64 * 100.0 / (nu_top_right + nu_top_wrong) as f64
        );
    }
    if !nu_cases.is_empty() {
        println!("  --- 放寬後會挑錯的實例（最多 20 筆）---");
        for (keys, want, got, n) in &nu_cases {
            println!("    {keys}  想打「{want}」→ 挑成「{got}」（{n} 個候選）");
        }
    }
    let actionable = recovered + wrong_fix;
    if actionable > 0 {
        println!(
            "\n  真正會動手的案例裡，救對 {:.1}%（{recovered}/{actionable}）",
            recovered as f64 * 100.0 / actionable as f64
        );
    }
    if !wrong_cases.is_empty() {
        println!("\n--- 救成別的詞的實例（最多 20 筆）---");
        for (keys, want, got) in &wrong_cases {
            println!("  {keys}  想打「{want}」→ 會被救成「{got}」");
        }
    }

    // === 總比值 ===
    println!("\n=== 結論：弄壞 vs 修好 ===");
    println!("  弄壞（本來對的被改掉）  {}", total.broke);
    println!("  修好（打錯的被救回）    {recovered}");
    if total.broke > 0 {
        println!(
            "  比值 修好:弄壞 = {:.2} : 1",
            recovered as f64 / total.broke as f64
        );
    } else if recovered > 0 {
        println!("  比值 修好:弄壞 = ∞（一個都沒弄壞）");
    }
}
