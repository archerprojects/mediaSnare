#!/bin/bash
# cargo-build.sh — build Rust binary via meson custom_target
set -e

SRC_ROOT="$1"
RUST_TARGET="$2"
CARGO_OPTIONS="$3"
OUTPUT="$5"

cd "$SRC_ROOT"

# Read version from Cargo.toml
VERSION=$(grep '^version' "${SRC_ROOT}/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')

export APP_ID="org.archerprojects.mediaSnare"
export VERSION
export PKGDATADIR="/usr/share/mediasnare"

if [ -n "$CARGO_OPTIONS" ]; then
    cargo build $CARGO_OPTIONS --manifest-path "$SRC_ROOT/Cargo.toml"
else
    cargo build --manifest-path "$SRC_ROOT/Cargo.toml"
fi

BINARY="$SRC_ROOT/target/$RUST_TARGET/mediasnare"
mkdir -p "$(dirname "$OUTPUT")"
cp "$BINARY" "$OUTPUT"
chmod 755 "$OUTPUT"
