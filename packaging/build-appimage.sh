#!/bin/sh
# Builds dist/DiskInfo-linux-<arch>.AppImage for this machine's architecture.
# Usage: packaging/build-appimage.sh [path/to/appimagetool]
set -eu
cd "$(dirname "$0")/.."
TOOL="${1:-appimagetool}"

cargo build --release

APPDIR="target/AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" "$APPDIR/usr/share/icons/hicolor/scalable/apps"
cp target/release/diskinfo "$APPDIR/usr/bin/"
# Bundled smartctl (smartmontools, GPL-2.0), used when the system has none.
packaging/build-smartctl.sh
cp target/smartctl "$APPDIR/usr/bin/smartctl"
mkdir -p "$APPDIR/usr/share/doc/smartmontools"
cp target/smartmontools-license/COPYING "$APPDIR/usr/share/doc/smartmontools/COPYING"
printf 'smartctl from smartmontools 7.5 (GPL-2.0-or-later), unmodified.\nSource: https://github.com/smartmontools/smartmontools/releases/tag/RELEASE_7_5\n' \
    > "$APPDIR/usr/share/doc/smartmontools/SOURCE"
strip "$APPDIR/usr/bin/diskinfo" 2>/dev/null || true
cp packaging/AppRun "$APPDIR/AppRun"
chmod +x "$APPDIR/AppRun"
cp packaging/diskinfo.desktop "$APPDIR/"
cp packaging/diskinfo.desktop "$APPDIR/usr/share/applications/"
cp packaging/diskinfo.svg "$APPDIR/"
cp packaging/diskinfo.svg "$APPDIR/usr/share/icons/hicolor/scalable/apps/"
mkdir -p "$APPDIR/usr/share/icons/hicolor/256x256/apps"
cp packaging/diskinfo.png "$APPDIR/usr/share/icons/hicolor/256x256/apps/"
cp packaging/diskinfo.png "$APPDIR/.DirIcon"
mkdir -p "$APPDIR/usr/share/doc/diskinfo"
cp LICENSE-CrystalDiskInfo.txt "$APPDIR/usr/share/doc/diskinfo/"

ARCH="${ARCH:-$(uname -m)}"
OUT="dist/DiskInfo-linux-${ARCH}.AppImage"
mkdir -p dist
rm -f "$OUT"
ARCH="$ARCH" "$TOOL" -n "$APPDIR" "$OUT"
echo "AppImage: $(pwd)/$OUT"
