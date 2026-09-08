//! 日文動詞的**活用還原**：給一串羅馬字，問它是不是某個動詞的活用形。
//!
//! # 為什麼不用後綴表
//!
//! 「語尾長得像活用形就算活用形」是拿結果反推原因，補不完：每加一條
//! 語尾就多一批誤觸，再加一道驗證去擋。實測過——加上一段動詞的裸
//! `ta`／`te`／`de` 之後，`gakkousite`（学校 site）、`onegaidata`
//! （お願い data）、`sigotomode`（仕事 mode）全被誤認成活用形，
//! 那正是引擎最該切開的「日文詞＋英文詞」。
//!
//! # 這裡的做法：照文法的結構走
//!
//! 日文動詞只有三類，這是**封閉**的：
//!
//! | 類別 | 辭書形 | 活用方式 |
//! |---|---|---|
//! | 一段 | `iru`／`eru` 結尾 | 去掉 `ru`，語幹直接接語尾 |
//! | 五段 | 其餘 `u` 結尾 | 語幹**音變**後接語尾 |
//! | 不規則 | `suru`／`kuru` 兩個 | 各自獨立 |
//!
//! 所以流程是**反推**：拿活用形去試各類的還原規則，還原出來的辭書形
//! 查得到詞典，就確定它是活用形，同時也知道它是哪個動詞。
//!
//! ```text
//! okita   一段·過去 → 語幹 oki → 辭書形 okiru      ✓ 詞典有 → 是活用形
//!         五段·過去 → 音便還原 → okitsu／okiku…    ✗ 都不在
//!
//! gakkousite  一段·て形 → gakkousiru               ✗
//!             五段·て形 → gakkousu／gakkousuru…    ✗
//!             → 不是活用形（正確：那是 学校＋site）
//! ```
//!
//! **詞典自己就是判準**，不必列舉語幹，也不必為每個動詞窮舉它的十幾種
//! 活用形——詞典收「起きる」一條，起きた／起きて／起きない／起きられる
//! 全部涵蓋得到。
//!
//! # 例外表
//!
//! `iru`／`eru` 結尾的**不一定**是一段動詞——帰る・走る・入る・切る・
//! 知る 都是五段。這批是有名的封閉集合，列在 `五段例外`。

/// **`iru`／`eru` 結尾卻是五段**的動詞。
///
/// 一段動詞的辭書形都以 `iru`／`eru` 結尾，但反過來不成立——這批
/// 長得像一段、實際是五段。
///
/// # 這張表不是用來「擋掉」一段還原的
///
/// 因為**羅馬字同形的動詞可能分屬兩類**：
///
/// ```text
/// kiru   切る（五段）→ 切って   ／ 着る（一段）→ 着て
/// neru   練る（五段）→ 練って   ／ 寝る（一段）→ 寝て
/// iru    要る（五段）→ 要って   ／ 居る（一段）→ 居て
/// ```
///
/// 羅馬字這一層分不開，硬用這張表擋掉一段還原的話，`nete`（寝て）
/// 就認不出來了。所以還原時**兩條路都試**，任一條走得通就算數——
/// 這張表的用途是讓五段還原**多試一輪**（見 `五段還原`），不是否決。
///
/// 收的是辭書形的羅馬字，`si`／`ti` 之類的替代打法各列一條。
///
/// # 目前它其實沒起作用（2026-09-07 實測）
///
/// 停用整張表之後漏斗與測試**完全不變**。原因是促音便還原本來就會
/// 試 `ru`（`("tta", "ru")` 排在表的最前面），而「辭書形要真的是動詞」
/// 那道驗證會擋掉錯的候選——不在表裡的例外動詞（齧る・焦る・擦る・
/// 罵る・覆る・蘇る・翻る）實測全部正常。
///
/// **留著是因為知識正確且將來可能需要**：還原順序或驗證方式一改，
/// 這批就可能重新需要特別處理。它也是這個模組唯一寫下「哪些
/// `iru`／`eru` 結尾其實是五段」的地方。
const 五段例外: &[&str] = &[
    "kaeru",   // 帰る・返る
    "hashiru", // 走る
    "hasiru",  // 走る（si 打法）
    "hairu",   // 入る
    "kiru",    // 切る
    "shiru",   // 知る
    "siru",    // 知る（si 打法）
    "iru",     // 要る
    "suberu",  // 滑る
    "keru",    // 蹴る
    "neru",    // 練る
    "heru",    // 減る
    "meiru",   // 滅入る
    "kagiru",  // 限る
    "chiru",   // 散る
    "tiru",    // 散る（ti 打法）
    "majiru",  // 混じる
    "mairu",   // 参る
    "nigiru",  // 握る
    "shaberu", // 喋る
    "syaberu", // 喋る（sy 打法）
    "hineru",  // 捻る
    "kudaru",  // 下る
    "azakeru", // 嘲る
    "yogiru",  // 過る
    "hoteru",  // 火照る
];

/// 五段動詞的**語尾音**與其て形／た形的音便。
///
/// 五段的過去形與て形會音變，而音變規則由**辭書形的最後一個音節**決定：
///
/// ```text
/// 書く kaku  → 書いて kaite   イ音便   く → いて
/// 泳ぐ oyogu → 泳いで oyoide  イ音便   ぐ → いで
/// 話す hanasu→ 話して hanashite        す → して
/// 待つ matsu → 待って matte   促音便   つ → って
/// 死ぬ shinu → 死んで shinde  撥音便   ぬ → んで
/// 遊ぶ asobu → 遊んで asonde  撥音便   ぶ → んで
/// 読む yomu  → 読んで yonde   撥音便   む → んで
/// 取る toru  → 取って totte   促音便   る → って
/// 買う kau   → 買って katte   促音便   う → って
/// ```
///
/// 表的方向是**還原用**：`(て形的尾巴, 辭書形的尾巴)`。反推時拿活用形
/// 的尾巴去對第一欄，換成第二欄就得到辭書形候選。
const 音便還原: &[(&str, &str)] = &[
    // **促音便排在イ音便前面**——兩者長度相同（`tta`／`ita` 都是三個
    // 字元），排序穩定所以先列的先試。`sitta`（知った）也以 `ita`
    // 結尾，イ音便先試的話會還原成 `siku`（敷く）。
    //
    // 促音便：辭書形可能是 る／う／つ 三種之一。**順序＝可能性**：
    // `kaetta` 該還原成 `kaeru`（帰る），但 `kaetsu` 碰巧也在詞典裡，
    // `tsu` 排前面就會給出那個。る 結尾的五段遠多於 つ 結尾的。
    ("tte", "ru"),
    ("tta", "ru"),
    ("tte", "tsu"),
    ("tta", "tsu"),
    ("tte", "tu"),
    ("tta", "tu"),
    ("tte", "u"),
    ("tta", "u"),
    // イ音便
    ("ite", "ku"),
    ("ita", "ku"),
    ("ide", "gu"),
    ("ida", "gu"),
    // す 結尾（不音便）
    ("shite", "su"),
    ("shita", "su"),
    ("site", "su"),
    ("sita", "su"),
    // 撥音便：ぬ／ぶ／む——同樣按可能性排，む 最常見（読む・飲む・住む）
    //
    // **`ん` 在這個輸入法打 `nn`**（`yonnda`＝読んだ），所以 `nnda`／`nnde`
    // 才是實際會收到的形式；`nda`／`nde` 一併留著，別的打法或已正規化的
    // 字串走那條。長的排前面，`sort_by_key` 會讓 `nnda` 先試。
    ("nnde", "mu"),
    ("nnda", "mu"),
    ("nnde", "bu"),
    ("nnda", "bu"),
    ("nnde", "nu"),
    ("nnda", "nu"),
    ("nde", "mu"),
    ("nda", "mu"),
    ("nde", "bu"),
    ("nda", "bu"),
    ("nde", "nu"),
    ("nda", "nu"),
];

