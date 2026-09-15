#!/usr/bin/env bash
set -euo pipefail

# pkg preinstall script: stop any running Portal and remove the old copy in
# /Applications before the new bundle is installed.
#
# Why this exists: macOS locks the executable inside a running .app, so a plain
# drag-to-/Applications replace silently fails to swap the binary — the old
# version keeps running. To make an "update" actually replace the app, we must
# (1) quit every running instance, (2) wait for the binary to be released, and
# (3) remove the old bundle before the pkg payload is copied in.

APP_NAME="Portal"
APP_BIN="portal"           # actual executable name ([[bin]] name = "portal")
APP_DIR="/Applications/${APP_NAME}.app"
BUNDLE_ID="com.portal.app"

quit_via_identifier() {
    osascript -e "tell application id \"${BUNDLE_ID}\" to quit" 2>/dev/null || true
}

# Terminate by process name. `pkill -x portal` matches the exact executable
# name (case-sensitive), while `pkill -f Portal.app` catches copies launched
# from a non-standard path (e.g. ~/Downloads) via the full command line.
terminate_processes() {
    pkill -x "${APP_BIN}" 2>/dev/null || true
    pkill -if "Portal.app" 2>/dev/null || true
}

echo "==> Stopping running ${APP_NAME} instances..."

# First ask nicely via Apple Events (allows a graceful save/exit).
quit_via_identifier

# Then signal the process tree directly.
terminate_processes

# Wait up to ~5s for the binary to actually be released. Give the OS time to
# reap the process so the executable is no longer held before we remove it.
for _ in 1 2 3 4 5; do
    if ! pgrep -x "${APP_BIN}" >/dev/null 2>&1 && ! pgrep -f "Portal.app" >/dev/null 2>&1; then
        break
    fi
    sleep 1
done

# Final, forceful fallback for any straggler still holding the binary.
pkill -9 -x "${APP_BIN}" 2>/dev/null || true
pkill -9 -f "Portal.app" 2>/dev/null || true
sleep 1

echo "==> Removing previous ${APP_NAME}.app..."
rm -rf "${APP_DIR}" 2>/dev/null || true

# Belt-and-braces: if the bundle still exists (e.g. permissions), fail loudly
# rather than silently keeping a stale app over the new one.
if [ -e "${APP_DIR}" ]; then
    echo "ERROR: could not remove ${APP_DIR}; aborting install." >&2
    exit 1
fi

echo "==> Previous version removed; proceeding with install."
exit 0
