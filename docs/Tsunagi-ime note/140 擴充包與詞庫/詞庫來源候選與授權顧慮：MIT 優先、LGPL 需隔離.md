---
原編號: "2.3"
status: done
一句話: 四類詞庫的來源候選與授權顧慮——MIT 優先、LGPL 需隔離、日文資料逐一確認
---

# 詞庫來源候選與授權顧慮：MIT 優先、LGPL 需隔離

> **這是早期的候選清單，不是實際採用的來源。** 實際在用的是 McBopomofo
> `BPMFMappings.txt`／`BPMFBase.txt`（MIT）、mozc 辭典（BSD-3）、FrequencyWords
> `en_50k`（MIT，取代表中的 wordfreq）、教育部字頻（CC BY-ND）、RIME 八股文
> bigram（LGPL-3.0）等——逐項清單與授權判斷見 `data/README.md`、`CREDITS.md`
> 與 [[發布前的授權盤點：資料比程式碼難]]；台語資料用台文華文線頂辭典
> （CC BY-SA 4.0），見 [[台語怎麼接進引擎：包層覆蓋、tw 層被 revert，最後段選單＋國字雙向詞表]]。

| 語言 | 來源候選 | 授權注意 |
|---|---|---|
| 注音字/詞庫 | McBopomofo 詞庫（MIT）、libchewing 詞庫（LGPL，注意連結方式）、教育部辭典 | MIT 優先；LGPL 需隔離 |
| 日文詞典 | mozc 資料（BSD）、SKK-JISYO（GPL 版本眾多，挑授權相容者）、JMdict（CC BY-SA） | 逐一確認再引入 |
| 英文詞頻 | Google Books Ngram 衍生 frequency list、wordfreq | 多為寬鬆授權 |
| 混打評測語料 | 自行錄製鍵擊 log（本人日常輸入）＋ 合成語料 | 自有 |
