#!/bin/bash
# 產出可以上傳 GitHub Release 的 macOS 安裝包（.pkg）。
#
# **跟 `build-app.sh` 是兩回事**：那支是開發用（詞庫用指路檔指回專案、
# 裝到自己的 Input Methods、ad-hoc 簽章），這支是給別人用的。
#
# 用法：
#     ./platform/macos/build-release.sh                    # 未簽章，自己測
#     SIGN_ID="Developer ID Application: 你的名字 (TEAMID)" \
#     INSTALLER_ID="Developer ID Installer: 你的名字 (TEAMID)" \
#         ./platform/macos/build-release.sh                # 正式
#
# 產出：target/release-pkg/tsunagi-ime-<版本>-macos[-unsigned].pkg
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
OUT="$ROOT/target/release-pkg"
STAGE="$OUT/root/Library/Input Methods"
APP="$STAGE/Tsunagi.app"

CARGO="${CARGO:-cargo}"
command -v "$CARGO" >/dev/null 2>&1 || CARGO="$HOME/.cargo/bin/cargo"

VERSION=$(plutil -extract CFBundleShortVersionString raw "$HERE/Info.plist")
BUNDLE_ID=$(plutil -extract CFBundleIdentifier raw "$HERE/Info.plist")

echo "==> 版本 ${VERSION}（${BUNDLE_ID}）"
rm -rf "$OUT"
mkdir -p "$STAGE"

# ── 一、二進位詞庫 ──
#
# **一定要先產**。缺 `dict_ja.bin` 的話輸入法每次啟動都要從 35MB 的文字
# 重建日文詞庫——實測 674ms，而宿主等我們連上線只等 3 秒（§2.52.23）。
# 產出來之後載入是 15ms（mmap 直接用），而且隨附體積反而更小：帶
# 40MB 的 .bin 就不必帶 111MB 的原始文字。
echo "==> 產生二進位詞庫（缺的才產）"
"$CARGO" build --release -p ime-core --bins --manifest-path "$ROOT/Cargo.toml" >/dev/null
for pair in "dict_ja.bin:gen_dict_ja" "dict_zh.bin:gen_dict_zh" "connection.bin:gen_connection"; do
	f="${pair%%:*}"; gen="${pair##*:}"
	if [ -z "$(find "$ROOT/data" -name "$f" -print -quit 2>/dev/null)" ]; then
		echo "    產 $f"
		(cd "$ROOT" && "./target/release/$gen" >/dev/null)
	fi
done

# ── 二、編譯 ──
echo "==> 編譯輸入法與設定頁"
"$CARGO" build --release -p ime-tip-macos -p ime-settings --manifest-path "$ROOT/Cargo.toml"

# ── 三、組 bundle ──
echo "==> 組 bundle"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$ROOT/target/release/tsunagi_ime" "$APP/Contents/MacOS/"
cp "$ROOT/target/release/ime_settings" "$APP/Contents/Resources/"
cp "$HERE/Info.plist" "$APP/Contents/"
cp "$HERE/menuTemplate.tiff" "$APP/Contents/Resources/"
cp "$HERE/uninstall.sh" "$APP/Contents/Resources/"
# **授權檔一定要帶**：專案是 GPL-3.0-or-later，而詞庫與擴充包各有自己的
# 授權（`CREDITS.md` 列著）。Windows 的安裝程式也是把這兩份放進安裝目錄。
cp "$ROOT/LICENSE" "$ROOT/CREDITS.md" "$APP/Contents/Resources/" 2>/dev/null || true
printf 'APPLTSNG' > "$APP/Contents/PkgInfo"

MODE_KEY="com.tsunagi.inputmethod.Tsunagi.Auto"
write_lproj() {  # $1=語言 $2=顯示名稱
	mkdir -p "$APP/Contents/Resources/$1.lproj"
	{
		printf 'CFBundleName = "%s";\n' "$2"
		printf 'CFBundleDisplayName = "%s";\n' "$2"
		printf '"%s" = "%s";\n' "$MODE_KEY" "$2"
	} > "$APP/Contents/Resources/$1.lproj/InfoPlist.strings"
}
write_lproj en      "通譯-Tsunagi"
write_lproj zh-Hant "通譯-Tsunagi"
write_lproj ja      "つなぎ"

