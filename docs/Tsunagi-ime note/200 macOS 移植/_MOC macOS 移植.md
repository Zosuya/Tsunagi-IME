---
tags: [MOC]
---

# MOC macOS 移植

> **panic 不得穿過 ObjC 邊界**，`define_class!` 裡每支方法都要包 `guard::catch`。
> 改圖示或模式定義要跳 `CFBundleVersion`；`Ctrl+…` 到不了 macOS 的輸入法。

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
FROM "200 macOS 移植"
WHERE file.name != "_MOC macOS 移植" AND status != "rejected"
SORT choice(status = "rejected", 1, choice(status = "paused", 2, choice(status = "planned", 3, choice(status = "unverified", 4, choice(status = "superseded", 5, 6))))) ASC, file.name ASC
```

## ⛔ 量完決定不做——不要重新提議

> 這些是**實測否決**的，不是還沒做。理由都寫在筆記裡，重新提議前先讀。

```dataview
TABLE WITHOUT ID file.link AS 筆記, 一句話, status_note AS 補充
FROM "200 macOS 移植"
WHERE status = "rejected"
SORT file.name ASC
```

## 這一類的坑

```dataview
TABLE WITHOUT ID file.link AS 筆記, 一句話
FROM "200 macOS 移植"
WHERE pitfall = true
SORT file.name ASC
```
