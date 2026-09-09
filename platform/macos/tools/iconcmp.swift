// 把系統裡各個輸入法的選單列圖示挖出來比對。**spike 工具，不是產品的一部分。**
//
// 起因：自己畫的圖示跟 Apple 的擺在一起就是不對（顏色、外框都不像），
// 而每改一次都要重裝＋重登入才看得到。與其反覆試，不如**直接把系統手上
// 那份圖示要出來**，看清楚規格再動手。
//
// 用法：
//   swiftc -O -o iconcmp iconcmp.swift
//   ./iconcmp dump <資料夾>   # 每個圖示倒成 PNG（放大 8 倍）＋印出屬性
//   ./iconcmp                 # 開一個視窗，淺色／深色底並排
import AppKit
import Carbon

/// 一個輸入源與它的圖示。
struct Item {
    let name: String
    let id: String
    let image: NSImage?
    let source: String   // 圖示是從哪個屬性拿到的
}

func str(_ src: TISInputSource, _ key: CFString) -> String? {
    guard let p = TISGetInputSourceProperty(src, key) else { return nil }
    return Unmanaged<CFString>.fromOpaque(p).takeUnretainedValue() as String
}

func collect() -> [Item] {
    guard let list = TISCreateInputSourceList(nil, false)?.takeRetainedValue() as? [TISInputSource]
    else { return [] }
    var out: [Item] = []
    for src in list {
        // 只看鍵盤類、而且**已啟用**的——沒啟用的幾百個不必比
        guard let id = str(src, kTISPropertyInputSourceID) else { continue }
        guard let enabled = TISGetInputSourceProperty(src, kTISPropertyInputSourceIsEnabled) else { continue }
        if !CFBooleanGetValue(Unmanaged<CFBoolean>.fromOpaque(enabled).takeUnretainedValue()) { continue }
        let name = str(src, kTISPropertyLocalizedName) ?? id

        // 兩條路：新的給檔案 URL，舊的給 IconRef
        var image: NSImage? = nil
        var from = "（沒有）"
        if let p = TISGetInputSourceProperty(src, kTISPropertyIconImageURL) {
            let url = Unmanaged<CFURL>.fromOpaque(p).takeUnretainedValue() as URL
            image = NSImage(contentsOf: url)
            from = "IconImageURL: \(url.lastPathComponent)"
        }
        // `IconRef` 是不透明指標不是物件，所以不能用 `Unmanaged`——直接轉。
        if image == nil, let p = TISGetInputSourceProperty(src, kTISPropertyIconRef) {
            image = NSImage(iconRef: IconRef(p))
            from = "IconRef"
        }
        out.append(Item(name: name, id: id, image: image, source: from))
    }
    return out
}

/// 把一張圖畫進指定大小的點陣圖，底色自訂。
func render(_ image: NSImage, size: Int, bg: NSColor) -> NSBitmapImageRep {
    let rep = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size,
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    bg.setFill()
    NSRect(x: 0, y: 0, width: size, height: size).fill()
    image.draw(in: NSRect(x: 0, y: 0, width: size, height: size))
    NSGraphicsContext.restoreGraphicsState()
    return rep
}

let args = CommandLine.arguments
if args.count > 2, args[1] == "dump" {
    let dir = URL(fileURLWithPath: args[2])
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    // 自己的那張也一起比
    var items = collect()
    if args.count > 3, let mine = NSImage(contentsOfFile: args[3]) {
        items.append(Item(name: "（我們的）", id: args[3], image: mine, source: "檔案"))
    }
    for it in items {
        guard let img = it.image else {
            print("\(it.name)  \(it.id)  → 沒有圖示")
            continue
        }
        let reps = img.representations.map { "\($0.pixelsWide)x\($0.pixelsHigh)" }.joined(separator: ",")
        print("\(it.name)\t來源=\(it.source)\t樣板圖=\(img.isTemplate)\t尺寸=\(img.size)\t解析度=\(reps)")
        // 淺底與深底各畫一張，放大 8 倍看得清楚
        for (tag, bg) in [("light", NSColor.white), ("dark", NSColor(white: 0.15, alpha: 1))] {
            let rep = render(img, size: 128, bg: bg)
            let safe = it.name.replacingOccurrences(of: "/", with: "_")
            let url = dir.appendingPathComponent("\(safe)-\(tag).png")
            try? rep.representation(using: .png, properties: [:])?.write(to: url)
        }
    }
    exit(0)
}

// ── 視窗模式 ──
final class Delegate: NSObject, NSApplicationDelegate {
    var window: NSWindow!
    func applicationDidFinishLaunching(_ n: Notification) {
        var items = collect().filter { $0.image != nil }
        // 要比的那張從參數給——**還沒裝進去的也能比**，這正是重點：
        // 每改一次就重裝＋重登入太慢，先在視窗裡確認對了再動。
        for path in CommandLine.arguments.dropFirst() {
            if let mine = NSImage(contentsOfFile: path) {
                items.append(Item(
                    name: "→ \((path as NSString).lastPathComponent)",
                    id: path, image: mine, source: "檔案"))
            }
        }
        let cell: CGFloat = 150
        let w = CGFloat(items.count) * cell + 40
        window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: max(w, 600), height: 360),
            styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "輸入法選單列圖示比對"
        let content = NSView(frame: window.contentView!.bounds)

        // 兩排：上面淺底、下面深底。**同一張圖在兩種底色下的樣子才是重點**
        // ——樣板圖會反色，不是樣板圖不會。
        for (row, (tag, bg)) in [("淺色選單", NSColor.white),
                                 ("深色選單列", NSColor(white: 0.15, alpha: 1))].enumerated() {
            let y = CGFloat(row) * 170 + 10
            let strip = NSView(frame: NSRect(x: 10, y: y, width: max(w, 600) - 20, height: 160))
            strip.wantsLayer = true
            strip.layer?.backgroundColor = bg.cgColor
            for (i, it) in items.enumerated() {
                let iv = NSImageView(frame: NSRect(x: CGFloat(i) * cell + 20, y: 60, width: 64, height: 64))
                iv.image = it.image
                iv.imageScaling = .scaleProportionallyUpOrDown
                strip.addSubview(iv)
                let lbl = NSTextField(labelWithString: "\(it.name)\n樣板圖=\(it.image!.isTemplate)")
                lbl.frame = NSRect(x: CGFloat(i) * cell + 8, y: 8, width: cell - 10, height: 40)
                lbl.font = .systemFont(ofSize: 10)
                lbl.textColor = row == 0 ? .black : .white
                strip.addSubview(lbl)
            }
            let title = NSTextField(labelWithString: tag)
            title.frame = NSRect(x: 12, y: y + 132, width: 200, height: 20)
            title.textColor = row == 0 ? .black : .white
            strip.addSubview(title)
            content.addSubview(strip)
        }
        window.contentView = content
        window.center()
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }
}
let app = NSApplication.shared
app.setActivationPolicy(.regular)
let d = Delegate()
app.delegate = d
app.run()
