---
原編號: "3.1"
date: 2026-08-23
status: done
一句話: Phase 0 退出條件達成——整條 TSF 管線在記事本／瀏覽器／Word 三環境實測通過
相關:
  - "[[已解決：TSF profile 註冊回傳 E_FAIL 的真正原因（權限）]]"
  - "[[架構決策與程式碼導覽]]"
---
# Phase 0 結案：TSF 管線在記事本／瀏覽器／Word 實測通過

| 項目 | 狀態 |
|---|---|
| Repo 結構（`core/`、`platform/windows/`、`data/`、`docs/`） | ✅ 完成 |
| Rust workspace + `ime-core` 最小骨架 | ✅ 完成，含單元測試 |
| Windows TSF TIP：COM class factory / `DllGetClassObject` / `DllCanUnloadNow` | ✅ 完成，編譯零警告 |
| `ITfTextInputProcessor`（Activate/Deactivate）+ `ITfKeyEventSink`（按鍵處理）+ `ITfCompositionSink` | ✅ 完成 |
| Echo IME 邏輯：A-Z 組字、Backspace、Esc 取消、Space 叫候選、Enter/數字鍵送出 | ✅ 完成（見 `text_service.rs`） |
| 候選視窗（自繪 Win32 popup，`WS_EX_NOACTIVATE` 不搶焦點） | ✅ 完成（見 `candidate_window.rs`） |
| COM 伺服器註冊（`DllRegisterServer`/`DllUnregisterServer`，**HKLM**） | ✅ 完成，已驗證 `CoCreateInstance` 能正確建立我們的 CLSID |
| 組字底線（display attribute provider） | ✅ 完成（見 `display_attribute.rs`） |
| **TSF profile / category 註冊**（`RegisterCategory`、`RegisterProfile`） | ✅ **已解決**（2026-08-23）：需管理員權限 + 寫 HKLM，見 [[已解決：TSF profile 註冊回傳 E_FAIL 的真正原因（權限）]] |
| 在**記事本、瀏覽器、Word** 實測組字、底線、候選定位、送出、游標同步 | ✅ **全數通過**（2026-08-23） |

**✅ Phase 0 退出條件已達成**（2026-08-23）。

整條「打字 → 系統看到組字串 → 顯示候選 → 送出文字」的 TSF 管線，已在
**記事本、瀏覽器、Word** 三種環境實際驗證通過，包含組字底線、候選視窗
跟隨定位、插入點同步、Esc 取消、以及滑鼠點擊候選視窗不搶焦點。

达成過程中解決的問題（各自的踩坑紀錄見後續章節）：

| 問題 | 根因 | 紀錄 |
|---|---|---|
| TSF profile 註冊回 `E_FAIL` | 需管理員權限（寫 HKLM） | [[已解決：TSF profile 註冊回傳 E_FAIL 的真正原因（權限）]] |
| Esc 取消後文字還在 | `EndComposition` 不刪文字 | [[架構決策與程式碼導覽]] |
| 組字沒有底線 | 缺 display attribute provider 一整套 | [[架構決策與程式碼導覽]] |
| 插入點不跟著跑 | `Collapse` 不通知宿主；`GetRange` 需重拿 | [[架構決策與程式碼導覽]] |
| 候選視窗定位錯誤 | 靠 Win32 API 猜，應向 TSF 問 | [[架構決策與程式碼導覽]] |
| 點候選視窗後插入點消失 | 未處理 `WM_MOUSEACTIVATE` | [[架構決策與程式碼導覽]] |