/// 「行く」的例外——不是イ音便而是促音便（行って／行った）。
///
/// **不能放進 `音便還原`**：那張表是當後綴比對的，而 `itta` 是四個字元、
/// 比 `tta` 長，於是 `sitta`（知った）會被它攔截，還原成 `s`＋`iku`
/// ＝ `siku`（敷く）。這條只在**整串就是它**時才成立。
const 行く例外: &[(&str, &str)] = &[("itte", "iku"), ("itta", "iku")];

/// 一段動詞的活用語尾——**語幹直接接**，沒有音變。
///
/// 反推時把這些從尾巴剝掉，剩下的就是語幹，加 `ru` 就是辭書形。
const 一段語尾: &[&str] = &[
    "ru",
    "ta",
    "te",
    "nai",
    "nakatta",
    "nakereba",
    "nakute",
    "masu",
    "masita",
    "mashita",
    "masen",
    "masyou",
    "mashou",
    "reba",
    "you",
    "rareru",
    "rareta",
    "rarenai",
    "saseru",
    "saseta",
    "tara",
    "tari",
    "teiru",
    "teita",
    "tekudasai",
    "temo",
    "nagara",
    "ro",
    "yo",
];

/// 五段動詞在**不音變**的活用形裡，語尾母音會變。
///
/// 五段的ない形／ます形不走音便，而是換母音段：
///
/// ```text
/// 書く kaku → 書かない kakanai   (u段 → a段) ＋ nai
///          → 書きます kakimasu   (u段 → i段) ＋ masu
///          → 書けば   kakeba     (u段 → e段) ＋ ba
///          → 書こう   kakou      (u段 → o段) ＋ u
/// ```
///
/// 表是**還原用**：`(活用形裡的母音, 辭書形的母音)`——一律還原成 `u`。
const 段母音: &[(char, &str)] = &[
    // ── a 段（未然形）：否定・使役・受身接在這一段後面 ──
    ('a', "nai"),
    ('a', "nakatta"),
    ('a', "nakereba"),
    ('a', "nakute"),
    ('a', "naide"),
    ('a', "nakau"),
    ('a', "reru"), // 受身・可能  書かれる
    ('a', "reta"),
    ('a', "rete"),
    ('a', "reteiru"),
    ('a', "renai"),
    ('a', "seru"), // 使役        書かせる
    ('a', "seta"),
    ('a', "sete"),
    ('a', "serareru"), // 使役受身  書かせられる
    ('a', "sareru"),   // 使役受身的縮約形 待たされる
    ('a', "sareta"),
    ('a', "sarete"),
    ('a', "sareteiru"),
    ('a', "nakattara"),
    // ── i 段（連用形）：ます・たい・ながら ──
    ('i', "masu"),
    ('i', "masita"),
    ('i', "mashita"),
    ('i', "masen"),
    ('i', "masyou"),
    ('i', "mashou"),
    ('i', "masuka"),
    ('i', "tai"),
    ('i', "takatta"),
    ('i', "nagara"),
    ('i', "yasui"),
    ('i', "nikui"),
    // ── e 段（假定形・可能形）──
    ('e', "ba"),
    ('e', "ru"), // 可能  書ける
    ('e', "ta"),
    ('e', "nai"),
    ('e', "masu"),
    // ── o 段（意志形）──
    ('o', "u"),
    ('o', "utosuru"),
];

/// 這串羅馬字**是某個動詞的活用形嗎**？
///
/// 只回答是／不是。要知道是哪個動詞用 [`辭書形`]。
pub fn is_inflected(keys: &str) -> bool {
    辭書形(keys).is_some()
}

/// 動詞辭書形的最後一個假名——**一定是 u 段音**。
///
/// 這是日文動詞的定義性特徵（所以才叫「五段」「一段」——指的是語尾在
/// 五十音圖上的活動範圍）。
const U段: &[char] = &[
    'う', 'く', 'ぐ', 'す', 'ず', 'つ', 'づ', 'ぬ', 'ふ', 'ぶ', 'ぷ', 'む', 'ゆ', 'る',
];

/// 這個假名串在詞典裡**真的是一個動詞**嗎？
///
/// # 為什麼「查得到」不夠
///
/// 還原規則會湊出假的辭書形，而詞典**大得足以碰巧收錄它們**：
///
/// ```text
/// arukimasu（歩きます）→ 一段規則剝掉 masu → aruki + ru = arukiru
///                        「あるきる」在詞典裡！ → アルキル（alkyl，化學名詞）
/// hanashimasu（話します）→ hanashiru →「鼻汁」
/// asobimasu（遊びます）  → asobiru   →「アソビル」
/// ```
///
/// 這些都不是動詞，只是碰巧同音的名詞或外來語。**光問「在不在詞典裡」
/// 分不出來**，於是五段的ます形被一段規則攔截，還原到不存在的動詞。
///
/// # 判準：有沒有「漢字＋送假名」的寫法
///
/// 動詞在詞典裡會有漢字寫法，而且**送假名是 u 段音**：
///
/// ```text
/// aruku  → 歩く   ✓ 漢字＋く
/// hanasu → 話す   ✓ 漢字＋す
/// hairu  → 入る   ✓ 漢字＋る
/// okiru  → 起きる ✓ 漢字＋きる
///
/// arukiru→ アルキル ✗ 全片假名
/// haiku  → 俳句     ✗ 沒有送假名
/// ```
///
/// 實測 13 個真假辭書形，這條判準全部分對。
fn 是動詞(kana: &str) -> bool {
    crate::dict::words_for_kana(kana)
        .iter()
        .any(|c| 是動詞表記(c))
}

