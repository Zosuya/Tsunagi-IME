/// (羅馬字, 期望漢字)
///
/// 「詞庫查得到整詞」與「要靠 DP 組」兩類都收，因為分水嶺正好落在這裡：
/// 十一〜十八日詞庫有整詞所以一直是對的，二十幾日沒有所以全滅。
const CASES: &[(&str, &str)] = &[
    // 訓讀日（一〜十日）。詞庫多半有整詞，但阿拉伯數字版常排前面。
    ("tsuitachi", "一日"),
    ("futsuka", "二日"),
    ("mikka", "三日"),
    ("yokka", "四日"),
    ("itsuka", "五日"),
    ("muika", "六日"),
    ("nanoka", "七日"),
    ("youka", "八日"),
    ("kokonoka", "九日"),
    ("tooka", "十日"),
    // 十一〜十九日。`juuyokka`/`juukunichi` 要組，其餘詞庫有整詞。
    ("juuichinichi", "十一日"),
    ("juuninichi", "十二日"),
    ("juusannichi", "十三日"),
    ("juuyokka", "十四日"),
    ("juugonichi", "十五日"),
    ("juurokunichi", "十六日"),
    ("juushichinichi", "十七日"),
    ("juuhachinichi", "十八日"),
    ("juukunichi", "十九日"),
    // 二十〜三十一日。詞庫幾乎都沒有，全靠 DP 組——原本全滅的就是這一段。
    ("hatsuka", "二十日"),
    ("nijyuuichinichi", "二十一日"),
    ("nijyuuninichi", "二十二日"),
    ("nijyuuyokka", "二十四日"),
    ("nijyuugonichi", "二十五日"),
    ("nijyuuhachinichi", "二十八日"),
    ("nijyuukunichi", "二十九日"),
    ("sanjyuunichi", "三十日"),
    ("sanjyuuichinichi", "三十一日"),
    // 月份。12 個都是詞庫有整詞，只是阿拉伯數字排第一（排序問題，可接受）。
    ("ichigatsu", "一月"),
    ("gogatsu", "五月"),
    ("juunigatsu", "十二月"),
    // 星期與相對日期。詞庫都有整詞，本來就正常，當對照組。
    ("getsuyoubi", "月曜日"),
    ("doyoubi", "土曜日"),
    ("kyou", "今日"),
    ("ashita", "明日"),
    ("kongetsu", "今月"),
    // 月日連打。長度接近上限，最容易掉。
    ("gogatsutsuitachi", "五月一日"),
];
