"""產生全半形／語言模式提示視窗用的 subset 字型。

# 為什麼需要

`width_window.rs` 的標籤（自／半／全、自／ㄅ／あ／英）原本指定一套商業
字型的名字，字型檔不進 DLL——**別人的電腦沒裝就靜默退回系統字型**，
字形跟設計的不一樣（開發文件 §4.11）。

原本打算把那套商業字型 subset 六個字嵌進來，但**授權不允許**：條款寫著
「限兩台裝置、不得轉讓給任何人」。`fsType=0`（技術上標示可嵌入）不能拿
來反駁白紙黑字的合約——那個位元只是字型檔裡的一個旗標，廠商常常沒設對。

改用 SIL Open Font License 的字型，那個授權明確允許 subset 與散布。

# 為什麼要改名字

來源字型的保留字型名稱（Reserved Font Name）是 `Source`。**OFL 規定
修改版不得使用保留名稱**，所以改名不是選擇而是義務。順帶也避開跟系統
已裝的同名字型搶 `CreateFontW`。

# 為什麼要定格字重

來源是可變字型（wght 100~900），**預設值是 100（Thin）**——那麼細的
筆畫在 28px 的提示視窗裡會糊掉。定格在 400（Regular）。

# 用法

    pip install fonttools
    python platform/windows/res/make_symbol_font.py

改了標籤文字（`width_bar.rs` 的 `symbol` / `LANG_OPTIONS`）之後要重跑，
不然新字沒有字形。
"""

import sys
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

HERE = Path(__file__).resolve().parent
OUT = HERE / "symbols.ttf"

# 來源字型。Windows 11 內建，SIL OFL 1.1。
SRC = Path(r"C:\Windows\Fonts\NotoSansTC-VF.ttf")

# **提示視窗會用到的每一個字**。少一個就會退回系統字型畫那一格，
# 而且不會有任何錯誤訊息——只是字形突然不一樣（實際踩過：第一版漏了
# 「注」「日」，語言模式那排就缺字）。
#
#   自半全    width_bar.rs 的 symbol()          全半形
#   自注日英  同檔 lang_symbol() + Language::short()  語言模式
#
# 注意「注」「日」來自 core 的 `Language::short()`，不在 width_bar.rs 裡
# ——改那邊的標籤也要回來加字。
#
# 順帶一提工作列圖示用的是 ㄅ／あ／A，那是另一套東西（res/*.ico，由
# make_icons.ps1 產生），跟這個字型無關。
TEXT = "自半全注日英"

# 改名的兩個理由見模組註解（OFL 的保留名稱、避免撞名）
NEW_FAMILY = "Tsunagi Symbols"

WEIGHT = 400


def main():
    if not SRC.is_file():
        sys.exit(f"找不到來源字型：{SRC}")

    font = TTFont(SRC)

    # 1. 可變字型定格成單一字重
    if "fvar" in font:
        font = instancer.instantiateVariableFont(font, {"wght": WEIGHT})

    # 2. 只留需要的字
    #
    # 保留 name ID 0/13/14（版權、授權、授權網址）——**OFL 要求散布時
    # 必須附帶授權聲明**，pyftsubset 預設會把 name table 砍掉大半。
    opts = subset.Options()
    opts.name_IDs = [0, 1, 2, 3, 4, 5, 6, 13, 14]
    opts.name_legacy = True
    opts.notdef_outline = True
    opts.recalc_bounds = True
    opts.drop_tables += ["DSIG"]

    subsetter = subset.Subsetter(options=opts)
    subsetter.populate(text=TEXT)
    subsetter.subset(font)

    # 3. 改名（OFL 的保留名稱義務）
    for rec in font["name"].names:
        if rec.nameID in (1, 4):
            rec.string = NEW_FAMILY
        elif rec.nameID == 6:
            rec.string = NEW_FAMILY.replace(" ", "")
        elif rec.nameID == 3:
            rec.string = f"{NEW_FAMILY}: subset of Noto Sans TC"

    font.save(OUT)

    size = OUT.stat().st_size
    print(f"{OUT.name}  {size:,} bytes  ({len(TEXT)} 個字)")

    # 驗證：每個字都要真的有字形
    check = TTFont(OUT, lazy=True)
    cmap = check.getBestCmap()
    missing = [c for c in TEXT if ord(c) not in cmap]
    if missing:
        sys.exit("!! 缺字：" + "".join(missing))
    print("六個字都在，family =", NEW_FAMILY)


if __name__ == "__main__":
    main()
