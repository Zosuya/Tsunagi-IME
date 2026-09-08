//! 台語 B 路線的覆蓋率量測（§2.64.4 第一件）。
//!
//! # 這支在問什麼
//!
//! B 路線是「用注音打華語詞、候選出台語漢字」。要做到這件事，得先把
//! 台華線頂辭典的**華語詞**轉成按鍵串。開發文件 §2.64.4 列的頭號風險就是
//! 這一步：九萬條華語詞裡，有多少能**可靠**轉出按鍵？
//!
//! 「可靠」不是「查得到」。`mkkeys` 的 `zh_keys` 有兩條路，品質差很多：
//!
//! | 路 | 怎麼來的 | 可靠嗎 |
//! |---|---|---|
//! | 整詞命中 `BPMFMappings` | 詞表本來就收了這個詞的讀音 | **可靠** |
//! | 逐字拼 `BPMFBase` | 詞表沒有，只好一個字一個字查 | **多音字會錯** |
//!
//! CLAUDE.md 那句「整句丟進 mkkeys 會逐字拼、多音字全錯」講的就是第二條。
//! 所以這支把兩條分開數，而不是混成一個「覆蓋率 95%」的漂亮數字。
//!
//! # 還要再問一層：打得回來嗎
//!
//! 按鍵產得出來，不代表引擎打得回那個華語詞。所以每條再走一次
//! `Incremental::from_keys → rank::sort → compose`，跟 `mkkeys` 同一條鏈。
//! 分三種結果，判準跟 `mkkeys` 的三個記號同構：
//!
//! - **命中**：按鍵可靠，而且引擎第一名就打回原詞 → 這條能用
//! - **排序輸**：按鍵可靠，但引擎打出別的詞 → 仍可用，台語詞靠自己的層排上來
//! - **不可靠**：只能逐字拼，或連字都查不到 → 要人工處理或放棄
//!
//! 用法：`cargo run --release -p ime-core --bin spike_taigi_cover -- <csv 路徑>`
//! 加 `--dump <檔>` 把逐條結果倒出來，方便人工翻查失敗的長什麼樣。

use ime_core::compose;
use ime_core::cutpoint::incremental::Incremental;
use ime_core::cutpoint::{normalize, rank};
use std::collections::HashMap;

fn data_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("core 的上層")
        .join("data")
}

/// 注音音節 → 按鍵。跟 `mkkeys::build_syllable_map` 同一份資料、同一條規則。
fn build_syllable_map(dir: &std::path::Path) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Ok(content) = std::fs::read_to_string(dir.join("bopomofo").join("BPMFBase.txt")) else {
        eprintln!("找不到 BPMFBase.txt");
        return out;
    };
    for line in content.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() == 5 {
            out.entry(f[1].to_string()).or_insert(f[3].to_string());
        }
    }
    out
}

/// 單字 → 按鍵（逐字拼的退路）。多音字取檔案裡第一個讀音。
fn build_char_map(dir: &std::path::Path) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Ok(content) = std::fs::read_to_string(dir.join("bopomofo").join("BPMFBase.txt")) else {
        return out;
    };
    for line in content.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() == 5 {
            let key = if f[1].ends_with(['ˊ', 'ˇ', 'ˋ', '˙']) {
                f[3].to_string()
            } else {
                // 一聲要補結尾空白，跟 `syllable_keys` 同一條規則
                format!("{} ", f[3])
            };
            out.entry(f[0].to_string()).or_insert(key);
        }
    }
    out
}

/// 表記 → 所有讀音。同一個詞可能有多行，全收下來後面一一試。
fn build_word_map(dir: &std::path::Path) -> HashMap<String, Vec<Vec<String>>> {
    let mut out: HashMap<String, Vec<Vec<String>>> = HashMap::new();
    let Ok(content) = std::fs::read_to_string(dir.join("bopomofo").join("BPMFMappings.txt")) else {
        eprintln!("找不到 BPMFMappings.txt");
        return out;
    };
    for line in content.lines() {
        let mut it = line.split_whitespace();
        let Some(word) = it.next() else { continue };
        let syls: Vec<String> = it.map(String::from).collect();
        if syls.is_empty() {
            continue;
        }
        let e = out.entry(word.to_string()).or_default();
        if !e.contains(&syls) {
            e.push(syls);
        }
    }
    out
}

