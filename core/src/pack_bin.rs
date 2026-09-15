//! 官方擴充包的二進位版面：**六層一個檔，直接在 mmap 的位元組上查**。
//!
//! # 為什麼要有它
//!
//! 字串池（§2.83 第一步）把台語包從 30.8MB 壓到 4.8MB，但那 4.8MB
//! **是私有記憶體**——DLL 載在每一個宿主行程裡，開五個就是 24MB。
//!
//! 要拿到跨行程共用只有一條路：mmap。而 mmap 的前提是**結構能直接
//! 在映射的位元組上查詢**——載入時若再轉一份到堆上（`String`／`Vec`／
//! `HashMap`），共用的頁沒人碰，私有的那份照樣每個宿主一份，等於白做。
//!
//! 實測（`spike_pack_bin`，82373 個詞逐筆比對通過）：
//!
//! | | 單一行程私有 | 五個行程私有總計 |
//! |---|---|---|
//! | 文字檔 | 5.29 MB | 27.7 MB |
//! | **mmap `.bin`** | **0.05 MB** | **3.5 MB** |
//!
//! # 誰走這條路
//!
//! **官方發布的語言擴充包**（台語這種）。使用者自己寫的包維持文字檔
//! ——它們隨時要改，而 `.bin` 是唯讀的，那是擴充包編輯器的前提。
//! 界線是「誰發布」不是「多大」，見 §2.83。
//!
//! # 版面
//!
//! ```text
//! 魔術字   8 bytes   "TSNGPACK"
//! 版本     u32
//! 池子長度 u32
//! 攤平數   u32       （值表的項數）
//! 檔頭欄數 u32       （metadata 的項數）
//! 六層各自 u32       （排序表的項數，順序：en ja zh zh_long tw sym）
//! ── 以下連續，順序固定 ──
//! 檔頭表      k × 16 bytes      (名稱 off, len, 值 off, len)
//! 六張排序表  各 n × 16 bytes   (key_off, key_len, span_at, span_len)
//! 值表        m × 8 bytes       (off, len)
//! 池子        UTF-8
//! ```
//!
//! 全部是小端 `u32`。三個目標平台都是 LE，不為理論上的可攜性付轉換
//! 成本（跟 `gen_connection` 同一個理由）。
//!
//! # 為什麼檔頭要進二進位檔
//!
//! `.txt` 的 `# name:` / `# license:` 那幾行**不是說明文字，是授權的
//! 一部分**——台語包的檔頭載明 CC BY-SA 4.0 要求的姓名標示（鄭良偉
//! 教授、楊允言教授與眾義工），拿掉就違反授權。
//!
//! 而設定頁顯示的名稱、版本、作者也都來自那裡。不存進 `.bin` 的話，
//! 官方包在設定頁會顯示成「沒有名稱、沒有版本、沒有授權」，看起來像
//! 作者偷懶沒填——跟 BOM 那個洞（§2.49.3）同樣的症狀。
//!
//! **存成「名稱 → 值」的清單而不是固定欄位**：`Meta` 之後加欄位時
//! 舊的 `.bin` 仍然讀得動（認不得的鍵忽略），跟 `parse_meta` 對文字
//! 檔頭的處理方式一致。
//!
//! # 為什麼六層都是同一個形狀
//!
//! 值一律是「指進值表的一段範圍」：`en` 是長度 0（只有鍵）、`ja`／
//! `zh`／`zh_long` 是長度 1、`sym`／`tw` 是一組。**一種形狀通吃**，
//! 省掉六套版面與六套讀寫程式碼。

/// 檔案開頭的識別字。
pub const MAGIC: &[u8; 8] = b"TSNGPACK";

/// 版面版本。
///
/// **改建表邏輯也要加一**，不只改版面時才加——`.bin` 是衍生檔、
/// 不進版控，舊檔讀得動但行為錯，症狀極難聯想到是檔案舊了。
/// 跟 `dict_bin_zh::VERSION` 同一條規矩。
pub const VERSION: u32 = 1;

