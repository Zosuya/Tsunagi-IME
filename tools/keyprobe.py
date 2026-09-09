# -*- coding: utf-8 -*-
"""把快捷鍵探針的 log 整理成「哪些組合到得了輸入法」的表。

用法：
    python tools/keyprobe.py                  # 自動找該平台的預設 log
    python tools/keyprobe.py 某個檔.log

「到得了」= 輸入法看得到這個組合，因此**可以拿它當快捷鍵**。
「沒到」= 被系統或宿主先吃掉了，綁上去也不會有反應。

**兩個平台共用這一支**（Windows 的 `platform/windows/src/keyprobe.rs`
與 macOS 的 `platform/macos/src/keyprobe.rs` 刻意寫出同樣的行格式），
這樣兩邊的結果才並排比得了。要測的組合清單依平台換——log 裡出現
`Cmd+` 就當作是 macOS 的資料。
"""
import io
import os
import re
import sys
from collections import defaultdict

# 要測的組合。分組是為了讀結果時知道每一格代表什麼意思。
SHEET = [
    ("我們自己用的", [
        # Shift+Space 是語言鎖定、Ctrl+Shift+Space 是全半形（2026-09-09
        # 換的鍵位，見 §2.73）。單按 Ctrl 已退休，留著看它有沒有真的沒人接。
        "Shift+Space", "Ctrl+Shift+Space", "Tab", "Esc",
        "Left", "Right", "Up", "Down",
        "Ctrl", "LCtrl", "RCtrl",
    ]),
    # 語言鎖定鍵的候選。舊的是單按 Ctrl，換掉的理由是誤觸與軟體衝突。
    #
    # **目標是 Windows 與 macOS 用同一個鍵位**（使用者定，2026-09-09），
    # 所以只量兩個平台都存在的鍵——右 Ctrl 不列入，MacBook 內建鍵盤
    # 右下角是方向鍵，那個鍵根本不存在。
    #
    # **一定要看「放開」那一欄**：`Ctrl+Shift` 切輸入法是在放開時生效，
    # 只看按下會得到「到得了」的錯誤結論。左右分開量，因為 Windows 的
    # 切換熱鍵只綁其中一邊。
    ("語言鎖定鍵候選", [
        # 實測已死（2026-09-09）：按下去會在兩個輸入法之間跳，
        # log 裡一行 Shift 都沒有——系統在 TSF 之前就吃掉了。
        # 留在表裡當對照組，看得到「✗ 沒到」才知道探針是好的。
        "LCtrl+LShift", "RCtrl+RShift", "LCtrl+RShift", "RCtrl+LShift",
        # 單獨的 Shift：相黏鍵問題（連按五次跳協助工具），不是好選擇，
        # 但要量得到才能確認探針正常。
        "LShift", "RShift",
        # 還沒排除的：CapsLock 兩邊都有、而且幾乎沒人拿它當快捷鍵。
        # 它在 Windows 是 0x14，Mac 也有對應的 flagsChanged 事件。
        "CapsLock",
        # 功能鍵：兩邊都有，但筆電上常要配 Fn。
        "F13", "F14", "F15",
    ]),
    ("常被宿主吃掉的", [
        "Ctrl+C", "Ctrl+V", "Ctrl+X", "Ctrl+Z", "Ctrl+A",
        "Ctrl+S", "Ctrl+W", "Ctrl+T", "Ctrl+F", "Ctrl+P", "Ctrl+N",
    ]),
    ("系統級", [
        "Win+Space", "Win+D", "Alt+Tab", "Ctrl+Shift+Esc",
    ]),
    ("可能有空位的", [
        "Alt+Q", "Ctrl+Alt+J", "Ctrl+Alt+K",
        "F1", "F2", "F9", "Ctrl+F9",
    ]),
]

# macOS 版。修飾鍵完全不同（主力是 Cmd 不是 Ctrl），所以整張表要換，
# 不是把 Ctrl 換成 Cmd 就好。
#
# **沒有單獨修飾鍵那幾格**：它們在 macOS 走 flagsChanged 而不是 keyDown，
# 探針本來就看不到。
#
# 語言鎖定鍵 macOS 也要做（2026-09-09 使用者改變主意，推翻 §2.52.8 的
# 裁決），而且要求兩邊同一個鍵位——所以 `Shift+Space` 與
# `Ctrl+Shift+Space` 這兩格**在 Mac 上也要量**，見 §2.73。
MAC_SHEET = [
    ("我們自己用的", [
        # Shift+Space 是語言鎖定、Ctrl+Shift+Space 是全半形（兩邊同鍵位）
        "Tab", "Shift+Space", "Ctrl+Shift+Space", "Esc",
        "Left", "Right", "Up", "Down",
    ]),
    ("常被宿主吃掉的", [
        "Cmd+C", "Cmd+V", "Cmd+X", "Cmd+Z", "Cmd+A",
        "Cmd+S", "Cmd+W", "Cmd+T", "Cmd+F", "Cmd+P", "Cmd+N",
    ]),
    ("系統級", [
        "Cmd+Space", "Ctrl+Cmd+Space", "Cmd+Tab", "Cmd+Q", "Ctrl+Up",
    ]),
    ("可能有空位的", [
        "Option+Q", "Ctrl+Option+J", "Ctrl+Option+K",
        "F1", "F2", "F9", "Cmd+F9",
    ]),
]

