//! 三種詞典的按鍵層查詢。
//!
//! 排序要問的是「這一段是不是一個詞」，而切法拿在手上的是**按鍵串**，
//! 不是文字。所以詞典得建成「按鍵串 → 是不是詞」的形式。
//!
//! | 詞典 | 原始格式 | 轉換 |
//! |---|---|---|
//! | 注音 | `一一九 ㄧ ㄧ ㄐㄧㄡˇ` | 注音符號 → 按鍵（用 `keymap` 反查） |
//! | 日文 | `ああると<TAB>…<TAB>アアルト` | 羅馬字 → 假名（`romaji::kana`） |
//! | 英文 | `you 28787591` | 直接用（見 `crate::english`） |
//!
//! # 日文詞典只能當平手判準，不能當門檻
//!
//! mozc 的詞典存的是**辭書形**，活用形不在裡面——
//! `teishutsushinakereba`（提出しなければ）查不到。
//! 要求「日文段一定要在詞典裡」的話，mixed_japanese_verbs 會從
//! 100% 掉到 0%。見 `cutpoint::rank` 的 `dict_chars`。

use crate::bopomofo::keymap;
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

/// 注音詞庫：多字詞表與同音字表，**一整塊二進位 bytes**，見 `dict_bin_zh`。
///
/// **兩張表是不同的鍵空間**：同音字表的鍵是單一音節，詞表的鍵是兩個
/// 音節以上，兩者不重複。
///
/// **注音只有這一張多字詞表**。以前還有一份 `BOPOMOFO: HashSet<String>`
/// 專門回答「這串按鍵是不是注音詞」，但兩者在同一個迴圈的同一個分支裡
/// 各插入一次同樣的鍵（還為此多做一次 `clone`）——實測 132050 條全部
/// 都查得到這張表，一條不差。「在不在」看這張表有沒有這把鍵就好。
///
/// 原本是兩張 `HashMap<String, _>`，13.2 萬個鍵值各自一次堆配置，常駐
/// 23MB、建表峰值再多 21MB，而資料本身只有 2.3MB。見 `dict_bin_zh`。
static ZH: OnceLock<Option<crate::dict_bin_zh::ZhDict>> = OnceLock::new();

/// 把一個唯讀資料檔的內容拿成 `&'static [u8]`。
///
/// # 為什麼要 mmap
///
/// 輸入法的 DLL 載在**每一個宿主行程**裡——檔案總管（工作列的輸入指示
/// 器）永遠挖著它，再加瀏覽器、編輯器、Word。二進位詞庫有 57MB 是檔案
/// 內容，讀進 `Vec` 的話每個行程各背一份：五個行程 345MB。mmap 讓它們
/// **共用同一份實體記憶體**，而且只有真正碰過的頁算進 RSS。
///
/// 對「打字不要卡」還有一層好處：唯讀檔案頁在記憶體壓力下可以直接丟棄
/// 重讀，不必寫進交換區——換頁停頓是最糟的那種卡。
///
/// # 為什麼 leak 是對的
///
/// 映射要活著，切出去的 `&[u8]` 才有效。詞庫本來就活到行程結束，所以
/// leak 一次是**刻意的**，不是漏——跟 `intern` 那個一次性的道理相同，
/// 不會隨使用增加。
///
/// # 兩層退路
///
/// mmap 失敗（檔案系統不支援、權限、被鎖）就退回 `fs::read`；連讀都失
/// 敗才回 `None`，由呼叫端退回從文字重建。關掉 `mmap` feature 時整段
/// 只剩 `fs::read`，行為一樣、只是不共用。
fn map_file(path: &Path) -> Option<&'static [u8]> {
    #[cfg(feature = "mmap")]
    {
        if let Ok(f) = std::fs::File::open(path) {
            // SAFETY: mmap 的風險是「映射期間檔案被外部改掉」——那會讓
            // 已經借出去的位元組在腳下變動，是 UB。
            //
            // 對策是**產生端一律先寫暫存檔再改名**（`gen_dict_ja`／
            // `gen_dict_zh`／`gen_connection` 都是）：改名是原子的，會換
            // 成新的 inode，已經映射的舊內容原封不動活到行程結束。所以
            // 重新產生詞庫**不會**動到正在打字的那些行程看到的位元組。
            //
            // 使用者自己拿別的工具就地覆寫那些檔仍然是危險的，但那些是
            // 衍生資料、位置在 data/ 底下、文件說明是「用腳本產生」，
            // 跟手改設定檔不是同一類行為。
            if let Ok(m) = unsafe { memmap2::Mmap::map(&f) } {
                let m: &'static memmap2::Mmap = Box::leak(Box::new(m));
                return Some(&m[..]);
            }
        }
    }
    let bytes = std::fs::read(path).ok()?;
    Some(Box::leak(bytes.into_boxed_slice()))
}

/// 寫一個會被 `map_file` 映射的資料檔：**先寫暫存檔，再原子改名**。
///
/// # 為什麼不能直接覆寫
///
/// `map_file` 的 SAFETY 就靠這一條。就地覆寫會動到**正在打字的那些宿主
/// 行程已經映射的位元組**——借出去的 `&[u8]` 在腳下變動是 UB。改名是
/// 原子的、會換成新的 inode，舊的映射原封不動活到那些行程結束。
///
/// 附帶一個好處：中途失敗（磁碟滿、斷電）不會留下半個檔，要嘛是舊的
/// 完整版、要嘛是新的完整版。
///
/// 暫存檔跟目標**放在同一個目錄**——跨檔案系統改不了名。
pub fn write_data_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    // Windows 的 rename 在目標已存在時會失敗，所以先移開舊的。
    // `fs::rename` 在 POSIX 上本來就會覆蓋，多這一步不影響。
    let _ = std::fs::remove_file(path);
    std::fs::rename(&tmp, path)
}

/// 注音版面載好了嗎？拿得到才查。
fn zh() -> Option<&'static crate::dict_bin_zh::ZhDict> {
    ZH.get().and_then(|d| d.as_ref())
}
/// 日文詞庫：假名 → 表記，**一整塊二進位 bytes**，見 `dict_bin`。
///
/// **日文只有這一張表**。以前還有一份 `JAPANESE: HashSet<String>` 專門
/// 回答「這串假名是不是日文詞」，但它的內容就是這張表的鍵——實測兩邊
/// 都是 745965 條且逐條相同。等於同一份資料的雜湊做了兩次，佔載入時間
/// 的 37%（1129ms 裡的 417ms）。「在不在」直接看這張表有沒有這把鍵就好。
///
/// 原本是 `HashMap<String, Vec<Cand>>`，74.6 萬個讀音要 187.5 萬次堆
/// 配置、常駐 287MB。改成二進位版面之後查詢時零配置，見 `dict_bin` 的
/// 模組說明。「有把握的預設表記」原本是另一張 `KANA_BEST` 小表，現在是
/// 版面裡的一個位圖。
static JA: OnceLock<Option<crate::dict_bin::KanaDict>> = OnceLock::new();

/// 一個日文候選。定義在 `dict_bin`——它的 `surface` 是**借來的**，
/// 指進那塊 bytes。
///
/// # 為什麼要留 id 與成本
///
/// 單詞轉換只要「哪個表記最便宜」，所以原本算完總成本就把 id 丟掉了。
/// 但**整句轉換要同時決定分詞與選字**，而詞跟詞之間接不接得起來是靠
/// `連接矩陣[前一個詞的 rid][後一個詞的 lid]`——沒有 id 就算不出來。
///
/// `total`（詞成本＋句首＋句尾接續）**不存在執行期**：它只有排序與
/// 決定「有把握」的門檻用得到，兩件都在產生版面時做完。
pub use crate::dict_bin::Cand;

