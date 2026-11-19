#!/bin/sh
# Builds dist/CrystalDiskInfo_for_Linux-x86_64.AppImage.
# Usage: packaging/build-appimage.sh [path/to/appimagetool]
set -eu
cd "$(dirname "$0")/.."
TOOL="${1:-appimagetool}"

cargo build --release

APPDIR="target/AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" "$APPDIR/usr/share/icons/hicolor/scalable/apps"
cp target/release/info-discos "$APPDIR/usr/bin/"
# Bundled smartctl (smartmontools, GPL-2.0), used when the system has none.
packaging/build-smartctl.sh
cp target/smartctl "$APPDIR/usr/bin/smartctl"
mkdir -p "$APPDIR/usr/share/doc/smartmontools"
cp target/smartmontools-license/COPYING "$APPDIR/usr/share/doc/smartmontools/COPYING"
printf 'smartctl from smartmontools 7.5 (GPL-2.0-or-later), unmodified.\nSource: https://github.com/smartmontools/smartmontools/releases/tag/RELEASE_7_5\n' \
    > "$APPDIR/usr/share/doc/smartmontools/SOURCE"
strip "$APPDIR/usr/bin/info-discos" 2>/dev/null || true
cp packaging/AppRun "$APPDIR/AppRun"
chmod +x "$APPDIR/AppRun"
cp packaging/info-discos.desktop "$APPDIR/"
cp packaging/info-discos.desktop "$APPDIR/usr/share/applications/"
cp packaging/info-discos.svg "$APPDIR/"
cp packaging/info-discos.svg "$APPDIR/usr/share/icons/hicolor/scalable/apps/"
mkdir -p "$APPDIR/usr/share/icons/hicolor/256x256/apps"
cp packaging/info-discos.png "$APPDIR/usr/share/icons/hicolor/256x256/apps/"
cp packaging/info-discos.png "$APPDIR/.DirIcon"
mkdir -p "$APPDIR/usr/share/doc/info-discos"
cp LICENSE-CrystalDiskInfo.txt "$APPDIR/usr/share/doc/info-discos/"

mkdir -p dist
rm -f dist/CrystalDiskInfo_for_Linux-x86_64.AppImage
ARCH=x86_64 "$TOOL" -n "$APPDIR" dist/CrystalDiskInfo_for_Linux-x86_64.AppImage
echo "AppImage: $(pwd)/dist/CrystalDiskInfo_for_Linux-x86_64.AppImage"
