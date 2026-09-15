---
原編號: "2.46"
date: 2026-09-04
status: done
pitfall: true
一句話: 逐字排版的預覽列吃掉空白，因為 DirectWrite 的 width 不含尾端空白，改用 measure_advance
---

# 預覽列吃掉空白：DirectWrite 的 width 不含尾端空白

使用者回報：「打英文＋早安，**候選視窗裡沒有空格，但送出去是對的**」。

### 2.46.1 先確定不是核心的問題

`show_ime` 把每一層都倒出來，全部都有那個空白：

```text
文字      footer 早安        ← 會送出的文字
切法選單  （中）footer 早安
格子      #1 footer  #2 ␣  #3 yl3 早  #4 0␣ 安
```

所以核心是對的，問題在**平台層的繪製**。

### 2.46.2 根因

預覽列是**逐字排版**的——框要對得準每一個字，所以 `layout_preview`
一個字一個字量寬。而它用的 `Renderer::measure` 最後拿的是
`DWRITE_TEXT_METRICS.width`，那個欄位**不含尾端空白**（DirectWrite 的
設計，為了右對齊時不讓尾巴的空白撐開版面）。

**單獨一個空白，整個就是尾端空白 → 量到 0。** 於是空白那一格寬度 0，
下一個字只往右挪一個字間距，看起來就像沒有空白。

候選清單那幾列不受影響——它們是 `draw_text` 整串畫的，中間的空白照樣
有寬度。**只有逐字排版的預覽列會中**。

### 2.46.3 修法

`widthIncludingTrailingWhitespace` 才是排字要的**前進寬度**。加一個
`Renderer::measure_advance` 用它，`layout_preview` 改用這個；
`measure` 保持原樣（反白塊的位置本來就該用不含尾端空白的那個）。

只有兩個呼叫端用到 2 參數的 `measure`，另一個是候選編號標籤
（`"1"`、`"2"`，沒有空白），所以改動範圍很小。

### 2.46.4 驗證

`d2d.rs` 的繪圖測試（`#[ignore]`，需要顯示裝置）加了兩條斷言：

```rust
assert_eq!(r.measure(" ", &fmt), 0.0, "measure 不含尾端空白");
assert!(r.measure_advance(" ", &fmt) > 0.0, "measure_advance 要含尾端空白");
```

**這兩條是真的呼叫 DirectWrite 量出來的**，不是推論——跑法是
`cargo test -p ime-tip-windows -- --ignored 建得起來`。

### 2.46.5 教訓

**「量寬」有兩種語意，別混用**：`width` 是排版寬度（不含尾端空白，
給對齊用），`widthIncludingTrailingWhitespace` 是前進寬度（給排字用）。
逐字排版一定要用後者，否則所有的空白都會消失。

同性質的地方還有 `measure_row_heights`／`measure_width`——它們量的是
整串，中間的空白不受影響，但如果哪天有候選列以空白結尾，寬度會短一點。
目前沒有這種列。
