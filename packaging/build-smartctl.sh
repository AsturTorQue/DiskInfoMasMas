#!/bin/sh
# Builds smartctl from the official smartmontools source, for bundling in the AppImage.
# Output: target/smartctl (needs only the C/C++ runtime every desktop distribution has).
# Usage: packaging/build-smartctl.sh
set -eu
cd "$(dirname "$0")/.."

VERSION=7.5
TAG=RELEASE_7_5
# Matches the MD5 published with the release (38c38b0b82db7fc4906cdd50d15a7931).
SHA256=690b83ca331378da9ea0d9d61008c4b22dde391387b9bbad7f29387f2595f76e
URL="https://github.com/smartmontools/smartmontools/releases/download/${TAG}/smartmontools-${VERSION}.tar.gz"

if [ -x target/smartctl ] && target/smartctl --version | grep -q "smartctl ${VERSION}"; then
    echo "target/smartctl ${VERSION} already built"
    exit 0
fi

WORK="target/smartmontools-build"
mkdir -p "$WORK"
TARBALL="$WORK/smartmontools-${VERSION}.tar.gz"
[ -f "$TARBALL" ] || curl -fsSL -o "$TARBALL" "$URL"
echo "${SHA256}  ${TARBALL}" | sha256sum -c -

tar -xzf "$TARBALL" -C "$WORK"
cd "$WORK/smartmontools-${VERSION}"
./configure --quiet --without-libsystemd --without-selinux --without-libcap-ng \
    --without-nvme-devicescan --with-drivedbdir=no --with-update-smart-drivedb=no \
    CXXFLAGS="-O2" LDFLAGS="-static-libgcc"
make -s smartctl
strip smartctl
cp smartctl ../../smartctl
mkdir -p ../../smartmontools-license
cp COPYING ../../smartmontools-license/COPYING
echo "Built $(../../smartctl --version | head -1)"
