---
原編號: "3.4"
date: 2026-08-31
status: done
一句話: 取代 regsvr32 看得到真正的 HRESULT；build-ime.ps1 靠改名讓路，deps\ 那份硬連結也要一起搬
pitfall: true
相關:
  - "[[已解決：TSF profile 註冊回傳 E_FAIL 的真正原因（權限）]]"
  - "[[架構決策與程式碼導覽]]"
---
# 開發小工具：register_tool 與 build-ime.ps1 的改名讓路

`platform/windows/src/bin/register_tool.rs` 是開發期用的診斷工具，行為上
等同 `regsvr32`（`LoadLibraryW` 目標 DLL、`GetProcAddress` 拿
`DllRegisterServer`/`DllUnregisterServer` 直接呼叫），但用一般主控台輸出
取代 `regsvr32` 的靜默結束碼／訊息框，方便看清楚失敗的 HRESULT 與錯誤說明。

```powershell
cargo build --release -p ime-tip-windows
target\release\register_tool.exe register   target\release\ime_tip_windows.dll
target\release\register_tool.exe unregister target\release\ime_tip_windows.dll
```

不帶路徑參數時預設抓自己同目錄下的 `ime_tip_windows.dll`。

> ⚠️ **必須在提權的終端機執行**（TSF 註冊寫 HKLM）。非提權時工具會回報
> `0x80070005`（存取被拒）並印出該怎麼做的說明，不會再像以前一樣只丟一個
> 沒有資訊量的 `E_FAIL`。

### 開發循環：改程式前要先把輸入法「放開」

輸入法開發有個固定的摩擦點：**Windows 不允許覆寫正在被載入的 DLL**。
只要有任何行程（VS Code、記事本、瀏覽器）還挖著 `ime_tip_windows.dll`，
`cargo build` 就會失敗：

```
error: failed to remove file `...\target\release\ime_tip_windows.dll`
```

**這個摩擦點已經解決**（2026-08-31）——用專案根目錄的 `build-ime.ps1`
取代 `cargo build`，不必關掉任何程式：

```powershell
.\build-ime.ps1             # 只建 DLL
.\build-ime.ps1 -All        # 連設定頁一起
.\build-ime.ps1 -CleanOnly  # 只清殘留檔
```

原理：**Windows 不允許刪除被載入的 DLL，但允許改名。**
把舊檔改名之後原路徑就空出來了，cargo 可以在那裡建立新檔；舊那份
繼續留在記憶體給既有行程用。

三個實作上的坑（都是實測踩出來的）：

| 坑 | 後果 | 解法 |
|---|---|---|
| 只搬 `target
elease\` 那份 | `LNK1104: 無法開啟檔案 ...\deps\ime_tip_windows.dll` | 那只是**硬連結**，連結器寫的是 `deps\` 底下那份，**兩個路徑都要讓開** |
| 用固定的 `.old` 名稱 | 上一輪的殘留檔還被佔用時，改名失敗，等於沒解決 | 用**時間戳**保證名字唯一，改名一定成功 |
| 腳本存成 UTF-8 無 BOM | Windows PowerShell 5.1 當 ANSI 讀，中文變亂碼**破壞語法** | 存成 **UTF-8 with BOM** |

> **驗證新版要開全新的行程**。既有的記事本、瀏覽器跑的仍是舊程式碼
> （舊 DLL 還映射在它們的位址空間裡），要開一個新的才會從磁碟載入新檔。

**改程式碼不需要重新註冊**——註冊表存的是「CLSID → 路徑」這種指標，
路徑沒變就不必動。只有改了顯示名稱、註冊類別、GUID，或搬動 DLL 位置
才要重註冊（那要提權終端機）。

手動的做法（腳本沒得用時的退路）：

1. `Win + Space` 切回別的輸入法（所有開著的視窗都要）
2. 關掉曾經切過我們輸入法的行程（切走不一定會馬上釋放 DLL）
3. `cargo build --release -p ime-tip-windows`

查誰還鎖著它：

```powershell
Get-Process | Where-Object { $_.Modules.ModuleName -contains 'ime_tip_windows.dll' } |
  Select-Object ProcessName, Id
```