/// 詞庫的版本號：每載完一本就加一。
///
/// # 為什麼需要這個
///
/// 詞庫是**背景載入**的，而切點引擎會把「這一段合不合法」的判斷
/// 快取起來。在詞庫載好之前算出來的答案，載好之後就不對了——沒有
/// 這個版本號的話，那些錯答案會一直留著，症狀是「剛切過去打的前
/// 幾個字切得怪怪的，重打一次又正常」。
///
/// 拿它當快取的有效期：版本變了就整份丟掉重算。
static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 目前的詞庫版本。快取要拿它比對，見 `GENERATION`。
pub fn generation() -> u64 {
    GENERATION.load(std::sync::atomic::Ordering::Relaxed)
}

/// 載完一本詞庫，版本加一。
pub(crate) fn bump_generation() {
    GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// 注音符號 → 按鍵的反查表。
///
/// `keymap` 只有「按鍵→符號」的方向，這裡把它反過來。
pub(crate) fn reverse_keymap() -> HashMap<char, char> {
    let mut rev = HashMap::new();
    // 大千鍵盤用到的所有鍵
    for k in "1qaz2wsxedcrfv5tgbyhnujm8ik,9ol.0p;/- 3467".chars() {
        if let Some(sym) = keymap::symbol_of(k) {
            rev.entry(sym).or_insert(k);
        }
    }
    rev
}

/// 載入注音詞典。多次呼叫只讀一次檔。
///
/// 來源是 `BPMFMappings.txt`，格式是「詞 注音1 注音2 …」。
/// 一聲在資料裡不標符號，但打字要按空白，所以轉換時要補上。
pub fn load_bopomofo(data_dir: &Path) -> Option<&'static crate::dict_bin_zh::ZhDict> {
    let first = ZH.get().is_none();
    ZH.get_or_init(|| {
        // 有現成的版面就直接讀。認不得就當作沒有，退回從文字重建
        let path = data_dir.join("bopomofo").join("dict_zh.bin");
        if let Some(bytes) = map_file(&path) {
            if let Some(d) = crate::dict_bin_zh::ZhDict::new(bytes) {
                return Some(d);
            }
        }
        let raw = build_zh_layout(data_dir)?;
        let leaked: &'static [u8] = Box::leak(raw.into_boxed_slice());
        crate::dict_bin_zh::ZhDict::new(leaked)
    });
    if first {
        bump_generation();
    }
    zh()
}

/// 從 `BPMFBase.txt`／`BPMFMappings.txt` 組出注音的二進位版面。
///
/// **這也是 `gen_dict_zh` 產生檔案時走的同一條路**——版面只有一種產生
/// 方式，不會出現「檔案版跟現建版不一樣」這種難查的問題。
///
/// 字頻、詞頻、偏好表、讀音別字頻這四份**只有這裡要用**：它們決定同音
/// 字的順序與詞的挑選，決定完就沒事了。改成讀 `.bin` 之後執行期根本不
/// 會載它們——注音載入的峰值比常駐高 21MB，那 21MB 就是這幾張表。
/// 建表用的詞表：**一把按鍵 → 這個讀音的所有詞**，各帶排序分數。
///
/// 「城市」與「程式」讀音完全相同，一鍵只留一個的話分數低的那個在建表
/// 時就被丟掉（實測 13,175 個詞）。分數見 `word_score`。
type WordTable = HashMap<String, Vec<(String, (u8, u64))>>;

