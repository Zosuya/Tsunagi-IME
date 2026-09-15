//! **spike（2026-09-10）**：`在`／`再` 的 bigram 資料站在哪一邊？
//!
//! §2.61.2 判定這一組卡在「詞庫沒有詞性」。但真正決定的是**字層
//! bigram**（`再決定`／`在決定` 兩邊詞庫都沒收），所以先問資料：
//! 模型知不知道「再＋動詞」這回事？
//!
//! 用法：cargo run --release -p ime-core --bin spike_zaizai

fn main() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data");
    ime_core::dict::load_bopomofo(&data);
    let Some(lm) = ime_core::lm::load(&data, ime_core::dict::char_freq_map(&data)) else {
        eprintln!("語言模型載不起來");
        return;
    };

    // 後面接動詞 → 該用「再」；接名詞／地點／時間 → 該用「在」
    let verbs = [
        '說', '決', '確', '開', '想', '試', '看', '做', '談', '討', '檢', '寫', '算', '問', '找',
    ];
    let nouns = [
        '家', '台', '學', '公', '外', '裡', '上', '下', '三', '這', '那', '哪', '路', '車', '床',
    ];

    let show = |title: &str, cs: &[char], want: char| {
        println!("\n=== {title}（該用「{want}」）===");
        let (mut right, mut wrong, mut miss) = (0, 0, 0);
        for &c in cs {
            let zai_in = lm.score('在', c);
            let zai_again = lm.score('再', c);
            let mark = match (zai_in, zai_again) {
                (Some(a), Some(b)) => {
                    let winner = if a >= b { '在' } else { '再' };
                    if winner == want {
                        right += 1;
                        "✓"
                    } else {
                        wrong += 1;
                        "✗"
                    }
                }
                _ => {
                    miss += 1;
                    "—"
                }
            };
            println!(
                "  {mark} 在{c}={:>7}  再{c}={:>7}",
                zai_in.map(|x| format!("{x:.2}")).unwrap_or("無".into()),
                zai_again.map(|x| format!("{x:.2}")).unwrap_or("無".into()),
            );
        }
        println!("  → 對 {right}、錯 {wrong}、查不到 {miss}");
    };

    show("後面接動詞", &verbs, '再');
    show("後面接名詞／地點", &nouns, '在');

    // 「請務必在期限內完成」：閘門為什麼擋掉正解？
    println!(
        "
=== 落單字的字頻比較 ==="
    );
    let greedy = ['必', '現', '內'];
    let viterbi = ['請', '再', '內'];
    let sum = |cs: &[char]| -> f32 { cs.iter().map(|&c| lm.log_freq(c)).sum() };
    for &c in &greedy {
        println!("  貪心落單 {c} = {:.3}", lm.log_freq(c));
    }
    println!("  → 合計 {:.3}", sum(&greedy));
    for &c in &viterbi {
        println!("  維特比落單 {c} = {:.3}", lm.log_freq(c));
    }
    println!("  → 合計 {:.3}", sum(&viterbi));
    println!(
        "
  維特比 > 貪心 ? {}  （false 就會被閘門擋掉）",
        sum(&viterbi) > sum(&greedy)
    );

    // 換個判準：**整條路的 bigram 總分**（詞內接續＋詞間接續）
    println!(
        "
=== 整句 bigram 總分 ==="
    );
    let path_score = |ws: &[&str]| -> f32 {
        let joined: String = ws.concat();
        let cs: Vec<char> = joined.chars().collect();
        cs.windows(2)
            .map(|p| lm.score(p[0], p[1]).unwrap_or(-1.0))
            .sum()
    };
    let g = ["請勿", "必", "再騎", "現", "內", "完成"];
    let v = ["請", "務必", "再", "期限", "內", "完成"];
    println!("  貪心  「{}」= {:.2}", g.concat(), path_score(&g));
    println!("  維特比「{}」= {:.2}", v.concat(), path_score(&v));
    println!("  維特比較高 ? {}", path_score(&v) > path_score(&g));

    // 再看「詞涵蓋的字數」——被詞（長度≥2）蓋住幾個字
    println!(
        "
=== 詞涵蓋的字數 ==="
    );
    let covered = |ws: &[&str]| -> usize {
        ws.iter()
            .filter(|w| w.chars().count() >= 2)
            .map(|w| w.chars().count())
            .sum()
    };
    println!("  貪心   {} / 9", covered(&g));
    println!("  維特比 {} / 9", covered(&v));
}
