---
原編號: "2.52.6"
date: 2026-09-06
status: done
一句話: "aarch64-apple-darwin 目標下 cargo check -p ime-core 全過，core 的「平台無關」是量過的"
相關:
  - "[[spike 1 在真的 Mac 上重跑：core 原生過，但翻出三個靜默的洞]]"
---

# spike 1：在 Windows 上假裝是 Mac 編譯

裝了 `aarch64-apple-darwin` 編譯目標（只檢查不連結，不需要 Apple SDK）：

| 檢查 | 結果 |
|---|---|
| `cargo check -p ime-core --all-targets --target aarch64-apple-darwin` | **過**（含測試與所有 bin） |
| 同上 `--features tui`（`try_ime`） | 過 |
| 同上 `--no-default-features` | 失敗——**但 Windows 目標也一樣失敗**，是既有的破洞（`config` 模組關掉後仍被引用），跟移植無關，另外處理 |
| `cargo check -p ime-settings --target aarch64-apple-darwin` | 失敗在 `windows` crate 本身（`windows-future` 在非 Windows 目標編不過）——**預期中**，用 target 區段隔開即可 |
| 暫存區最小 eframe 0.32 專案 `--target aarch64-apple-darwin` | **過**——設定頁的 Mac 化只剩那五個檔 |

結論：**core 的「平台無關」不是口號，是量過的**。
