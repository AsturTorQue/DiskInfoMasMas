#!/bin/sh
# Builds smartctl from the official smartmontools source, for bundling with DiskInfo.
#
#   packaging/build-smartctl.sh                 -> target/smartctl      (this machine)
#   packaging/build-smartctl.sh windows-x86_64  -> target/smartctl.exe  (needs mingw-w64)
#   packaging/build-smartctl.sh macos-universal -> target/smartctl      (Intel + Apple silicon, run on a Mac)
#
# The result needs only the C/C++ runtime every system has.
set -eu
cd "$(dirname "$0")/.."
FLAVOUR="${1:-native}"

VERSION=7.5
TAG=RELEASE_7_5
# Matches the MD5 published with the release (38c38b0b82db7fc4906cdd50d15a7931).
SHA256=690b83ca331378da9ea0d9d61008c4b22dde391387b9bbad7f29387f2595f76e
URL="https://github.com/smartmontools/smartmontools/releases/download/${TAG}/smartmontools-${VERSION}.tar.gz"

CONFIGURE_ARGS="--without-libsystemd --without-selinux --without-libcap-ng --with-drivedbdir=no --with-update-smart-drivedb=no"
OUT=smartctl
case "$FLAVOUR" in
    native)
        set -- CXXFLAGS="-O2" LDFLAGS="-static-libgcc"
        ;;
    windows-x86_64)
        OUT=smartctl.exe
        CONFIGURE_ARGS="$CONFIGURE_ARGS --host=x86_64-w64-mingw32"
        set -- CXXFLAGS="-O2" LDFLAGS="-static -static-libgcc -static-libstdc++"
        ;;
    macos-universal)
        set -- CXX="clang++ -arch x86_64 -arch arm64" CC="clang -arch x86_64 -arch arm64" CXXFLAGS="-O2 -mmacosx-version-min=11.0"
        CONFIGURE_ARGS="$CONFIGURE_ARGS --disable-dependency-tracking"
        ;;
    *)
        echo "unknown flavour: $FLAVOUR" >&2
        exit 2
        ;;
esac

if [ "$FLAVOUR" = native ] && [ -x "target/$OUT" ] && "target/$OUT" --version | grep -q "smartctl ${VERSION}"; then
    echo "target/$OUT ${VERSION} already built"
    exit 0
fi

WORK="target/smartmontools-build-$FLAVOUR"
mkdir -p "$WORK"
TARBALL="target/smartmontools-${VERSION}.tar.gz"
[ -f "$TARBALL" ] || curl -fsSL -o "$TARBALL" "$URL"
if command -v sha256sum >/dev/null; then
    echo "${SHA256}  ${TARBALL}" | sha256sum -c -
else
    echo "${SHA256}  ${TARBALL}" | shasum -a 256 -c -
fi

tar -xzf "$TARBALL" -C "$WORK"
cd "$WORK/smartmontools-${VERSION}"
# shellcheck disable=SC2086
./configure --quiet $CONFIGURE_ARGS "$@"
make -s "$OUT"
case "$FLAVOUR" in
    windows-x86_64) x86_64-w64-mingw32-strip "$OUT" ;;
    macos-universal) strip -x "$OUT" ;;
    *) strip "$OUT" ;;
esac
cp "$OUT" "../../$OUT"
mkdir -p ../../smartmontools-license
cp COPYING ../../smartmontools-license/COPYING
echo "Built target/$OUT"
