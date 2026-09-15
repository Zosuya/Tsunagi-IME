---
原編號: "2.52.47"
date: 2026-09-09
status: done
一句話: "一定要 pkg 不能拖 .app（quarantine 會 translocate）；先產 .bin 讓載入 697ms→15ms；pkg 三個坑都會咬到使用者"
pitfall: true
相關:
  - "[[系統不再自動拉起輸入法：是工作階段內的累積效應]]"
  - "[[選單列圖示：量了四輪，前三輪有一半在測快取]]"
---

# macOS 的建置與發布流程

**兩支腳本，用途完全不同**，不要混用：

| | `build-app.sh` | `build-release.sh` |
|---|---|---|
| 給誰 | 開發者自己 | 使用者 |
| 詞庫 | `data-dir.txt` 指回專案 | **真的複製進 bundle**（76MB） |
| 簽章 | ad-hoc | Developer ID ＋ 公證 |
| 產出 | 直接裝進 `~/Library/Input Methods` | `target/release-pkg/*.pkg` |
| 何時跑 | 每次改完程式 | 要發版時 |

發布走**手動建置再上傳 Release**，跟 Windows 那邊同一個形狀（使用者的既有
習慣，不另外發明流程）。

### 一、為什麼一定要先產二進位詞庫

`gen_dict_ja` 跑 1.2 秒產出 `dict_ja.bin`（40MB）。差別是量出來的：

| | 缺 `dict_ja.bin` | 有 `dict_ja.bin` |
|---|---|---|
| 詞庫載入 | **697ms**（日文 674ms） | **15ms** |
| 要隨附的檔案 | 111MB 的原始文字 | 40MB 的 `.bin` |

**兩邊都贏**：啟動快 46 倍，包還更小。而且這不只是「比較快」——宿主等
輸入法連上線只等 3 秒（[[系統不再自動拉起輸入法：是工作階段內的累積效應]] 的 log 裡看得到那個逾時），主執行緒載不完
就變成「選得到卻打不出字」。

`connection.bin`、`dict_zh.bin` 同理，腳本會檢查缺哪個就產哪個。

### 二、只帶執行期真的會讀的檔

`data/` 全部是 **146MB**，但執行期只讀 **11 個檔、76MB**：

```
dict_ja.bin  dict_zh.bin  connection.bin  zh_bigram.gram  en_50k.txt
BPMFBase.txt  BPMFMappings.txt  word_freq.txt
char_freq.txt  char_freq_by_reading.txt  priority.txt
```

其餘（mozc 的十份原始詞典、教育部的 zip、NAER 的 xlsx）是拿來產二進位的，
使用者不需要。**這份清單要跟 core 實際開的檔名對得上**——漏一個的症狀是
「某個語言完全打不出字」，而且不會有錯誤訊息，所以腳本裡缺檔就直接 `exit 1`。

### 三、一定要 pkg，不能讓使用者拖 `.app`

**這是今天用 vChewing 實地踩出來的**（[[選單列圖示：量了四輪，前三輪有一半在測快取]]）：手動把 `.app` 拖進
`~/Library/Input Methods/` 之後，檔案帶著 `com.apple.quarantine`，macOS 會把
它 **translocate** 到 `/private/var/folders/…/AppTranslocation/…` 的唯讀路徑
執行——輸入源登記落不了地，怎麼弄都啟用不了。

而且 `xattr -dr` 救不了：對 `~/Library/Input Methods/` 裡的檔案下會被擋
（終端機沒有完整磁碟取用權），只有對 `~/Downloads` 裡的原始檔有效。

vChewing 的官方安裝指引因此是 pkg：

```bash
xattr -dr com.apple.quarantine ~/Downloads/xxx.pkg
installer -pkg ~/Downloads/xxx.pkg -target CurrentUserHomeDirectory
```

`-target CurrentUserHomeDirectory` 只裝使用者目錄，**不必 sudo**。我們照做。

### 四、postinstall 的順序

```sh
killall TextInputMenuAgent    # 先讓選單的 agent 放掉舊清單
sleep 1
open "$HOME/Library/Input Methods/Tsunagi.app"   # 再讓輸入法自己登記
```

順序抄 vChewing 的安裝程式：**先殺 agent、等一下、再登記**。反過來的話剛
登記好的會被 agent 手上那份舊清單蓋掉（[[選單列圖示：量了四輪，前三輪有一半在測快取]]）。輸入法啟動時會自己呼叫
`TISRegisterInputSource`（`echo_ime::register_self`），所以 `open` 一次就夠。

> **啟用還是要使用者自己來**：`TISEnableInputSource` 回 `noErr` 但永遠不
> 落地，那是 macOS 防側錄的設計（[[選單列圖示：量了四輪，前三輪有一半在測快取]]）。安裝完要在「系統設定 → 鍵盤 →
> 輸入方式」加入「通譯-Tsunagi」。

### 五、還沒做的：簽章與公證

現況 ad-hoc，`spctl -a -t exec` 判 **rejected**。

