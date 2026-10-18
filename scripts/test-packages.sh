#!/usr/bin/env bash
# Smoke tests for the files scripts/release.sh made (run on Debian/Ubuntu x86_64 with sudo,
# rpm, pacman-package-manager, qemu-user, wine). Usage: scripts/test-packages.sh
set -eu   # no pipefail: `| grep -q` closes the pipe early on purpose
cd "$(dirname "$0")/.."
VER=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
D="$(pwd)/dist/v$VER"; cd "$D"
ok() { echo "PASS  $*"; }
sha256sum -c --quiet SHA256SUMS.txt && ok "SHA256SUMS.txt matches all $(wc -l < SHA256SUMS.txt) files"
for a in x86_64 aarch64; do
  N="WaveHub-$VER-linux-$a"
  # glibc floor
  T=$(mktemp -d); tar -C "$T" -xzf "$N.tar.gz"
  g=$(objdump -T "$T/$N/wave-hub" | grep -o 'GLIBC_[0-9.]*' | sort -Vu | tail -1)
  [ "$g" = GLIBC_2.30 ] || [ "$g" \< GLIBC_2.30 ] && ok "$a needs glibc ${g#GLIBC_} or newer"
  run() { if [ $a = x86_64 ]; then "$@"; else qemu-aarch64 -L /usr/aarch64-linux-gnu "$@"; fi; }
  run "$T/$N/wave-hub" --version | grep -q "Wave Hub $VER" && ok "$N.tar.gz runs"
  (cd "$T/$N" && HOME="$T/home" XDG_DATA_HOME= XDG_BIN_HOME= ./install.sh >/dev/null)
  test -x "$T/home/.local/bin/wave-hub" && test -f "$T/home/.local/share/applications/wave-hub.desktop" && ok "$N.tar.gz install.sh"
  run "$T/home/.local/bin/wave-hub" --update-info | grep -q "installed as: tar.gz" && ok "$N.tar.gz detected as tar.gz"
  (cd "$T/$N" && HOME="$T/home" XDG_DATA_HOME= XDG_BIN_HOME= ./install.sh --uninstall >/dev/null); test ! -e "$T/home/.local/bin/wave-hub" && ok "$N.tar.gz uninstall"
  # AppImage
  cp "$N.AppImage" "$T/"; (cd "$T" && run "./$N.AppImage" --appimage-extract >/dev/null 2>&1)
  test -f "$T/squashfs-root/wave-hub.desktop" && run "$T/squashfs-root/usr/bin/wave-hub" --version | grep -q "$VER" && ok "$N.AppImage extracts and runs"
  [ $a = x86_64 ] && "./$N.AppImage" --update-info | grep -q "installed as: AppImage" && ok "$N.AppImage detected as AppImage"
  # deb
  dpkg-deb -c "$N.deb" | grep -q "./usr/bin/wave-hub" && dpkg-deb -f "$N.deb" Depends | grep -q "libc6 (>= 2.31)" && ok "$N.deb contents + Depends ($(dpkg-deb -f "$N.deb" Architecture))"
  # rpm
  rpm -qpl "$N.rpm" 2>/dev/null | grep -qx /usr/bin/wave-hub && [ "$(rpm -qp --qf '%{ARCH}' "$N.rpm" 2>/dev/null)" = "$a" ] && ok "$N.rpm contents ($a)"
  R=$(mktemp -d); sudo rpm --root "$R" --nodeps --ignorearch -i "$N.rpm" 2>/dev/null; run "$R/usr/bin/wave-hub" --version | grep -q "$VER" && ok "$N.rpm installs (rpm --root) and runs"; sudo rm -rf "$R"
  # Arch
  bsdtar -xOf "$N.pkg.tar.zst" .PKGINFO | grep -q "arch = $a" && bsdtar -tf "$N.pkg.tar.zst" | grep -qx usr/bin/wave-hub && ok "$N.pkg.tar.zst contents ($a)"
  R=$(mktemp -d); mkdir -p "$R/var/lib/pacman"
  sudo pacman --root "$R" --dbpath "$R/var/lib/pacman" --arch "$a" --noconfirm -U --nodeps --nodeps --noscriptlet "$N.pkg.tar.zst" >/dev/null 2>&1
  run "$R/usr/bin/wave-hub" --version | grep -q "$VER" && ok "$N.pkg.tar.zst installs (pacman --root) and runs"; sudo rm -rf "$R"
  rm -rf "$T"
done
# deb: a real install on this machine
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -q "./WaveHub-$VER-linux-x86_64.deb" >/dev/null 2>&1
wave-hub --update-info | grep -q "installed as: Debian/Ubuntu package" && test -f /usr/share/icons/hicolor/256x256/apps/wave-hub.png && ok "x86_64 .deb apt install, detected as deb"
sudo apt-get purge -y -q wave-hub >/dev/null 2>&1; ! command -v wave-hub >/dev/null && ok "x86_64 .deb removed"
# legacy bare programs
./WaveHub-linux-x86_64 --version | grep -q "$VER" && ok "WaveHub-linux-x86_64 runs"
# Windows under Wine
export WINEPREFIX="${WINEPREFIX:-/tmp/wh/wineprefix}" WINEDEBUG=-all
I="$WINEPREFIX/drive_c/users/$USER/AppData/Local/Programs/Wave Hub"
timeout 120 wine "WaveHub-$VER-setup-x64.exe" /S && test -f "$I/WaveHub.exe" && ok "setup /S installs per-user to %LOCALAPPDATA%\\Programs\\Wave Hub"
test -f "$WINEPREFIX/drive_c/users/$USER/AppData/Roaming/Microsoft/Windows/Start Menu/Programs/Wave Hub/Wave Hub.lnk" && ok "Start menu shortcut"
wine reg query 'HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\WaveHub' /v DisplayVersion 2>/dev/null | grep -q "$VER" && ok "Apps & features entry (HKCU)"
timeout 30 wine "$I/WaveHub.exe" --update-info 2>/dev/null | tr -d '\r' | grep -q "installed as: Windows setup" && ok "installed copy detected as Windows setup"
timeout 60 wine "$I/Uninstall.exe" /S; sleep 4; test ! -e "$I" && ok "uninstaller removes it"
T=$(mktemp -d); (cd "$T" && unzip -q "$D/WaveHub-$VER-windows-x64.zip")
timeout 30 wine "$T/WaveHub.exe" --update-info 2>/dev/null | tr -d '\r' | grep -q "installed as: Windows portable" && ok "portable zip runs, detected as portable"
timeout 30 wine "$D/WaveHub.exe" --version 2>/dev/null | grep -q "$VER" && ok "WaveHub.exe runs"
rm -rf "$T"
echo "all tests passed"