pub fn build_zh_layout(data_dir: &Path) -> Option<Vec<u8>> {
    {
        let rev = reverse_keymap();
        // 選詞模組要的兩張表
        let char_freq = load_char_freq(data_dir);
        let word_freq = load_word_freq(data_dir);
        let priority = load_priority(data_dir, &rev);
        let reading_share = load_reading_share(data_dir);
        let mut chars: HashMap<String, Vec<(String, u32)>> = HashMap::new();
        // 值是**這個讀音的所有詞**，各帶排序分數（見下方 `word_score`）。
        //
        // **一個鍵存得下好幾個詞**——「城市」與「程式」讀音完全相同
        // （ㄔㄥˊㄕˋ）。舊版只留分數最高的那個，實測**丟掉了 13,175
        // 個詞**（8% 的按鍵串有兩個以上的詞）。詞層查不到「程式」，
        // 選字時選了「程」，「市」也就無從變成「式」。
        let mut words: WordTable = HashMap::new();
        // 字 → 這個字的所有讀音按鍵。輕聲詞的本調別名要靠它，見下方
        let mut readings: HashMap<char, Vec<String>> = HashMap::new();

        // ── 單字表 BPMFBase.txt ──
        //
        // BPMFMappings 只收多字詞，單字在這個檔。第 4 欄已經是按鍵序列
        // （`,4`、`-3`），不必轉換；但一聲那欄省略了空白（`ㄡ` 寫成 `.`），
        // 要補回來。
        let base = data_dir.join("bopomofo").join("BPMFBase.txt");
        if let Ok(content) = std::fs::read_to_string(&base) {
            for line in content.lines() {
                let f: Vec<&str> = line.split_whitespace().collect();
                let (Some(text), Some(bopomofo), Some(keys)) = (f.first(), f.get(1), f.get(3))
                else {
                    continue;
                };
                let mut keys = (*keys).to_string();
                if !bopomofo.chars().any(|c| "ˊˇˋ˙".contains(c)) {
                    keys.push(' ');
                }
                // 字頻沒有「讀音」這個維度——「吃」的次數幾乎全來自
                // ㄔ（吃飯），卻讓它在 ㄐㄧˊ（口吃）底下壓過「及」「集」。
                // 乘上讀音佔比等於「依這個字念這個音的機率打折」。查不到
                // 的字視為 1000‰（不打折），排序與加這張表之前一致。
                let ch = text.chars().next();
                let base = ch.and_then(|c| char_freq.get(&c)).copied().unwrap_or(0);
                let share = ch
                    .map(|c| format!("{c}{bopomofo}"))
                    .and_then(|k| reading_share.get(&k).copied())
                    .unwrap_or(1000);
                let freq = base.saturating_mul(share);
                if let Some(c) = ch {
                    readings.entry(c).or_default().push(keys.clone());
                }
                chars
                    .entry(keys)
                    .or_default()
                    .push(((*text).to_string(), freq));
            }
        }

        // ── 多字詞表 BPMFMappings.txt ──
        let path = data_dir.join("bopomofo").join("BPMFMappings.txt");
        let Ok(content) = std::fs::read_to_string(&path) else {
            // 多字詞表讀不到就整份沒有。沿用舊行為——這個檔不在的話
            // 詞庫本來就是壞的，給半套（只有單字）比給空的更難查
            return None;
        };
        // 含輕聲音節的詞，留著等主迴圈跑完再補別名——**別名不能比真詞
        // 早進表**，不然後面才讀到的真詞會被 `or_insert` 擋在門外
        let mut pending: Vec<(String, Vec<String>)> = Vec::new();
        for line in content.lines() {
            let mut it = line.split_whitespace();
            let Some(word) = it.next() else { continue };
            let mut keys = String::new();
            let mut syls: Vec<String> = Vec::new();
            let mut ok = true;
            for syllable in it {
                let at = keys.len();
                for sym in syllable.chars() {
                    match rev.get(&sym) {
                        Some(k) => keys.push(*k),
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
                if !ok {
                    break;
                }
                // 一聲不標符號，但打字要按空白
                if !syllable.chars().any(|c| "ˊˇˋ˙".contains(c)) {
                    keys.push(' ');
                }
                syls.push(keys[at..].to_string());
            }
            if ok && !keys.is_empty() {
                if syls.iter().any(|k| k.ends_with('7')) {
                    pending.push((word.to_string(), syls));
                }
                let score = word_score(word, &word_freq, &char_freq);
                let e = words.entry(keys).or_default();
                // 同一個詞可能有多列（不同的變調寫法），只留一次
                if !e.iter().any(|(w, _)| w == word) {
                    e.push((word.to_string(), score));
                }
            }
        }

        // ── 輕聲詞的本調別名 ──
        //
        // 語料標的是**這個字怎麼念**，使用者打的是**字典音**。「一個」
        // 「孩子」在語料裡標輕聲是對的（口語確實念輕聲），但沒有人會
        // 為了打「個」去按輕聲鍵。於是 `5k4ek4` 查不到「這個」，詞層
        // 一落空就**整個詞逐字重排**——而被拖垮的常常不是那個輕聲字：
        //
        // ```text
        // 孩子 c96y7 → 孩子      c96y3 → 還子   ← 壞在「孩」，不是「子」
        // ```
        //
        // **新酷音的做法是同一個詞收兩份讀音**，本調那份給頻率 1
        // ——查得到，但不競爭排名（`這個,1,ㄓㄜˋ ㄍㄜˋ` 與
        // `這個,25909,ㄓㄜˋ ㄍㄜ˙` 並存）。它的詞庫有 11.5% 的條目
        // 頻率是 1，那一整層就是別名。
        //
        // 我們一個鍵只放一個詞，等價做法是**只填空位**：別名鍵上已經
        // 有真正的詞就不動它。這樣別名永遠不會排擠到誰。
        //
        // **不改 `BPMFMappings.txt`**——那是上游資料，改了下次重下就
        // 沒了（`en_50k` 已經咬過一次）。加在建表這一層，`.bin` 與
        // 文字備援兩條路都會有。
        //
        // **一次只換一個音節**。第一版寫成整串 `replace`，詞裡有兩個
        // 「個」而輕重不同時（`一個個` ㄧ ㄍㄜ˙ ㄍㄜˋ）就生不出逐位置
        // 的變體，實測漏掉 9 條。
        //
        // **全部的輕聲字都做，不挑名單。**用引擎跑完 1397 條候選
        // （判準是「打本調還原得出原詞嗎」）：不補是 715 條壞掉，
        // 只補前四名（子 440、麼 144、得 43、夫 26）剩 50 條，
        // **全做剩 0 條**。而全做只比挑名單多 67 個詞條、1.1KB 檔案、
        // 常駐 9KB——連一頁記憶體都填不滿（`.bin` 是 mmap 的），
        // 三支計分器兩者都零變動。成本量出來等於零就沒有挑的理由。
        {
            for (word, syls) in &pending {
                let cs: Vec<char> = word.chars().collect();
                if cs.len() != syls.len() {
                    continue; // 字數與音節數對不上，位置對不起來就別猜
                }
                for (i, k) in syls.iter().enumerate() {
                    if !k.ends_with('7') {
                        continue;
                    }
                    // 同樣聲介韻、不同聲調的讀音才算——`ㄍㄜ˙`→`ㄍㄜˋ`
                    // 可以，`ㄍㄜ˙`→`ㄍㄨˇ` 不行
                    let stem = &k[..k.len() - 1];
                    for k2 in readings.get(&cs[i]).into_iter().flatten() {
                        if k2 == k || k2.len() != k.len() || &k2[..k2.len() - 1] != stem {
                            continue;
                        }
                        // **上游資料有錯的列**：`公 ㄍㄨㄥ˙ gong5 ej/5`
                        // ——按鍵欄該是 `ej/7`，卻寫成拼音的調號 `5`。
                        // `5` 在大千是ㄓ，`ej/5` 根本打不出來，照收只會
                        // 生出永遠查不到的死條目。合法聲調鍵只有這五個。
                        if !matches!(k2.chars().next_back(), Some(' ' | '3' | '4' | '6' | '7')) {
                            continue;
                        }
                        let alias: String = syls
                            .iter()
                            .enumerate()
                            .map(|(j, s)| if j == i { k2.as_str() } else { s.as_str() })
                            .collect();
                        // **一鍵多詞之後別名不必再「只填空位」**：分數給
                        // 最低，排在真詞後面就好。`gp6ai6`（ㄕㄣˊㄇㄛˊ）
                        // 上面有「神魔」，補「什麼」不會改變預設，只是
                        // 多一個選得到的入口。
                        let e = words.entry(alias).or_default();
                        if !e.iter().any(|(w, _)| w == word) {
                            e.push((word.clone(), (0, 0)));
                        }
                    }
                }
            }
        }

        // 同音字依字頻排序——`su3` 有 29 個字，字頻讓「你」排在
        // 「儗」「旎」前面
        let chars_out: Vec<(String, Vec<(String, u32)>)> = {
            chars
                .into_iter()
                .map(|(k, mut v)| {
                    v.sort_by_key(|(_, f)| std::cmp::Reverse(*f));
                    v.dedup_by(|a, b| a.0 == b.0);
                    // 偏好表列的依序搬到最前面，其餘維持字頻順序
                    if let Some(wanted) = priority.get(&k) {
                        for w in wanted.iter().rev() {
                            if let Some(i) = v.iter().position(|(c, _)| c == w) {
                                let c = v.remove(i);
                                v.insert(0, c);
                            }
                        }
                    }
                    // **分數留著**：單字學習要拿它跟 `k^N` 相乘，
                    // 不然「學過就贏」等於 libchewing 那條被否決的曲線。
                    // 見 `best_char_for`。
                    (k, v)
                })
                .collect()
        };
        // 偏好表凌駕詞頻——使用者已經表態，統計不該再有意見。
        //
        // **可以指定詞庫沒收的詞**：「這部」「這不」兩個都不在
        // BPMFMappings 裡，打 `5k41j4` 的「這不」是逐字選出來的。這種
        // 情況只能由偏好表補進來，不然無論怎麼調排序都出不來。
        //
        // 守門是**字數要跟音節數對得上**——選詞層一格一個字地填回去
        // （見 `compose::apply_word_context`），對不上的詞填不進去，
        // 而且多半代表表裡打錯了。
        for (keys, wanted) in &priority {
            let Some(syllables) = crate::bopomofo::split_syllables(keys) else {
                continue;
            };
            // 單音節的由 CHARS 的排序處理，不進詞表——選詞層只查跨兩格
            // 以上的按鍵，單音節放進來也不會被查到
            if syllables.len() < 2 {
                continue;
            }
            let Some(w) = wanted.iter().find(|w| w.chars().count() == syllables.len()) else {
                continue;
            };
            // **偏好表凌駕一切，排到最前面**；原本就有的詞留在後面，
            // 使用者仍然選得到。分數給最大值，排序時自然在最前。
            let e = words.entry(keys.clone()).or_default();
            e.retain(|(x, _)| x != w);
            e.insert(0, (w.clone(), (u8::MAX, u64::MAX)));
        }
        // 依分數排序後用 `SEP` 接成一段——版面沒變，只是那一段裡有
        // 好幾個詞。`ZhDict::word` 仍回第一個，`words` 回全部。
        let words_out: Vec<(String, String)> = words
            .into_iter()
            .map(|(k, mut v)| {
                v.sort_by_key(|(_, sc)| std::cmp::Reverse(*sc));
                let joined = v
                    .into_iter()
                    .map(|(w, _)| w)
                    .collect::<Vec<_>>()
                    .join(&crate::dict_bin_zh::SEP.to_string());
                (k, joined)
            })
            .collect();
        Some(crate::dict_bin_zh::build(words_out, chars_out))
    }
}

/// 把一串注音符號轉成按鍵，音節之間補上一聲空白。
///
/// `priority.txt` 的注音是**連寫**的（`ㄕˊㄗㄨㄛˋ`），跟
/// `BPMFMappings.txt` 用空白分隔不一樣，所以要自己找音節邊界。
///
/// 邊界靠**角色順序**判斷：一個音節裡的符號一定是
/// 聲母 → 介音 → 韻母 → 聲調 的順序，各出現至多一次。下一個符號的
/// 角色沒有往後走（跟前一個相同或更前面），就是新音節開始了。
///
/// 一聲在書寫時不標符號但打字要按空白，所以換音節或收尾時，
/// 前一個音節沒有聲調就補一個空白。
pub(crate) fn symbols_to_keys(symbols: &str, rev: &HashMap<char, char>) -> Option<String> {
    use crate::bopomofo::keymap::Role;
    let mut out = String::new();
    let mut last: Option<Role> = None;
    for sym in symbols.chars() {
        let key = *rev.get(&sym)?;
        let (_, role) = keymap::lookup(key)?;
        // 角色沒往後走 = 新音節；先把前一個音節收掉
        let new_syllable = match last {
            Some(prev) => role as u8 <= prev as u8,
            None => false,
        };
        if new_syllable && last != Some(Role::Tone) {
            out.push(' '); // 前一個音節是一聲
        }
        out.push(key);
        last = Some(role);
    }
    last?; // 空字串沒有音節
    if last != Some(Role::Tone) {
        out.push(' '); // 最後一個音節是一聲
    }
    Some(out)
}

/// 人工排序調整表：按鍵 → 要排到最前面的候選（依序）。
///
/// # 為什麼需要它
///
/// 詞頻與字頻都是**統計**，統計必然有偏差。`word_freq.txt` 混了字幕
/// 語料，於是「上船」(13411) 贏過「上傳」(2332)——2018 年的字幕裡
/// 沒人在講上傳檔案。這種個案沒辦法靠調演算法解決，只能人工指定。
///
/// 格式是 `注音 候選1 候選2 …`，注音可以是單一音節（調同音字的順序）
/// 或多音節連寫（指定同音詞選哪個）。兩者用同一套機制——查詢表的
/// key 本來就是完整的按鍵序列。
///
/// **這張表凌駕詞頻**：使用者已經表態了，統計不該再有意見。
fn load_priority(data_dir: &Path, rev: &HashMap<char, char>) -> HashMap<String, Vec<String>> {
    let mut out = HashMap::new();
    let path = data_dir.join("bopomofo").join("priority.txt");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return HashMap::new();
    };
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.split_whitespace();
        let Some(symbols) = it.next() else { continue };
        let Some(keys) = symbols_to_keys(symbols, rev) else {
            continue;
        };
        let wanted: Vec<String> = it.map(|s| s.to_string()).collect();
        if wanted.is_empty() {
            continue;
        }
        // 同一個注音出現兩次就接起來（檔案裡 ㄗㄞˋ、ㄋㄥˊ 都出現兩次）
        out.entry(keys).or_insert_with(Vec::new).extend(wanted);
    }
    out
}

