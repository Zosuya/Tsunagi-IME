---
原編號: "2.52.1"
date: 2026-09-06
status: done
一句話: "core 只有兩處綁 Windows（user_dir、migrate_app_dir），要重寫的是平台層、D2D、設定頁五個檔"
---

# 移植可行性評估：core 只有兩處綁 Windows，要重寫三塊

- **`core/` 幾乎不用動**。整個 core 只有兩處綁 Windows：`config::user_dir`
  用 `%APPDATA%`、`migrate_app_dir` 的資料夾改名搬家。其餘全是純邏輯，
  `memmap2`／`toml` 都跨平台。
- **要重寫三塊**：平台整合層（TSF → IMK）、候選視窗繪圖（Direct2D →
  Cocoa）、設定頁裡叫系統對話框的五個檔。跟第二章開頭「架構分層」當初
  的預期一致。
- **真正的門檻是環境不是程式**：macOS 輸入法不能在 Windows 上建置或
  測試。Windows 上能做的只有「確認 core 在 Mac 目標下編得過」（就是這次
  的 spike 1）。