/// 平假名或片假名？
fn 是假名(c: char) -> bool {
    ('\u{3040}'..='\u{30ff}').contains(&c)
}

/// 平假名？
///
/// 跟 [`是假名`] 分開，是因為**片假名的意義不同**：詞典首選是片假名
/// 時那多半是外來語名詞（`ほてる`→「ホテル」、`まいる`→「マイル」），
/// 不代表這個動詞習慣寫假名。混為一談的話「火照った」「参った」
/// 會被整批擋掉。
fn 是平假名(c: char) -> bool {
    ('\u{3040}'..='\u{309f}').contains(&c)
}

/// 這個辭書形的**詞典首選**就是動詞嗎？
///
/// 比 [`辭書形成立`] 嚴：那個只要求「候選裡有動詞」，這個要求動詞
/// 排第一。同音的還原候選要分高下時用這個——見 `五段還原` 的說明。
fn 首選是動詞(cand: &str) -> bool {
    let Some(kana) = crate::romaji::kana::to_kana(cand) else {
        return false;
    };
    crate::dict::words_for_kana(&kana)
        .first()
        .is_some_and(|w| 是動詞表記(w))
}

/// 這個羅馬字還原出來的辭書形，**站得住嗎**？
///
/// 兩關：詞典查得到，而且真的是個動詞（見 [`是動詞`]）。
fn 辭書形成立(cand: &str) -> bool {
    match crate::romaji::kana::to_kana(cand) {
        Some(k) => 是動詞(&k),
        None => false,
    }
}

/// 把活用形**還原成辭書形**，還原不出來（或還原出來的詞典查不到）回 `None`。
///
/// # 為什麼要查詞典
///
/// 還原規則本身很鬆——任何字串都能硬套規則湊出一個「辭書形候選」。
/// 詞典是那道把關：湊出來的東西**真的是一個動詞**才算數。
///
/// ```text
/// okita       → oki + ru = okiru      ✓ 詞典有   → 起きた
/// gakkousite  → gakkousi + ru         ✗ 沒有
///             → gakkousu（五段す）    ✗ 沒有     → 不是活用形
/// ```
///
/// # 一個查不到的東西：同音異類
///
/// `kiru` 可能是「切る」（五段）也可能是「着る」（一段），羅馬字一樣。
/// 把活用形**寫成漢字**：`おきた` → `起きた`。
///
/// # 為什麼需要這個
///
/// mozc 只收辭書形，所以活用形查不到，選字層只好原樣顯示假名——
/// 打「おきた」出來就是「おきた」，要漢字得自己選。但漢字其實推得出來：
///
/// ```text
/// おきた  → 還原辭書形 おきる → 詞典有「起きる」
///         → 漢字語幹「起」＋ 活用語尾「きた」  → 起きた
/// のんだ  → 還原 のむ → 「飲む」→ 「飲」＋「んだ」 → 飲んだ
/// ```
///
/// 調 `CONFIDENT_COST` 那道門檻救不了這件事——「起きた」在詞典裡
/// 根本不存在，門檻放得再寬也只會撈到同音的「沖田」（實測放寬到
/// 8000 時正是如此）。要的字得自己組出來。
///
/// # 取漢字時要取**動詞**那一個
///
/// `かく` 的候選第一名是「各」，「書く」排在後面。取第一個含漢字的
/// 會拿到「各」，組出「各きます」。所以要挑**送假名是 u 段音**的那個
/// （見 [`是動詞`] 的說明）。
///
/// 回傳 `None` 的情況：不是活用形、辭書形習慣寫假名（`ある`）、
/// 沒有漢字寫法、或語幹對不上。要全部候選用 [`漢字候選`]。
pub fn 漢字表記(keys: &str) -> Option<String> {
    // **辭書形習慣寫平假名的，顯示就維持假名**。
    //
    // `ある`／`わかる`／`いく` 的日常寫法就是假名，預設顯示「有ります」
    // 「分かりました」反而是錯的（實測 cutpoint 節賠 14 句）。詞典自己
    // 知道這件事——`best_kana_word` 對這些詞給的首選就是假名。
    //
    // 這跟 `compose::best_japanese` 的既有哲學一致：**平假名該贏的
    // 時候會自己贏**，不必為它們另外維護例外表。
    //
    // **但這只擋顯示，不擋候選**——`漢字候選` 照樣把「行った」「有った」
    // 組出來，使用者按選字鍵就選得到。有歧義的不算錯，但候選要有。
    //
    // 只看平假名：片假名的首選是外來語名詞（見 [`是平假名`]），
    // 那不代表這個動詞習慣寫假名。
    let 辭書假名 = 辭書形(keys).and_then(|d| crate::romaji::kana::to_kana(&d))?;
    if crate::dict::best_kana_word(&辭書假名).is_some_and(|w| w.chars().all(是平假名)) {
        return None;
    }
    漢字候選(keys).into_iter().next()
}

/// 這個活用形**所有**組得出來的漢字寫法，最可能的排前面。
///
/// # 為什麼不能只給一個
///
/// 同一個活用形常常對應好幾個動詞，而且都合理：
///
/// ```text
/// よんだ  読んだ（読む）・呼んだ（呼ぶ）・詠んだ（詠む）
/// かった  買った（買う）・勝った（勝つ）・飼った（飼う）
/// かえった 帰った（帰る）・返った（返る）・変えた…（変える 不合法）
/// ```
///
/// 只給第一個的話，其餘的**使用者選不回來**——候選清單裡只剩三種
/// 假名寫法。排序猜錯就完全沒救。
pub fn 漢字候選(keys: &str) -> Vec<String> {
    let Some(活用假名) = crate::romaji::kana::to_kana(keys) else {
        return Vec::new();
    };
    let Some(辭書羅馬字) = 辭書形(keys) else {
        return Vec::new();
    };
    let Some(辭書假名) = crate::romaji::kana::to_kana(&辭書羅馬字) else {
        return Vec::new();
    };
    // 辭書形本身就是輸入時不必組——那不是活用形
    if 辭書假名 == 活用假名 {
        return Vec::new();
    }

    let 全部 = crate::dict::words_for_kana(&辭書假名);
    // **同一個語幹有沒有更短的送假名寫法**——那是五段的訊號。
    //
    // 五段的活用語尾只有最後一個音在變（交**る**→交**った**），所以
    // 送假名可以只寫「る」；一段的語幹尾音是固定的（起**き**る→
    // 起**き**た），省不掉。於是：
    //
    // ```text
    // まじる  交じる ／ 交る   長短並存 → 五段
    // おきる  起きる           只有一種 → 一段
    // ```
    //
    // 實測 まじる・おわる（五段）有長短並存，おきる・たべる・かんがえる・
    // しらべる（一段）一個都沒有。
    let 有短寫法: std::collections::HashSet<String> = {
        use std::collections::{HashMap, HashSet};
        let mut 依語幹: HashMap<String, Vec<usize>> = HashMap::new();
        for w in &全部 {
            let n = w.chars().rev().take_while(|c| 是假名(*c)).count();
            if n == 0 || n == w.chars().count() {
                continue;
            }
            let 幹: String = w.chars().take(w.chars().count() - n).collect();
            依語幹.entry(幹).or_default().push(n);
        }
        依語幹
            .into_iter()
            .filter(|(_, v)| v.iter().min() != v.iter().max())
            .map(|(k, _)| k)
            .collect::<HashSet<_>>()
    };
    // 詞典的順序就是可能性的順序，逐個試組，組得出來的都留著
    let mut out: Vec<String> = Vec::new();
    for w in &全部 {
        if !是動詞表記(w) {
            continue;
        }
        if let Some(組) = 接語尾(w, &辭書假名, &活用假名, &有短寫法) {
            if !out.contains(&組) {
                out.push(組);
            }
        }
    }
    out
}

