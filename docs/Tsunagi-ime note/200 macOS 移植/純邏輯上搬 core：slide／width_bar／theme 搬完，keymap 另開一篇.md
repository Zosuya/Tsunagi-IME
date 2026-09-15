---
原編號: "2.52.24"
date: 2026-09-08
status: done
一句話: "39c214f 搬了 slide／width_bar／theme，測試守恆、漏斗持平；keymap 卡在鍵碼編號，另開一篇"
相關:
  - "[[鍵位表上搬 core：按鍵改用中性的 Key，兩平台實打通過]]"
  - "[[繪製層在 Mac 上量了一遍：1452 行已可攜]]"
  - "[[移植前的程式碼盤點：平台層 11700 行重寫，約 1900 行可上搬 core]]"
---

# 純邏輯上搬 core：slide／width_bar／theme 搬完，keymap 另開一篇

> ✅ **2026-09-08 核准並做完三個檔**（`39c214f`）：
>
> - `slide.rs`、`width_bar.rs` → `core/src/render/`，`render.rs` 改成 `render/mod.rs`
>   ——下面「命名」那節的傾向照做了
> - `theme.rs` → `core/src/theme.rs`，`to_colorref` 留在平台層做成 `ToColorRef` 擴充 trait。
>   **沒跟 `theme_preset.rs` 合併**：後者管「有哪些主題、怎麼讀寫 toml」，前者是
>   「當下生效的那組值」，不同層
> - 測試 585+126 → 624+88，總數守恆；漏斗 1277 持平；`--target aarch64-apple-darwin` 過
>
> **`keymap.rs` 沒搬**：`DEFAULT_BINDINGS` 整張表寫的是 `VK_*`，下面說的
> 「照 `is_repeat_bits` 的天然切點切一刀」切不開——表本身就綁著 Windows 鍵碼。
> 2026-09-13 裁決改用 core 自己的按鍵名，另寫成
> [[鍵位表上搬 core：按鍵改用中性的 Key，兩平台實打通過]]。
>
> 以下是提案當時的原文。

[[macOS 移植的六項待決：四項裁決，不做語言鎖定一天就推翻]] 待決 1 講好「繪製層上搬 core **等 spike 2 通過再做**」——
spike 2 通過了，
所以這件事解鎖了。CLAUDE.md 規定拆檔重構要先提案，**這一節就是提案，
點頭之前不動手。**

> **這件事在 Windows 那台做**，不需要 Mac：要搬的檔都有完整測試，而
> [[繪製層在 Mac 上量了一遍：1452 行已可攜]] 已經在 Mac 上把它們原封不動跑過了。改完用
> `cargo check -p ime-core --all-targets --target aarch64-apple-darwin`
> 反向確認 Mac 側沒破。

### 為什麼要先做這個，而不是先在 Mac 上接 core

**Mac 版的候選視窗要用的正是這三個檔。** 如果 Mac 那邊先接了 core、
Windows 這邊接著把檔案搬走，Mac 的引用會集體失效——不是合併衝突，是
**接在一個即將移動的東西上**。所以順序是：

```
Windows：純邏輯上搬 core → push
   ↓
Mac：pull → 接 core（候選面板接 render／theme／slide／width_bar）
```

在那之前 Mac 那台可以做**不依賴這三個檔**的事：組字緩衝從 `thread_local`
換成 ivar、候選面板的 `mouseDown:` 選字——兩件都是純 IMK 的工作。

### 要搬的（數字是合併後實測）

| 檔 | 行數 | 測試 | `windows::` | 怎麼處理 |
|---|---|---|---|---|
| `slide.rs` | 345 | 12 | **0 處** | **整檔直接搬**，一個字都不用改 |
| `width_bar.rs` | 388 | 18 | **0 處** | **整檔直接搬**，一個字都不用改 |
| `theme.rs` | 338 | 9 | **1 處** | 搬，但**把 `to_colorref` 留在 Windows** |
| `keymap.rs` | 1020 | 34 | 2 處（實際更多） | **要切一刀**，見下 |

