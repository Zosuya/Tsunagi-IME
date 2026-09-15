---
原編號: "2.52.32"
date: 2026-09-09
status: superseded
superseded_by: "[[提示列與全半形提示面板]]"
一句話: "盤出 Shift+空白、日文詞界伸縮、CuttingMenu 三處；當天就補完了，提示列那節指出前兩個其實是同一個洞"
相關:
  - "[[提示列與全半形提示面板]]"
---

# 鍵位對照稽核：macOS 缺的兩處當天補完

把 macOS 的 `on_key` 跟 Windows 的 `keymap::DEFAULT_BINDINGS` 逐條對過。

> **兩處都補完了**（2026-09-09 當天）：全半形連同提示列見
> [[提示列與全半形提示面板]]，
> 日文詞界伸縮補在 `on_key_selecting`。這一節保留當時的盤點原貌。

| Windows 的綁定 | macOS 現況 |
|---|---|
| `Shift+空白` → `ToggleWidth`（Typing／Idle／Selecting **三個模式**） | **完全沒有**。`shift` 只傳進 `on_key_segmenu` → 已補 |
| `Selecting` 的 `Shift+←→` → `WidenWord`／`NarrowWord`（日文詞界伸縮） | **沒有**。`on_key_selecting` 連 `shift` 參數都沒收 → 已補 |
| `Mode::CuttingMenu` 整組（舊的整句選單） | 不用做——那組在 Windows 也**沒有入口**了，段選單實測沒問題之後要一起刪 |

其餘（Backspace／Esc／Enter／TAB／方向鍵／數字／空白展開收合／段選單
那一整組）都對得上。

### 全半形切換卡在「切了看不出來」

`Session::toggle_width()` 本身照用就好，難的是**回饋**。Windows 切完會在
候選視窗上方滑出一條提示列（`core::render::width_bar`：滑動 110ms、停留
500ms、淡出），而且沒組字時也看得到。

macOS 這邊**整個提示列都還不存在**——面板只在有候選字時才出現，而
`width_bar` 需要每幀重畫的計時器。同一個洞還吃掉另外兩樣東西：

- `↑↑↓↓ ⚙ 開啟設定` 的指令提示（[[從輸入法裡開設定頁：↑↑↓↓ 手勢與輸入法選單]] 暫時用選單列頂替）
- `鎖定：注音`／`日文已停用` 這類持久狀態的提示（`ui.rs` 的
  `lock_hint`／`off_hint`）

所以這三件事其實是**同一件**：macOS 需要一條提示列。做法上 macOS 反而
比 Windows 單純——`NSView` 有 `NSTimer` 與原生的動畫，不必像 TSF 那樣
借宿主行程的訊息迴圈（[[執行模型的根本差異]] 講的執行模型差異，這裡是站在我們這邊的
那一面）。
