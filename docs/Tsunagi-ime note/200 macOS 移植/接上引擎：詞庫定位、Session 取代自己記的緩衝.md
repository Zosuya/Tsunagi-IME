---
原編號: "2.52.27"
date: 2026-09-08
status: done
一句話: "preload 要在背景執行緒（實測 645ms，宿主只等 3 秒）；組字區顯示轉換後結果而非按鍵，所以 macOS 不做預覽列"
相關:
  - "[[系統不再自動拉起輸入法：是工作階段內的累積效應]]"
---

# 接上引擎：詞庫定位、Session 取代自己記的緩衝

Echo 從此不再是 Echo——打 `su3cl3` 出「你好」。

### 入口只有一個：`ime_core::preload`

找了一圈才確認 core 的門面就是這支：

```rust
ime_core::preload(data_dir, config::Engines::default())
```

叫完之後詞庫進 core 的全域快取（`OnceLock`），**任何執行緒之後建的
`Session` 都看得到**。平台層不必自己管詞庫物件的生命週期。

**要在背景執行緒載**：`preload` 讀幾十 MB，缺 `dict_ja.bin` 時還要從文字
重建，這台實測 **645ms**。放主執行緒會拖慢輸入法啟動，而
[[系統不再自動拉起輸入法：是工作階段內的累積效應]] 的 log 裡看得到**宿主只等 3 秒**，拖過頭就變成
「選得到卻打不出字」。Windows 那邊也是背景載（`background.rs`）。

### 詞庫在哪：`paths::data_dir()`

對應 Windows 的 `registration::shipped_dir()`，但規則不一樣——那邊是
「先看 DLL 旁邊、再往上兩層找專案根」，而 macOS 的 `.app` 裝在
`~/Library/Input Methods/`，往上找不到東西。

| 順序 | 位置 | 用途 |
|---|---|---|
| 1 | `.app/Contents/Resources/data` | **正式安裝**（[[專案配置怎麼改：多一個 macOS 執行檔 crate，ad-hoc 簽章夠自己用]] 定的佈局） |
| 2 | `.app/Contents/Resources/data-dir.txt` 裡寫的路徑 | **開發用** |

詞庫 146MB，每次建置都複製進 bundle 太慢、改詞庫還要重裝，所以開發時寫一個
指路檔指回專案的 `data/`。**路徑由 `build-app.sh` 用 `$ROOT` 算出來**，
不寫死（CLAUDE.md 的跨電腦開發注意事項）。**正式包不該有那個檔。**

執行期真正會讀的是 11 個檔（`bopomofo/` 8 個、`english/en_50k.txt`、
`japanese/connection.bin`、`japanese/dict_ja.bin`），不是整個 146MB——
其餘是產生用的原料（`naer_wordfreq.xlsx` 一個就 16.5MB）。**打包時只要那
11 個**，這是之後做安裝包要用的數字。

### 組字區顯示什麼：兩個平台刻意不一致

這是這一節唯一需要拍板的設計決定。

`Session::composition_text()` 在**自動模式**下回的是**原始按鍵**
（`su3cl3`），因為 Windows 版的設計是「組字區顯示按鍵、**另一條獨立的
預覽列**顯示會送出什麼」——那條預覽列為什麼是獨立視窗，見
`platform/windows/src/preview_window.rs` 開頭（兩者寬度不相干，塞同一個
視窗會留一大塊白，而系統陰影依視窗矩形畫，靠「不畫」解不掉）。

**macOS 不這樣做。** 每個原生輸入法都是組字區直接顯示轉換後的結果——
日文輸入法打 `nihongo` 當場顯示假名，不是顯示羅馬字。所以這裡用跟
「鎖定語言」時同一條公式：

```rust
session.text() + session.pending_symbols()
```

實測（直接問引擎，不靠人工）：

```text
su3cl3 → 你好      su3c → 你c      hello → hello
```

「已轉換的部分 ＋ 還沒湊成一個單位的殘留按鍵」。**所以 macOS 版不做預覽列**
——那條資訊已經在組字區裡了。

> 這是兩個平台**刻意不一致**的一處，理由是遵循各自的慣例，不是漏做。
> 寫在 `composition()` 的註解裡，免得日後有人「順手對齊」。

### 其他

- **`Session` 放 ivar**：每個宿主連線一份（[[組字緩衝從 thread_local 換成 ivar]] 的理由，這下更重要了
  ——`Session` 是有狀態的，共用會串味）
- **點候選字走 `pick_char()`**，不是自己把字串挑出來送出——選了哪個字要
  進學習層（`learn_on_commit`），繞過去就學不到

### 還沒接的：選字模式

`char_candidates()` 是**選字模式**下「目前那個字」的候選清單，打字模式下
是空的，所以**候選面板現在不會出現**——那不是 bug。CLAUDE.md 講的三個
狀態機（mod／cutting／select）只接了打字那一段。

要接的是 `enter_select_first()`／`open_cands()`／`next_cand()`／
`confirm_cand()` 那組，還要對 `keymap::DEFAULT_BINDINGS` 決定哪些鍵進選字
——而 macOS 的鍵位空間跟 Windows 不一樣（[[spike 5：快捷鍵攔截]]：`Cmd` 系被宿主與系統
佔滿、`Option` 系是打特殊字元的地盤，剩下的空位只有 `F1`～`F12` 那一類）。
