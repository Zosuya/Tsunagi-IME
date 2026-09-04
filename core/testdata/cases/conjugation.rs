/// 使用者提供的測試資料：(羅馬字輸入, 期望輸出, 文法點)
///
/// 這批是刻意挑難的——使役受身、受身+てもらう、なければ+受身+てしまう
/// ——但都是日常會用到的範圍，不是文語或方言。
const CASES: &[(&str, &str, &str)] = &[
    ("zangyousaserareta", "残業させられた", "使役受身"),
    ("shuppatsushiteokeba", "出発しておけば", "て形+ておけば"),
    ("naosaretemo", "直されても", "受身+ても"),
    (
        "manzokushitemoraenai",
        "満足してもらえない",
        "てもらう可能否定",
    ),
    ("matasareru", "待たされる", "使役受身"),
    ("ogotteageru", "奢ってあげる", "てあげる"),
    ("oshitsukerareteiru", "押し付けられている", "受身進行形"),
    ("teishutsushinakereba", "提出しなければ", "なければ"),
    ("otosareteshimau", "落とされてしまう", "受身+てしまう"),
    ("kiiteireba", "聞いていれば", "ていれば"),
    ("shucchousaserarete", "出張させられて", "使役受身て形"),
    ("naosarete", "直されて", "受身て形"),
    ("otosarete", "落とされて", "受身て形"),
    ("oshitsukerare", "押し付けられ", "受身語幹"),
    ("manzokushite", "満足して", "サ変て形"),
    ("teishutsushite", "提出して", "サ変て形"),
    ("tsuzukeru", "続ける", "一段基本形"),
    ("kiite", "聞いて", "て形"),
    ("ogotte", "奢って", "て形"),
    ("naranakatta", "ならなかった", "なかった"),
    // 接頭詞「お」讓長度超過 compose_sentence 的上限，整段就組不出漢字
    // （9 個假名 vs 上限 8）。`sannposimasyou` 剛好在門檻內所以正常，
    // 差別只在開頭那個「お」——2026-08-25 把上限放寬到 12。
    ("osannposimasyou", "お散歩しましょう", "接頭詞+複合動詞"),
    ("sannposimasyou", "散歩しましょう", "複合動詞"),
    ("osannpo", "お散歩", "接頭詞"),
];
