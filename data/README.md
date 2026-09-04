# data/

Phase 1 三個語言引擎的詞庫原始檔。**這些檔案不進版控**（見根目錄
`.gitignore`），開發機需要各自執行 `download.ps1` 下載一次。

```powershell
.\data\download.ps1
```

## 來源與授權

詳細查證過程見[開發文件.md §2.3](../開發文件.md#23-詞庫與資料來源)。

| 目錄 | 來源 | 授權 | 格式 |
|---|---|---|---|
| `bopomofo/` | [McBopomofo](https://github.com/openvanilla/McBopomofo) `Source/Data/BPMFMappings.txt` | MIT | `詞 注音符號1 注音符號2 ...`（空白分隔，每字一組注音） |
| `bopomofo/` | [McBopomofo](https://github.com/openvanilla/McBopomofo) `Source/Data/BPMFBase.txt` | MIT | `字 注音符號 拼音 鍵位 編碼`（單字對照表，見下方說明） |
| `japanese/` | [mozc](https://github.com/google/mozc) `src/data/dictionary_oss/dictionary0*.txt` | BSD-3-Clause | `讀音\t左id\t右id\t詞頻\t表記` |
| `japanese/connection_single_column.txt` | mozc `src/data/dictionary_oss/` | BSD-3-Clause | 接續成本矩陣，單欄 2672×2672，`表[右id][左id]`；產品端尚未使用，給沙盒探索接續成本用 |
| `english/` | [hermitdave/FrequencyWords](https://github.com/hermitdave/FrequencyWords) `content/2018/en/en_50k.txt` | MIT | `word 頻率數字` |
| `bopomofo/char_freq.txt` | [教育部字頻總表](https://language.moe.gov.tw/001/Upload/files/SITE_CONTENT/M0001/PIN/biau1.htm)（`BIAU1.zip`） | 創用 CC 姓名標示-禁止改作 3.0 臺灣 | `字 頻次`（由 download.ps1 從 Big5 表格轉出） |

原計畫（見開發文件.md §2.3 修訂前版本）列的英文來源是 `wordfreq`，改用
`hermitdave/FrequencyWords` 是因為前者資料為 Python 專用的
`.msgpack.gz` 二進位格式、授權標記不明確；後者純文字、MIT，對 Rust
專案更直接。

`BPMFBase.txt` 是 2026-08-23 人工實測時追加的：`BPMFMappings.txt` 完全
不收單字條目（全部是 2 字以上的詞），導致打單一注音符號一律查無候選。
`BPMFBase.txt` 是同一個 repo、同樣 MIT 授權的單字對照表，補上這個缺口，
見[開發文件.md §2.1.1](../開發文件.md#phase-1--三個單語引擎估-46-週可並行)。

## 教育部字頻總表：為什麼要轉檔

`char_freq.txt` 是**唯一需要就地轉換**的來源，其他都是直接下載。

原始檔（`BIAU1.zip` 裡的 `BIAU1.TXT`）是 Big5 編碼的 ASCII 表格，
用 `│` 畫框線：

```
│     1  │ 的 │白│08│  32739 │   32739│  1.651 │
  序號     字  部首 筆畫  頻次    累積     百分比
```

`download.ps1` 的 `Convert-MoeCharFreq` 負責解 Big5、切欄位、抽出
「字 頻次」。解析率 5701/5731（99.5%），少掉的是 Big5 之外的罕見字
（頻次都 ≤49）與「○」這種非漢字。

**轉換在使用者的機器上做，這個 repo 不散布那份資料**——跟 mozc、
McBopomofo 詞庫同樣的模式（`data/` 已在 `.gitignore`）。

### 授權（2026-08-25 查證）

創用 CC 姓名標示-禁止改作 3.0 臺灣：**允許格式修改與後續使用，
只要不改資料內容**，散布時要註明出處。

**「禁止改作」不限制格式轉換**——這是教育部自己的解釋，由
[g0v/moedict-data](https://github.com/g0v/moedict-data) 引述：

> 依教育部之解釋，「創用CC-姓名標示-禁止改作 臺灣3.0版授權條款」之
> 改作限制標的為文字資料本身，不限制格式轉換及後續應用。

那個專案把整部《重編國語辭典》轉成 JSON 公開散布多年，是可靠的先例。
所以本專案只擷取「字」與「頻次」兩欄（丟掉部首、筆畫、累積頻次、
百分比）沒有問題。

剩下的義務是**散布時要標示出處**，詳見
[開發文件.md §4.32](../開發文件.md#432-待辦詞庫的安裝與散布給一般使用者用之前必須解決)。

### 為什麼不用既有的 zh_tw_50k

那份是**詞**頻不是**字**頻，而且來源是字幕語料，有兩個問題：

- 混了簡體（摆、参、担、临、谅、们）
- 單字條目的頻率嚴重低估常用字——「明」很少單獨出現（都在「明天」
  「明白」裡），所以 `螟(3003) > 名(1721) > 明(952)`，罕見字反而排前面

教育部這份以《國字標準字體表》為據，天然只收繁體，罕見字也不在表中。
