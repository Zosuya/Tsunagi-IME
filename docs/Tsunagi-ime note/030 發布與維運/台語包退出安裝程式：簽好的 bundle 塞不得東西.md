---
原編號: "2.79"
date: 2026-09-01
status: done
一句話: 選裝元件往簽好的 .app 塞檔案會破壞資源封印；台語包改成從 repo 下載，兩平台一致
pitfall: true
---

# 台語包退出安裝程式：簽好的 bundle 塞不得東西

**起因是一個問句**：「安裝包會有問題嘛」。查下去翻出一個**現在看不出來、
簽章之後才會炸**的東西。

### 2.79.1 病因：選裝元件是在簽章之後往 bundle 裡塞檔案

`build-release.sh` 的順序是「組 bundle → 簽章 → 做台語元件」，而台語元件
的 payload 路徑是 `…/Tsunagi.app/Contents/Resources/packs/台語.txt`
——**安裝時往已經簽好的 bundle 裡丟檔案**。實測：

```text
原樣驗簽                → 過
事後塞一個包進去再驗    → a sealed resource is missing or invalid
```

`Contents/Resources` 是被簽章封存的（`CodeResources`），多一個檔案就破封。

**為什麼一直沒察覺**：現在全是 ad-hoc 簽章，而 pkg 裝出來的檔案不帶
quarantine，執行時只核主執行檔的 cdhash，不會去驗資源封印。§2.52.47
那次實測當然是好的。**等 [[Code signing：第一版先不簽，公開後再申請開源免費簽章]] 拿到開源簽章、做了公證之後才會炸**，
而且只炸在「有勾台語」的人身上——同一個版本兩種簽章狀態，這種最難查。

### 2.79.2 三條路與量出來的數字

| 路 | 做法 | 問題 |
|---|---|---|
| A | 台語搬到簽章**之前** | 簽章乾淨，但**沒有選裝這回事**了 |
| B | 裝到 bundle **外面**（`/Library/Application Support/…`） | 保住選裝，但 `core` 的 `dirs()` 要加第三個搜尋位置 |
| C | **不進安裝程式**，從 repo 另外下載 | 要有地方指路，不然沒人知道它存在 |

當初讓它可選的三個理由，量完只剩一個半：

```text
台語.txt          1.6 MB
詞庫（一定要帶）   76 MB
程式本體          9.1 MB
                 ─────
台語佔整包          1.9%
```

- **「它很大」站不住**：條數多（9 萬），但佔整包 1.9%
- **「授權要讓使用者明確選」**：仍然成立，但**裝進去 ≠ 啟用**——預設設定
  只啟用內建符號與 emoji，台語得自己去設定頁勾，勾的那一刻就是表態
- **「跟 Windows 語意一致」**：兩邊一起改就一致

### 2.79.3 選 C：它本來就是 §2.49 決定好的路

