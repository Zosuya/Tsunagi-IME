//! 測資稽核：三欄之間對不對得起來？
//!
//! # 為什麼要有這支
//!
//! 測資有三欄（文字期望、按鍵期望、實際按鍵），它們**應該互相一致**：
//!
//! - 按鍵期望去掉 `|` 要等於實際按鍵
//! - 文字欄與按鍵欄的段數要一樣（`|` 的數量對齊）
//! - 標點與空白的寫法要跟 `cutpoint::normalize` 的規則一致
//!
//! 對不上的話計分器會拿錯的東西去比，**數字騙人而且很難查**——
//! 實測回報「記事本裡標點各是一段」就是這樣浮出來的。
//!
//! # 用法
//!
//! ```text
//! cargo run --release -p ime-core --bin check_testdata
//! ```

#[path = "common/testdata.rs"]
mod testdata;

use std::collections::BTreeMap;

fn main() {
    let (rows, _) = testdata::load_with_sections("check_testdata");
    if rows.is_empty() {
        eprintln!("沒有測資");
        return;
    }

    // 每一類問題各收幾筆例子
    let mut issues: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let bump = |kind: &'static str,
                msg: String,
                issues: &mut BTreeMap<&str, Vec<String>>,
                counts: &mut BTreeMap<&str, usize>| {
        *counts.entry(kind).or_insert(0) += 1;
        let v = issues.entry(kind).or_default();
        if v.len() < 80 {
            v.push(msg);
        }
    };

    for row in &rows {
        let line = row.line_no;
        let tag = &row.tag;

        // ① 按鍵期望去掉 `|` 要等於實際按鍵
        let joined: String = row.key_expect.replace('|', "");
        if joined != row.keys {
            bump(
                "按鍵欄跟實際按鍵對不上",
                format!("  {tag}:{line} 期望 {joined:?} vs 實際 {:?}", row.keys),
                &mut issues,
                &mut counts,
            );
            continue;
        }

        // 寬容-段／寬容-列舉不必比段數
        if row.want.starts_with('~') || row.want.contains(testdata::ALT) {
            continue;
        }

        // **符號的寫法是例外**：`\|star|\` 三段按鍵打出一個 `★`。
        //
        // 符號打法是把名字用兩個反斜線夾起來（見 `Incremental::push`
        // 的說明），那三段本來就對應一個字，段數天生不一樣。
        if row.keys.contains('\\') {
            continue;
        }

        // ② 兩欄的段數要一樣
        let n_keys = row.key_expect.split('|').count();
        let n_text = row.want.split('|').count();
        if n_keys != n_text {
            bump(
                "文字欄與按鍵欄段數不同",
                format!(
                    "  {tag}:{line} 文字 {n_text} 段 vs 按鍵 {n_keys} 段\n      {:?}\n      {:?}",
                    row.want, row.key_expect
                ),
                &mut issues,
                &mut counts,
            );
            continue;
        }

        // ③ 標點段的寫法：`.` `,` 這種該自成一段
        //
        // 切點引擎一律讓標點自成一段（`Segment::is_mark`），測資把兩個
        // 標點寫在同一段的話，正解永遠對不上引擎給的東西。
        for part in row.key_expect.split('|') {
            let n_punct = part.chars().filter(|c| c.is_ascii_punctuation()).count();
            if n_punct >= 2 && part.chars().count() == n_punct {
                bump(
                    "多個標點寫在同一段",
                    format!("  {tag}:{line} 段 {part:?}（標點該各自一段）"),
                    &mut issues,
                    &mut counts,
                );
            }
        }

        // ④ 文字欄的段數跟「濾掉空白之後」對不對得上
        //
        // 空白段在文字欄寫 `_`，按鍵欄寫一個空白。兩邊要一致。
        for (k, t) in row.key_expect.split('|').zip(row.want.split('|')) {
            let k_blank = !k.is_empty() && k.trim().is_empty();
            let t_blank = t.trim() == "_";
            if k_blank != t_blank {
                bump(
                    "空白段兩欄寫法不一致",
                    format!("  {tag}:{line} 按鍵 {k:?} vs 文字 {t:?}"),
                    &mut issues,
                    &mut counts,
                );
            }
        }
    }

    println!("測資稽核：{} 列\n", rows.len());
    if counts.is_empty() {
        println!("**沒有問題**——三欄互相對得起來。");
        return;
    }
    for (kind, n) in &counts {
        println!("【{kind}】{n} 處");
        for m in &issues[kind] {
            println!("{m}");
        }
        if *n > 5 {
            println!("  …還有 {} 處", n - 5);
        }
        println!();
    }
    let total: usize = counts.values().sum();
    println!("共 {total} 處");
}
