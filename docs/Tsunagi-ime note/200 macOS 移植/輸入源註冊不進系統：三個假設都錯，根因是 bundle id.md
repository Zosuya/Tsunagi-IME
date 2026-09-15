---
原編號: "2.52.13"
date: 2026-09-08
status: done
一句話: "簽章、plist、要不要登入三個假設全錯；根因是 bundle id 不含 .inputmethod. 被掃描器直接跳過"
pitfall: true
相關:
  - "[[專案配置怎麼改：多一個 macOS 執行檔 crate，ad-hoc 簽章夠自己用]]"
  - "[[macOS 移植的六項待決：四項裁決，不做語言鎖定一天就推翻]]"
  - "[[啟用那關：plist 沒對齊 Apple]]"
---

# 輸入源註冊不進系統：三個假設都錯，根因是 bundle id

Echo 輸入法（spike 2）起得來、`IMKServer` 開得成，卻**沒有被系統登記成
輸入源**——`TISRegisterInputSource` 一律回 noErr，輸入源總數永遠是 318。
依序試了三個假設：**macOS 26 靜默拒絕未公證的輸入法**（簽章問題）、
**Info.plist 有欄位寫錯**（照威注音逐項比對修了四個真錯）、
**登記要等下一次登入才生效**（三次受控的登出登入實驗，連對照組一起失敗）。
三個全錯。根因是 **bundle id 不含 `.inputmethod.`（前後都要點）的 bundle
會被掃描器直接跳過**——不讀 Info.plist、不留 log、照樣回 noErr。改成
`com.tsunagi.inputmethod.Tsunagi` 之後不登出不重開，總數立刻 318 → 320。

## 試過的路

### 一、spike 2 卡住：假設是簽章（原 2.52.13）

Echo 輸入法本身是好的——`.app` 起得來、`IMKServer` 開得成、run loop
在跑。**卡在更前面一關：那個輸入源根本沒被系統登記**，所以「鍵盤 →
輸入方式 → 編輯 → `+`」裡搜尋 `通譯` 或 `Tsunagi` 都是空的。

**量到的事實**（每一條都實測過，不是推測）：

| 檢查 | 結果 |
|---|---|
| `TISRegisterInputSource` | **回 0（noErr）**——宣稱成功 |
| `TISCreateInputSourceList` 依 bundle ID 查 | **NULL** |
| 同上依 `InputSourceID` 查 | **NULL** |
| 對照組：查 `com.apple.inputmethod.TCIM.Zhuyin` | **1 個**——過濾語法本身沒問題 |
| 列全部輸入源 | 318 個，**改任何東西前後都是 318** |
| 完整登出再登入一次 | **沒用**（`loginwindow` 10:52:53 重啟，之後仍是 318） |
| `lsregister -f` 單一 app／`-R -domain user` 整個目錄重掃 | 沒用 |
| 用 `open` 透過 LaunchServices 正式啟動一次 | 行程起來了，**還是沒用** |
| 踢掉 `TextInputMenuAgent` | 沒用 |
| `log show` 查 `com.tsunagi` / `Tsunagi.app` | **系統從頭到尾沒提過它一次** |
| `codesign -vvv --deep --strict` | `valid on disk`／`satisfies its Designated Requirement` |
| `spctl -a -t exec` | **rejected** |

**沿路修掉的兩個真的錯**（都不是根因，但都是錯的）：

1. **`tsInputModeListKey` 要包在 `ComponentInputModeDict` 底下**，不能放
   Info.plist 最外層。放錯位置時 `TISRegisterInputSource` **照樣回 0**，
   輸入源卻不會出現——靜悄悄的失敗。系統內建那幾支（`TCIM` 等）走特權
   路徑、Info.plist 裡沒有這段，**不能拿來當對照**
2. 缺 `CFBundleInfoDictionaryVersion`／`CFBundleDevelopmentRegion`／
   `CFBundleSignature`／`NSPrincipalClass` 與任何 `.lproj`。CFBundle
   少了這些可能把 bundle 當畸形的直接跳過

**最強的線索是「系統一句話都不說」**。連拒絕的 log 都沒有，代表那個
bundle 在被解析之前就被跳過了。配合 `spctl` 的 `rejected`，當時最合理的
假設是：

> **macOS 26 不讓沒有 Developer ID／未公證的輸入法登記，而且是靜默拒絕。**
> **（這個假設是錯的，見底下「根因」。）**