§2.49 早就寫過
擴充包怎麼走：「使用者下載 `.txt` 丟進 `packs\`，設定頁按重新整理。
**這條路今天就通了，不必寫任何程式。**」台語是擴充包，**反而是那個決定
的例外**。改成 C 之後三件事一起解掉：

- 簽章封印不再被破（bundle 裡不多任何東西）
- CC BY-SA 4.0 的資料不跟 GPL-3 的安裝包綁在一起（§2.49.1 的第一個理由）
- 詞表更新不必跟著發一版程式

**不必為它另開 repo**：`packs/台語.txt` 本來就進版控，公開快照也帶著它，
下載連結指 repo 檔案即可。包多起來再搬進 §2.49 說的 `tsunagi-packs`，
對使用者的流程完全一樣。

**代價只有一個，但是真的：沒人知道它存在。** 這條路成立的前提是有地方
指路——README 改好了，Release 說明與設定頁的連結**還沒做**（等 repo
網址定案）。

### 2.79.4 改了哪些、怎麼驗的

| 檔案 | 改動 |
|---|---|
| `platform/macos/build-release.sh` | 刪掉台語元件（`root-taigi`、`taigi.pkg`、`distribution.xml` 的 choice）；`customize` 從 `always` 變回 `never` |
| `installer/tsunagi.iss` | 刪掉 `taigi` 元件、它的語系訊息與 `[Files]` 那一條 |
| `installer/build-installer.ps1` | 打包清單拿掉 `packs\台語.txt` |
| `README.md`、`tools/取台語資料.md`、`相容性測試清單_macOS.md` | 敘述改成「從 repo 下載」 |

**驗收是真的建一次包，不是只看程式碼**：

```text
pkgutil --expand  → 只有 component.pkg，沒有 taigi.pkg
Distribution      → customize="never"，只有一個 choice
bundle 裡的 packs → 內建emoji.txt、內建符號.txt（沒有台語）
codesign --verify --deep --strict  → 過
```

**內建符號與 emoji 不動**：CC0、加起來不到 300KB，而且 `\星\` 得開箱即用。

### 2.79.5 Windows 那邊：沒這個病，是為了兩邊一致才一起拿掉

**Windows 不會踩到 §2.79.1**。Authenticode 簽的是**單一 PE 檔**（DLL、
EXE），不是資料夾——往 `{app}\packs` 丟一個 `.txt` 不會讓任何簽章失效，
沒有「資源封印」這種東西。所以 Windows 那邊拿掉台語元件**不是被迫的，
是為了兩邊語意一致**：同一份 README 要能同時講清楚兩個平台，而「macOS
要自己下載、Windows 安裝時可以勾」只會讓人以為自己記錯了。

反過來說，**想在 Windows 保留選裝在技術上完全可行**，代價只有兩邊分岔。

改了三處（已列在 §2.79.4）：`taigi` 元件、它的三行語系訊息、`[Files]`
那一條，以及 `build-installer.ps1` 的打包清單。

#### 這台（Mac）驗得到什麼、驗不到什麼

**驗不到的是真正的驗收**：ISCC 只跑在 Windows。Mac 上只能做靜態一致性
檢查，做過的是這三條：

```text
殘留的 taigi 參照（含 .github）  → 0（CI 目前也還沒接）
[Files] 每一條的 Components: main → 仍然存在（12 條）
[Types] 與 [Components]           → 兩段都在，仍然配套
```

第三條是重點：那兩段**一定要嘛都在、要嘛都不在**。只留一半的話每個元件
都不屬於任何安裝類型，**預設全部沒選中，按下一步一個檔案都不會裝**
——症狀是安裝到一半跳「register_tool.exe 找不到」，這個坑踩過兩次。

**Windows 上要補的驗收**：跑一次 `installer\build-installer.ps1`，裝起來
確認 `{app}\packs` 底下只有兩個內建包、輸入法能註冊、設定頁打得開。

#### 留下的一個選擇：單一元件還要不要那一頁

現在只剩一個元件而且是 `fixed`，但 `[Components]` 只要存在，Inno 就會顯示
「選擇元件」那一頁——上面一個勾不掉的項目，使用者多按一次下一步。

要拿掉那一頁，得把 `[Types]`、`[Components]` 與**每一條** `Components: main`
一起刪（沒有元件時 Inno 一律全裝）。**半套就是上面那個坑**，所以在拿到
Windows 機器實測之前先不動。

#### 升級的邊角：舊版裝過的人，包不會消失

Inno 不會刪「不在 `[Files]` 裡」的檔案，pkg 也只覆蓋自己帶的那些。所以
先前用舊安裝程式勾過台語的人，升級後 `{app}\packs\台語.txt`（macOS 是
bundle 裡那份）**仍然在，也仍然有作用**。

Windows 那邊這是好事，沒有副作用。**macOS 那邊代表那些人的簽章封印仍然
是破的**——真的要發簽章版時得在 Release 說明寫「先整個移除再裝」。目前
只有開發機裝過，實務影響是零，但別忘了。
