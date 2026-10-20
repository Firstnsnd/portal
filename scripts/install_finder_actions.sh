#!/usr/bin/env bash
set -euo pipefail

# Install Finder Quick Actions "New Portal Tab Here" / "New Portal Window Here"
# into ~/Library/Services. After running this, right-click a folder in Finder
# → Quick Actions (or the right-click menu directly on newer macOS).
#
# The actions invoke the installed Portal app via
#   open -nb com.portal.app --args --open-tab|--open-window <dir>
# A second instance forwards the request to the running one over IPC and
# exits immediately (see src/ipc.rs).
#
# Dev override: PORTAL_BIN=/path/to/portal ./scripts/install_finder_actions.sh
# makes the actions run your dev binary instead of the installed app.
#
# Uninstall: ./scripts/install_finder_actions.sh --uninstall

BUNDLE_ID="com.portal.app"
SERVICES_DIR="$HOME/Library/Services"

uninstall() {
    rm -rf "$SERVICES_DIR/New Portal Tab Here.workflow" \
           "$SERVICES_DIR/New Portal Window Here.workflow"
    echo "Removed Portal Finder Quick Actions."
    exit 0
}

[ "${1:-}" = "--uninstall" ] && uninstall

# Resolve PORTAL_BIN to an absolute path — the workflow runs with Finder's
# cwd, so a relative path would silently break.
if [ -n "${PORTAL_BIN:-}" ]; then
    PORTAL_BIN="$(cd "$(dirname "$PORTAL_BIN")" && pwd)/$(basename "$PORTAL_BIN")"
    if [ ! -x "$PORTAL_BIN" ]; then
        echo "error: PORTAL_BIN is not executable: $PORTAL_BIN" >&2
        exit 1
    fi
fi

# The Run Shell Script body. Receives the folder as $1 (inputMethod=1:
# "as arguments"). XML-escaped on the way into the .wflow below.
make_script() {
    local flag="$1"
    if [ -n "${PORTAL_BIN:-}" ]; then
        printf 'if [ -d "$1" ]; then\n  "%s" %s "$1" &\nfi\n' "$PORTAL_BIN" "$flag"
    else
        printf 'if [ -d "$1" ]; then\n  open -nb %s --args %s "$1"\nfi\n' "$BUNDLE_ID" "$flag"
    fi
}

tab_script="$(make_script --open-tab)"
window_script="$(make_script --open-window)"

xml_escape() {
    sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g'
}

# Render a minimal Automator service workflow that runs a shell script with
# the selected folder passed as an argument.
make_wflow() {
    local name="$1" script="$2" uuid
    uuid="$(uuidgen)"
    local escaped
    escaped="$(printf '%s' "$script" | xml_escape)"
    mkdir -p "$SERVICES_DIR/$name.workflow/Contents"
    cat > "$SERVICES_DIR/$name.workflow/Contents/document.wflow" <<WFLOW
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>AMDocumentVersion</key>
	<string>2</string>
	<key>actions</key>
	<array>
		<dict>
			<key>action</key>
			<dict>
				<key>AMAccepts</key>
				<dict>
					<key>Container</key>
					<string>List</string>
					<key>Optional</key>
					<true/>
					<key>Types</key>
					<array>
						<string>com.apple.cocoa.path</string>
					</array>
				</dict>
				<key>AMActionVersion</key>
				<string>2.0.3</string>
				<key>AMApplication</key>
				<array>
					<string>Automator</string>
				</array>
				<key>AMParameterProperties</key>
				<dict>
					<key>COMMAND_STRING</key>
					<dict/>
					<key>CheckedForUserDefaultShell</key>
					<dict/>
					<key>inputMethod</key>
					<dict/>
					<key>shell</key>
					<dict/>
					<key>source</key>
					<dict/>
				</dict>
				<key>AMProvides</key>
				<dict>
					<key>Container</key>
					<string>List</string>
					<key>Types</key>
					<array>
						<string>com.apple.cocoa.path</string>
					</array>
				</dict>
				<key>ActionBundlePath</key>
				<string>/System/Library/Automator/Run Shell Script.action</string>
				<key>ActionName</key>
				<string>Run Shell Script</string>
				<key>ActionParameters</key>
				<dict>
					<key>COMMAND_STRING</key>
					<string>$escaped</string>
					<key>CheckedForUserDefaultShell</key>
					<true/>
					<key>inputMethod</key>
					<integer>1</integer>
					<key>shell</key>
					<string>/bin/sh</string>
					<key>source</key>
					<string></string>
				</dict>
				<key>Application</key>
				<array>
					<string>Automator</string>
				</array>
				<key>UUID</key>
				<string>$uuid</string>
			</dict>
			<key>isViewVisible</key>
			<integer>0</integer>
		</dict>
	</array>
	<key>connectors</key>
	<dict/>
	<key>workflowMetaData</key>
	<dict>
		<key>serviceApplicationBundleID</key>
		<string>$BUNDLE_ID</string>
		<key>workflowTypeIdentifier</key>
		<string>com.apple.Automator.servicesMenu</string>
	</dict>
</dict>
</plist>
WFLOW
}

make_wflow "New Portal Tab Here" "$tab_script"
make_wflow "New Portal Window Here" "$window_script"

# Refresh the Services registry so the actions show up without logging out.
PBS=/System/Library/CoreServices/pbs
if [ -x "$PBS" ]; then
    "$PBS" -update 2>/dev/null || true
fi

echo "Installed Finder Quick Actions:"
echo "  $SERVICES_DIR/New Portal Tab Here.workflow"
echo "  $SERVICES_DIR/New Portal Window Here.workflow"
echo "Right-click a folder in Finder → Quick Actions. (If they don't appear at once, they will after a re-login.)"
