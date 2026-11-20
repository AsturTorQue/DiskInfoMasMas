#!/bin/sh
# Builds dist/DiskInfo-macos-universal.zip: DiskInfo.app for Intel and Apple silicon Macs,
# with a bundled universal smartctl. Run on a Mac with Rust and the Xcode command line tools.
set -eu
cd "$(dirname "$0")/.."

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
rustup target add x86_64-apple-darwin aarch64-apple-darwin
MACOSX_DEPLOYMENT_TARGET=11.0 cargo build --release --target x86_64-apple-darwin
MACOSX_DEPLOYMENT_TARGET=11.0 cargo build --release --target aarch64-apple-darwin
packaging/build-smartctl.sh macos-universal

APP="target/DiskInfo.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
lipo -create -output "$APP/Contents/MacOS/DiskInfo" \
    target/x86_64-apple-darwin/release/diskinfo target/aarch64-apple-darwin/release/diskinfo
cp target/smartctl "$APP/Contents/MacOS/smartctl"
sed "s/@VERSION@/$VERSION/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"

# Icon: an iconset from the 1024 px picture, turned into DiskInfo.icns.
ICONSET="target/DiskInfo.iconset"
rm -rf "$ICONSET"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
    sips -z $s $s packaging/diskinfo-1024.png --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
    d=$((s * 2))
    sips -z $d $d packaging/diskinfo-1024.png --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/DiskInfo.icns"

mkdir -p "$APP/Contents/Resources/licenses"
cp LICENSE "$APP/Contents/Resources/licenses/LICENSE-DiskInfo.txt"
cp LICENSE-CrystalDiskInfo.txt "$APP/Contents/Resources/licenses/"
cp target/smartmontools-license/COPYING "$APP/Contents/Resources/licenses/COPYING-smartmontools.txt"

# Ad-hoc signature: unsigned apps on Apple silicon must carry at least this one.
codesign --force --deep --sign - "$APP"

mkdir -p dist
rm -f dist/DiskInfo-macos-universal.zip
ditto -c -k --keepParent "$APP" dist/DiskInfo-macos-universal.zip
echo "Built dist/DiskInfo-macos-universal.zip"
