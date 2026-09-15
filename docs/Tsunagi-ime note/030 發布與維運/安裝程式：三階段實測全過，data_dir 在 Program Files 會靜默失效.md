---
原編號: "2.35"
date: 2026-09-01
status: done
一句話: 首次／升級／反安裝三階段實測全過；data_dir() 寫死往上兩層裝到 Program Files 會靜默失效
pitfall: true
相關:
  - "[[spike：升級安裝時檔案被鎖住]]"
  - "[[Code signing：第一版先不簽，公開後再申請開源免費簽章]]"
---

# 安裝程式：三階段實測全過，data_dir 在 Program Files 會靜默失效

照 §2.34.6 定案的流程做出來了。**首次安裝、升級安裝、反安裝三個階段都已實測
通過**（首次安裝見 §2.35.4，升級與反安裝 2026-09-03 補驗，見 §2.35.5）。

### 2.35.1 產出

| 檔案 | 做什麼 |
|---|---|
| `installer/tsunagi.iss` | Inno 腳本 |
| `installer/build-installer.ps1` | 建置＋打包，不互動、失敗回非零（為了 CI 簽章） |
| `installer/languages/ChineseTraditional.isl` | 繁中語言檔（Inno 沒內建，放專案裡 CI 才有） |
| `tools/check-install.ps1` | 唯讀盤點：裝了什麼、殘留在哪、誰載著 DLL |

安裝程式 **19.5 MB**（打包內容 61MB，lzma2/max）。裝到
`C:\Program Files\Tsunagi IME`，`data/` 就在執行檔旁邊。

### 2.35.2 實作時才發現的四件事

**一、`data_dir()` 會指到錯的地方。** 原本寫死「往上兩層」（開發環境的
`target/release/x.dll` → 專案根 `data/`），裝到 Program Files 之後同一個
算式指向 `C:\data`。**不會報錯，只是所有候選變空的**——切得過去、有反應、
打不出字。改成先看執行檔旁邊。設定頁的 `project_data_dir()` 同一個毛病。

**二、`runasoriginaluser` 只有 `[Run]` 支援。** 安裝程式跑在系統管理員
身分，而「加進輸入法清單」寫的是**目前使用者**的設定——不降權就加到
Administrator 的清單去了。`[UninstallRun]` 沒有這個旗標，所以反安裝不做
`disable`，靠 `unregister` 移掉 profile 讓清單項目自然消失。

**三、`ime_settings.exe` 沒有內嵌圖示**，於是「設定 → 應用程式」和開始
功能表都是白紙一張（`UninstallDisplayIcon` 正是指向它）。`settings` 補
`build.rs`，做法同 `platform/windows/build.rs`——呼叫 Windows SDK 的
`rc.exe`，不引第三方套件，圖示檔直接引用 `platform/windows/res/ime.ico`
不複製。

**四、目錄頁的「瀏覽」會多接一層。** 使用者選中已存在的安裝資料夾時，
Inno 預設再接一層 AppName，於是出現巢狀的
`Tsunagi IME\Tsunagi IME`。要 `AppendDefaultDirName=no`。

### 2.35.3 反安裝要連空殼一起收

移除之後 Program Files 底下會留一個空資料夾——那時 DLL 殘骸還在（已登記
開機刪除但還沒開機），目錄非空所以 Inno 不刪。改成先試 `RemoveDir`，
失敗就把目錄本身也登記開機刪除。

**順序是對的**：檔案先登記、目錄後登記，而 `PendingFileRenameOperations`
按登記順序執行——開機時先刪檔案，輪到目錄時才空得掉。

### 2.35.4 已驗證

在**全新乾淨的機器狀態**下實測（先把開發用的註冊與使用者資料完全移除）：

| 項目 | 結果 |
|---|---|
| 首次安裝 | ✅ 檔案佈局正確，`data/` 在 DLL 旁邊 |
| 註冊 | ✅ COM 指向 Program Files 下的 DLL，TSF 樹完整 |
| **打字** | ✅ `su3cl3` → 你好（這條驗的是 `data_dir()` 的修復） |
| **輸入法清單** | ✅ 勾了選項就自己出現，不必去設定裡新增（驗 `runasoriginaluser`） |
| 開始功能表 | ✅ 單一捷徑 |

### 2.35.5 升級與反安裝也驗過了（2026-09-03）

三個階段都在真實安裝程式上跑完：**首次安裝 → 升級安裝 → 反安裝**。

| 項目 | 結果 |
|---|---|
| 升級安裝（DLL 被宿主載著） | ✅ 順利完成，不卡不報錯 |
| 反安裝後的註冊 | ✅ COM 與 TSF 兩棵樹全清 |
| 反安裝後的資料夾 | ✅ `C:\Program Files\Tsunagi IME` **整個消失** |
| 殘骸與開機排程 | ✅ 沒有留下任何東西 |

「連資料夾本身也登記開機刪除」那條修改是有效的——沒有它的話，Program
Files 底下會留一個空殼（改名前那次安裝就是這樣，留到手動清掉為止）。

**改了安裝資料夾名稱之後要先手動清乾淨再測**：Inno 靠 AppId 認出是同一個
程式，會沿用註冊表裡記的舊路徑，舊安裝不會自己搬家。這次就是先移除舊版
（`tsunagi-ime`）才裝到新路徑（`Tsunagi IME`）。

### 2.35.6 安裝程式完成，剩下的是簽章

> **後來的變化**：code signing 決定**第一版先不簽**，GitHub Actions 因此也先不接
> （它唯一的硬性理由就是走 CI 簽章；repo 目前沒有 `.github/workflows`），
> 見 [[Phase 5 穩定性與發布]]。下面「下一步」那句是當時的規劃。

Phase 5 的安裝程式這一項可以結案了。還沒接上的只有 code signing——那要等
repo 公開後申請（[[Code signing：第一版先不簽，公開後再申請開源免費簽章]]），
而且是走 CI 管線簽，所以下一步是把 `build-installer.ps1` 接進 GitHub Actions。
