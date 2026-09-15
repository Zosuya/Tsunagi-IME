---
原編號: "2.34"
date: 2026-09-01
status: done
一句話: 載入中的 DLL 與 mmap 詞庫都不能就地覆寫，但改名讓路可以；Inno 的 CloseApplications 必須關掉
pitfall: true
相關:
  - "[[安裝程式：三階段實測全過，data_dir 在 Program Files 會靜默失效]]"
---

# spike：升級安裝時檔案被鎖住

**問題**：安裝程式要覆寫的兩種檔案，在升級當下都可能正被使用——DLL 載在
每一個宿主行程裡（檔案總管永遠挖著它），詞庫則被 mmap 映射著。
**Inno Setup 的預設行為是直接覆寫，那會失敗。**

先不碰安裝工具，直接量 Windows 的檔案鎖定行為——那才是問題的根，跟用
哪個安裝器無關。

### 2.34.1 量到的：兩種檔案行為不同

| 操作 | DLL（`LoadLibrary` 載入中） | 詞庫（mmap 映射中） |
|---|---|---|
| **就地覆寫** | ❌ 被另一個行程使用 | ❌ 「檔案的使用者對應區段在開啟狀態」 |
| **改名讓路，再放新檔** | ✅ | ✅ |
| **刪掉改名後的殘骸** | ❌ 還被載著 | ✅ **可以立刻刪** |

模擬條件是照 Rust 的實際行為設的：`File::open` 在 Windows 上是**全共享**
（`FILE_SHARE_READ | WRITE | DELETE`），所以詞庫那欄用同樣的 share mode 測，
不是隨便開一個 FileStream——第一次量錯就是因為 share mode 設太嚴，
測到的是 FileStream 的鎖而不是 mmap 的。

**差異的原因**：DLL 載入後是 image section，Windows 不讓刪；詞庫是 data
section，開檔時允許刪除，**刪掉之後映射仍然有效**（實測：改名並刪除原檔
之後，映射讀到的還是舊內容）。這也正是 §2.29.5 那條「產生器先寫暫存檔
再原子改名」的 SAFETY 依據——舊映射看到的位元組不會在腳下變動。

### 2.34.2 安裝策略因此確定

**跟 `build-ime.ps1` 同一招**（那支腳本每天在對付一樣的問題）：

1. 動手前先掃並刪掉上次留下的殘骸（那時佔用的行程通常已經關了）
2. 舊檔改名成帶時間戳的名字讓出路徑——**固定名稱會卡住**，上一輪的殘骸
   還被佔用時這一輪改名就失敗
3. 放新檔
4. 試刪殘骸：詞庫會成功，DLL 通常失敗，**失敗是預期的**，留到下次

**不用 Inno 的 `restartreplace`**（排程重開機替換）。那招乾淨但要求使用者
重開機，而改名讓路當場就完成——反正宿主行程本來就要重開才會載到新版
DLL，多要求一次重開機沒有換到任何東西。

### 2.34.3 Inno Setup 實測：策略成立，但要先關掉一個預設行為

裝了 Inno Setup 6.7.3，寫一支最小的 `.iss` 實測（安裝一個假 DLL、載入
鎖住、再跑升級）。**兩次失敗才過**，兩次的原因都值得記。

#### 第一次失敗：Restart Manager 主動關掉佔用的程式

結束碼 5，而且 `PrepareToInstall` **連叫都沒被叫到**。log 說得很清楚：

```text
RestartManager found an application using one of our files: PowerShell 7
Shutting down applications using our files.
（30 秒後）
Some applications could not be shut down.
User canceled the installation process.
```

Inno 6 預設 `CloseApplications=yes`：偵測到檔案被占用就**主動關閉那些
程式**。對一般軟體合理，**對輸入法是災難**——占用我們 DLL 的是檔案總管，
關掉等於桌面與工作列消失，而且它會自動重啟又立刻載回 DLL，永遠關不乾淨。
使用者看到的是卡 30 秒然後安裝失敗。

必要設定：

```ini
CloseApplications=no
RestartApplications=no
```