> ★ 先更正一個我自己抄錯的推論 ★
>
> 這一節原本寫「[[系統不再自動拉起輸入法：是工作階段內的累積效應]] 量到系統不會自動拉起輸入法，唯一的差別是
> Gatekeeper」。**那是錯的**——[[系統不再自動拉起輸入法：是工作階段內的累積效應]] 的結論是**工作階段內的累積退化**
> （反覆 `pkill` 十幾輪之後才失效，登出就重置），Gatekeeper 那條是中途被
> 否證掉的推論，那一節還特地留著當教訓，結果我又把它抄回來一次。
>
> **實測：ad-hoc 簽章的輸入法，在乾淨的工作階段裡系統照樣自動拉得起來**
> （`CoreServicesUIAgent: _LSLaunchRB → Successfully spawned tsunagi_ime`）。

所以**沒有簽章公證，功能是完整的**，代價只在「分發給別人」那一段：

| | 未簽章 | Developer ID ＋ 公證 |
|---|---|---|
| 自己用 | 完全正常 | 正常 |
| 別人下載安裝 | 要先 `xattr -dr com.apple.quarantine <pkg>`，或右鍵→開啟 | 雙擊就好 |
| 每次更新 | 都要再做一次 | 不必 |
| 觀感 | 「來自未識別的開發者」 | 正常 |

**這在生態裡有前例**：vChewing 官方同時發 signed 與 **`-unsigned.pkg`** 兩種，
安裝指引直接寫著那行 `xattr -dr`。所以先發未簽章版是站得住腳的，但
**一定要把那行指令寫進 Release 說明**——不然使用者雙擊被擋，多半直接放棄，
不會自己想到去下 `xattr`。

腳本已經留好插槽，憑證到手之後：

```bash
SIGN_ID="Developer ID Application: … (TEAMID)" INSTALLER_ID="Developer ID Installer: … (TEAMID)"     ./platform/macos/build-release.sh
xcrun notarytool submit <pkg> --keychain-profile <設定檔> --wait
xcrun stapler staple <pkg>
```

需要 Apple Developer Program（USD 99／年）。

### 六、架構：目前只有 arm64

`lipo -info` 顯示 `Non-fat file: arm64`。要支援 Intel 得建
`x86_64-apple-darwin` 再 `lipo -create` 合成 universal——**但沒有 Intel 測試
機**。CLAUDE.md 對 Windows ARM64 的原則是「**盲發比不發更糟**」，同一條原則
套過來：**先只發 Apple Silicon 並在說明裡寫清楚**。

### 三之一、pkg 踩到的三個坑（都會咬到使用者）

**① bundle relocation——安裝「成功」但檔案不在該在的地方**

`pkgbuild` 預設把 bundle 標成可重新定位。安裝時 macOS 會問 LaunchServices
「這個 bundle id 我認得嗎」，認得就**把安裝目標搬過去**：

```
PackageKit: Library/Input Methods/Tsunagi.app relocated to
    …/target/release-pkg/root/Library/Input Methods/Tsunagi.app
```

它找到的是**我們自己的暫存目錄**。收據寫了、`installer` 回成功，但
`~/Library/Input Methods/` 空空如也。使用者端一樣會踩：只要他電腦上任何
地方有過同一個 bundle id 的舊版（甚至只是下載資料夾裡沒刪的那份），新版
就會被塞進去。

兩道修正：`pkgbuild --component-plist` 把 `BundleIsRelocatable` 設成
`false`，以及打包完 `lsregister -u` 撤掉暫存那份的登記（`build-app.sh`
早就有這一步，`build-release.sh` 漏抄了才踩到，跟 [[系統不再自動拉起輸入法：是工作階段內的累積效應]] 同一個根因）。

**② 雙擊會裝到系統層**

`pkgbuild` 出來的元件包沒有安裝範圍宣告，圖形安裝程式預設裝到開機磁碟的
`/Library/Input Methods`——要管理員權限，而且**系統層與使用者層各一份會讓
同一個 bundle id 出現兩次**。用 `productbuild --distribution` 包一層並宣告：

```xml
<domains enable_anywhere="false" enable_currentUserHome="true" enable_localSystem="false"/>
```

限死成「只供我使用」，雙擊就會裝對地方。vChewing 的安裝指引特地提醒使用者
「要點更改安裝位置」，就是因為他們沒宣告；我們改成在包裡處理掉。

**③ 安裝完成頁不是裝飾**

macOS 不讓程式自己把輸入法加進清單（[[選單列圖示：量了四輪，前三輪有一半在測快取]]），所以最後一步一定要使用者
自己做——不講的話他會以為裝完就能用。而且**新裝的輸入源要重新登入才會出現
在清單裡**（實測：檔案到位、TIS 回報 `enabled=1`，但系統設定與選單列都看不
到，登出再登入就有了；跟 [[選單列圖示：量了四輪，前三輪有一半在測快取]] 的快取同一類）。

`<conclusion>` 那一頁因此寫了三件事：登出登入、去哪裡加、常用鍵位與怎麼移除。

### 三之二、台語擴充包做成選裝元件

