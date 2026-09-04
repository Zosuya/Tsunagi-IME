const CASES: &[(&str, &str)] = &[
    // 格助詞在句中
    ("watashihagakuseidesu", "私は学生です"),
    ("nihonkarakimashita", "日本から来ました"),
    ("gakkoumadearukimasu", "学校まで歩きます"),
    ("tomodachitoaimasu", "友達と会います"),
    ("hongayomitai", "本が読みたい"),
    ("mizuwonomimasu", "水を飲みます"),
    ("tokyouniikimasu", "東京に行きます"),
    // 副助詞在句中
    ("korededaijoubu", "これで大丈夫"),
    ("sukoshidakekudasai", "少しだけください"),
    ("ichijikanhodomachimashita", "一時間ほど待ちました"),
    // 接續助詞在句中
    ("takaikedokaimasu", "高いけど買います"),
    ("samuinodekaerimasu", "寒いので帰ります"),
    ("tabenagarahanasu", "食べながら話す"),
    // 終助詞在句尾
    ("ashitaikimasune", "明日行きますね"),
    ("sorehaiidesuyo", "それはいいですよ"),
    ("dokoheikimasuka", "どこへ行きますか"),
    ("ashitaharedarouka", "明日晴れるだろうか"),
    // 助動詞・敬語：詞庫的片假名 cost 異常（デス cost=0 < です cost=40）
    ("desu", "です"),
    ("soudesu", "そうです"),
    ("gozaimasu", "ございます"),
    ("arigatougozaimasu", "ありがとうございます"),
    // 慣用平假名的和語（整句組詞的 DP 最容易把它們拆成漢字碎片）
    ("iikara", "いいから"),
    ("soudesune", "そうですね"),
    ("sonnakotonai", "そんなことない"),
    ("nandemonai", "なんでもない"),
    ("chottomatte", "ちょっと待って"),
    ("dakedone", "だけどね"),
    // 使用者實際回報過的
    ("iitennkikara", "いい天気から"),
    ("gannbattene", "頑張ってね"),
    ("osannposimasyou", "お散歩しましょう"),
];
