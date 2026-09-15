#!/usr/bin/env bash
set -euo pipefail

# pkg postinstall: ensure the freshly-installed executable is marked runnable
# and the bundle metadata is consistent, so the app launches cleanly after an
# update. (pkgbuild copies files but doesn't guarantee the +x bit survives
# cross-arch/packing round-trips.)

APP_DIR="/Applications/Portal.app"
APP_BIN="${APP_DIR}/Contents/MacOS/portal"

if [ -f "${APP_BIN}" ]; then
    chmod +x "${APP_BIN}" 2>/dev/null || true
    echo "==> Post-install: ensured ${APP_BIN} is executable."
fi

# Touch the bundle so LaunchServices re-registers it (avoids a stale cached
# icon/version after an in-place upgrade).
touch "${APP_DIR}" 2>/dev/null || true

exit 0
