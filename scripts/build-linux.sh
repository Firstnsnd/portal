#!/usr/bin/env bash
set -euo pipefail

# Build Portal Linux packages (.deb and .rpm)
#
# Usage:
#   ./scripts/build-linux.sh [version]
#
# Examples:
#   ./scripts/build-linux.sh              # Auto-detect version from Cargo.toml
#   ./scripts/build-linux.sh 0.10.4       # Use specific version

# Get version from argument or Cargo.toml
if [ -n "${1:-}" ]; then
    VERSION="$1"
else
    VERSION="$(grep ^version Cargo.toml | head -1 | awk -F'"' '{print $2}')"
fi

ARCH="$(uname -m)"
APP_NAME="portal"
OUTPUT_DIR="target/release"

echo "==> Building ${APP_NAME} v${VERSION} for Linux (${ARCH})..."

# ── 1. Build release binary ──
echo "==> Building release binary..."
cargo build --release

if [ ! -f "target/release/${APP_NAME}" ]; then
    echo "ERROR: target/release/${APP_NAME} not found. Build may have failed."
    exit 1
fi

# ── 2. Build .deb package ──
if command -v cargo-deb &> /dev/null; then
    echo "==> Building .deb package..."
    cargo deb --no-build

    DEB_FILE="target/debian/${APP_NAME}_${VERSION}_amd64.deb"
    if [ -f "$DEB_FILE" ]; then
        OUTPUT_DEB="${OUTPUT_DIR}/${APP_NAME}-${VERSION}-amd64.deb"
        cp "$DEB_FILE" "$OUTPUT_DEB"
        echo "==> Created: $OUTPUT_DEB"
    else
        echo "WARNING: .deb file not found at expected location"
    fi
else
    echo "==> cargo-deb not found, skipping .deb build"
    echo "    Install with: cargo install cargo-deb"
fi

# ── 3. Build .rpm package ──
if command -v cargo-generate-rpm &> /dev/null; then
    echo "==> Building .rpm package..."
    cargo generate-rpm --no-build

    RPM_FILE="target/generate-rpm/${APP_NAME}-${VERSION}-1.x86_64.rpm"
    if [ -f "$RPM_FILE" ]; then
        OUTPUT_RPM="${OUTPUT_DIR}/${APP_NAME}-${VERSION}-x86_64.rpm"
        cp "$RPM_FILE" "$OUTPUT_RPM"
        echo "==> Created: $OUTPUT_RPM"
    else
        echo "WARNING: .rpm file not found at expected location"
    fi
else
    echo "==> cargo-generate-rpm not found, skipping .rpm build"
    echo "    Install with: cargo install cargo-generate-rpm"
fi

# ── 4. Summary ──
echo ""
echo "==> Linux packages built successfully!"
echo ""
echo "Output files:"
if [ -f "${OUTPUT_DEB:-}" ]; then
    echo "  .deb:  $OUTPUT_DEB"
fi
if [ -f "${OUTPUT_RPM:-}" ]; then
    echo "  .rpm:  $OUTPUT_RPM"
fi
echo ""
echo "Install with:"
echo "  Debian/Ubuntu:  sudo dpkg -i ${OUTPUT_DEB:-<package.deb>}"
echo "  Fedora/RHEL:    sudo rpm -i ${OUTPUT_RPM:-<package.rpm>}"
