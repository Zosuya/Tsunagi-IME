---
tags: [MOC]
---

# MOC 設定頁 UI

> egui-winit 會把輸入法吃掉的鍵**用實體鍵位補回來**——方向鍵選字丟字、Enter 中止組字都是它。
> Grid 裡自由文字的欄一定要設上限，一欄撐寬會讓整頁跟著變寬。

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
FROM "160 設定頁 UI"
WHERE file.name != "_MOC 設定頁 UI" AND status != "rejected"
SORT choice(status = "rejected", 1, choice(status = "paused", 2, choice(status = "planned", 3, choice(status = "unverified", 4, choice(status = "superseded", 5, 6))))) ASC, file.name ASC
```

## ⛔ 量完決定不做——不要重新提議

> 這些是**實測否決**的，不是還沒做。理由都寫在筆記裡，重新提議前先讀。

```dataview
TABLE WITHOUT ID file.link AS 筆記, 一句話, status_note AS 補充
FROM "160 設定頁 UI"
WHERE status = "rejected"
SORT file.name ASC
```

## 這一類的坑

```dataview
TABLE WITHOUT ID file.link AS 筆記, 一句話
FROM "160 設定頁 UI"
WHERE pitfall = true
SORT file.name ASC
```
