//! 把擴充包的 `.txt` 編成 `.bin`（官方包的二進位版本，給 mmap 用）。
//!
//! # 為什麼要有這支
//!
//! 文字包每個宿主行程各載一份到私有記憶體——台語包字串池壓完仍是
//! 4.8MB，開五個宿主就是 24MB。`.bin` 走 mmap **直接在映射的位元組上
//! 查**，五個行程共用同一份檔案頁（實測 27.7MB → 3.5MB，§2.83）。
//!
//! 這支是產檔那一端：讀文字包 → 走 `pack::layers_for_bin`（跟執行期
//! **同一條 `build_index`**）→ 寫成 `pack_bin` 的版面。
//!
//! # 誰該被編
//!
//! **官方發布的語言擴充包**（台語這種）。使用者自己寫的包維持文字檔
//! ——`.bin` 是唯讀的，而那些包隨時要在擴充包編輯器裡改。界線是
//! 「誰發布」不是「多大」。
//!
//! 產出放在**跟輸入 `.txt` 同一個目錄**、同名換副檔名。載入時
//! `pack::find_pack` 的規則是「`.bin` 優先於同名的 `.txt`」，所以
//! 產完就自動生效，不必改設定。
//!
//! 多給幾個名字會編成**一份**（合併，順序就是命令列的順序）——
//! 那是 `build_index` 的既有語意（符號同名接起來、單值層前面的贏），
//! 輸出用第一個名字。
//!
//! 這是衍生檔、不進版控，跟 `connection.bin`、`dict_zh.bin` 一樣。
//!
//!     cargo run --release -p ime-core --bin gen_pack_bin -- 台語

use std::path::PathBuf;
use std::time::Instant;

/// 六層的名字，順序跟 `pack_bin::LAYERS` 一致，只給印表用。
const LAYER_NAMES: [&str; ime_core::pack_bin::LAYERS] = ["en", "ja", "zh", "zh_long", "tw", "sym"];

