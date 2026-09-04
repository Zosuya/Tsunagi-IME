use std::cell::RefCell;

use windows::core::{implement, Result};
use windows::Win32::UI::TextServices::{ITfEditSession, ITfEditSession_Impl};

/// 把一個 Rust closure 包成 TSF 要求的 `ITfEditSession`。
///
/// TSF 的讀寫操作（插入文字、修改組字範圍…）都必須在 `RequestEditSession`
/// 核發的 edit cookie（`ec`）範圍內執行，本專案一律用 `TF_ES_SYNC`，
/// 所以 closure 會在 `RequestEditSession`呼叫當下就同步跑完。
/// 拿到 edit cookie 之後要跑的那段程式。
type EditCallback = Box<dyn FnMut(u32) -> Result<()>>;

#[implement(ITfEditSession)]
pub struct EditSession {
    callback: RefCell<Option<EditCallback>>,
}

impl EditSession {
    pub fn new(callback: impl FnMut(u32) -> Result<()> + 'static) -> Self {
        Self {
            callback: RefCell::new(Some(Box::new(callback))),
        }
    }
}

impl ITfEditSession_Impl for EditSession_Impl {
    /// **panic 不能穿過這裡**：這是 TSF 回呼進來的 COM 方法，而回呼裡跑
    /// 的正是組字與核心引擎的邏輯——最可能 panic 的地方。攔下來回
    /// `E_FAIL`，TSF 會當成這次編輯沒成功，使用者頂多是那一鍵沒生效。
    fn DoEditSession(&self, ec: u32) -> Result<()> {
        crate::guard::com("DoEditSession", || {
            if let Some(f) = self.callback.borrow_mut().as_mut() {
                f(ec)
            } else {
                Ok(())
            }
        })
    }
}
