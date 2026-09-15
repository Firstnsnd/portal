#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Build Portal.app, code-sign it, and package into a .dmg installer.
#
# Code Signing:
#   SIGNING_IDENTITY  - Developer ID identity for distribution to other Macs.
#                       e.g. "Developer ID Application: Your Name (TEAMID)"
#                       If unset, falls back to ad-hoc signing (allows install
#                       via right-click > Open, but Gatekeeper still warns).
#
# Notarization (requires Apple Developer account):
#   APPLE_ID          - Apple ID email
#   APPLE_TEAM_ID     - Team ID (10-char)
#   APPLE_PASSWORD    - App-specific password (NOT your Apple ID password)
#                       Generate at https://appleid.apple.com/account/manage
#
# Examples:
#   # Ad-hoc sign (no Apple Developer account needed):
#   ./scripts/build-dmg.sh
#
#   # Developer ID sign:
#   SIGNING_IDENTITY="Developer ID Application: Name (TEAMID)" ./scripts/build-dmg.sh
#
#   # Developer ID sign + notarize:
#   SIGNING_IDENTITY="Developer ID Application: Name (TEAMID)" \
#   APPLE_ID="you@example.com" APPLE_TEAM_ID="XXXX" APPLE_PASSWORD="xxxx-xxxx-xxxx-xxxx" \
#   ./scripts/build-dmg.sh

# Get version from $VERSION (set by the CI workflow from the release tag),
# falling back to the latest git tag, then "0.0.0" if no tags exist.
# Remove 'v' prefix if present (e.g., "v0.10.0" -> "0.10.0")
VERSION="${VERSION:-$(git describe --tags --abbrev=0 2>/dev/null | sed 's/^v//')}"
VERSION="${VERSION:-0.0.0}"
ARCH="$(uname -m)"
APP_NAME="Portal"
DMG_NAME="${APP_NAME}-${VERSION}-${ARCH}.dmg"
PKG_NAME="${APP_NAME}-${VERSION}-${ARCH}.pkg"
BUNDLE_DIR="target/release/bundle/osx"
APP_PATH="${BUNDLE_DIR}/${APP_NAME}.app"
DMG_OUTPUT="target/release/${DMG_NAME}"
PKG_OUTPUT="target/release/${PKG_NAME}"
STAGING_DIR="target/release/dmg-staging"
PKG_ROOT="target/release/pkg-root"
PKG_SCRIPTS="target/release/pkg-scripts"

SIGNING_IDENTITY="${SIGNING_IDENTITY:-}"
APPLE_ID="${APPLE_ID:-}"
APPLE_TEAM_ID="${APPLE_TEAM_ID:-}"
APPLE_PASSWORD="${APPLE_PASSWORD:-}"

# ── 1. Build ──
echo "==> Building ${APP_NAME}.app (release)..."
cargo bundle --format osx --release

if [ ! -d "${APP_PATH}" ]; then
    echo "ERROR: ${APP_PATH} not found. cargo bundle may have failed."
    exit 1
fi

# ── 2. Code Sign ──
if [ -n "${SIGNING_IDENTITY}" ]; then
    echo "==> Signing with: ${SIGNING_IDENTITY}"
    codesign --force --deep --options runtime \
        --sign "${SIGNING_IDENTITY}" \
        "${APP_PATH}"
    echo "==> Verifying signature..."
    codesign --verify --verbose=2 "${APP_PATH}"
else
    echo "==> No SIGNING_IDENTITY set, using ad-hoc signature..."
    echo "    (Other Macs can install via right-click > Open)"
    codesign --force --deep --sign - "${APP_PATH}"
fi

# ── 3. Create DMG ──
echo "==> Preparing DMG staging directory..."
rm -rf "${STAGING_DIR}"
mkdir -p "${STAGING_DIR}"
cp -R "${APP_PATH}" "${STAGING_DIR}/"
ln -s /Applications "${STAGING_DIR}/Applications"

echo "==> Creating ${DMG_NAME}..."
rm -f "${DMG_OUTPUT}"
hdiutil create \
    -volname "${APP_NAME}" \
    -srcfolder "${STAGING_DIR}" \
    -ov \
    -format UDZO \
    "${DMG_OUTPUT}"

rm -rf "${STAGING_DIR}"

# Sign the DMG itself (if using Developer ID)
if [ -n "${SIGNING_IDENTITY}" ]; then
    echo "==> Signing DMG..."
    codesign --force --sign "${SIGNING_IDENTITY}" "${DMG_OUTPUT}"
fi

# ── 3b. Create pkg installer ─────────────────────────────────────────
# The .pkg installs to /Applications and runs preinstall.sh, which stops any
# running Portal and removes the old copy first — so an "update" actually
# replaces the executable instead of silently keeping the old version running.
echo "==> Preparing pkg root (${PKG_ROOT})..."
rm -rf "${PKG_ROOT}" "${PKG_SCRIPTS}"
mkdir -p "${PKG_ROOT}" "${PKG_SCRIPTS}"
cp -R "${APP_PATH}" "${PKG_ROOT}/"

echo "==> Installing preinstall script..."
cp "${SCRIPT_DIR}/pkg/preinstall.sh" "${PKG_SCRIPTS}/preinstall"
chmod +x "${PKG_SCRIPTS}/preinstall"

echo "==> Building ${PKG_NAME}..."
rm -f "${PKG_OUTPUT}"
PKGBUILD_ARGS=(--root "${PKG_ROOT}" --scripts "${PKG_SCRIPTS}" --identifier "com.portal.app" --version "${VERSION}" --install-location /Applications)
if [ -n "${SIGNING_IDENTITY}" ]; then
    PKGBUILD_ARGS+=(--sign "${SIGNING_IDENTITY}")
fi
pkgbuild "${PKGBUILD_ARGS[@]}" "${PKG_OUTPUT}"

rm -rf "${PKG_ROOT}" "${PKG_SCRIPTS}"

# ── 4. Notarize (optional) ──
if [ -n "${SIGNING_IDENTITY}" ] && [ -n "${APPLE_ID}" ] && [ -n "${APPLE_TEAM_ID}" ] && [ -n "${APPLE_PASSWORD}" ]; then
    echo "==> Submitting for notarization..."
    xcrun notarytool submit "${DMG_OUTPUT}" "${PKG_OUTPUT}" \
        --apple-id "${APPLE_ID}" \
        --team-id "${APPLE_TEAM_ID}" \
        --password "${APPLE_PASSWORD}" \
        --wait

    echo "==> Stapling notarization ticket..."
    xcrun stapler staple "${DMG_OUTPUT}"
    xcrun stapler staple "${PKG_OUTPUT}"
    echo "==> Notarization complete!"
else
    if [ -n "${SIGNING_IDENTITY}" ]; then
        echo ""
        echo "    NOTE: DMG/pkg are signed but NOT notarized."
        echo "    Set APPLE_ID, APPLE_TEAM_ID, APPLE_PASSWORD to enable notarization."
    fi
fi

echo "==> Done! DMG created at: ${DMG_OUTPUT}"
echo "             pkg created at: ${PKG_OUTPUT}"