/// 同音詞誰勝出？回傳可比較的排序分數。
///
/// # 為什麼是 (層級, 值) 而不是一個數字
///
/// 兩張表的**單位不同**，混在一起比會錯得離譜：詞頻是「每百萬詞頻
/// ×1000」（希望 796851），字頻是「在語料庫裡出現幾次」（希 905）。
/// 直接比大小等於拿公里跟英里相加。
///
/// 所以分層：**詞頻表收錄的一律贏過沒收錄的**，同層之內才比數值。
/// 詞頻表有 19 萬詞，涵蓋常用詞綽綽有餘；不在裡面的多半是罕用詞，
/// 排在後面本來就對。同層之內沒收錄的仍用舊的字頻代理當次要依據，
/// 至少有個順序，不會退化成隨機。
fn word_score(
    word: &str,
    word_freq: &HashMap<String, u64>,
    char_freq: &HashMap<char, u32>,
) -> (u8, u64) {
    match word_freq.get(word) {
        Some(f) => (1, *f),
        // 詞頻表沒收：退回「組成字的最低字頻」——一個詞的常用程度
        // 受限於它最生僻的那個字
        None => (
            0,
            word.chars()
                .filter_map(|c| char_freq.get(&c))
                .copied()
                .min()
                .unwrap_or(0) as u64,
        ),
    }
}

/// 詞頻表：詞 → 每百萬詞頻×1000。來源是 `word_freq.txt`
/// （國教院＋教育部＋字幕語料合併，見 `data/download.ps1`）。
///
/// # 為什麼需要它
///
/// `BPMFMappings.txt` 沒有詞頻欄位，同音詞誰勝出只能另外找依據。
/// 原本用「組成字的最低**字**頻」當替代品，但那個代理量測會錯：
/// 「班」的字頻（841）比「般」（751）高，於是 `u 10 ` 給「一班」
/// 而不是「一般」；「策士」的最低字頻比「測試」高，於是打 `hk4g4`
/// 得到「策士」。字頻不是詞頻，常用字組不出常用詞。
///
/// 檔案不在就回空表——那時整個排序退回原本的字頻代理，不會壞掉。
fn load_word_freq(data_dir: &Path) -> HashMap<String, u64> {
    let mut out = HashMap::new();
    let path = data_dir.join("bopomofo").join("word_freq.txt");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return HashMap::new();
    };
    for line in content.lines() {
        let mut it = line.split_whitespace();
        let (Some(w), Some(n)) = (it.next(), it.next()) else {
            continue;
        };
        let Ok(n) = n.parse::<u64>() else { continue };
        out.insert(w.to_string(), n);
    }
    out
}

/// 字頻表：字 → 次數。來源是教育部字頻總表。
fn load_char_freq(data_dir: &Path) -> HashMap<char, u32> {
    let mut out = HashMap::new();
    let path = data_dir.join("bopomofo").join("char_freq.txt");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return HashMap::new();
    };
    for line in content.lines() {
        let mut it = line.split_whitespace();
        let (Some(w), Some(n)) = (it.next(), it.next()) else {
            continue;
        };
        let (Some(c), Ok(n)) = (w.chars().next(), n.parse::<u32>()) else {
            continue;
        };
        out.insert(c, n);
    }
    out
}

/// 讀音別字頻的佔比表：`(字, 注音)` → 千分比。
///
/// 由 `gen_reading_freq` 產生（見該檔的說明）。查不到就當 1000‰，
/// 所以資料檔缺席時行為會退回「只看字頻」，不會壞掉。
///
/// key 是字與注音直接串起來（`吃ㄐㄧˊ`）——兩者的字元集不重疊，
/// 串起來不會撞號，比 `(char, String)` 的 tuple 少一次配置。
fn load_reading_share(data_dir: &Path) -> HashMap<String, u32> {
    let mut out = HashMap::new();
    let path = data_dir.join("bopomofo").join("char_freq_by_reading.txt");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return out;
    };
    for line in content.lines() {
        if line.starts_with('#') {
            continue;
        }
        let mut it = line.split_whitespace();
        let (Some(c), Some(r), Some(n)) = (it.next(), it.next(), it.next()) else {
            continue;
        };
        let Ok(n) = n.parse::<u32>() else {
            continue;
        };
        out.insert(format!("{c}{r}"), n);
    }
    out
}

/// 這個注音音節有哪些同音字？依字頻排序。
///
/// 選詞模組用它列候選——`su3` 回傳「你 妳 擬 禰 …」29 個字。
/// 這個音節**有字可以顯示嗎**？
///
/// `chars_for` 會配置一整個 `Vec`，切點排序只想知道「有沒有」——
/// 那是熱路徑，不能為了一個布林值配置字串向量。
pub fn has_chars(syllable: &str) -> bool {
    zh().is_some_and(|d| d.has_chars(syllable))
}

