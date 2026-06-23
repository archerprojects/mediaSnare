#!/bin/bash
# build-deb.sh — assemble and build the mediasnare .deb
# Args: $1=build_root $2=dist_dir
set -e

BUILD_ROOT="$1"
DIST_DIR="$2"
SRC_ROOT="$(dirname "$0")/.."

# Version from Cargo.toml
VERSION=$(grep '^version' "${SRC_ROOT}/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')

PKG="mediasnare_${VERSION}_amd64"
STAGING="${BUILD_ROOT}/${PKG}"

mkdir -p "${STAGING}/DEBIAN"
mkdir -p "${STAGING}/usr/bin"
mkdir -p "${STAGING}/usr/share/applications"
mkdir -p "${STAGING}/usr/share/glib-2.0/schemas"
mkdir -p "${STAGING}/usr/share/mediasnare"
mkdir -p "${STAGING}/usr/share/icons/hicolor/scalable/apps"
mkdir -p "${STAGING}/usr/share/icons/lean-icons/apps/scalable"

for size in 16x16 22x22 24x24 32x32 48x48 64x64 128x128 256x256; do
    mkdir -p "${STAGING}/usr/share/icons/hicolor/${size}/apps"
done

# binary
BINARY="${SRC_ROOT}/target/debug/mediasnare"
if [ ! -f "$BINARY" ]; then
    BINARY="${SRC_ROOT}/target/release/mediasnare"
fi
cp "$BINARY" "${STAGING}/usr/bin/mediasnare"
chmod 755 "${STAGING}/usr/bin/mediasnare"

# GResource bundle
cp "${BUILD_ROOT}/mediasnare.gresource" "${STAGING}/usr/share/mediasnare/"

# desktop file
cp "${SRC_ROOT}/data/mediasnare.desktop" "${STAGING}/usr/share/applications/"

# GSettings schema
cp "${SRC_ROOT}/data/org.archerprojects.mediaSnare.gschema.xml" \
    "${STAGING}/usr/share/glib-2.0/schemas/"

# icons — SVG
cp "${SRC_ROOT}/data/icons/mediasnare.svg" \
    "${STAGING}/usr/share/icons/hicolor/scalable/apps/mediasnare.svg"
cp "${SRC_ROOT}/data/icons/mediasnare.svg" \
    "${STAGING}/usr/share/icons/lean-icons/apps/scalable/mediasnare.svg"

# icons — PNG all sizes
for size in 16x16 22x22 24x24 32x32 48x48 64x64 128x128 256x256; do
    src="${SRC_ROOT}/data/icons/hicolor/${size}/apps/mediasnare.png"
    if [ -f "$src" ]; then
        cp "$src" "${STAGING}/usr/share/icons/hicolor/${size}/apps/mediasnare.png"
    fi
done

cat > "${STAGING}/DEBIAN/control" << CTRL
Package: mediasnare
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: amd64
Maintainer: archerprojects <archer.projects@proton.me>
Depends: libgtk-4-1, libadwaita-1-0, libgstreamer1.0-0,
 gstreamer1.0-plugins-base, gstreamer1.0-plugins-good,
 gstreamer1.0-plugins-ugly, gstreamer1.0-pipewire,
 libpipewire-0.3-0, xdg-desktop-portal
Recommends: gstreamer1.0-vaapi, wmctrl, xdotool
Description: Screen, video, and audio capture
 mediaSnare captures screenshots, screen recordings with audio, and
 standalone audio. X11 and Wayland ready via PipeWire.
 Developed for Lean Linux by archerprojects.
CTRL

cp "${SRC_ROOT}/debian/postinst" "${STAGING}/DEBIAN/postinst"
chmod 755 "${STAGING}/DEBIAN/postinst"

mkdir -p "${DIST_DIR}"
dpkg-deb --build --root-owner-group "${STAGING}" "${DIST_DIR}/${PKG}.deb"

echo "  Built: ${DIST_DIR}/${PKG}.deb"