# ── 四、詞庫 ──
#
# **只帶執行期真的會讀的檔**（76MB），不要整個 `data/`（146MB）。
# 原始文字檔（mozc 的十份詞典、教育部的 xlsx…）是拿來產二進位的，
# 使用者不需要。**清單要跟 core 實際開的檔名對得上**——漏一個的症狀是
# 「某個語言完全打不出字」，而且不會有錯誤訊息。
echo "==> 帶詞庫（只挑執行期需要的）"
NEEDED=(dict_ja.bin dict_zh.bin connection.bin zh_bigram.gram en_50k.txt
        BPMFBase.txt BPMFMappings.txt word_freq.txt char_freq_by_reading.txt
        char_freq.txt priority.txt)
for f in "${NEEDED[@]}"; do
	src=$(find "$ROOT/data" -name "$f" -print -quit 2>/dev/null || true)
	if [ -z "$src" ]; then
		echo "    ★ 缺 $f——先跑 data/download.ps1 把詞庫備齊" >&2
		exit 1
	fi
	rel="${src#$ROOT/data/}"
	mkdir -p "$APP/Contents/Resources/data/$(dirname "$rel")"
	cp "$src" "$APP/Contents/Resources/data/$rel"
done
# 內建的領域包（符號、emoji）。**台語不在這裡**——它是選裝的，見下面。
mkdir -p "$APP/Contents/Resources/packs"
cp "$ROOT/packs/內建"*.txt "$APP/Contents/Resources/packs/" 2>/dev/null || true
echo "    詞庫 $(du -sh "$APP/Contents/Resources/data" | cut -f1)"

# ── 五、簽章 ──
#
# 沒給 `SIGN_ID` 就 ad-hoc。**ad-hoc 的包不能發給別人**：Gatekeeper 會擋，
# 而且 §2.52.23 量到系統不會自動拉起未公證的輸入法。
SIGN_ID="${SIGN_ID:--}"
echo "==> 簽名（身分：${SIGN_ID}）"
codesign --force --sign "$SIGN_ID" "$APP/Contents/Resources/ime_settings"
codesign --force --deep --sign "$SIGN_ID" "$APP"
if [ "$SIGN_ID" = "-" ]; then
	echo "    ⚠ ad-hoc 簽章——只能自己測，不要發出去"
fi

# ── 六、打包 ──
#
# **一定要 pkg，不能給 DMG 讓使用者自己拖**。手動拖進 Input Methods 的
# `.app` 帶著 `com.apple.quarantine`，會被 macOS 丟到 AppTranslocation 的
# 唯讀路徑跑，登記落不了地（實測踩過，見開發文件 §2.52.43）。
#
# `--install-location` 是相對於安裝目標；使用者端用
# `installer -pkg … -target CurrentUserHomeDirectory` 就會裝進自己的家目錄，
# **不必 sudo**。這條抄 vChewing 的官方安裝指引。
# ★ `pkgutil --payload-files` 會列出一堆 `._名字`，那不是垃圾 ★
#
# 那是 cpio 用來攜帶 xattr 的 AppleDouble 條目，**安裝時會還原成 xattr，
# 不會變成散落的檔案**。用 `pkgutil --expand-full` 攤開驗過：`._` 檔 0 個。
#
# 一度想用 `xattr -cr` 清掉來源的屬性，那是白費工——`com.apple.provenance`
# 是 macOS 13+ 的受保護屬性，使用者權限移不掉（對別人的 app 下 `xattr -dr`
# 會得到 Operation not permitted，同一個原因）。
echo "==> 打包 pkg"
mkdir -p "$OUT/scripts"
cat > "$OUT/scripts/postinstall" <<'POST'
#!/bin/bash
# 裝完之後：讓選單的 agent 重載清單，再把輸入法叫起來讓它自己登記。
#
# 順序抄 vChewing 的安裝程式——**先殺 agent、等一下、再登記**，反過來的話
# 剛登記好的會被 agent 手上那份舊清單蓋掉（開發文件 §2.52.42）。
killall TextInputMenuAgent 2>/dev/null || true
sleep 1
open "$HOME/Library/Input Methods/Tsunagi.app" 2>/dev/null || true
exit 0
POST
chmod +x "$OUT/scripts/postinstall"

