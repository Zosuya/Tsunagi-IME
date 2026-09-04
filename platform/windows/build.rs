//! 把 Windows 圖示資源編進 DLL。
//!
//! 為什麼需要這一步：`RegisterProfile` 交給 Windows 的圖示是
//! 「DLL 路徑 + 索引」，Windows 會去 DLL 裡撈圖示資源。Rust 預設
//! 不會編資源檔，撈不到就退回顯示語言名稱（「繁體」）。
//!
//! 不引入第三方套件——直接呼叫 Windows SDK 自帶的 `rc.exe`，
//! 產生 `.res` 再交給連結器。

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    // **每一個圖示檔都要監看**。只寫 ime.ico 的話，改了工作列模式圖示
    // 之後 cargo 認為沒事發生，資源不重編、DLL 裡還是舊圖——而且完全
    // 沒有錯誤訊息，只會納悶「明明改了怎麼沒變」。
    println!("cargo:rerun-if-changed=res/ime.rc");
    for f in std::fs::read_dir("res").into_iter().flatten().flatten() {
        if f.path().extension().is_some_and(|e| e == "ico") {
            println!("cargo:rerun-if-changed={}", f.path().display());
        }
    }

    let Some(rc) = find_rc() else {
        // 找不到 rc.exe 就跳過，只是沒有圖示，不該讓建置失敗
        println!("cargo:warning=找不到 rc.exe，略過圖示資源");
        return;
    };

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let res = Path::new(&out_dir).join("ime.res");

    let status = Command::new(&rc)
        .args(["/nologo", "/fo"])
        .arg(&res)
        .arg("res/ime.rc")
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("cargo:rustc-link-arg={}", res.display());
        }
        _ => println!("cargo:warning=rc.exe 編譯資源失敗，略過圖示"),
    }
}

/// 在 Windows SDK 裡找 `rc.exe`（挑版本最新的 x64 版）。
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
