---
原編號: "2.52.9"
date: 2026-09-08
status: done
一句話: "532 測試全過但翻出三個洞：user_dir 在 Mac 回 None、漏斗沒詞庫時安靜吐假數字、spike_partition 一直空跑"
pitfall: true
相關:
  - "[[spike 1：在 Windows 上假裝是 Mac 編譯]]"
  - "[[詞庫就位後的完整量測：引擎跟 Windows 逐節相同]]"
---

# spike 1 在真的 Mac 上重跑：core 原生過，但翻出三個靜默的洞

[[spike 1：在 Windows 上假裝是 Mac 編譯]] 是**在 Windows 上用 `--target aarch64-apple-darwin` 假裝**編一次。
這回是在真機上原生跑：MacBook（arm64）／macOS 26.6.2／rustup 裝的
rustc 1.98.1。**詞庫原始檔刻意不裝**（`data/` 只有 priority.txt），
先看「不靠資料的部分站不站得住」。

**結果：core 的『平台無關』不是口號，是量過的**——這句在真機上依然成立。

| 檢查 | 結果 |
|---|---|
| `cargo check -p ime-core --all-targets` | 過，零警告 |
| `cargo test -p ime-core` | **532 過／0 失敗**，而且完全沒有詞庫 |
| `--features tui`（`try_ime`） | 過 |
| `cargo clippy --all-targets`／`cargo fmt --check` | 過（1 個既有的 `large_enum_variant`，Windows 也有） |
| `cargo build --release -p ime-core` | 過，28 支工具全是 `Mach-O 64-bit executable arm64` |
| 符號稽核 `check_symbols` **實跑** | ✓ 沒問題（2051 名字／5457 符號） |
| `--no-default-features` | 失敗在 `config`——**跟 [[spike 1：在 Windows 上假裝是 Mac 編譯]] 一模一樣**，既有破洞、非平台問題 |
| `cargo check -p ime-settings` | 失敗在 `windows-future` crate 本身——預期中 |

`cargo check` 只證明編得過，**`cargo build --release` ＋ 實際執行才證明連結
得起來、跑得動**——這是這輪比 spike 1 多拿到的東西。

**翻出來的三個洞**（都不是 macOS 才有的，只是在 Mac 上才會露出來）：

**1. `config::user_dir` 在 Mac 上一定回 `None`**。[[專案配置怎麼改：多一個 macOS 執行檔 crate，ad-hoc 簽章夠自己用]] 早就寫了要改，
這次是看到它實際的症狀：`std::env::var_os("APPDATA")?` 在 macOS 沒有這個
變數，整支回 `None`，設定檔、領域包、學習層**全部讀不到**——而且編得過、
532 個測試也全過，不會出任何錯。已修：拆出 `base_dir` 三個 cfg 分支
（Windows `%APPDATA%`／macOS `~/Library/Application Support`／其餘走 XDG），
搬家與 `OLD_APP_DIR` 只在 Windows 編進來。

- 拆出 `base_dir` **是為了能測**：`user_dir` 在 Windows 上會順手觸發資料夾
  搬家，測試不該去動使用者真正的 `%APPDATA%`
- macOS 不用 `~/Library/Preferences`：那裡歸 `NSUserDefaults`（plist）管，
  手寫 toml 進去會跟系統打架
- Linux 那支只是讓 core 在別的平台編得過，Phase 6 再確認位置對不對
- 新增測試節 `config::tests::使用者資料夾`——**它就是這個洞的守門員**

**2. 漏斗在沒有詞庫時安靜地吐出一張看起來完全正常的表**。這個比第 1 點
更危險。第一次在 Mac 上跑 `check_all`，它跑完 1421 句、印出完整表格、
exit code 0，**全程一句警告都沒有**：

```
  總計   1421 │  0  184  917  142 │  178  12.5%  +87 │ 8.87  0  125.6%
```

真實值是 90.1%。而且 `japanese_verbs` 87.8%、`symbol` 82.0% 還接近正常
（那兩節多半不靠下載來的詞庫），更容易讓人以為資料齊了。`check_freeze`
同病。**`check_lm` 一直都做對了**（找不到 `.gram` 就 `exit(1)`），所以修
法就是把它的規矩收成共用的 `testdata::require_dicts`，套到漏斗與凍結安全，
並列出缺哪幾份、怎麼補。原則：**寧可不出數字，也不要出假數字。**

- 問 bigram 用 `lm::get()` 不用 `lm::load()`：`char_freq_map` 沒有記憶化，
  每叫一次就重讀一次 `char_freq.txt`，拿它只為了問一句「載了沒」太貴
- `bench_dict`／`bench_typing`／`bench_learn` 還沒套——它們的輸出裡看得到
  「0 條」「句數 0」，沒有前兩支那麼會騙人，但同一類問題

**3. `spike_partition` 從測資合併那天起就一直是空跑**。commit `49181d2`
把十幾個 `mixed_*.txt` 併成一份 `測資.txt`、改用共用載入器時漏了這一支，
它預設讀的還是舊檔名，讀不到任何檔卻照樣印出「句數 0」的完整掃描表、
exit code 0。**這正是 CLAUDE.md「不要再各自寫死檔名清單」那條在講的事**，
規則寫了，但漏網的那支沒人回頭檢查。

- 已讓它讀不到就 `exit(1)`。真正的修法是接 `testdata::load`，但
  `rank_mode` 也自己讀檔、還要期望欄，改它等於動到 §2.51 已經記錄的量測
  口徑——**照 CLAUDE.md 要先提案，還沒做**

**還沒解決、留給下一輪的**：

| 項目 | 現況 |
|---|---|
| 根目錄的 `cargo test` 在 Mac 上跑不動 | workspace 含 `platform/windows` 與 `settings`，兩者都要 `windows` crate。**不能把 `default-members` 縮成 `core`**——那會在 Windows 上默默少跑 140 個測試（platform/windows 125、settings 15）。Mac 上一律用 `cargo test -p ime-core` |
| `settings` 的 Mac 化 | 卡在 [[macOS 移植的六項待決：四項裁決，不做語言鎖定一天就推翻]] 待決 2（`rfd` 還是手刻），沒動 |
| 詞庫資料 | 這台完全沒有。`download.ps1` 要 pwsh，而 Mac 上沒裝——**待決 5 現在是活的問題**，`require_dicts` 的提示訊息目前寫的是「Mac 要先裝 pwsh 7」 |

**反向驗證**：裝了 `x86_64-pc-windows-msvc` 目標，每個 commit 都跑一次
`cargo check -p ime-core --all-targets --target x86_64-pc-windows-msvc`，
確認在 Mac 上改的東西沒把 Windows 弄壞（含 `cfg(windows)` 那半的搬家程式
與測試）。**這是 [[spike 1：在 Windows 上假裝是 Mac 編譯]] 的鏡像，兩邊互相守著。**
