---
原編號: "2.52.4"
date: 2026-09-06
status: done
一句話: "逐項對照表：setMarkedText 的底線是 attribute 自帶、密碼欄免做、註冊不用提權也沒有註冊表"
---

# TSF → IMK 對照表

| Windows 現況 | macOS 對應 | 備註 |
|---|---|---|
| 組字區 `SetText` ＋ display attribute 底線 | `setMarkedText:selectionRange:replacementRange:` 帶 attributed string | 底線是 attribute 的一部分，不必另外註冊 provider |
| 送出／取消（`EndComposition` 不刪字，§3） | `insertText:replacementRange:`／空字串的 `setMarkedText` | 取消一樣要主動清空 |
| 候選視窗定位 `GetTextExt`（`TS_E_NOLAYOUT` 退路，§2.38） | `attributesForCharacterIndex:lineHeightRectangle:` | 部分 App 回 0 矩形，**要同樣的退路**（spike 4） |
| 自繪候選視窗 D2D | 自繪非啟用式 `NSPanel` ＋ CoreText | 或沿用設定頁的 egui 預覽（spike 3） |
| 工作列指示器＋自繪右鍵選單（§3.6／§3.7） | IMK `menu` 回傳選單、圖示由 bundle `tsInputModeListKey` 宣告 | **比 Win11 簡單很多**，900 行自繪選單不用搬 |
| 密碼欄兩條訊號（§2.12） | 免做——安全欄位系統根本不呼叫輸入法 | |
| 註冊寫 HKLM、要提權（§3.2） | 複製到 `~/Library/Input Methods/`，登出登入 | 沒有註冊表、不用管理員 |
| 內嵌字型 `AddFontMemResourceEx`（§4.11） | `CTFontManagerRegisterGraphicsFont` | 一樣只有本行程看得到 |
| `keyprobe` 量快捷鍵攔截（§2.14） | 要有 Mac 版重量一次 | spike 5 |