跟 Windows 的 Inno Setup 對齊（`installer/tsunagi.iss` 的 `[Components]`），
但**預設值刻意不同**：

| | Windows | macOS |
|---|---|---|
| 主程式 | `Flags: fixed`（取消不掉） | `enabled="false"`（同義） |
| 台語包 | `Types: full`＝**預設會裝** | **預設不勾**（使用者裁定） |

理由：多數人用不到，而且它有自己的授權（CC BY-SA 4.0），讓使用者明確地選
才對。沒勾的人事後重跑同一個安裝程式勾起來即可——pkg 跟 Inno 一樣可重入。

`build-release.sh` 是「有 `packs/台語.txt` 就做成選項、沒有就跳過」。

### 三之三、解除安裝要讓一般人找得到

`uninstall.sh` 放在 `Tsunagi.app/Contents/Resources/`（跟 vChewing 一樣），
但**一般人不會去翻 bundle 內部**。Windows 走系統的「新增或移除程式」，
macOS 沒有那種東西，所以要自己給入口。

兩個參考實作的答案一致：**放進設定頁**。fcitx5-macos 放在「關於」分頁
（`src/config/about.swift`，含確認對話框），vChewing 則是主程式的
`uninstall` 子命令。我們照 fcitx5 的做法新增「關於」分頁。

> **順序有死結，差點做反**：設定頁的入口是輸入法自己（選單列的
> 「通譯設定…」、或打 `config` 按 ↑↑↓↓）。`uninstall.sh` 原本寫著「請**先**
> 到系統設定移除」——照做的話就再也開不了設定頁，那顆按鈕也按不到。
> 正確順序是**先移除檔案、再清系統設定那一筆**（殘影無害，下次登入也會
> 自己消失）。逃生口寫在安裝完成頁：設定頁可以直接執行，不需要輸入法在跑
> （實測驗過）。

「關於」分頁同時解掉另一個問題：**授權的姓名標示沒地方顯示**。詞庫來自十幾
個外部來源，CC BY-SA、CC BY-ND、國教院的開放資料政策都明文要求標示出處，
而 `CREDITS.md` 躺在安裝目錄裡不算真的讓使用者看得到。

### 三之四、`tsInputModeScriptKey` 改成 `smUnicode`：手寫面板不再自動附掛

實測回報「安裝完會多啟用一個繁體手寫」。那是
`com.apple.inputmethod.ChineseHandwriting`，型別 `TISTypeCharacterPalette`
——跟 Emoji 面板、日文假名面板同一類，**macOS 對這種面板有自動附掛的行為**：
加入一個中文輸入源時把對應的手寫面板一起打開。不是我們做的（出貨的程式碼
裡沒有 `TISEnableInputSource`），但源頭是我們宣告成 `smTradChinese`。

vChewing 用的是 `smUnicode`，而它照樣出現在繁體中文分類底下——**分類是
`TISIntendedLanguage` 決定的，不是 script**。而且這個輸入法本來就不只一種
文字系統（中／日／英同時判斷），宣告成單一繁體中文本來就不準確。

改成 `smUnicode` 之後**實測不再多出手寫面板**。所以那個附掛行為確實是
`tsInputModeScriptKey` 帶出來的，跟 `TISIntendedLanguage`（我們仍然宣告
`zh-Hant`）與 repertoire 無關——**分類靠語言，附掛靠 script，兩者是分開的**。

> 這條也是「抄 Apple 不一定對」的又一例（[[選單列圖示：量了四輪，前三輪有一半在測快取]] 是圖示，這裡是 script）。
> Apple 的 TCIM 七個模式全用 `smTradChinese`，因為它**就是**繁體中文輸入法，
> 手寫面板對它的使用者是合理的配套。我們照抄就把那個配套一起繼承了，而
> 對一套「中日英自動判斷」的輸入法來說那個宣告本來就不準確。

### 七、CI 能自動到哪裡

目前沒有任何 `.github/workflows`。發布這條**不必自動化**（手動建置再上傳，
跟 Windows 一致），但有一條值得加：

- **每次 push 跑檢查**：`cargo fmt --check`、`clippy`、`cargo test -p ime-core`、
  `cargo test -p ime-tip-macos`、**`cargo check --target x86_64-pc-windows-msvc`**。
  跑在 `macos-14` runner，十分鐘內完成，**不需要詞庫**（都是單元測試）。
  它擋得住這幾天最常見的錯：在 Mac 上改東西把 Windows 弄壞

發布 workflow 則卡在三件事，值得記下來免得日後重新評估：

1. **詞庫不進版控**：`data/download.ps1` 414 行、從十幾個來源抓 119MB，
   而且是 PowerShell。CI 要建產物就得跑它，還要 `actions/cache`，否則每次
   重抓又慢又會撞上游飄動（`en_50k` 已經有前科）
2. **公開走無歷史快照**（`tools/publish-snapshot.ps1`）：開發與發布是兩個
   repo，CI 要放哪邊得先決定
3. **簽章憑證要進 Secrets**：憑證匯出成 base64 ＋ App Store Connect API key