/// 拿辭書形的漢字寫法去組活用形：`帰る` ＋ `かえった` → `帰った`。
///
/// 組不出來（語幹對不上）回 `None`——這同時就是**挑對候選**的判準。
/// `かえる` 的動詞候選有「変える」（一段）和「帰る」（五段），
/// 而 `かえった` 是促音便：
///
/// ```text
/// 帰る    假名語幹「かえ」  かえった 去掉語幹 → った  → 帰った   ✓
/// 変える  假名語幹「かえ」  ……一樣接得上          → 変えった ✗
/// ```
///
/// 兩個都「接得上」，所以還要看**送假名**：一段動詞的活用形一定
/// 保留辭書形的送假名開頭（変え**る**→変え**た**），五段不會
/// （帰**る**→帰**った**）。這個差別就是判準，見下面的送假名檢查。
fn 接語尾(
    表記: &str,
    辭書假名: &str,
    活用假名: &str,
    有短寫法: &std::collections::HashSet<String>,
) -> Option<String> {
    // 送假名＝**從尾端連續的假名**，語幹＝其餘的部分。
    //
    // 語幹**不能用「開頭連續的漢字」**：複合動詞的漢字不連續——
    // 「話しかける」是 話→し→かける，取開頭只得到「話」，接上語尾
    // 就成了「話けられた」（少一個「し」）。
    let 送假名數 = 表記.chars().rev().take_while(|c| 是假名(*c)).count();
    let 語幹字數 = 表記.chars().count().checked_sub(送假名數)?;
    let 漢字語幹: String = 表記.chars().take(語幹字數).collect();
    // 純假名的表記沒有語幹可言（那種詞不該轉漢字，上游已經擋掉）
    if 漢字語幹.is_empty() || 漢字語幹.chars().all(是假名) {
        return None;
    }
    let 語幹假名數 = 辭書假名.chars().count().checked_sub(送假名數)?;
    let 假名語幹: String = 辭書假名.chars().take(語幹假名數).collect();
    let 語尾 = 活用假名.strip_prefix(假名語幹.as_str())?;
    // **一段動詞沒有音便**——促音（っ）與撥音（ん）是五段專有的。
    //
    // ```text
    // 変える（一段）→ 変えた・変えて・変えます   語尾不會有 っ／ん
    // 帰る  （五段）→ 帰った・帰って             促音便
    // 読む  （五段）→ 読んだ・読んで             撥音便
    // ```
    //
    // `かえった` 的兩個候選都接得上語幹：「帰る」給「帰った」（對），
    // 「変える」給「変えった」（錯，一段不可能促音便）。判準就是這個。
    //
    // 一段的辨識：**送假名不只一個音**。
    //
    // 「帰る」送假名是「る」（1 個）→ 五段
    // 「変える」送假名是「える」（2 個）→ 一段
    //
    // 不能拿漢字語幹的字數去減——漢字跟假名不等長（「帰」一個字
    // 對應「かえ」兩個假名）。`送假名數` 是從表記尾端數的，才準。
    // **一段與五段的活用形不能互串**。
    //
    // ```text
    // 変える（一段）→ 変えた・変えて       語尾直接接，沒有音便
    // 帰る  （五段）→ 帰った・帰って       促音便
    // 読む  （五段）→ 読んだ・読んで       撥音便
    // ```
    //
    // 不擋的話兩邊都會冒出不存在的字：`kaetta` 給出「変えった」、
    // `kaeta` 給出「帰た」。這不是「把字藏起來」——想打「変えた」就
    // 輸入 `kaeta`，想打「帰った」就輸入 `kaetta`，兩種活用形各有各的
    // 按鍵，一個字都沒有消失（實測 かえる／きる／とる／よむ 四個讀音
    // 的所有動詞都涵蓋得到）。
    //
    // **一段的辨識要逐候選看**，因為同一個讀音可能兩類都有：
    // `かえる` → 変える（一段）／帰る（五段）。
    //
    // 判準是這個**表記自己**的送假名：
    //
    // ```text
    // 変える  送假名「える」→ 語幹到「変」，讀音上語幹只有「か」→ 一段
    // 帰る    送假名「る」  → 語幹「帰」＝「かえ」            → 五段
    // 終わる  送假名「わる」→ 但 `わ` 不是 い／え 段          → 五段
    // ```
    //
    // 所以看**送假名的第一個音是不是 い／え 段**——那是一段動詞的
    // 定義性特徵（上一段／下一段就是指這個）。單看送假名字數會把
    // 「終わる」誤判成一段，把「終わった」擋成「終った」。
    // **同語幹有短寫法的一律是五段**（交じる／交る 並存）。
    //
    // 這道要排在字面判準之前：「交じる」的送假名開頭是「じ」（い段），
    // 字面上跟「起きる」一模一樣，光看表記分不出來。
    let 送假名開頭 = 表記.chars().nth(語幹字數);
    let 一段 = !有短寫法.contains(&漢字語幹)
        && 送假名數 > 1
        && 送假名開頭
            .is_some_and(|c| "いきしちにひみりぎじぢびぴえけせてねへめれげぜでべぺ".contains(c));
    let 有音便 = {
        // 一段的語尾帶著辭書形保留的那個假名（`変える` → 語尾「えった」），
        // 跳過它再看
        let 起點 = if 一段 { 1 } else { 0 };
        語尾
            .chars()
            .nth(起點)
            .is_some_and(|c| c == 'っ' || c == 'ん')
    };
    if 一段 && 有音便 {
        return None;
    }
    if !一段 && 語尾.starts_with(['た', 'て']) {
        return None;
    }
    Some(format!("{漢字語幹}{語尾}"))
}

