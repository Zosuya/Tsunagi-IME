---
原編號: "4.10"
date: 2026-08-29
status: done
一句話: spike 驗證 D2D+DComp 管線建得起來，換來合成器層級的透明與反鋸齒
相關:
  - "[[換掉 GDI：改用 Direct2D + DirectComposition]]"
---

# spike：Direct2D + DirectComposition

**結論：技術上可行，綁定全部對得上，管線建得起來。**

驗證程式：`platform/windows/src/bin/spike_render.rs`
（獨立小程式，不接輸入法。`cargo run -p ime-tip-windows --bin spike_render`）

### 管線長這樣

```text
D3D11 Device ──→ DXGI Device ──→ D2D Device ──→ D2D DeviceContext
                      │                              ↑
                      └→ DComp Device ──→ Target ──→ Visual ──→ Surface
```

比 GDI 囉唆得多——約 60 行只是為了「能開始畫」。換來的是合成器層級的
透明與反鋸齒。

### 三個踩坑（實作時會再遇到）

1. **`WS_EX_NOREDIRECTIONBITMAP` 不加就是黑底**
   沒有它的話，系統會替視窗準備一張不透明的重導向點陣圖，
   DComp 畫的透明內容會被那張圖蓋掉。

2. **顏色必須預乘 alpha**
   合成器用 `DXGI_ALPHA_MODE_PREMULTIPLIED`，RGB 要先乘上 alpha，
   不然半透明的顏色會算錯。

3. **`Matrix3x2` 在另一個 crate**
   `SetTransform` 要的型別在 `windows-numerics`，`windows` crate 沒有
   再匯出。spike 改成把 `BeginDraw` 回傳的偏移直接加進座標，效果一樣、
   不必加依賴。正式實作可考慮同樣做法。

4. **`BeginDraw` 給的貼圖不保證從 (0,0) 開始**
   合成器可能給一塊大貼圖的一角，所有座標都要加上回傳的偏移。

### 相對 GDI 拿到什麼

| | GDI（現行） | D2D + DComp |
|---|---|---|
| 半透明 | ✗ | ✓ |
| 圓角 | `SetWindowRgn`，**鋸齒** | `FillRoundedRectangle`，反鋸齒 |
| 陰影 | `CS_DROPSHADOW`，不可調 | 自繪，可調 |
| 動畫 | 自己每幀重畫會閃 | 合成器負責 |
| 文字 | `DrawTextW`／ClearType | DirectWrite |

### 尚待使用者確認 → **已結案（2026-08-30）**

實際換過去之後（見 [[換掉 GDI：改用 Direct2D + DirectComposition]]），
使用者的評價是「都可以，感覺比舊的還精緻」：

| 項目 | 結果 |
|---|---|
| 透明度 | **先不做**——目前是不透明，之後要再開 |
| 圓角觀感 | 確實優於 GDI 版（反鋸齒） |
| DirectWrite 中文清晰度 | 沒有問題 |

### 不影響跨平台

繪圖層本來就要每平台各寫一次（macOS 用 Cocoa、Linux 用 GTK/Qt），
所以選哪套 Windows 技術跟移植成本無關。`Theme` 結構是純資料，
移植時搬到 `core/` 即可沿用，規格與設定檔格式都不用重訂。
