// 問系統「你到底收下這個輸入源了沒」。**spike 2 那三小節的救命工具**
// （開發文件 §2.52.13～§2.52.16）——TISRegisterInputSource 回 noErr 不代表
// 系統真的收下，要用 TISCreateInputSourceList 查回來才算數。
//
// 編：swiftc -O -o /tmp/tisq platform/macos/tools/tisq.swift   （只要 CLT，不用 Xcode）
// 用：/tmp/tisq                      列總數、所有第三方輸入源、依 bundle id 查
//     /tmp/tisq register             對 ~/Library/Input Methods/Tsunagi.app 呼叫
//                                    TISRegisterInputSource——這會強制系統重掃，
//                                    不用登出登入
//     /tmp/tisq enable               先重掃再 TISEnableInputSource。**實測回 0 但一律不落地**
//                                    ——命令列與前景 GUI 都一樣，推測是 macOS 26
//                                    刻意的閘（§2.52.16）。啟用請走系統設定 →
//                                    鍵盤 → 輸入方式 → 編輯 → ＋。留著這個動作
//                                    是為了日後系統放寬時好複驗。
//
// 看不到自己時，先開 HIToolbox 的追蹤再跑一次 register：
//     defaults write -g AppleTISTraceCacheRebuild -bool true
//     defaults write com.apple.HIToolbox AppleTISTraceCacheRebuild -bool true
//     log show --last 2m --predicate 'eventMessage CONTAINS "TIS cache rebuild"'
//     （看完記得 defaults delete 兩個鍵）
// 有 `AddInputMethodAppInfoForCache … SUCCESS for …/Tsunagi.app` 才代表掃描器
// 真的讀了 Info.plist；一行都沒有就是 bundle id 沒過 `.inputmethod.` 那道過濾。
import Carbon
import Foundation

let bid = "com.tsunagi.inputmethod.Tsunagi"
let appPath = NSString(string: "~/Library/Input Methods/Tsunagi.app").expandingTildeInPath

func prop(_ s: TISInputSource, _ k: CFString) -> String {
    guard let p = TISGetInputSourceProperty(s, k) else { return "-" }
    return String(describing: Unmanaged<AnyObject>.fromOpaque(p).takeUnretainedValue())
}

func byBundle(_ id: String) -> [TISInputSource] {
    let f = [kTISPropertyBundleID as String: id] as CFDictionary
    return TISCreateInputSourceList(f, true)?.takeRetainedValue() as? [TISInputSource] ?? []
}

func list() {
    let all = TISCreateInputSourceList(nil, true).takeRetainedValue() as! [TISInputSource]
    print("全部輸入源：\(all.count)（含未啟用）")

    // ★ 判準就是這一行 ★ includeAllInstalled = false 只回**已啟用**的。
    //   `defaults read com.apple.HIToolbox AppleEnabledInputSources` **不是**
    //   判準——tsInputModeDefaultStateKey 為 true 的模式預設就啟用，
    //   偏好裡不會留一筆。為了這個白繞一大圈，見開發文件 §2.52.17。
    let on = TISCreateInputSourceList(nil, false).takeRetainedValue() as! [TISInputSource]
    print("已啟用共 \(on.count) 個；其中本輸入法：")
    for s in on where prop(s, kTISPropertyInputSourceID).contains("tsunagi") {
        print("  ✓ \(prop(s, kTISPropertyInputSourceID))")
    }
    var third = 0
    for s in all where !prop(s, kTISPropertyInputSourceID).hasPrefix("com.apple.") {
        third += 1
        print("  第三方：\(prop(s, kTISPropertyInputSourceID))  type=\(prop(s, kTISPropertyInputSourceType))  enabled=\(prop(s, kTISPropertyInputSourceIsEnabled))")
    }
    print("第三方輸入源：\(third)")
    print("依 bundle id 查 \(bid)：\(byBundle(bid).count)")
    // 對照組：查詢語法本身有沒有問題
    print("對照組 com.apple.inputmethod.TCIM：\(byBundle("com.apple.inputmethod.TCIM").count)")
}

switch CommandLine.arguments.dropFirst().first {
case "register":
    let url = URL(fileURLWithPath: appPath)
    let st = TISRegisterInputSource(url as CFURL)
    print("TISRegisterInputSource(\(appPath)) → \(st)")
    list()
case "enable":
    // 先 register 讓**這個行程**重掃——每個行程各有一份快取副本，全新行程
    // 沒重掃前拿到的是舊的（§2.52.16），直接查會撲空。
    _ = TISRegisterInputSource(URL(fileURLWithPath: appPath) as CFURL)
    let found = byBundle(bid)
    if found.isEmpty { print("系統沒有 \(bid)——bundle id 過不了 .inputmethod. 那道過濾？"); exit(1) }
    for s in found {
        let st = TISEnableInputSource(s)
        print("TISEnableInputSource \(prop(s, kTISPropertyInputSourceID)) → \(st)")
    }
    // 啟用狀態是非同步寫進 com.apple.HIToolbox 偏好的，行程馬上結束會丟掉。
    RunLoop.current.run(until: Date(timeIntervalSinceNow: 2))
    CFPreferencesAppSynchronize("com.apple.HIToolbox" as CFString)
default:
    list()
}
