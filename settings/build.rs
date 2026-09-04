//! 把圖示編進設定頁的 exe。
//!
//! # 為什麼需要
//!
//! 沒有內嵌圖示的 exe，Windows 到處都顯示白紙：「設定 → 應用程式」的
//! 清單、開始功能表的捷徑、工作管理員。安裝程式的
//! `UninstallDisplayIcon` 指向這支 exe，也是撈它的資源。
//!
//! # 為什麼不共用 platform/windows 的 build.rs
//!
//! `build.rs` 是每個 crate 各自的，沒有辦法直接共用。抽成一個小 crate
//! 只為了這幾十行不划算——複製，但兩邊都註明對方的存在。
//! 改動時記得看另一邊：`platform/windows/build.rs`。
//!
//! **圖示檔不複製**，直接引用 `platform/windows/res/ime.ico`，免得哪天
//! 換了圖示只更新一邊。

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let ico = manifest
        .parent()
        .unwrap()
        .join("platform")
        .join("windows")
        .join("res")
        .join("ime.ico");

    println!("cargo:rerun-if-changed={}", ico.display());
    if !ico.is_file() {
        println!("cargo:warning=找不到 {}，略過圖示", ico.display());
        return;
    }

    let Some(rc) = find_rc() else {
        // 找不到 rc.exe 就跳過，只是沒有圖示，不該讓建置失敗
        println!("cargo:warning=找不到 rc.exe，略過圖示資源");
        return;
    };

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let rc_file = Path::new(&out_dir).join("settings.rc");
    let res = Path::new(&out_dir).join("settings.res");

    // **ID 要用 1**：Windows 取 exe 的圖示時挑數字最小的那個 ICON 資源。
    // 路徑裡的反斜線在 .rc 語法中要跳脫。
    let content = format!(
        "1 ICON \"{}\"\n",
        ico.display().to_string().replace('\\', "\\\\")
    );
    if std::fs::write(&rc_file, content).is_err() {
        println!("cargo:warning=寫不出 .rc，略過圖示");
        return;
    }

    let status = Command::new(&rc)
        .args(["/nologo", "/fo"])
        .arg(&res)
        .arg(&rc_file)
        .status();

    match status {
        Ok(s) if s.success() => println!("cargo:rustc-link-arg={}", res.display()),
        _ => println!("cargo:warning=rc.exe 編譯資源失敗，略過圖示"),
    }
}

/// 在 Windows SDK 裡找 `rc.exe`（挑版本最新的 x64 版）。
/// 跟 `platform/windows/build.rs` 的同名函式一樣。
fn find_rc() -> Option<PathBuf> {
    let roots = [
        r"C:\Program Files (x86)\Windows Kits\10\bin",
        r"C:\Program Files\Windows Kits\10\bin",
    ];
    let mut found: Vec<PathBuf> = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path().join("x64").join("rc.exe");
            if p.is_file() {
                found.push(p);
            }
        }
    }
    found.sort();
    found.pop()
}
