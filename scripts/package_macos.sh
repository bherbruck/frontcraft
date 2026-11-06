#!/usr/bin/env bash
# Wraps a built prs_game for macOS as FrontCraft.app inside a disk image. Runs on a Mac.
# Usage: scripts/package_macos.sh <prs_game> <version> <out dir> [source dir]
# Writes <out dir>/FrontCraft-<version>-macos.dmg. The binary can be universal (lipo) or one architecture.
#
# Not notarised: it is signed ad hoc only, which Apple Silicon needs to run it at all. The first time, a player
# opens it with right click > Open (or System Settings > Privacy & Security > Open Anyway).
#
#   FRONTCRAFT_SERVER   where it plays when opened from Finder, wss://play.frontcraft.online by default.
#   FRONTCRAFT_NOTICES  THIRD_PARTY_LICENSES.html made already; otherwise scripts/third_party_licenses.sh makes it.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
exe="$1"
version="$2"
out="$3"
src="$(cd "${4:-$here}" && pwd)"
server="${FRONTCRAFT_SERVER:-wss://play.frontcraft.online}"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
app="$work/dmg/FrontCraft.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources/mods"
cp "$exe" "$app/Contents/MacOS/prs_game"
cp "$src/mods/"*.toml "$app/Contents/Resources/mods/"
python3 "$here/scripts/make_icons.py" "$app/Contents/Resources" icns

# The game's own licences and every crate's, beside the game.
notices="${FRONTCRAFT_NOTICES:-}"
if [[ -z "$notices" ]]; then
  notices="$work/notices.html"
  "$here/scripts/third_party_licenses.sh" "$notices" "$src"
fi
cp "$src/LICENSE-MIT" "$src/LICENSE-APACHE" "$app/Contents/Resources"/
cp "$notices" "$app/Contents/Resources"/THIRD_PARTY_LICENSES.html
cp "$app/Contents/Resources/"{LICENSE-MIT,LICENSE-APACHE,THIRD_PARTY_LICENSES.html} "$work/dmg/"

# Finder starts an app with no arguments and in /, so this says where to play and where the mods are.
# Started from a terminal with arguments of its own (--map range, --server ...), those win.
cat > "$app/Contents/MacOS/FrontCraft" <<EOF
#!/bin/sh
here="\$(cd "\$(dirname "\$0")" && pwd)"
export PRS_MODS_DIR="\${PRS_MODS_DIR:-\$here/../Resources/mods}"
case "\$*" in
  *--*) ;;
  *) set -- --server "$server" ;;
esac
exec "\$here/prs_game" "\$@"
EOF
chmod +x "$app/Contents/MacOS/FrontCraft" "$app/Contents/MacOS/prs_game"

cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>FrontCraft</string>
  <key>CFBundleDisplayName</key><string>FrontCraft</string>
  <key>CFBundleIdentifier</key><string>online.frontcraft.game</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleExecutable</key><string>FrontCraft</string>
  <key>CFBundleIconFile</key><string>frontcraft</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.action-games</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>FrontCraft uses the microphone for voice chat with your squad.</string>
</dict>
</plist>
EOF

cat > "$work/dmg/README.txt" <<EOF
FrontCraft $version for macOS

Drag FrontCraft into Applications. It is not notarised by Apple, so the first time, right click it and choose
Open, then Open again (or System Settings > Privacy & Security > Open Anyway).

Opened from Finder it plays online at $server. From a terminal:
  /Applications/FrontCraft.app/Contents/MacOS/FrontCraft --map range      practise alone on the shooting range
  PRS_MOD=arcade /Applications/FrontCraft.app/Contents/MacOS/FrontCraft --map range   with a mod from Contents/Resources/mods
EOF
ln -s /Applications "$work/dmg/Applications"

codesign --force --deep --sign - "$app"
codesign --verify --deep "$app"

mkdir -p "$out"
dmg="$out/FrontCraft-$version-macos.dmg"
rm -f "$dmg"
hdiutil create -volname "FrontCraft" -srcfolder "$work/dmg" -fs HFS+ -format UDZO -imagekey zlib-level=9 -ov "$dmg"
echo "Built $dmg ($(($(stat -f %z "$dmg") / 1000000)) MB)"
