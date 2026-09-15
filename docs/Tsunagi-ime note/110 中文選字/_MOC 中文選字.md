---
tags: [MOC]
---

# MOC 中文選字

> 注音選字的瓶頸是**資料不是演算法**——詞頻推不出句子語境。
> 接了 bigram 之後仍要記得：擴充包的詞必須豁免，否則會被推翻。

## 全部

```dataview
TABLE WITHOUT ID
  choice(status = "done", "✅ 完成",
  choice(status = "unverified", "🔨 未驗收",
  choice(status = "rejected", "⛔ 不做",
  choice(status = "paused", "⏸ 暫停",
  choice(status = "planned", "⏳ 待做",
  choice(status = "superseded", "🔄 已取代", "❓")))))) + choice(status_note, " — " + status_note, "") AS 狀態,
  file.link AS 筆記,
  一句話
FROM "110 中文選字"
WHERE file.name != "_MOC 中文選字" AND status != "rejected"
SORT choice(status = "rejected", 1, choice(status = "paused", 2, choice(status = "planned", 3, choice(status = "unverified", 4, choice(status = "superseded", 5, 6))))) ASC, file.name ASC
```

## ⛔ 量完決定不做——不要重新提議

> 這些是**實測否決**的，不是還沒做。理由都寫在筆記裡，重新提議前先讀。

```dataview
TABLE WITHOUT ID file.link AS 筆記, 一句話, status_note AS 補充
FROM "110 中文選字"
WHERE status = "rejected"
SORT file.name ASC
```

## 這一類的坑

```dataview
TABLE WITHOUT ID file.link AS 筆記, 一句話
FROM "110 中文選字"
WHERE pitfall = true
SORT file.name ASC
```
