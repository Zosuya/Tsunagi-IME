---
原編號: "2.52.26"
date: 2026-09-08
status: done
一句話: "顏色字級版面全部來自 core::theme／render，點選用 ObjC selector 通知；順帶修掉 Cmd+C/V 被輸入法吃掉"
pitfall: true
相關:
  - "[[spike 5：快捷鍵攔截]]"
  - "[[繪製層在 Mac 上量了一遍：1452 行已可攜]]"
---

# 候選面板接上 core 的繪製決策，並且點得動

[[spike 2：IMK Echo 輸入法]] 記的兩個「接 core 前要還的債」，這是第二件（第一件見
[[組字緩衝從 thread_local 換成 ivar]]）。做完之後 spike 3 的產出從「證明面板行為對」變成
**真的候選視窗的雛形**。

### 為什麼非換掉 `NSTextField` 不可

spike 3 的面板 content view 是一個 `NSTextField` label，把三個候選字排成
一行字串塞進去。那**只夠證明面板行為**，做不了三件事：

1. 畫不出「編號＋候選字、選中的那個有反白底」——label 只有一種前景色
2. **接不到滑鼠**——label 不處理 `mouseDown:`
3. 尺寸與位置對不上——沒有每一格的範圍，就沒辦法判定點到第幾個

所以改成自訂 `NSView`（`TsunagiCandidateView`），自己畫、自己收滑鼠。

### 繪製的「決策」全部來自 core

顏色、字級、行高、內距、圓角、編號間距、反白圓角半徑，**沒有一個是寫死
在 macOS 這邊的**：

| 用到的 | 來源 |
|---|---|
| `window_bg`／`text`／`highlight_bg`／`highlight_text` | `ime_core::theme::Colors` |
| 字級、行高、內距、圓角、編號間距、最小寬度 | `ime_core::theme::Metrics`（都已套縮放百分比） |
| 反白條的圓角半徑 | `ime_core::render::highlight_radius_for_row` |

**跟 Windows 版與設定頁預覽是同一份**。這裡只負責把那些決策用 Cocoa 畫
出來——等同 Windows 那邊 `d2d.rs` 的角色。[[繪製層在 Mac 上量了一遍：1452 行已可攜]] 早就量出
「繪製的決策幾乎全部可攜、真正要重寫的只有 `d2d.rs`」，這一節是那個結論
第一次真的被用上。

`ime-core` 掛在 **macOS 的 target 區段**，Windows 目標下這個 crate
仍然零依賴（[[spike 2：IMK Echo 輸入法]] 量過的性質，別讓它退化）。

### 點選怎麼通知控制器：用 ObjC 訊息，不用 Rust callback

`mouseDown:` 算出點到第幾格之後要通知控制器。三條路：

| 做法 | 為什麼不用 |
|---|---|
| Rust closure 存進 ivars | 要 `Box<dyn Fn>`，生命週期與型別都麻煩 |
| `block2` | 為了一個回呼多一個相依 |
| **ObjC selector** ✅ | 訊息傳遞本來就是為這件事設計的 |

所以在 `EchoController` 上加一個 `tsunagiSelectCandidate:`，面板拿到目標
物件就送過去。**名字前綴 `tsunagi` 是刻意的**——那是我們自己加在
`IMKInputController` 子類別上的方法，不是 IMK 的協定；沒有前綴的話，哪天
Apple 加了同名方法就會靜悄悄地互相覆蓋。

連帶：**控制器要自己記住最後一個宿主**（`Composition::client`）。滑鼠事件
沒有 client 可以拿，而送出文字一定要對著某個宿主。每個控制器對應一個宿主
連線（[[組字緩衝從 thread_local 換成 ivar]]），所以記在 ivars 裡不會混到別人的。

### 兩個實作細節

- **`isFlipped` 回 `true`**：原點放左上角。`core::theme` 的版面常數（行高、
  內距）本來就是照「從上往下堆」寫的，翻過來才對得起來。
- **量與畫用同一組數字**：`layout()` 一邊算總寬度、一邊把每格的水平範圍
  存進 ivars 給 `mouseDown:` 用。分開算遲早會對不上——那跟
  §2.46（DirectWrite 的 `width` 不含尾端空白）是同一類的坑。
- **視窗要透明**（`setOpaque(false)` ＋ `clearColor` 背景）：圓角是我們自己
  畫的，視窗不透明的話圓角外會露出一塊方形底。

### 測出來的 bug：複製貼上被輸入法攔走

裝上去實測，**`Cmd+C`／`Cmd+V` 全被吃掉，而且組字區會多出一個 `c`／`v`**。

原因是 `charactersIgnoringModifiers` 對 `Cmd+C` 回的是 `"c"`——單一可見
字元，於是「只收印得出來的單一字元」那條把它當成打字收進緩衝。

修法是在 `match code` **之前**擋掉：帶 `Cmd`／`Ctrl`／`Option` 的一律
`return false` 讓回宿主。這樣 `Cmd+C/V/X/Z`、`Cmd+←`、`Cmd+Backspace`
行為一致。`Shift` 不算——`Shift+字母`是打大寫，那是真的輸入。

> **教訓**：spike 5（[[spike 5：快捷鍵攔截]]）明明白白量到 `Cmd+C/V/X/Z`
> **到得了輸入法**，但我沒把「**到得了**」跟「**該不該吃**」分開想。
> 到得了只代表我們有機會決定，而正確的決定是放行——這跟
> §2.14.2 在 Windows 上的結論是同一句話：「宿主自己的快捷鍵不會
> 擋住輸入法看到它們，**是我們主動回『不要』才交還給宿主的**」。
> 那句話我讀過，還是踩了。

`Option` 也放行：macOS 的 `Option+字母` 是輸入特殊字元的正規用法
（`Option+A` 打 å、`Option+R` 打 ®，[[spike 5：快捷鍵攔截]] 量到過），交給宿主處理才對。
**要不要改成由輸入法自己產生那些字元，接 core 時再決定**——註解裡標了。

### 還沒做的

- **主題還沒接設定檔**，用的是 `Theme::default()`。那要先決定 macOS 版的
  設定檔位置與熱重載怎麼做，是獨立的一件事。
- 候選字仍是 Echo 假造的。**面板本身不用再改**——接 core 之後把
  `fake_candidates` 換成真的候選來源即可。
