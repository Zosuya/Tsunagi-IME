---
原編號: "2.52.21"
date: 2026-09-08
status: done
一句話: "egui 這條是死結（不搶焦點是 NSPanel 建立時的 style mask）；路上把宿主弄當兩次，都不是 panic、catch_unwind 攔不到"
pitfall: true
相關:
  - "[[執行模型的根本差異]]"
  - "[[spike 4：插入點座標 7 個宿主都拿得到]]"
---

# spike 3：候選視窗用 NSPanel

[[macOS 移植的 spike 排序：先做 Echo 輸入法，五個全部結案]] 排第 3 順位的 spike。要回答的是「候選視窗用哪套畫」，而真正的
未知數是**做不做得出「浮在宿主上方、點它也不搶焦點」的面板**。

### 結論：用 `NSPanel`，egui 那條路在 macOS 是死結

[[macOS 移植的 spike 排序：先做 Echo 輸入法，五個全部結案]] 原本列的兩個選項是「手刻 Cocoa」或「沿用設定頁的 egui 預覽」。
**egui 這條可以直接刪掉**：egui 在 macOS 要透過 winit 開視窗，winit 開的是
`NSWindow`，而**不搶焦點在 macOS 是 `NSPanel` 建立時的
`NonactivatingPanel` style mask 決定的**——它不是事後拿 API 關得掉的屬性。
這不是「egui 比較醜」的取捨，是它做不到。

實作在 `platform/macos/src/candidate_panel.rs`，關鍵設定：

| 設定 | 為什麼 |
|---|---|
| `NonactivatingPanel \| Borderless` | 整個 spike 的重點。少了它點面板會把焦點從宿主搶走 |
| `setLevel(NSPopUpMenuWindowLevel)`（101） | 候選視窗跟選單同類，要蓋得過宿主自己的浮動面板。`NSFloatingWindowLevel`（3）不夠 |
| `setBecomesKeyOnlyIfNeeded(true)` | 預設是「點了就變 key window」，那會中斷組字 |
| `setHidesOnDeactivate(false)` | 我們是 `LSUIElement`，本來就不會「作用中」；沒這行面板會在切宿主時自己消失 |
| `CanJoinAllSpaces \| FullScreenAuxiliary \| Stationary` | 換桌面、宿主全螢幕都要看得到 |
| `orderFront`（**不是** `makeKeyAndOrderFront`） | 後者會把面板變 key window，焦點就跑了 |

定位靠 [[spike 4：插入點座標 7 個宿主都拿得到]] 的成果：問 `attributesForCharacterIndex:0`，貼在那一行下面；
掉出螢幕就翻到上方、往左收。

### 收面板的時機：`set_marked` 收不完

第一版把「收面板」掛在「組字區清空」上，實測**滑鼠點宿主空白處送出時面板
不會消失**——那條路走的是 `commitComposition:`，根本不經過 `set_marked`。

**兩個地方都要收，而且要無條件收**：

- `commit()` 一開頭就 `hide()`，放在「緩衝是空的就提前 return」**之前**
- 另外實作 `deactivateServer:`——跟 `commitComposition:` **不是同一件事**：
  宿主內部換焦點走前者，離開這個輸入法走後者。少了它，切走之後面板會留在
  螢幕上

### ★ 把宿主弄當兩次，兩次都不是 panic ★

這一節最重要的收穫。**都是在宿主行程裡崩的，`catch_unwind` 完全無能為力。**

**崩 1：`setMarkedText:` 傳純 `NSString`**

```
*** CFEqual() called with NULL second argument ***
CFEqual
-[NSTextInputContext _forceAttributedString]
...
-[IMKInputSession_Modern setMarkedText:selectionRange:replacementRange:]
```

Terminal 當場 `SIGTRAP`。AppKit 想把我們的純字串強制轉成 attributed string
時踩到 NULL。修法是包成帶兩個 attribute 的 `NSAttributedString`：

- `NSUnderlineStyle` — 底線（macOS 的底線是 attribute 的一部分，不像 TSF
  要另外註冊 display attribute provider，[[TSF → IMK 對照表]]）
