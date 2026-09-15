---
tags: [MOC]
---

# MOC 踩坑與陷阱

> 這一頁**橫跨整個 vault**，撈所有標了 `pitfall: true` 的筆記。
>
> 什麼算坑：**花了時間才查出來的**。症狀跟根因差很遠、或是「以為是 A
> 其實是 B」的那種。單純的功能實作不算。

## 還沒解決的

> 標了坑、狀態卻不是完成——這些**還在咬人**。

```dataview
TABLE WITHOUT ID
  choice(status = "unverified", "🔨 未驗收",
  choice(status = "planned", "⏳ 待做",
  choice(status = "paused", "⏸ 暫停",
  choice(status = "rejected", "⛔ 不做",
  choice(status = "superseded", "🔄 已取代", "❓"))))) + choice(status_note, " — " + status_note, "") AS 狀態,
  file.folder AS 類別,
  file.link AS 筆記,
  一句話
FROM ""
WHERE pitfall = true AND status != "done"
SORT file.folder ASC, file.name ASC
```

## 全部的坑

```dataview
TABLE WITHOUT ID
  file.folder AS 類別,
  file.link AS 筆記,
  一句話
FROM ""
WHERE pitfall = true
SORT file.folder ASC, file.name ASC
```
