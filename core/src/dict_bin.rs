//! 日文詞庫的**二進位版面**：一整塊 bytes，查詢時零配置、零解析。
//!
//! # 為什麼需要它
//!
//! 原本是 `HashMap<String, Vec<Cand>>`，74.6 萬個讀音、112.9 萬個候選。
//! 真正的文字只有 27.5 MB，但**每一個字串都是一次獨立的堆配置**：
//!
//! | | |
//! |---|---|
//! | 候選陣列本體 | 43.1 MB（112.9 萬 × 40 bytes） |
//! | 雜湊表條目 | 34.1 MB（74.6 萬 × 48 bytes） |
//! | 字串的堆配置次數 | **187.5 萬次** |
//!
//! 量到的常駐是 287 MB，目標 50 MB。包在文字外面的結構是文字的五倍。
//!
//! # 版面
//!
//! 目標是**載入時零解析、零配置**——讀進來就是能查的樣子。
//!
//! ```text
//! 檔頭      magic(8) ver(u16) 保留(u16) n_read(u32) n_cand(u32)
//!           hash_sz(u32) 各區塊位移(u32 × 6)
//! 雜湊表    hash_sz × u32     槽 = 讀音序號 + 1，0 代表空
//! 塊索引    n_blocks × u32    每 16 個讀音一塊，記它在讀音 blob 的位移
//! 讀音 blob 前綴共用           排序後相鄰讀音重疊極高，13.5 MB → 4.9 MB
//! 候選起點  (n_read+1) × u32  讀音 i 的候選是 [start[i], start[i+1])
//! 候選      n_cand × 11 bytes 表記位移(u32) 長度(u8) lid rid cost(u16 ×3)
//! 表記 blob 所有表記貼著排，沒有標頭
//! 有把握    ceil(n_read/8)    位圖：這個讀音的第一個候選夠不夠常用
//! ```
//!
//! # 兩個刻意的取捨
//!
//! **`total` 不存。** 它是 `詞成本 + 句首接續 + 句尾接續`，只有兩件事
//! 用到：排序、決定「有把握」的門檻。兩件都在**產生檔案時**做完，執行
//! 期完全用不到。省 4.5 MB（112.9 萬 × u32）。
//!
//! **雜湊索引，不是二分搜尋。** 雛形用二分搜尋，單次查詢 690ns；量到
//! 平均每次按鍵查詞庫 1267 次，換算 0.87ms。而按鍵延遲最慢一鍵已經
//! 14.7ms、預算 16ms——**餘裕只剩 1.3ms，不夠賭**。雜湊表多花 4 MB
//! 換回 O(1)。

/// 檔案識別碼。改版面就換尾巴那兩碼，舊檔會被認出來而重建。
const MAGIC: &[u8; 8] = b"TSNGJA02";
const VERSION: u16 = 2;

/// 前綴共用的塊大小：每 16 個讀音存一次完整的鍵。
///
/// 塊內第 k 個要往前走 k 步才還原得出來，所以這個數字是「省多少空間」
/// 與「查詢要走幾步」的取捨。16 讓塊索引只佔 0.19 MB，而還原最多 15 步
/// ——每步是一次二十來位元組的複製，量級遠低於一次快取失誤。
const BLOCK: usize = 16;

/// 一筆候選在檔案裡佔幾個位元組：表記位移(4) 長度(1) lid(2) rid(2) cost(2)
const CAND_SIZE: usize = 11;

/// 讀音與表記的長度都用 u8 存，超過就放棄那一筆。
///
/// 實測 mozc 的讀音最長 60 幾位元組、表記更短，離 255 很遠。留這個檢查
/// 是為了**上游資料變了不會靜默壞掉**——寧可少一筆也不要位移全部錯開。
const MAX_LEN: usize = 255;

/// 檔頭大小：magic(8) + ver(2) + 保留(2) + n_read(4) + n_cand(4)
/// + hash_sz(4) + 六個位移(24)
const HEADER: usize = 48;

