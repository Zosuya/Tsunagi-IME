---
原編號: "2.52.45"
date: 2026-09-09
status: done
一句話: 掃描器誤報不可用；解構守設定（編譯期自動）、比對 Session 96 支 API 守行為（要人判讀）
相關:
  - "[[設定頁依平台隱藏無效的欄位]]"
  - "[[設定的行為稽核：領域包與 enter_in_select 在 macOS 都沒接]]"
  - "[[語言鎖定接上 macOS，鍵位改成空白鍵那一族]]"
---

# 怎麼系統性抓出平台層漏接：解構與 API 反查

「設定頁改得動，平台層不理它」這種洞**不會有任何症狀**——編譯過、控制項
畫得出來、使用者也改得動，就是沒反應。試了三種找法：**用測試掃描器 grep
欄位名**（不可靠，誤報三個）、**用解構把設定漏接變成編譯錯誤**（有效，但
只管 `Behavior`）、**比對 `core` 的 96 支公開 API 兩平台各用了幾次**（掃出
19 支、真的有洞的三支）。後兩者互補：解構守**設定**、是編譯期的自動的；
API 反查守**行為**、要人來判讀但一次掃完只花幾分鐘。

## 試過的路

### 一、用解構把設定漏接變成編譯錯誤（原 2.52.45）

macOS 移植的過程中，同一類錯連續踩了四次（領域包、`enter_in_select`、
`commit_on_last`，加上設定頁的預覽字型，見
[[設定頁依平台隱藏無效的欄位]]～
[[設定的行為稽核：領域包與 enter_in_select 在 macOS 都沒接]]）：

> **設定頁改得動，平台層不理它。**

加一個設定要碰 4～6 個地方（core 的欄位、兩個平台的 apply、設定頁
的控制項、`platform.rs` 的平台旗標、預覽的正規化），而**只有第一個是編譯器
管的**。

#### 先試過的路：用測試去掃

寫了一支掃描器，把 `Behavior`／`Colors`／`Metrics` 的欄位名逐個 grep 兩個
平台的原始碼。**不可靠**：`scale_percent` 走 `Metrics::scale()`、`strength`
走 `overlay_alpha()`、`text_outline` 被複製進 `Theme`——三個都是誤報。
欄位可以透過方法或轉換被讀到，字串比對數不準。

#### 有效的路：解構，而且不准寫 `..`

兩個平台的 `apply` 改成解構整個 `Behavior`：

```rust
let ime_core::config::Behavior {
    enter_in_select, commit_on_last, width, engines,
    backspace_whole_cell, packs, packs_dir, lock_punct, ctrl_punct,
} = &l.config.behavior;   // ← 沒有 `..`
```

加欄位就**兩邊同時編譯失敗**，訊息直接點名：

```
error[E0027]: pattern does not mention field `假的欄位`
   --> platform/macos/src/settings.rs:161
   --> platform/windows/src/text_service/background.rs:132
```

（真的加一個假欄位驗過，兩邊都擋下來了。）

用不到的欄位就 `let _ =` 掉並寫清楚原因——**把靜默的洞換成一句明確的
決定**。目前 macOS 唯一 `let _` 掉的是 `ctrl_punct`：它是「鎖定注音時按
Ctrl+標點明講我要標點」的逃生口，而 macOS 不做語言鎖定（使用者裁定），
沒有需要逃生的情境；何況那邊 `Ctrl` 是整批讓回宿主的（[[spike 5：快捷鍵攔截]]），要接
得先在 `on_key` 開洞。

#### 涵蓋範圍與缺口

這道防護只管 `Behavior`（功能性的設定），**`Colors`／`Metrics` 沒有**：
新增一個顏色不會讓任何平台編譯失敗，因為「有沒有把它畫出來」是繪製層的事，
型別上看不出來。那一類目前只能靠設定頁的 `platform.rs` 與預覽的正規化擋
（[[設定頁依平台隱藏無效的欄位]]），以及發布前跑相容性測試清單。

## 另一半：用 core 的 API 反查平台層漏接了什麼（原 2.52.48）

