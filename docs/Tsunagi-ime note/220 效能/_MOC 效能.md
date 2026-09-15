---
tags: [MOC]
---

# MOC 效能

> **排序是熱路徑**，一幀預算 16ms；新增判斷走 `Memo` 快取，改完跑 `bench_typing` 複量。
> 瓶頸多半是重複查詢與零碎配置，不是演算法本身——先量再改。

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
FROM "220 效能"
WHERE file.name != "_MOC 效能" AND status != "rejected"
SORT choice(status = "rejected", 1, choice(status = "paused", 2, choice(status = "planned", 3, choice(status = "unverified", 4, choice(status = "superseded", 5, 6))))) ASC, file.name ASC
```

## ⛔ 量完決定不做——不要重新提議

> 這些是**實測否決**的，不是還沒做。理由都寫在筆記裡，重新提議前先讀。

```dataview
TABLE WITHOUT ID file.link AS 筆記, 一句話, status_note AS 補充
FROM "220 效能"
WHERE status = "rejected"
SORT file.name ASC
```

## 這一類的坑

```dataview
TABLE WITHOUT ID file.link AS 筆記, 一句話
FROM "220 效能"
WHERE pitfall = true
SORT file.name ASC
```
