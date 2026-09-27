//! `gen_dict_ja`／`gen_dict_zh`／`gen_connection` 共用的兩個旗標。
//!
//! ```text
//! （不帶）      照舊，一律重產
//! --if-stale    現有的檔這一版程式認得就跳過；認不得或不存在才重產
//! --check       只檢查：認得回 0，認不得或不存在回 1，不產任何東西
//! ```
//!
//! # 為什麼需要
//!
//! 版面改版（`VERSION` 加一）之後，舊的 `.bin` 程式認不得：開發機會退回
//! 從文字詞典重建（每個行程多等約 1 秒、多吃一份私有記憶體），**安裝版
//! 只帶 `.bin`、沒有文字詞典可退，日文詞典整個消失**。過去只有「缺檔才
//! 產」的關卡，檔在但舊了沒人發現。建置與打包腳本改成一律跑
//! `--if-stale`，打包再跑一次 `--check` 當守門。
//!
//! **「認得」的判準在 core**（`dict_bin::is_current` 那幾支，跟執行期
//! 載入同一套），這裡只管旗標與訊息，不碰檔頭格式。
//!
//! 用 `#[path]` 掛進三支 `gen_*`，跟 `common/testdata.rs` 同一個做法。

use ime_core::dict::{bin_state, BinState};
use std::path::Path;

#[derive(Clone, Copy)]
pub enum Mode {
    Always,
    IfStale,
    Check,
}

/// 解析命令列。認不得的參數直接結束——打錯字默默照「一律重產」跑，
/// 比失敗更難發現（打包腳本會以為自己做過檢查）。
pub fn mode(bin: &str) -> Mode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => Mode::Always,
        ["--if-stale"] => Mode::IfStale,
        ["--check"] => Mode::Check,
        _ => {
            eprintln!("用法：{bin} [--if-stale | --check]");
            eprintln!("  （不帶）     一律重產");
            eprintln!("  --if-stale   這一版程式認得現有的檔就跳過，否則重產");
            eprintln!("  --check      只檢查現有的檔是不是這一版的版面（不是就回 1）");
            std::process::exit(2);
        }
    }
}

/// 依模式決定要不要往下產。**不必產的情況在這裡就結束行程**：
/// `--if-stale` 遇到已是最新 → 0；`--check` 一律在這裡結束（0 或 1）。
/// 回來了就代表呼叫端要重產。
pub fn gate(bin: &str, mode: Mode, out: &Path, is_current: fn(&[u8]) -> bool) {
    if let Mode::Always = mode {
        return;
    }
    let name = out
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| out.display().to_string());
    let state = match bin_state(out, is_current) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("讀不了 {}：{e}", out.display());
            std::process::exit(1);
        }
    };
    match (mode, state) {
        (Mode::IfStale, BinState::Current) => {
            println!("{name} 已是最新（這一版程式認得它的版面），跳過");
            std::process::exit(0);
        }
        (Mode::IfStale, BinState::Stale) => {
            println!("{name} 是舊版面（或壞檔），這一版程式認不得——重產");
        }
        (Mode::IfStale, BinState::Missing) => {
            println!("沒有 {name}——產生");
        }
        (Mode::Check, BinState::Current) => {
            println!("✓ {name} 是這一版程式認得的版面");
            std::process::exit(0);
        }
        (Mode::Check, s) => {
            let why = if s == BinState::Missing {
                "不存在"
            } else {
                "是舊版面（或壞檔），這一版程式認不得"
            };
            eprintln!("✗ {name} {why}：{}", out.display());
            eprintln!("  重產：cargo run --release -p ime-core --bin {bin} -- --if-stale");
            std::process::exit(1);
        }
        (Mode::Always, _) => unreachable!("上面已經回去了"),
    }
}