#### 第二次失敗：路徑字串被跳脫字元吃掉

改名那段完全沒作用，log 顯示 Inno 直接去刪舊檔然後失敗。根因不在 Inno
——是寫 `.iss` 時經過 shell 與 Python 兩層字串解析，反斜線被當成跳脫序列，
`路徑 + 反斜線 + test.dll` 變成了 `路徑 + TAB + est.dll`。那個檔案永遠
不存在，`FileExists` 永遠回 false。

**教訓**：產生含 Windows 路徑的檔案時不要經過多層字串解析。這個坑在同
一天踩了四次（CLAUDE.md、開發文件、README、`.iss`），前三次都是把
`反斜線 b` 寫成了退格字元。

#### 通過的樣子

```text
結束碼 = 0
  test.dll
  test.dll.old-20260903002057      ← 改名讓路留下的殘骸
log: PrepareToInstall 進來了 → 舊檔已改名讓路
```

放開鎖再裝一次，舊殘骸確實被清掉。

### 2.34.4 一個要修的：不該無條件改名

現在的 `PrepareToInstall` 只要舊檔存在就改名，**沒人佔用時也照改**——
於是每次安裝都留一個 1.5MB 的殘骸，下次安裝才清。

正確順序是**先試刪除，刪不掉才改名**：

1. 掃並刪掉上次的殘骸
2. `DeleteFile(舊檔)` 成功 → 什麼都不用做，讓 Inno 正常覆寫
3. 失敗 → 才改名讓路

沒人佔用時乾乾淨淨，被佔用時才留殘骸。正式版要照這個寫。

### 2.34.5 spike v2：剩下三件也驗完了（提權執行）

| 驗證 | 結果 |
|---|---|
| 沒被鎖住 → 直接刪除，不留殘骸 | ✅ |
| 被鎖住 → 改名讓路，升級成功 | ✅ 結束碼 0 |
| 殘骸刪不掉 → 登記開機刪除 | ✅ |
| 註冊表真的有那筆排程 | ✅ 值以 `*1` 開頭，代表刪除 |
| 外部程式的結束碼讀得到 | ✅ 拿 `cmd /c exit 42` 當替身，讀回 42 |

**開機刪除怎麼呼叫**：`MoveFileExW` 的第二個參數傳 NULL 就是「刪除」。
在 Inno 的 Pascal Script 裡要把它宣告成 `Cardinal` 才傳得了 0——傳空
字串不等於 NULL，那樣會失敗。需要管理員權限（寫的是 HKLM 的
`PendingFileRenameOperations`），而正式安裝程式本來就有。

**驗外部程式結束碼是為了註冊**：正式版要用 `Exec` 跑 `register_tool`，
**註冊失敗必須偵測得到**，不能默默裝完讓使用者以為成功了。

#### 真實升級的形狀

v2 第四步的 log 同時出現兩行，值得記下來：

```text
殘骸刪不掉，已登記下次開機刪除: test.dll.old-20260903002635
舊檔沒被佔用，直接刪掉，不留殘骸
```

被鎖住的是**上一輪改名過的舊檔**，而當前的 DLL 沒人載。真實升級就是這個
形狀：每次只有當前 DLL 被宿主鎖著，更早的殘骸靠開機排程收尾。

### 2.34.6 正式版的安裝流程（驗證後定案）

1. 掃殘骸：能刪就刪，刪不掉就登記開機刪除
2. 當前檔案：**先試刪除**，成功就讓 Inno 正常覆寫（不留垃圾）
3. 刪不掉才改名讓路，名字帶時間戳
4. 改名也失敗才中止安裝並回報
5. 裝完用 `Exec` 跑 `register_tool register`，檢查結束碼
6. 反安裝時先 `unregister` 再刪檔

`[Setup]` 必要設定：

```ini
CloseApplications=no
RestartApplications=no
PrivilegesRequired=admin
```

前兩條的理由見 §2.34.3；`admin` 是因為要寫 Program Files、註冊 TSF、
登記開機刪除，三件都需要。
