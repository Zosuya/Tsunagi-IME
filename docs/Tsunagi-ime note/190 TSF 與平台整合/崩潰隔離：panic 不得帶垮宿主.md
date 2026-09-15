---
原編號: "2.33"
date: 2026-09-02
status: done
一句話: 13 個 COM／wndproc 邊界包 catch_unwind；漏了 clear_poison 導致中毒 85 次、每鍵重設狀態
pitfall: true
相關:
  - "[[資料檔的信任邊界：壞資料不得帶走宿主]]"
---
# 崩潰隔離：panic 不得帶垮宿主

輸入法是**寄生在宿主行程裡的 DLL**。一般程式 panic 是自己當掉，我們
panic 是把使用者正在編輯的文件一起帶走——而且 `#[implement]` 生成的
COM 方法與 `wndproc` 都是 `extern "system"`，**unwinding 穿過去不是 UB
就是直接 abort**，連宿主的例外處理都攔不到。

`debug_log::install_panic_hook` 早就在了，但它只負責留下線索。這一節是
另一半。

### 2.33.1 降級的原則：寧可不作為，不要作亂

新增 `platform/windows/src/guard.rs`，在 13 個邊界包 `catch_unwind`：

| 邊界 | 降級成 | 使用者感受 |
|---|---|---|
| 按鍵處理 | `BOOL(0)`（沒處理） | 那一鍵原樣送進文件，像沒開輸入法 |
| 視窗訊息 | `DefWindowProcW` | 候選視窗行為退回預設 |
| 其他 COM | `E_FAIL` | TSF 自己決定怎麼辦 |

**絕不回「已處理」**——那會讓按鍵憑空消失，比崩潰更難查。這一條有測試
釘住。

包起來的是：`ITfKeyEventSink` 六個方法、`Activate`／`ActivateEx`／
`Deactivate`、`DoEditSession`（回呼裡跑的正是核心引擎，最可能 panic）、
候選視窗／語言選單／全半形提示三個 `wndproc`。

### 2.33.2 真正的難點是 `Mutex` 中毒，而且第一版寫錯了

持鎖期間 panic 會讓 Rust 的 `Mutex` 中毒。原本十處 `.lock().unwrap()`
在中毒之後會**再 panic 一次**——`catch_unwind` 攔得住當機，攔不住「從此
每一鍵都失敗」。另外三處寫成 `if let Ok(..) = lock() `，中毒後**靜靜什麼
都不做**，症狀是「輸入法還在、按鍵完全沒反應」，更難查。

改用 `lock_state()` 統一處理。**但第一版只做了 `into_inner()`，漏了清
旗標**——中毒旗標是**黏著的**，於是每一次取鎖都還是回 `Err`，每按一鍵
都跑一次狀態重設：

```text
打 s → 復原分支 → 清空 → 收下 s
打 u → 復原分支 → 清空（s 沒了）→ 收下 u
```

永遠只留得住最後一鍵。**沒當機、沒再 panic，但實質上不能用**——正好落在
這個功能想避免的情境。

本機實測抓到的，log 的形狀是決定性的證據：

| | 第一版（壞） | 修好後 |
|---|---|---|
| `[攔下 panic]` | 2 次 | **4 次** |
| `[狀態鎖中毒]` | **85 次** | **4 次** |
| debug log 行數 | 123 | 27 |

修法是一行 `m.clear_poison()`（Rust 1.77 起穩定）。**「中毒次數必須等於
panic 次數」是這件事最可靠的判準**，比肉眼看打字順不順準得多。

### 2.33.3 三個教訓

**一、`catch_unwind` 的測試抓不到中毒的 bug。**六條 `guard` 測試全過，
但沒有一條問「復原之後鎖是什麼狀態」。補的那條就是會抓到它的：

```rust
drop(lock_state(&m));
assert!(!m.is_poisoned(), "復原之後旗標必須清掉");
```