**如果這個假設成立，[[專案配置怎麼改：多一個 macOS 執行檔 crate，ad-hoc 簽章夠自己用]] 那句「自己用，ad-hoc 簽名免費就能裝」是錯的**
——那句是規劃階段查文件寫的，不是量出來的。這正是 spike 存在的意義。連帶
影響 [[macOS 移植的六項待決：四項裁決，不做語言鎖定一天就推翻]] 待決 4（發布範圍）：**如果連自己用都要 99 美元的
Developer ID，那個決定就不是「要不要發給別人」而是「做不做得下去」。**

**當時的岔路**：

1. **自簽憑證**：免費，但自簽一樣沒有 Team ID，能不能過關是未知數；而且
   要把憑證加進鑰匙圈並設為信任
2. **申請 Apple Developer ID**（99 美元／年）：能一次驗證假設並解掉待決 4
3. **先擱置 spike 2**：core 可攜性（[[詞庫就位後的完整量測：引擎跟 Windows 逐節相同]] 逐節相同）與繪製層
   （[[繪製層在 Mac 上量了一遍：1452 行已可攜]]）的結論不受影響，那兩塊已經站得住

**當時還沒被否證的其他可能**：`LSBackgroundOnly` 用 `<true/>` 而非
`<string>1</string>`、缺 `tsInputModeKeyEquivalentKey`（我只留了
modifiers 那半）、圖示格式（16×16 模板 TIFF）不合系統要求。這幾個都比
簽章假設弱，但都還沒逐一排除。

### 二、照著威注音的 Info.plist 對出四個錯，但根因是別的（原 2.52.14）

`git clone` 了威注音（McBopomofo）來逐項比對——**這是
[[macOS 移植的 spike 排序：先做 Echo 輸入法，五個全部結案]] 那個「spike 0：讀威注音作者那篇指南」的替代做法**，那篇 Medium
文章擋 403 讀不到，但原始碼比文章可靠。

**它的安裝流程跟我做的一模一樣**（`Source/Installer/AppDelegate.swift`）：
複製到 `~/Library/Input Methods/` → `TISRegisterInputSource` → 用
bundle id 找回來 → `TISEnableInputSource`。**沒有任何祕方。**

**逐項比對挖出四個我寫錯的地方**（每一個錯了系統都不吭聲）：

| 我原本 | 正確 | 為什麼 |
|---|---|---|
| `LSBackgroundOnly` | **`LSUIElement`** | `LSBackgroundOnly` 的 app 完全不能有 UI，而輸入法要畫候選視窗、要有選單列圖示。Apple 自己寫的是字串 `"1"` 不是 `<true/>` |
| repertoire 填 `zh-Hant`／`ja`／`en` | **`Hant`／`Han`／`Jpan`／`Latn`** | 要 **ISO 15924 書寫系統代碼**，不是 BCP-47 語言標籤 |
| `TISInputSourceID` 只放在模式字典裡 | **最外層也要有一份** | ~~模式字典的鍵本身就是模式 ID，裡面不再放這個~~ **2026-09-08 稍後更正：模式字典裡也要明寫**，不寫系統會自己拼一個疊兩層的，見底下「根因」 |
| `tsInputModeScriptKey` = `smUnicodeScript` | `smTradChinese` | 對齊 `TISIntendedLanguage` |

另外補了 `PkgInfo`、`CFBundleSignature`（給 `TSNG`，`????` 是「沒有」）、
`NSPrincipalClass`、`.lproj`。

**但全部改完，還是註冊不進去。** 於是做了兩個對照實驗：

**實驗一：拿 Apple 自己的輸入法來註冊。** 把
`/System/Library/Input Methods/AinuIM.app` 複製出來，只改 bundle id、
ad-hoc 重簽，裝進 `~/Library/Input Methods/` 再註冊——**一樣註冊不進去**。

> **當時的結論：不是我的 Info.plist 的問題，也不是我的 Rust 程式碼的問題。**
> （**對照組的 id 也不含 `.inputmethod.`**，所以兩邊一起失敗——見「根因」。）

**實驗二：讓輸入法自己註冊自己。** 原本是從一支命令列小工具呼叫
`TISRegisterInputSource`，懷疑那支 API 只認「已被系統驗證過的 app」發出的
請求（威注音是從它自己的安裝程式裡呼叫）。所以在 `echo_ime.rs` 加了
`register_self()`，開機時對自己的 bundle URL 呼叫一次——**回 noErr，
還是註冊不進去**。

`register_self` 留著沒拿掉：那本來就是對的設計（威注音的安裝程式也是這樣
做），而且日後有了正式簽章就會派上用場。

