#!/usr/bin/env bash
# The Linux files for a release, into dist/v<version>/, plus SHA256SUMS.txt.
# Windows is built with Microsoft's toolchain by .github/workflows/windows.yml and macOS by
# .github/workflows/macos.yml; download their artifacts into the same folder, then run
# `scripts/release.sh --sums-only`. WINDOWS_LOCAL=1 also makes a MinGW Windows build here
# (for testing only).
#
#   WaveHub-<v>-setup-x64.exe           Windows setup (per-machine, admin)
#   WaveHub-<v>-windows-x64.zip         Windows portable
#   WaveHub-<v>-linux-<arch>.AppImage   x86_64 and aarch64
#   WaveHub-<v>-linux-<arch>.deb
#   WaveHub-<v>-linux-<arch>.rpm
#   WaveHub-<v>-linux-<arch>.pkg.tar.zst
#   WaveHub-<v>-linux-<arch>.tar.gz
#   WaveHub.exe, WaveHub-linux-x86_64   bare programs under 0.1.0's names: 0.1.0's
#                                       "Check for updates" links straight to these
#   WaveHub-setup-x64.exe, WaveHub-linux-<arch>.AppImage, WaveHub-macos-universal.dmg
#                                       versionless copies (--sums-only makes them), so the
#                                       website's .../releases/latest/download/<name> links
#                                       never go stale
#
# Needs: rustup targets x86_64-pc-windows-gnu, x86_64-/aarch64-unknown-linux-gnu; cargo-zigbuild
# + zig (Linux builds for glibc 2.31+); cargo-deb; cargo-generate-rpm; mingw-w64; nsis; zip;
# makepkg (Debian: pacman-package-manager); appimagetool + type2 runtimes (see package-linux.sh).
# Usage: scripts/release.sh
set -euo pipefail
cd "$(dirname "$0")/.."
VER=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
OUT="dist/v$VER"
# Versionless copies of the main downloads (from 0.2.4 on).
versionless() {
  local f
  for f in setup-x64.exe linux-x86_64.AppImage linux-aarch64.AppImage macos-universal.dmg; do
    if [ -f "$OUT/WaveHub-$VER-$f" ]; then cp -f "$OUT/WaveHub-$VER-$f" "$OUT/WaveHub-$f"; else echo "note: no WaveHub-$VER-$f, so no WaveHub-$f" >&2; fi
  done
}
if [ "${1:-}" = "--sums-only" ]; then
  versionless
  (cd "$OUT" && sha256sum WaveHub* | sort -k2 > SHA256SUMS.txt && cat SHA256SUMS.txt)
  exit 0
fi
python3 scripts/check-ico.py assets/icons/wavehub.ico || { echo "assets/icons/wavehub.ico needs BMP small frames (see scripts/check-ico.py)"; exit 1; }
rm -rf "$OUT" && mkdir -p "$OUT"
cargo test --locked --release
for arch in x86_64 aarch64; do
  cargo zigbuild --release --locked --target "$arch-unknown-linux-gnu.2.31"
  scripts/package-linux.sh "$VER" "$arch"
done
if [ "${WINDOWS_LOCAL:-}" = 1 ]; then
  scripts/package-windows.sh "$VER"
  cp target/x86_64-pc-windows-gnu/release/wave-hub.exe "$OUT/WaveHub.exe"
fi
install -m755 target/x86_64-unknown-linux-gnu/release/wave-hub "$OUT/WaveHub-linux-x86_64"
(cd "$OUT" && sha256sum WaveHub* | sort -k2 > SHA256SUMS.txt)
rm -rf dist/work-* dist/pkg
ls -l "$OUT"