pub fn chars_for(syllable: &str) -> Vec<String> {
    let mut v: Vec<(&str, u64)> = zh()
        .map(|d| {
            d.chars(syllable)
                .map(|(c, f)| (c, weighted(syllable, c, f)))
                .collect()
        })
        .unwrap_or_default();
    // 沒學過任何東西時順序不變（權重一律等於原分數），排序是穩定的
    if crate::learn::any() {
        v.sort_by_key(|(_, w)| std::cmp::Reverse(*w));
    }
    v.into_iter().map(|(c, _)| c.to_string()).collect()
}

/// 這個音節最可能是哪個字？**借用不複製。**
///
/// `chars_for().first()` 會把整份候選 clone 一次，而這支在熱路徑上
/// （每一鍵、每個候選切法的每個音節各一次）——跟 `best_kana_word`
/// 是同一個道理。
pub fn best_char_for(syllable: &str) -> Option<&'static str> {
    let d = zh()?;
    if !crate::learn::any() {
        return d.chars(syllable).next().map(|(c, _)| c);
    }
    d.chars(syllable)
        .max_by_key(|(c, f)| weighted(syllable, c, *f))
        .map(|(c, _)| c)
}

/// 學習權重：**原本的分數乘上 `k^N`**（`N` 是使用者選過幾次）。
///
/// # 為什麼是乘不是換掉
///
/// 「學過就贏」正是 libchewing 被抱怨的那條曲線（選一次就衝到第一，
/// 之後每次都要手動改回來）。乘上去的話：常用字選一次翻不動，罕見字
/// 選三四次才會贏——**兩條既定規則同時成立**，見開發文件 §2.22.5.1。
fn weighted(syllable: &str, ch: &str, base: u32) -> u64 {
    if !crate::learn::any() {
        return base as u64;
    }
    learned_weight(base, crate::learn::index().count(syllable, ch))
}

/// 原本的分數乘上 `k^N`。**純函式，方便單獨測**——學習的索引是全域的，
/// 測試又平行跑，碰它會害到別的測試（這個坑踩過一次）。
fn learned_weight(base: u32, n: u32) -> u64 {
    if n == 0 {
        return base as u64;
    }
    // 上限擋溢位：`k^10` 已經是 10 億倍，再多沒有意義
    (base as u64).saturating_mul(crate::learn::GROWTH.pow(n.min(10)))
}

/// 這串按鍵對應哪個多字詞？
///
/// 選詞模組用它修正逐字選錯的結果——`su3cl3` 逐字是「擬郝」，
/// 查到「你好」就整組換掉。
/// **回傳 `Cow` 的理由**：詞庫是載一次就固定的，借用得出去；領域包
/// 那一層是可替換的（見 `pack::Index`），借用活不過換表。查不到包的
/// 常路徑仍然是零成本的借用，只有真的命中包才複製一次。
pub fn word_for(keys: &str) -> Option<Cow<'static, str>> {
    // **學習排在最前面**：包是整批引進的通用詞，學習是這個人自己打
    // 出來的。沒到 `LEARNED` 次的候選不會回傳，所以「選一次」不會
    // 突然改變輸出（見 `learn` 的模組說明）。
    if crate::learn::any() {
        if let Some(w) = crate::learn::index().best(keys) {
            return Some(Cow::Owned(w.to_string()));
        }
    }
    // 領域包是獨立的一層，接著問它
    if crate::pack::any() {
        if let Some(w) = crate::pack::index().zh.get(keys) {
            return Some(Cow::Owned(w.clone()));
        }
    }
    zh().and_then(|d| d.word(keys)).map(Cow::Borrowed)
}

/// 這串按鍵的**所有**同讀音詞，best 在前。
///
/// 「城市」與「程式」讀音完全相同（ㄔㄥˊㄕˋ）。`word_for` 只回第一個
/// ——那是「不動選字鍵直接送出」要的預設值；選字時要挑得到第二個，
/// 才可能有「選了『程』，『市』跟著變『式』」。
///
/// 層順序跟 `word_for` 一致：學習 → 領域包 → 靜態詞庫。前面的層是
/// 使用者的表態，排在前面。
pub fn words_for(keys: &str) -> Vec<Cow<'static, str>> {
    let mut out: Vec<Cow<'static, str>> = Vec::new();
    let mut push = |w: Cow<'static, str>| {
        if !out.contains(&w) {
            out.push(w);
        }
    };
    if crate::learn::any() {
        if let Some(w) = crate::learn::index().best(keys) {
            push(Cow::Owned(w.to_string()));
        }
    }
    if crate::pack::any() {
        if let Some(w) = crate::pack::index().zh.get(keys) {
            push(Cow::Owned(w.clone()));
        }
    }
    if let Some(d) = zh() {
        for w in d.words(keys) {
            push(Cow::Borrowed(w));
        }
    }
    out
}

/// 這個假名有哪些漢字表記？依 mozc cost 排序（越小越常用）。
pub fn words_for_kana(kana: &str) -> Vec<String> {
    let Some(d) = ja() else { return Vec::new() };
    let Some(i) = d.find(kana) else {
        return Vec::new();
    };
    d.cands(i).map(|c| c.surface.to_string()).collect()
}

/// 日文版面載好了嗎？拿得到才查。
fn ja() -> Option<&'static crate::dict_bin::KanaDict> {
    JA.get().and_then(|d| d.as_ref())
}

/// 領域包與學習的詞，在 Viterbi 眼中的詞類 id。
///
/// mozc 的 id 是詞類編號，1851 是最常見的一般名詞（`アアルト` 那類）。
/// 使用者加的詞多半是專有名詞，當成一般名詞接續最不會出錯。
const USER_WORD_ID: u16 = 1851;

/// 領域包與學習的詞成本。
///
/// **要比典型的詞便宜，但不是零**：使用者明講要的詞該贏過詞典的拆法
/// （`うさだぺこら` 不該被拆成 `us`＋`仇`＋…），但留一點空間讓接續
/// 成本仍有發言權——不然一個包裡的詞會硬吃掉整句的合理分法。
/// mozc 的詞成本典型落在 3000～8000。
const USER_WORD_COST: u16 = 2000;

/// 這個假名有哪些候選（含 Viterbi 要的 id 與成本）。
///
/// # 為什麼要 `Cow`
///
/// 沒啟用包也沒學過東西時直接借用詞典那份（Viterbi 一次要查 O(n²) 個
/// 子字串，每次配置一個 `Vec` 是白花的）；有的話才合成一份。
///
/// # 為什麼包與學習要進到這裡
///
/// 它們原本只接在 `best_kana_word`，那條路是「整段假名查一次」——
/// 所以包只在**整串剛好就是那個詞**時有效，一旦出現在句子中間，
/// 分詞就看不到它了。實測 `usadapekoradesu` 會變成 `us仇ぺこらです`。
pub fn cands_for_kana(kana: &str) -> Cands {
    let hit = ja().and_then(|d| d.find(kana).map(|i| (d, i)));
    if !crate::pack::any() && !crate::learn::any() {
        return match hit {
            Some((d, i)) => Cands::Bin(d, i),
            None => Cands::Empty,
        };
    }
    // **要先把 Arc 抓在手上**：`index()` 回傳的是暫時值，
    // 直接對它 `.best()` 借出去的字串活不過那一行
    let mut extra: Vec<String> = Vec::new();
    if crate::learn::any() {
        let idx = crate::learn::index();
        if let Some(w) = idx.best(kana) {
            extra.push(w.to_string());
        }
    }
    if crate::pack::any() {
        let idx = crate::pack::index();
        if let Some(w) = idx.ja.get(kana) {
            extra.push(w.clone());
        }
    }
    if extra.is_empty() {
        return match hit {
            Some((d, i)) => Cands::Bin(d, i),
            None => Cands::Empty,
        };
    }
    // 使用者的詞排前面，其餘照舊。**字串要 leak 成 'static**——`Cand`
    // 借的是版面裡的位元組，使用者的詞不在版面裡，得自己撐住。數量是
    // 「這個讀音學過幾個詞」，個位數，不是熱路徑的量級
    let mut out: Vec<Cand> = extra
        .iter()
        .map(|surface| Cand {
            surface: intern(surface),
            lid: USER_WORD_ID,
            rid: USER_WORD_ID,
            cost: USER_WORD_COST,
        })
        .collect();
    if let Some((d, i)) = hit {
        out.extend(d.cands(i).filter(|c| !extra.iter().any(|e| e == c.surface)));
    }
    Cands::Owned(out)
}