**當時站得住的推論**（**後來證明是錯的**）：`TISRegisterInputSource` 在這台機器上，對任何裝在
`~/Library/Input Methods/` 的東西都是**回 noErr 但什麼都不做**，登記要等
下一次登入才生效。前一節那次登出登入之所以沒用，是因為**當時 plist
還帶著上表那四個錯**——那次測試的是一個壞掉的 bundle，不算數。

**下一個測試已經佈好**（等一次登出登入）：`~/Library/Input Methods/` 底下
現在同時放著 `Tsunagi.app`（修好的）與 `CtrlIM.app`（Apple 的 AinuIM 改
bundle id ＋ ad-hoc 重簽）。登入後三種結果各自指向不同的根因：

| 登入後 | 代表 |
|---|---|
| 兩個都出現 | 就是「plist 要對 ＋ 要登入一次」，spike 2 可以往下做 |
| 只有 `CtrlIM` 出現 | 還是我的 bundle 有問題，但範圍縮到剩下幾個欄位 |
| **兩個都沒出現** | **ad-hoc 簽章不夠**——[[專案配置怎麼改：多一個 macOS 執行檔 crate，ad-hoc 簽章夠自己用]] 那句「自己用 ad-hoc 就能裝」是錯的，連帶把待決 4 從「要不要發給別人」變成「做不做得下去」 |

這個對照組是刻意留的，測完要記得把 `CtrlIM.app` 刪掉。

### 三、三次登出登入的對照實驗：ad-hoc 與自簽都註冊不進去（原 2.52.15）

前兩節的收尾。**當時的結論：spike 2 卡在「輸入源註冊不進系統」，
三次受控實驗都沒過，暫停在這裡等決定。**

> **同日稍後已解**：這節的實驗設計沒錯，只是每一次都在
> 測一個 id 不合格的 bundle——**包括對照組**（AinuIM 改掉的 id 也不含
> `.inputmethod.`），所以怎麼測都一樣。

**實驗設計**：`~/Library/Input Methods/` 底下同時放兩個東西——我們的
`Tsunagi.app`，與對照組 `CtrlIM.app`（把 Apple 自己的
`/System/Library/Input Methods/AinuIM.app` 複製出來、只改 bundle id
再重簽）。每次登出登入後用 `TISCreateInputSourceList` 依 bundle id 查。

| 登入時刻 | 當時的狀態 | Tsunagi | 對照組 |
|---|---|---|---|
| 10:52 | plist 有前一節那四個錯，ad-hoc | ✗ | ✗ |
| 11:11 | plist 修好，**ad-hoc** | ✗ | ✗ |
| 11:21 | plist 修好，**自簽憑證**（非 ad-hoc） | ✗ | ✗ |

「全部輸入源」的總數**從頭到尾都是 318**，一次都沒變過。對照組查
`com.apple.inputmethod.TCIM.Zhuyin` 一律回 1，所以查詢方法本身沒問題。

**排除掉的**（每一條都實測，不是推測）：

- 不是 Info.plist——**Apple 自己寫的 bundle 換個 id 也一樣註冊不進去**
- 不是我的 Rust 程式碼——同上
- 不是「呼叫者是命令列工具」——在輸入法自己行程裡呼叫（`register_self`）
  也是回 `noErr` 但沒作用
- 不是快取——三次完整登出登入
- 不是 Launch Services——`lsregister -f` 與 `-R -domain user` 都做過
- 不是 MDM／描述檔限制——`/Library/Managed Preferences` 不存在，
  `profiles list` 是空的，SIP 正常
- 不是 ad-hoc 專屬問題——自簽憑證（`flags=0x0`、有真的簽章）一樣失敗

**系統從頭到尾沒有留下任何一句相關的 log**。唯一提到那兩個 app 的是
`GamePolicyAgent` 掃目錄時的 sandbox `deny(file-read-xattr)`，跟輸入法
註冊無關。連拒絕訊息都沒有。

**「必須由前景 GUI app 發起註冊」——已經試了，也不是**：

威注音的開發流程是「用 Xcode 建一個安裝程式 target、執行那個 .app 來
安裝」（`README.markdown`／`AGENTS.md`），不是命令列。所以做了
`platform/macos/src/installer.rs`：一個**前景**（不帶 `LSUIElement`）、
有視窗、用 `open` 從 Finder 啟動的一般 app，唯一的工作就是對輸入法
bundle 呼叫 `TISRegisterInputSource`。

**結果一樣：回 `noErr`，查不到。** 這條排除掉了。

**跟 Xcode 無關**——Xcode 只是產出 `.app`，`build-installer.sh` 自己就會
做。差異從來不在「用什麼建的」。它們的 `AGENTS.md` 另外寫著：*macOS 限制
單次登入工作階段內能砍掉輸入法行程的次數，安裝多次後會失效，要登出再
登入*，這條也記著。