前面幾節都是「使用者回報 → 查」。這次反過來做一次系統性的稽核：把
`core` 的 `Session` 公開 API（96 支）逐一比對兩個平台各用了幾次，**Windows
用了而 macOS 沒用的**就是候選缺口。掃出 19 支，逐一判讀之後真的有問題的
三支。

### 三個真的洞

**① 台語模式的四顆鍵**（`tw_next`／`tw_prev`／`tw_widen`／`tw_narrow`）

鎖定注音＋裝了台語包時（`tw_mode()`），整串按鍵就是一段，換段沒有意義；
反白單位改成「詞」，`←→` 是跳詞、`Shift+←→` 是調那個詞的寬度。Windows 有
四處分支，**macOS 一處都沒有**——`←→` 會去換段、`Shift+←→` 會去推段邊界。

這條在 [[語言鎖定接上 macOS，鍵位改成空白鍵那一族]] 接上語言鎖定之前**根本進不去**（`tw_mode()` 永遠是 false），
所以不是退化，是新通的路上的洞。

**② 段選單的 Enter 沒看 `enter_in_select`**

macOS 用的是 `seg_confirm()`＝永遠往下一段。但那個設定**選字與段選單共用**
（core 的註解寫著「使用者裁定」）。只接一半的話，使用者會覺得「同一個設定
在 TAB 選單裡沒作用」——跟 [[設定的行為稽核：領域包與 enter_in_select 在 macOS 都沒接]] 抓到的三個是同一類。

**③ 組字區的框：`marked_index` 不是 `select_index`**

**選完字離開選字模式之後，那一格的框要留著**，讓使用者看到自己剛改了哪個。
`select_index()` 一離開就變 `None`，反白整個消失。候選面板那邊仍然看
`select_index`（離開了就不該再列候選），**框與候選是兩件事**。

### 判讀掉的十六支（記下來免得下次重掃又重想一次）

| API | 為什麼不是洞 |
|---|---|
| `cutting_*`／`next_cutting`／`prev_cutting`／`expand_cutting`／`collapse_cutting`／`set_cutting_index` | 舊的整句選單，Windows 也沒有入口了，段選單穩定後要一起刪 |
| `arrow_left`／`arrow_right` | 只有「沒在選字時按方向鍵」那條分支跟 `select_left/right` 不同，而 Windows 只在選字模式綁它們——實際等價 |
| `cand_scroll`／`set_cand_col_first` | 候選捲軸，macOS 還沒做（已知缺口） |
| `composition_text` | macOS 刻意用 `text() + pending_symbols()`——自動模式下 `composition_text` 回的是原始按鍵（[[接上引擎：詞庫定位、Session 取代自己記的緩衝]]） |
| `engines` | macOS 從 `settings::engines()` 拿，同一份設定 |
| `push_punct` | `ctrl_punct` 的路，macOS 不做語言鎖定以外的 Ctrl 組合（而且 Ctrl 到不了我們，[[語言鎖定接上 macOS，鍵位改成空白鍵那一族]]） |
| `seg_reset` | macOS 的 Esc 是「關掉但保留」（使用者裁定，[[段選單與學習存檔：三個狀態機接完]]） |
| `set_lock` | macOS 走 `cycle_lock()`，它內部就呼叫這支 |

### 這個方法值得留著

**「兩邊各用了哪些 API」是可以機械比對的**，而它抓到的正是最難自己發現的
那類洞：功能會動、編譯過、沒有錯誤訊息，只是某條路的行為跟另一個平台不同。

跟前半的解構防護互補：那個守的是**設定**（欄位加了沒接會編譯失敗），
這個守的是**行為**（API 用了沒接不會有任何症狀）。前者是編譯期的、自動的；
後者要人來判讀，但一次掃完 96 支只花幾分鐘。

### 順帶：台語那條路現在量得到了

裝上台語包之後 `bench_segmenu` 不再跳過那三列，數字跟 Windows 幾乎一樣：

| 操作 | Windows | macOS |
|---|---|---|
| 台語開選單（含斷詞） | 0.03ms | 0.03ms |
| 台語跳詞（→） | 0.04ms | 0.04ms |
| 台語定案 | 0.21ms | 0.24ms |

**比一般段選單快兩個數量級**（定案 0.24ms vs 9.31ms）——鎖定注音時
`seg_cands` 不必窮舉長度×語言，只查一次雜湊。