/// 六層的順序。**寫檔與讀檔共用這個順序**，不要各寫一份。
pub const LAYERS: usize = 6;

/// header 的長度：魔術字 ＋ 版本 ＋ 池子長度 ＋ 攤平數 ＋ 檔頭欄數
/// ＋ 六層的項數。
const HEAD: usize = 8 + 4 + 4 + 4 + 4 + LAYERS * 4;

/// 排序表一項的大小。
const ROW: usize = 16;

/// 值表一項的大小。
const VAL: usize = 8;

#[inline]
fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// 映射進來的一份官方包。**所有欄位都是借用，沒有任何配置**。
#[derive(Debug)]
pub struct PackBin {
    /// 檔頭：`(名稱, 值)` 的清單，**依寫入順序**不排序。
    ///
    /// 欄位少（七個），線性掃比二分搜尋簡單，而且 `Meta` 只在設定頁
    /// 列清單時讀一次，不是熱路徑。
    meta: &'static [u8],
    /// 六張排序表，各自是位元組切片。順序見 `LAYERS`。
    tables: [&'static [u8]; LAYERS],
    /// 值表。
    vals: &'static [u8],
    /// 字串池。
    pool: &'static str,
}

impl PackBin {
    /// 把映射的位元組解讀成六層。**只算位移，不配置**。
    ///
    /// 認不得就回 `None`——呼叫端退回文字檔那條路。
    pub fn new(bytes: &'static [u8]) -> Option<Self> {
        if bytes.len() < HEAD || &bytes[..8] != MAGIC || u32_at(bytes, 8) != VERSION {
            return None;
        }
        let pool_len = u32_at(bytes, 12) as usize;
        let m = u32_at(bytes, 16) as usize;
        let meta_n = u32_at(bytes, 20) as usize;
        let mut counts = [0usize; LAYERS];
        for (i, c) in counts.iter_mut().enumerate() {
            *c = u32_at(bytes, 24 + i * 4) as usize;
        }

        // 檔頭表排在六張表之前
        let meta_end = HEAD.checked_add(meta_n.checked_mul(ROW)?)?;
        if meta_end > bytes.len() {
            return None;
        }
        let meta = &bytes[HEAD..meta_end];

        // 六張表連續排，逐一切出來
        let mut tables: [&[u8]; LAYERS] = [&[]; LAYERS];
        let mut at = meta_end;
        for (i, &n) in counts.iter().enumerate() {
            let size = n.checked_mul(ROW)?;
            let end = at.checked_add(size)?;
            if end > bytes.len() {
                return None;
            }
            tables[i] = &bytes[at..end];
            at = end;
        }
        let vals_end = at.checked_add(m.checked_mul(VAL)?)?;
        let pool_end = vals_end.checked_add(pool_len)?;
        // **要求剛好相等**——只擋「太小」的話，版面算錯（多寫或少寫一個
        // 欄位）會安靜地讀到錯位的資料。spike 就是這樣踩到的，檔案大了
        // 329KB 才現形（§2.83）
        if bytes.len() != pool_end {
            return None;
        }
        let pool = std::str::from_utf8(&bytes[vals_end..pool_end]).ok()?;
        Some(PackBin {
            meta,
            tables,
            vals: &bytes[at..vals_end],
            pool,
        })
    }

    /// 檔頭的某個欄位。**沒有就回 `None`**。
    ///
    /// 名稱一律小寫（寫檔時就轉好了），跟 `parse_meta` 對文字檔頭的
    /// 處理一致。
    pub fn meta(&self, key: &str) -> Option<&'static str> {
        (0..self.meta.len() / ROW).find_map(|i| {
            let at = i * ROW;
            let k = self.slice(u32_at(self.meta, at), u32_at(self.meta, at + 4));
            (k == key).then(|| self.slice(u32_at(self.meta, at + 8), u32_at(self.meta, at + 12)))
        })
    }

