//! panic 不得穿過 ObjC 的邊界。
//!
//! # 為什麼一定要包
//!
//! unwinding 穿過 `extern "C"` 會直接 **abort**（見開發文件 §2.52.3），
//! 而 `define_class!` 展開出來的每個方法都是 ObjC runtime 直接呼叫的
//! `extern "C"` 函式。沒包的話，任何一個 panic 都會讓整個輸入法行程死掉。
//!
//! # 跟 Windows 那邊的差別
//!
//! `platform/windows/src/guard.rs` 包了 13 個 COM／wndproc 邊界，目的是
//! **保護宿主**——DLL 載在宿主行程裡，panic 會帶垮記事本。
//!
//! macOS 的輸入法是獨立行程，panic 不會弄壞宿主，系統還會自動重啟它。
//! 但**使用者正在打的那串字會整個不見**。所以規矩一樣，目的變成
//! 「保住組字狀態」。
//!
//! # ★ 但這不代表 macOS 版弄不當宿主 ★
//!
//! **這支攔得住的只有我們自己的 panic。** 傳錯參數給 IMK／AppKit 的 API
//! 一樣會弄當宿主——而且是在**宿主行程裡**崩，`catch_unwind` 碰都碰不到。
//! 實測踩過兩次（開發文件 §2.52.21）：`setMarkedText:` 傳純 `NSString`、
//! 以及 repertoire 亂宣稱 `Latn` 讓 `ASCIICapable` 變 1，兩次都把 Terminal
//! 弄當。
//!
//! 所以規矩有兩條，這支只管第一條：
//!
//! 1. **我們的程式碼不准 panic 出去**——這支
//! 2. **送給系統的東西一定要合格**——沒有任何機制擋得住，只能靠寫的時候小心
//!
//! # 回傳值要給什麼
//!
//! 攔下來之後一律回「沒處理」——把按鍵讓回給宿主，使用者至少還打得出
//! 字。回「處理了」會讓按鍵消失得無聲無息。

/// 包住一個會被 ObjC 直接呼叫的函式。panic 就回 `fallback`。
pub fn catch<T>(what: &str, fallback: T, f: impl FnOnce() -> T) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(v) => v,
        Err(_) => {
            // 不能再 panic，也不該吞掉——輸入法沒有主控台，寫 stderr
            // 至少 Console.app 看得到。
            eprintln!("[通譯] {what} 裡發生 panic，已攔下，按鍵讓回宿主");
            fallback
        }
    }
}

#[cfg(test)]
mod tests {
    use super::catch;

    #[test]
    fn 正常時原封不動回傳() {
        assert!(catch("t", false, || true));
    }

    #[test]
    fn panic_時回_fallback_而不是往外炸() {
        // 這一則就是這個模組存在的理由：真的讓它 panic 一次，
        // 確認拿得回 fallback。
        let got = catch("t", false, || -> bool { panic!("故意的") });
        assert!(!got, "panic 之後要回 fallback（沒處理），把按鍵讓回宿主");
    }
}
