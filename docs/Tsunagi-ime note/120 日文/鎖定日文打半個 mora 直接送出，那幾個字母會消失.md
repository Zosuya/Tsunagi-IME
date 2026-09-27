---
date: 2026-09-20
status: done
一句話: 裁決 A——鎖定日文打 sush 按 Enter 送「すsh」，加 Session::commit_text() 統一送出入口，漏斗 0 差異，未做三宿主實打
pitfall: true
相關:
  - "[[Mac 送出時把沒成字的注音也送出去：追了五輪的定位過程]]"
  - "[[日文活用形：從長度門檻換成動詞分類還原]]"
---

# 鎖定日文打半個 mora 直接送出，那幾個字母會消失

> **狀態**：已修並合併（`b5445f5`），2026-09-27 使用者 Mac 實機驗過。兩平台共用 `Session::commit_text()`。
> 2026-09-20 查 macOS 送出 bug 時順帶挖出來的，**不是 macOS 專屬**
> ——兩個平台都有，修在 core。

**症狀**：鎖定日文打 `sush`，**直接按 Enter**，送出的是「す」，
**`sh` 兩個字元憑空消失**。使用者打過的東西沒了，也沒有任何提示。

## 為什麼會這樣

`RomajiInput` 的 `keys` 與 `pending` 是**互斥兩層**：`pending` 裡的字元
**不在** `keys` 裡，因此也**不在 `text()` 裡**。而送出一律用 `text()`。

- `sush` → `su` 湊成「す」進 `keys`，`sh` 卡在 `pending`
- 送出走 `text()` → 只拿得到「す」

## 諷刺的地方：core 裡有測試反對這件事

`core/src/session/tests.rs` 明文寫著：

```rust
// 殘留的半個 mora 也不能丟——sh 是使用者打過的
assert!(half.contains("sh"), "殘留的半個 mora 也要在：{half}");
```

**但那條測試測的是 `jp("sush,")`——打了標點。** 標點會觸發
`RomajiInput::push` 把 pending 沖回 keys，所以測試過。

**直接按 Enter 沒有任何東西做這件事。** 測試守的是另一條路。

> 這是「**測試要走產品真正跑的那條路**」的另一種形態：測試沒有自己組資料，
> 走的也是真的路——**但走的是「另一條」真的路**。
> 判準仍然是那句老話：**故意把修正還原，看測試會不會紅**。
> 這條測試在這個 bug 面前不會紅。

## 注音那一側有同樣的警告，而且處置方向相反

`core/src/input.rs` 的註解：

> **停在緩衝裡是錯的**：那時直接按 Enter 送出，`text()` 只看已收尾的按鍵，
> 那個標點就憑空消失了。

**這是全專案最明確的「送出時 pending 會被丟掉」的書面記錄。**
而它的處置方式是**「不要讓東西停在緩衝裡」**，不是「送出時把緩衝一起送」
——印證了「送出不含 pending」本身是刻意的設計，要修的是**別讓該送的東西
留在 pending**。

## 兩平台都一樣，不是 macOS 漏接

- Windows 送出路徑共 8 處，**全部** `state.session.text()`，零例外
- `platform/windows/src/` 底下 grep `pending`：**0 筆命中**。
  Windows 平台層根本不知道 pending 這個概念存在——它連「要不要送 pending」
  這個問題都沒問過，純粹因為一律用 `text()` 而**碰巧在注音那件事上正確**

## 裁決（2026-09-27）：A

**已成字的照常轉假名，沒成字的尾巴照原樣字母送出**：
`sush` + Enter → **`すsh`**（不是憑空消失，也不是硬湊一個不完整的假名）。

**鎖定注音刻意不比照辦理**——那條路已經有明確的相反規則（`input.rs` 的
`drain_keys` 註解、2026-09-20 的修法）：半個注音符號不成字，送出時就是
要丟掉。這個裁決只動日文那一條路。

## 修法：`Session::commit_text()`

沒有動 `Input::drain_keys()`（那支是「換模式」用的，結算方式是把 pending
併回 `keys` 讓輸入層重新解析——不適合這裡，會把 `sh` 拿去跟後面的東西
重新湊 mora）。而是在 `session/cutting.rs` 加一支跟 `text()` 平行的方法：

```rust
/// 真正送出（Enter／空白確認／切換語言／失焦……）時該送的文字。
pub fn commit_text(&self) -> String {
    let mut t = self.text();
    if self.lock == Some(crate::language::Language::Romaji) {
        t.push_str(&self.input.pending());
    }
    t
}
```

只在鎖定日文時把 `pending`（未成字的殘留字母）原樣接在 `text()` 後面。
鎖定注音、自動模式、鎖定英文呼叫這支等於呼叫 `text()`（前者是刻意
不比照，後兩者的 `pending()` 本來就是空字串）。

### 兩平台都要換掉所有送出入口

**macOS**：只有一個入口，`echo_ime.rs` 的 `commit()`——原本送出用
`session.text()`（`composition()` 含 pending 只給顯示用，見同一支函式
的長註解），改成 `session.commit_text()`。

**Windows**：8 個入口，全部是 `let text = state.session.text();` 緊接
`end_composition(..., EndKind::Commit(&text))?` 這個寫法，一次 sed 換掉：

- `Action::Commit`（Enter）
- `Action::NumpadInput`（數字鍵中斷組字）
- `Action::SegPick` / `Action::SegConfirm` 的「最後一段選完直接送出」
  （`commit_on_last_seg`，各一處）
- `Action::ConfirmCand`（選字選完直接送出，`commit_on_last`）
- `Action::CycleLock` / `Action::ToggleWidth` 切進「直接輸入」模式時
  先送出手上的組字（各一處）
- `ui.rs` 的 `on_candidate_picked`（滑鼠點候選字，跟鍵盤版 `ConfirmCand`
  同一套邏輯，是獨立的第 8 個入口）

`ui.rs:60` 的 `state.session.text()` 是組字區預覽用（顯示反白位置），
**不是送出**，沒有換。Windows 的 `Deactivate` / `OnCompositionTerminated`
（失焦、被系統中止組字）呼叫的是 `state.session.clear()`，本來就是
取消不是送出，也沒有這個問題。

## 驗證

- `core/src/session/tests.rs` 新增 4 條測試，涵蓋 `sush`→`すsh`、
  `k`→`k`（連一個 mora 都沒有）、`kan`→`かn`（單獨的 `n` 不是完整撥音，
  不能硬湊成撥音）、`sushi`（完整打完不受影響）、鎖定注音／自動模式／
  鎖定英文的 `commit_text()` 等於 `text()`。判準驗過：把 `commit_text()`
  改回單純呼叫 `text()`，新測試會紅
- `cargo test --release -p ime-core` 連跑 3 次全過，`clippy --all-targets`
  0 警告
- 漏斗 `check_all --all`：改前改後逐句比對 **0 差異**（這個 bug 的觸發
  條件是「組字中途、pending 非空時直接送出」，測資裡沒有這種句子）

**還沒做**：TSF／IMK 實際在記事本、瀏覽器、Word 打字驗證（`status:
unverified` 的原因）。也還沒合併回 master——目前在獨立的
`fix/half-mora` worktree 分支上。

**注意**：這件事跟「[[Mac 送出時把沒成字的注音也送出去：追了五輪的定位過程]]」
**性質不同，沒有混在同一次修改裡**——那個是 macOS 誤用顯示用的函式（純 bug、
一行修掉），這個是兩平台共有的設計缺口。