/// 這個表記是**動詞**的寫法嗎——漢字＋u 段送假名。
fn 是動詞表記(w: &str) -> bool {
    let cs: Vec<char> = w.chars().collect();
    cs.len() >= 2
        && U段.contains(cs.last().unwrap())
        && cs[..cs.len() - 1].iter().any(|c| !是假名(*c))
}

/// 這一層分不開，也不需要分——兩邊都會還原到查得到的辭書形，
/// 對「這是不是活用形」這個問題答案相同。真正要選哪個字是選字層的事。
pub fn 辭書形(keys: &str) -> Option<String> {
    // **便宜的前置排除**：活用形與辭書形的尾巴一定落在這些字母裡。
    //
    // 這是熱路徑（排序每鍵要對上千個段落問一次），下面每條還原規則都
    // 要試著剝語尾再查詞典，貴得多。先用最後一個字母擋掉絕大多數段落。
    //
    // 涵蓋的是：辭書形的 u 段（u/ku/gu/su/tsu/nu/bu/mu/ru 全以 `u` 結尾）、
    // 過去形／て形（`ta`／`te`／`da`／`de` → a・e）、ない形（`nai` → i）、
    // ます形（`masu` → u）、意志形（`you` → u）、命令形（`ro`／`yo` → o）。
    // 也就是母音全都要，只有子音結尾能擋——羅馬字裡那是撥音 `nn` 與
    // 促音，兩者都不會是活用形的結尾。
    if !keys
        .chars()
        .last()
        .is_some_and(|c| matches!(c, 'a' | 'i' | 'u' | 'e' | 'o' | 'A' | 'I' | 'U' | 'E' | 'O'))
    {
        return None;
    }
    let low = keys.to_ascii_lowercase();
    // **太短的一律不算**。
    //
    // mozc 收了大量單假名詞條（`る`、`た`、`て`、`ない` 都在裡面），
    // 所以「查得到詞典」對短字串完全沒有鑑別力。而這些正是從中文串
    // 切出來的假名碎片——`ru`（ㄅㄐㄧ）、`su`（ㄋㄧ）、`fu`（ㄑㄧ）——
    // 被 `KANA_FRAGMENT` 罰的那批。認成活用形就等於把碎片保護起來。
    //
    // 動詞的辭書形最短是 `miru`／`neru`／`suru` 這種 4 個字元。
    if low.chars().count() < 4 {
        return None;
    }
    // 辭書形本身也算——「起きる」不是活用形，但問它時該答得出來。
    //
    // **要問「是不是動詞」不是「在不在詞典」**：`おきた`（起きた）碰巧
    // 也是詞典裡的「沖田」，用 `is_japanese_word` 的話這一關就先命中，
    // 後面的活用還原根本輪不到。「沖田」不是動詞，過不了 `辭書形成立`。
    if 辭書形成立(&low) {
        return Some(low);
    }
    // **順序有意義**：規則越specific的越先試。
    //
    // `suru` 那條最貪心——`saseru`／`masita` 這些語尾它也吃得下，
    // 於是 `okisaseru`（起きさせる）會被還原成不存在的 `okisuru`。
    // 把它擺到最後，一段／五段先有機會回答。
    一段還原(&low)
        .or_else(|| 五段還原(&low))
        .or_else(|| 不規則還原(&low))
        .or_else(|| 補助動詞鏈還原(&low))
}

/// **活用鏈**：活用形後面再接補助動詞，接兩層以上。
///
/// ```text
/// 起きてきた   okite    ＋ kita     （て形 ＋ 来る的過去）
/// 浴びられたい abirare  ＋ tai      （受身 ＋ 願望）
/// 進めていきます susume ＋ teikimasu（て形 ＋ 行く的敬體）
/// ```
///
/// 一層層剝，每剝掉一個補助動詞就把剩下的**再問一次**——遞迴，
/// 但因為每次都變短，一定會停。
///
/// 剝完剩下的要能自己還原成辭書形，整串才算活用形；這樣
/// `gakkousite`（学校 site）不會因為尾巴像 `te` 就中招。
fn 補助動詞鏈還原(low: &str) -> Option<String> {
    /// 能接在活用形後面的補助動詞（含它們自己的活用）。
    const 補助: &[&str] = &[
        "kita",
        "kite",
        "kimasu",
        "kuru", // ～てくる
        "iku",
        "ikimasu",
        "itta",
        "iru",
        "imasu", // ～ていく／～ている
        "tai",
        "takatta",
        "taku", // ～たい
        "oku",
        "oita",
        "okimasu",
        "okou", // ～ておく
        "shimau",
        "simau",
        "shimatta",
        "simatta", // ～てしまう
        "miru",
        "mita",
        "mimasu", // ～てみる
        "ageru",
        "ageta",
        "kureru",
        "kureta",
        "morau",
        "moratta",
        "moraeru", // 授受
        "kudasai",
        "itadaku",
        "itadakemasu",
    ];
    // **長的補助動詞優先剝**——`kurenai` 要排在 `kureru` 前面，
    // 不然 `tetudattekurenai` 會剝到一半停住。
    let mut hits: Vec<&&str> = 補助.iter().filter(|b| low.ends_with(**b)).collect();
    hits.sort_by_key(|b| std::cmp::Reverse(b.len()));
    for b in hits {
        let Some(head) = low.strip_suffix(*b) else {
            continue;
        };
        // 剝完太短的不算——剩下的要撐得起一個動詞
        if head.chars().count() < 3 {
            continue;
        }
        // 剝掉補助動詞之後，前面是て形／連用形。**て形要先補回動詞尾巴**
        // 才查得到辭書形：`tetudatte` 的 `tte` 是促音便，還原成 `tetudau`。
        // 這件事 `五段還原` 已經會做，原樣丟給它即可。
        if let Some(d) = 一段還原(head)
            .or_else(|| 五段還原(head))
            .or_else(|| 不規則還原(head))
        {
            return Some(d);
        }
        // 剝完還留著て形的（`erannde` ＋ `kudasai`）——把て／で 也
        // 拿掉再問一次。撥音便・促音便在這裡收。
        for t in ["te", "de"] {
            if let Some(h2) = head.strip_suffix(t) {
                if h2.chars().count() >= 2 {
                    // `erann` → `erabu`（撥音便）要交給 `五段還原` 去解，
                    // 所以拼回て形的樣子再問一次
                    let restored = format!("{h2}{t}");
                    if let Some(d) = 五段還原(&restored) {
                        return Some(d);
                    }
                }
            }
        }
    }
    None
}