/// 一串注音音節換成按鍵。一聲要補結尾空白，不然整串會黏成一個音節。
fn syllable_keys(syls: &[String], map: &HashMap<String, String>) -> Option<String> {
    let mut out = String::new();
    for s in syls {
        let k = map.get(s)?;
        out.push_str(k);
        if !s.ends_with(['ˊ', 'ˇ', 'ˋ', '˙']) {
            out.push(' ');
        }
    }
    Some(out)
}

/// 這個華語詞的按鍵是怎麼轉出來的——決定了它可不可靠。
#[derive(PartialEq, Clone, Copy)]
enum Src {
    /// 整詞在 `BPMFMappings` 裡，讀音是詞表給的
    Word,
    /// 詞表沒有，逐字拼——多音字會挑錯
    PerChar,
    /// 有字連 `BPMFBase` 都查不到
    Miss,
}

fn zh_keys(
    word: &str,
    words: &HashMap<String, Vec<Vec<String>>>,
    syl: &HashMap<String, String>,
    chars: &HashMap<String, String>,
) -> (Vec<String>, Src) {
    if let Some(readings) = words.get(word) {
        let v: Vec<String> = readings
            .iter()
            .filter_map(|r| syllable_keys(r, syl))
            .collect();
        if !v.is_empty() {
            return (v, Src::Word);
        }
    }
    let mut acc: Vec<String> = Vec::new();
    for ch in word.chars() {
        let s = ch.to_string();
        let k = words
            .get(&s)
            .and_then(|r| r.first())
            .and_then(|r| syllable_keys(r, syl))
            .or_else(|| chars.get(&s).cloned());
        let Some(k) = k else {
            return (Vec::new(), Src::Miss);
        };
        acc.push(k);
    }
    if acc.is_empty() {
        (Vec::new(), Src::Miss)
    } else {
        (vec![acc.concat()], Src::PerChar)
    }
}

/// 這串按鍵打出來是什麼？走的是 `mkkeys` 那條完整的鏈。
fn compose_text(keys: &str) -> String {
    let cands = rank::sort(Incremental::from_keys(keys).cuttings());
    let segs = cands.first().map(|c| normalize(c)).unwrap_or_default();
    compose::text_of(&compose::compose(&segs))
}