前兩個是白送的——[[繪製層在 Mac 上量了一遍：1452 行已可攜]] 已經在 macOS 上編過並跑過它們自己的測試。

`theme.rs` 唯一的 Windows 相依是第 27 行 `use windows::…::COLORREF`，
給 `to_colorref` 用。**那個轉換函式本來就該留在平台層**，搬的時候把它
留在 `platform/windows/`，core 只放顏色的資料與決策。

### `keymap.rs` 的切點（這是提案裡唯一需要判斷的地方）

CLAUDE.md 已經警告過：「`keymap` 裡混著 `is_repeat` 這種明確平台相依的
東西，搬的時候要在平台相依與純邏輯之間切一刀」。實際盤點：

| 留在 `platform/windows/` | 搬進 `core/` |
|---|---|
| `GetKeyState` ＋ `VK_*` 常數（第 22～24、181、186 行）——**讀鍵盤狀態是平台的事** | `Mode`／`Action`／`Combo` 的型別與資料 |
| `is_repeat(LPARAM)`（第 201 行）——薄包裝 | `is_repeat_bits(isize)`（第 196 行）——**已經是純函式了**，切點天然 |
| `CtrlTap`（第 220 行）——語言鎖定，[[macOS 移植的六項待決：四項裁決，不做語言鎖定一天就推翻]] 待決 3 裁決 macOS 不做，**不必搬** | `DEFAULT_BINDINGS` 與比對邏輯 |

`is_repeat_bits` 拆成純函式那件事**當初就做對了**（CLAUDE.md 記著這條
規則已經錯過三次），現在直接受益：切點不用重新設計，照現成的邊界切。

### 命名要先決定：core 裡已經有名字很近的檔

這是**動手前一定要拍板**的一項，不然事後改檔名更痛：

| 要搬進去的 | core 已有的 | 撞不撞 |
|---|---|---|
| `theme.rs` | `theme_preset.rs` | 不同檔，但**兩個 theme 容易混** |
| `width_bar.rs` | `width.rs` | 不同東西（`width.rs` 是全半形），**但名字會誤導** |
| `slide.rs` | — | 沒問題 |

**傾向**：`theme.rs` 進 core 之後跟 `theme_preset.rs` 合成一個 `theme/`
模組（`theme/mod.rs`＋`theme/preset.rs`），`width_bar.rs` 改名成
`render/width_bar.rs` 掛在 `render` 底下——它本來就是繪製決策的一部分，
跟 `render.rs` 是同一類。`slide.rs` 同理。這樣 core 的頂層不會越來越平。

**但這是我的傾向，不是決定**——要不要順手做這層目錄整理，還是先原地搬
完、之後再說，你決定。原地搬的 diff 乾淨得多，好 review。

### 不搬的

- **`CtrlTap`**：macOS 不做語言鎖定（待決 3 已裁決），搬過去也沒人用
- **`candidate_window.rs`**（2,399 行、23 處 `windows::`）：**另案**。
  它是候選視窗的**實作**不是決策，而且台語段選單剛動過它。
  [[繪製層在 Mac 上量了一遍：1452 行已可攜]] 已經量出「繪製的決策幾乎全部可攜、真正要重寫的只有 `d2d.rs`」，
  但把 2,399 行拆成「決策」與「Win32 繪製」是獨立的一次重構，
  **不要跟這次混在一起**

### 驗證

1. `cargo test`（Windows 上跑全部）——搬動不該改變任何行為，**測試數要一樣**
2. `cargo test -p ime-core`——搬進去的 39 個測試要跟著進來
3. `cargo check -p ime-core --all-targets --target aarch64-apple-darwin`
   ——確認 Mac 側編得過（只檢查不連結，不需要 Apple SDK）
4. `.\build-ime.ps1` ＋ `.\check-dll.ps1`——照 CLAUDE.md 的規矩，commit 完就重建
5. **跑一次漏斗**：搬動理論上不影響，但 `theme`／`slide`／`width_bar` 都在
   繪製路徑上，跑一次才知道有沒有手滑
