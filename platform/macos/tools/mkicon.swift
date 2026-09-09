// 產生選單列的圖示（menuTemplate.tiff）。
//
// # 規格是抄兩個第三方輸入法量出來的，不是抄 Apple
//
// **不要照 Apple 的 `zhuyin.tiff` 調**——§2.52.41 證明過那張圖根本不會被
// 畫（系統對自家來源用內建符號表），它是 macOS 15 時代的舊資產，正方形、
// 圓角很小。照它調出來的東西跟畫面上看到的永遠差一截。
//
// 真正的參考是兩個第三方 IMK 輸入法，它們**各自獨立地收斂到同一組數字**
// （見開發文件 §2.52.42）：
//
//   ・vChewing（原生 IMK）：`MenuIcon-TCVIM.png` 22×16 點（＋@2x 44×32）
//   ・fcitx5-macos：`penguin-26.svg` 29×21 點
//
// 兩者的長寬比都是 **1.38**，圓角指標都是 **22%**（用同一支工具量的）。
// fcitx5 更直接：它同時附 `penguin-15.svg`（圓角 3）與 `penguin-26.svg`
// （圓角 7），`update.sh` 依 `sw_vers` 主版本 ≥ 26 選後者——**macOS 26 把
// 選單列圖示的圓角改大了**，那是社群實測出來的。
//
// 所以這裡：長寬 22:16、圓角 = 高的三分之一、字挖空約高的 65%。
//
// # 兩張都宣告 22×16「點」
//
// `NSImage` 的 size 取自 representation 宣告的**點**尺寸。寫成像素尺寸的話
// 系統會拿一張「44 點的圖」去畫 22 點的位置，大小當場就不對（踩過）。
//
// 用法：swiftc -O -o mkicon mkicon.swift && ./mkicon 通 出力.tiff
import AppKit
import QuartzCore

let glyph = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "通"
let out = CommandLine.arguments.count > 2 ? CommandLine.arguments[2] : "menu.tiff"



/// 圖示的長寬（點）。**不是正方形**——vChewing 與 fcitx5 都是 1.38。
let W = 22.0
let H = 16.0
/// 圓角佔高的比例。0.30 是量出來的——fcitx5 的 macOS 26 版與 vChewing
/// 用同一支工具量都是 22%，我們調到一樣。
let CORNER = 0.30
/// 字的墨水框佔**高**的比例。

let INK = 0.65

/// 一張 `W*scale` × `H*scale` 的空點陣圖。
func bitmap(_ scale: CGFloat) -> NSBitmapImageRep {
    NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: Int(W * scale), pixelsHigh: Int(H * scale),
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
}

/// 有顏色的像素落在哪個範圍。回 `nil` 代表整張是空的。
func inkBounds(_ b: NSBitmapImageRep) -> NSRect? {
    var minX = b.pixelsWide, maxX = -1, minY = b.pixelsHigh, maxY = -1
    for y in 0..<b.pixelsHigh {
        for x in 0..<b.pixelsWide where (b.colorAt(x: x, y: y)?.alphaComponent ?? 0) > 0.35 {
            minX = min(minX, x); maxX = max(maxX, x)
            minY = min(minY, y); maxY = max(maxY, y)
        }
    }
    guard maxX >= 0 else { return nil }
    return NSRect(x: minX, y: minY, width: maxX - minX + 1, height: maxY - minY + 1)
}

/// 把字畫進空白圖，用來量墨水框。
func measure(_ scale: CGFloat, fontSize: CGFloat) -> NSRect? {
    let rep = bitmap(scale)
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    NSColor.clear.setFill()
    NSRect(x: 0, y: 0, width: W * scale, height: H * scale).fill()
    let font = NSFont.systemFont(ofSize: fontSize, weight: .semibold)
    let s = NSAttributedString(string: glyph, attributes: [.font: font, .foregroundColor: NSColor.black])
    s.draw(at: .zero)
    NSGraphicsContext.restoreGraphicsState()
    return inkBounds(rep)
}

/// 畫一張圖：**黑色圓角方塊填滿畫布、字從裡面挖空**。
///
/// 挖空是因為兩個參考實作都是這樣做的——不透明的方塊由系統上色（配合
/// Info.plist 的 `TISIconIsTemplate`），透明的字透出底下的顏色，所以同一張
/// 圖在淺色選單裡是黑塊白字，被反白選到時自動變成反過來。
func draw(_ scale: CGFloat) -> NSBitmapImageRep {
    let w = W * scale, h = H * scale

    // ── 先量 ──
    //
    // **迭代逼近，不是算一次就好**：小尺寸下有 hinting，字級與墨水框不是
    // 線性關係，一次換算會差一成。跑幾輪收斂到目標值。
    let target = h * INK
    var fontSize = (h * 0.6).rounded()
    var ink = NSRect.zero
    for _ in 0..<8 {
        guard let m = measure(scale, fontSize: fontSize) else {
            fatalError("量不到字的墨水框：\(glyph)")
        }
        ink = m
        let got = max(m.width, m.height)
        if abs(got - target) < 0.6 { break }
        fontSize = (fontSize * target / got).rounded()
    }

    // ── 再畫 ──
    let rep = bitmap(scale)
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    NSColor.clear.setFill()
    NSRect(x: 0, y: 0, width: w, height: h).fill()

    // 方塊：填滿整個畫布，圓角是**正圓弧**。
    //
    // 之前為了對上 Apple 的舊資產用過 `CALayer.cornerCurve = .continuous`
    // （squircle）。那條路作廢了——參考實作用的是普通的圓弧
    // （fcitx5 的 SVG 就是 `a 7 7 0 0 0`），半徑大到 1/3 之後兩者的差別
    // 也看不出來了。
    NSColor.black.setFill()
    let r = h * CORNER
    NSBezierPath(roundedRect: NSRect(x: 0, y: 0, width: w, height: h), xRadius: r, yRadius: r).fill()

    // 字挖空。`destinationOut` 是「把來源畫到的地方從目的地扣掉」。
    //
    // ★ 兩套座標 ★ `colorAt(x:y:)` 的原點在**左上**（量墨水框用的是它），
    // `draw(at:)` 的原點在**左下**。直接把量到的 y 拿去畫，字會沉到下半部。
    NSGraphicsContext.current?.compositingOperation = .destinationOut
    let inkBottom = h - (ink.minY + ink.height)
    let dx = ((w - ink.width) / 2 - ink.minX).rounded()
    let dy = ((h - ink.height) / 2 - inkBottom).rounded()
    let font = NSFont.systemFont(ofSize: fontSize, weight: .semibold)
    let s = NSAttributedString(string: glyph, attributes: [.font: font, .foregroundColor: NSColor.black])
    s.draw(at: NSPoint(x: dx, y: dy))
    NSGraphicsContext.restoreGraphicsState()

    // ★ 兩張都宣告 W×H「點」★ 見檔頭的說明，寫錯的話系統會畫成兩倍大。
    rep.size = NSSize(width: W, height: H)
    return rep
}

let image = NSImage(size: NSSize(width: W, height: H))
image.addRepresentation(draw(1))   // 一般螢幕
image.addRepresentation(draw(2))   // Retina（@2x）

guard let data = image.tiffRepresentation else {
    FileHandle.standardError.write("產不出 TIFF\n".data(using: .utf8)!)
    exit(1)
}
try! data.write(to: URL(fileURLWithPath: out))
print("寫出 \(out)（\(Int(W))×\(Int(H)) 點，1x 與 2x 兩張）")
