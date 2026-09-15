---
tags: [MOC]
---

# MOC 待辦與決策

> 這一頁**橫跨整個 vault**，撈的是散在各主題資料夾裡的筆記。

## 還沒結束的事

```dataview
TABLE WITHOUT ID
  choice(status = "unverified", "🔨 未驗收",
  choice(status = "planned", "⏳ 待做",
  choice(status = "paused", "⏸ 暫停", "❓"))) + choice(status_note, " — " + status_note, "") AS 狀態,
  file.folder AS 類別,
  file.link AS 筆記,
  一句話
FROM ""
WHERE status = "planned" OR status = "unverified" OR status = "paused"
SORT choice(status = "unverified", 1, choice(status = "planned", 2, 3)) ASC, file.folder ASC
```

## ⛔ 量完決定不做——不要重新提議

> 這些是**實測否決**的，不是還沒做。提議任何看起來很聰明的點子之前，
> 先在這裡搜一下——理由都寫在筆記裡。

```dataview
TABLE WITHOUT ID
  file.folder AS 類別,
  file.link AS 筆記,
  一句話,
  status_note AS 補充
FROM ""
WHERE status = "rejected"
SORT file.folder ASC, file.name ASC
```

## 🔄 被後來的做法取代

```dataview
TABLE WITHOUT ID
  file.folder AS 類別,
  file.link AS 筆記,
  一句話,
  superseded_by AS 被誰取代
FROM ""
WHERE status = "superseded"
SORT file.folder ASC, file.name ASC
```

## ✅ 已完成

> 放最後——完成的通常不需要再看，但要查「這件事做過沒」時就在這裡。

```dataview
TABLE WITHOUT ID
  file.folder AS 類別,
  file.link AS 筆記,
  一句話
FROM ""
WHERE status = "done"
SORT file.folder ASC, file.name ASC
```
