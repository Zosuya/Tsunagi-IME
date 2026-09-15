---
date: 2026-09-13
status: done
一句話: 鍵位表寫死 VK_* 所以沒搬；改用 core 自己的 Key 名稱，兩平台各翻譯一次，Mac 手寫的 match 盤出六處跟 Windows 不一樣
相關:
  - "[[純邏輯上搬 core：slide／width_bar／theme 搬完，keymap 另開一篇]]"
  - "[[鍵位對照稽核：macOS 缺的兩處當天補完]]"
  - "[[選字按鍵的兩個層次]]"
  - "[[快捷鍵攔截：哪些組合根本到不了輸入法]]"
---

# 鍵位表上搬 core：按鍵改用中性的 Key，兩平台實打通過

[[純邏輯上搬 core：slide／width_bar／theme 搬完，keymap 另開一篇]] 做了四個檔裡的三個（`39c214f`），`keymap.rs`
留下來了，原因寫在那個 commit：

> `DEFAULT_BINDINGS` 整張表都寫 `VK_*`，core 要先決定用哪套鍵碼編號，待裁決。

**2026-09-13 裁決：core 自己定一套按鍵名**（`Key::Space`、`Key::Left`），
不沿用 Windows 的 `VK_*` 編號，也不維持兩平台各一份。理由：表讀起來不必
懂任何平台、平台差異集中在各自一小段翻譯、加 Linux 不必動表。

CLAUDE.md 規定拆檔重構要先提案，這篇就是提案。

> ✅ **2026-09-13 全部點頭**：整體做法、檔名 `binding.rs`、`Shift+數字` 不再挑候選、
> 段選單 Esc 統一成保留。
>
> ✅ **第一步當天做完**：`cargo test -p ime-core` 719 個全過（binding 33 個）、
> Windows 平台層 `cargo check --all-targets` 與 clippy 乾淨。**Windows 那台 2026-09-14 補驗完**，
> 見下面的收尾。
>
> 跟下面寫的有一處不同：Windows 保留了 `keymap::lookup(mode, vk)` 這個簽名，
> 裡面讀 `GetKeyState`、翻 `Key`、再轉呼叫 core——**`text_service` 一行都不用改**，
> 比「呼叫點改傳 `KeyEvent`」的 diff 小得多。測試：Windows 那邊從 34 個剩 7 個
> （自動重複、US 配置表、以及新加的「鍵碼翻得對」四個），core 多 33 個，
> 其中「按著主修飾鍵的字母回 None」與「Shift+數字是符號」是**搬進來之後才測得到的**。
>
> 🔨 **第二步 2026-09-14 做完程式碼**：`on_key_segmenu`／`on_key_selecting` 與 `on_key` 裡的
> 手寫 `match keyCode` 全部拿掉，換成 `to_key`（鍵碼翻譯）→ `binding::lookup` → `dispatch`
> （依 `Action` 分派）。差異表 2～5 跟著補上：倒退鍵照 `delete_marked_seg`／`delete_marked_cell`
> 刪整段／整格（`settings.rs` 補兩支現查）、數字鍵盤翻成 `Numpad`、印不出來的鍵組字中吞掉。
> `cargo test -p ime-tip-macos` 11 個過（新加翻譯與三態共 4 個）、clippy 乾淨、`build-app.sh` 裝好。
>
> ✅ **兩平台都實打通過**（2026-09-14，使用者回報）。Windows 那台的收尾數字：
> `cargo test` 整個 workspace **722 個全過**，測試總數守恆如預期——core `binding::`
> **正好 33 個**、Windows `keymap` **縮到 7 個**（自動重複、US 配置表、四個翻譯測試）；
> `clippy --all-targets` 在 `binding.rs`／`keymap.rs` **零警告**；`build-ime.ps1` 重建後
> 記事本／瀏覽器／Word 各打一輪過關。**這篇結案。**
>
> 改完才看出來的三處連帶變化（都是跟 Windows 對齊）：
>
> - **Cmd 不再整批放行**，改成跟按鍵一起查表（主修飾鍵）。`Cmd+C` 查不到照樣讓回宿主，
>   但「`Cmd+Shift+空白` 要在放行之前判」那個順序陷阱消失了
> - **`Shift+空白`／`Cmd+Shift+空白` 在段選單與展開的選字裡不再有作用**——表只綁在
>   打字、沒組字、未展開選字三個模式（Windows 一直是這樣）
> - **段選單開著時打字會關掉選單**。以前 macOS 會在選單開著的狀態下把字推進去

## 現況：兩個平台各寫一份