# ── 台語詞庫：獨立的元件，使用者可以選裝 ──
#
# **為什麼不跟內建包一起塞進去**：
#
# 1. 它很大（9 萬多筆），而且大多數人用不到
# 2. **授權不同**——包檔頭那幾行是 CC BY-SA 4.0 的姓名標示與相同方式分享，
#    是授權的一部分不是說明文字（見 `tools/取台語資料.md`）。做成可以明確
#    勾選的元件，使用者才知道自己裝了什麼
# 3. Windows 的安裝程式也做成選項，兩邊語意要一致
#
# **資料不進版控**，所以這裡是「有就做成選項、沒有就跳過」。要產包看
# `tools/取台語資料.md`（那兩步在 Mac 上照樣跑得動）。
TAIGI_SRC="$ROOT/packs/台語.txt"
HAS_TAIGI=0
if [ -f "$TAIGI_SRC" ]; then
	HAS_TAIGI=1
	TAIGI_ROOT="$OUT/root-taigi"
	TAIGI_PACKS="$TAIGI_ROOT/Library/Input Methods/Tsunagi.app/Contents/Resources/packs"
	mkdir -p "$TAIGI_PACKS"
	cp "$TAIGI_SRC" "$TAIGI_PACKS/"
	echo "==> 台語詞庫：做成選裝元件（$(du -h "$TAIGI_SRC" | cut -f1)）"
else
	echo "==> 沒有 packs/台語.txt，安裝包不會有台語選項"
	echo "    要有的話看 tools/取台語資料.md（資料不進版控）"
fi

# ★ 一定要關掉 bundle relocation ★
#
# `pkgbuild` 預設把 bundle 標成「可重新定位」。安裝時 macOS 會去問
# LaunchServices「這個 bundle id 我認得嗎」，認得的話就**把安裝目標搬到那裡**
# ——實測踩過：它找到我們自己的暫存目錄，於是
#
#     Library/Input Methods/Tsunagi.app relocated to
#         …/target/release-pkg/root/Library/Input Methods/Tsunagi.app
#
# 安裝「成功」、收據也寫了，但 `~/Library/Input Methods/` 空空如也。使用者
# 端一樣會踩：只要他電腦上任何地方有過同一個 bundle id 的舊版本（甚至只是
# 下載資料夾裡的舊 app），新版就會被塞到那裡去。
#
# 這跟 §2.52.23「同一個 bundle id 有兩份」是同一個根因。
COMPONENT="$OUT/component.pkg"
echo "==> 關掉 bundle relocation"
pkgbuild --analyze --root "$OUT/root" "$OUT/component.plist" >/dev/null
/usr/bin/python3 - "$OUT/component.plist" <<'PLIST'
import plistlib, sys
p = sys.argv[1]
with open(p, "rb") as f:
    items = plistlib.load(f)
for it in items:
    it["BundleIsRelocatable"] = False
with open(p, "wb") as f:
    plistlib.dump(items, f)
PLIST

pkgbuild \
	--root "$OUT/root" \
	--component-plist "$OUT/component.plist" \
	--identifier "$BUNDLE_ID" \
	--version "$VERSION" \
	--scripts "$OUT/scripts" \
	--install-location "/" \
	"$COMPONENT" >/dev/null

TAIGI_ID="$BUNDLE_ID.taigi"
if [ "$HAS_TAIGI" = 1 ]; then
	# 這個元件只放一個檔進既有的 bundle 裡，**不要再宣告一次 bundle**
	# ——`--component-plist` 給空陣列，免得兩個元件都說自己擁有那個 app。
	printf '<?xml version="1.0" encoding="UTF-8"?><!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd"><plist version="1.0"><array/></plist>' \
		> "$OUT/taigi-component.plist"
	pkgbuild \
		--root "$TAIGI_ROOT" \
		--component-plist "$OUT/taigi-component.plist" \
		--identifier "$TAIGI_ID" \
		--version "$VERSION" \
		--install-location "/" \
		"$OUT/taigi.pkg" >/dev/null
fi

