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

echo "==> 編譯"
"$CARGO" build --release -p ime-tip-macos --manifest-path "$ROOT/Cargo.toml"

echo "==> 組 bundle"
rm -rf "$BUILT"
mkdir -p "$BUILT/Contents/MacOS" "$BUILT/Contents/Resources"
cp "$ROOT/target/release/tsunagi_ime" "$BUILT/Contents/MacOS/"
cp "$HERE/Info.plist" "$BUILT/Contents/"
# 老派的 bundle 標記，Apple 自己的輸入法都有。CFBundleSignature 是 TSNG。
printf 'APPLTSNG' > "$BUILT/Contents/PkgInfo"
cp "$HERE/menu.tiff" "$BUILT/Contents/Resources/"
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
write_lproj en      "Tsunagi"
write_lproj zh-Hant "通譯"
write_lproj ja      "つなぎ"

# 簽章。預設 ad-hoc（`-`），用 SIGN_ID 換成真的身分：
#     SIGN_ID="Tsunagi Self Signed" ./build-app.sh
#
# 自己用 ad-hoc 就夠（實測，開發文件 §2.52.16）。曾一度懷疑是簽章擋的，
# 其實是 bundle id 少了 `.inputmethod.`。
SIGN_ID="${SIGN_ID:--}"
echo "==> 簽名（身分：${SIGN_ID}）"
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
echo "輸入法在選單裡不見了：$TISQ register"
echo "選得到卻打不出字（系統不肯自動拉起來）：open \"$DEST/$APP_NAME\""
