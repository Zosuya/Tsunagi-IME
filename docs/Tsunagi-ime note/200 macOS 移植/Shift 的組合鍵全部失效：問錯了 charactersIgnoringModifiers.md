---
原編號: "2.52.39"
date: 2026-09-09
status: done
一句話: "printable_char 問的是 charactersIgnoringModifiers，Shift+1 回 1 就變注音鍵；改問 characters()，用合成事件鎖住"
pitfall: true
相關:
  - "[[spike 5：快捷鍵攔截]]"
---

# Shift 的組合鍵全部失效：問錯了 charactersIgnoringModifiers

實測回報「shift 相關組合鍵現在都用不了，驚嘆號問號打不出來」。

先確認引擎沒事：`Session::push('!')` 之後 `text()` 就是 `"!"`，
`push('？')`、`push('A')` 同理。所以問題在 macOS 這一層根本沒把那個字交
出去。

根因在 `printable_char`：它問的是 **`charactersIgnoringModifiers()`**。
那支的語意就是字面上的意思——「**沒按修飾鍵的話**會是什麼字」，所以
`Shift+1` 回的是 `1`。而在這個輸入法裡 `1` 是注音鍵（ㄅ），於是驚嘆號、
問號、冒號這些要按 Shift 的標點通通變成注音，大寫字母同理。

改成先問 **`characters()`**（使用者實際打出來的那個字），空字串才退回舊的
那支（死鍵、某些版面配置下 `characters()` 會是空的）。

**為什麼這樣改是安全的**：Cmd／Ctrl／Option 在更上面就整批放行給宿主了
（[[spike 5：快捷鍵攔截]] 決定的），所以走到 `printable_char` 的修飾鍵只剩 Shift 與
Caps Lock——兩者都是「真的輸入」，本來就該照它們的結果走。

### 這是 spike 留下來的，不是後來改壞的

`printable_char` 從 spike 2 就是這樣寫的（那時只要驗「按鍵進得來」，
用哪一支都看得到字）。接上引擎之後它就變成錯的，但**症狀只在按 Shift
的鍵上出現**，而測試多半打的是注音與小寫英文，所以一路活到現在。

同一類的東西還有一個沒踩到的：`Option+字母`（macOS 打 å、® 的正規用法）
現在是放行給宿主，那是 [[spike 5：快捷鍵攔截]] 的決定，仍然成立。

### 用合成事件鎖住

**測資的 `regression` 節鎖不到這個錯**——引擎本來就是對的，錯在平台層。
`NSEvent` 有現成的建構子（`keyEventWithType:…characters:charactersIgnoringModifiers:…`），
兩個字串可以各給各的值，正好模擬 `Shift+1`（`characters` 是 `!`、
`charactersIgnoringModifiers` 是 `1`）。

它測不到「系統實際會回什麼」，但**鎖得住我們問哪一支**——而退化的正是
那個選擇。連同「空字串要退回去」與「功能鍵不算」一起三條。CLAUDE.md 的
測試注意事項加了這條通則。