| | Windows | macOS |
|---|---|---|
| 在哪 | `platform/windows/src/keymap.rs`（1095 行，34 個測試） | `platform/macos/src/echo_ime.rs` 的 `on_key`／`on_key_segmenu`／`on_key_selecting` |
| 形式 | 一張 `(Mode, Combo, Action)` 表 ＋ `lookup` | 手寫 `match code { 123 => …, 36 \| 76 => … }` |
| 鍵碼 | `VK_LEFT`（37）、`VK_SPACE`（32） | `keyCode` 裸數字（123、49） |
| 測試 | 有，但**按不出 Shift**——`lookup` 自己去讀 `GetKeyState` | 沒有 |

Mac 那份的註解寫「鍵位照 Windows 的 `DEFAULT_BINDINGS` 對過」，也就是
**人工抄一份**。[[鍵位對照稽核：macOS 缺的兩處當天補完]] 就是抄漏的實證。

## 設計

### core 放什麼

```rust
/// 一顆鍵的中性名稱。平台層把自己的鍵碼翻成這個再來查表。
pub enum Key {
    /// 印得出來的字元，**平台已經套過 Shift 與鍵盤配置**（Shift+1 就是 '!'）
    Char(char),
    /// 數字鍵盤。跟 Char 分開：主鍵盤的 5 是ㄓ，數字鍵盤的 5 就是 5
    Numpad(char),
    Space, Enter, Tab, Esc, Backspace,
    Left, Right, Up, Down,
    /// 其他（Home、End、F1…）——組字中一律吃掉，靠的就是這個
    Other,
}

pub struct KeyEvent {
    pub key: Key,
    pub shift: bool,
    /// **主修飾鍵**：Windows 是 Ctrl，macOS 是 Cmd
    pub primary: bool,
}

pub fn lookup(mode: Mode, ev: KeyEvent) -> Option<Action>
```

連同 `Mode`、`Action`、`Combo`、`DEFAULT_BINDINGS` 與 `lookup` 的判斷順序
（數字挑候選 → 數字鍵盤 → 字元輸入 → 查表 → Shift 退路 → 組字中吞掉）
整段搬過去。**`Combo.vk` 換成 `Combo.key`，`ctrl` 改名 `primary`**，其餘照舊。

`Key::Enter` 同時涵蓋主鍵盤的 Return 與數字鍵盤的 Enter——Windows 本來就
不分（都是 `VK_RETURN`），Mac 那邊也是 `36 | 76` 當同一顆。

### 平台層留什麼

| Windows | macOS |
|---|---|
| `VK_*` → `Key` 的翻譯（`typed_char` 的 US 配置表、`numpad_char` 搬進這裡） | `keyCode` → `Key` 的翻譯（字元走現有的 `printable_char`） |
| `GetKeyState` 讀 Shift／Ctrl，填進 `KeyEvent` | `modifierFlags` 讀 Shift／Cmd，填進 `KeyEvent` |
| `is_repeat`（`lparam` 第 30 位元） | 不需要（macOS 修飾鍵不自動重複，§2.52.3） |
| `defer_decide` 的 Alt／Win 整組讓給系統 | Ctrl／Option 整組讓回宿主 |
| `ctrl_punct`（鎖定注音時的 `Ctrl+標點`） | 沒有這條 |

**判斷一條東西該放哪的準則**：跟「這個平台的鍵盤怎麼回報」有關的留在平台，
跟「這顆鍵在這個模式要做什麼」有關的進 core。

### 附帶的好處：測試終於按得出 Shift

現在的 `lookup` 自己呼叫 `shift_down()` 讀實體鍵盤，所以測試裡好幾處註解
在說「單元測試按不出 Shift，只好直接查表」。改成 `KeyEvent` 帶進來之後，
`Shift+←`、`Ctrl+Shift+空白` 那些組合可以走完整的 `lookup` 測。

### 命名：core 已經有兩個 `keymap`

`core/src/bopomofo/keymap.rs`（注音鍵位）與 `core/src/romaji/keymap.rs`
（羅馬字母音子音表）已經在用這個名字，**再來一個頂層 `keymap` 會很混**。

**傾向叫 `core/src/binding.rs`**（按鍵綁定）——它做的正是「鍵 → 動作」的綁定，
跟那兩個「鍵 → 音」的表是不同東西。

## 分兩步做

### 第一步：搬進 core，Windows 改用它（行為只變一處）

