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

//! # macOS：一樣是「跟系統要」，只是要的方式不同
//!
//! macOS 沒有 `GetFontData`，但有 `CTFontCopyTable`——**一張表一張表地
//! 要出來，再自己拼回一個完整的字型檔**（`rebuild_sfnt`）。
//!
//! 先試過看起來更簡單的一條：`kCTFontURLAttribute` 問出字型檔的路徑再讀檔。
//! **不行**，而且失敗的方式很陰險：那給的是「檔案」，但一個檔案裡有幾十個
//! 字面，「哪一個」對不回來。實測 `PingFang TC` 的路徑指向 `PingFangUI.ttc`，
//! 集合裡的字面叫 `.PingFangUITextTC-Default`，跟 `fontName()` 回的
//! `PingFangTC-Regular` **根本不同名**——比對一律落空，於是統統退回字面 0
//! （簡體版）。檔案格式完全正常、看起來也像模像樣，只有字形悄悄是錯的。
//!
//! 逐表撈就沒有這個問題：**CoreText 已經解析好是哪一個字面**，撈到的表就是
//! 那個字面的。這跟 Windows 用 GDI 的理由是同一個——讓系統照它自己的規則
//! 挑，我們只負責把資料要出來。
//!
//! 所以 macOS 這條路的 `index` **永遠是 0**：拼出來的是單一字型檔，沒有集合。
//!
//! 為此手宣告了幾支 CoreText／CoreFoundation 的 C 函式，**沒有多拉套件**
//! ——跟 `platform/macos/src/echo_ime.rs` 呼叫 Carbon 的
//! `TISRegisterInputSource` 是同一個做法。

#[cfg(windows)]
mod imp {
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
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::NSFont;
    use objc2_foundation::{NSData, NSString};

    /// 撈到的字型：檔案內容 ＋ 要用裡面第幾個字面。
    pub struct Loaded {
        pub bytes: Vec<u8>,
        /// 字型集合裡的第幾個字面。
        ///
        /// **macOS 這條路永遠是 0**——我們是把表重組成一個單一字型檔，
        /// 本來就沒有集合。欄位留著是因為 Windows 那邊需要。
        pub index: u32,
    }

    // `CTFontRef` 與 `NSFont`、`CFDataRef` 與 `NSData` 都是 toll-free bridged，
    // 指標直接轉就行——這也是為什麼不必拉 `objc2-core-text`。
    //
    // `CFArray` 這裡**不能**當成 `NSArray` 用：`CTFontCopyAvailableTables`
    // 裝進去的元素不是物件，是四位元組的表格標籤直接塞在指標欄位裡。
    // 交給 objc2 的容器會被當成物件去 retain，那會當場爆掉。
    #[link(name = "CoreText", kind = "framework")]
    extern "C" {
        fn CTFontCopyAvailableTables(font: *const AnyObject, options: u32) -> *const AnyObject;
        fn CTFontCopyTable(font: *const AnyObject, tag: u32, options: u32) -> *const AnyObject;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFArrayGetCount(arr: *const AnyObject) -> isize;
        fn CFArrayGetValueAtIndex(arr: *const AnyObject, idx: isize) -> *const std::ffi::c_void;
        fn CFRelease(obj: *const AnyObject);
    }

    pub fn family_font(family: &str) -> Option<Loaded> {
        if family.trim().is_empty() {
            return None;
        }
        // **讓系統照它自己的規則挑**（家族名、PostScript 名、別名都吃），
        // 跟 Windows 用 GDI 挑是同一個道理：使用者填什麼就對到什麼。
        let font = NSFont::fontWithName_size(&NSString::from_str(family), 12.0)?;
        Some(Loaded {
            bytes: rebuild_sfnt(&font)?,
            index: 0,
        })
    }

