//! 按鍵擷取：**設定頁舉旗，輸入法送出時改送原始按鍵**。
//!
//! # 為什麼需要這條路
//!
//! 擴充包編輯器的「怎麼打」欄要收的是原始按鍵（`su3cl3`），但
//! **輸入法開著時宿主收不到任何鍵盤事件**——按鍵先進輸入法，宿主拿到的
//! 只有組字結果。spike 在兩個平台都實測過（macOS 2026-09-20、
//! Windows §2.75.8）：`Event::Key` 一個都沒有。
//!
//! Windows 之所以沒事，是因為它的組字區裝的**本來就是原始按鍵**
//! （`composition_text()` 在自動模式回 `input.keys()`），編輯器順手撈
//! 組字區就拿到了。**macOS 的組字區裝的是轉換後的國字**（遵循 Mac 慣例，
//! 見 `platform/macos/src/echo_ime.rs` 的 `composition()`），那條路走不通。
//!
//! 所以改成：**設定頁主動舉旗**，輸入法看到旗子就在送出時改送原始按鍵。
//!
//! # 為什麼不是「認出宿主是設定頁」
//!
//! 想過，但**設定頁沒有自己的 bundle id**——它是 `.app` 裡的裸執行檔，
//! `keyprobe.log` 實測它報的是 `?`，而別的宿主也有報 `?` 的。拿那個當
//! 判準會誤傷它們（那些宿主會變成打不出中文）。舉旗把「誰要按鍵」這件事
//! 交給要的人自己說，繞過整個識別問題。
//!
//! # 為什麼是檔案
//!
//! **設定頁是另一個行程**（輸入法用 `spawn` 開的），記憶體不共用。而設定
//! 與學習檔本來就走同一個資料夾，多一個小檔案不引進任何新機制。
//!
//! # 旗子會過期
//!
//! 設定頁當掉沒收旗的話，旗子留著會讓**別的 app 送出也變成按鍵串**。
//! 所以旗子帶時間、只認 `FRESH` 之內的，設定頁聚焦期間定期重舉。
//! 最糟情況是殘留 `FRESH` 這麼久。

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// 旗標檔名。點開頭，跟使用者自己的檔案分開。
const FILE: &str = ".keycapture";

/// 旗子多新才算數。設定頁每隔比這個短的時間重舉一次。
const FRESH: Duration = Duration::from_secs(5);

/// 舉旗（設定頁用）。**要定期重舉**，不然 `FRESH` 之後就失效。
pub fn raise_in(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    // 內容無意義，看的是檔案的修改時間；寫一行字是為了使用者偶然
    // 打開它時看得懂這是什麼
    let _ = std::fs::write(dir.join(FILE), "擴充包編輯器要原始按鍵；可以安全刪除\n");
}

/// 收旗（設定頁用）。刪不掉不算錯——旗子本來就會過期。
pub fn lower_in(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(FILE));
}

/// 現在要不要送原始按鍵（輸入法用）。
///
/// 旗子不在、或已經過期都回 `false`。**不在熱路徑上**——只有送出
/// 那一下會問，不是每按一鍵。
pub fn wanted_in(dir: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(dir.join(FILE)) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        // 拿不到時間就當作不新鮮——寧可少做，也不要讓別的 app
        // 莫名其妙收到按鍵串
        return false;
    };
    match SystemTime::now().duration_since(modified) {
        Ok(age) => age <= FRESH,
        // 時間倒退（系統時鐘被調過）：當成剛寫的
        Err(_) => true,
    }
}

fn dir() -> Option<PathBuf> {
    crate::config::user_dir()
}

/// 舉旗。設定頁聚焦在按鍵欄時呼叫。
pub fn raise() {
    if let Some(d) = dir() {
        raise_in(&d);
    }
}

/// 收旗。離開按鍵欄或關視窗時呼叫。
pub fn lower() {
    if let Some(d) = dir() {
        lower_in(&d);
    }
}

/// 送出時要不要改送原始按鍵。
pub fn wanted() -> bool {
    dir().is_some_and(|d| wanted_in(&d))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每條測試自己一個資料夾——`cargo test` 預設並行，共用一個
    /// 會互相踩（CLAUDE.md 的並行那一條）。
    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tsunagi_keycapture_{name}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn 沒舉旗就不要() {
        let d = tmp("none");
        assert!(!wanted_in(&d), "沒有旗子不該要按鍵");
    }

    #[test]
    fn 舉了就要收了就不要() {
        let d = tmp("cycle");
        raise_in(&d);
        assert!(wanted_in(&d), "舉旗後該要按鍵");
        lower_in(&d);
        assert!(!wanted_in(&d), "收旗後不該再要");
    }

    /// **過期的旗子不算數**：設定頁當掉沒收旗時，別的 app 最多被
    /// 影響 `FRESH` 這麼久。
    #[test]
    fn 過期的旗子不算() {
        let d = tmp("stale");
        raise_in(&d);
        // 直接把修改時間推回去，不要真的等
        let f = std::fs::File::options()
            .write(true)
            .open(d.join(FILE))
            .unwrap();
        f.set_modified(SystemTime::now() - FRESH - Duration::from_secs(1))
            .unwrap();
        drop(f);
        assert!(!wanted_in(&d), "過期的旗子不該算數");
    }

    #[test]
    fn 資料夾不存在也不當機() {
        let d = std::env::temp_dir().join("tsunagi_keycapture_missing");
        let _ = std::fs::remove_dir_all(&d);
        assert!(!wanted_in(&d));
        lower_in(&d);
    }
}
