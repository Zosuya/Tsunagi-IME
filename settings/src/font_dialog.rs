//! 系統的「選擇字型」對話框。
//!
//! # 為什麼用系統對話框而不是自己列清單
//!
//! 自己列的話要 `EnumFontFamiliesExW` 抓幾百項，還得過濾掉 Wingdings
//! 那類符號字型，再自己做搜尋與捲動。系統對話框一行呼叫就有完整清單、
//! 即時預覽與字級，而且使用者早就熟悉它。
//!
//! 代價是它是 Win32 老介面，跟 egui 的長相不一致。Win11 上會自動套
//! 系統主題，所以不會醜得太明顯——使用者已知並接受這個取捨。

use windows::Win32::Graphics::Gdi::{
    GetDC, GetDeviceCaps, ReleaseDC, DEFAULT_CHARSET, FW_NORMAL, LOGFONTW, LOGPIXELSY,
};
use windows::Win32::UI::Controls::Dialogs::{
    ChooseFontW, CF_EFFECTS, CF_INITTOLOGFONTSTRUCT, CF_SCREENFONTS, CHOOSEFONTW,
};

/// 跳出系統字型對話框。回傳 `(字族, 字級)`；使用者取消回 `None`。
///
/// `family` 空字串代表「系統預設」，對話框會停在預設字型上。
pub fn choose(family: &str, size_pt: i32) -> Option<(String, i32)> {
    unsafe {
        let mut lf = LOGFONTW {
            lfCharSet: DEFAULT_CHARSET,
            lfWeight: FW_NORMAL.0 as i32,
            ..Default::default()
        };
        // 點轉邏輯單位：對話框要的是 lfHeight（負數代表字元高度）。
        //
        // **要用實際的螢幕 DPI**，不能寫死 96——`CF_INITTOLOGFONTSTRUCT`
        // 會拿 `lfHeight` 決定打開時預選哪個字級，寫死的話在 125%／150%
        // 的螢幕上會預選到別的大小。
        //
        // （回傳值讀的是 `iPointSize`，那是點數、與 DPI 無關，所以
        // 使用者選完的結果本來就是對的——這裡只影響「開啟時停在哪」。）
        let hdc = GetDC(None);
        let dpi = {
            let d = GetDeviceCaps(Some(hdc), LOGPIXELSY);
            if d > 0 {
                d
            } else {
                96
            }
        };
        lf.lfHeight = -(size_pt * dpi / 72);

        // 字族名塞進 lfFaceName（最多 32 個 UTF-16 字元，含結尾 0）
        for (i, c) in family.encode_utf16().take(31).enumerate() {
            lf.lfFaceName[i] = c;
        }

        let mut cf = CHOOSEFONTW {
            lStructSize: std::mem::size_of::<CHOOSEFONTW>() as u32,
            hDC: hdc,
            lpLogFont: &mut lf,
            // 換算回點數要 *10（對話框的單位是 1/10 點）
            iPointSize: size_pt * 10,
            Flags: CF_SCREENFONTS | CF_INITTOLOGFONTSTRUCT | CF_EFFECTS,
            ..Default::default()
        };

        let ok = ChooseFontW(&mut cf).as_bool();
        if !hdc.is_invalid() {
            ReleaseDC(None, hdc);
        }
        if !ok {
            return None; // 使用者按取消
        }

        // lfFaceName 是定長陣列，尾端補 0——要在第一個 0 截斷
        let name_len = lf
            .lfFaceName
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(lf.lfFaceName.len());
        let name = String::from_utf16_lossy(&lf.lfFaceName[..name_len]);

        // iPointSize 是 1/10 點，除回來
        let size = (cf.iPointSize / 10).clamp(6, 48);
        Some((name, size))
    }
}
