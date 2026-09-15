---
原編號: "2.52.5"
date: 2026-09-06
status: done
一句話: "多一個 platform/macos crate（執行檔非動態庫），core 的 user_dir 依平台推導，ad-hoc 簽章實測夠自己用"
相關:
  - "[[輸入源註冊不進系統：三個假設都錯，根因是 bundle id]]"
---

# 專案配置怎麼改：多一個 macOS 執行檔 crate，ad-hoc 簽章夠自己用

- **Workspace**：多一個 `platform/macos/` crate，是**執行檔**（IMK 輸入法
  是 `.app`，不是動態庫）。Apple 框架的依賴掛在
  `[target.'cfg(target_os = "macos")'.dependencies]`，Windows 上的
  `cargo test` 完全不受影響。綁定用 `objc2-input-method-kit`
  （0.3.2，2026-08 更新，含 `IMKServer`／`IMKInputController`／
  `IMKCandidates`；Zlib／Apache／MIT）
- **core 兩處**：`user_dir` 依平台推導（macOS 是
  `~/Library/Application Support/tsunagi-ime/`，資料夾名兩邊一樣，設定檔
  格式不變）；`migrate_app_dir` 只在 Windows 編進去
- **`settings/Cargo.toml`**：`windows` 依賴移到 target 區段，五個檔各給
  Mac 版（`NSFontPanel`／`NSOpenPanel`／CoreText；或引 `rfd`，待決）
- **資料與打包**：四個 `.bin`／`.txt` 放 `.app/Contents/Resources/data`，
  `registration::shipped_dir` 的「先看執行檔旁邊、再往上找」要懂 bundle
  布局。`download.ps1` 在 Mac 要裝 PowerShell 7 才能跑
- **簽章**：自己用，ad-hoc 簽名免費就能裝（2026-09-08 實測成立）；**發給別人**要 Apple
  Developer ID（每年 99 美元）＋公證，否則 Gatekeeper 直接擋。Phase 6
  原本寫的「需 Apple 開發者帳號」精確說是**發布才需要**。§2.30「第一版
  先不簽」在 Mac 這邊不成立，要重新看
  - ✅ 2026-09-08 一度存疑（ad-hoc 與自簽都註冊不進系統），當天查出
    根因是 bundle id 少了 `.inputmethod.`，**跟簽章無關**，
    見 [[輸入源註冊不進系統：三個假設都錯，根因是 bundle id]]