LINE = re.compile(r"\[key\] (\S+) (\w+) (.+?) → (\S+)")
FOCUS = re.compile(r"\[focus\] (\S+)")


def parse(path):
    """回傳 {宿主: {組合: {出現過的階段}}}

    **兩個階段任一個出現就算「到得了」**。這一點踩過坑：一開始只算
    `Test`，結果記事本整組看起來像被吃光——實際上是**宿主的行為不同**，
    有些 App 會先問 `OnTestKeyDown`（這個鍵你要不要）再送 `OnKeyDown`，
    有些直接送 `OnKeyDown`。要量的是「輸入法看不看得到」，那跟宿主
    走哪一條路無關。

    **`Up`（放開）不算進「到得了」**——它是另一個問題的答案。有些
    組合按下到得了、放開卻被系統攔走（`Ctrl+Shift` 切鍵盤配置就是
    在放開時生效），混在一起算會蓋掉那個差別，所以輸出時分兩欄。

    順便數 `[focus] 失去`：鍵盤配置真的被切走時輸入法會失去焦點，
    那是比「按鍵沒到」更嚴重的情況。
    """
    seen = defaultdict(lambda: defaultdict(set))
    lost = 0
    with io.open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            if FOCUS.search(line) and "失去" in line:
                lost += 1
                continue
            m = LINE.search(line)
            if not m:
                continue
            host, stage, combo, _reply = m.groups()
            seen[host][combo].add(stage)
    return seen, lost


def default_path():
    """該平台的預設 log 位置。"""
    if sys.platform == "darwin":
        return os.path.expanduser(
            "~/Library/Application Support/tsunagi-ime/keyprobe.log")
    return os.path.join(os.environ.get("TEMP", "."), "ime_debug.log")


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else default_path()
    if not os.path.exists(path):
        sys.exit(f"找不到 log：{path}\n"
                 "（Windows 要先在 data/ 建一個空的 debug.on；"
                 "macOS 不用開關，切到本輸入法按過那些組合就會有）")

    seen, lost = parse(path)
    if not seen:
        sys.exit("log 裡沒有任何 [key] 紀錄——Windows 要確認 debug.on 存在、"
                 "宿主在建置新 DLL 之後整個關掉重開過；"
                 "macOS 要確認真的切到了本輸入法")

    # 哪一張表：log 裡看得到 Cmd 就是 macOS 的資料。
    is_mac = any("Cmd+" in c for combos in seen.values() for c in combos)
    sheet = MAC_SHEET if is_mac else SHEET
    listed = {c for _, w in sheet for c in w}
    if lost:
        print(f"\n⚠ log 裡有 {lost} 次「失去焦點」——如果那是按 Ctrl+Shift "
              "之後發生的，就代表系統把鍵盤配置切走了，那個組合不能用。")

    # 「到得了」只看按下那兩關；放開是另一個問題，分一欄印，見 parse()
    down_stages = {"Test", "Down"}
    for host, combos in sorted(seen.items()):
        asked = sum(1 for st in combos.values() if "Test" in st)
        hit = sum(1 for c in listed if down_stages & combos.get(c, set()))
        print(f"\n══════ {host} ══════")
        print(f"  清單 {len(listed)} 格中到得了 {hit} 格；"
              f"{asked}/{len(combos)} 種宿主有先問過 OnTestKeyDown")
        for group, wanted in sheet:
            print(f"\n  【{group}】")
            for c in wanted:
                st = combos.get(c, set())
                if down_stages & st:
                    how = "先問再送" if "Test" in st else "直接送"
                    # **放開那一欄才是組合鍵能不能用的關鍵**：按下到得了、
                    # 放開卻沒到，代表系統在中間把它攔走了。
                    up = "放開也到" if "Up" in st else "⚠ 放開沒到"
                    print(f"    {c:<18} ✓ 到得了（{how}）  {up}")
                elif "Up" in st:
                    print(f"    {c:<18} ~ 只有放開到得了")
                else:
                    print(f"    {c:<18} ✗ 沒到")
        extra = sorted(set(combos) - listed)
        if extra:
            print("\n  【清單外也看到的】\n    " + "、".join(extra))


if __name__ == "__main__":
    main()