**當時剩下的線索只有一條**：
**macOS 26 Tahoe 有一批已知的輸入法災情**（選單列的輸入法選單整個不見
等），社群的解法是 `defaults delete com.apple.HIToolbox; killall
SystemUIServer`。

支持這個假設的一個觀察：**「全部輸入源」的總數從頭到尾都是 318，一次都
沒動過**——不管裝什麼、註冊幾次、登入幾遍。連 Apple 自己的 bundle 都加
不進去。那比較像是**這台的輸入源資料庫卡住了**，而不是我們的 bundle 有
什麼問題。

**還沒試**，因為那會清掉使用者現有的輸入源設定。**但可以做成非破壞性的**：
先 `defaults export com.apple.HIToolbox <備份>`，測完不行就
`defaults import` 還原。

**沒有受影響的結論**：[[詞庫就位後的完整量測：引擎跟 Windows 逐節相同]]（core 逐節與 Windows 相同）與 [[繪製層在 Mac 上量了一遍：1452 行已可攜]]
（繪製層 1,452 行、52 個測試已可攜）都是獨立量到的，不因 spike 2 卡住
而失效。**「引擎搬得過去」是確定的；「輸入法在 macOS 上跑得起來」還沒
確定。**

**環境殘留**：登入鑰匙圈裡留了一張測試憑證 `Tsunagi Self Signed`
（`security delete-identity -c "Tsunagi Self Signed"` 可刪）；對照組
`CtrlIM.app` 已經刪掉。

## 根因：bundle id 必須含 .inputmethod（原 2.52.16）

**結論先講**：

> **macOS 掃 `~/Library/Input Methods/` 時，bundle id 不含 `.inputmethod.`
> （前後都要有點）的 bundle 會被直接跳過**——不讀 Info.plist、不留 log、
> `TISRegisterInputSource` 照樣回 noErr。我們的 id 是 `com.tsunagi.inputmethod`
> （結尾沒有點），所以怎麼註冊都是 318 個。

改成 `com.tsunagi.inputmethod.Tsunagi` 之後，同一台機器、同一份 ad-hoc
簽章、不登出不重開，系統立刻看到：輸入源表 67 → 69（輸入法本體＋一個
模式），總數 318 → **320**。**簽章從頭到尾不是問題**。

**這條規則不是我猜的**——所有活著的第三方輸入法 id 都含它：威注音
`org.openvanilla.inputmethod.McBopomofo`、Google 日文
`com.google.inputmethod.Japanese`、搜狗 `com.sogou.inputmethod.sogou`、
鼠鬚管 `im.rime.inputmethod.Squirrel`，Apple 自己的也全是
`com.apple.inputmethod.*`。前一節的對照組（AinuIM 改掉的 id）**也不含**，
所以三次實驗裡對照組跟我們一起失敗——實驗設計沒錯，只是兩邊都不合格。

### 怎麼找到的（方法比結論值錢，下次別的靜默失敗也用得上）

1. **輸入源清單是一份磁碟快取**：`/System/Library/Caches/com.apple.IntlDataCache.le.kbdx`
   （root 寫的，mtime 停在 9/1 22:08——那是 26.6.2 更新後的開機），每個
   行程另外在 `$(getconf DARWIN_USER_CACHE_DIR)/C/<bundle id>/` 留一份
   複本。**25 份複本 bytes 一模一樣，沒有一份含 tsunagi**——所以不是誰的
   快取沒更新，是掃描本身沒撿到。
2. **HIToolbox 有內建的追蹤**：`strings` dyld 共享快取（`.01` 那個 1.7GB 的檔）
   撈出一整組 `TIS cache rebuild log:` 訊息，以及開關 **`AppleTISTraceCacheRebuild`**。
   `defaults write -g AppleTISTraceCacheRebuild -bool true`（`com.apple.HIToolbox`
   也寫一份）之後，訊息走 os_log 的預設層級，`log show --predicate
   'eventMessage CONTAINS "TIS cache rebuild"'` 就看得到。看完記得刪。
3. **追蹤說出了沉默的原因**：

   ```text
   TISFileInterrogator begin EUID=501 iNumberOfFileInfo 2 updateSystemInputSources? 0
   TISFileInterrogator end OK … inputSourceTableCountSys=67, inputSourceTableCount=67
   ```

   使用者層的掃描**有跑**（兩個目錄），但**沒有任何一行
   `AddInputMethodAppInfoForCache … for …/Tsunagi.app`**——它連 Info.plist
   都沒去讀。而字串表裡掃描器旁邊躺著 `.inputmethod.`。
