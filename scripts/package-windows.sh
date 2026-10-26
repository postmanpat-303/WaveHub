#!/usr/bin/env bash
# Windows x64, cross-built on Linux with mingw-w64:
#   dist/v<ver>/WaveHub-<ver>-windows-x64.zip   portable: WaveHub.exe + README + licences
#   dist/v<ver>/WaveHub-<ver>-setup-x64.exe     NSIS per-machine setup (installer/wavehub.nsi)
# Needs: rustup target add x86_64-pc-windows-gnu, mingw-w64, nsis, zip.
# Usage: scripts/package-windows.sh 0.2.0
set -euo pipefail
VER="${1:?version}"
cd "$(dirname "$0")/.."
grep -q "^version = \"$VER\"" Cargo.toml || { echo "Cargo.toml version is not $VER" >&2; exit 1; }
CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc \
  cargo build --release --locked --target x86_64-pc-windows-gnu
OUT="dist/v$VER"; mkdir -p "$OUT"
PKG=dist/pkg; rm -rf "$PKG" && mkdir -p "$PKG"
cp target/x86_64-pc-windows-gnu/release/wave-hub.exe "$PKG/WaveHub.exe"
sed "1s/.*/Wave Hub $VER - Windows x64/" packaging/windows-README.txt > "$PKG/README.txt"
cp LICENSE "$PKG/LICENSE.txt"
cp assets/fonts/OFL-Poppins.txt "$PKG/OFL-Poppins.txt"
for f in "$PKG"/*.txt; do sed -i 's/\r*$/\r/' "$f"; done     # Notepad-friendly line endings
rm -f "$OUT/WaveHub-$VER-windows-x64.zip"
(cd "$PKG" && zip -9 -q -X "../../$OUT/WaveHub-$VER-windows-x64.zip" WaveHub.exe README.txt LICENSE.txt OFL-Poppins.txt)
VI=$(echo "$VER" | awk -F. '{printf "%d.%d.%d.%d", $1, $2, $3, $4}')
(cd installer && makensis -V2 -DVERSION="$VER" -DVIVERSION="$VI" -DPKGDIR=../dist/pkg \
  -DOUTFILE="../$OUT/WaveHub-$VER-setup-x64.exe" wavehub.nsi)
ls -l "$OUT"/WaveHub-$VER-windows-x64.zip "$OUT"/WaveHub-$VER-setup-x64.exe