    /// 把 CoreText 手上那個字型的每一張表撈出來，重組成一個完整的字型檔。
    ///
    /// # 為什麼不是去讀字型檔
    ///
    /// 試過，不行。`kCTFontURLAttribute` 給的是**檔案**，但一個檔案裡可能
    /// 有幾十個字面，而「哪一個」對不回來——實測 `PingFang TC` 的路徑指向
    /// `PingFangUI.ttc`，那個集合裡的字面叫 `.PingFangUITextTC-Default`，
    /// 跟 `fontName()` 回的 `PingFangTC-Regular` 根本不同名。拿名字去比對
    /// 一律落空，於是統統退回字面 0（簡體版），使用者選 TC 卻預覽成 SC。
    ///
    /// 改成逐表撈就沒有這個問題：**CoreText 已經解析好是哪一個字面**，
    /// 我們拿到的表就是那個字面的。這也正是 Windows 那邊用 `GetFontData`
    /// 的理由——讓系統照它自己的規則挑，再把資料整份要出來。
    fn rebuild_sfnt(font: &NSFont) -> Option<Vec<u8>> {
        let f = font as *const NSFont as *const AnyObject;
        let tables = unsafe { CTFontCopyAvailableTables(f, 0) };
        if tables.is_null() {
            return None;
        }
        let n = unsafe { CFArrayGetCount(tables) };
        let mut entries: Vec<(u32, Vec<u8>)> = Vec::with_capacity(n as usize);
        for i in 0..n {
            // 元素本身就是標籤，不是物件（見上面的說明）
            let tag = unsafe { CFArrayGetValueAtIndex(tables, i) } as usize as u32;
            let raw = unsafe { CTFontCopyTable(f, tag, 0) };
            if raw.is_null() {
                continue;
            }
            // Copy 規則：回傳是 +1，交給 `from_raw` 接管
            let Some(data) = (unsafe { Retained::from_raw(raw as *mut NSData) }) else {
                continue;
            };
            entries.push((tag, data.to_vec()));
        }
        unsafe { CFRelease(tables) };
        (!entries.is_empty()).then(|| assemble(entries))
    }

    /// 依 sfnt 的格式把表拼成一個檔案。
    ///
    /// 版面是：12 位元組的檔頭、每張表 16 位元組的目錄、然後是表的內容
    /// （**各自對齊 4 位元組**）。目錄要按標籤遞增排序。
    fn assemble(mut entries: Vec<(u32, Vec<u8>)>) -> Vec<u8> {
        entries.sort_by_key(|(tag, _)| *tag);
        let n = entries.len() as u16;

        // 有 `CFF ` 表的是 PostScript 輪廓，簽章要用 `OTTO`；
        // 其餘是 TrueType 輪廓，簽章是 1.0。認錯的話解析器會直接拒收。
        let has_cff = entries
            .iter()
            .any(|(t, _)| *t == u32::from_be_bytes(*b"CFF "));
        let version: u32 = if has_cff {
            u32::from_be_bytes(*b"OTTO")
        } else {
            0x0001_0000
        };

        // 這三個是二分搜尋用的提示值，照公式算就好（解析器多半不看，
        // 但格式規定要有，填錯的解析器會抱怨）。
        let entry_selector = (15 - n.max(1).leading_zeros() as u16).min(15);
        let search_range = (1u16 << entry_selector) * 16;
        let range_shift = n * 16 - search_range;

        let mut out = Vec::new();
        out.extend_from_slice(&version.to_be_bytes());
        out.extend_from_slice(&n.to_be_bytes());
        out.extend_from_slice(&search_range.to_be_bytes());
        out.extend_from_slice(&entry_selector.to_be_bytes());
        out.extend_from_slice(&range_shift.to_be_bytes());

        let mut offset = 12 + 16 * entries.len() as u32;
        let mut dir = Vec::new();
        for (tag, data) in &entries {
            dir.extend_from_slice(&tag.to_be_bytes());
            dir.extend_from_slice(&checksum(data).to_be_bytes());
            dir.extend_from_slice(&offset.to_be_bytes());
            dir.extend_from_slice(&(data.len() as u32).to_be_bytes());
            offset += data.len().next_multiple_of(4) as u32;
        }
        out.extend_from_slice(&dir);
        for (_, data) in &entries {
            out.extend_from_slice(data);
            out.resize(out.len().next_multiple_of(4), 0);
        }
        out
    }

