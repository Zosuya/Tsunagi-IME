---
原編號: "2.52.35"
date: 2026-09-09
status: rejected
一句話: "跟隨深淺／系統強調色／毛玻璃三種都做得到，代價依序遞增；使用者裁定先維持 config 配色"
---

# macOS 系統外觀：三種做法量過了，先不接

問題是：面板的顏色**全部來自 `config.toml`**（`Theme::from_config`），跟
系統外觀零關聯——切成深色模式，候選面板還是照設定檔那組顏色畫。而組字區
的反白用的是 `selectedTextBackgroundColor`（系統動態色，會跟著深淺變），
**兩者在深色模式下會不搭**。

### 三種做法與各自的代價

跟 [[從輸入法裡開設定頁：↑↑↓↓ 手勢與輸入法選單]]
問過的「切換語言那個 HUD」不同，**外觀這三樣的 API 都是公開的**，繫結
也都有（`NSApp.effectiveAppearance`／`NSColor.controlAccentColor`／
`NSVisualEffectView`）。差別在客製化被吃掉多少：

| 做法 | 客製化怎麼了 |
|---|---|
| 跟隨系統深／淺 | **不受影響**。設計成「淺色用哪套、深色用哪套」，兩套都能自訂，反而變成兩套 |
| 反白用系統強調色 | 只有 `highlight_bg` 被接管，可獨立開關 |
| 毛玻璃底 | `window_bg` 直接失效、背景圖也蓋不上去，而且**對比不再是我們能保證的**（底下是使用者的文件） |

第一項不必是 macOS 專屬——Windows 也有深色模式的訊號，旗標放 config
兩邊都能用，設定頁預覽也還準。

### 展示視窗：`--appearance-demo`

用講的講不清楚，所以做了 `appearance_demo.rs`：淺色／深色各一排、每排四張
卡，卡片底下鋪一段假文件（毛玻璃要有東西可以透才看得出可讀性風險）。
**卡片一律用 `candidate_panel` 的 `CandidateView` 畫**——展示如果用另一份
繪製程式碼，看到的就不是真的長相。深淺用 `NSAppearance` 強制，不必去改
系統設定。

一個要記住的差別：展示裡的毛玻璃用 `WithinWindow`（糊掉同一個視窗裡的
假文件），**正式用會是 `BehindWindow`**（糊掉宿主的文件）。視覺同一類，
但真實情況下底下是什麼完全不可控。

### 結論：使用者裁定先不接

先維持現況（config 配色），使用者要自己調配色看看。展示程式留著——
`appearance_demo.rs` 與 `--appearance-demo` 這個旗標**是 spike 不是產品
功能**，真的決定不做的話連同 `CandidateView::paint_bg` 那個旗標一起刪。
