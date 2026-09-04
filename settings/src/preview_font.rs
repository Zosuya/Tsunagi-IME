//! 把使用者選的字型真的載進來，預覽才會跟著換。
//!
//! # 為什麼需要這一步
//!
//! 設定頁自己的介面用的是微軟正黑體（見 `install_cjk_font`），跟使用者
//! 幫輸入法選的字型是兩回事。不做這件事的話，換字型之後預覽區長得
//! 一模一樣——**預覽就失去意義了**。
//!
//! # 為什麼用 GDI 撈而不是找檔案
//!
//! 「字型名稱 → 檔案路徑」沒有直接的對應：登錄檔裡的鍵名長得像
//! `Microsoft JhengHei & Microsoft JhengHei UI (TrueType)`，要自己拆
//! 字串比對，遇到別名與替換規則就不準了。
//!
//! `GetFontData` 是反過來做：**讓 GDI 照它自己的規則挑好字型，再把
//! 那份資料整個要出來**。使用者在字型對話框看到的是什麼，這裡拿到的
//! 就是什麼。
//!
//! # 字型集合（.ttc）這個坑
//!
//! Windows 的中文字型幾乎都是 `.ttc`——一個檔案裡裝好幾個字面
//! （msjh.ttc 裡有「微軟正黑體」與「微軟正黑體 UI」）。
//!
//! 對集合裡的字面呼叫 `GetFontData(表格 = 0)`，拿到的東西**看起來像
//! 一個完整的字型檔，其實不是**：開頭的簽章是對的，但表格目錄裡的
//! 偏移量指向的是「整個集合」裡的位置。單獨拿出來就是壞的，餵給
//! egui 解析時會 panic——**那等於設定頁整個閃退**。
//!
//! 正確做法是用 `ttcf` 這個標籤要整份集合，再告訴 egui 要用第幾個
//! 字面（`FontData::index`）。這裡順便驗證結構，撈到怪東西寧可退回
//! 預設字型也不要冒險。

use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateFontIndirectW, DeleteDC, DeleteObject, GetFontData, SelectObject,
    DEFAULT_CHARSET, GDI_ERROR, HDC, LOGFONTW,
};

/// 撈到的字型：檔案內容 ＋ 要用裡面第幾個字面。
pub struct Loaded {
    pub bytes: Vec<u8>,
    /// 字型集合裡的第幾個字面。單一字型檔一律 0。
    pub index: u32,
}

/// `ttcf` 標籤。`GetFontData` 的標籤要**反過來排**，所以用 `from_le_bytes`。
const TAG_TTCF: u32 = u32::from_le_bytes(*b"ttcf");

/// 取得這個字型家族的資料。撈不到或結構不對就回 `None`。
pub fn family_font(family: &str) -> Option<Loaded> {
    unsafe {
        let hdc = CreateCompatibleDC(None);
        if hdc.is_invalid() {
            return None;
        }
        let mut lf = LOGFONTW {
            lfCharSet: DEFAULT_CHARSET,
            ..Default::default()
        };
        // lfFaceName 是定長陣列，超過就截斷（LF_FACESIZE 是 32，含結尾 0）
        for (i, c) in family.encode_utf16().take(31).enumerate() {
            lf.lfFaceName[i] = c;
        }
        let hfont = CreateFontIndirectW(&lf);
        if hfont.is_invalid() {
            let _ = DeleteDC(hdc);
            return None;
        }
        let old = SelectObject(hdc, hfont.into());

        // 被選中那個字面的 sfnt。**是集合的話這份是壞的**，只拿來
        // 比對它是集合裡的第幾個。
        let face = font_data(hdc, 0);
        let collection = font_data(hdc, TAG_TTCF);

        SelectObject(hdc, old);
        let _ = DeleteObject(hfont.into());
        let _ = DeleteDC(hdc);

        // 是集合：用整份，並找出正確的字面索引
        if let Some(coll) = collection {
            let index = face
                .as_deref()
                .and_then(|f| ttc_index_of(&coll, f))
                .unwrap_or(0);
            return ttc_face_ok(&coll, index).then_some(Loaded { bytes: coll, index });
        }

        // 不是集合：那份就是完整的字型檔
        let f = face?;
        sfnt_ok(&f, 0).then_some(Loaded { bytes: f, index: 0 })
    }
}