**二、能往下推到平台無關的地方，就別留在只能靠實測的地方。**
順手掃出 `core` 有三個同性質但更隱蔽的（`learn.rs` 兩個、`pack.rs` 一
個）：讀的中毒後永遠回空的（學習層與領域包會靜靜變成沒東西），寫的靜靜
消失。改成 `read_or_recover`／`write_or_recover`——**同樣的 bug，平台層
要實測才發現，`core` 這邊寫個測試就抓到了**。

**三、驗證用的 grep 不要寫太窄。**這輪一度宣稱「clippy 乾淨」，其實是
grep 只濾了 `warning: unu` 與 `warning: fun`，漏掉
`items_after_test_module`。要嘛不濾，要嘛濾了就別說「乾淨」。

### 2.33.4 怎麼重驗

**觸發點已經留在程式碼裡了**（`guard::maybe_panic`），不必再臨時埋。

```powershell
.\build-ime.ps1 -PanicTest      # 正式版編不進這段程式碼
```

然後在 `data/` 建一個 `panic.on`，內容寫要炸的邊界關鍵字：

| 寫什麼 | 炸哪裡 |
|---|---|
| `OnKeyDown` | 按鍵處理 |
| `paint` | 候選視窗繪製 |
| `DoEditSession` | 其他 COM 路徑 |
| `wndproc` | 視窗訊息 |

比對用**子字串**，所以寫 `On` 會連 `OnKeyUp`、`OnSetFocus` 一起炸。
**炸完檔案自己刪掉，只發生一次**——要驗的第二件事正是「攔完之後還能
不能繼續打字」，檔案留著會每一鍵都炸，那條就驗不到。

判準是**兩個 log 都要有**：`%TEMP%\ime_panic.log` 有「故意炸一次」代表
觸發到了，`%TEMP%\ime_debug.log` 有「[攔下 panic]」才代表 guard 攔住。
只有前者代表宿主要死了。

跑完**務必不帶開關重建一次**，否則磁碟上留著含測試開關的 DLL。

#### 為什麼從「臨時埋 F9」改成 feature

原本的做法是在 `text_service/mod.rs` 插一行 `if vk == 0x78 { panic!() }`，
測完 `git checkout` 還原。那樣可行，但有兩個成本：**每次重驗都要改程式碼**，
而且**忘了還原就把測試觸發點帶進正式版**。

改成 cargo feature 之後，觸發點是常駐的程式碼但 `#[cfg(not(feature))]`
那半是空函式，正式版整段編不進去。重驗不必動任何一行程式碼。

**為什麼不用 debug build**：註冊表寫的是 `target
elease\` 的絕對路徑，
debug 版產在別的地方，要測得提權重新註冊一次——那樣沒人會用。

改動任何 COM 邊界之後都該重跑一次。

### 2.33.5 第二輪：換成 feature 之後重驗（2026-09-02）

三個邊界各測一次，記事本：

| 邊界 | 實際看到 | log |
|---|---|---|
| `OnKeyDown` | 按 `s` 原樣冒出字母 `s`，記事本活著 | `[攔下 panic] OnKeyDown` |
| 復原 | 下一鍵 `u` 就恢復正常組字 | —— |
| `paint` | **完全無感**（60fps 丟一幀看不出來） | `[攔下 panic] paint` |

**這一輪的工具本身寫錯過一次，把記事本帶走了**：`maybe_panic` 放在
`catch_unwind` **外面**，炸的位置根本不在保護範圍內。`ime_panic.log`
留下的證據是 panic 後面緊接著一行 `panic in a function that cannot unwind`
——**那就是宿主的死亡證明**，也是本節開頭那段理論的實測版。

修法見 commit `8353e5e`：三處都移進閉包裡，`paint` 改成走 `guard` 而不是
自己 `catch_unwind`（自己包一層就繞過觸發點了），`maybe_panic` 收成模組
私有——位置放錯在編譯期看不出來，只有真的把宿主弄當才會發現。

**測 log 的兩個坑**：log 檔累積不輪替，混著前幾天的紀錄很容易讀錯
（第一次就差點被一筆早已移除的臨時 panic 誤導）；攔截那行也不一定在
檔尾，後面的正常打字會把它推上去，要用搜尋不要用 `tail`。