    /// 表的檢查碼：把內容當成一串大端序的 32 位元整數加起來（溢位就繞回）。
    /// **尾端不足四位元組的部分補零**再算。
    fn checksum(data: &[u8]) -> u32 {
        data.chunks(4).fold(0u32, |acc, c| {
            let mut w = [0u8; 4];
            w[..c.len()].copy_from_slice(c);
            acc.wrapping_add(u32::from_be_bytes(w))
        })
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    /// 撈到的字型：檔案內容 ＋ 要用裡面第幾個字面。
    pub struct Loaded {
        pub bytes: Vec<u8>,
        /// 字型集合裡的第幾個字面。單一字型檔一律 0。
        pub index: u32,
    }

    /// 其他平台**還沒實作**，一律回 `None`。
    pub fn family_font(family: &str) -> Option<Loaded> {
        let _ = family;
        None
    }
}

pub use imp::*;

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    /// 蘋方是 macOS 的系統中文字型，每台機器都有。
    ///
    /// 它同時是這一支最難的案例：**系統把它解析到 `PingFangUI.ttc`**，
    /// 那個集合裡的字面名字跟 `fontName()` 回的對不上（見 `rebuild_sfnt`
    /// 的說明）。所以這條測試守的是「有沒有真的拿到那個字面的資料」。
    #[test]
    fn 蘋方撈得出可用的字型資料() {
        let Some(loaded) = family_font("PingFang TC") else {
            // 找不到就跳過：這條測試依賴系統字型，不該因為換一台
            // 機器就紅（CLAUDE.md 的「靠外部資料的測試」那一條）
            eprintln!("這台機器沒有 PingFang TC，跳過");
            return;
        };
        assert_eq!(loaded.index, 0, "重組出來的是單一字型檔，不是集合");
        // 簽章：TrueType 是 1.0，PostScript 輪廓是 OTTO
        let magic = &loaded.bytes[..4];
        assert!(
            magic == [0, 1, 0, 0] || magic == b"OTTO",
            "簽章不對：{magic:?}"
        );
        // 目錄要自洽：每張表的位置與長度都得落在檔案裡。**格式壞掉的話
        // egui 解析時會 panic，那等於設定頁閃退**，所以這裡先擋。
        let tables = table_dir(&loaded.bytes);
        assert!(!tables.is_empty(), "一張表都沒有");
        for (tag, off, len) in &tables {
            assert!(off + len <= loaded.bytes.len(), "表 {tag} 超出檔案範圍");
        }
        for must in ["cmap", "head", "hmtx"] {
            assert!(tables.iter().any(|(t, _, _)| t == must), "缺 {must} 表");
        }

        // ★ 真正要守的性質 ★
        //
        // 拿到的是不是**那個字面**。原本的 bug 是統統退回集合裡的第一個
        // （簡體版），檔案格式完全正常、看起來也像模像樣——只有名字對得
        // 出來。所以這條斷言比「解析得動」重要。
        let name = postscript_name(&loaded.bytes).expect("重組的字型應該有 name 表");
        assert!(
            name.contains("TC"),
            "選 PingFang TC 卻拿到 {name}——又挑錯字面了"
        );
    }

    /// 讀表格目錄：`(標籤, 位移, 長度)`。
    fn table_dir(d: &[u8]) -> Vec<(String, usize, usize)> {
        let n = u16::from_be_bytes([d[4], d[5]]) as usize;
        (0..n)
            .map(|i| {
                let e = 12 + i * 16;
                let tag = String::from_utf8_lossy(&d[e..e + 4]).to_string();
                let off = u32::from_be_bytes(d[e + 8..e + 12].try_into().unwrap()) as usize;
                let len = u32::from_be_bytes(d[e + 12..e + 16].try_into().unwrap()) as usize;
                (tag, off, len)
            })
            .collect()
    }

    /// 從 name 表撈 PostScript 名字（nameID 6）。
    fn postscript_name(d: &[u8]) -> Option<String> {
        let (_, off, _) = table_dir(d).into_iter().find(|(t, _, _)| t == "name")?;
        let count = u16::from_be_bytes([d[off + 2], d[off + 3]]) as usize;
        let storage = off + u16::from_be_bytes([d[off + 4], d[off + 5]]) as usize;
        for r in 0..count {
            let p = off + 6 + r * 12;
            let platform = u16::from_be_bytes([d[p], d[p + 1]]);
            if u16::from_be_bytes([d[p + 6], d[p + 7]]) != 6 {
                continue;
            }
            let len = u16::from_be_bytes([d[p + 8], d[p + 9]]) as usize;
            let at = storage + u16::from_be_bytes([d[p + 10], d[p + 11]]) as usize;
            let raw = &d[at..at + len];
            return Some(if platform != 1 {
                raw.as_chunks::<2>()
                    .0
                    .iter()
                    .filter_map(|c| char::from_u32(u16::from_be_bytes(*c) as u32))
                    .collect()
            } else {
                raw.iter().map(|&b| b as char).collect()
            });
        }
        None
    }

    /// 空字串＝跟隨系統，不該去撈任何東西。
    #[test]
    fn 空字串回_none() {
        assert!(family_font("").is_none());
        assert!(family_font("   ").is_none());
    }
}
