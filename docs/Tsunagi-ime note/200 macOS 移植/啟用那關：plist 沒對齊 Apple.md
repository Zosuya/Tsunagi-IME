---
原編號: "2.52.17"
date: 2026-09-08
status: done
一句話: "PrimaryInScript 該是 false、KeyEquivalentModifiers 是孤兒，兩個改完已啟用 9→11；判準要用 TISCreateInputSourceList(nil,false)"
pitfall: true
相關:
  - "[[輸入源註冊不進系統：三個假設都錯，根因是 bundle id]]"
---

# 啟用那關：plist 沒對齊 Apple

接 [[輸入源註冊不進系統：三個假設都錯，根因是 bundle id]]。**註冊進系統之後還有第二關**：輸入源要「啟用」才會出現在
輸入選單裡。這一關又卡了一輪，而且症狀跟上一關一模一樣——**回 noErr，
什麼都沒發生**。

**結論先講**：

> 模式字典裡 `tsInputModePrimaryInScriptKey` 寫成 `true`、以及
> `tsInputModeKeyEquivalentModifiersKey` 沒有配對的
> `tsInputModeKeyEquivalentKey`，這兩個欄位讓輸入源進不了啟用清單。
> 兩個都對齊 Apple 之後，已啟用數 9 → **11**（輸入法本體＋模式），
> 選單裡就出現了。

**兩個欄位為什麼是錯的**（拿 Apple 自己的 TCIM 逐項比對出來的）：

| 欄位 | 我原本 | Apple | 為什麼 |
|---|---|---|---|
| `tsInputModePrimaryInScriptKey` | `true` | **每個模式都 `false`**（注音／倉頡／速成／拼音無一例外） | `true` 等於宣稱自己是整個繁中書寫系統的**首選**輸入法，跟內建的繁體中文輸入法搶 |
| `tsInputModeKeyEquivalentModifiersKey` | 有，`0` | **一定跟 `tsInputModeKeyEquivalentKey` 成對**（如倉頡 `"C"` ＋ `4608`） | 只有修飾鍵、沒有鍵，是個孤兒 |

順帶照 Apple 補了 `TISIconLabels`／`Primary`（給「通」）——**選單列上顯示的
短標籤**，Apple 每個模式都給（注音給「注」）。沒有它就只剩那張 tiff，在
彩色底上是一團黑。

### 判準搞錯了，白繞一大圈

**`defaults read com.apple.HIToolbox AppleEnabledInputSources` 不是判準。**
修好之後那個鍵**到現在仍然是 9 筆、裡面沒有我們**，但輸入法確實已啟用、
選單裡也有。原因是 `tsInputModeDefaultStateKey` 為 `true` 的模式**預設就是
啟用**，不需要在偏好裡留一筆。

唯一可靠的判準是：

```swift
TISCreateInputSourceList(nil, false)   // includeAllInstalled = false ⇒ 只回已啟用的
```

`tisq.swift` 現在就是印這個數字。

### 被推翻的（自己在 [[輸入源註冊不進系統：三個假設都錯，根因是 bundle id]] 裡寫的）

[[輸入源註冊不進系統：三個假設都錯，根因是 bundle id]] 一度寫下「`TISEnableInputSource` 回 noErr 但一律不落地，合理推測是
macOS 26 刻意的安全閘、第三方輸入法只能由使用者親手加」。**那是錯的。**
量到的現象都對（命令列與前景 GUI 都回 0、偏好紋風不動、行程寫得動偏好、
`IsEnableCapable` 是 1），但**推論錯了**——當時看的是錯的鍵，而且 plist 還壞著。

教訓跟 [[輸入源註冊不進系統：三個假設都錯，根因是 bundle id]] 同一條，而且是**同一天犯第二次**：

> **靜默失敗時，先確認「我用來判斷成敗的那個東西，是不是真的能反映成敗」，
> 再去推論系統為什麼拒絕我。** 兩次都是「API 回 noErr、我查的地方沒變化」，
> 兩次的真相都不是系統在擋，而是我的 bundle 有欄位不合格、加上我查錯地方。

### 這一關的完整流程（下次照做）

1. `build-app.sh` 建好裝進 `~/Library/Input Methods/`
2. `tisq register`——強制系統重掃，不用登出登入
3. `open ~/Library/Input\ Methods/Tsunagi.app`——**經 LaunchServices 啟動一次**
   （實測 `open` 起得來、`IMKServer` 正常；系統自己拉輸入法走的也是這條）
4. `tisq`——看「已啟用共 N 個」裡有沒有自己
5. 選單列切過去打字

**每個行程各有一份輸入源快取副本**（[[輸入源註冊不進系統：三個假設都錯，根因是 bundle id]]），所以改完 plist 之後
**系統設定要整個關掉重開**，不然它拿的還是舊的——中間顯示名稱一直是一長串
bundle id，就是這個原因，砍掉它的快取副本再重開就正常了。
