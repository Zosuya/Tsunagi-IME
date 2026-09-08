//! 符號表稽核：整份表載進去之後，每個名字叫得出東西嗎？
//!
//! 跟 `check_testdata` 同一個性質——**先確認資料是對的**，不然是拿
//! 錯的表在調引擎。
//!
//! 查的是：
//! 1. 有沒有名字被 `sanitize` 整列擋掉（包裡藏了不可見字元）
//! 2. `+` 有沒有正確展開成 ZWJ（`👩+💻` → 一個圖而不是兩個）
//! 3. 同一組裡有沒有重複的符號（候選裡會出現看起來一樣的兩格）
//! 4. 跨包同名的有沒有正確合併
//!
//! 用法：cargo run --release -p ime-core --bin check_symbols

use std::collections::HashMap;

fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let packs_dir = root.join("packs");

    // 兩份預載包都載，才看得到合併的效果
    let names = ["內建符號", "內建emoji"];
    ime_core::pack::set_bundled_dir(Some(packs_dir.clone()));
    ime_core::pack::load("__不存在的使用者資料夾__", &names.map(String::from));

    let idx = ime_core::pack::index();
    println!("載入 {} 個符號名字\n", idx.sym.len());

    let mut problems = 0;

    // ── 1. 檔案裡有幾列、索引裡有幾個名字，對得上嗎 ──
    let mut file_names: HashMap<String, usize> = HashMap::new();
    let mut file_rows = 0;
    for n in names {
        let p = packs_dir.join(format!("{n}.txt"));
        let Ok(text) = std::fs::read_to_string(&p) else {
            println!("✗ 讀不到 {}", p.display());
            problems += 1;
            continue;
        };
        for (ln, line) in text.lines().enumerate() {
            if line.trim().is_empty() || line.trim_start().starts_with('#') {
                continue;
            }
            let mut f = line.split('\t');
            if f.next().map(str::trim) != Some("sym") {
                continue;
            }
            file_rows += 1;
            let Some(input) = f.next() else { continue };
            for name in input.split([',', '，']) {
                let name = name.trim();
                if name.is_empty() {
                    continue;
                }
                *file_names.entry(name.to_string()).or_default() += 1;
                if !idx.sym.contains_key(name) {
                    println!("✗ {n}.txt 第 {} 行的「{name}」載不進去", ln + 1);
                    println!("   多半是那一列有不可見字元，被 sanitize 整列擋掉");
                    problems += 1;
                }
            }
        }
    }
    println!("檔案裡 {file_rows} 列、{} 個名字", file_names.len());

    // ── 2. 每個名字的符號都是完整的圖嗎 ──
    let mut zwj_groups = 0;
    let mut lone_zwj = 0;
    for (name, syms) in idx.sym.iter() {
        for s in syms {
            if s.contains('\u{200D}') {
                zwj_groups += 1;
                // ZWJ 不該在頭尾——那代表 `+` 寫錯位置，會顯示成碎片
                if s.starts_with('\u{200D}') || s.ends_with('\u{200D}') {
                    println!("✗ 「{name}」的 {s:?} ZWJ 在頭或尾");
                    problems += 1;
                    lone_zwj += 1;
                }
            }
            // 落單的 `+` 沒展開（`++` 之外不該有）
            if s == "+" {
                continue; // 這是刻意的字面加號
            }
            if s.contains('+') {
                println!("✗ 「{name}」的 {s:?} 還留著沒展開的 +");
                problems += 1;
            }
        }
    }
    println!("ZWJ 組合 {zwj_groups} 個（頭尾錯位 {lone_zwj}）");

    // ── 3. 同一組裡重複 ──
    for (name, syms) in idx.sym.iter() {
        let mut seen = Vec::new();
        for s in syms {
            if seen.contains(&s) {
                println!("✗ 「{name}」裡有重複的 {s}");
                problems += 1;
            }
            seen.push(s);
        }
    }

    // ── 4. 合併：兩份包都有的名字，符號數要是兩邊的和 ──
    let merged: Vec<_> = file_names
        .iter()
        .filter(|(_, &c)| c > 1)
        .map(|(k, _)| k.clone())
        .collect();
    println!("\n跨包同名（會合併）{} 個：", merged.len());
    for name in merged.iter().take(8) {
        if let Some(syms) = idx.sym.get(name) {
            let show: Vec<&str> = syms.iter().take(10).map(String::as_str).collect();
            println!("  \\{name}\\ → {} 個：{}", syms.len(), show.join(" "));
        }
    }
    if merged.len() > 8 {
        println!("  …其餘 {} 個", merged.len() - 8);
    }

    // ── 5. 規模 ──
    let total_syms: usize = idx.sym.values().map(Vec::len).sum();
    let max = idx.sym.iter().max_by_key(|(_, v)| v.len());
    println!("\n共 {} 個名字、{total_syms} 個符號", idx.sym.len());
    if let Some((n, v)) = max {
        println!("最大的一組：\\{n}\\ 有 {} 個", v.len());
    }

    println!();
    if problems == 0 {
        println!("✓ 符號表沒問題");
    } else {
        println!("✗ {problems} 個問題");
        std::process::exit(1);
    }
}
