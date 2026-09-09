#!/bin/bash
# 建出 macOS 的輸入法 .app 並安裝。**Windows 的 build-ime.ps1 對應物。**
#
# 兩邊的差別（見開發文件 §2.52.3）：
#   - Windows：DLL 被宿主行程鎖住，腳本要靠「改名讓路」才刪得掉
#   - macOS：輸入法是獨立行程，直接殺掉它、讓系統重新拉起來就好
#     ——「宿主要整個關掉重開」那條在這裡不存在
#
# 不寫死絕對路徑（CLAUDE.md 的跨電腦開發注意事項）。
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
APP_NAME="Tsunagi.app"
DEST="$HOME/Library/Input Methods"
BUILT="$ROOT/target/$APP_NAME"

CARGO="${CARGO:-cargo}"
command -v "$CARGO" >/dev/null 2>&1 || CARGO="$HOME/.cargo/bin/cargo"

# --all 連設定頁一起建（對應 Windows 的 `build-ime.ps1 -All`）。
ALL=0
for arg in "$@"; do
	case "$arg" in
	--all) ALL=1 ;;
	*) echo "不認得的參數：$arg（只有 --all）" >&2; exit 2 ;;
	esac
done

echo "==> 編譯"
"$CARGO" build --release -p ime-tip-macos --manifest-path "$ROOT/Cargo.toml"
if [ "$ALL" = 1 ]; then
	echo "==> 編譯設定頁"
	"$CARGO" build --release -p ime-settings --manifest-path "$ROOT/Cargo.toml"
fi

echo "==> 組 bundle"
rm -rf "$BUILT"
mkdir -p "$BUILT/Contents/MacOS" "$BUILT/Contents/Resources"
cp "$ROOT/target/release/tsunagi_ime" "$BUILT/Contents/MacOS/"
cp "$HERE/Info.plist" "$BUILT/Contents/"
# 老派的 bundle 標記，Apple 自己的輸入法都有。CFBundleSignature 是 TSNG。
printf 'APPLTSNG' > "$BUILT/Contents/PkgInfo"
# 選單列的圖示。**要改就用 tools/mkicon.swift 重產**，不要手工修圖——
# 那支會一次把三件事做對：16px＋32px 兩種解析度放同一個 TIFF、
# 標成樣板圖（系統依選單列深淺自動反色）、字貼齊整數像素。
#     swiftc -O -o target/mkicon platform/macos/tools/mkicon.swift
#     ./target/mkicon 通 platform/macos/menuTemplate.tiff
cp "$HERE/menuTemplate.tiff" "$BUILT/Contents/Resources/"

# ★ 設定頁跟輸入法一起裝 ★
#
# 放進 `Contents/Resources/`，`paths::settings_exe()` 就是去那裡找。輸入法
# 有兩條路叫它：組字時打 `config` 再按 ↑↑↓↓，或選單列的「通譯設定…」。
#
# **沒建過設定頁就沿用上一次的**——`--all` 才重建，但既有的執行檔照樣裝
# 進去，不然平常建輸入法會把設定頁弄不見。
if [ -f "$ROOT/target/release/ime_settings" ]; then
	cp "$ROOT/target/release/ime_settings" "$BUILT/Contents/Resources/"
	echo "==> 帶上設定頁"
else
	echo "==> ⚠ 沒有設定頁執行檔（跑一次 ./build-app.sh --all），↑↑↓↓ 會開不出來"
fi

# ★ 開發用的詞庫指路檔 ★
#
# 詞庫 146MB，每次建置都複製進 bundle 太慢，改詞庫還要重裝。寫一個指路檔
# 指回專案的 data/，建置快、改了立刻生效。**正式包不該有這個檔**——那時
# 詞庫要真的放進 Contents/Resources/data（§2.52.5 定的佈局）。
#
# 路徑用 $ROOT 算出來，不寫死（CLAUDE.md 的跨電腦開發注意事項）。
if [ -d "$ROOT/data" ]; then
	printf '%s\n' "$ROOT/data" > "$BUILT/Contents/Resources/data-dir.txt"
	echo "==> 詞庫指向 $ROOT/data（開發用指路檔）"
else
	echo "==> ⚠ 找不到 $ROOT/data，輸入法會沒有詞庫"
