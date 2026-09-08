# -*- coding: utf-8 -*-
"""用人審過的測資片段組合出長句。

按鍵一律來自既有測資（CLAUDE.md：新測資的按鍵一律程式產生），這裡更嚴格
——連期望文字都是既有的，只是把片段接起來。

讀的是**合併後的單一測資檔** `core/testdata/測資.txt`（`## 節` 分節），
不再讀已經不存在的 `mixed_*.txt`。
"""
import io, os, random, sys

SRC = sys.argv[1]   # core/testdata/測資.txt
OUT = sys.argv[2]
random.seed(20260906)  # 固定種子，重跑結果一樣

# 取片段的節——原本的 `mixed_*` 檔案合併後就是這些標籤。
# **不取 long 自己**（會遞迴串自己），也不取 number／symbol
# （它們的期望帶符號語意，串起來會亂）。
TAGS = {"cutpoint","daily","trilingual","en_vowel","en_split","ja_en",
        "otaku","holdout","japanese_verbs","en_bopomofo","kana_fragment",
        "zh_words"}

def lang_of(text):
    """片段的語言：用期望文字判斷，最可靠。"""
    has_kana = any(0x3040 <= ord(c) <= 0x30ff for c in text)
    has_han  = any(0x4e00 <= ord(c) <= 0x9fff for c in text)
    if has_kana: return "ja"
    if has_han:  return "zh"
    return "en"

frags = []
tag = None
for line in io.open(SRC, encoding="utf-8"):
    line = line.rstrip("\r\n")
    if line.startswith("## "):
        tag = line[3:].split("｜")[0].strip()
        continue
    if not line.strip() or line.startswith("#"): continue
    if tag not in TAGS: continue
    cols = line.split("\t")
    if len(cols) < 3: continue
    want, kexp, keys = cols[0], cols[1], cols[2]
    # 跳過帶寬容標記的——組合之後那些標記的語意會亂掉
    if "||" in want or want.startswith("~") or not keys.strip(): continue
    n = len(keys)
    if not (3 <= n <= 22): continue
    frags.append((want, kexp, keys, lang_of(want)))

# 連接方式：只用空白與標點。
# **不做「直接黏」**——相鄰同語言的兩段黏起來之後 normalize 會合併成一段，
# 期望欄若仍寫成兩段就永遠比不中，那是測資的錯不是引擎的錯。中英黏接的
# 難題既有測資（en_bopomofo 等）已經覆蓋，這批測的是**長度**。
SPACE  = (" ", "_", " ")
COMMA  = (",", "，", ",")
PERIOD = (".", "。", ".")
JOINS       = [SPACE, COMMA, PERIOD, SPACE, COMMA]
# **跨語言的接縫不放逗號**（2026-09-08 使用者裁決）。
#
# 逗號的全形寫法由 `compose::dominant_cjk` 決定：數逗號**前面**的中文段與
# 日文段誰多，中文給 `，`、日文給 `、`。而這批是隨機拼接的句子，沒有語意
# 主體——「boring です email commit 美味しい，data…」前半日文段真的比較多，
# 說它該用中文逗號毫無依據。引擎照規則給 `、` 是對的，錯的是期望欄一律
# 寫死 `，`。
#
# 那是一道**沒有正確答案的題目**，所以從源頭不要出它：語言不同的兩段之間
# 只用空白或句號（句號中日文一樣，不受影響）。同語言之間照樣用逗號——那時
# 該是什麼很明確，該抓的還是要抓。
JOINS_XLANG = [SPACE, PERIOD, SPACE, PERIOD, SPACE]

def build(target):
    """組一句，長度盡量靠近 target。回 (文字, 按鍵期望, 按鍵)。"""
    for _ in range(400):
        parts, langs = [], set()
        keys_len = 0
        prev_lang = None
        while keys_len < target - 12:
            f = random.choice(frags)
            if parts:
                # 前後兩段語言不同時換一組不含逗號的連接方式
                pool = JOINS if f[3] == prev_lang else JOINS_XLANG
                j = random.choice(pool)
                parts.append(("join", j))
                keys_len += 1
            parts.append(("frag", f))
            langs.add(f[3])
            prev_lang = f[3]
            keys_len += len(f[2])
        if len(langs) < 2:      # 至少兩種語言才算混合句
            continue
        if not (target - 12 <= keys_len <= target + 12):
            continue
        want, kexp, keys = [], [], []
        for kind, v in parts:
            if kind == "frag":
                want.append(v[0]); kexp.append(v[1]); keys.append(v[2])
            else:
                k, w, kk = v
                want.append(w); kexp.append(k); keys.append(kk)
        return "|".join(want), "|".join(kexp), "".join(keys)
    return None

rows = []
for _ in range(50):
    r = build(100)
    if r: rows.append(r)
for _ in range(10):
    r = build(200)
    if r: rows.append(r)

head = """# 長句壓力測資（平均 100 鍵 50 句、200 鍵 10 句）
#
# 為什麼需要這批：`ALIVE_LIMIT = 400` 當初是拿 570 句測資掃出來的，而那批
# 最長只有 35 個按鍵、沒有一句中英交錯三次以上——正是會出事的區域
# （見開發文件 §2.51.3）。這批補的就是那個盲點。
#
# 怎麼造的：**全部由人審過的既有測資片段組合而成**（tools/gen_long_testdata.py，
# 固定亂數種子）。按鍵與期望文字都直接沿用原測資，只是用空白或標點把片段
# 接起來——所以這批不引入任何新的人工判斷。
#
# 片段之間**只用空白與標點連接，不直接黏**：相鄰同語言的兩段黏起來之後
# normalize 會合併成一段，期望欄若仍寫兩段就永遠比不中，那是測資的錯不是
# 引擎的錯。中英黏接的難題既有測資（en_bopomofo 等）已經覆蓋，
# 這批測的是**長度**。
#
# **跨語言的接縫不放逗號**：逗號要全形還是讀點由整句的書寫系統決定
# （`compose::dominant_cjk` 數前面的中日段誰多），而隨機拼接的句子沒有
# 語意主體，那道題沒有正確答案。同語言之間照樣用逗號。
#
# 每一句至少含兩種語言。
#
# 格式：文字期望 <TAB> 按鍵期望 <TAB> 按鍵序列
"""
with io.open(OUT, "w", encoding="utf-8", newline="\n") as f:
    f.write(head)
    for r in rows:
        f.write("%s\t%s\t%s\n" % r)
print("寫出 %d 句" % len(rows))
