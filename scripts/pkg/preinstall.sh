#!/usr/bin/env bash
set -euo pipefail

# pkg preinstall script: stop any running Portal and remove the old copy in
# /Applications before the new bundle is installed. This avoids the classic
# "installed but old version still running" problem where replacing a
# running executable is blocked by the OS.

APP_NAME="Portal"
APP_DIR="/Applications/${APP_NAME}.app"
BUNDLE_ID="com.portal.app"

echo "==> Stopping running ${APP_NAME} instances..."
# killall by bundle id is most reliable; tolerate failure if nothing matches.
pkill -x portal 2>/dev/null || true
osascript -e "tell application id \"${BUNDLE_ID}\" to quit" 2>/dev/null || true

# Give the app a moment to actually release its files.
sleep 1

echo "==> Removing previous ${APP_NAME}.app..."
rm -rf "${APP_DIR}" 2>/dev/null || true

exit 0
