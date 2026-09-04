"""把設計稿 doc/icon.svg 轉成 ime.ico 與 mode_auto.ico。

# 為什麼需要這支

那份 .svg 其實不是向量圖——Affinity 匯出時把一張 944x780 的 PNG 用
base64 包在 <image> 裡。所以這支要先解出點陣圖，再自己縮放。

# 小尺寸為什麼要加黑外框

設計是「白填充＋黑細線」。縮到 16~24px 時細線被平均成淡灰，等於只剩
白色——**深色工作列上很清楚，淺色工作列上幾乎看不見**（實測過）。

Windows 不會幫工作列圖示反色，所以兩種佈景都要能讀。解法是把整個字形
往外擴一圈黑色墊在下面：淺色底看得到外框，深色底看得到白色內部。這跟
make_icons.ps1 對另外三個模式圖示做的事是同一個原則，只是那邊用描邊、
這邊用形態學膨脹（因為來源是點陣圖，沒有路徑可描）。

**外框厚度 1px 是量出來的**：0 完全看不見，2px 會把 16px 的字內部吃光。

# 為什麼先超取樣再加框

直接在 16px 上做膨脹會鋸齒。先放大到 4 倍畫框再縮小，邊緣才乾淨——
跟 make_icons.ps1 的超取樣是同一招。

# 用法

    pip install pillow
    python platform/windows/res/make_icons_from_svg.py

改了 doc/icon.svg 之後重跑，兩個 .ico 會一起更新。
"""

import io
import re
import base64
import struct
from pathlib import Path

from PIL import Image, ImageFilter

HERE = Path(__file__).resolve().parent
# 優先吃 PNG——設計稿現在直接匯出點陣圖（去背、線條加粗過）。
# 舊的 icon.svg 留著當後備，它其實也只是包了一張 PNG 的殼。
PNG_SRC = HERE.parent.parent.parent / "doc" / "icon.png"
SVG_SRC = HERE.parent.parent.parent / "doc" / "icon.svg"
TARGETS = ["ime.ico", "mode_auto.ico"]

# 涵蓋 100%~200% DPI 的工作列（16~32），外加設定頁與檔案總管的大圖
SIZES = [16, 20, 24, 32, 48, 64, 128, 256]
# 這個尺寸以下才加外框——48 以上細線本來就留得住
OUTLINE_MAX = 32
OUTLINE_PX = 1
# 外框的黑色濃度。**65% 是量出來的折衷**：
#   100%  淺色底清楚，但深色底上顯得厚重、16px 更擠
#    65%  淺色底清楚，深色底跟完全不加幾乎沒差別  ← 選這個
#    45%  淺色底偏淡
#     0%  淺色底幾乎看不見（白填充融進背景）
OUTLINE_ALPHA = 0.65
SUPERSAMPLE = 4


def load_source():
    """讀設計稿。PNG 優先，沒有才去 SVG 裡挖那張 base64 PNG。"""
    if PNG_SRC.exists():
        return Image.open(PNG_SRC).convert("RGBA")
    text = SVG_SRC.read_text(encoding="utf-8")
    m = re.search(r"base64,([A-Za-z0-9+/=]+)", text)
    if not m:
        raise SystemExit(f"{SVG_SRC} 裡找不到內嵌的 PNG——換成真向量圖了？")
    return Image.open(io.BytesIO(base64.b64decode(m.group(1)))).convert("RGBA")


def squarify(im):
    """ICO 必須正方形。字形是寬扁的，置中留白而不是裁切——裁會切掉筆畫。

    先 crop 掉四周多餘的透明邊，字才佔滿整個圖示；留著的話小尺寸會
    白白浪費好幾個像素。
    """
    im = im.crop(im.getbbox())
    side = max(im.size)
    out = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    out.paste(im, ((side - im.width) // 2, (side - im.height) // 2))
    return out


def add_outline(im, grow, alpha=OUTLINE_ALPHA):
    """整個字形往外擴一圈黑色，墊在原圖下面。

    半透明是刻意的：深色底本來就看得到白色字，全黑外框只會讓字顯得
    厚重；淺色底才真正需要它。
    """
    fat = im.getchannel("A").filter(ImageFilter.MaxFilter(grow * 2 + 1))
    fat = fat.point(lambda v: int(v * alpha))
    base = Image.new("RGBA", im.size, (0, 0, 0, 0))
    base.paste(Image.new("RGBA", im.size, (0, 0, 0, 255)), (0, 0), fat)
    base.alpha_composite(im)
    return base


def render(square, size):
    mid = size * SUPERSAMPLE
    im = square.resize((mid, mid), Image.LANCZOS)
    if size <= OUTLINE_MAX:
        im = add_outline(im, OUTLINE_PX * SUPERSAMPLE)
    return im.resize((size, size), Image.LANCZOS)


def write_ico(path, images):
    """自己組 ICO 容器。

    Pillow 的 save(format='ICO') 只能從**單一**影像縮放出所有尺寸，
    而我們每個尺寸的內容不同（小的有外框、大的沒有），所以得自己寫。
    格式很簡單：ICONDIR + 每張圖一筆 ICONDIRENTRY + 影像資料。
    """
    blobs = []
    for im in images:
        buf = io.BytesIO()
        im.save(buf, format="PNG", optimize=True)
        blobs.append(buf.getvalue())

    out = io.BytesIO()
    out.write(struct.pack("<HHH", 0, 1, len(images)))
    offset = 6 + 16 * len(images)
    for im, blob in zip(images, blobs):
        # 256 在 ICO 的欄位裡記成 0（那個欄位只有一個位元組）
        w = 0 if im.width >= 256 else im.width
        h = 0 if im.height >= 256 else im.height
        out.write(struct.pack("<BBBBHHII", w, h, 0, 0, 1, 32, len(blob), offset))
        offset += len(blob)
    for blob in blobs:
        out.write(blob)
    path.write_bytes(out.getvalue())
    return len(out.getvalue())


def main():
    square = squarify(load_source())
    frames = [render(square, s) for s in SIZES]
    for name in TARGETS:
        n = write_ico(HERE / name, frames)
        print(f"{name}  {n:,} bytes  {len(SIZES)} 個尺寸")


if __name__ == "__main__":
    main()
