//! 崩潰隔離：**panic 不得穿過 COM 邊界**。
//!
//! # 為什麼這是當機等級的事
//!
//! 輸入法是**寄生在宿主行程裡的 DLL**。一般程式 panic 是自己當掉，我們
//! panic 是把使用者正在編輯的文件一起帶走——而且 `#[implement]` 生成的
//! COM 方法與 `wndproc` 都是 `extern "system"`，**unwinding 穿過去不是
//! UB 就是直接 abort**，連宿主的例外處理都攔不到。
//!
//! `debug_log::install_panic_hook` 只負責留下線索，不阻止行程結束。
//! 這個模組是另一半：把每個邊界包起來，panic 就地攔下、降級成
//! 「這個鍵我不處理」，交還給宿主。
//!
//! # 降級的原則：寧可不作為，不要作亂
//!
//! 攔下來之後回什麼值，決定使用者看到什麼：
//!
//! | 邊界 | 降級成 | 使用者感受 |
//! |---|---|---|
//! | 按鍵處理 | `BOOL(0)`（沒處理） | 那一鍵原樣送進文件，像沒開輸入法 |
//! | 視窗訊息 | `DefWindowProcW` | 候選視窗行為退回預設 |
//! | 其他 COM | `E_FAIL` | TSF 自己決定怎麼辦 |
//!
//! **絕不回「已處理」**——那會讓按鍵憑空消失，比崩潰更難查。
//!
//! # 為什麼 `AssertUnwindSafe` 在這裡是對的
//!
//! `catch_unwind` 要求閉包 `UnwindSafe`，而我們的狀態在 `Mutex` 裡（內部
//! 可變性），編譯器不可能自己證明。真正要擔心的是「panic 之後狀態半殘，
//! 下一次操作讀到不一致的資料」——那個問題**不是靠型別解決的，是靠
//! `recover` 把狀態重設**。見 `TextService::state` 的中毒處理。

use windows::core::{BOOL, HRESULT};
use windows::Win32::Foundation::{E_FAIL, LRESULT};

/// 把一個「panic 不能穿過去」的邊界包起來。
///
/// `what` 只用在 log 上，出事時要看得出是哪個進入點。
pub fn guard<T>(what: &str, fallback: T, f: impl FnOnce() -> T) -> T {
    // SAFETY 之外的說明見模組註解：`AssertUnwindSafe` 是刻意的，
    // 狀態一致性由呼叫端的重設負責，不是靠這個型別界限
    //
    // **`maybe_panic` 一定要在閉包裡面**——放在 `catch_unwind` 前面的話
    // 它的 panic 直接穿過 COM 邊界，宿主當場 abort。第一版就是這樣寫的，
    // 實測時把記事本帶走了：那個測試工具測不到保護，只示範了沒有保護
    // 會怎樣。
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        maybe_panic(what);
        f()
    })) {
        Ok(v) => v,
        Err(_) => {
            // panic hook 已經把細節寫進 %TEMP%\ime_panic.log，這裡只補
            // 「在哪個邊界攔下的」，那是 hook 看不到的資訊
            crate::debug_log::log(&format!(
                "[攔下 panic] {what} —— 這一次降級處理，細節見 %TEMP%\\ime_panic.log"
            ));
            fallback
        }
    }
}

/// 按鍵處理的邊界：panic 就當成「這個鍵我不處理」。
///
/// 回 `Ok(BOOL(0))` 而不是 `Err`——TSF 對按鍵回傳錯誤的行為各家宿主不
/// 一致，而「沒處理」是每一家都懂的意思：那一鍵原樣送給宿主。
pub fn key<T>(what: &str, f: impl FnOnce() -> windows::core::Result<T>) -> windows::core::Result<T>
where
    T: From<BOOL>,
{
    guard(what, Ok(T::from(BOOL(0))), f)
}

/// 回傳 `Result<()>` 的 COM 方法。
pub fn com(what: &str, f: impl FnOnce() -> windows::core::Result<()>) -> windows::core::Result<()> {
    guard(what, Err(HRESULT(E_FAIL.0).into()), f)
}

/// 視窗程序的邊界：panic 就交給 `DefWindowProcW`。
///
/// **不能回 `LRESULT(0)` 了事**——那對某些訊息代表「我處理了」，會讓
/// 視窗行為變得更怪。交給預設處理才是「不作為」。
pub fn wndproc(
    what: &str,
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
    f: impl FnOnce() -> LRESULT,
) -> LRESULT {
    // 同 `guard`：觸發點必須在閉包裡，不然 panic 穿出去帶走宿主
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        maybe_panic(what);
        f()
    })) {
        Ok(v) => v,
        Err(_) => {
            crate::debug_log::log(&format!(
                "[攔下 panic] {what} msg={msg:#06x} —— 交給 DefWindowProc"
            ));
            // SAFETY: 參數原封不動轉交，跟正常路徑走同一個 API
            unsafe {
                windows::Win32::UI::WindowsAndMessaging::DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }
    }
}