fi
# 至少要有一個 .lproj，不然 CFBundle 認不出這是個正常的 app bundle。
#
# ★ 輸入模式在選單裡的名字，鍵是**模式字典的鍵**（不是 TISInputSourceID，
#   兩者剛好同值）——照 Apple 的 AinuIM 對出來的，見開發文件 §2.52.16。
#   沒有這一行的話選單上顯示的是一長串 bundle id。
MODE_KEY="com.tsunagi.inputmethod.Tsunagi.Auto"
write_lproj() {  # $1=語言 $2=顯示名稱
	mkdir -p "$BUILT/Contents/Resources/$1.lproj"
	{
		printf 'CFBundleName = "%s";\n' "$2"
		printf 'CFBundleDisplayName = "%s";\n' "$2"
		printf '"%s" = "%s";\n' "$MODE_KEY" "$2"
	} > "$BUILT/Contents/Resources/$1.lproj/InfoPlist.strings"
}
# ★ 顯示名稱是「通譯-Tsunagi」★
#
# 系統設定與 Fn 的切換清單裡，第三方輸入法只會顯示這個名字（不像 Apple
# 自家還有內建符號可用）。**中英並列**是為了在兩種情境下都認得出來：
# 中文介面看得懂「通譯」，英文介面或搜尋時打得到「Tsunagi」。
#
# 日文那份維持「つなぎ」——日文介面下並列拉丁字反而突兀，而且日文使用者
# 搜尋時打的是假名。
write_lproj en      "通譯-Tsunagi"
write_lproj zh-Hant "通譯-Tsunagi"
write_lproj ja      "つなぎ"

# 簽章。預設 ad-hoc（`-`），用 SIGN_ID 換成真的身分：
#     SIGN_ID="Tsunagi Self Signed" ./build-app.sh
#
# 自己用 ad-hoc 就夠（實測，開發文件 §2.52.16）。曾一度懷疑是簽章擋的，
# 其實是 bundle id 少了 `.inputmethod.`。
SIGN_ID="${SIGN_ID:--}"
echo "==> 簽名（身分：${SIGN_ID}）"
# **裡面那支要先簽**：`--deep` 只處理巢狀的 bundle，Resources 底下一支
# 裸的 Mach-O 不歸它管。先簽它、再簽外層，外層的資源封印才蓋得到它。
if [ -f "$BUILT/Contents/Resources/ime_settings" ]; then
	codesign --force --sign "$SIGN_ID" "$BUILT/Contents/Resources/ime_settings"
fi
codesign --force --deep --sign "$SIGN_ID" "$BUILT"

echo "==> 建查詢工具（target/tisq）"
# 只要 Command Line Tools，不用 Xcode。判準工具跟 .app 一起建，
# 免得每次要用還得想「那支編到哪去了」。
TISQ="$ROOT/target/tisq"
LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
swiftc -O -o "$TISQ" "$HERE/tools/tisq.swift"

echo "==> 安裝到 $DEST"
mkdir -p "$DEST"
# ★ 先複製到旁邊再換過去，不要「先 rm -rf 再 cp -R」★
#
# 系統掃 ~/Library/Input Methods/ 的時機不由我們決定（FSEvent、別的行程
# 觸發的重建都算）。舊做法在整個 cp -R 期間目錄裡沒有這個 .app，
# 那段空窗只要被掃到，寫出來的快取就沒有我們——**輸入法會當場從系統
# 消失**，實測踩過（開發文件 §2.52.19）。改成複製完再換，空窗縮到
# rm＋mv 那一瞬間。
STAGE="$DEST/.$APP_NAME.new"
rm -rf "$STAGE"
cp -R "$BUILT" "$STAGE"
rm -rf "${DEST:?}/$APP_NAME"
mv "$STAGE" "$DEST/$APP_NAME"

# 已經在跑的話殺掉。
#
# **不能只殺不開**：macOS 對「單次登入工作階段內能砍掉輸入法行程的次數」
# 有上限（威注音的 AGENTS.md 也記著這條），反覆重裝之後系統就不再自動
# 拉起來了，症狀是選單裡選得到卻打不出字。所以自己 open 一次。
pkill -x tsunagi_ime 2>/dev/null && echo "==> 舊的行程已終止"

