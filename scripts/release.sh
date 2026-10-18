#!/usr/bin/env bash
# The Linux files for a release, into dist/v<version>/, plus SHA256SUMS.txt.
# Windows is built with Microsoft's toolchain by .github/workflows/windows.yml and macOS by
# .github/workflows/macos.yml; download their artifacts into the same folder, then run
# `scripts/release.sh --sums-only`. WINDOWS_LOCAL=1 also makes a MinGW Windows build here
# (for testing only).
#
#   WaveHub-<v>-setup-x64.exe           Windows setup (per-user, no admin)
#   WaveHub-<v>-windows-x64.zip         Windows portable
#   WaveHub-<v>-linux-<arch>.AppImage   x86_64 and aarch64
#   WaveHub-<v>-linux-<arch>.deb
#   WaveHub-<v>-linux-<arch>.rpm
#   WaveHub-<v>-linux-<arch>.pkg.tar.zst
#   WaveHub-<v>-linux-<arch>.tar.gz
#   WaveHub.exe, WaveHub-linux-x86_64   bare programs under 0.1.0's names: 0.1.0's
#                                       "Check for updates" links straight to these
#
# Needs: rustup targets x86_64-pc-windows-gnu, x86_64-/aarch64-unknown-linux-gnu; cargo-zigbuild
# + zig (Linux builds for glibc 2.31+); cargo-deb; cargo-generate-rpm; mingw-w64; nsis; zip;
# makepkg (Debian: pacman-package-manager); appimagetool + type2 runtimes (see package-linux.sh).
# Usage: scripts/release.sh
set -euo pipefail
cd "$(dirname "$0")/.."
VER=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
OUT="dist/v$VER"
if [ "${1:-}" = "--sums-only" ]; then
  (cd "$OUT" && sha256sum WaveHub* | sort -k2 > SHA256SUMS.txt && cat SHA256SUMS.txt)
  exit 0
fi
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
