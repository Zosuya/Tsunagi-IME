#!/usr/bin/env python3
"""筆記的守門員：frontmatter 壞掉不會有任何錯誤，只會在 MOC 裡靜默消失。

症狀是那一列變成 `❓` 與 `-`——檔案看起來好好的，Dataview 卻讀不到任何欄位。
所以這種錯要靠掃描抓，不能靠眼睛。

這是**範本**：複製到專案的 `tools/check-notes.py`。vault 位置自動找（`docs/` 底下
含 `000 專案總覽` 的那個資料夾），資料夾名不限。

用法：
    python tools/check-notes.py            # 檢查
    python tools/check-notes.py --quiet    # 只在有問題時輸出（給 hook 用）

退出碼永遠是 0——照 .githooks/commit-msg 的精神，講一聲但不擋下來。
"""
import io
import os
import re
import sys

try:
    import yaml
except ImportError:
    print('需要 pyyaml：pip install pyyaml')
    sys.exit(0)

def find_vault():
    """找 vault：docs/ 底下唯一含 `000 專案總覽` 的資料夾。

    資料夾名刻意不寫死——Obsidian 的儲存庫切換器拿資料夾名當顯示名，
    每個專案都叫 notes 的話在切換器裡分不出來，所以慣例是
    `<專案名> note`（例如 `docs/Tsunagi-ime note/`）。
    """
    docs = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'docs')
    if not os.path.isdir(docs):
        return None
    for d in sorted(os.listdir(docs)):
        p = os.path.join(docs, d)
        if os.path.isdir(p) and os.path.isdir(os.path.join(p, '000 專案總覽')):
            return p
    return None


NOTES = find_vault()
VALID_STATUS = {'done', 'unverified', 'rejected', 'paused', 'planned', 'superseded'}
ATTACH_RE = re.compile(r'\.(png|jpg|jpeg|gif|svg|webp|pdf|mp4|mov)$', re.I)
NOTE_MAX = 10  # status_note 上限，超過會把 MOC 表格第一欄撐爆

# 剛從單檔轉過來時填舊檔名（例如 '開發文件.md'），掃出還沒改完的殘留連結。
# 轉換完成後設回 None——長期使用的 vault 留著它只會製造雜訊。
LEGACY_DOC = None   # 2026-09-12 從單檔轉過來，2026-09-13 殘留連結已清乾淨


def check():
    problems = []
    names = set()
    notes = []

    for root, dirs, files in os.walk(NOTES):
        dirs[:] = [d for d in dirs if d != '.obsidian']
        for f in files:
            if f.endswith('.md'):
                names.add(f[:-3])
                notes.append(os.path.join(root, f))

    for path in notes:
        rel = os.path.relpath(path, NOTES)
        name = os.path.basename(path)[:-3]
        is_moc = name.startswith('_MOC ') or name.startswith('MOC ') or name == '__首頁__'
        text = io.open(path, encoding='utf-8').read()

        def bad(msg):
            problems.append((rel, msg))

        if not text.startswith('---\n'):
            bad('沒有 frontmatter')
            continue
        end = text.find('\n---\n', 4)
        if end < 0:
            bad('frontmatter 沒有結束的 ---')
            continue

        try:
            fm = yaml.safe_load(text[4:end]) or {}
        except Exception as e:
            # 最常見的三種：值開頭是逗號、值裡有冒號、雙引號內的反斜線
            bad('YAML 解析失敗（欄位會全部讀不到）：%s' % str(e).split('\n')[0][:70])
            continue

        if not is_moc:
            st = fm.get('status')
            if not st:
                bad('缺 status——這篇不會出現在任何 MOC 裡')
            elif st not in VALID_STATUS:
                bad('status 無效：%r（有效值：%s）' % (st, '/'.join(sorted(VALID_STATUS))))

            if not fm.get('一句話'):
                bad('缺 一句話——MOC 表格會是空的')

            note = fm.get('status_note')
            if note and len(str(note)) > NOTE_MAX:
                bad('status_note %d 字，超過 %d——完整說明請搬進內文的狀態引言'
                    % (len(str(note)), NOTE_MAX))

            if st == 'superseded' and not fm.get('superseded_by'):
                bad('標了 superseded 卻沒說被誰取代')

        # 斷鏈。`![[x]]` 是嵌入附件不是筆記連結，程式碼區塊裡的是範例，兩者都跳過
        body = re.sub(r'```.*?```', '', text, flags=re.S)
        for m in re.finditer(r'(!?)\[\[([^\]|#]+)', body):
            if m.group(1):          # ![[...]] 嵌入附件
                continue
            link = m.group(2).strip()
            if link and link not in names and not ATTACH_RE.search(link):
                bad('連到不存在的筆記：[[%s]]' % link)

        # 剛轉換完才檢查的殘留舊連結（講搬家經過的筆記放 000，除外）
        if LEGACY_DOC and LEGACY_DOC in text and not rel.startswith('000 '):
            bad('還留著指向 %s 的連結' % LEGACY_DOC)

    return problems, len(notes)


if __name__ == '__main__':
    quiet = '--quiet' in sys.argv
    problems, total = check()

    if problems:
        print('\n筆記檢查：%d 篇裡有 %d 個問題\n' % (total, len(problems)))
        last = None
        for rel, msg in problems:
            if rel != last:
                print('  %s' % rel)
                last = rel
            print('      %s' % msg)
        print('')
    elif not quiet:
        print('筆記檢查：%d 篇全部通過' % total)
