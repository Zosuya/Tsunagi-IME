---
原編號: "3.2"
date: 2026-08-23
status: done
一句話: msctf 把 HKLM 寫入的權限不足吞成 E_FAIL；繞了九項排查才想到「有沒有提權」
pitfall: true
相關:
  - "[[Phase 0 結案：TSF 管線在記事本／瀏覽器／Word 實測通過]]"
  - "[[開發小工具：register_tool 與 build-ime.ps1 的改名讓路]]"
---
# 已解決：TSF profile 註冊回傳 `E_FAIL` 的真正原因（權限）

> **結論先講**（2026-08-23 解決）：`RegisterCategory` / `RegisterProfile` 內部寫的是
> `HKLM\SOFTWARE\Microsoft\CTF\TIP`，**非提權行程寫不進去**，msctf 就回傳一個
> 毫無資訊量的 `E_FAIL`。在提權終端機下跑同一支 `register_tool.exe`，
> 立刻回傳 `S_OK`。與程式碼、windows-rs、系統映像完整性都無關。

### 現象

呼叫下列任一 API 都回傳 `E_FAIL`（`0x80004005`）：

- `ITfCategoryMgr::RegisterCategory(clsid, GUID_TFCAT_TIP_KEYBOARD, clsid)`
- `ITfInputProcessorProfiles::Register(clsid)`（舊版兩段式 API）
- `ITfInputProcessorProfileMgr::RegisterProfile(...)`（Vista 後新版一次到位 API）

而在同一個行程裡，`CoCreateInstance(CLSID_TF_CategoryMgr / CLSID_TF_InputProcessorProfiles, ..., CLSCTX_INPROC_SERVER)`
本身都成功——代表 COM 基礎設施是通的，失敗的只有「寫入類」功能。

### 根因

TSF 的 text service 註冊資訊是**全機器共用**的，一律登記在
`HKLM\SOFTWARE\Microsoft\CTF\TIP\{CLSID}` 底下。因此：

- **註冊 TSF profile / category 必須以系統管理員權限執行**。
- msctf 在 HKLM 寫入失敗時，**不會**回傳 `E_ACCESSDENIED`，而是統一吞成
  `E_FAIL`。這是整件事最誤導人的地方——錯誤碼完全沒指向真正的問題。

本專案早期的 `registration.rs` 刻意只寫 HKCU，註解裡的理由是「開發期
`regsvr32` 不需要系統管理員權限就能跑」。**那個推論對純 COM 註冊成立**
（COM 解析 CLSID 時會合併查詢 `HKCU\Software\Classes`），這也是為什麼
`CoCreateInstance` 我們自己的 CLSID 一直都成功——**而這正是把排查方向
帶偏的關鍵**：COM 那半邊能動，讓人誤以為註冊機制整體沒問題。但 TSF 不吃
HKCU 這套，寫在 HKCU 的 TIP 項目即使寫得進去也不會生效。

### 排除過程（依嘗試順序）與事後檢討

下列 1–9 項是當初的排查紀錄。**它們每一項的觀察都正確，但因為沒想到
「權限」這個變因，全部走向了錯誤的結論**，保留於此作為教訓。

1. **懷疑是我們自己 CLSID 註冊寫錯** → 用 `[Type]::GetTypeFromCLSID` /
   `[Activator]::CreateInstance` 從 PowerShell 直接 `CoCreateInstance`
   我們的 CLSID，成功建立物件。排除。
2. **懷疑是舊版 `ITfInputProcessorProfiles` API 過時** → 改用 Vista 後的
   `ITfInputProcessorProfileMgr::RegisterProfile`，結果一樣 `E_FAIL`。排除。
3. **懷疑是 icon file 參數傳空陣列導致內部出錯** → 改傳自己 DLL 的完整路徑
   當 icon file，仍然 `E_FAIL`。排除。
4. **懷疑是目標語言（zh-TW, `LANGID 0x0404`）未安裝** → 確認 zh-TW、ja
   都已安裝。排除。
5. **懷疑是 Group Policy 關掉了 Advanced Text Services** → 檢查
   `HKLM/HKCU\Software\Policies\Microsoft\CTF`，兩處都不存在。排除。
6. **懷疑 TSF 內部會反過來 `CoCreateInstance` 我們的 CLSID 做驗證** →
   在 `ClassFactory::CreateInstance` 加 log，確認呼叫期間沒有回呼。排除。
7. **懷疑呼叫端行程沒有訊息佇列/視窗站台** → 確認 window station、
   session id、訊息佇列都正常。排除。
8. **懷疑是本工具鏈造成的假象** → 改用真正的 `regsvr32.exe` 重試，結果
   相同（exit code 5）。排除「是我們自己工具有問題」。
   **← 這裡其實已經摸到答案了**：`regsvr32` 的 exit code 5 正是
   `ERROR_ACCESS_DENIED`。當時只把它讀成「`DllRegisterServer` 失敗」的
   泛稱，沒有去查那個 5 的真正含義。
9. **決定性測試：寫一支原生 C++ 小程式**（見下方原始碼），完全不經過
   Rust / windows-rs，結果完全相同。這正確地證明了「與 Rust / windows-rs
   無關」，**但當時錯誤地把它推論成「所以是這台機器的 msctf.dll 有缺陷」**
   ——真正的共通點不是「都用了某個壞掉的 DLL」，而是**這些測試全都在同一個
   非提權的工作階段裡跑**。

