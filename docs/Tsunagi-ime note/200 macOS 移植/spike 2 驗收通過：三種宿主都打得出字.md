---
原編號: "2.52.18"
date: 2026-09-08
status: done
一句話: "按鍵進得來／標記文字出得去／送出進得了宿主，TextEdit、瀏覽器、終端機三種全過，整份移植規劃站得住"
相關:
  - "[[輸入源註冊不進系統：三個假設都錯，根因是 bundle id]]"
  - "[[啟用那關：plist 沒對齊 Apple]]"
---

# spike 2 驗收通過：三種宿主都打得出字

[[macOS 移植的 spike 排序：先做 Echo 輸入法，五個全部結案]] 排第一順位的那個 spike——理由是「風險最高，不通則整份規劃作廢」
——**通過了**。

| 驗收項目 | TextEdit | 瀏覽器 | 終端機 |
|---|---|---|---|
| 按鍵進得來（`handleEvent:client:`） | ✅ | ✅ | ✅ |
| 標記文字出得去（`setMarkedText:`，帶底線） | ✅ | ✅ | ✅ |
| 送出進得了宿主（`insertText:`） | ✅ | ✅ | ✅ |

打 `abc` 出現帶底線的 `abc`、Backspace 退一個、Enter 送出、Esc 取消，
三種宿主行為一致。**Windows 那邊 §2.38 栽的瀏覽器這次沒事**——
`setMarkedText:` 不像 TSF 要自己算座標、註冊 display attribute provider，
底線是 attributed string 的一部分（[[TSF → IMK 對照表]] 的預期成立）。

**這代表整份 macOS 移植規劃站得住**：

| 層 | 狀態 |
|---|---|
| 引擎（core） | 確定，漏斗跟 Windows 逐節相同（[[詞庫就位後的完整量測：引擎跟 Windows 逐節相同]]） |
| 繪製層 | 確定，1,452 行可攜、卡點 4 個方法（[[繪製層在 Mac 上量了一遍：1452 行已可攜]]） |
| 輸入法整合（IMK） | **確定**，三件事都通 |

### 兩個靜默失敗吃掉了大半天

| 關卡 | 症狀 | 根因 |
|---|---|---|
| 註冊 | `TISRegisterInputSource` 回 noErr，輸入源清單紋風不動 | bundle id 少了 `.inputmethod.`，掃描器直接跳過（[[輸入源註冊不進系統：三個假設都錯，根因是 bundle id]]） |
| 啟用 | `TISEnableInputSource` 回 noErr、系統設定按「加入」也沒反應 | `tsInputModePrimaryInScriptKey` 該是 `false`、`tsInputModeKeyEquivalentModifiersKey` 是孤兒（[[啟用那關：plist 沒對齊 Apple]]） |

兩關的共同點值得記住：**API 回 noErr 完全不代表事情發生了，而「我拿來判斷
成敗的那個東西」本身也可能是錯的**。真正有用的動作只有兩個——把 Apple 自己
的 bundle 逐欄位比對，以及打開 `AppleTISTraceCacheRebuild` 看系統到底有沒有
讀我們的 Info.plist。

### 接 core 之前一定要改掉的兩處（[[spike 2：IMK Echo 輸入法]] 就記著）

1. ~~**組字緩衝從 `thread_local` 改成 ivar**~~ **2026-09-08 做了**，見
   [[組字緩衝從 thread_local 換成 ivar]]
2. **`NSString` 改成 `NSAttributedString`**：要分「已轉換／未轉換」兩種底線

### 下一步

[[macOS 移植的 spike 排序：先做 Echo 輸入法，五個全部結案]] 的順序：**spike 4（插入點座標）→ spike 3（候選視窗）→ spike 5
（快捷鍵攔截）**。spike 4 排前面是刻意的——若多數 App 拿不到座標，候選視窗
的設計會完全不一樣，先畫會白畫。