4. **改 id 驗證**：直接改已安裝那份的 `CFBundleIdentifier`、ad-hoc 重簽、
   再跑一次 `TISRegisterInputSource`，追蹤立刻出現
   `AddInputMethodAppInfoForCache - CFBundleCopyInfoDictionaryInDirectory SUCCESS for …/Tsunagi.app`，
   `inputSourceTableCount=69`。

### 順帶量到的機制（「要等下一次登入」的推論是錯的）

- **`TISRegisterInputSource` 會強制重掃**（追蹤裡的 `islcRebuildInputSourceCache … ForceUpdate`），
  目錄變動的 FSEvent 也會。**不用登出、不用重開機**。
- 使用者層掃描只掃兩個目錄，結果寫在 `DARWIN_USER_CACHE_DIR`；
  `/System/Library/Caches` 那份只有 root（開機／登入時的 `loginwindow`）
  會寫。一般使用者的行程寫不動它是正常的，不是壞掉。
- **註冊完還有第二關「啟用」**，當天也卡了一輪：`TISEnableInputSource`
  回 noErr、`AppleEnabledInputSources` 紋風不動，命令列與前景 GUI 都一樣。
  當時推論是「macOS 26 的安全閘，只能使用者自己在系統設定按」——**那是錯的**，
  真正的原因是 plist 還有兩個欄位沒對齊 Apple，見 [[啟用那關：plist 沒對齊 Apple]]。
- **模式在選單裡的顯示名稱，鍵是「模式字典的鍵」**，放在
  `<語言>.lproj/InfoPlist.strings`。沒有它的話 `kTISPropertyLocalizedName`
  回的是一長串 bundle id，選單上就長那樣。照 Apple 的 AinuIM 對出來的
  （它的 `InfoPlist.loctable` 裡 `"com.apple.AinuIM.Ainu" => "Ainu"`，
  用的是**字典鍵**而不是 `TISInputSourceID`）。`build-app.sh` 現在產
  en／zh-Hant／ja 三份。
- **每個行程各有一份快取副本**，重掃只更新自己那份；全新行程沒觸發重掃
  前拿到的可能是舊的 318。所以 `tisq` 的查詢要先 `register` 再查，
  不然會誤判「還是沒有」。
- **模式字典裡要明寫 `TISInputSourceID`**——前面第二節第 3 點「鍵本身就是
  模式 ID、裡面不再放」是錯的。沒寫的話系統自己拼：鍵
  `com.tsunagi.inputmethod.tsunagi` 被登記成
  `com.tsunagi.inputmethod.Tsunagi.tsunagi`，鍵改成 `…Tsunagi.Auto` 就變
  `…Tsunagi.Tsunagi.Auto`，多疊一層。Apple 的 AinuIM 模式字典裡就是明寫的
  （鍵 `com.apple.AinuIM.Ainu`、ID `com.apple.inputmethod.AinuIM.Ainu`）。
  現在鍵與 ID 都是 `com.tsunagi.inputmethod.Tsunagi.Auto`，connection
  name 跟著改成 `com.tsunagi.inputmethod.Tsunagi_Connection`。

### 被推翻的

| 哪裡 | 原本 | 現在 |
|---|---|---|
| 第一節 | 假設「macOS 26 靜默拒絕未公證的輸入法」 | **錯**。跟簽章無關 |
| 第二節 | 「登記要等下一次登入才生效」 | **錯**。`TISRegisterInputSource` 當場重掃 |
| [[專案配置怎麼改：多一個 macOS 執行檔 crate，ad-hoc 簽章夠自己用]] | 「自己用 ad-hoc 就能裝」存疑 | **成立** |
| [[macOS 移植的六項待決：四項裁決，不做語言鎖定一天就推翻]] 待決 4 | 可能變成「做不做得下去」 | 回到原意：只是「要不要發給別人」 |

### 工具與殘留

- `platform/macos/tools/tisq.swift`：`swiftc` 一行編，`list`／`register`／
  `enable` 三個動作，檔頭寫了追蹤開關的用法。**noErr 不算數，查回來才算**。
- `platform/macos/src/installer.rs` 留著（威注音的流程本來就是獨立安裝
  程式），它誕生時要排除的假設已經證偽，檔頭改過了。
- 環境：HIToolbox 偏好設定備份在 scratchpad（沒動到它，只是保險）；
  追蹤開關兩個鍵已刪；`~/Library/Input Methods/Tsunagi.app` 由
  `build-app.sh` 重裝覆蓋。

**接下來**是「啟用」那一關，見 [[啟用那關：plist 沒對齊 Apple]]。
