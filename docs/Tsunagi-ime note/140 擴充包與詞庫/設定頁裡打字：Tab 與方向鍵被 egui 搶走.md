---
date: 2026-09-14
status: done
一句話: 刪事件擋不掉焦點移動——egui 的焦點在 begin_pass 就決定了，早於 update()，正解是 EventFilter
pitfall: true
相關:
  - "[[擴充包編輯器的實作與收尾]]"
  - "[[擴充包編輯器的符號支援，與追了三輪的捲軸重疊]]"
  - "[[切法選單改成「段選單」：反白一段、選它是什麼]]"
---
# 設定頁裡打字：Tab 與方向鍵被 egui 搶走

> **狀態**：三輪修完，使用者實測通過（2026-09-14）。

**設定頁是拿來打字試效果的地方**，擴充包編輯器尤其如此——一邊輸入
一邊用 Tab 選切法是主要用途。但 egui 把 Tab 與方向鍵當成移動焦點的鍵，
跟輸入法搶。

### 2.82.1 兩件不同的事，一開始混在一起

| | 誰搶走的 | 症狀 | 歸誰管 |
|---|---|---|---|
| **A** | egui-winit 把被吃掉的鍵**退回實體鍵位** | 組字中按 Enter，文字框收到 Enter 就結束編輯，IMM32 一關 IME 就把沒送出的字整個丟掉 | `main::shield_ime_keys`（刪事件） |
| **B** | egui 的**焦點系統** | Tab 跳欄、方向鍵移焦點，文字框失焦、組字中止 | `pack_editor::lock_ime_keys_while_composing`（`EventFilter`） |

**A 早就修好了**（見 `shield_ime_keys` 的長註解）。這次是 B，而**前兩輪
都因為把 B 當成 A 而失敗**。

### 2.82.2 第一輪：判準太寬，Tab 被平白吃掉

`shield_ime_keys` 原本的觸發條件是「這一幀有任何 IME 事件」：

```rust
if !(was_composing || saw_ime) { return; }
```

而 `saw_ime` 把 `ImeEvent::Enabled`／`Disabled` 也算進去——**那兩個是
輸入法被開啟／關閉的通知，不是組字**。egui 在焦點進入文字框時就會送
`Enabled`，於是沒在組字時按 Tab 也被吞掉。

而且它**會自我維持**：Tab 被吃掉 → 焦點留在原地 → 下次按又是同樣情況，
所以症狀是「永遠跳不出去」而不是偶爾失靈。

判準收窄成只看 `Preedit` 的內容（那才是「組字區有東西」），外加送出
候選那一幀的 `Commit`——Enter 跟 Commit 同一幀來，那個 Enter 仍然是
輸入法吃掉的。

### 2.82.3 第二輪：刪事件擋不掉焦點移動

使用者裁決「Tab 永遠是輸入法的」，於是改成一律從事件裡刪掉 Tab。
**實測回報：還是會跳欄。**

根因是**時序**：

```
begin_pass          ← egui 在這裡把 Tab 轉成 FocusDirection::Next
   ↓
update()            ← shield_ime_keys 在這裡刪事件，焦點早就移走了
```

`memory/mod.rs` 的 `begin_pass` 就是焦點處理的位置，而它跑在
`eframe::App::update` **之前**。所以 `update()` 裡刪事件對焦點毫無作用
——只能擋到「事件被 widget 讀取」那一層，擋不到焦點系統。

> **教訓**：**擋一個鍵要先問「誰在什麼時候吃掉它」**。同一個鍵可能有
> 好幾個消費者，分別在不同階段。刪事件只擋得到比你晚的那些。

### 2.82.4 第三輪：`EventFilter` 才是正式管道

egui 給 widget 宣告「這個鍵我自己要」的方式是 `EventFilter`，而
`begin_pass` 的焦點處理**會先問它**（`event_filter.matches(event)`）。

```rust
m.set_focus_lock_filter(id, egui::EventFilter {
    tab: composing,
    horizontal_arrows: composing,
    vertical_arrows: composing,
    escape: false,   // 那是 shield_ime_keys 管的
});
```

**值綁 `composing`**，所以組字中鎖住、打完就放行——這正是使用者要的
規則（2026-09-14 裁決）：**跳欄要留著，只是組字中不能跳**。沒在組字時
Tab 與方向鍵就該是一般的介面操作，那是所有表單都有的行為。

**`escape` 維持 `false`**：Esc 歸 `shield_ime_keys` 管（它在組字中把
Esc 從事件裡拿掉）。兩邊都管的話，沒在組字時 Esc 反而關不掉文字框的焦點。

### 2.82.5 第四輪：只鎖 Tab 會從方向鍵漏掉

實測回報：**按 Tab 開了段選單之後按方向鍵會直接跳離組字**。

同一個病的另外四個出口——段選單的**左右選段、上下翻候選**，四個方向
都是輸入法的。`EventFilter` 的 `horizontal_arrows` 與 `vertical_arrows`
補上就好，但要**四個一起**：少鎖一邊就從那一邊漏掉。

> 這條可以一開始就想到：`binding.rs` 裡段選單綁了哪些鍵是查得到的，
> 不必等使用者一個一個踩出來。**擋鍵要照著鍵位表列，不要憑印象**。

### 2.82.6 順手修掉的一個洞（還沒被踩到）

`state.composing` 原本只在 `draft_row`（「新增一條」那一列）裡更新，
但鎖鍵三個欄位都要用，而**列的欄位畫在 `draft_row` 之前**——編輯既有
列時會拿到上一幀的舊值，鎖不住。

改成在 `page()` 開頭就掃一次，判準跟 `main::scan_ime` 一致。

**這是「同一個狀態兩個地方讀，但只有一個地方寫」的典型**，跟 CLAUDE.md
那條「一個設定套用到兩層實作時兩邊都要各自實作」是同一族的問題。

### 2.82.7 驗收

| 項目 | 結果 |
|---|---|
| `cargo test -p ime-settings` | 39 全過 |
| clippy | 乾淨 |
| 實測 | 使用者確認（Tab 開段選單、方向鍵選段、非組字時正常跳欄） |

**沒有碰到輸入法**：只改 `settings/`，建置時只有 `ime-settings` 重編
——`core` 與 DLL 都不依賴設定頁，依賴是單向的。使用者在這一輪特別提醒
過這件事，值得記下來當習慣：**改設定頁時附上「只有它重編」當證據**，
比說「應該不會影響」有用。

### 2.82.8 教訓

- **擋一個鍵之前先問「誰在什麼時候吃掉它」**。刪事件只擋得到比你晚的
  消費者；egui 的焦點系統跑在 `update()` 之前，得用 `EventFilter`
- **「IME 被開啟」不等於「正在組字」**。`Enabled`／`Disabled` 是開關
  通知，拿它當組字判準會讓防護在不該生效的時候生效
- **會自我維持的 bug 症狀是「永遠」而不是「偶爾」**。焦點被吃掉 → 下次
  按還是同樣情況，這種迴圈值得一眼認出來
- **擋鍵要照鍵位表列全**（`binding.rs` 查得到），不要憑印象挑幾個