/// 純漢字的華語詞——含英數（`FB`、`7-11`）、標點（諺語）的另外算。
fn is_pure_han(s: &str) -> bool {
    !s.is_empty()
        && s.chars().all(|c| {
            ('\u{4e00}'..='\u{9fff}').contains(&c)
                || ('\u{3400}'..='\u{4dbf}').contains(&c)
                || c >= '\u{20000}'
        })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut path = String::new();
    let mut dump: Option<String> = None;
    let mut pack: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--dump" => {
                i += 1;
                dump = args.get(i).cloned();
            }
            "--pack" => {
                i += 1;
                pack = args.get(i).cloned();
            }
            s => path = s.to_string(),
        }
        i += 1;
    }
    if path.is_empty() {
        eprintln!("用法: spike_taigi_cover <台華線頂 csv> [--dump <檔>] [--pack <檔>]");
        std::process::exit(1);
    }

    let dir = data_dir();
    // **載的東西必須跟 `mkkeys` 一模一樣**，一項都不能少——少載任何一層，
    // 這支量出來的覆蓋率就跟計分器各說各話。第一版就是漏了整段，
    // 中文詞庫沒載，引擎把注音按鍵全判成日文，命中率印出 0.0%
    ime_core::pack::set_bundled_dir(dir.parent().map(|d| d.join("packs")));
    ime_core::pack::load(
        "__不存在的資料夾__",
        &[ime_core::pack::BUNDLED_SYMBOLS.to_string()],
    );
    ime_core::english::load(&dir);
    ime_core::dict::load_bopomofo(&dir);
    ime_core::lm::load(&dir, ime_core::dict::char_freq_map(&dir));
    ime_core::dict::load_japanese(&dir);

    let syl = build_syllable_map(&dir);
    let chars = build_char_map(&dir);
    let words = build_word_map(&dir);
    eprintln!(
        "詞表 {} 條、單字 {} 個、音節 {} 種",
        words.len(),
        chars.len(),
        syl.len()
    );

    // CSV 帶 BOM（§2.49 那個坑），要先剝掉再讀檔頭，不然第一欄名字會多出雜訊
    let raw = std::fs::read_to_string(&path).expect("讀不到 CSV");
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
    let mut lines = raw.lines();
    let header: Vec<&str> = lines.next().expect("空檔").split(',').collect();
    let col = |name: &str| {
        header
            .iter()
            .position(|h| h.trim_matches('"') == name)
            .unwrap_or_else(|| panic!("CSV 沒有 {name} 欄"))
    };
    let (c_hoa, c_han) = (col("HoaBun"), col("HanLoTaibunKip"));

    // 欄位不含跳脫逗號（實測過），欄數對不上的直接算髒資料
    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut dirty = 0usize;
    for line in lines {
        let f: Vec<&str> = line.split(',').collect();
        if f.len() != header.len() {
            dirty += 1;
            continue;
        }
        let hoa = f[c_hoa].trim_matches('"').trim().to_string();
        let han = f[c_han].trim_matches('"').trim().to_string();
        if hoa.is_empty() || han.is_empty() {
            dirty += 1;
            continue;
        }
        pairs.push((hoa, han));
    }

    let total = pairs.len();
    let (pure, impure): (Vec<_>, Vec<_>) = pairs.into_iter().partition(|(h, _)| is_pure_han(h));

    // 同一個華語詞可能對到多個台語詞（一對多，§2.64.7）——按鍵只要產一次
    let mut uniq: HashMap<String, Vec<String>> = HashMap::new();
    for (hoa, han) in &pure {
        uniq.entry(hoa.clone()).or_default().push(han.clone());
    }
    let mut keys: Vec<&String> = uniq.keys().collect();
    keys.sort();

    let (mut hit, mut lost, mut perchar, mut miss) = (0usize, 0usize, 0usize, 0usize);
    let mut perchar_ok = 0usize;
    // 包檔的行。只收引擎打得回原詞、而且查得到整詞注音的——
    // 逐字拼的那批雖然有些剛好對，但注音符號要逐字組，多音字會寫錯，
    // 寫進包裡就是永久的錯資料，不如不收
    let mut pack_lines: Vec<String> = Vec::new();
    let mut by_len: HashMap<usize, (usize, usize)> = HashMap::new();
    let mut out = String::new();
    let mut lost_ex: Vec<String> = Vec::new();
    let mut perchar_ex: Vec<String> = Vec::new();
    let mut miss_ex: Vec<String> = Vec::new();

    for hoa in &keys {
        let (ks, src) = zh_keys(hoa, &words, &syl, &chars);
        let n = hoa.chars().count();
        let mark = match src {
            Src::Miss => {
                miss += 1;
                if miss_ex.len() < 15 {
                    miss_ex.push((*hoa).clone());
                }
                "查不到"
            }
            Src::PerChar => {
                // 逐字拼**不等於一定錯**。「一個星期」「一卡通」這種由常見詞
                // 組成的複合詞，詞表沒收整串，但每個字取第一讀音剛好都對。
                // 真正危險的是多音字（「一」「行」「長」），所以這裡再問一次
                // 引擎：打得回原詞的，那條路其實安全。
                perchar += 1;
                let ok = ks.iter().any(|k| compose_text(k) == **hoa);
                if ok {
                    perchar_ok += 1;
                    "逐字拼✓"
                } else {
                    if perchar_ex.len() < 15 {
                        perchar_ex.push(format!(
                            "{hoa} → {}",
                            ks.first().map(|k| compose_text(k)).unwrap_or_default()
                        ));
                    }
                    "逐字拼✗"
                }
            }
            Src::Word => {
                // 任一讀音打得回原詞就算命中
                let ok = ks.iter().any(|k| compose_text(k) == **hoa);
                let e = by_len.entry(n).or_insert((0, 0));
                e.1 += 1;
                if ok {
                    hit += 1;
                    e.0 += 1;
                    "命中"
                } else {
                    lost += 1;
                    if lost_ex.len() < 15 {
                        lost_ex.push(format!(
                            "{hoa} → {}",
                            ks.first().map(|k| compose_text(k)).unwrap_or_default()
                        ));
                    }
                    "排序輸"
                }
            }
        };
        if dump.is_some() {
            out.push_str(&format!(
                "{mark}\t{hoa}\t{}\t{}\n",
                ks.first().map(String::as_str).unwrap_or(""),
                uniq[*hoa].join("／")
            ));
        }
    }

    // ── 包檔：**國字當鍵，不轉注音** ──
    //
    // 段選單拿組字區已經選好的國字去查（見 `session::segmenu` 的斷詞），
    // 歧義在選字階段就解掉了，不必再對一次按鍵。
    //
    // 所以上面那整套「轉注音」只剩量測的用途——它回答的是「這個詞打
    // 得出來嗎」，跟收不收進包無關。**每個詞都收得進來**，不像舊版
    // 要「整詞查得到注音」才行（那道關卡丟掉 20%，多半是多音字）。
    if pack.is_some() {
        for hoa in &keys {
            // 一對多：同一個華語詞掛多個台語說法，一詞一行
            for t in &uniq[*hoa] {
                pack_lines.push(format!("tw\t{hoa}\t{t}"));
            }
        }
    }

    let n = keys.len();
    let pc = |x: usize| x as f64 * 100.0 / n as f64;
    println!();
    println!("華語詞 → 注音按鍵，覆蓋率");
    println!("────────────────────────────────────────────");
    println!("CSV 總筆數        {total}（欄位不齊／空白另計 {dirty}）");
    println!("純漢字華語詞      {}（去重後 {n} 個）", pure.len());
    println!("含英數／標點      {}（B 路線先不處理）", impure.len());
    println!();
    println!("可靠（引擎打得回原詞）");
    println!(
        "  命中   {hit:6}  {:5.1}%   整詞查得到讀音，第一名就打回原詞",
        pc(hit)
    );
    println!(
        "  逐字拼✓ {perchar_ok:5}  {:5.1}%   詞表沒收整串，但逐字拼剛好對",
        pc(perchar_ok)
    );
    println!(
        "  小計   {:6}  {:5.1}%   ← 這是 B 路線真正能用的量",
        hit + perchar_ok,
        pc(hit + perchar_ok)
    );
    println!();
    println!("不可靠");
    println!(
        "  排序輸 {lost:6}  {:5.1}%   按鍵對，但引擎打出別的詞",
        pc(lost)
    );
    println!(
        "  逐字拼✗ {:5}  {:5.1}%   詞表沒收，而且真的拼錯了",
        perchar - perchar_ok,
        pc(perchar - perchar_ok)
    );
    println!(
        "  查不到 {miss:6}  {:5.1}%   有字連 BPMFBase 都沒有",
        pc(miss)
    );
    println!();
    println!("依詞長看命中率（可靠那批）");
    let mut ls: Vec<&usize> = by_len.keys().collect();
    ls.sort();
    for l in ls {
        let (h, t) = by_len[l];
        println!(
            "  {l} 字  {h:5}/{t:5}  {:5.1}%",
            h as f64 * 100.0 / t as f64
        );
    }
    println!();
    println!("「排序輸」的例子：");
    for e in &lost_ex {
        println!("  {e}");
    }
    println!("「逐字拼✗」（真的拼錯）的例子：");
    println!("  {}", perchar_ex.join(" "));
    println!("「查不到」的例子：");
    println!("  {}", miss_ex.join(" "));

    if let Some(p) = pack {
        let mut body = String::new();
        body.push_str("# name: 台語\n");
        body.push_str("# version: 0.2\n");
        body.push_str("# description: 用注音打華語詞、候選出台語漢字。資料：台文華文線頂辭典\n");
        body.push_str("#\n");
        // **底下六行是授權條件的一部分，不是說明文字，不可以拿掉。**
        // CC BY-SA 4.0 要求標示出處、並以相同方式分享。資料歸資料、
        // 程式歸程式：本包的授權是 CC BY-SA 4.0，跟專案的 GPL-3.0 各自獨立。
        body.push_str("# 資料來源：ChhoeTaigi 找台語｜2002+ 台文華文線頂辭典\n");
        body.push_str("#   基礎資料：鄭良偉 教授；增補校訂：楊允言 教授與眾義工\n");
        body.push_str("#   https://github.com/ChhoeTaigi/ChhoeTaigiDatabase\n");
        body.push_str("# 授權：CC BY-SA 4.0（姓名標示-相同方式分享）\n");
        body.push_str("#   https://creativecommons.org/licenses/by-sa/4.0/deed.zh_TW\n");
        body.push_str("#\n");
        body.push_str("# 只收「整詞查得到注音」的條目，見 tools/取台語資料.md。\n\n");
        for l in &pack_lines {
            body.push_str(l);
            body.push('\n');
        }
        std::fs::write(&p, body).expect("寫不出包");
        eprintln!("包已寫到 {p}（{} 行）", pack_lines.len());
    }

    if let Some(d) = dump {
        std::fs::write(&d, out).expect("寫不出 dump");
        eprintln!("逐條結果已寫到 {d}");
    }
}