    /// 檔頭有幾個欄位。
    pub fn meta_len(&self) -> usize {
        self.meta.len() / ROW
    }

    /// 從池子切一段字。
    #[inline]
    fn slice(&self, off: u32, len: u32) -> &'static str {
        let a = off as usize;
        &self.pool[a..a + len as usize]
    }

    /// 第 `layer` 層第 `i` 項的鍵。
    #[inline]
    fn key(&self, layer: usize, i: usize) -> &'static str {
        let t = self.tables[layer];
        let at = i * ROW;
        let off = u32_at(t, at) as usize;
        let len = u32_at(t, at + 4) as usize;
        &self.pool[off..off + len]
    }

    /// 第 `layer` 層有幾項。
    #[inline]
    pub fn len(&self, layer: usize) -> usize {
        self.tables[layer].len() / ROW
    }

    /// 第 `layer` 層是空的嗎？
    #[inline]
    pub fn is_empty(&self, layer: usize) -> bool {
        self.tables[layer].is_empty()
    }

    /// 二分搜尋——跟 `Index` 那邊同一套，只是資料在映射的位元組上。
    ///
    /// 回傳值在值表裡的 `(起點, 長度)`。
    fn find(&self, layer: usize, key: &str) -> Option<(usize, usize)> {
        let (mut lo, mut hi) = (0usize, self.len(layer));
        while lo < hi {
            let mid = (lo + hi) / 2;
            match self.key(layer, mid).cmp(key) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => {
                    let at = mid * ROW;
                    let t = self.tables[layer];
                    return Some((u32_at(t, at + 8) as usize, u32_at(t, at + 12) as usize));
                }
            }
        }
        None
    }

    /// 有這個鍵嗎？**不取值**（`en` 那層只需要這個）。
    pub fn has(&self, layer: usize, key: &str) -> bool {
        self.find(layer, key).is_some()
    }

    /// 這個鍵的第一個值。單值層（`ja`／`zh`／`zh_long`）用。
    pub fn first(&self, layer: usize, key: &str) -> Option<&'static str> {
        let (at, len) = self.find(layer, key)?;
        (len > 0).then(|| self.val(at))
    }

    /// 這個鍵有幾個值。**不取值**——段選單的斷詞會從長到短一路試，
    /// 多數落空，不該為了數數就配置一個 `Vec`。
    pub fn count(&self, layer: usize, key: &str) -> usize {
        self.find(layer, key).map_or(0, |(_, len)| len)
    }

    /// 這個鍵的所有值。
    pub fn all(&self, layer: usize, key: &str) -> Vec<&'static str> {
        match self.find(layer, key) {
            Some((at, len)) => (at..at + len).map(|i| self.val(i)).collect(),
            None => Vec::new(),
        }
    }

    /// 走訪一層的所有「鍵 → 值」。**給稽核與 dump 用**，不是熱路徑。
    pub fn iter(
        &self,
        layer: usize,
    ) -> impl Iterator<Item = (&'static str, Vec<&'static str>)> + '_ {
        (0..self.len(layer)).map(move |i| {
            let at = i * ROW;
            let t = self.tables[layer];
            let (s_at, s_len) = (u32_at(t, at + 8) as usize, u32_at(t, at + 12) as usize);
            (
                self.key(layer, i),
                (s_at..s_at + s_len).map(|j| self.val(j)).collect(),
            )
        })
    }

    /// 值表第 `i` 項。
    #[inline]
    fn val(&self, i: usize) -> &'static str {
        let at = i * VAL;
        let off = u32_at(self.vals, at) as usize;
        let len = u32_at(self.vals, at + 4) as usize;
        &self.pool[off..off + len]
    }
}