- `NSMarkedClauseSegment` — 第幾個「文節」。日文輸入法拿它把組字區切段
  各畫各的底線。我們只有一段固定給 `0`，**但不能不給**

**[[spike 2：IMK Echo 輸入法]] 把「傳純 NSString」記成「之後要改成帶 attribute 的，才分得出
兩種底線」——那是低估了，它不是美觀問題，是會弄當宿主。**

**崩 2：repertoire 裡的 `Latn` 讓 `ASCIICapable` 變成 1**

```
*** CFRelease() called with NULL ***
-[TUINSCursorUIController _selectCurrentInputSource]
-[TUINSCursorUIController moveTextInputMenuHUD:]
```

切換到我們的輸入法就崩，重現率 100%。**查法是把我們的輸入源跟 Apple 注音的
每個 TIS 屬性逐一倒出來比對**，只有一個不同：

| | 我們 | Apple 注音 |
|---|---|---|
| `ASCIICapable` | **1** | 0 |
| `KeyLayoutData` | nil | nil |

「宣稱能直接產生 ASCII、卻沒有鍵盤配置」。實測驗證：`tsInputModeCharacterRepertoireKey`
裡的 `Latn` 就是那個旗標的來源——拿掉它 `ASCIICapable` 立刻翻成 0，崩潰
也就沒了。repertoire 只是給系統 UI 分類用的中繼資料，**拿掉不影響我們實際
輸出英文**。

⚠ **但這個結論有個反例，不要當成定律**：Apple 的日文羅馬字輸入法
（打拉丁字母、出日文，跟通譯最像）有兩個模式，`Roman` 那個就是
`ASCIICapable=1`，而且不會崩。所以不是「ASCIICapable=1 一定崩」，比較可能
是「**宣稱 ASCII-capable、卻沒有一個真的能打 ASCII 的模式或鍵盤配置**」。

**這件事背後是個設計差異，日後還會碰到**：Kotoeri 把中英分成**兩個模式**，
而通譯的整個賣點是**一個模式吃中英日、不用手動切**。系統的模型偏向前者。
真的要讓系統知道我們能打英文時，得再研究怎麼講才不會踩到這種洞。

### 推翻 [[執行模型的根本差異]] 的半句話

[[執行模型的根本差異]] 寫著「macOS 的輸入法是獨立行程，panic 不會弄壞宿主，系統還會自動
重啟它。所以 `guard.rs` 的規矩一樣，目的變成『保住組字狀態』」。

**這只對了一半。** 我們的 panic 確實傷不到宿主，但**傳錯參數給 IMK／AppKit
的 API 會**——而且是在**宿主行程裡**崩，`catch_unwind` 攔不到、`guard.rs`
一點用都沒有。上面兩次都是這樣。

> **macOS 版一樣會弄當宿主，只是路徑不同**：Windows 是我們的程式碼跑在
> 宿主行程裡（DLL），macOS 是我們送過去的**資料**讓宿主自己崩。
> 前者靠 `catch_unwind` 擋，**後者只能靠「送出去的東西一定要合格」**。

### 驗收：通過

**點面板不會搶走宿主焦點**——實測回報是「**可以點但沒反應，不會中斷選字**」，
正是要的行為。組字狀態原封不動，宿主的游標繼續閃。

這一格是整個 spike 存在的理由，而且**比 Windows 那邊乾淨**：

| | Windows | macOS |
|---|---|---|
| 宣告方式 | `WS_EX_NOACTIVATE` | `NSPanel` 的 `NonactivatingPanel` style mask |
| 滑鼠點擊 | **擋不住**，還要在 `wndproc` 回 `MA_NOACTIVATEANDEAT` 補 | **一起管住了**，不必另外處理 |

「沒反應」是預期的——Echo 沒有選字邏輯，面板只是塊會顯示文字的板子。

### 接 core 時要補的：點candidate 要能選字

面板現在的 content view 是一個 `NSTextField` label，不吃滑鼠事件。真的要
讓使用者點候選字，得換成自訂的 `NSView` 覆寫 `mouseDown:`，把座標換算成
第幾個候選。**`NonactivatingPanel` 已經證明點了不搶焦點，所以這條路是通的**
——只是還沒鋪。