/// 把使用者的詞（學習／領域包）駐留成 `&'static str`。
///
/// # 為什麼需要駐留，不能直接 leak
///
/// `Cand::surface` 借的是二進位版面裡的位元組，但使用者的詞不在版面裡
/// ——它得自己撐到 `'static`。而 `cands_for_kana` **在 Viterbi 的 O(n²)
/// 迴圈裡每按一鍵被呼叫上千次**，每次直接 `Box::leak` 等於每鍵漏一份，
/// 而且無上限。駐留讓同一個字串只漏一次，總量的上限是「使用者學過幾個
/// 不同的日文詞」，千條級。
///
/// 鎖只在「這個讀音真的有學過的詞」時才拿，常路徑一次都不碰。
fn intern(s: &str) -> &'static str {
    static POOL: OnceLock<std::sync::Mutex<std::collections::HashSet<&'static str>>> =
        OnceLock::new();
    let pool = POOL.get_or_init(Default::default);
    // 鎖中毒不該讓輸入法停擺——拿回裡面的資料繼續用
    let mut g = pool.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(v) = g.get(s) {
        return v;
    }
    let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
    g.insert(leaked);
    leaked
}

/// 一個讀音的候選清單。
///
/// **常路徑不配置**：沒有領域包也沒學過東西時直接指進二進位版面，
/// 走訪時才現解每一筆。只有使用者的詞要插隊才配一份。
pub enum Cands {
    /// 直接指進版面的第 `1` 個讀音
    Bin(&'static crate::dict_bin::KanaDict, usize),
    /// 有領域包或學習的詞要插隊
    Owned(Vec<Cand>),
    /// 查不到
    Empty,
}

impl Cands {
    pub fn iter(&self) -> Box<dyn Iterator<Item = Cand> + '_> {
        match self {
            Cands::Bin(d, i) => Box::new(d.cands(*i)),
            Cands::Owned(v) => Box::new(v.iter().copied()),
            Cands::Empty => Box::new(std::iter::empty()),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Cands::Bin(d, i) => d.cand_count(*i) == 0,
            Cands::Owned(v) => v.is_empty(),
            Cands::Empty => true,
        }
    }
}

/// 這串假名預設寫成什麼？（總成本最低的表記）
///
/// **不要用 `words_for_kana().first()` 代替**——那個會把整份候選清單
/// clone 一次。選詞層每一鍵要對幾百個候選跑一次 `compose`，實測那樣
/// 寫 p99 從 13.5ms 衝到 26.9ms（預算 16ms）。這裡只借用不複製。
pub fn best_kana_word(kana: &str) -> Option<Cow<'static, str>> {
    // 學習排最前面，理由同 `word_for`
    if crate::learn::any() {
        if let Some(w) = crate::learn::index().best(kana) {
            return Some(Cow::Owned(w.to_string()));
        }
    }
    if crate::pack::any() {
        if let Some(w) = crate::pack::index().ja.get(kana) {
            return Some(Cow::Owned(w.clone()));
        }
    }
    let d = ja()?;
    let i = d.find(kana)?;
    // 位圖說這個讀音的第一個候選夠常用，才敢不問使用者就寫成漢字
    d.confident(i).then(|| Cow::Borrowed(d.cand(i, 0).surface))
}

/// 這串按鍵是注音詞典裡的詞嗎？
pub fn is_bopomofo_word(keys: &str) -> bool {
    if crate::learn::cut_any()
        && crate::learn::cutting().lang_of(keys) == Some(crate::language::Language::Bopomofo)
    {
        return true;
    }
    if crate::pack::any() && crate::pack::index().zh.contains_key(keys) {
        return true;
    }
    zh().is_some_and(|d| d.has_word(keys))
}

/// 注音詞典載入了嗎？
pub fn bopomofo_loaded() -> bool {
    zh().is_some_and(|d| !d.is_empty())
}

/// 載入日文詞典（讀音欄，平假名）。多次呼叫只讀一次檔。
///
/// `dictionary00.txt` 的格式是 `讀音<TAB>…<TAB>表記`，第一欄就是
/// 平假名讀音。切法拿的是羅馬字，查之前要先用 `romaji::kana::to_kana`
/// 轉成平假名。
/// 敢不敢預設顯示漢字的總成本上限。
///
/// 總成本 = 詞成本 + 句首接續 + 句尾接續。超過這個數字就維持假名——
/// 那多半是「詞組被誤當成一個詞」的假命中：`どうしよう` 是「どう＋
/// しよう」兩個詞，詞典裡沒有這個條目，但「同仕様」剛好有，於是打
/// `doushiyou` 得到「同仕様」（8447）。正確的單詞轉換都在 4588～5740。
///
/// 這道門很嚴——74.6 萬個讀音只有 1.5 萬個（2%）過得了。那正是意圖：
/// **只有夠常用的詞才敢不問使用者就寫成漢字**，其餘維持假名，
/// 使用者要漢字自己按選字鍵。
const CONFIDENT_COST: u32 = 6500;

/// mozc 接續成本矩陣的**兩條線**：`BOS→各左id` 與 `各右id→EOS`。
///
/// # 為什麼只要兩條線
///
/// 完整矩陣是 2672×2672 ≈ 714 萬格（14～28MB）。但**單段轉換只走
/// 「句首 → 這個詞 → 句尾」**，用得到的只有第 0 列與第 0 行，共 5344
/// 個數字（約 21KB）。整句 Viterbi 才需要全部。
///
/// 常駐記憶體目前 222MB、目標 50MB，這個差別不能忽略。
///
/// 回傳 `(bos, eos)`，索引就是詞性 id。檔案不在就回空的——那時排序
/// 退回只看 cost，不會壞掉。
/// mozc 的接續矩陣（完整版）。
///
/// # 為什麼存成一整塊位元組
///
/// **載入時零解析**：檔案讀進來就是能查的樣子，不做任何轉換。
/// 轉成 `Vec<u16>` 要再走一次 714 萬格、多配置一份 13.6MB；查表時
/// 才取那兩個位元組便宜得多（一次 `from_le_bytes`）。
///
/// 這跟[二進位詞庫的雛形](../../開發文件.md)是同一個原則。
/// 接續矩陣檔頭的長度：magic(4) + ver(2) + n(2)
const CONNECTION_HEADER: usize = 8;

pub struct Connection {
    n: usize,
    /// 整個檔案的 bytes，**借的不是複製的**。
    ///
    /// 原本是 `raw[8..].to_vec()`——為了跳過 8 位元組的檔頭，把 13.6MB
    /// 整份複製一次。改成借用整份、查詢時加上檔頭位移，省掉那一次複製
    /// 與它留下的配置器碎片。
    bytes: &'static [u8],
}

