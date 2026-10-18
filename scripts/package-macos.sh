#!/usr/bin/env bash
# "Wave Hub.app" (universal: Apple Silicon + Intel), ad-hoc signed, as a .dmg and a .zip.
# Runs on macOS only (CI: .github/workflows/macos.yml).
#   dist/v<ver>/WaveHub-<ver>-macos-universal.dmg   (app + Applications link + read-me)
#   dist/v<ver>/WaveHub-<ver>-macos-universal.zip   (the .app)
# Usage: scripts/package-macos.sh 0.2.0
set -euo pipefail
VER="${1:?version}"
cd "$(dirname "$0")/.."
grep -q "^version = \"$VER\"" Cargo.toml || { echo "Cargo.toml version is not $VER" >&2; exit 1; }
export MACOSX_DEPLOYMENT_TARGET=11.0
for t in aarch64-apple-darwin x86_64-apple-darwin; do
  cargo build --release --locked --target "$t"
done
OUT="dist/v$VER"; mkdir -p "$OUT"
APP="dist/Wave Hub.app"
rm -rf "$APP"; mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
lipo -create -output "$APP/Contents/MacOS/WaveHub" \
  target/aarch64-apple-darwin/release/wave-hub target/x86_64-apple-darwin/release/wave-hub
lipo -info "$APP/Contents/MacOS/WaveHub"
sed "s/@VERSION@/$VER/g" packaging/macos/Info.plist.in > "$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist"
# Icon: an .iconset from the elephant, then iconutil.
SET="$(mktemp -d)/WaveHub.iconset"; mkdir -p "$SET"
for s in 16 32 128 256 512; do
  sips -z $s $s assets/icons/wavehub_1024.png --out "$SET/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2)); sips -z $d $d assets/icons/wavehub_1024.png --out "$SET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$SET" -o "$APP/Contents/Resources/WaveHub.icns"
printf 'APPL????' > "$APP/Contents/PkgInfo"
# Not notarised, but ad-hoc signed so Apple Silicon will run it.
codesign --force --deep --sign - "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"

NAME="WaveHub-$VER-macos-universal"
rm -f "$OUT/$NAME.zip" "$OUT/$NAME.dmg"
ditto -c -k --sequesterRsrc --keepParent "$APP" "$OUT/$NAME.zip"
DMGDIR="$(mktemp -d)"
cp -R "$APP" "$DMGDIR/"
ln -s /Applications "$DMGDIR/Applications"
cp packaging/macos/README.txt "$DMGDIR/Read me first.txt"
# hdiutil sometimes fails with "Resource busy" on CI runners; try a few times.
for i in 1 2 3 4 5; do
  hdiutil create -volname "Wave Hub $VER" -srcfolder "$DMGDIR" -fs HFS+ -format UDZO -ov "$OUT/$NAME.dmg" && break
  echo "hdiutil failed (try $i), retrying"; sleep $((i * 5))
done
test -f "$OUT/$NAME.dmg"
rm -rf "$DMGDIR"
ls -la "$OUT"/"$NAME".*