/// 把檔頭與六層寫成一份 `.bin` 的位元組。
///
/// 每一層是「**已經依鍵的字面排序好**的 (鍵, 值清單)」——排序是查詢
/// 的前提，這裡不重排，由呼叫端保證（`Index` 建出來就是排好的）。
///
/// `meta` 是 `(欄位名, 值)`，**名稱要先轉小寫**（查詢端直接比字面）。
/// 順序照傳進來的樣子保留——文字檔頭是有順序的，`name` 通常在最前面。
///
/// 回傳 `None` 代表資料超出 `u32` 能表達的範圍（池子 4GB、或項數
/// 超過 42 億）。實務上碰不到——台語包 9 萬筆才 613KB 的池子——但
/// **靜靜截斷比回錯誤糟得多**，所以明確擋下來。
pub fn write(
    meta: &[(String, String)],
    layers: &[Vec<(String, Vec<String>)>; LAYERS],
) -> Option<Vec<u8>> {
    let mut pool = String::new();
    let mut seen: std::collections::HashMap<String, (u32, u32)> = std::collections::HashMap::new();
    // 同一個字串只進池子一次。台語的雙向索引讓每個詞平均出現 3.7 次，
    // 不去重的話池子會膨脹成三倍多
    let mut put = |s: &str, pool: &mut String| -> Option<(u32, u32)> {
        if let Some(&p) = seen.get(s) {
            return Some(p);
        }
        let off = u32::try_from(pool.len()).ok()?;
        let len = u32::try_from(s.len()).ok()?;
        pool.push_str(s);
        seen.insert(s.to_string(), (off, len));
        Some((off, len))
    };

    // 檔頭先進池子——它的字串通常最短，排在前面讓位移數字小一點
    let mut meta_table: Vec<u8> = Vec::with_capacity(meta.len() * ROW);
    for (k, v) in meta {
        let (k_off, k_len) = put(k, &mut pool)?;
        let (v_off, v_len) = put(v, &mut pool)?;
        meta_table.extend_from_slice(&k_off.to_le_bytes());
        meta_table.extend_from_slice(&k_len.to_le_bytes());
        meta_table.extend_from_slice(&v_off.to_le_bytes());
        meta_table.extend_from_slice(&v_len.to_le_bytes());
    }

    let mut tables: Vec<Vec<u8>> = Vec::with_capacity(LAYERS);
    let mut vals: Vec<u8> = Vec::new();
    let mut m = 0u32;
    for rows in layers {
        let mut t: Vec<u8> = Vec::with_capacity(rows.len() * ROW);
        for (key, values) in rows {
            let (k_off, k_len) = put(key, &mut pool)?;
            let at = m;
            for v in values {
                let (off, len) = put(v, &mut pool)?;
                vals.extend_from_slice(&off.to_le_bytes());
                vals.extend_from_slice(&len.to_le_bytes());
                m = m.checked_add(1)?;
            }
            t.extend_from_slice(&k_off.to_le_bytes());
            t.extend_from_slice(&k_len.to_le_bytes());
            t.extend_from_slice(&at.to_le_bytes());
            t.extend_from_slice(&u32::try_from(values.len()).ok()?.to_le_bytes());
        }
        tables.push(t);
    }

    let total = HEAD
        + meta_table.len()
        + tables.iter().map(Vec::len).sum::<usize>()
        + vals.len()
        + pool.len();
    let mut out: Vec<u8> = Vec::with_capacity(total);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&u32::try_from(pool.len()).ok()?.to_le_bytes());
    out.extend_from_slice(&m.to_le_bytes());
    out.extend_from_slice(&u32::try_from(meta.len()).ok()?.to_le_bytes());
    for (rows, _) in layers.iter().zip(&tables) {
        out.extend_from_slice(&u32::try_from(rows.len()).ok()?.to_le_bytes());
    }
    out.extend_from_slice(&meta_table);
    for t in &tables {
        out.extend_from_slice(t);
    }
    out.extend_from_slice(&vals);
    out.extend_from_slice(pool.as_bytes());
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一份六層都有東西的檔，測 round-trip。
    /// 一份典型的檔頭。**照台語包的樣子**——它是實際會走這條路的包。
    fn meta_sample() -> Vec<(String, String)> {
        let s = |x: &str| x.to_string();
        vec![
            (s("name"), s("台語")),
            (s("version"), s("0.2")),
            (s("readonly"), s("true")),
            (s("license"), s("CC BY-SA 4.0")),
        ]
    }

    fn sample() -> [Vec<(String, Vec<String>)>; LAYERS] {
        let s = |x: &str| x.to_string();
        [
            // en：只有鍵沒有值
            vec![(s("api"), vec![]), (s("sdk"), vec![])],
            // ja：單值
            vec![(s("ほろらいぶ"), vec![s("ホロライブ")])],
            // zh：單值
            vec![(s("su3cl3"), vec![s("今天")])],
            // zh_long：單值
            vec![(s("ji3"), vec![s("您好，很高興認識您")])],
            // tw：一組
            vec![
                (s("沙發"), vec![s("沙發"), s("膨椅")]),
                (s("膨椅"), vec![s("沙發"), s("膨椅")]),
            ],
            // sym：一組
            vec![(s("音樂"), vec![s("♪"), s("♫")])],
        ]
    }

    /// 寫出去再讀回來，六層都要一模一樣。
    #[test]
    fn 寫出去讀回來六層都對() {
        let data = sample();
        let bytes = write(&meta_sample(), &data).expect("寫得出來");
        let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
        let p = PackBin::new(leaked).expect("讀得回來");

        assert!(p.has(0, "api"), "en 認得");
        assert!(!p.has(0, "nope"), "en 不認得的要回 false");
        assert_eq!(p.first(1, "ほろらいぶ"), Some("ホロライブ"));
        assert_eq!(p.first(2, "su3cl3"), Some("今天"));
        assert_eq!(p.first(3, "ji3"), Some("您好，很高興認識您"));
        assert_eq!(p.all(4, "膨椅"), vec!["沙發", "膨椅"], "台語雙向");
        assert_eq!(p.count(4, "沙發"), 2);
        assert_eq!(p.count(4, "不存在"), 0, "查不到要回 0 不是 panic");
        assert_eq!(p.all(5, "音樂"), vec!["♪", "♫"]);
    }

    /// **每一層的項數要對得上**——版面算錯的話這裡先掛。
    #[test]
    fn 六層的項數都對() {
        let bytes = write(&meta_sample(), &sample()).unwrap();
        let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
        let p = PackBin::new(leaked).unwrap();
        assert_eq!(
            (0..LAYERS).map(|i| p.len(i)).collect::<Vec<_>>(),
            vec![2, 1, 1, 1, 2, 1]
        );
    }

    /// 空的層不能把別層的位移算歪。
    #[test]
    fn 某一層是空的也要讀得回來() {
        let s = |x: &str| x.to_string();
        let mut data = sample();
        data[0] = vec![]; // en 整層清空
        data[4] = vec![];
        let bytes = write(&meta_sample(), &data).unwrap();
        let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
        let p = PackBin::new(leaked).unwrap();
        assert!(p.is_empty(0));
        assert!(p.is_empty(4));
        assert_eq!(p.first(2, "su3cl3"), Some("今天"), "後面的層沒被推歪");
        assert_eq!(p.all(5, &s("音樂")), vec!["♪", "♫"]);
    }

    /// **截斷的檔案要認不得，不是讀到半套**。
    ///
    /// 這條守的是最危險的失敗模式：下載中斷、磁碟滿。讀到錯位的
    /// 資料不會報錯，只會安靜地給出亂七八糟的候選。
    #[test]
    fn 截斷的檔案要拒絕() {
        let bytes = write(&meta_sample(), &sample()).unwrap();
        for cut in [0, 4, 8, 20, HEAD, bytes.len() - 1] {
            let part: &'static [u8] = Box::leak(bytes[..cut].to_vec().into_boxed_slice());
            assert!(PackBin::new(part).is_none(), "截到 {cut} bytes 應該認不得");
        }
        // **多出來的也要擋**——只擋「太小」的話版面算錯會安靜讀錯位
        let mut longer = bytes.clone();
        longer.push(0);
        let longer: &'static [u8] = Box::leak(longer.into_boxed_slice());
        assert!(PackBin::new(longer).is_none(), "多一個 byte 也不該接受");
    }

    /// 魔術字或版本不對就認不得。
    #[test]
    fn 認不得的檔案要拒絕() {
        let bytes = write(&meta_sample(), &sample()).unwrap();

        let mut bad = bytes.clone();
        bad[0] = b'X';
        let bad: &'static [u8] = Box::leak(bad.into_boxed_slice());
        assert!(PackBin::new(bad).is_none(), "魔術字不對");

        let mut old = bytes.clone();
        old[8] = VERSION as u8 + 1;
        let old: &'static [u8] = Box::leak(old.into_boxed_slice());
        assert!(PackBin::new(old).is_none(), "版本不對");
    }

    /// 走訪一層拿得到全部。
    #[test]
    fn 走訪一層() {
        let bytes = write(&meta_sample(), &sample()).unwrap();
        let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
        let p = PackBin::new(leaked).unwrap();
        let got: Vec<_> = p.iter(4).collect();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].0, "沙發");
        assert_eq!(got[0].1, vec!["沙發", "膨椅"]);
    }

    /// **檔頭要原封不動回來**——授權的姓名標示在那裡面，掉了就違反授權。
    #[test]
    fn 檔頭寫得進去也讀得回來() {
        let bytes = write(&meta_sample(), &sample()).unwrap();
        let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
        let p = PackBin::new(leaked).unwrap();
        assert_eq!(p.meta_len(), 4);
        assert_eq!(p.meta("name"), Some("台語"));
        assert_eq!(p.meta("version"), Some("0.2"));
        assert_eq!(p.meta("license"), Some("CC BY-SA 4.0"));
        assert_eq!(p.meta("readonly"), Some("true"));
        assert_eq!(p.meta("author"), None, "沒寫的欄位要回 None");
    }

    /// **沒有檔頭的包也要讀得回來**——檔頭是選填的。
    #[test]
    fn 沒有檔頭也要讀得回來() {
        let bytes = write(&[], &sample()).unwrap();
        let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
        let p = PackBin::new(leaked).unwrap();
        assert_eq!(p.meta_len(), 0);
        assert_eq!(p.meta("name"), None);
        // 檔頭是空的不能把後面六層的位移算歪
        assert_eq!(p.first(2, "su3cl3"), Some("今天"));
        assert_eq!(p.all(4, "沙發"), vec!["沙發", "膨椅"]);
    }

    /// **同一個字串只進池子一次**——台語的雙向索引靠這個省下三倍空間。
    #[test]
    fn 重複的字串只存一份() {
        let s = |x: &str| x.to_string();
        // 「沙發」出現四次（兩個鍵各含它一次、自己當鍵一次）
        let data: [Vec<(String, Vec<String>)>; LAYERS] = [
            vec![],
            vec![],
            vec![],
            vec![],
            vec![
                (s("沙發"), vec![s("沙發"), s("膨椅")]),
                (s("膨椅"), vec![s("沙發"), s("膨椅")]),
            ],
            vec![],
        ];
        // **檔頭留空**——不然池子長度會被檔頭的字串撐大，測不準
        let bytes = write(&[], &data).unwrap();
        let pool_len = u32_at(&bytes, 12) as usize;
        // 「沙發」「膨椅」各 6 bytes，去重之後池子就是 12
        assert_eq!(pool_len, 12, "重複的字串沒去重，池子會是 36");
    }
}