fn main() {
    // 沒有參數解析 crate——core 維持零依賴，手動比對就夠了
    let names: Vec<String> = std::env::args().skip(1).collect();
    if names.is_empty() || names.iter().any(|a| a == "-h" || a == "--help") {
        eprintln!("用法：gen_pack_bin <包名> [包名…]");
        eprintln!("把擴充包的 .txt 編成同目錄的 .bin（官方包用，給 mmap 共用）。");
        eprintln!("例：gen_pack_bin 台語");
        std::process::exit(1);
    }
    if let Some(bad) = names.iter().find(|n| n.starts_with('-')) {
        eprintln!("認不得的選項 {bad}——這支只收包名，沒有其他選項");
        std::process::exit(1);
    }

    // 每個名字找出它的 `.txt`。**刻意不走 `pack::find_pack`**：那條規則
    // 是 `.bin` 優先，產檔時會撈到上一次自己產的 `.bin`，變成拿產物當原料
    let dirs = pack_dirs();
    let mut paths: Vec<PathBuf> = Vec::with_capacity(names.len());
    for name in &names {
        let Some(p) = dirs
            .iter()
            .map(|d| d.join(format!("{name}.txt")))
            .find(|p| p.is_file())
        else {
            eprintln!("找不到 {name}.txt");
            eprintln!("找過這些資料夾：");
            for d in &dirs {
                eprintln!("  {}", d.display());
            }
            eprintln!("把包放進其中一個，或在設定頁的「擴充包資料夾」指定位置。");
            std::process::exit(1);
        };
        paths.push(p);
    }

    // ① 讀檔、建索引、倒出檔頭與六層
    let t = Instant::now();
    let (meta, layers) = match ime_core::pack::layers_for_bin(&paths) {
        Ok(l) => l,
        Err((p, ime_core::pack::PackReadError::NotUtf8)) => {
            eprintln!(
                "{} 不是 UTF-8（多半是用記事本存成「ANSI」＝Big5）。",
                p.display()
            );
            eprintln!("用編輯器另存成 UTF-8 再跑一次。");
            std::process::exit(1);
        }
        Err((p, ime_core::pack::PackReadError::Missing)) => {
            eprintln!("讀不到 {}", p.display());
            std::process::exit(1);
        }
        // 這支只讀 `.txt`（上面刻意繞開 `find_pack` 的 `.bin` 優先），
        // 走不到這裡——但列出來，之後多一種錯時編譯器會提醒這邊也要想
        Err((p, ime_core::pack::PackReadError::BadBin)) => {
            eprintln!("{} 認不得——這支只該讀 .txt，不該碰到這個錯。", p.display());
            std::process::exit(1);
        }
    };
    let build_ms = t.elapsed().as_millis();

    // 一條都沒有的話產出來也沒用，而且多半是格式打錯了（欄位要用 Tab 分隔）
    if layers.iter().all(Vec::is_empty) {
        eprintln!("六層都是空的——包裡沒有讀得懂的條目。");
        eprintln!("欄位要用 Tab 分隔，第一欄是 en／ja／zh／tw／sym。");
        std::process::exit(1);
    }

    // 檔頭**不能掉**——設定頁顯示的名稱與版本來自那裡，而授權欄更是
    // 重新發布的前提
    if meta.is_empty() {
        eprintln!(
            "警告：{} 沒有檔頭（`# name:` 那幾行）。",
            paths[0].display()
        );
        eprintln!("      官方包應該要有名稱、版本與授權，設定頁會顯示它們。");
    } else if !meta.iter().any(|(k, _)| k == "license") {
        // **只有 `parse_meta` 認得的鍵進得了 `.bin`**：`# license:` 這種
        // ASCII 冒號的才算，寫成「# 授權：CC BY-SA 4.0」（全形冒號＋中文
        // 鍵名）的**會被整行忽略**，產出的 `.bin` 等於沒有授權聲明。
        //
        // 台語包正是這樣寫的，而它是 CC BY-SA 4.0——姓名標示是授權條件，
        // 掉了就不能散布。這裡只能出聲，不能替使用者猜要標誰
        eprintln!("警告：{} 的檔頭沒有 `license` 欄。", paths[0].display());
        eprintln!("      只有 `# license: …`（半形冒號）這種寫法進得了 .bin；");
        eprintln!("      寫成「# 授權：…」會被忽略，產出的包等於沒有授權聲明。");
        eprintln!("      要散布這個 .bin 的話，先把授權與姓名標示補成正式欄位。");
    }

    // ② 編成版面。
    //
    // `write` 要求每層已經依鍵的字面排序好——`layers_for_bin` 倒出來的
    // 就是 `Builder::finish` 排序攤平的結果，這裡不必也不該再排一次
    let t = Instant::now();
    let Some(bytes) = ime_core::pack_bin::write(&meta, &layers) else {
        eprintln!("資料超出版面能表達的範圍（字串池 4GB 或項數 42 億）。");
        std::process::exit(1);
    };
    let write_ms = t.elapsed().as_millis();

    // ③ 寫出去。**一定要走 `write_data_file`**——它先寫暫存檔再原子改名，
    // 那是 mmap 的 SAFETY 前提：映射到一半被覆寫的檔案會讀到半套資料
    let out = paths[0].with_extension("bin");
    if let Err(e) = ime_core::dict::write_data_file(&out, &bytes) {
        eprintln!("寫不進 {}：{e}", out.display());
        std::process::exit(1);
    }

    // ④ 立刻讀回來確認自己認得——**寫出一個載不了的檔比沒有檔更糟**：
    // 執行期 `map_pack_bin` 認不得只會安靜地當成「沒有這個包」
    let size = bytes.len();
    let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
    let Some(p) = ime_core::pack_bin::PackBin::new(leaked) else {
        eprintln!("產生出來的檔案自己認不得，格式有問題——那個 .bin 不要用。");
        std::process::exit(1);
    };

    // 檔頭要逐欄對回來——**授權資訊掉了要在這裡就爆**，不能等到使用者
    // 在設定頁看到「沒有授權」才發現
    for (k, v) in &meta {
        if p.meta(k) != Some(v.as_str()) {
            eprintln!("檔頭的「{k}」對不上：寫進去 {v:?} 讀回來 {:?}", p.meta(k));
            std::process::exit(1);
        }
    }

    // 自我驗證再進一步：六層的項數要跟倒出來的一致，而且每層的第一個鍵
    // 要查得回同一組值。只看 `new()` 過不過的話，位移算歪但長度剛好對得上
    // 的版面照樣會通過
    for (i, rows) in layers.iter().enumerate() {
        if p.len(i) != rows.len() {
            eprintln!(
                "第 {} 層（{}）項數對不上：寫進去 {} 讀回來 {}",
                i,
                LAYER_NAMES[i],
                rows.len(),
                p.len(i)
            );
            std::process::exit(1);
        }
        let Some((key, vals)) = rows.first() else {
            continue;
        };
        if p.all(i, key) != *vals {
            eprintln!("第 {} 層（{}）查「{key}」的結果對不上。", i, LAYER_NAMES[i]);
            std::process::exit(1);
        }
    }

    for (i, name) in LAYER_NAMES.iter().enumerate() {
        println!("{name:<12}{}", layers[i].len());
    }
    // 檔頭印出來給人眼確認——授權與姓名標示有沒有帶進去，看這裡
    println!(
        "檔頭        {} 欄（{}）",
        meta.len(),
        meta.iter()
            .map(|(k, _)| k.as_str())
            .collect::<Vec<_>>()
            .join("、")
    );
    println!("輸出大小    {:.2} MB", size as f64 / 1048576.0);
    println!("建索引      {build_ms}ms");
    println!("編版面      {write_ms}ms");
    println!("寫入        {}", out.display());
    println!("自我驗證    通過");
}

/// 要去哪些資料夾找包，**依優先序**。跟 `spike_pack_bin` 同一套：
/// 設定裡指定的位置 → `%APPDATA%` → 專案根的 `packs/`。
///
/// 最後那個後備是給開發用的——repo 裡就有 `packs\台語.txt`，
/// 不必先把包搬進 `%APPDATA%` 才跑得動這支工具。
fn pack_dirs() -> Vec<PathBuf> {
    let user_dir = ime_core::config::user_dir();
    let cfg = ime_core::config::Config::load(user_dir.as_deref());
    let mut v: Vec<PathBuf> = Vec::new();
    // 去重要比**正規化後**的路徑：設定裡填的是絕對路徑、後備是相對的
    // `packs`，兩者在開發環境常常是同一個地方，不正規化就會列兩次
    let mut push = |d: PathBuf| {
        let key = d.canonicalize().unwrap_or_else(|_| d.clone());
        if !v
            .iter()
            .any(|x: &PathBuf| x.canonicalize().unwrap_or_else(|_| x.clone()) == key)
        {
            v.push(d);
        }
    };
    if let Some(d) = ime_core::pack::resolved_dir(&cfg.behavior.packs_dir) {
        push(d);
    }
    if let Some(d) = user_dir {
        push(d.join("packs"));
    }
    push(PathBuf::from("packs"));
    v
}
