#!/usr/bin/env bash
set -euo pipefail

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

VERSION="0.10.0"
ARCH="$(uname -m)"
APP_NAME="Portal"
DMG_NAME="${APP_NAME}-${VERSION}-${ARCH}.dmg"
BUNDLE_DIR="target/release/bundle/osx"
APP_PATH="${BUNDLE_DIR}/${APP_NAME}.app"
DMG_OUTPUT="target/release/${DMG_NAME}"
STAGING_DIR="target/release/dmg-staging"

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

# ── 4. Notarize (optional) ──
if [ -n "${SIGNING_IDENTITY}" ] && [ -n "${APPLE_ID}" ] && [ -n "${APPLE_TEAM_ID}" ] && [ -n "${APPLE_PASSWORD}" ]; then
    echo "==> Submitting for notarization..."
    xcrun notarytool submit "${DMG_OUTPUT}" \
        --apple-id "${APPLE_ID}" \
        --team-id "${APPLE_TEAM_ID}" \
        --password "${APPLE_PASSWORD}" \
        --wait

    echo "==> Stapling notarization ticket..."
    xcrun stapler staple "${DMG_OUTPUT}"
    echo "==> Notarization complete!"
else
    if [ -n "${SIGNING_IDENTITY}" ]; then
        echo ""
        echo "    NOTE: DMG is signed but NOT notarized."
        echo "    Set APPLE_ID, APPLE_TEAM_ID, APPLE_PASSWORD to enable notarization."
    fi
fi

echo "==> Done! DMG created at: ${DMG_OUTPUT}"
