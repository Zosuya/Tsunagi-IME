#!/bin/bash
# 移除 macOS 版的通譯輸入法。**`build-app.sh` 的對應物。**
#
# 預設**只移除程式**，使用者資料（設定、學習檔、主題、擴充包）留著——
# 重裝之後那些東西還在，那才是使用者要的。真的要清空加 `--all`。
#
# # 為什麼不能只是 rm -rf
#
# 輸入法還在被系統用著的話，直接刪掉會讓它卡在「登記了但檔案不見」的狀態
# ——選單裡還看得到、選了打不出字，而且重裝也蓋不掉（實測踩過類似的，
# 見開發文件 §2.52.19）。順序要對：**先從系統的輸入源清單移除、殺掉行程、
# 再刪檔案**。
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
APP="$HOME/Library/Input Methods/Tsunagi.app"
DATA="$HOME/Library/Application Support/tsunagi-ime"
LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister

ALL=0
for arg in "$@"; do
	case "$arg" in
	--all) ALL=1 ;;
	*) echo "不認得的參數：$arg（只有 --all）" >&2; exit 2 ;;
	esac
done

if [ ! -d "$APP" ]; then
	echo "==> $APP 不存在，沒東西可移除"
else
	# ★ 順序是「先移除檔案、再清系統設定的那一筆」★
	#
	# 反過來會**死結**：設定頁的入口是輸入法自己（選單列的「通譯設定…」、
	# 或打 `config` 按 ↑↑↓↓），先把輸入法從系統設定移掉的話就再也開不了
	# 設定頁，那顆解除安裝按鈕也按不到了。
	#
	# 這樣做的代價只是清單裡會留一筆死的殘影——`TISDisableInputSource` 跟
	# `TISEnableInputSource` 一樣回成功但不落地（§2.52.43），程式清不掉。
	# 使用者自己刪，或等下次登入它自己消失。

	echo "==> 結束輸入法行程"
	pkill -x tsunagi_ime 2>/dev/null || true
	pkill -x ime_settings 2>/dev/null || true

	echo "==> 從 LaunchServices 撤銷登記"
	"$LSREGISTER" -u "$APP" >/dev/null 2>&1 || true

	echo "==> 刪除 $APP"
	rm -rf "$APP"

	# 選單的 agent 抓著舊清單，不重載的話「通譯」會在選單裡留成殘影
	echo "==> 重載選單的 agent"
	killall TextInputMenuAgent 2>/dev/null || true
fi

if [ "$ALL" = 1 ]; then
	echo "==> 連使用者資料一起刪：$DATA"
	rm -rf "$DATA"
else
	echo
	echo "使用者資料留著（設定、學習檔、主題、擴充包）："
	echo "    $DATA"
	echo "要一起刪的話：$0 --all"
fi

echo
echo "★ 還有一步：到「系統設定 → 鍵盤 → 輸入方式」把「通譯-Tsunagi」刪掉"
echo "  （程式改不了那份清單，macOS 不允許。不刪的話下次登入它也會自己消失）"
echo
echo "完成。"