impl Connection {
    /// 前一個詞的右 id 接後一個詞的左 id，要付多少成本。
    ///
    /// 越小越順——「名詞→を→動詞」便宜，「助詞→助詞」貴。
    /// 超出範圍回 `u16::MAX`（接不起來），不 panic：矩陣是外部資料，
    /// id 對不上時整個輸入法不該掛掉。
    pub fn cost(&self, rid: u16, lid: u16) -> u16 {
        let (r, l) = (rid as usize, lid as usize);
        if r >= self.n || l >= self.n {
            return u16::MAX;
        }
        let i = CONNECTION_HEADER + (r * self.n + l) * 2;
        u16::from_le_bytes([self.bytes[i], self.bytes[i + 1]])
    }

    pub fn size(&self) -> usize {
        self.n
    }
}

static CONNECTION: OnceLock<Option<Connection>> = OnceLock::new();

/// 載入完整接續矩陣。**檔案不在就回 `None`**——那時退回只用兩條線的
/// 單詞轉換，功能降級但不會壞掉（`data/download.ps1` 才會產生它）。
pub fn load_connection(data_dir: &Path) -> Option<&'static Connection> {
    CONNECTION
        .get_or_init(|| {
            let path = data_dir.join("japanese").join("connection.bin");
            let bytes = map_file(&path)?;
            // 檔頭：magic(4) + ver(2) + n(2)
            if bytes.len() < CONNECTION_HEADER || &bytes[..4] != b"TSCM" {
                return None;
            }
            let ver = u16::from_le_bytes([bytes[4], bytes[5]]);
            let n = u16::from_le_bytes([bytes[6], bytes[7]]) as usize;
            if ver != 1 || n == 0 {
                return None;
            }
            (bytes.len() == CONNECTION_HEADER + n * n * 2).then_some(Connection { n, bytes })
        })
        .as_ref()
}

/// 完整矩陣載進來了嗎？
pub fn connection() -> Option<&'static Connection> {
    CONNECTION.get().and_then(|c| c.as_ref())
}

fn load_connection_edges(data_dir: &Path) -> (Vec<u16>, Vec<u16>) {
    let path = data_dir
        .join("japanese")
        .join("connection_single_column.txt");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return (Vec::new(), Vec::new());
    };
    let mut lines = content.lines();
    let Some(Ok(n)) = lines.next().map(|l| l.trim().parse::<usize>()) else {
        return (Vec::new(), Vec::new());
    };
    // 品詞 id 是 u16，矩陣邊長不可能超過它（mozc 是 2672）。第一行寫個
    // 天文數字的話 `vec!` 會配置失敗，而配置失敗是 abort 不是 panic
    // ——`catch_unwind` 攔不住，整個宿主直接消失
    if n == 0 || n > u16::MAX as usize {
        return (Vec::new(), Vec::new());
    }
    let mut bos = vec![0u16; n];
    let mut eos = vec![0u16; n];
    // 資料依 rid × n + lid 排列。**只解析用得到的那 5344 行**——
    // 整份有 714 萬行，每行都 parse 是白花的
    for (i, line) in lines.enumerate() {
        if i >= n * n {
            break;
        }
        let (rid, lid) = (i / n, i % n);
        if rid != 0 && lid != 0 {
            continue;
        }
        let Ok(v) = line.trim().parse::<u16>() else {
            continue;
        };
        if rid == 0 {
            bos[lid] = v;
        }
        if lid == 0 {
            eos[rid] = v;
        }
    }
    (bos, eos)
}

pub fn load_japanese(data_dir: &Path) -> Option<&'static crate::dict_bin::KanaDict> {
    let first = JA.get().is_none();
    JA.get_or_init(|| {
        // **完整矩陣跟詞典一起載**，不要讓呼叫端自己記得。
        //
        // 這裡踩過坑：整句轉換沒有矩陣時會產生垃圾（`あにめ` 被切成
        // `あに`＋`目`），因為 mozc 讓高頻詞的詞成本趨近零（`目` 是 12、
        // `に` 是 0），**全靠矩陣擋住亂接**。而幾支計分器各自載詞典、
        // 沒載矩陣，量出來的行為跟使用者看到的不一樣。
        load_connection(data_dir);
        // 有現成的版面就直接讀——那是「零解析」的整個意義，實測 12ms，
        // 從文字重建要 700ms。認不得就當作沒有，退回重建
        let path = data_dir.join("japanese").join("dict_ja.bin");
        if let Some(bytes) = map_file(&path) {
            if let Some(d) = crate::dict_bin::KanaDict::new(bytes) {
                return Some(d);
            }
        }
        let raw = build_kana_layout(data_dir)?;
        let leaked: &'static [u8] = Box::leak(raw.into_boxed_slice());
        crate::dict_bin::KanaDict::new(leaked)
    });
    if first {
        bump_generation();
    }
    ja()
}

/// 從 mozc 的十個文字詞典組出二進位版面。
///
/// **這也是 `gen_dict_ja` 產生檔案時走的同一條路**——版面只有一種產生
/// 方式，不會出現「檔案版跟現建版不一樣」這種難查的問題。
pub fn build_kana_layout(data_dir: &Path) -> Option<Vec<u8>> {
    use crate::dict_bin::RawCand;
    let mut kana_words: HashMap<String, Vec<RawCand>> = HashMap::new();
    let (bos, eos) = load_connection_edges(data_dir);
    // mozc 的詞典分成 00~09 十個檔，**十個都要讀**——
    // 只讀前三個的話 すし、がっこう 這些常見詞會查不到。
    let mut any = false;
    for i in 0..10 {
        let path = data_dir
            .join("japanese")
            .join(format!("dictionary{i:02}.txt"));
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        any = true;
        for line in content.lines() {
            // **不要 collect 成 Vec**——這個迴圈跑一百多萬次，
            // 每次配一個 Vec 是白花的
            let mut f = line.split('\u{9}');
            let Some(kana) = f.next() else { continue };
            if kana.is_empty() {
                continue;
            }
            // 欄位：讀音 左id 右id cost 表記
            let lid = f.next().and_then(|v| v.parse::<usize>().ok());
            let rid = f.next().and_then(|v| v.parse::<usize>().ok());
            let cost = f.next().and_then(|c| c.parse::<u32>().ok());
            let surface = f.next();
            let (Some(lid), Some(rid), Some(cost), Some(surface)) = (lid, rid, cost, surface)
            else {
                continue;
            };
            // **總成本 = 詞成本 + 句首接續 + 句尾接續**。
            //
            // 只看詞成本會錯得很明顯：「酸し」(4451) 比「寿司」(4520)
            // 便宜，於是打 sushi 得到「酸し」。但「酸し」是文語形容詞，
            // 句首接它要 5748、接名詞只要 1066——差距在接縫不在詞。
            let total = cost
                + bos.get(lid).copied().unwrap_or(0) as u32
                + eos.get(rid).copied().unwrap_or(0) as u32;
            // **同一個讀音會出現在很多詞條上**。用 entry 的話每次都得
            // 先配一個 String 當鍵，即使那把鍵早就在表裡了。
            let c = RawCand {
                surface: surface.to_string(),
                lid: lid as u16,
                rid: rid as u16,
                cost: cost as u16,
                total,
            };
            match kana_words.get_mut(kana) {
                Some(v) => v.push(c),
                None => {
                    kana_words.insert(kana.to_string(), vec![c]);
                }
            }
        }
    }
    if !any {
        return None;
    }
    // 排序、去重、決定「有把握」的門檻全部在 `build` 裡做完
    Some(crate::dict_bin::build(
        kana_words.into_iter().collect(),
        CONFIDENT_COST,
    ))
}

/// 這串羅馬字是日文詞典裡的詞嗎？
///
/// 先轉平假名再查——詞典存的是假名，切法拿的是羅馬字。
pub fn is_japanese_word(keys: &str) -> bool {
    if crate::learn::cut_any()
        && crate::learn::cutting().lang_of(keys) == Some(crate::language::Language::Romaji)
    {
        return true;
    }
    let Some(d) = ja() else {
        return false;
    };
    match crate::romaji::kana::to_kana(keys) {
        Some(k) => d.contains(&k),
        None => false,
    }
}