/// 故意 panic 一次，用來驗證上面那些降級路徑真的會走到。
///
/// # 為什麼需要這個
///
/// `guard` 的三條降級路徑寫完之後**從來沒有被真正走過**——單元測試裡
/// 的 `panic!("裝的")` 只證明 `catch_unwind` 會攔，證明不了「宿主不會
/// 一起死」「攔完之後還能繼續打字」。那兩件事只有在真的宿主行程裡
/// 才看得到。
///
/// # 為什麼用 feature 而不是 `debug_assertions`
///
/// 註冊表寫的是 `target\release\` 的**絕對路徑**。debug build 產在
/// `target\debug\`，要測就得提權重新註冊一次——實務上沒人會這樣做，
/// 等於做了也不會用。開 feature 建 release，DLL 還是同一個路徑，
/// 不必碰註冊表。
///
/// # 只能從這個模組內部呼叫
///
/// 刻意不是 `pub`：呼叫點放錯位置（`catch_unwind` 外面）就等於沒有
/// 保護，而那個錯誤在編譯期看不出來，只有真的把宿主弄當才會發現。
/// 統一由 `guard` / `wndproc` 在閉包內部呼叫，外面沒有機會放錯。
///
/// # 觸發方式（one-shot）
///
/// 在 `data/` 建一個 `panic.on`，內容寫要炸的邊界關鍵字：
///
/// ```text
/// OnKeyDown      按鍵處理 → 那一鍵原樣進文件
/// paint          候選視窗繪製 → 這一幀跳過
/// wndproc        視窗訊息 → 交給 DefWindowProc
/// DoEditSession  其他 COM → E_FAIL
/// ```
///
/// 比對用**子字串**，所以寫 `On` 會連 `OnKeyUp`、`OnSetFocus` 一起炸。
///
/// **炸完檔案就自己刪掉**，只發生一次。這是刻意的：要驗的第二件事
/// 正是「攔完之後還能不能繼續打字」，檔案留著會每一鍵都炸，那條就
/// 驗不到了。
#[cfg(feature = "panic-test")]
fn maybe_panic(what: &str) {
    let Some(f) = crate::registration::data_dir().map(|d| d.join("panic.on")) else {
        return;
    };
    let Ok(want) = std::fs::read_to_string(&f) else {
        return;
    };
    let want = want.trim();
    if want.is_empty() || !what.contains(want) {
        return;
    }
    // 先刪再炸——順序反了就永遠刪不掉（panic 之後這行不會執行），
    // 使用者會陷在每一鍵都炸的狀態裡
    let _ = std::fs::remove_file(&f);
    panic!("panic-test：在「{what}」故意炸一次（來自 data/panic.on）");
}

/// 正式版什麼都不做，而且會被整個最佳化掉。
#[cfg(not(feature = "panic-test"))]
#[inline(always)]
fn maybe_panic(_what: &str) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// panic 被攔下來、回傳降級值。
    ///
    /// **這條在擋的是「宿主整個 abort」**——`extern "system"` 的邊界
    /// unwinding 出去不是 UB 就是 abort，使用者正在編輯的文件會一起沒。
    #[test]
    fn panic_被攔下來並降級() {
        let r = guard("測試", 42, || panic!("裝的"));
        assert_eq!(r, 42, "要回降級值，不是往上炸");
    }

    #[test]
    fn 沒有_panic_時原樣回傳() {
        assert_eq!(guard("測試", 42, || 7), 7);
    }

    /// 按鍵的降級一定是「沒處理」，不能是「已處理」。
    ///
    /// 回「已處理」會讓那一鍵**憑空消失**——使用者按了沒反應、字也沒進
    /// 文件，那比當掉更難查。回「沒處理」則是原樣送給宿主，看起來就像
    /// 那一刻沒開輸入法。
    #[test]
    fn 按鍵降級成沒處理() {
        let r: windows::core::Result<BOOL> = key("測試", || panic!("裝的"));
        assert_eq!(r.unwrap(), BOOL(0), "必須是 0（沒處理）");
    }

    #[test]
    fn 按鍵正常時原樣回傳() {
        let r: windows::core::Result<BOOL> = key("測試", || Ok(BOOL(1)));
        assert_eq!(r.unwrap(), BOOL(1));
    }

    #[test]
    fn com_降級成錯誤而不是假裝成功() {
        // 回 Ok 會讓 TSF 以為事情辦好了，狀態就對不上了
        assert!(com("測試", || panic!("裝的")).is_err());
        assert!(com("測試", || Ok(())).is_ok());
    }

    /// 攔一次之後還能再用——`catch_unwind` 不會讓後續呼叫失效。
    ///
    /// 真正會「壞掉就永遠壞著」的是 `Mutex` 中毒，那條在
    /// `text_service::lock_state` 處理。
    #[test]
    fn 攔過之後還能繼續用() {
        assert_eq!(guard("測試", 0, || panic!("裝的")), 0);
        assert_eq!(guard("測試", 0, || 5), 5, "攔過一次不影響下一次");
    }
}