/// する・来る——各自獨立，直接列。
fn 不規則還原(low: &str) -> Option<String> {
    // する系：し／さ／せ ＋ 語尾
    const SURU: &[&str] = &[
        "shita", "sita", "shite", "site", "shinai", "sinai", "shimasu", "simasu", "sareru",
        "saseru", "shiyou", "siyou", "sureba",
    ];
    // 来る系
    const KURU: &[&str] = &[
        "kita", "kite", "konai", "kimasu", "koyou", "kureba", "korareru",
    ];
    for s in SURU {
        if let Some(stem) = low.strip_suffix(s) {
            // `benkyousuru`（勉強する）這種複合動詞：**辭書形整個**要
            // 查得到，不能只問語幹。
            //
            // 只問語幹的話 `gakkousite`（学校 site）會中招——`gakkousi`
            // （学校し）確實在詞典裡，但 `gakkousuru` 不是動詞。
            if stem.is_empty() {
                return Some("suru".to_string());
            }
            let cand = format!("{stem}suru");
            if 辭書形成立(&cand) {
                return Some(cand);
            }
        }
    }
    for k in KURU {
        if low == *k {
            return Some("kuru".to_string());
        }
    }
    None
}

/// 一段：剝掉語尾，語幹加 `ru` 就是辭書形。
fn 一段還原(low: &str) -> Option<String> {
    // 長的語尾優先——`nakatta` 要贏過 `ta`，不然語幹會多剝走東西
    let mut cands: Vec<&&str> = 一段語尾.iter().filter(|s| low.ends_with(**s)).collect();
    cands.sort_by_key(|s| std::cmp::Reverse(s.len()));
    for suf in cands {
        let Some(stem) = low.strip_suffix(*suf) else {
            continue;
        };
        // 語幹太短的不算——那是碎片不是動詞
        if stem.chars().count() < 2 {
            continue;
        }
        let cand = format!("{stem}ru");
        // 一段動詞的辭書形一定是 `iru` 或 `eru` 結尾
        if !(cand.ends_with("iru") || cand.ends_with("eru")) {
            continue;
        }
        if 辭書形成立(&cand) {
            return Some(cand);
        }
    }
    None
}