- core 新增 `binding.rs`，34 個測試跟著搬（`is_repeat` 那個留在 Windows）
- Windows 的 `keymap.rs` 縮成翻譯層（實作時改成保留 `lookup(mode, vk)` 簽名，見開頭）
- **這一步在 Mac 上就能做**：實測 `cargo check -p ime-tip-windows --target x86_64-pc-windows-msvc`
  在 Mac 上 7 秒過；但 Windows 平台層的測試（`defer_decide` 那組）在 Mac 上
  連結不了，**要到 Windows 那台跑**，連同 `build-ime.ps1` 與三宿主實打

**唯一的行為變化**（要確認）：選字與段選單裡按 `Shift+數字`，現在 Windows
會挑候選（它看的是 VK，不管 Shift），改成 `Key::Char('!')` 之後會變成輸入 `!`。
macOS 本來就是後者。**傾向接受**——`Shift+1` 本來就是驚嘆號。

### 第二步：macOS 改用 core 的 `lookup`

手寫的三支 `match` 換成「翻譯成 `Key` → `lookup` → 依 `Action` 分派」。
**這一步會把兩邊不一致的地方全部逼出來**，因為表只有一份。盤出六處：

| # | 情境 | Windows | macOS | 性質 |
|---|---|---|---|---|
| 1 | 段選單按 Esc | ~~`SegReset`：丟掉已定案的段~~ → 保留 | 關選單、保留已定案的段 | ✅ **2026-09-13 已統一**，見下 |
| 2 | 段選單按倒退鍵 | `DeleteSeg`（照三態設定刪整段） | 退一鍵 | Mac 沒接（`settings.rs` 自己寫著「macOS 還沒接」） |
| 3 | 選字中按倒退鍵 | `DeleteCell`（照三態設定刪整格） | 退一鍵 | 同上 |
| 4 | 數字鍵盤 | 打出數字；選字時挑候選 | 當一般字元，**數字鍵盤的 5 會變成ㄓ** | Mac 漏了 |
| 5 | 組字中按沒綁的鍵（F1 等） | 一律吞掉 | 只吞方向鍵與 Home／End／PgUp／PgDn，其餘放行 | Mac 漏了 |
| 6 | 選字中 `Shift+數字` | 挑候選 | 輸入符號 | 第一步就會統一成 Mac 這樣 |

4、5 是 Mac 漏抄，接上 core 就自動修好。2、3 是 Mac 缺動作的實作，
表統一之後 `Action` 會送過來，要補分派（引擎那半已經有了）。

**1 已裁決並先做了**（2026-09-13）：統一成「保留」，Windows 那邊改。
`Action::SegReset` 連同分派一起刪掉，`Session::seg_reset()` 留在 core
但兩平台都沒有入口。不必等搬表——這是一行綁定的事。**Windows 2026-09-14 實測過**。

共用一張表只能有一個答案；表不支援「某平台覆寫」，那等於開後門。

## 不在這次

- **舊的整句選單**（`Mode::CuttingMenu` 與相關 `Action`）：沒有入口但還在。
  照原樣搬，刪不刪是另一件事——混在一起 diff 就看不出是搬還是改
- **使用者自訂鍵位**：已決定不做，這次也不開這個門
- **`DeleteCell`／`DeleteSeg` 動作內部的 `shift_down()`**：它在動作執行時
  又讀一次鍵盤。改成吃 `KeyEvent.shift` 比較乾淨，但那是動作層不是綁定層，
  第一步先不動

## 要點頭的（2026-09-13 全部裁決）

1. 整體做法（`Key` 列舉、`primary` 修飾鍵、分兩步）→ **照做**
2. 檔名 `core/src/binding.rs` → **照做**
3. 第一步那個行為變化（`Shift+數字` 不再挑候選）→ **接受**
4. 第二步的差異 1：段選單 Esc 統一成哪種 → **保留**

## 驗證

> **2026-09-14：下面每一條都跑過了，全過。**

**第一步**

1. `cargo test -p ime-core`——搬進去的測試要全過，**測試總數守恆**
   （core 多 33～34 個、Windows 少同樣數量）
2. `cargo check -p ime-tip-windows --target x86_64-pc-windows-msvc`（Mac 上）
3. Windows 那台：`cargo test`、`.\build-ime.ps1`、`.\check-dll.ps1`，
   記事本／瀏覽器／Word 各打一輪（方向鍵選字、TAB 段選單、`Shift+空白`、
   `Ctrl+Shift+空白`、`Ctrl+C` 要讓回宿主）

**第二步**

1. `cargo test -p ime-tip-macos`
2. 差異表六項逐條實打，確認變成裁決後的那一邊
3. `build-app.sh`，切走輸入法再切回來
