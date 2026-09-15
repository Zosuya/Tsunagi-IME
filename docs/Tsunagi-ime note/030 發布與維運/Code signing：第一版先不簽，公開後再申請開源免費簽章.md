---
原編號: "2.30"
date: 2026-09-01
status: planned
一句話: 第一版先不簽；等 repo 公開後申請 SignPath 開源免費簽章，安裝程式先照 CI 無人值守設計
相關:
  - "[[Phase 5 剩下的（發布前必做）]]"
---

# Code signing：第一版先不簽，公開後再申請開源免費簽章

**結論先講**：現在不買憑證，**等 repo 公開後申請開源專案的免費簽章**。
但安裝程式現在就要照「能在 CI 無人值守跑完」來設計，理由見下。

### 這件事不是技術問題，是信任摩擦

Phase 0 就驗過了（§3.5）：**未簽章的 TIP
在記事本、瀏覽器、Word 都能正常載入**。功能上不缺任何東西。

沒簽章付的代價是別的：

| 情境 | 後果 |
|---|---|
| 下載安裝程式 | SmartScreen 跳「不明的發行者」，要點兩層才裝得下去 |
| 防毒軟體 | 對未簽章、又會載進每個行程的 DLL 特別敏感，可能誤判 |
| 公司電腦 | 政策常直接擋未簽章元件 |
| 反作弊遊戲 | 對未簽章模組最敏感（相容性測試清單的 H11 因此標成可跳過） |

**輸入法在這件事上比一般軟體嚴重。** 它會被載進**每一個**程式的行程裡
——包括網銀分頁與密碼欄。使用者要把「我打的每一個字」交給它。這類軟體
不署名，等於要求別人相信一個沒有出處的東西。

### 選項（2026-09-02 查證）

| 方案 | 費用 | 適用 |
|---|---|---|
| **SignPath Foundation** | **免費** | 開源專案，OV 級，走他們的簽章管線 |
| Certum 開源方案 | 低價／免費 | 波蘭 CA，Authenticode |
| OSSign | 免費 | 開源專案 |
| 商業 OV | 年費 | **新憑證仍要累積 SmartScreen 信譽** |
| 商業 EV | 年費（明顯較貴） | ~~SmartScreen 立即信任~~ ⛔ **2026-09-15 查證：這條已經不成立**，見下 |
| Sigstore | 免費 | **不適用**——產不出 Windows 認得的 exe/dll 簽章 |

**商業 OV 的陷阱要記住**：買了不代表警告馬上消失，要等足夠多人下載、
微軟認得你之後才會安靜。對下載量小的個人專案，那個信譽**可能永遠累積
不起來**——花了錢，警告照跳。

2023 年之後私鑰必須存在硬體裡（USB token 或雲端簽署服務），所以商業方案
除了年費還有硬體成本，簽章也不再是本機跑一個指令那麼單純。

### 要不要 CI，取決於選哪個方案

**SignPath 是走管線簽的**——CI 建置完把產物送過去、簽好再回來。所以選它
就得有 CI，而且建置與打包必須能無人值守跑完。

**但那不是唯一的路。** Certum 那類是**給你憑證**（雲端 HSM），本機
`signtool` 就簽得了，完全不需要 CI。

所以先後順序是：**先決定簽章方案，才知道要不要 CI**，不是反過來。

**這件事現在不必決定。** `installer/build-installer.ps1` 本來就寫成不互動、
失敗回非零，哪天要接 CI 那 50 行 workflow 隨時能加——晚做沒有額外成本。

### 決定：第一版先不簽（2026-09-03）

先把東西放出去，看有沒有人要用，再投資在發布的工程上。

**觸發重新評估的條件**（任一發生就回來處理）：

- 有人回報防毒軟體把它擋掉
- 使用者數量到了「SmartScreen 那兩層點擊真的在勸退人」的程度
- 要進企業環境（那邊常有「未簽章元件一律擋」的政策）

在那之前：本機 `build-installer.ps1` 建好、直接給檔案或掛 GitHub
Releases，附一句 SmartScreen 的說明就夠了。

### 兩個前置條件

1. **授權還沒定**（repo 裡沒有 LICENSE 檔）。這是申請的硬性前提，而且
   **公開之前定最省事**——有人開始用之後再改授權會很麻煩。使用者裁決。
2. **CI 還沒有**（`.github/workflows` 是空的）。不急，但跟安裝程式一起做
   最順。

### 一個 2026 年的新規則

**從 2026-03-01 起，code signing 憑證的最長效期從 39 個月砍到 460 天**
（約 15 個月）。意思是簽章變成**每年都要重來一次**的例行維護，不是一次
搞定的事。免費方案同樣受影響，但反正不用錢。

### 什麼時候做

順序是**安裝程式 → 公開 repo → 申請 → 接上 CI**。現在不花錢也不卡進度；
真正的觸發條件是「repo 公開」，不是「發布」——申請要幾天到幾週，早點送
比較不會卡在最後一哩。

---

## 2026-09-15 重新查證：三條更正、一個新障礙

發 1.0 前重查一次（實際打開官方文件讀，不看搜尋摘要）。

