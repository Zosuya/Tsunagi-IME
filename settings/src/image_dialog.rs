//! 系統的「選擇圖片」對話框。
//!
//! 理由同 [`crate::font_dialog`]：自己做檔案瀏覽器要處理捲動、縮圖、
//! 我的最愛、網路磁碟機……系統對話框一行呼叫就全有，而且使用者早就熟悉。

use windows::core::PCWSTR;
use windows::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};

/// 跳出系統的開檔對話框選一張圖。使用者取消回 `None`。
pub fn choose() -> Option<String> {
    // **篩選字串的格式很特別**：每一段用 `\0` 隔開，整串用 `\0\0` 收尾。
    // 顯示名稱與副檔名成對出現。
    let filter: Vec<u16> = "圖片檔\0*.png;*.jpg;*.jpeg;*.bmp;*.gif\0所有檔案\0*.*\0\0"
        .encode_utf16()
        .collect();

    // 對話框會把選到的路徑**寫回這個緩衝區**，所以要先備好空間
    let mut buf = vec![0u16; 1024];

    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: windows::core::PWSTR(buf.as_mut_ptr()),
        nMaxFile: buf.len() as u32,
        // 只接受真的存在的檔案——路徑打錯的話當場擋掉，
        // 比之後在輸入法裡靜靜載入失敗好
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST,
        ..Default::default()
    };

    unsafe {
        if !GetOpenFileNameW(&mut ofn).as_bool() {
            return None; // 使用者按取消
        }
    }

    // 緩衝區是定長的，尾端補 0——要在第一個 0 截斷
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let s = String::from_utf16_lossy(&buf[..len]);
    (!s.is_empty()).then_some(s)
}
