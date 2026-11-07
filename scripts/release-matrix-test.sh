#!/usr/bin/env bash
# Builds a fake release directory and checks the matrix rules. No network, no GitHub.
set -euo pipefail
cd "$(dirname "$0")/.."
root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT
ver=0.2.9
src=$root/src
dest=$root/dest
mkdir -p "$src/linux" "$src/windows" "$src/mac" "$src/flatpak"

# Versionless copies are made by stage. Builders only supply the originals.
primary() {
  cat <<EOF
WaveHub-${ver}-setup-x64.exe
WaveHub-${ver}-windows-x64.zip
WaveHub.exe
WaveHub-${ver}-linux-x86_64.AppImage
WaveHub-${ver}-linux-x86_64.deb
WaveHub-${ver}-linux-x86_64.rpm
WaveHub-${ver}-linux-x86_64.pkg.tar.zst
WaveHub-${ver}-linux-x86_64.tar.gz
WaveHub-${ver}-linux-aarch64.AppImage
WaveHub-${ver}-linux-aarch64.deb
WaveHub-${ver}-linux-aarch64.rpm
WaveHub-${ver}-linux-aarch64.pkg.tar.zst
WaveHub-${ver}-linux-aarch64.tar.gz
WaveHub-linux-x86_64
WaveHub-${ver}-linux-x86_64.flatpak
WaveHub-${ver}-macos-universal.dmg
WaveHub-${ver}-macos-universal.zip
EOF
}

while IFS= read -r name; do
  case "$name" in
    *setup-x64.exe|*windows-x64.zip|WaveHub.exe) dir=$src/windows ;;
    *flatpak) dir=$src/flatpak ;;
    *macos*) dir=$src/mac ;;
    *) dir=$src/linux ;;
  esac
  printf 'bytes-for-%s\n' "$name" > "$dir/$name"
done < <(primary)

# A partial checksum from one platform must be ignored, not trusted.
echo "not-a-real-sum  SHA256SUMS.txt" > "$src/linux/SHA256SUMS.txt"
# The same legacy binary supplied twice, identical, is fine.
cp "$src/linux/WaveHub-linux-x86_64" "$src/windows/WaveHub-linux-x86_64.duplicate-dir" 2>/dev/null || true
mkdir -p "$src/linux/extra"
cp "$src/linux/WaveHub-linux-x86_64" "$src/linux/extra/WaveHub-linux-x86_64"

bash scripts/release-matrix.sh stage "$ver" "$dest" "$src/linux" "$src/windows" "$src/mac" "$src/flatpak"
bash scripts/release-matrix.sh sums "$ver" "$dest"
bash scripts/release-matrix.sh check "$ver" "$dest"

# Versionless files are the versioned bytes.
cmp "$dest/WaveHub-${ver}-setup-x64.exe" "$dest/WaveHub-setup-x64.exe"
cmp "$dest/WaveHub-${ver}-linux-x86_64.AppImage" "$dest/WaveHub-linux-x86_64.AppImage"
cmp "$dest/WaveHub-${ver}-linux-aarch64.AppImage" "$dest/WaveHub-linux-aarch64.AppImage"
cmp "$dest/WaveHub-${ver}-linux-x86_64.flatpak" "$dest/WaveHub-linux-x86_64.flatpak"
cmp "$dest/WaveHub-${ver}-macos-universal.dmg" "$dest/WaveHub-macos-universal.dmg"
cmp "$dest/WaveHub-${ver}-macos-universal.zip" "$dest/WaveHub-macos-universal.zip"

count=$(bash scripts/release-matrix.sh expected "$ver" | grep -c .)
lines=$(grep -cve '^$' "$dest/SHA256SUMS.txt")
test "$count" = "$lines"
# Sorted by name, sha256sum's two-space format.
sort -k2 -c "$dest/SHA256SUMS.txt"

# Missing a platform fails.
rm "$dest/WaveHub-${ver}-macos-universal.dmg"
if bash scripts/release-matrix.sh check "$ver" "$dest"; then
  echo "check passed with a Mac disk image missing" >&2
  exit 1
fi

# Same bytes under both names, but not the bytes that were checksummed.
printf 'tampered\n' > "$dest/WaveHub-${ver}-macos-universal.dmg"
cp "$dest/WaveHub-${ver}-macos-universal.dmg" "$dest/WaveHub-macos-universal.dmg"
if bash scripts/release-matrix.sh check "$ver" "$dest"; then
  echo "check passed with a stale checksum" >&2
  exit 1
fi

# An extra file fails even when every expected name is present.
bash scripts/release-matrix.sh stage "$ver" "$root/full" "$src/linux" "$src/windows" "$src/mac" "$src/flatpak"
bash scripts/release-matrix.sh sums "$ver" "$root/full"
bash scripts/release-matrix.sh check "$ver" "$root/full"
printf 'nope\n' > "$root/full/WaveHub-extra.bin"
if bash scripts/release-matrix.sh check "$ver" "$root/full"; then
  echo "check passed with an extra file" >&2
  exit 1
fi

# Two different copies of one name fail at stage time.
printf 'other-exe\n' > "$src/linux/WaveHub.exe"
if bash scripts/release-matrix.sh stage "$ver" "$root/clash" "$src/linux" "$src/windows"; then
  echo "stage accepted two different WaveHub.exe files" >&2
  exit 1
fi

echo "release matrix ok"