# ★ 再包一層 product archive，宣告「只能裝在使用者家目錄」★
#
# `pkgbuild` 出來的是**元件包**，沒有安裝範圍的宣告。使用者雙擊時圖形安裝
# 程式會預設裝到**開機磁碟**的 `/Library/Input Methods`——要管理員權限，
# 而且變成系統層安裝，跟我們的設計不合（輸入法本來就該裝在使用者目錄，
# 而且系統層與使用者層各一份會讓同一個 bundle id 出現兩次，§2.52.23）。
#
# `<domains enable_currentUserHome="true" enable_localSystem="false"/>` 把
# 選項限死成「只供我使用」，雙擊就會裝對地方，不必叫使用者自己去點「更改
# 安裝位置」——vChewing 的安裝指引特地提醒那件事，就是因為他們遇過。
#
# 命令列那條 `installer -target CurrentUserHomeDirectory` 本來就會裝對，
# 這一層是給雙擊的人用的。
# ── 安裝完成畫面 ──
#
# **這一頁不是裝飾，是必要的**：macOS 不讓程式自己把輸入法加進清單
# （`TISEnableInputSource` 回 noErr 但不落地，那是防側錄的設計，§2.52.43），
# 所以最後一步一定要使用者自己做。不講的話他會以為裝完就能用。
#
# 而且**新裝的輸入源要重新登入才會出現在清單裡**——實測踩過：檔案到位、
# TIS 也回報 enabled=1，但系統設定與選單列都看不到它，登出再登入就有了。
# 這跟 §2.52.43 的快取是同一類：改了輸入源的中繼資料之後，有些介面只在
# 登入時才重讀。
echo "==> 產生安裝完成的說明頁"
mkdir -p "$OUT/resources"
cat > "$OUT/resources/conclusion.html" <<'HTML'
<!DOCTYPE html>
<html><head><meta charset="utf-8">
<style>
  body { font: -apple-system-body, "Helvetica Neue", sans-serif; margin: 0; color: #1d1d1f; }
  h2 { font-size: 15px; margin: 0 0 8px; }
  ol { margin: 0 0 12px 18px; padding: 0; }
  li { margin-bottom: 6px; line-height: 1.5; }
  .k { background: #f0f0f2; border-radius: 4px; padding: 1px 5px; font-family: ui-monospace, Menlo, monospace; }
  table { border-collapse: collapse; margin-top: 4px; }
  td { padding: 2px 10px 2px 0; line-height: 1.5; }
  .note { color: #6e6e73; margin-top: 12px; line-height: 1.5; }
</style></head><body>

<h2>還差兩步才能用</h2>
<ol>
  <li><b>登出再登入一次。</b>新安裝的輸入法要重新登入之後才會出現在系統的清單裡。</li>
  <li>到<b>「系統設定 → 鍵盤 → 輸入方式」</b>按 <b>＋</b>，在繁體中文底下加入
      <b>通譯-Tsunagi</b>。<br>
      （macOS 不允許程式自己加輸入法，這一步一定要手動。）</li>
</ol>

<h2>常用按鍵</h2>
<table>
  <tr><td><span class="k">Shift</span> + <span class="k">空白</span></td><td>語言鎖定輪替：自動 → 注音 → 日文 → 英文</td></tr>
  <tr><td><span class="k">⌘</span> + <span class="k">Shift</span> + <span class="k">空白</span></td><td>全半形：自動 → 半形 → 全形</td></tr>
  <tr><td><span class="k">Tab</span></td><td>段選單——反白一段，往下選它是什麼</td></tr>
  <tr><td><span class="k">←</span> <span class="k">→</span> <span class="k">↑</span> <span class="k">↓</span></td><td>選字</td></tr>
</table>

<h2>設定與移除</h2>
<p class="note">
設定頁：選單列的輸入法圖示 →「通譯設定…」，或組字時打
<span class="k">config</span> 再按 <span class="k">↑↑↓↓</span>。<br>
<b>要移除的話從設定頁最下面按「解除安裝」</b>——移除之後再去系統設定把那一筆刪掉。
<b>順序不能反</b>：先從系統設定移除就開不了設定頁了。
</p>
<p class="note">
萬一已經先移除、進不去設定頁，在「終端機」貼這行也可以：<br>
<span class="k">open ~/Library/Input\ Methods/Tsunagi.app/Contents/Resources/ime_settings</span>
</p>

</body></html>
HTML

echo "==> 宣告安裝範圍（只供我使用）"
# 有台語包時多一個可勾選的元件，`customize="always"` 才會出現選擇畫面。
#
# **預設不勾**（使用者裁定）。Windows 的 Inno Setup 那邊是 `Types: full`
# ＝預設會裝，**這裡刻意不同**：多數人用不到，而且它有自己的授權
# （CC BY-SA 4.0），讓使用者明確地選才對。
#
# 沒勾的人事後要補：重跑同一個安裝程式、把台語勾起來即可——pkg 跟 Inno
# 一樣可重入，已裝好的部分不受影響。
#
# 主元件標成不可取消（`enabled="false"`），對應 Inno 的 `Flags: fixed`。
if [ "$HAS_TAIGI" = 1 ]; then
	CUSTOMIZE='always'
	TAIGI_CHOICE_LINE='        <line choice="taigi"/>'
	TAIGI_CHOICE="    <choice id=\"taigi\" title=\"台語擴充包\" description=\"用注音打華語詞、候選出台語漢字（台文華文線頂辭典，CC BY-SA 4.0）。鎖定注音時按 TAB 的段選單會多出台語說法。\" start_selected=\"false\">
        <pkg-ref id=\"$TAIGI_ID\"/>
    </choice>
    <pkg-ref id=\"$TAIGI_ID\" version=\"$VERSION\">taigi.pkg</pkg-ref>"
else
	CUSTOMIZE='never'
	TAIGI_CHOICE_LINE=''
	TAIGI_CHOICE=''
fi

cat > "$OUT/distribution.xml" <<XML
<?xml version="1.0" encoding="utf-8"?>
<installer-gui-script minSpecVersion="2">
    <title>通譯-Tsunagi</title>
    <options customize="$CUSTOMIZE" require-scripts="false"/>
    <domains enable_anywhere="false" enable_currentUserHome="true" enable_localSystem="false"/>
    <conclusion file="conclusion.html" mime-type="text/html"/>
    <choices-outline>
        <line choice="ime"/>
$TAIGI_CHOICE_LINE
    </choices-outline>
    <choice id="ime" title="輸入法主程式" description="輸入法本體，含中文注音／日文羅馬字／英文的詞庫。" enabled="false" selected="true">
        <pkg-ref id="$BUNDLE_ID"/>
    </choice>
$TAIGI_CHOICE
    <pkg-ref id="$BUNDLE_ID" version="$VERSION">component.pkg</pkg-ref>
</installer-gui-script>
XML

# 檔名。**英文、而且講明平台**：
#
#   * 中文檔名讓使用者在終端機打不出來——沒簽章的包一定要跑
#     `xattr -dr com.apple.quarantine <路徑>`，那行是要複製貼上的。
#   * Windows 的產物是 `tsunagi-ime-<版本>-win-setup.exe`，兩個平台的
#     檔案並排在同一個 Release 底下，光看副檔名分不夠清楚。
#
# `-unsigned` **自己判斷**，不靠人記得改：沒給 `SIGN_ID` 就是 ad-hoc，
# 那種包發給別人會被 Gatekeeper 擋，檔名要講清楚（vChewing 也這樣發）。
# 之後真的拿 Developer ID 簽了，後綴會自動消失。
if [ "$SIGN_ID" = "-" ]; then
	PKG="$OUT/tsunagi-ime-$VERSION-macos-unsigned.pkg"
else
	PKG="$OUT/tsunagi-ime-$VERSION-macos.pkg"
fi
productbuild \
	--distribution "$OUT/distribution.xml" \
	--package-path "$OUT" \
	--resources "$OUT/resources" \
	"$PKG" >/dev/null
rm -f "$COMPONENT"

INSTALLER_ID="${INSTALLER_ID:-}"
if [ -n "$INSTALLER_ID" ]; then
	echo "==> 簽 pkg（${INSTALLER_ID}）"
	productsign --sign "$INSTALLER_ID" "$PKG" "$PKG.signed" && mv "$PKG.signed" "$PKG"
fi

# 暫存的那份也要從 LaunchServices 撤掉，理由同上——留著會讓下一次安裝
# （或別的工具）又對到它。`build-app.sh` 早就這樣做了，這支漏了才踩到。
LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
"$LSREGISTER" -u "$OUT/root/Library/Input Methods/Tsunagi.app" >/dev/null 2>&1 || true

echo
echo "完成：$PKG"
echo "      $(du -h "$PKG" | cut -f1)"
echo
echo "使用者端安裝："
echo "    xattr -dr com.apple.quarantine <下載到的 pkg>"
echo "    installer -pkg <pkg> -target CurrentUserHomeDirectory"
echo "  （或直接雙擊——安裝範圍已限死成「只供我使用」，不會裝錯地方）"
echo
if [ "$SIGN_ID" = "-" ]; then
	echo "★ 這份沒簽章，發布前要做："
	echo "    1. SIGN_ID/INSTALLER_ID 用 Developer ID 憑證重跑"
	echo "    2. 公證：xcrun notarytool submit \"$PKG\" --keychain-profile <設定檔> --wait"
	echo "    3. 蓋章：xcrun stapler staple \"$PKG\""
fi