/// 一個日文候選。**表記是借來的**，指進那塊 bytes，不複製。
///
/// 原本 `surface` 是 `String`（24 bytes 標頭 ＋ 一次堆配置），現在是
/// `&'static str`（16 bytes，不配置）。整個結構從 40 bytes 變 24 bytes，
/// 而且 112.9 萬次堆配置全部消失。
#[derive(Debug, Clone, Copy)]
pub struct Cand {
    pub surface: &'static str,
    /// 左 id：**接在前一個詞後面**時用這個
    pub lid: u16,
    /// 右 id：**後一個詞接在它後面**時用這個
    pub rid: u16,
    /// 詞本身的成本（不含接續）
    pub cost: u16,
}

/// 產生檔案時用的一筆原始資料。
pub struct RawCand {
    pub surface: String,
    pub lid: u16,
    pub rid: u16,
    pub cost: u16,
    /// 詞成本 ＋ 句首接續 ＋ 句尾接續。**只在產生檔案時用**——排序與
    /// 決定「有把握」的門檻，不寫進檔案。
    pub total: u32,
}

/// FNV-1a。選它是因為短鍵快、實作只有幾行、不必引進依賴。
fn hash_of(key: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key {
        h ^= *b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}

/// 雜湊槽數：不小於 `n / 0.72` 的 2 的冪。
///
/// 線性探測在載入因子 0.72 附近平均探測次數還在 2 以下，再低就是拿
/// 記憶體換不到東西了。74.6 萬個讀音落在 2^20 = 104.8 萬槽（4 MB）。
fn hash_slots(n: usize) -> usize {
    let want = (n as f64 / 0.72).ceil() as usize;
    want.next_power_of_two().max(16)
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn get_u32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn get_u16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

/// 索引的四個區塊，`build_*` 組出來之後由呼叫端決定擺在檔案哪裡。
pub struct IndexParts {
    /// 雜湊表：槽存「鍵的序號 + 1」，0 代表空
    pub hash: Vec<u32>,
    /// 每 `BLOCK` 個鍵一塊，記它在鍵 blob 的位移
    pub blocks: Vec<u32>,
    /// 前綴共用過的鍵
    pub keys: Vec<u8>,
    /// 第 i 把鍵的酬載範圍是 `[starts[i], starts[i+1])`
    pub starts: Vec<u32>,
}

impl IndexParts {
    /// 四塊接起來的總位元組數。呼叫端算位移要用。
    pub fn len(&self) -> usize {
        self.hash.len() * 4 + self.blocks.len() * 4 + self.keys.len() + self.starts.len() * 4
    }

    pub fn is_empty(&self) -> bool {
        self.starts.len() <= 1
    }

    /// 依 hash → blocks → keys → starts 的順序寫進 `out`。
    /// **順序必須跟 `IndexRef::new` 的位移計算一致。**
    pub fn write(&self, out: &mut Vec<u8>) {
        for v in &self.hash {
            put_u32(out, *v);
        }
        for v in &self.blocks {
            put_u32(out, *v);
        }
        out.extend_from_slice(&self.keys);
        for v in &self.starts {
            put_u32(out, *v);
        }
    }
}

/// 把一串**已排序**的鍵組成索引。
///
/// `counts[i]` 是第 i 把鍵有幾筆酬載——`starts` 是它的前綴和，讓查詢
/// 時一次算出範圍。鍵必須已排序，前綴共用才有效，雜湊表存的也是排序
/// 後的序號。
pub fn build_index(keys: &[&str], counts: &[u32]) -> IndexParts {
    let n = keys.len();
    let mut blob: Vec<u8> = Vec::new();
    let mut blocks: Vec<u32> = Vec::new();
    let mut prev: &str = "";
    for (i, k) in keys.iter().enumerate() {
        if i % BLOCK == 0 {
            blocks.push(blob.len() as u32);
            blob.push(k.len() as u8);
            blob.extend_from_slice(k.as_bytes());
        } else {
            // 共用長度要落在字元邊界上，不然還原出來的不是合法 UTF-8
            let mut shared = k
                .bytes()
                .zip(prev.bytes())
                .take_while(|(a, b)| a == b)
                .count()
                .min(MAX_LEN);
            while shared > 0 && !k.is_char_boundary(shared) {
                shared -= 1;
            }
            blob.push(shared as u8);
            blob.push((k.len() - shared) as u8);
            blob.extend_from_slice(&k.as_bytes()[shared..]);
        }
        prev = k;
    }

    let slots = hash_slots(n);
    let mut hash = vec![0u32; slots];
    for (i, k) in keys.iter().enumerate() {
        let mut at = (hash_of(k.as_bytes()) as usize) & (slots - 1);
        while hash[at] != 0 {
            at = (at + 1) & (slots - 1);
        }
        hash[at] = i as u32 + 1;
    }

    let mut starts: Vec<u32> = Vec::with_capacity(n + 1);
    let mut acc = 0u32;
    for c in counts {
        starts.push(acc);
        acc += c;
    }
    starts.push(acc);

    IndexParts {
        hash,
        blocks,
        keys: blob,
        starts,
    }
}

/// 查詢端：借用那塊 bytes，用四個位移把鍵找回來。
#[derive(Clone, Copy)]
pub struct IndexRef {
    bytes: &'static [u8],
    n: usize,
    slots: usize,
    off_hash: usize,
    off_blk: usize,
    off_keys: usize,
    off_starts: usize,
}

impl IndexRef {
    /// `at` 是索引四塊的起點、`n` 是鍵的數量、`keys_len` 是鍵 blob 的
    /// 長度（那個長度沒有存進索引本身，得由檔頭帶過來）。
    ///
    /// 回 `None` 代表算出來的範圍超出檔尾——那是壞檔，**不要當成空表
    /// 默默吞掉**，讓呼叫端退回從文字重建。
    pub fn new(bytes: &'static [u8], at: usize, n: usize, keys_len: usize) -> Option<Self> {
        let slots = hash_slots(n);
        let off_hash = at;
        let off_blk = off_hash + slots * 4;
        let off_keys = off_blk + n.div_ceil(BLOCK) * 4;
        let off_starts = off_keys + keys_len;
        let end = off_starts + (n + 1) * 4;
        (end <= bytes.len()).then_some(IndexRef {
            bytes,
            n,
            slots,
            off_hash,
            off_blk,
            off_keys,
            off_starts,
        })
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 還原第 `idx` 把鍵，寫進 `buf`，回傳長度。
    ///
    /// 前綴共用的代價都在這裡：要從所屬那一塊的開頭走過來，最多
    /// `BLOCK - 1` 步。
    fn key_into(&self, idx: usize, buf: &mut [u8; 256]) -> usize {
        let b = idx / BLOCK;
        let mut at = self.off_keys + get_u32(self.bytes, self.off_blk + b * 4) as usize;
        let len = self.bytes[at] as usize;
        at += 1;
        buf[..len].copy_from_slice(&self.bytes[at..at + len]);
        at += len;
        let mut cur = len;
        for _ in 0..(idx % BLOCK) {
            let shared = self.bytes[at] as usize;
            let rest = self.bytes[at + 1] as usize;
            at += 2;
            buf[shared..shared + rest].copy_from_slice(&self.bytes[at..at + rest]);
            at += rest;
            cur = shared + rest;
        }
        cur
    }

    /// 這把鍵排第幾？查不到回 `None`。
    pub fn find(&self, key: &str) -> Option<usize> {
        if self.n == 0 || key.len() > MAX_LEN {
            return None;
        }
        let key = key.as_bytes();
        let mut buf = [0u8; 256];
        let mut at = (hash_of(key) as usize) & (self.slots - 1);
        // 探測次數封頂在槽數，理由同 `KanaDict::find`：每槽都填滿的壞檔
        // 會讓無上限的迴圈把宿主凍住，而那不是 panic、攔不到
        for _ in 0..self.slots {
            let slot = get_u32(self.bytes, self.off_hash + at * 4);
            if slot == 0 {
                return None;
            }
            let idx = slot as usize - 1;
            if idx >= self.n {
                return None;
            }
            let n = self.key_into(idx, &mut buf);
            if &buf[..n] == key {
                return Some(idx);
            }
            at = (at + 1) & (self.slots - 1);
        }
        None
    }

    /// 第 `idx` 把鍵的酬載範圍。
    pub fn range(&self, idx: usize) -> (usize, usize) {
        (
            get_u32(self.bytes, self.off_starts + idx * 4) as usize,
            get_u32(self.bytes, self.off_starts + (idx + 1) * 4) as usize,
        )
    }
}

/// 把「讀音 → 候選」組成二進位版面。
///
/// `entries` 不必先排序，這裡會排——**前綴共用要求鍵有序**，而且雜湊
/// 表存的是排序後的序號。
///
/// 每個讀音的候選會依 `total` 排序並去掉重複的表記，跟原本文字版建表時
/// 做的事一樣；`confident` 判斷第一個候選夠不夠常用。
pub fn build(mut entries: Vec<(String, Vec<RawCand>)>, confident_cost: u32) -> Vec<u8> {
    // **超長的鍵要先丟掉**：長度用 u8 存，`as u8` 會靜默截斷，而那會讓
    // 後面每一筆的位移全部錯開——寧可少一個讀音，也不要整份版面壞掉。
    // 實測 mozc 最長的讀音才 60 幾位元組，這一條平常不會刪到任何東西。
    entries.retain(|(k, _)| k.len() <= MAX_LEN);
    entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    let n_read = entries.len();

    // ── 讀音 blob：每 BLOCK 個存一次完整鍵，其餘只存「跟上一個共用幾個
    //    位元組」加上剩下的部分 ──
    let mut kana_blob: Vec<u8> = Vec::new();
    let mut blk_off: Vec<u32> = Vec::new();
    let mut prev: &str = "";
    for (i, (k, _)) in entries.iter().enumerate() {
        if i % BLOCK == 0 {
            blk_off.push(kana_blob.len() as u32);
            kana_blob.push(k.len() as u8);
            kana_blob.extend_from_slice(k.as_bytes());
        } else {
            // 共用長度要落在字元邊界上，不然還原出來的不是合法 UTF-8
            let mut shared = k
                .bytes()
                .zip(prev.bytes())
                .take_while(|(a, b)| a == b)
                .count()
                .min(MAX_LEN);
            while shared > 0 && !k.is_char_boundary(shared) {
                shared -= 1;
            }
            kana_blob.push(shared as u8);
            kana_blob.push((k.len() - shared) as u8);
            kana_blob.extend_from_slice(&k.as_bytes()[shared..]);
        }
        prev = k;
    }

    // ── 候選與表記 ──
    let mut cand_bytes: Vec<u8> = Vec::new();
    let mut surf_blob: Vec<u8> = Vec::new();
    let mut cstart: Vec<u32> = Vec::with_capacity(n_read + 1);
    // 位圖：每個讀音一位，第一個候選夠不夠常用
    let mut confident = vec![0u8; n_read.div_ceil(8)];
    let mut n_cand = 0u32;
    for (i, (_, cands)) in entries.iter_mut().enumerate() {
        cstart.push(n_cand);
        // **過濾要排在決定「有把握」之前**：那個位元說的是「第 0 個候選
        // 夠常用」，而查詢時拿到的第 0 個是過濾後的。先排序後過濾的話，
        // 萬一第一個因為超長被丟掉，位元就對應到別的表記了
        cands.retain(|c| c.surface.len() <= MAX_LEN);
        cands.sort_by_key(|c| c.total);
        cands.dedup_by(|a, b| a.surface == b.surface);
        if cands.first().is_some_and(|c| c.total <= confident_cost) {
            confident[i / 8] |= 1 << (i % 8);
        }
        for c in cands.iter() {
            put_u32(&mut cand_bytes, surf_blob.len() as u32);
            cand_bytes.push(c.surface.len() as u8);
            cand_bytes.extend_from_slice(&c.lid.to_le_bytes());
            cand_bytes.extend_from_slice(&c.rid.to_le_bytes());
            cand_bytes.extend_from_slice(&c.cost.to_le_bytes());
            surf_blob.extend_from_slice(c.surface.as_bytes());
            n_cand += 1;
        }
    }
    cstart.push(n_cand);

    // ── 雜湊表：槽存「讀音序號 + 1」，0 代表空 ──
    let slots = hash_slots(n_read);
    let mut table = vec![0u32; slots];
    for (i, (k, _)) in entries.iter().enumerate() {
        let mut at = (hash_of(k.as_bytes()) as usize) & (slots - 1);
        while table[at] != 0 {
            at = (at + 1) & (slots - 1);
        }
        table[at] = i as u32 + 1;
    }

    // ── 組檔 ──
    let off_hash = HEADER;
    let off_blk = off_hash + slots * 4;
    let off_kana = off_blk + blk_off.len() * 4;
    let off_cstart = off_kana + kana_blob.len();
    let off_cand = off_cstart + cstart.len() * 4;
    let off_surf = off_cand + cand_bytes.len();
    let off_conf = off_surf + surf_blob.len();

    let mut out = Vec::with_capacity(off_conf + confident.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    put_u32(&mut out, n_read as u32);
    put_u32(&mut out, n_cand);
    put_u32(&mut out, slots as u32);
    for v in [off_blk, off_kana, off_cstart, off_cand, off_surf, off_conf] {
        put_u32(&mut out, v as u32);
    }
    debug_assert_eq!(out.len(), HEADER);
    for v in &table {
        put_u32(&mut out, *v);
    }
    for v in &blk_off {
        put_u32(&mut out, *v);
    }
    out.extend_from_slice(&kana_blob);
    for v in &cstart {
        put_u32(&mut out, *v);
    }
    out.extend_from_slice(&cand_bytes);
    out.extend_from_slice(&surf_blob);
    out.extend_from_slice(&confident);
    out
}

/// 查詢用的門面。**只借用那塊 bytes，自己不持有任何字串。**
pub struct KanaDict {
    bytes: &'static [u8],
    n_read: usize,
    slots: usize,
    off_blk: usize,
    off_kana: usize,
    off_cstart: usize,
    off_cand: usize,
    off_surf: usize,
    off_conf: usize,
}

impl KanaDict {
    /// 認檔頭。版面對不上就回 `None`——呼叫端會退回從文字重建。
    pub fn new(bytes: &'static [u8]) -> Option<Self> {
        if bytes.len() < HEADER || &bytes[..8] != MAGIC {
            return None;
        }
        if get_u16(bytes, 8) != VERSION {
            return None;
        }
        let d = KanaDict {
            bytes,
            n_read: get_u32(bytes, 12) as usize,
            slots: get_u32(bytes, 20) as usize,
            off_blk: get_u32(bytes, 24) as usize,
            off_kana: get_u32(bytes, 28) as usize,
            off_cstart: get_u32(bytes, 32) as usize,
            off_cand: get_u32(bytes, 36) as usize,
            off_surf: get_u32(bytes, 40) as usize,
            off_conf: get_u32(bytes, 44) as usize,
        };
        // 位移必須遞增且不超出檔尾，否則就是壞檔。
        //
        // 光看「遞增」不夠——每一區宣告的**元素數量**也要塞得進它到下一區
        // 的空間，不然 `slots`／`n_read` 灌大之後查詢會切到別區甚至檔尾外。
        // 這個檔案是誰都能寫的資料，而讀它的是每一個宿主行程，檔頭要當
        // 敵意輸入看
        let ok = d.off_blk <= d.off_kana
            && d.off_kana <= d.off_cstart
            && d.off_cstart <= d.off_cand
            && d.off_cand <= d.off_surf
            && d.off_surf <= d.off_conf
            && d.off_conf + d.n_read.div_ceil(8) <= bytes.len()
            && d.slots.is_power_of_two()
            && HEADER + d.slots * 4 <= d.off_blk
            && d.off_blk + d.n_read.div_ceil(BLOCK) * 4 <= d.off_kana
            && d.off_cstart + (d.n_read + 1) * 4 <= d.off_cand
            && d.off_cand + get_u32(bytes, 16) as usize * CAND_SIZE <= d.off_surf;
        ok.then_some(d)
    }

    pub fn len(&self) -> usize {
        self.n_read
    }

    pub fn is_empty(&self) -> bool {
        self.n_read == 0
    }

    /// 還原第 `idx` 個讀音，寫進 `buf`，回傳長度。
    ///
    /// 前綴共用的代價都在這裡：要從所屬那一塊的開頭走過來，最多 15 步。
    fn key_into(&self, idx: usize, buf: &mut [u8; 256]) -> usize {
        let b = idx / BLOCK;
        let mut at = self.off_kana + get_u32(self.bytes, self.off_blk + b * 4) as usize;
        let len = self.bytes[at] as usize;
        at += 1;
        buf[..len].copy_from_slice(&self.bytes[at..at + len]);
        at += len;
        let mut cur = len;
        for _ in 0..(idx % BLOCK) {
            let shared = self.bytes[at] as usize;
            let rest = self.bytes[at + 1] as usize;
            at += 2;
            buf[shared..shared + rest].copy_from_slice(&self.bytes[at..at + rest]);
            at += rest;
            cur = shared + rest;
        }
        cur
    }

    /// 這個讀音排第幾？查不到回 `None`。
    pub fn find(&self, kana: &str) -> Option<usize> {
        if self.n_read == 0 {
            return None;
        }
        let key = kana.as_bytes();
        if key.len() > MAX_LEN {
            return None;
        }
        let mut buf = [0u8; 256];
        let mut at = (hash_of(key) as usize) & (self.slots - 1);
        // 探測次數封頂在槽數：正常檔一定有空槽（載入因子 0.72），但一個
        // 每槽都填滿的壞檔會讓 `loop` 永遠轉下去——那不是 panic，宿主
        // 的 `catch_unwind` 攔不到，症狀是每個宿主按一鍵就凍結
        for _ in 0..self.slots {
            let slot = get_u32(self.bytes, self.off_hash() + at * 4);
            if slot == 0 {
                return None;
            }
            let idx = slot as usize - 1;
            if idx >= self.n_read {
                return None;
            }
            let n = self.key_into(idx, &mut buf);
            if &buf[..n] == key {
                return Some(idx);
            }
            at = (at + 1) & (self.slots - 1);
        }
        None
    }

    fn off_hash(&self) -> usize {
        HEADER
    }

    pub fn contains(&self, kana: &str) -> bool {
        self.find(kana).is_some()
    }

    /// 第 `idx` 個讀音的候選有幾個。
    pub fn cand_count(&self, idx: usize) -> usize {
        let s = get_u32(self.bytes, self.off_cstart + idx * 4) as usize;
        let e = get_u32(self.bytes, self.off_cstart + (idx + 1) * 4) as usize;
        e - s
    }

    /// 第 `idx` 個讀音的第 `k` 個候選。**每次現解，不配置。**
    pub fn cand(&self, idx: usize, k: usize) -> Cand {
        let s = get_u32(self.bytes, self.off_cstart + idx * 4) as usize;
        let at = self.off_cand + (s + k) * CAND_SIZE;
        let so = get_u32(self.bytes, at) as usize;
        let sl = self.bytes[at + 4] as usize;
        let start = self.off_surf + so;
        // 產生檔案時就是從 `&str` 切出來的，這裡切回去一定落在字元邊界
        let surface = std::str::from_utf8(&self.bytes[start..start + sl]).unwrap_or_default();
        Cand {
            surface,
            lid: get_u16(self.bytes, at + 5),
            rid: get_u16(self.bytes, at + 7),
            cost: get_u16(self.bytes, at + 9),
        }
    }

    /// 這個讀音的第一個候選夠常用嗎？（原本 `KANA_BEST` 那張小表）
    pub fn confident(&self, idx: usize) -> bool {
        self.bytes[self.off_conf + idx / 8] & (1 << (idx % 8)) != 0
    }

    /// 走訪某個讀音的所有候選。
    pub fn cands(&self, idx: usize) -> impl Iterator<Item = Cand> + '_ {
        (0..self.cand_count(idx)).map(move |k| self.cand(idx, k))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(surface: &str, total: u32) -> RawCand {
        RawCand {
            surface: surface.to_string(),
            lid: 1,
            rid: 2,
            cost: 3,
            total,
        }
    }

    fn built() -> &'static [u8] {
        let e = vec![
            ("あい".to_string(), vec![raw("愛", 100), raw("藍", 200)]),
            ("あいさつ".to_string(), vec![raw("挨拶", 50)]),
            ("あいさ".to_string(), vec![raw("あいさ", 9999)]),
            ("か".to_string(), vec![raw("蚊", 10)]),
        ];
        Box::leak(build(e, 500).into_boxed_slice())
    }

    #[test]
    fn 查得到而且順序照總成本() {
        let d = KanaDict::new(built()).unwrap();
        assert_eq!(d.len(), 4);
        let i = d.find("あい").unwrap();
        let c: Vec<&str> = d.cands(i).map(|c| c.surface).collect();
        assert_eq!(c, vec!["愛", "藍"], "總成本小的排前面");
    }

    #[test]
    fn 前綴共用還原得回來() {
        // あい／あいさ／あいさつ 三個共用前綴，跨塊邊界也要對
        let d = KanaDict::new(built()).unwrap();
        for k in ["あい", "あいさ", "あいさつ", "か"] {
            assert!(d.contains(k), "{k} 應該查得到");
        }
    }

    #[test]
    fn 查不到的回_none() {
        let d = KanaDict::new(built()).unwrap();
        assert!(d.find("ぬ").is_none());
        assert!(d.find("あいさつだ").is_none());
    }

    #[test]
    fn 有把握的門檻() {
        let d = KanaDict::new(built()).unwrap();
        assert!(d.confident(d.find("あいさつ").unwrap()), "50 <= 500");
        assert!(!d.confident(d.find("あいさ").unwrap()), "9999 > 500");
    }

    #[test]
    fn 欄位原封不動帶過來() {
        let d = KanaDict::new(built()).unwrap();
        let c = d.cand(d.find("か").unwrap(), 0);
        assert_eq!((c.surface, c.lid, c.rid, c.cost), ("蚊", 1, 2, 3));
    }

    #[test]
    fn 超長的鍵要被丟掉而不是截斷() {
        // 長度用 u8 存，截斷的話後面每一筆的位移都會錯開
        let long = "あ".repeat(200); // 600 位元組
        let e = vec![
            (long.clone(), vec![raw("長", 1)]),
            ("か".to_string(), vec![raw("蚊", 10)]),
        ];
        let d = KanaDict::new(Box::leak(build(e, 500).into_boxed_slice())).unwrap();
        assert_eq!(d.len(), 1, "超長的那筆要被丟掉");
        assert!(d.find(&long).is_none());
        assert_eq!(
            d.cand(d.find("か").unwrap(), 0).surface,
            "蚊",
            "其餘不受影響"
        );
    }

    #[test]
    fn 壞檔要被認出來() {
        assert!(KanaDict::new(b"not a dict at all........").is_none());
        let mut bad = built().to_vec();
        bad[9] = 0xff; // 版本號
        assert!(KanaDict::new(Box::leak(bad.into_boxed_slice())).is_none());
    }

    /// 檔頭每個數字都合理、但雜湊表每一槽都填了東西——原本的 `find`
    /// 會永遠轉下去（實測三秒還在跑），而且不是 panic、宿主攔不到。
    #[test]
    fn 每槽都填滿的壞檔不能卡死() {
        let mut bad = built().to_vec();
        let slots = get_u32(&bad, 20) as usize;
        for i in 0..slots {
            let at = HEADER + i * 4;
            bad[at..at + 4].copy_from_slice(&1u32.to_le_bytes());
        }
        let d = KanaDict::new(Box::leak(bad.into_boxed_slice())).unwrap();
        assert!(d.find("ぬ").is_none(), "查不到就是查不到，不能轉不停");
        // 槽裡指到不存在的讀音也一樣
        let mut bad = built().to_vec();
        bad[HEADER..HEADER + 4].copy_from_slice(&999u32.to_le_bytes());
        let d = KanaDict::new(Box::leak(bad.into_boxed_slice())).unwrap();
        let _ = d.find("あい");
        let _ = d.find("ぬ");
    }

    /// 位移遞增但數量灌大：`slots` 或 `n_read` 超過它那一區塞得下的量。
    #[test]
    fn 數量灌大要被認出來() {
        let mut bad = built().to_vec();
        bad[20..24].copy_from_slice(&(1u32 << 30).to_le_bytes()); // slots
        assert!(KanaDict::new(Box::leak(bad.into_boxed_slice())).is_none());
        let mut bad = built().to_vec();
        bad[12..16].copy_from_slice(&(1u32 << 30).to_le_bytes()); // n_read
        assert!(KanaDict::new(Box::leak(bad.into_boxed_slice())).is_none());
        let mut bad = built().to_vec();
        bad[16..20].copy_from_slice(&(1u32 << 30).to_le_bytes()); // n_cand
        assert!(KanaDict::new(Box::leak(bad.into_boxed_slice())).is_none());
    }
}
