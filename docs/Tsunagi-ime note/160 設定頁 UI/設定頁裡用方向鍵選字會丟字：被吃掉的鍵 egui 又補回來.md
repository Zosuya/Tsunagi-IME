---
原編號: "2.77"
date: 2026-09-11
status: done
pitfall: true
一句話: egui-winit 把輸入法吃掉的鍵用實體鍵位補回來，Enter 關掉 IME 中止組字
---

# 設定頁裡用方向鍵選字會丟字：被吃掉的鍵 egui 又補回來

**做擴充包編輯器時翻出來的**（§2.75）。一開始記成「輸入法本體的
bug、影響所有宿主」，**錯了**——使用者一句「那為何記事本正常」把
方向指回設定頁自己。這一節記正確的診斷。

### 2.77.1 症狀

在設定頁（egui）的文字框裡用注音打字，**只要用方向鍵在候選視窗裡
選過字**，按 Enter 之後文字框留下組字區的原始按鍵（`su3`），輸入法
送出的是空字串。直接按 Enter 選第一個候選則正常。**記事本兩種都正常。**

### 2.77.2 事件序列

拿 `settings/src/bin/spike_keyfield.rs` 連打兩次對照（由下往上讀）：

```text
正常（直接 Enter）：
  Ime(Preedit("su3cl3"))
  Key(Enter) Ime(Disabled) Ime(Commit("你好"))

出問題（先用方向鍵選）：
  Ime(Preedit("su3"))
  Key(ArrowLeft) Key(ArrowDown) Key(ArrowDown)
  Key(Enter) Ime(Preedit("su3"))          ← 輸入法還在組字
  Ime(Disabled) Ime(Commit("")) ...       ← 下一幀，組字被中止
```

關鍵在 **Enter 那一幀後面跟著 `Preedit("su3")`**：輸入法把 Enter 當
`ConfirmCand`（選中候選，`keymap.rs`：「Enter 在選字選單裡是選中反白
的候選字，不是送出」），**留在組字狀態**。真正送出空字串的是下一幀
——那不是輸入法做的。

### 2.77.3 病因：四步連鎖

1. 輸入法吃掉 Enter（`VK_PROCESSKEY`），選中候選、繼續組字
2. **egui-winit 對被吃掉的鍵會退回實體鍵位**
   （`egui-winit/src/lib.rs`：`logical_key.or(physical_key)`），
   文字框仍然收到一個 Enter——這也是為什麼 log 裡看得到
   `Key(ArrowLeft)`：方向鍵其實有被輸入法吃掉，只是 egui 也拿到一份
3. egui 的單行文字框收到 Enter → 結束編輯、失去焦點 → 通知 winit
   「關掉 IME」
4. winit 走的是 **IMM32**（`WM_IME_COMPOSITION` 那套），相容層一收到
   「關掉」就把還沒送出的組字**整個中止**→ `Commit("")`

**記事本沒事**：它是真正的 TSF 宿主，被吃掉的鍵根本不會到它手上。
**直接 Enter 沒事**：輸入法在同一下就先把字送出了（`Action::Commit`
→ `end_composition`），之後 egui 再關 IME 已經無傷。**只有「輸入法
吃了鍵但還在組字」會炸**——選字、段選單、Esc 退出選字都是。

順帶一提 egui 自己知道這件事的一半：組字中它會濾掉方向鍵與 Backspace
（`remove_ime_incompatible_events`），**但沒濾 Enter／Esc／Tab**。

### 2.77.4 修法：在設定頁每一幀開頭攔掉那三個鍵

`settings/src/main.rs` 的 `App::shield_ime_keys`：追蹤「輸入法在不在
組字」（`Preedit` 非空進、`Commit`／`Disabled` 出），組字中或這一幀
有任何 IME 事件時，把 `Enter`／`Escape`／`Tab` 從事件裡 `retain` 掉。
文字框看不到就不會誤動作。

- **放在整個 App 而不是某個欄位**：名稱、說明那些一般文字框一樣會踩到
- **「這一幀有 IME 事件」也算**：送出候選的那一幀 Enter 跟 `Commit`
  一起來，那個 Enter 也是輸入法吃的
- **不動輸入法**：這是宿主的問題，輸入法在記事本上本來就是對的

### 2.77.5 這一輪學到的

- **「別的宿主正不正常」是最便宜的分岔測試**。一句話就把「輸入法 bug」
  跟「宿主 bug」分開了，比讀半小時程式碼有用
- **spike 視窗（乾淨的 egui 文字框）是對照組**——編輯器裡疊了太多
  自己的邏輯，在那裡看事件會被自己的東西干擾
- **egui-winit 的實體鍵位退路是雙面刃**：它讓輸入法關著時按鍵能用，
  也讓輸入法開著時被吃掉的鍵漏進來。凡是在 egui 裡放注音打字的欄位，
  都要記得這條