# ★ 一定要重新登記，而且要「等一下再登記」＋「登記完要驗證」★
#
# 換掉 bundle 會發出 FSEvent，系統因此重掃一次輸入源——**而那次重掃比
# 我們晚落地，還會寫出一份不含我們的快取**（實測：換完不登記的話 3～5 秒
# 內就消失）。舊做法在 mv 之後**立刻**登記，好的結果轉眼就被那次遲到的
# 重掃蓋掉，症狀是輸入法在選單裡憑空不見。整個經過見 §2.52.19。
#
# 所以順序是：**先讓它掉**，等 FSEvent 的重掃落地，**再**登記。驗證是
# 因為時間差不保證固定——不驗的話又會變成「大部分時候可以」的東西。
# ★ 登記之前先讓選單的 agent 重新載入清單 ★
#
# 順序抄 vChewing 的安裝程式（`InstallerVMProtocol.swift`）：
# **先 killall TextInputMenuAgent、等半秒、再 register**。反過來做的話，
# 剛登記好的東西會被 agent 手上那份舊清單蓋過去——症狀是「圖示／plist 明明
# 換了，畫面上還是舊的」，我在 §2.52.40 那輪就是這樣反覆試了好幾次。
#
# **只殺 TextInputMenuAgent**。vChewing 的註解講得很清楚：殺 imklaunchagent
# 會讓正在打字的宿主失去與所有輸入法的對接、非重開宿主不可；而
# TextInputSwitcher（按 Fn 那個大 HUD）只是純 UI，殺它沒有意義。
echo "==> 讓選單的 agent 重載清單"
killall TextInputMenuAgent 2>/dev/null || true
sleep 1

echo "==> 等系統重掃落地，再向系統登記"
ok=0
for attempt in 1 2 3 4 5; do
	sleep 3
	"$TISQ" register > /dev/null 2>&1 || true
	sleep 2
	if "$TISQ" 2>/dev/null | grep -q "✓ com.tsunagi"; then
		echo "    第 $attempt 次登記成功"
		ok=1
		break
	fi
	echo "    第 $attempt 次還沒站穩，再試"
done
if [ "$ok" = 1 ]; then
	"$TISQ" | sed 's/^/    /'
else
	echo "    ★ 五次都沒站穩——手動跑 $TISQ register，並看 §2.52.19"
fi

# ★ 把建置產出從 LaunchServices 撤掉並刪除 ★
#
# `$BUILT` 跟裝好的那份**是同一個 bundle id**。LaunchServices 會把專案目錄
# 底下的 .app 也收進資料庫，於是同一個 id 有兩筆記錄，系統依 id 解析時
# 可能挑到專案裡那份（實測發生過，見 §2.52.23）。留著沒有好處。
echo "==> 清掉建置產出，避免同一個 bundle id 有兩份"
"$LSREGISTER" -u "$BUILT" >/dev/null 2>&1 || true
rm -rf "$BUILT"
"$LSREGISTER" -f "$DEST/$APP_NAME" >/dev/null 2>&1 || true

# ★ 一定要自己 open ★
#
# **系統不會自動拉起這個輸入法**——`com.apple.inputmethodkit.launcher` 收到
# 啟動請求後對我們毫無動作（對 Apple 自己的輸入法則是 17 毫秒內就送進
# LaunchServices）。目前唯一量到的差別是 Gatekeeper：`spctl -a -t exec`
# 對 ad-hoc 簽章的我們回 rejected。詳見 §2.52.23。
echo "==> 啟動輸入法（系統不會自己來，見 §2.52.23）"
open "$DEST/$APP_NAME"

echo
echo "完成：$DEST/$APP_NAME"
# ★ 這一行是給人看的，不要拿掉 ★
#
# 腳本會 pkill 舊行程再 open 新的，**宿主手上那條連線在那一瞬間就斷了**，
# 而它不一定會自己接到新行程。症狀是「剛改的東西沒生效」或「本來會的
# 功能突然不會了」——跟 Windows 那條「驗證新版必須讓宿主整個行程重開」
# 是同一類的坑（見 CLAUDE.md 的已知陷阱）。
echo "★ 測之前先切走輸入法再切回來 ★（宿主可能還連著被砍掉的舊行程）"
echo "輸入法在選單裡不見了：$TISQ register"
echo "選得到卻打不出字（系統不肯自動拉起來）：open \"$DEST/$APP_NAME\""