### 兩條把方向帶偏的假線索

1. **`GetLastError()` 回報 `127`（`ERROR_PROC_NOT_FOUND`）**：當初據此推論
   「msctf 內部 `GetProcAddress` 找不到函式 → 系統映像不完整」。實際上
   **`GetLastError` 在 API 已經回傳失敗的 HRESULT 之後不保證有意義**，
   那個 127 極可能是更早之前某次無關呼叫的殘留值。追一個沒有保證的
   錯誤碼，是這次繞遠路的主因之一。
2. **「環境是沙盒／精簡版 Windows」的假設**：事後查證，這台機器是實體的
   Windows 11 Pro 25H2（build 26200），`ctfmon.exe` 與 `TextInputHost.exe`
   都正常運作、session 一致、系統本身已註冊數十個 TIP、Group Policy 也
   沒有任何限制。環境完全正常。

**教訓**：遇到 `E_FAIL` 這種泛用錯誤碼時，「目前是不是以管理員權限在跑」
應該是最先確認的幾件事之一，而不是繞完一圈才想到。

### 用來排除的原生 C++ 驗證程式

保留這支程式的原始碼供日後參考——它成功證明了「與 Rust / windows-rs 無關」
這一點（只是當時對結果的解讀有誤）。若要重跑，**記得在提權的終端機下執行**，
否則會重現同一個 `E_FAIL`。

```cpp
#include <windows.h>
#include <msctf.h>
#include <initguid.h>
#include <stdio.h>

DEFINE_GUID(CLSID_TEXT_SERVICE, 0xa00a3a9b, 0x0c3a, 0x4306,
    0xb4, 0xad, 0x5d, 0x47, 0xae, 0x8c, 0x37, 0x05);

int main() {
    CoInitializeEx(NULL, COINIT_APARTMENTTHREADED);

    ITfCategoryMgr* pCategoryMgr = NULL;
    HRESULT hr = CoCreateInstance(CLSID_TF_CategoryMgr, NULL,
        CLSCTX_INPROC_SERVER, IID_ITfCategoryMgr, (void**)&pCategoryMgr);
    if (SUCCEEDED(hr)) {
        hr = pCategoryMgr->RegisterCategory(CLSID_TEXT_SERVICE,
            GUID_TFCAT_TIP_KEYBOARD, CLSID_TEXT_SERVICE);
        printf("RegisterCategory: 0x%08X\n", hr);
        // 非提權：0x80004005 (E_FAIL)／提權：0x00000000 (S_OK)
    }

    ITfInputProcessorProfileMgr* pProfileMgr = NULL;
    hr = CoCreateInstance(CLSID_TF_InputProcessorProfiles, NULL,
        CLSCTX_INPROC_SERVER, IID_ITfInputProcessorProfileMgr, (void**)&pProfileMgr);
    if (SUCCEEDED(hr)) {
        hr = pProfileMgr->RegisterProfile(CLSID_TEXT_SERVICE, 0x0404, GUID_NULL,
            L"probe", 5, NULL, 0, 0, NULL, 0, TRUE, 0);
        printf("RegisterProfile: 0x%08X\n", hr);
    }

    CoUninitialize();
    return 0;
}
```

編譯：`cl /nologo /EHsc tsf_probe.cpp ole32.lib`（需先跑
`vcvars64.bat` 設好 MSVC 環境）。

### 修正後的程式碼行為

`platform/windows/src/registration.rs` 已據此調整：

- **COM 的 `InProcServer32` 也改寫 HKLM**，與 TSF profile 保持一致。
  兩邊不一致會造成一個隱形陷阱：TSF 那份登記在 HKLM（全機器可見），
  COM 那份卻只在 HKCU（僅該使用者可見），跨使用者情境的行程（登入畫面、
  UAC 提示框、其他帳號的程式）會看得到這個 TIP、卻查不到 DLL 在哪。
- `unregister` 時**一併清掉 HKCU 的舊殘留**。舊新兩份同時存在時，COM
  解析 CLSID 會優先命中 HKCU，若它指向舊路徑就會載入錯誤的 DLL。
- `register()` / `unregister()` **開頭先檢查是否提權**，未提權直接回傳
  `E_ACCESSDENIED` 並附說明，不再讓人對著 `E_FAIL` 猜。
- `register_tool` 在收到 `0x80070005` 時額外印出一句中文指示（Error 附帶的
  訊息字串過不了 DLL 邊界，所以要在工具端再講一次）。

### 使用方式

註冊/反註冊**必須在提權的終端機**下執行：

```powershell
# 在「以系統管理員身分執行」的 PowerShell 裡
cargo build --release -p ime-tip-windows
.\target\release\register_tool.exe register   .\target\release\ime_tip_windows.dll
.\target\release\register_tool.exe unregister .\target\release\ime_tip_windows.dll
```

從一般權限的終端機彈出提權視窗：

```powershell
Start-Process pwsh -Verb RunAs -ArgumentList '-NoExit','-Command','cd ''d:\CODE\通用語言輸入法開發'''
```

> ⚠️ **注意 DLL 路徑**：註冊寫進登錄檔的是 DLL 的**絕對路徑**，目前指向
> `target\release\`。`cargo clean`、改用 debug build、或搬動專案資料夾之後，
> 這個路徑就會失效，必須重新註冊。
