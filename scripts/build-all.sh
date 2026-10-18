#!/usr/bin/env bash
# Builds Wave Hub for Linux (x86_64) and Windows (x86_64, cross-compiled with mingw-w64)
# into dist/v<version>/ with a SHA256SUMS.txt.
# The file names must stay WaveHub-linux-x86_64 and WaveHub.exe: the About page's
# "Check for updates" links straight to them.
# Needs: rustup target add x86_64-pc-windows-gnu, and the mingw-w64 package.
set -euo pipefail
cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
cargo build --release
CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc \
  cargo build --release --target x86_64-pc-windows-gnu
out="dist/v$version"
mkdir -p "$out"
cp target/release/wave-hub "$out/WaveHub-linux-x86_64"
cp target/x86_64-pc-windows-gnu/release/wave-hub.exe "$out/WaveHub.exe"
chmod +x "$out/WaveHub-linux-x86_64"
(cd "$out" && sha256sum WaveHub-linux-x86_64 WaveHub.exe > SHA256SUMS.txt)
ls -l "$out"
cat "$out/SHA256SUMS.txt"