### ⛔ 更正一：EV 憑證**已經不再**讓 SmartScreen 免警告

上面那張表的「EV → SmartScreen 立即信任」**是錯的**，微軟自己的文件
（2026-08 更新）逐字寫著：

> **EV certificates no longer bypass SmartScreen.** …this behavior no
> longer exists… Paying a premium for EV solely to avoid SmartScreen
> warnings is **no longer justified**.

根因是 Trusted Root Program 的規定：**2024 年 8 月起所有 EV Code
Signing OID 從根憑證移除，所有簽章憑證一視同仁**。

**所以不要為了 SmartScreen 買 EV。** 那是這篇初稿寫的時候還成立、現在
已經消失的差異。

### 更正二：簽章仍然值得，但理由換了

SmartScreen 那個對話框不再是主要理由。真正買得到的是三件事：

| 好處 | 對本專案的價值 |
|---|---|
| 警告框顯示**發行者名稱**而不是「不明的發行者」 | 中 |
| 信譽**跨版本累積**（同一個身分），不是每版重來 | 中 |
| **Smart App Control 不擋** | **高** |

最後一條對輸入法特別重要：Win11 的 Smart App Control「會封鎖未簽章
檔案執行」，而我們的 DLL 會被載進**每一個**行程。這比對話框重要得多。

**OV 的信譽仍然要靠下載量**（官方說法是「數週、數百次乾淨安裝」，
而且**沒有機制可以申請加速**）——對下載量小的個人專案幫助有限，但
**不是零**，因為上面那三條跟下載量無關。

### 更正三：憑證效期 460 天確認屬實，但不必擔心

CA/B Forum 官方文件確認：**2026-03-01 起最長 460 天**，OV 與 EV 都適用
（有些 CA 自己壓到 459 天當安全邊際）。

**但只要加時間戳記，已發布的二進位永遠有效**，不必重簽——微軟文件明載
「時間戳記讓 Authenticode 簽章在憑證過期後仍可驗證」。簽的時候記得帶
`/fd SHA256 /tr <RFC3161 URL> /td SHA256`。

### 🚧 新障礙：免費方案現在全部有問題

| 方案 | 2026-09-15 現況 |
|---|---|
| **SignPath Foundation** | ✅ 還在，GPL 明確可以。**但強制走 CI**（金鑰在他們的 HSM，本機拿不到） |
| **Certum 開源** | ✅ 可以買（**「缺貨」是誤判**，見下方的更正） |
| **OSSign** | ❌ **官網明載暫停收件**，而且要求「專案至少活躍 6 個月」 |
| **Azure Artifact Signing** | ❌ **台灣不在名單**（個人開發者限美國／加拿大；日韓也只收組織） |

### 🚧 跟「無歷史快照」政策直接衝突

SignPath 的條款有一句「常見誤解」：

> **we cannot sign binaries based on source code that nobody knows.**
> …we require a certain **verifiable reputation.**

**沒有任何規則要求完整 commit 歷史**（讀過整份條款，確實沒有），
所以 `tools/publish-snapshot.ps1` 的單一 commit repo **不是被規則擋掉**。

**但那是主觀審核。** 一個只有單一 initial commit、沒有 star、沒有
issue、沒有發布歷史的 repo，正好是「reviewer 看不到任何信譽證據」的
形狀。而且他們明說「沒有義務接受你的專案，也沒有獨立申訴機制」。

另外條款預設**有團隊**：每次簽章要由團隊成員核准（Author／Reviewer／
**Approver** 三種角色），對個人專案很彆扭。

**審核要多久查不到**——官網沒有任何 SLA，找到的開發者紀錄只寫到送出
表單為止。

### 這一輪的結論

**1.0 照發、不簽章**（跟 2026-09-03 的決定一致）。CI 先建起來鋪路——
`.github/workflows/` 已經有 `ci.yml` 與 `release.yml`，**簽章掛點都預留
並註解清楚**（Windows 兩個、macOS 一個）。

---

## ⛔ 更正上面那條：Certum「缺貨」是誤判，買得到（2026-09-15 當天）

上面那張表原本寫「Certum 三個開源品項全部缺貨，**已確認不是版面雜訊**
——只有開源品項有缺貨標記，Standard 與 EV 沒有」。**整條是錯的。**

**使用者用瀏覽器實際看過**：藍色的 **Add to Cart** 按鈕、**€49.00**，
頁面逐字寫「Using the SimplySign cloud solution **eliminates the need
to use a physical card and reader**」。

正確的網址是 **`certum.store`**（`shop.certum.eu` 已經 404）：

| 產品 | 網址 | 價格 | 要實體卡？ |
|---|---|---|---|
| **開源・雲端** ★ | `certum.store/open-source-code-signing-on-simplysign.html` | **€49** | **不用** |
| 開源・只買憑證 | `certum.store/open-source-code-signing-code.html` | $29 | 要 `cryptoCertum 3.6/3.7` |

### 為什麼會誤判成缺貨——這個坑要記住

