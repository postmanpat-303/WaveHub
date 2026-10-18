#!/usr/bin/env bash
# Linux packages for one processor, from a build made for glibc 2.31 and newer
# (cargo zigbuild --release --target <arch>-unknown-linux-gnu.2.31; see scripts/release.sh):
#   dist/v<ver>/WaveHub-<ver>-linux-<arch>.tar.gz       program + menu entry + icons + install.sh
#   dist/v<ver>/WaveHub-<ver>-linux-<arch>.deb          cargo deb
#   dist/v<ver>/WaveHub-<ver>-linux-<arch>.rpm          cargo generate-rpm
#   dist/v<ver>/WaveHub-<ver>-linux-<arch>.AppImage     appimagetool (APPIMAGETOOL, RUNTIME_DIR)
#   dist/v<ver>/WaveHub-<ver>-linux-<arch>.pkg.tar.zst  makepkg on packaging/arch/wave-hub-bin/PKGBUILD
# Usage: scripts/package-linux.sh 0.2.0 x86_64|aarch64
set -euo pipefail
VER="${1:?version}"; ARCH="${2:?arch}"
cd "$(dirname "$0")/.."
grep -q "^version = \"$VER\"" Cargo.toml || { echo "Cargo.toml version is not $VER" >&2; exit 1; }
grep -q "^pkgver=$VER$" packaging/arch/wave-hub-bin/PKGBUILD || { echo "PKGBUILD pkgver is not $VER" >&2; exit 1; }
TRIPLE="$ARCH-unknown-linux-gnu"
BIN="target/$TRIPLE/release/wave-hub"
[ -x "$BIN" ] || { echo "build $BIN first" >&2; exit 1; }
OUT="dist/v$VER"; mkdir -p "$OUT"
NAME="WaveHub-$VER-linux-$ARCH"
WORK="dist/work-$ARCH"; rm -rf "$WORK"; mkdir -p "$WORK"
ICONS="16 32 48 64 128 256 512"

# --- tar.gz -------------------------------------------------------------------
S="$WORK/$NAME"; mkdir -p "$S/share/applications" "$S/share/metainfo"
install -m755 "$BIN" "$S/wave-hub"
install -m644 packaging/linux/wave-hub.desktop "$S/share/applications/"
install -m644 packaging/linux/wave-hub.metainfo.xml "$S/share/metainfo/"
for s in $ICONS; do install -Dm644 "assets/icons/wavehub_$s.png" "$S/share/icons/hicolor/${s}x${s}/apps/wave-hub.png"; done
install -m644 LICENSE "$S/LICENSE"
sed "1s/.*/Wave Hub $VER - Linux $ARCH/" packaging/linux/README.txt > "$S/README.txt"
install -m755 packaging/linux/install.sh "$S/install.sh"
tar -C "$WORK" --owner=0 --group=0 -czf "$OUT/$NAME.tar.gz" "$NAME"

# --- .deb / .rpm ----------------------------------------------------------------
cargo deb --no-build --no-strip --target "$TRIPLE" --output "$OUT/$NAME.deb"
cargo generate-rpm --target "$TRIPLE" --output "$OUT/$NAME.rpm"

# --- AppImage -------------------------------------------------------------------
APPIMAGETOOL="${APPIMAGETOOL:-/workspace/tools/appimage/appimagetool}"
RUNTIME_DIR="${RUNTIME_DIR:-/workspace/tools/appimage}"
A="$WORK/AppDir"; mkdir -p "$A/usr/bin" "$A/usr/share/applications" "$A/usr/share/metainfo"
install -m755 "$BIN" "$A/usr/bin/wave-hub"
install -m644 packaging/linux/wave-hub.desktop "$A/usr/share/applications/"
install -m644 packaging/linux/wave-hub.desktop "$A/wave-hub.desktop"
install -m644 packaging/linux/wave-hub.metainfo.xml "$A/usr/share/metainfo/wave-hub.appdata.xml"
for s in $ICONS; do install -Dm644 "assets/icons/wavehub_$s.png" "$A/usr/share/icons/hicolor/${s}x${s}/apps/wave-hub.png"; done
install -m644 assets/icons/wavehub_256.png "$A/wave-hub.png"
ln -sf wave-hub.png "$A/.DirIcon"
cat > "$A/AppRun" <<'RUN'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/wave-hub" "$@"
RUN
chmod +x "$A/AppRun"
# No libraries to bundle: Wave Hub uses rustls (no OpenSSL); GL, X11/Wayland and glibc come
# from the system, as AppImages expect.
APPIMAGE_EXTRACT_AND_RUN=1 ARCH="$ARCH" "$APPIMAGETOOL" --no-appstream \
  --runtime-file "$RUNTIME_DIR/runtime-$ARCH" "$A" "$OUT/$NAME.AppImage" >/dev/null
chmod +x "$OUT/$NAME.AppImage"

# --- Arch package -----------------------------------------------------------------
B="$WORK/arch"; mkdir -p "$B"
cp packaging/arch/wave-hub-bin/PKGBUILD "$B/"
cp "$OUT/$NAME.tar.gz" "$B/"     # makepkg uses the local copy instead of downloading
CONF="$WORK/makepkg.conf"
sed -e "s/^CARCH=.*/CARCH=\"$ARCH\"/" -e "s/^CHOST=.*/CHOST=\"$ARCH-pc-linux-gnu\"/" /etc/makepkg.conf > "$CONF"
echo 'PKGEXT=".pkg.tar.zst"' >> "$CONF"
(cd "$B" && makepkg --config "$(realpath "../makepkg.conf")" -f --nodeps --noconfirm --skipinteg >/dev/null)
cp "$B"/wave-hub-bin-$VER-*-"$ARCH".pkg.tar.zst "$OUT/$NAME.pkg.tar.zst"

ls -l "$OUT"/$NAME.*
