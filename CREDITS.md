# 來源與致謝

通譯輸入法用到的外部資料與參考過的專案。列在這裡有兩個理由：**標示出處
是這些授權共同的義務**（CC 的 BY、國教院開放資料政策都明文要求），
以及讓使用者知道自己裝進電腦的東西從哪裡來。

## 詞庫資料

安裝包裡的詞庫是**編譯後的二進位檔**，由下列來源產生。原始資料本身不隨
安裝包散布（開發者用 `data/download.ps1` 自行下載）。

| 資料 | 來源 | 授權 |
|---|---|---|
| 注音映射表<br>`BPMFMappings.txt`／`BPMFBase.txt` | [McBopomofo](https://github.com/openvanilla/McBopomofo) | MIT |
| 日文辭典與接續矩陣<br>`dictionary_oss/`、`connection_single_column.txt` | [mozc](https://github.com/google/mozc) | BSD-3-Clause |
| 中英詞頻<br>`zh_tw_50k.txt`／`en_50k.txt` | [hermitdave/FrequencyWords](https://github.com/hermitdave/FrequencyWords) | MIT |
| 通用詞頻表 | [國家教育研究院](https://coct.naer.edu.tw/) | 開放資料（可改作、可再授權，須標示出處） |
| 常用字頻表 | [教育部](https://language.moe.gov.tw/) | CC BY-ND 3.0 TW |

**教育部字頻的說明**：該授權禁止散布改作版本。本專案**不散布這份資料**
——它只在開發階段參與 `dict_zh.bin` 的排序計算，原表不在安裝包裡，也
無法從二進位檔還原。二進位化本身屬於格式轉換，依 Creative Commons 的
說明不構成改作。

## 內嵌字型

全半形／語言模式提示視窗的六個標籤字（自半全ㄅあ英）用的是 subset 過的
內嵌字型，**跟著程式散布**：

| 檔案 | 來源 | 授權 |
|---|---|---|
| `platform/windows/res/symbols.ttf`（約 4KB） | Noto Sans TC | SIL Open Font License 1.1 |

只取那六個字的字形，字重定格在 400。依 OFL 的規定，修改版不得使用來源
的保留字型名稱（`Source`），因此更名為 `Tsunagi Symbols`。產生方式見
`platform/windows/res/make_symbol_font.py`。

## 參考過的專案

沒有使用它們的程式碼，但設計上受益良多：

| 專案 | 參考了什麼 |
|---|---|
| [libchewing](https://codeberg.org/chewing/libchewing) / [windows-chewing-tsf](https://codeberg.org/chewing/windows-chewing-tsf) | 分層字典的架構、學習曲線的取捨（以及該避開的做法）、使用者回報揭露的真實問題 |
| [mozc](https://github.com/google/mozc) | 日文分詞的成本模型（詞成本＋接續矩陣） |

## 授權

- **程式碼**（`core/`、`platform/`、`settings/`）：GPL-3.0-or-later，見 [LICENSE](LICENSE)
- **詞庫二進位檔**：依上表各原始來源的授權

程式碼與資料分開標示是刻意的——把資料檔一併宣告成 GPL-3 等於替它加上
原始授權沒有給的權利，那個宣告站不住。
