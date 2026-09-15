#!/bin/bash
# 建 spike 2 的安裝程式 .app（前景 GUI app）。
# 為什麼要獨立一支，見 src/installer.rs 的說明。
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
APP="$ROOT/target/Tsunagi Installer.app"

CARGO="${CARGO:-cargo}"
command -v "$CARGO" >/dev/null 2>&1 || CARGO="$HOME/.cargo/bin/cargo"

"$CARGO" build --release -p ime-tip-macos --bin tsunagi_installer --manifest-path "$ROOT/Cargo.toml"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources/en.lproj"
cp "$ROOT/target/release/tsunagi_installer" "$APP/Contents/MacOS/"
cp "$HERE/Installer-Info.plist" "$APP/Contents/Info.plist"
printf 'APPLTSNI' > "$APP/Contents/PkgInfo"
printf 'CFBundleName = "Tsunagi Installer";\n' > "$APP/Contents/Resources/en.lproj/InfoPlist.strings"

SIGN_ID="${SIGN_ID:--}"
codesign --force --deep --sign "$SIGN_ID" "$APP"

echo "完成：$APP"
