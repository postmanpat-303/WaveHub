#!/bin/sh
# Install Wave Hub for this user: ~/.local/bin/wave-hub, plus the menu entry and icons
# under ~/.local/share. No admin password needed.
# Uninstall: ./install.sh --uninstall   (apps and settings in ~/.local/share/WaveHub are kept)
set -e
cd "$(dirname "$0")"
BIN="${XDG_BIN_HOME:-$HOME/.local/bin}"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
files="applications/wave-hub.desktop metainfo/wave-hub.metainfo.xml"
for s in 16 32 48 64 128 256 512; do files="$files icons/hicolor/${s}x${s}/apps/wave-hub.png"; done
if [ "${1:-}" = "--uninstall" ]; then
  rm -f "$BIN/wave-hub"
  for f in $files; do rm -f "$DATA/$f"; done
  echo "Wave Hub removed."
  exit 0
fi
install -Dm755 wave-hub "$BIN/wave-hub"
for f in $files; do install -Dm644 "share/$f" "$DATA/$f"; done
sed -i "s|^Exec=wave-hub|Exec=$BIN/wave-hub|" "$DATA/applications/wave-hub.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$DATA/applications" 2>/dev/null || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$DATA/icons/hicolor" 2>/dev/null || true
echo "Wave Hub installed: $BIN/wave-hub (and in your applications menu)."
