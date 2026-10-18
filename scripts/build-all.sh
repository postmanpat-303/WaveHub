#!/usr/bin/env bash
# Builds Wave Hub for Linux (x86_64) and Windows (x86_64, cross-compiled with mingw-w64)
# and puts both programs in dist/.
# Needs: rustup target add x86_64-pc-windows-gnu, and the mingw-w64 package.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release
CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc \
  cargo build --release --target x86_64-pc-windows-gnu
mkdir -p dist
cp target/release/wave-hub dist/WaveHub-linux-x86_64
cp target/x86_64-pc-windows-gnu/release/wave-hub.exe dist/WaveHub.exe
ls -lh dist
