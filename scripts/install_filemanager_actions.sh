#!/usr/bin/env bash
set -euo pipefail

# Linux file-manager integration for Portal's "open here" feature:
#
#   • GNOME Files (Nautilus) — scripts in ~/.local/share/nautilus/scripts/
#     Right-click → Scripts → New Portal Tab/Window Here.
#   • Dolphin (KDE) — service menu in ~/.local/share/kio/servicemenus/
#     Right-click a folder → Actions → New Portal Tab/Window Here.
#
# Both invoke:  portal --open-tab|--open-window <dir>
# A second instance forwards the request to the running one over IPC and
# exits immediately (see src/ipc.rs).
#
# Override the binary with PORTAL_BIN=/path/to/portal.
# Uninstall: ./scripts/install_filemanager_actions.sh --uninstall

PORTAL_BIN="${PORTAL_BIN:-portal}"

NAUTILUS_DIR="$HOME/.local/share/nautilus/scripts"
KIO_DIR="$HOME/.local/share/kio/servicemenus"
# KDE < 5.89 reads service menus from kservices5 instead of kio.
KSERVICES5_DIR="$HOME/.local/share/kservices5/ServiceMenus"

uninstall() {
    rm -f "$NAUTILUS_DIR/New Portal Tab Here" \
          "$NAUTILUS_DIR/New Portal Window Here" \
          "$KIO_DIR/portal_here.desktop" \
          "$KSERVICES5_DIR/portal_here.desktop"
    echo "Removed Portal file-manager actions."
    exit 0
}

[ "${1:-}" = "--uninstall" ] && uninstall

# ── Nautilus scripts ────────────────────────────────────────────────────────
# Nautilus exposes the current folder as a file:// URI and the selection as
# absolute paths (one per line). A folder selection wins; else use the
# current folder. %XX sequences are decoded when python3 is available.
write_nautilus_script() {
    local name="$1" flag="$2"
    mkdir -p "$NAUTILUS_DIR"
    cat > "$NAUTILUS_DIR/$name" <<SCRIPT
#!/bin/sh
# First selected path (one per line); a folder selection wins.
sel=\$(printf '%s\n' "\$NAUTILUS_SCRIPT_SELECTED_FILE_PATHS" | head -n 1)
if [ -n "\$sel" ] && [ -d "\$sel" ]; then
    dir="\$sel"
else
    uri="\${NAUTILUS_SCRIPT_CURRENT_URI#file://}"
    if command -v python3 >/dev/null 2>&1; then
        dir="\$(python3 -c 'import sys,urllib.parse; print(urllib.parse.unquote(sys.argv[1]))' "\$uri")"
    else
        dir="\$uri"
    fi
fi
if [ -d "\$dir" ]; then
    '$PORTAL_BIN' $flag "\$dir" &
fi
SCRIPT
    chmod +x "$NAUTILUS_DIR/$name"
}

write_nautilus_script "New Portal Tab Here" --open-tab
write_nautilus_script "New Portal Window Here" --open-window

# ── Dolphin service menu ────────────────────────────────────────────────────
desktop_entry="[Desktop Entry]
Type=Service
X-KDE-ServiceTypes=inode/directory
Actions=portalTabHere;portalWindowHere;

[Desktop Action portalTabHere]
Name=New Portal Tab Here
Exec=$PORTAL_BIN --open-tab %f
Icon=utilities-terminal

[Desktop Action portalWindowHere]
Name=New Portal Window Here
Exec=$PORTAL_BIN --open-window %f
Icon=utilities-terminal
"

if [ -d "$HOME/.local/share/kio" ] || [ ! -d "$HOME/.local/share/kservices5" ]; then
    mkdir -p "$KIO_DIR"
    printf '%s' "$desktop_entry" > "$KIO_DIR/portal_here.desktop"
    installed_menu="$KIO_DIR/portal_here.desktop"
else
    mkdir -p "$KSERVICES5_DIR"
    printf '%s' "$desktop_entry" > "$KSERVICES5_DIR/portal_here.desktop"
    installed_menu="$KSERVICES5_DIR/portal_here.desktop"
fi

# Reload Nautilus' script list if it's running.
command -v nautilus >/dev/null 2>&1 && nautilus -q 2>/dev/null || true

echo "Installed:"
echo "  $NAUTILUS_DIR/New Portal Tab Here"
echo "  $NAUTILUS_DIR/New Portal Window Here"
echo "  $installed_menu"