那個 `Product is out of stock` 是 **Magento 模板的休眠元素**，JS 跑起來
之後才換成真正的按鈕。**抓網頁的工具只讀得到 JS 執行前的 HTML，所以
每一頁都看到它。**

判斷的關鍵證據是**去看一個確定在賣的商業產品**：$249 的 Standard 雲端版
**同樣顯示缺貨**。一家 CA 不可能所有商業產品同時缺貨——那就證明了它跟
庫存無關。

三個帶得走的判準：

- **抓網頁看到的不是使用者看到的。** 現在的購物網站幾乎都是 JS 渲染，
  靜態 HTML 裡的東西可能是模板骨架而不是實際狀態
- **要下「只有 X 有而 Y 沒有」的結論，必須真的去看 Y。** 第一次的錯誤
  正是宣稱比對過卻沒有
- **這種事最後要人用瀏覽器確認**，工具查到的只能當線索

（這是 §2.58.10 那條教訓的又一個實例——只是這次不是搜尋摘要，是
**渲染前的頁面**。）

### 順帶：Certum 自己的文案也過期了

他們的 EV 產品頁還寫著「immediately eliminating the Microsoft
SmartScreen Filter warning message」——**那個說法 2024-08 就不成立了**
（微軟移除 EV OID 的特殊待遇，見上方的更正一）。CA 的行銷文案沒跟上。
**不要拿 CA 的產品頁當 SmartScreen 行為的依據。**

## 付費管道（2026-09-15 查證）

### 各家對「台灣個人」的政策

| CA | 賣個人？ | 文件要求 | 價格 |
|---|---|---|---|
| **Certum 開源** | ✅「issued only for individuals」 | 身分驗證＋**水電帳單**＋公開專案網址 | **€49** 雲端／€69 含卡 |
| **SSL.com IV** | ✅「For individual developers／No business docs required」 | 護照或身分證正反面＋**手持證件自拍**，免公證免帳單 | **$129/年**（3 年期 $109.65/年） |
| Certum Standard | ✅ 個人免公司登記 | 身分驗證＋水電帳單 | €209 雲端 |
| Sectigo | ✅ | 未細載 | $536.25/年（5 年期） |
| DigiCert | ❌ **只收組織** | 下單必選 validated organization | ~$696/年 |

**Certum 開源有硬限制**：「distributed commercially → 憑證撤銷」，
而且憑證主體會冠上 `Open Source Developer` 前綴。GPL-3.0 的本專案符合
資格。

### ⛔ 台灣本地 CA 完全不用考慮

中華電信與 TWCA 的官網**根本沒有 code signing 這個商品**。更關鍵的是
微軟官方文件逐字寫著中華電信 ePKI 的兩張根憑證被
**NotBefore the Code Signing EKU**（2020-08，未見後續回復）——
**技術上就是死路**，不是價格問題。台灣代理商（寰宇）則明載要統編。

### 本機簽章：兩條路成立、一條有硬傷

這是最在意的一點（不想被迫改走 CI）：

| 方案 | 無人值守本機簽？ |
|---|---|
| **SSL.com CodeSignTool** | ✅ **官方明載**：`-totp_secret` 參數「allowing automated use」 |
| **SSL.com eSigner CKA** | ✅ **官方明載**：裝成虛擬 token，指令就是一般 `signtool /sha1 <thumbprint>`，自動模式「without the additional need for OTPs」 |
| **Certum SimplySign** | ⚠️ **官方文件查不到**無人值守的說明。網路上的自動化做法是個人部落格用 SendKeys 灌 OTP——**非權威來源** |
| DigiCert KeyLocker | 技術上支援，但沒資格買 |

**一個買之前要問清楚的未知數**：eSigner CKA 的官方文件**通篇以 EV 舉例**，
沒有明說 IV 憑證能不能搭 CKA。退路是 CodeSignTool（確定可自動化，
只是指令不是 `signtool`）。

**USB token 別買**：SSL.com 官方明載只寄到「officially-registered
address of your business or organization」，個人住址過不過**查不到**。
雲端方案直接繞過這件事。另外 eSigner 要**另外付費**（$20/月起，
前 30 天免費無限簽）。

### 兩個最實際的選項

**第一選擇：Certum 開源雲端 €49（約 NT$1,700）。** 最便宜、明文只賣
個人、GPL 專案完全符合，**2026-09-15 確認買得到**（Add to Cart 可按）。

剩下兩個未知數：

- **無人值守簽章沒有官方保證**——SimplySign 能不能讓
  `build-installer.ps1` 自動簽，官方文件查不到，網路上的做法是用
  SendKeys 灌 OTP（非權威）
- **水電帳單要求**：台灣的帳單是中文，波蘭 CA 接不接受**查不到**

**第二選擇：SSL.com IV $129/年。** 唯一**官方文件白紙黑字保證無人值守
本機簽章**的方案，驗證流程對台灣個人最友善（護照自拍，免公證、免帳單、
免公司登記），能直接寫進 `build-installer.ps1`。貴 2.6 倍買的是確定性。

**兩者都不必改走 CI**——這跟 SignPath 那條路的根本差別。

**查不到的**：付款方式與實際到件時間（各家結帳頁要登入才看得到）。