/// 向 GDI 要一份字型資料。要不到回 `None`。
unsafe fn font_data(hdc: HDC, table: u32) -> Option<Vec<u8>> {
    unsafe {
        let size = GetFontData(hdc, table, 0, None, 0);
        if size == GDI_ERROR as u32 || size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        let got = GetFontData(
            hdc,
            table,
            0,
            Some(buf.as_mut_ptr() as *mut std::ffi::c_void),
            size,
        );
        (got != GDI_ERROR as u32).then_some(buf)
    }
}

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// 這個位置是不是一份結構完整的字型？
///
/// **重點是每個表格都要落在檔案範圍內**。集合裡的字面單獨拿出來時，
/// 偏移量指向的是整個集合的位置，會超出這份資料的長度——正是這一項
/// 攔得下來，而只看開頭簽章的話攔不到。
fn sfnt_ok(b: &[u8], base: usize) -> bool {
    let Some(tag) = u32_at(b, base) else {
        return false;
    };
    // 0x00010000 = TrueType、OTTO = CFF、true/typ1 = 舊 Mac
    if !matches!(tag, 0x0001_0000 | 0x4F54_544F | 0x7472_7565 | 0x7479_7031) {
        return false;
    }
    let Some(n) = u16_at(b, base + 4) else {
        return false;
    };
    if n == 0 || n > 512 {
        return false;
    }
    (0..n as usize).all(|i| {
        let rec = base + 12 + i * 16;
        match (u32_at(b, rec + 8), u32_at(b, rec + 12)) {
            (Some(off), Some(len)) => (off as usize).saturating_add(len as usize) <= b.len(),
            _ => false,
        }
    })
}

/// 集合裡第 `index` 個字面的表格目錄在哪。
fn ttc_face_offset(b: &[u8], index: u32) -> Option<u32> {
    if b.get(0..4)? != b"ttcf" {
        return None;
    }
    let n = u32_at(b, 8)?;
    if index >= n {
        return None;
    }
    u32_at(b, 12 + index as usize * 4)
}

/// 集合裡第 `index` 個字面的結構完整嗎？
fn ttc_face_ok(b: &[u8], index: u32) -> bool {
    match ttc_face_offset(b, index) {
        Some(off) => sfnt_ok(b, off as usize),
        None => false,
    }
}

/// `face`（被選中那份、偏移量指向集合的 sfnt）是集合裡的第幾個？
///
/// 兩邊的表格目錄記的是**同一組絕對偏移量**，比第一筆就分得出來。
fn ttc_index_of(coll: &[u8], face: &[u8]) -> Option<u32> {
    let want = u32_at(face, 20)?; // 第一筆表格記錄的 offset 欄位
    let n = u32_at(coll, 8)?;
    (0..n).find(|i| {
        ttc_face_offset(coll, *i)
            .and_then(|off| u32_at(coll, off as usize + 20))
            .is_some_and(|got| got == want)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 撈到的東西結構完整嗎？集合的話驗那個字面，否則驗開頭。
    fn 完整(f: &Loaded) -> bool {
        if f.bytes.starts_with(b"ttcf") {
            ttc_face_ok(&f.bytes, f.index)
        } else {
            sfnt_ok(&f.bytes, 0)
        }
    }

    /// **這是這個模組最危險的一段**：拿到壞資料會讓 egui 解析時 panic，
    /// 等於設定頁閃退。常見的中文字型都是 `.ttc`，正是踩到坑的那一種。
    #[test]
    fn 微軟正黑體撈得出完整結構() {
        let Some(f) = family_font("Microsoft JhengHei") else {
            eprintln!("這台機器沒有微軟正黑體，跳過");
            return;
        };
        assert!(f.bytes.len() > 10_000, "太小：{} bytes", f.bytes.len());
        assert!(完整(&f), "結構驗證沒過（index {}）", f.index);
    }

    /// 細明體也是集合，而且字面順序跟微軟正黑體不同，多驗一種。
    #[test]
    fn 細明體也撈得出完整結構() {
        let Some(f) = family_font("MingLiU") else {
            eprintln!("這台機器沒有細明體，跳過");
            return;
        };
        assert!(完整(&f), "結構驗證沒過（index {}）", f.index);
    }

    #[test]
    fn 亂打的字型名稱也不會回傳壞資料() {
        // GDI 會替換成預設字型，所以通常撈得到東西——但一定要是好的
        if let Some(f) = family_font("這個字型不存在12345") {
            assert!(完整(&f), "替換來的字型也該是完整的");
        }
    }

    #[test]
    fn 偏移量超出範圍的擋得下來() {
        // 模擬「集合裡的字面單獨拿出來」：簽章對、表格數對，但偏移量
        // 指到檔案外面——**這正是造成閃退的那種資料**，而只看開頭的
        // 簽章檢查放它過關了
        let mut b = vec![0u8; 28];
        b[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        b[4..6].copy_from_slice(&1u16.to_be_bytes()); // numTables = 1
        b[20..24].copy_from_slice(&99_999u32.to_be_bytes()); // offset 遠超長度
        b[24..28].copy_from_slice(&100u32.to_be_bytes());
        assert!(!sfnt_ok(&b, 0), "偏移量超出範圍就該擋下來");
    }

    #[test]
    fn 明顯的垃圾擋得下來() {
        assert!(!sfnt_ok(b"", 0));
        assert!(!sfnt_ok(b"short", 0));
        assert!(!sfnt_ok(&[0xFF; 64], 0));
    }
}