/// 五段：兩條路——音便（て形・た形）與母音段變化（ない形・ます形…）。
fn 五段還原(low: &str) -> Option<String> {
    // ① 音便還原：尾巴對上就換回辭書形的尾巴
    // 行く 的例外先問——它只在整串就是那個形式時成立
    if let Some((_, dic)) = 行く例外.iter().find(|(a, _)| low == *a) {
        if 辭書形成立(dic) {
            return Some(dic.to_string());
        }
    }
    let mut hits: Vec<&(&str, &str)> = 音便還原.iter().filter(|(a, _)| low.ends_with(a)).collect();
    hits.sort_by_key(|(a, _)| std::cmp::Reverse(a.len()));
    // 候選先全部湊出來，**分兩趟挑**——見下面的說明
    let cands: Vec<String> = hits
        .iter()
        .filter_map(|(onbin, dic)| {
            let stem = low.strip_suffix(onbin)?;
            (!stem.is_empty()).then(|| format!("{stem}{dic}"))
        })
        .collect();
    // **第一趟：詞典首選就是動詞的**。
    //
    // 促音便有三種還原（る／う／つ），光靠固定順序決定不了：
    //
    // ```text
    // katta（買った）→ karu 也是動詞（狩る）→ ru 排前面就給錯答案
    // kaetta（帰った）→ kaeru 才對，但 kaetsu 也在詞典裡
    // ```
    //
    // 分界在**詞典首選**：`かう`→「買う」（動詞），`かる`→「軽」（名詞）。
    // 首選就是動詞的那個明顯更常用，優先取它。
    for cand in &cands {
        if 首選是動詞(cand) {
            return Some(cand.clone());
        }
    }
    // 第二趟：放寬成「動詞排在候選裡就好」
    for cand in &cands {
        if 辭書形成立(cand) {
            return Some(cand.clone());
        }
    }
    // ② 母音段還原：語尾的母音換回 `u`
    //
    // `kakanai` → 剝掉 `nai` → `kaka` → 尾巴 `a` 換 `u` → `kaku`
    //
    // **長的語尾優先**——`nakatta` 要贏過 `nai`，不然
    // `mitukaranakatta` 會被剝成 `mitukaranakat` 而還原失敗。
    let mut vs: Vec<&(char, &str)> = 段母音.iter().filter(|(_, s)| low.ends_with(s)).collect();
    vs.sort_by_key(|(_, s)| std::cmp::Reverse(s.len()));
    for (vowel, suf) in vs {
        let Some(stem) = low.strip_suffix(suf) else {
            continue;
        };
        let mut cs: Vec<char> = stem.chars().collect();
        if cs.len() < 3 || cs.last() != Some(vowel) {
            continue;
        }
        cs.pop();
        let base: String = cs.into_iter().collect();
        // **拗音音節不能只換最後一個字母**。
        //
        // `shi`（し）、`chi`（ち）、`tsu`（つ）在羅馬字裡是一整個音節，
        // 換段時整個要換：
        //
        // ```text
        // hanashimasu → 剝 masu → hanashi → 換段  hanasu   ○（話す）
        //                                  只換尾  hanashu  ✗
        // machimasu   → 剝 masu → machi   → 換段  matsu    ○（待つ）
        //                                  只換尾  machu    ✗
        // ```
        //
        // 所以候選要多幾個：把整個 i 段音節換掉的那些。
        let mut cands = vec![format!("{base}u")];
        for (i段, u段) in [
            ("sh", "su"),  // hanashi → hanasu
            ("s", "su"),   // hanasi  → hanasu（si 打法）
            ("ch", "tsu"), // machi   → matsu
            ("t", "tsu"),  // mati    → matsu（ti 打法）
            ("t", "tu"),   // mati    → matu
        ] {
            if let Some(head) = base.strip_suffix(i段) {
                if !head.is_empty() {
                    cands.push(format!("{head}{u段}"));
                }
            }
        }
        for cand in &cands {
            if 辭書形成立(cand) {
                return Some(cand.clone());
            }
        }
        // 五段裡 `る` 結尾的（取る・帰る…）——`u` 查不到就試 `ru`。
        // `iru`／`eru` 結尾卻是五段的那一批就落在這裡。
        let cand_ru = format!("{base}ru");
        if 五段例外.contains(&cand_ru.as_str()) && 辭書形成立(&cand_ru) {
            return Some(cand_ru);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dict;

    /// 這些測試要有詞典才有意義——沒載入時 `is_japanese_word` 一律回
    /// false，整套還原都會落空。CI 沒有詞典檔時直接跳過。
    fn 有詞典() -> bool {
        dict::japanese_loaded()
    }

    #[test]
    fn 一段動詞的活用形認得出來() {
        if !有詞典() {
            return;
        }
        for k in [
            "okita", "okite", "okinai", "abita", "abite", "tabeta", "tabete",
        ] {
            assert!(is_inflected(k), "{k} 該被認成活用形");
        }
    }

    #[test]
    fn 日文詞加英文詞不算活用形() {
        if !有詞典() {
            return;
        }
        // 這些是「日文詞＋英文詞」被誤黏，正是後綴表會誤中的那批
        for k in ["gakkousite", "onegaidata", "sigotomode", "hatuonnguide"] {
            assert!(!is_inflected(k), "{k} 不是活用形（那是日文詞＋英文詞）");
        }
    }

    #[test]
    fn 五段的音便還原得回正確的辭書形() {
        if !有詞典() {
            return;
        }
        // 同一個促音便形能還原成好幾個「查得到的詞」，`音便還原` 的表
        // 順序決定取哪個——這個測試把那個順序釘住。
        // `kaetta` 若還原成 `kaetsu` 就是表被改亂了。
        for (活用, 辭書) in [
            ("kaetta", "kaeru"),     // 帰った → 帰る（促音便·る）
            ("hashitta", "hashiru"), // 走った → 走る
            ("yonda", "yomu"),       // 読んだ → 読む（撥音便·む）
            ("nonda", "nomu"),       // 飲んだ → 飲む
            ("oyoida", "oyogu"),     // 泳いだ → 泳ぐ（イ音便）
        ] {
            assert_eq!(辭書形(活用).as_deref(), Some(辭書), "{活用}");
        }
    }

    #[test]
    fn 五段的未然形連用形認得出來() {
        if !有詞典() {
            return;
        }
        // a 段＋ない、i 段＋ます……這些不走音便，是換母音段
        for (活用, 辭書) in [
            ("kakanai", "kaku"),             // 書かない
            ("kakimasu", "kaku"),            // 書きます
            ("mitukaranakatta", "mitukaru"), // 見つからなかった（否定過去）
            ("matasareteiru", "matu"),       // 待たされている（使役受身＋進行）
        ] {
            assert_eq!(辭書形(活用).as_deref(), Some(辭書), "{活用}");
        }
    }

    #[test]
    fn 拗音音節換段時要整個換() {
        if !有詞典() {
            return;
        }
        // `shi`／`chi` 是一整個音節，只換最後一個字母會湊出 `hanashu`、
        // `machu` 這種不存在的東西
        for (活用, 辭書) in [
            ("hanashimasu", "hanasu"), // 話します → 話す
            ("hanasimasu", "hanasu"),  // （si 打法）
            ("machimasu", "matsu"),    // 待ちます → 待つ
            ("matimasu", "matu"),      // （ti 打法）
        ] {
            assert_eq!(辭書形(活用).as_deref(), Some(辭書), "{活用}");
        }
    }

    #[test]
    fn 假辭書形不算數() {
        if !有詞典() {
            return;
        }
        // 這些是還原規則湊出來、詞典**碰巧收了**的非動詞。
        // 光問「在不在詞典裡」會放行，要問「是不是動詞」才擋得掉。
        //
        //   あるきる → アルキル（alkyl）   はなしる → 鼻汁
        //   あそびる → アソビル            はいく   → 俳句
        for 假 in ["arukiru", "hanashiru", "asobiru", "haiku"] {
            let k = crate::romaji::kana::to_kana(假).expect("合法羅馬字");
            assert!(
                dict::is_japanese_word(假),
                "{假} 本來就在詞典裡，這個測試才有意義"
            );
            assert!(!是動詞(&k), "{假} 不是動詞（{k}）");
        }
        // 而真的動詞要過
        for 真 in ["aruku", "hanasu", "asobu", "hairu", "okiru", "taberu"] {
            let k = crate::romaji::kana::to_kana(真).expect("合法羅馬字");
            assert!(是動詞(&k), "{真} 是動詞（{k}）");
        }
    }

    #[test]
    fn 活用形組得出漢字() {
        if !有詞典() {
            return;
        }
        // 選字層靠這個把「おきた」寫成「起きた」——mozc 只收辭書形，
        // 活用形查不到，漢字要自己組
        for (活用, 漢字) in [
            ("okita", "起きた"),
            ("tabete", "食べて"),
            ("yonnda", "読んだ"), // 撥音便，`ん` 打 `nn`
            ("oyoida", "泳いだ"), // イ音便
            ("matta", "待った"),  // 促音便·つ
            ("katta", "買った"),  // 促音便·う
            ("totta", "取った"),  // 促音便·る
            ("sitta", "知った"),
            ("kitta", "切った"),
            ("haitta", "入った"),
            ("matimasu", "待ちます"),
            ("hanasimasu", "話します"),
        ] {
            assert_eq!(漢字表記(活用).as_deref(), Some(漢字), "{活用}");
        }
    }

    #[test]
    fn 複合動詞的語幹不能只取開頭的漢字() {
        if !有詞典() {
            return;
        }
        // 「話しかける」是 話→し→かける，漢字不連續。用「開頭連續的
        // 漢字」當語幹只會拿到「話」，組出「話けられた」。
        //
        // 期望列**兩種漢字寫法**——「話しかける」與「話し掛ける」詞典
        // 都收，哪個排第一取決於載入狀態，釘死其中一個會偶爾紅。
        // 這個測試要驗的是語幹沒被截斷，不是選了哪個表記。
        for (活用, 可接受) in [
            (
                "hanasikakerareta",
                &["話しかけられた", "話し掛けられた"][..],
            ),
            (
                "hikiuketekureta",
                &["引き受けてくれた", "引受けてくれた"][..],
            ),
            ("otituitekita", &["落ち着いてきた"][..]),
        ] {
            let 得 = 漢字表記(活用);
            assert!(
                得.as_deref().is_some_and(|w| 可接受.contains(&w)),
                "{活用} 得到 {得:?}，應該是 {可接受:?} 之一"
            );
        }
    }

    #[test]
    fn 五段例外的活用形組得出漢字() {
        if !有詞典() {
            return;
        }
        // `iru`／`eru` 結尾卻是五段的那批——這套規則最脆弱的地方。
        // 它們的過去形是促音便（帰った），一段判斷寫錯就會整批失效。
        //
        // 只收詞典確實有、而且日常會用到的；罕用字（照る・耀る）
        // 不放進來，那些卡在詞典資料而不是規則。
        for (按鍵, 語幹) in [
            ("kaetta", "帰"),
            ("hasitta", "走"),
            ("haitta", "入"),
            ("kitta", "切"),
            ("sitta", "知"),
            ("subetta", "滑"),
            ("ketta", "蹴"),
            ("hetta", "減"),
            ("kagitta", "限"),
            ("titta", "散"),
            ("nigitta", "握"),
            ("kudatta", "下"),
            ("hinetta", "捻"),
            ("maitta", "参"),    // 首選是「マイル」，片假名不該擋住動詞
            ("hotetta", "火照"), // 首選是「ホテル」，同上
            ("kosutta", "擦"),
            ("nonositta", "罵"),
            ("kutugaetta", "覆"),
            ("yomigaetta", "蘇"),
        ] {
            let 候選 = 漢字候選(按鍵);
            assert!(
                候選.iter().any(|w| w.starts_with(語幹)),
                "{按鍵} 的候選裡該有「{語幹}…」，實得 {候選:?}"
            );
        }
    }

    #[test]
    fn 習慣寫假名的動詞候選裡仍要有漢字() {
        if !有詞典() {
            return;
        }
        // **有歧義的不算錯，但候選要有**（使用者的裁決）。
        //
        // `ある`／`わかる`／`いく` 顯示維持假名是對的，但漢字寫法必須
        // 選得到——否則想打「行った」「分かりました」的人沒救。
        for (按鍵, 該有) in [
            ("itta", "行った"),
            ("arimasu", "有ります"),
            ("wakarimasita", "分かりました"),
            ("tetta", "照った"), // てる 首選是平假名，但「照った」要選得到
        ] {
            let 候選 = 漢字候選(按鍵);
            assert!(
                候選.iter().any(|w| w == 該有),
                "{按鍵} 的候選裡該有「{該有}」，實得 {候選:?}"
            );
        }
    }

    #[test]
    fn 習慣寫假名的基本動詞顯示維持假名() {
        if !有詞典() {
            return;
        }
        // 詞典首選是**平假名**的，**顯示**維持假名（ある・わかる・いく）。
        //
        // 只擋顯示不擋候選——漢字寫法仍要選得到，見
        // `習慣寫假名的動詞候選裡仍要有漢字`。
        for k in ["arimasu", "wakarimasita"] {
            assert_eq!(漢字表記(k), None, "{k} 顯示該維持假名");
        }
        // 片假名首選不算——那是外來語名詞碰巧同音（ほてる→ホテル），
        // 擋掉的話「火照った」連顯示都拿不到。
        assert!(
            漢字表記("hotetta").is_some(),
            "hotetta 不該被片假名首選擋掉"
        );
        assert!(漢字表記("maitta").is_some(), "maitta 不該被片假名首選擋掉");
    }

    #[test]
    fn 送假名有長短兩種寫法的是五段() {
        if !有詞典() {
            return;
        }
        // 「交じる」和「交る」是同一個五段動詞的兩種正書法。
        // 光看字面分不出來——「交じる」的送假名開頭是「じ」（い段），
        // 跟真正的一段「起きる」（き，い段）一模一樣。
        //
        // 分界在**長短並存**：五段的活用語尾只有最後一個音在變
        // （交る→交った），送假名可以只寫「る」；一段的語幹尾音固定
        // （起きる→起きた），省不掉。
        assert_eq!(漢字表記("mazitta").as_deref(), Some("交じった"));
        // 一段不該受影響
        assert_eq!(漢字表記("okita").as_deref(), Some("起きた"));
        assert_eq!(漢字表記("tabeta").as_deref(), Some("食べた"));
    }

    #[test]
    fn 同音動詞的候選要齊全() {
        if !有詞典() {
            return;
        }
        // 同一個活用形常常對應好幾個動詞，全部都要在候選裡——
        // 只給一個的話其餘的使用者選不回來
        let c = 漢字候選("yonnda");
        assert!(c.contains(&"読んだ".to_string()), "{c:?}");
        assert!(c.contains(&"詠んだ".to_string()), "{c:?}");
        let c = 漢字候選("totta");
        for w in ["取った", "撮った", "採った"] {
            assert!(c.contains(&w.to_string()), "{w} 不在 {c:?}");
        }
    }

    #[test]
    fn 一段與五段的活用形不互串() {
        if !有詞典() {
            return;
        }
        // `かえる` 同時有 変える（一段）和 帰る（五段）。
        // 兩種活用形各有各的按鍵，不該互相污染——但合起來要涵蓋所有動詞。
        let 促音 = 漢字候選("kaetta");
        let 直接 = 漢字候選("kaeta");
        assert!(促音.contains(&"帰った".to_string()), "{促音:?}");
        assert!(
            !促音.iter().any(|w| w.contains("変え")),
            "一段不會促音便：{促音:?}"
        );
        assert!(直接.contains(&"変えた".to_string()), "{直接:?}");
        assert!(
            !直接.iter().any(|w| w == "帰た"),
            "五段不會直接接た：{直接:?}"
        );
        // 五段的多字送假名不能被誤判成一段（終わる → 終わった 不是 終った）
        assert_eq!(漢字表記("owatta").as_deref(), Some("終わった"));
    }

    #[test]
    fn 習慣寫假名的動詞不轉漢字() {
        if !有詞典() {
            return;
        }
        // `ある`／`わかる` 的日常寫法就是假名，硬轉成「有ります」
        // 「分かりました」反而是錯的（實測 cutpoint 節賠 14 句）。
        // 判準是辭書形的詞典首選——那些詞的首選本來就是假名。
        for 假名詞 in ["arimasu", "arimasita", "wakarimasita", "wakaranai"] {
            assert_eq!(漢字表記(假名詞), None, "{假名詞} 該維持假名");
        }
    }

    #[test]
    fn 入った還原成入る不是歩く() {
        if !有詞典() {
            return;
        }
        // `haitta` 曾經還原成 `haiku`（俳句）——促音便的 `ru` 那條沒中，
        // 掉到イ音便的 `ku`，而「はいく」在詞典裡。
        assert_eq!(辭書形("haitta").as_deref(), Some("hairu"));
        assert_eq!(辭書形("haitte").as_deref(), Some("hairu"));
    }

    #[test]
    fn 碎片不算活用形() {
        if !有詞典() {
            return;
        }
        // `KANA_FRAGMENT` 防的那批：從中文串切出來的短假名
        for k in ["ru", "su", "fu", "vu", "xu", "ta", "te", "ita", "tta"] {
            assert!(!is_inflected(k), "{k} 是碎片不是活用形");
        }
    }
}
