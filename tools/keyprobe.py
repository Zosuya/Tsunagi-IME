# -*- coding: utf-8 -*-
"""把快捷鍵探針的 log 整理成「哪些組合到得了輸入法」的表。

用法：
    python tools/keyprobe.py                  # 讀 %TEMP%\\ime_debug.log
    python tools/keyprobe.py 某個檔.log

「到得了」= 輸入法看得到這個組合，因此**可以拿它當快捷鍵**。
「沒到」= 被系統或宿主先吃掉了，綁上去也不會有反應。
"""
import io
import os
import re
import sys
from collections import defaultdict

# 要測的組合。分組是為了讀結果時知道每一格代表什麼意思。
SHEET = [
    ("我們自己用的", [
        "Ctrl", "Tab", "Shift+Space", "Esc",
        "Left", "Right", "Up", "Down",
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

LINE = re.compile(r"\[key\] (\S+) (\w+) (.+?) → (\S+)")


def parse(path):
    """回傳 {宿主: {組合: {出現過的階段}}}

    **兩個階段任一個出現就算「到得了」**。這一點踩過坑：一開始只算
    `Test`，結果記事本整組看起來像被吃光——實際上是**宿主的行為不同**，
    有些 App 會先問 `OnTestKeyDown`（這個鍵你要不要）再送 `OnKeyDown`，
    有些直接送 `OnKeyDown`。要量的是「輸入法看不看得到」，那跟宿主
    走哪一條路無關。
    """
    seen = defaultdict(lambda: defaultdict(set))
    with io.open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            m = LINE.search(line)
            if not m:
                continue
            host, stage, combo, _reply = m.groups()
            seen[host][combo].add(stage)
    return seen


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else os.path.join(
        os.environ.get("TEMP", "."), "ime_debug.log")
    if not os.path.exists(path):
        sys.exit(f"找不到 log：{path}\n（要先在 data/ 建一個空的 debug.on 檔案）")

    seen = parse(path)
    if not seen:
        sys.exit("log 裡沒有任何 [key] 紀錄——確認 debug.on 存在，"
                 "而且宿主程式在建置新 DLL 之後**整個關掉重開**過")

    listed = {c for _, w in SHEET for c in w}
    for host, combos in sorted(seen.items()):
        asked = sum(1 for st in combos.values() if "Test" in st)
        hit = sum(1 for c in listed if c in combos)
        print(f"\n══════ {host} ══════")
        print(f"  清單 {len(listed)} 格中到得了 {hit} 格；"
              f"{asked}/{len(combos)} 種宿主有先問過 OnTestKeyDown")
        for group, wanted in SHEET:
            print(f"\n  【{group}】")
            for c in wanted:
                if c in combos:
                    how = "先問再送" if "Test" in combos[c] else "直接送"
                    print(f"    {c:<18} ✓ 到得了（{how}）")
                else:
                    print(f"    {c:<18} ✗ 沒到")
        extra = sorted(set(combos) - listed)
        if extra:
            print("\n  【清單外也看到的】\n    " + "、".join(extra))


if __name__ == "__main__":
    main()