/// 日文詞典載入了嗎？
pub fn japanese_loaded() -> bool {
    ja().is_some_and(|d| !d.is_empty())
}

#[cfg(test)]
mod tests {
    /// 接續矩陣的第一行寫個天文數字：不能去配置那麼大的陣列
    /// （配置失敗是 abort，會把宿主整個帶走），當作沒有這個檔。
    #[test]
    fn 接續矩陣的邊長灌大要拒收() {
        let base = std::env::temp_dir().join("tsunagi_conn_huge");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("japanese")).unwrap();
        std::fs::write(
            base.join("japanese").join("connection_single_column.txt"),
            "4000000000\n0\n0\n",
        )
        .unwrap();
        let (bos, eos) = super::load_connection_edges(&base);
        assert!(bos.is_empty() && eos.is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 駐留：同一個字串重複要，拿到的必須是**同一塊記憶體**。
    ///
    /// 這條在擋一個實際發生過的漏洞：`cands_for_kana` 在 Viterbi 的
    /// O(n²) 迴圈裡每鍵被呼叫上千次，若每次直接 `Box::leak`，只要使用者
    /// 學過任何日文詞就會無上限地漏記憶體。
    #[test]
    fn 使用者的詞只會漏一次() {
        let a = super::intern("寿司");
        let b = super::intern("寿司");
        let c = super::intern("刺身");
        assert_eq!(a.as_ptr(), b.as_ptr(), "同一個字串要拿到同一塊");
        assert_ne!(a.as_ptr(), c.as_ptr(), "不同字串各自一塊");
        assert_eq!((a, c), ("寿司", "刺身"));
    }

    use super::*;

    fn data_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data")
    }

    #[test]
    fn 載入注音詞典() {
        let d = load_bopomofo(&data_dir());
        if d.is_none_or(|d| d.is_empty()) {
            eprintln!("詞庫未下載，跳過（跑 data/download.ps1）");
            return;
        }
        let n = d.map(|d| d.word_count()).unwrap_or(0);
        assert!(n > 100_000, "應該有十萬條以上，實際 {n}");
    }

    #[test]
    fn 查得到常見詞() {
        let d = load_bopomofo(&data_dir());
        if d.is_none_or(|d| d.is_empty()) {
            return;
        }
        // 你好、今天、問題、時間
        for (w, keys) in [
            ("你好", "su3cl3"),
            ("今天", "rup wu0 "),
            ("問題", "jp4wu6"),
            ("時間", "g6ru0 "),
        ] {
            assert!(is_bopomofo_word(keys), "{w} 的按鍵 {keys:?} 該在詞典裡");
        }
    }

    #[test]
    fn 單字不算詞() {
        let d = load_bopomofo(&data_dir());
        if d.is_none_or(|d| d.is_empty()) {
            return;
        }
        // BPMFMappings 存的是多字詞，單字在 BPMFBase
        assert!(!is_bopomofo_word("su3"), "單字「你」不在多字詞表裡");
    }

    #[test]
    fn 亂碼查不到() {
        let d = load_bopomofo(&data_dir());
        if d.is_none_or(|d| d.is_empty()) {
            return;
        }
        assert!(!is_bopomofo_word("zzxxqq"));
    }

    #[test]
    fn 注音符號轉按鍵_連寫也切得出音節() {
        let rev = reverse_keymap();
        let k = |s: &str| symbols_to_keys(s, &rev);
        // 單音節
        assert_eq!(k("ㄕˋ").as_deref(), Some("g4"));
        // 一聲不標符號，但打字要按空白
        assert_eq!(k("ㄊㄚ").as_deref(), Some("w8 "));
        // 多音節連寫——邊界靠角色順序判斷，不靠分隔符
        assert_eq!(k("ㄕˊㄗㄨㄛˋ").as_deref(), Some("g6yji4"));
        assert_eq!(k("ㄓㄜˋㄅㄨˋ").as_deref(), Some("5k41j4"));
        // 中間夾一聲音節：前一個音節要補空白才收得掉
        assert_eq!(k("ㄊㄚㄇㄣˊ").as_deref(), Some("w8 ap6"));
        // 表外的字元
        assert_eq!(k("abc"), None);
        assert_eq!(k(""), None);
    }

    #[test]
    fn 偏好表凌駕詞頻() {
        let d = load_bopomofo(&data_dir());
        if d.is_none_or(|d| d.is_empty()) {
            return;
        }
        // 詞頻讓「上船」(13411) 贏「上傳」(2332)，偏好表把它扳回來
        assert_eq!(word_for("g;4tj06").as_deref(), Some("上傳"));
        // 「實作」也在表裡，它跟「十座」同音
        assert_eq!(word_for("g6yji4").as_deref(), Some("實作"));
    }

    /// **指數曲線的核心行為**：選一次翻不動差很多的字，選幾次才會。
    ///
    /// 這是兩條既定規則的交會點——「使用者的選擇要贏過統計」與
    /// 「不要一次就跳到最前面」（libchewing 被抱怨的那條）。
    /// 見開發文件 §2.22.5.1。
    #[test]
    fn 學習權重要累積才翻得動() {
        // 常用字 vs 罕見字，字頻差一千倍
        let common = 100_000u32;
        let rare = 100u32;
        assert!(
            learned_weight(rare, 1) < common as u64,
            "選一次翻不動差一千倍的字"
        );
        assert!(learned_weight(rare, 2) < common as u64, "選兩次也還不夠");
        assert!(
            learned_weight(rare, 4) > common as u64,
            "選四次該贏（8⁴ = 4096 > 1000）"
        );
        // 差距小的話一次就夠——那是對的，兩個都常用時使用者說了算
        assert!(learned_weight(90, 1) > 100);
        // 沒選過就是原分數
        assert_eq!(learned_weight(123, 0), 123);
    }

    #[test]
    fn 偏好表可以補詞庫沒收的詞() {
        let d = load_bopomofo(&data_dir());
        if d.is_none_or(|d| d.is_empty()) {
            return;
        }
        // 「這部」「這不」兩個都不在 BPMFMappings 裡，只能由偏好表補
        assert_eq!(word_for("5k41j4").as_deref(), Some("這部"));
    }

    #[test]
    fn 偏好表也調同音字的順序() {
        let d = load_bopomofo(&data_dir());
        if d.is_none_or(|d| d.is_empty()) {
            return;
        }
        // priority.txt: ㄕˋ 是 事 市 世 士 …
        let c = chars_for("g4");
        assert_eq!(c.first().map(String::as_str), Some("是"));
        assert_eq!(c.get(1).map(String::as_str), Some("事"));
        // 表外的字仍在，只是排後面——核心原則是「候選只排序、不排除」
        assert!(c.iter().any(|x| x == "室"), "沒列在偏好表的字不該消失");
    }

    #[test]
    fn 詞頻表沒收的詞退回字頻代理() {
        // 兩個都不在詞頻表裡時，比的是組成字的最低字頻
        let mut wf = HashMap::new();
        let mut cf = HashMap::new();
        cf.insert('甲', 100u32);
        cf.insert('乙', 50u32);
        assert_eq!(word_score("甲甲", &wf, &cf), (0, 100));
        assert_eq!(word_score("甲乙", &wf, &cf), (0, 50));
        // 詞頻表收錄的一律贏過沒收錄的——單位不同，只能分層比
        wf.insert("乙乙".to_string(), 1u64);
        assert!(word_score("乙乙", &wf, &cf) > word_score("甲甲", &wf, &cf));
    }
}
