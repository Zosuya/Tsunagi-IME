---
原編號: "2.52.12"
date: 2026-09-08
status: done
一句話: "objc2-input-method-kit 0.3.2 可用、Windows 目標零依賴；刻意留了 thread_local 與純 NSString 兩個簡化"
相關:
  - "[[spike 2 驗收通過：三種宿主都打得出字]]"
  - "[[組字緩衝從 thread_local 換成 ivar]]"
---

# spike 2：IMK Echo 輸入法

[[macOS 移植的 spike 排序：先做 Echo 輸入法，五個全部結案]] 排第一順位的那個，理由是「風險最高，不通則整份規劃作廢」。
程式碼在 `platform/macos/`（待決 6 選了放進本專案）。

**已經確認的**：

| 事項 | 結果 |
|---|---|
| `objc2-input-method-kit` 0.3.2 | 可用，`IMKServer`／`IMKInputController`／`IMKCandidates` 三個 feature 都在 |
| `define_class!` 造 `IMKInputController` 子類別 | 過（objc2 0.6.4） |
| `.app` bundle ＋ ad-hoc 簽名 | 過，`Mach-O thin (arm64)`、`Signature=adhoc` |
| Info.plist 的類別名對得上 runtime | 過——`[通譯] 控制器類別已註冊：TsunagiEchoController` |
| `IMKServer` 起得來、run loop 在跑 | 過 |
| **Windows 完全不受影響** | 過——`cargo check --target x86_64-pc-windows-msvc` 通過，且 `cargo tree` 在該目標下**零依賴** |

**還沒驗收的，就是 [[macOS 移植的 spike 排序：先做 Echo 輸入法，五個全部結案]] 列的那三件**：按鍵進得來、標記文字出得去、
送出文字進得了宿主。**第一次安裝要先登出再登入**，系統才看得到這個輸入源
（`defaults read com.apple.HIToolbox` 目前還找不到 tsunagi）。驗收要在
**TextEdit、瀏覽器、終端機三種都通**才算過——不是 TextEdit 通了就算。

**兩個刻意的簡化，接 core 前要回頭改**：

1. **組字緩衝用 `thread_local` 不用 ivar**。IMK 自己呼叫
   `initWithServer:delegate:client:` 造控制器，我們沒經手那個 init，
   `define_class!` 的 ivars 不會被初始化，碰它是未定義行為。**每個宿主
   連線各有一個控制器**，所以真的接 core 時一定要改成覆寫 init ＋ ivars
2. ~~**組字區傳的是純 `NSString`**，靠系統畫預設底線。真的要分「已轉換／
   未轉換」兩種底線時要換成帶 attribute 的 `NSAttributedString`~~
   **2026-09-08 已改，而且這條原本就寫錯了**：傳純 `NSString` **會弄當
   宿主**（Terminal 當場 `SIGTRAP`），不是「之後為了美觀再改」的事，
   見 [[spike 3：候選視窗用 NSPanel]]

**`catch_unwind` 一個都沒少**：unwinding 穿過 `extern "C"` 會 abort，
而 `define_class!` 展開出來的每個方法都是 ObjC runtime 直接呼叫的
`extern "C"`。`platform/macos/src/guard.rs` 跟 Windows 的 `guard.rs`
同一個規矩，目的從「保護宿主」變成「保住組字狀態」（[[執行模型的根本差異]]）。攔下來
之後一律回「沒處理」把按鍵讓回宿主——回「處理了」會讓按鍵無聲消失。
