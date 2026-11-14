//! # File-Manager Integration ("Open new Portal tab/window here")
//!
//! Installs the OS file-manager right-click actions that feed Portal's
//! single-instance IPC ([`crate::ipc`]):
//!
//! - **macOS Finder**: Automator Quick Action bundles
//!   (`New Portal {Tab,Window} Here.workflow`) in `~/Library/Services`. A
//!   working install needs THREE pieces, and the original v0.15.2 scripts
//!   produced none of them properly:
//!   1. `Contents/document.wflow` (the Run Shell Script action) with correct
//!      `workflowMetaData` (input source = Finder, fileSystemObject in/out),
//!   2. `Contents/Info.plist` declaring the `NSServices` entry — without it
//!      the Services daemon (`pbs`) never registers the bundle at all,
//!   3. an enablement entry in the `pbs` defaults domain
//!      (`NSServicesStatus`) — on modern macOS a registered-but-unlisted
//!      service stays out of Finder's context menu.
//! - **Linux**: Nautilus scripts (`~/.local/share/nautilus/scripts/`) and a
//!   Dolphin/KDE service menu (`~/.local/share/kio/servicemenus/`).
//!
//! Every action ends in `--open-tab|--open-window <dir>`; a freshly
//! launched instance forwards the request to the running one over the
//! socket and exits (see ipc.rs).

use std::path::PathBuf;

const BUNDLE_ID: &str = "com.portal.app";

/// Whether the file-manager actions are installed — `Stale` means files from
/// an older layout exist that never register (e.g. pre-Info.plist bundles);
/// reinstalling fixes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegrationStatus {
    NotInstalled,
    Installed,
    Stale,
}

/// Install the file-manager actions for the current OS.
pub fn install() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        install_macos()?;
        refresh_services_cache();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        install_linux()?;
    }
    #[cfg(not(unix))]
    {
        return Err("file-manager integration is not supported on this OS".to_string());
    }
    Ok(())
}

/// Remove the file-manager actions.
pub fn uninstall() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let dir = services_dir()?;
        for name in ["New Portal Tab Here", "New Portal Window Here"] {
            let _ = std::fs::remove_dir_all(dir.join(format!("{name}.workflow")));
        }
        refresh_services_cache();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let home = home_dir()?;
        let _ = std::fs::remove_file(home.join(".local/share/nautilus/scripts/New Portal Tab Here"));
        let _ = std::fs::remove_file(home.join(".local/share/nautilus/scripts/New Portal Window Here"));
        let _ = std::fs::remove_file(home.join(".local/share/kio/servicemenus/portal_here.desktop"));
        let _ = std::fs::remove_file(home.join(".local/share/kservices5/ServiceMenus/portal_here.desktop"));
    }
    Ok(())
}

/// Current install state, for the Settings UI.
pub fn status() -> IntegrationStatus {
    let Ok(home) = home_dir() else {
        return IntegrationStatus::NotInstalled;
    };
    #[cfg(target_os = "macos")]
    {
        let contents = home
            .join("Library/Services/New Portal Tab Here.workflow")
            .join("Contents");
        match (contents.join("Info.plist").exists(), contents.join("document.wflow").exists()) {
            // Both files: a bundle the Services daemon can actually register.
            (true, true) => IntegrationStatus::Installed,
            // Old layout (document.wflow only): silently ignored by pbs.
            (false, true) => IntegrationStatus::Stale,
            _ => IntegrationStatus::NotInstalled,
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let nautilus = home.join(".local/share/nautilus/scripts/New Portal Tab Here");
        let dolphin = home.join(".local/share/kio/servicemenus/portal_here.desktop");
        if nautilus.exists() || dolphin.exists() {
            IntegrationStatus::Installed
        } else {
            IntegrationStatus::NotInstalled
        }
    }
    #[cfg(not(unix))]
    {
        let _ = home;
        IntegrationStatus::NotInstalled
    }
}

// ── macOS ────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn services_dir() -> Result<PathBuf, String> {
    Ok(home_dir()?.join("Library/Services"))
}

#[cfg(target_os = "macos")]
fn home_dir() -> Result<PathBuf, String> {
    dirs::home_dir().ok_or_else(|| "cannot resolve home directory".to_string())
}

/// The Run Shell Script body for one action. `bin` (the `PORTAL_BIN` dev
/// override) makes the workflow invoke that binary directly; the default
/// routes through the installed app bundle via `open -nb`. A folder
/// selection opens as-is; a file selection opens its parent folder.
fn workflow_command(flag: &str, bin: Option<&str>) -> String {
    let invoke = match bin.filter(|b| !b.is_empty()) {
        Some(bin) => format!("\"{bin}\" {flag} \"$dir\" &"),
        None => format!("open -nb {BUNDLE_ID} --args {flag} \"$dir\""),
    };
    format!(
        "dir=\"\"\n\
         if [ -d \"$1\" ]; then\n\
         \x20 dir=\"$1\"\n\
         elif [ -f \"$1\" ]; then\n\
         \x20 dir=$(dirname \"$1\")\n\
         fi\n\
         if [ -n \"$dir\" ]; then\n\
         \x20 {invoke}\n\
         fi\n"
    )
}

#[cfg(target_os = "macos")]
fn install_macos() -> Result<(), String> {
    let dir = services_dir()?;
    let bin = std::env::var("PORTAL_BIN").ok();
    for (name, id, flag) in [
        ("New Portal Tab Here", "com.portal.app.quickaction.tab-here", "--open-tab"),
        ("New Portal Window Here", "com.portal.app.quickaction.window-here", "--open-window"),
    ] {
        let contents = dir.join(format!("{name}.workflow")).join("Contents");
        std::fs::create_dir_all(&contents)
            .map_err(|e| format!("{}: {}", contents.display(), e))?;
        std::fs::write(contents.join("document.wflow"), document_wflow(name, flag, bin.as_deref()))
            .map_err(|e| format!("document.wflow: {}", e))?;
        // The registration half: without an Info.plist declaring NSServices,
        // pbs ignores the bundle entirely; without CFBundleIdentifier Finder
        // never resolves it into the context menu.
        std::fs::write(contents.join("Info.plist"), info_plist(name, id))
            .map_err(|e| format!("Info.plist: {}", e))?;
        // Ad-hoc signature to match what the system expects of a bundle
        // (Automator bundles ship unsigned and work, so this is belt and
        // braces — failure is not fatal).
        let _ = std::process::Command::new("codesign")
            .args(["--force", "--deep", "--sign", "-"])
            .arg(dir.join(format!("{name}.workflow")))
            .output();
        // Belt and braces for enablement too: registered services with a
        // bundle identifier should appear on their own, but writing the
        // switch the System Settings toggle would write costs nothing.
        enable_service(name, id);
    }
    Ok(())
}

/// Turn on the context-menu/Services-menu switches for one workflow in the
/// `pbs` defaults domain, exactly as the System Settings Extensions toggle
/// would. The key is passed as a single argv element (no shell), and works
/// bare — only parentheses (the `(null)` slot of identifier-less bundles)
/// force plist-quoting. Best-effort: if the write fails, the user can still
/// enable the action manually in System Settings.
#[cfg(target_os = "macos")]
fn enable_service(name: &str, bundle_id: &str) {
    let key = format!("{bundle_id} - {name} - runWorkflowAsService");
    let value = "{ enabled_context_menu = 1; enabled_services_menu = 1; \
                  presentation_modes = { ContextMenu = 1; ServicesMenu = 1; }; }";
    let _ = std::process::Command::new("defaults")
        .args(["write", "pbs", "NSServicesStatus", "-dict-add", &key, value])
        .output();
}

/// Kick the Services daemon so the new actions appear without re-login.
#[cfg(target_os = "macos")]
fn refresh_services_cache() {
    let _ = std::process::Command::new("/System/Library/CoreServices/pbs").arg("-update").output();
}

/// XML-escape text embedded in a plist `<string>` (run before any template
/// interpolation; `&` first so entities aren't double-escaped).
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The Run Shell Script action document. `flag` is `--open-tab`/`--open-window`;
/// `bin` is the optional dev-override binary (see [`workflow_command`]).
/// (The action name itself lives only in Info.plist — the document is shared.)
fn document_wflow(_name: &str, flag: &str, bin: Option<&str>) -> String {
    let escaped = xml_escape(&workflow_command(flag, bin));
    let uuid = uuid::Uuid::new_v4();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
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
					<string>{escaped}</string>
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
				<string>{uuid}</string>
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
		<string>com.apple.finder</string>
		<key>serviceApplicationPath</key>
		<string>/System/Library/CoreServices/Finder.app</string>
		<key>serviceInputTypeIdentifier</key>
		<string>com.apple.Automator.fileSystemObject</string>
		<key>serviceOutputTypeIdentifier</key>
		<string>com.apple.Automator.nothing</string>
		<key>serviceProcessesInput</key>
		<integer>0</integer>
		<key>workflowTypeIdentifier</key>
		<string>com.apple.Automator.servicesMenu</string>
	</dict>
</dict>
</plist>
"#
    )
}

/// The bundle registration plist. `CFBundleIdentifier` is load-bearing:
/// Automator-generated Quick Actions carry one, and a registered service
/// without it gets keyed under a null identifier that Finder's context menu
/// never resolves — the item silently stays hidden (system bundles like
/// `Set Desktop Picture` all have one and need no manual enablement).
fn info_plist(name: &str, bundle_id: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en_US</string>
	<key>CFBundleIdentifier</key>
	<string>{bundle_id}</string>
	<key>CFBundleName</key>
	<string>{name}</string>
	<key>CFBundleShortVersionString</key>
	<string>1.0</string>
	<key>NSServices</key>
	<array>
		<dict>
			<key>NSMenuItem</key>
			<dict>
				<key>default</key>
				<string>{name}</string>
			</dict>
			<key>NSMessage</key>
			<string>runWorkflowAsService</string>
			<key>NSRequiredContext</key>
			<dict>
				<key>NSApplicationIdentifier</key>
				<string>com.apple.finder</string>
			</dict>
			<key>NSSendFileTypes</key>
			<array>
				<string>public.item</string>
			</array>
		</dict>
	</array>
</dict>
</plist>
"#
    )
}

// ── Linux ────────────────────────────────────────────────────────────────────

/// The portal binary to invoke. `current_exe` keeps dev builds working while
/// installed packages resolve to /usr/bin/portal naturally.
#[cfg(all(unix, not(target_os = "macos")))]
fn portal_bin() -> String {
    std::env::var("PORTAL_BIN")
        .ok()
        .filter(|b| !b.is_empty())
        .or_else(|| std::env::current_exe().ok().map(|p| p.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "portal".to_string())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn home_dir() -> Result<PathBuf, String> {
    dirs::home_dir().ok_or_else(|| "cannot resolve home directory".to_string())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn install_linux() -> Result<(), String> {
    use std::io::Write;
    let home = home_dir()?;
    let bin = portal_bin();

    // ── Nautilus scripts ──
    // Nautilus exposes the selection as absolute paths (one per line); a
    // folder selection wins, else the current folder (a file:// URI that
    // needs percent-decoding — python3 when present).
    let nautilus_dir = home.join(".local/share/nautilus/scripts");
    for (name, flag) in [
        ("New Portal Tab Here", "--open-tab"),
        ("New Portal Window Here", "--open-window"),
    ] {
        std::fs::create_dir_all(&nautilus_dir)
            .map_err(|e| format!("{}: {}", nautilus_dir.display(), e))?;
        let script = format!(
            r#"#!/bin/sh
# First selected path (one per line); a folder selection wins.
sel=$(printf '%s\n' "$NAUTILUS_SCRIPT_SELECTED_FILE_PATHS" | head -n 1)
if [ -n "$sel" ] && [ -d "$sel" ]; then
    dir="$sel"
else
    uri="${{NAUTILUS_SCRIPT_CURRENT_URI#file://}}"
    if command -v python3 >/dev/null 2>&1; then
        dir="$(python3 -c 'import sys,urllib.parse; print(urllib.parse.unquote(sys.argv[1]))' "$uri")"
    else
        dir="$uri"
    fi
fi
if [ -d "$dir" ]; then
    '{bin}' {flag} "$dir" &
fi
"#
        );
        let path = nautilus_dir.join(name);
        let mut f = std::fs::File::create(&path).map_err(|e| format!("{}: {}", path.display(), e))?;
        f.write_all(script.as_bytes()).map_err(|e| format!("{}: {}", path.display(), e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| format!("{}: {}", path.display(), e))?;
        }
    }

    // ── Dolphin service menu ──
    // KDE ≥ 5.89 reads from kio/servicemenus; older versions from
    // kservices5/ServiceMenus.
    let desktop_entry = format!(
        "[Desktop Entry]\n\
         Type=Service\n\
         X-KDE-ServiceTypes=inode/directory\n\
         Actions=portalTabHere;portalWindowHere;\n\
         \n\
         [Desktop Action portalTabHere]\n\
         Name=New Portal Tab Here\n\
         Exec={bin} --open-tab %f\n\
         Icon=utilities-terminal\n\
         \n\
         [Desktop Action portalWindowHere]\n\
         Name=New Portal Window Here\n\
         Exec={bin} --open-window %f\n\
         Icon=utilities-terminal\n"
    );
    let kio = home.join(".local/share/kio");
    let kservices5 = home.join(".local/share/kservices5");
    let menu_dir = if kio.exists() || !kservices5.exists() {
        kio.join("servicemenus")
    } else {
        kservices5.join("ServiceMenus")
    };
    std::fs::create_dir_all(&menu_dir).map_err(|e| format!("{}: {}", menu_dir.display(), e))?;
    std::fs::write(menu_dir.join("portal_here.desktop"), desktop_entry)
        .map_err(|e| format!("portal_here.desktop: {}", e))?;

    // Reload Nautilus' script list if it's running.
    let _ = std::process::Command::new("nautilus").arg("-q").output();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_escape_handles_plist_breakers() {
        assert_eq!(xml_escape("a & b < c > d"), "a &amp; b &lt; c &gt; d");
        // Already-escaped input must not double-escape the entity.
        assert_eq!(xml_escape("&amp;"), "&amp;amp;");
    }

    #[test]
    fn workflow_command_default_routes_through_bundle() {
        let cmd = workflow_command("--open-tab", None);
        assert!(cmd.contains("open -nb com.portal.app --args --open-tab \"$dir\""), "{cmd}");
        // Folder opens as-is; a file falls back to its parent directory.
        assert!(cmd.contains("if [ -d \"$1\" ]"), "{cmd}");
        assert!(cmd.contains("dir=$(dirname \"$1\")"), "{cmd}");
    }

    #[test]
    fn workflow_command_dev_override_invokes_binary_directly() {
        let cmd = workflow_command("--open-window", Some("/path/to/portal"));
        assert!(cmd.contains("\"/path/to/portal\" --open-window \"$dir\" &"), "{cmd}");
    }

    #[test]
    fn document_wflow_carries_fixed_metadata_and_escaped_script() {
        // The dev override embeds a literal `&` (backgrounding), which must
        // survive XML-escaping into the COMMAND_STRING.
        let doc = document_wflow("New Portal Tab Here", "--open-tab", Some("/p/portal"));
        // The metadata that makes Finder offer the action on folders.
        assert!(doc.contains("serviceApplicationBundleID"));
        assert!(doc.contains("<string>com.apple.finder</string>"));
        assert!(doc.contains("com.apple.Automator.fileSystemObject"));
        assert!(doc.contains("com.apple.Automator.nothing"));
        assert!(doc.contains("<integer>0</integer>"));
        assert!(doc.contains("com.apple.Automator.servicesMenu"));
        // The command body is embedded XML-escaped.
        assert!(doc.contains("\"/p/portal\" --open-tab"));
        assert!(doc.contains("&amp;"));
        assert!(!doc.contains("--open-window"), "tab workflow must not embed the window flag");
    }

    #[test]
    fn info_plist_declares_the_service() {
        let plist = info_plist("New Portal Tab Here", "com.portal.app.quickaction.tab-here");
        assert!(plist.contains("<key>NSServices</key>"));
        assert!(plist.contains("<key>NSMessage</key>\n\t\t\t<string>runWorkflowAsService</string>"));
        assert!(plist.contains("<string>New Portal Tab Here</string>"));
        assert!(plist.contains("<string>com.apple.finder</string>"));
        // Without CFBundleIdentifier Finder never surfaces the action.
        assert!(plist.contains("<string>com.portal.app.quickaction.tab-here</string>"));
    }

    #[test]
    fn status_classifies_old_layout_as_stale() {
        // (info.plist present, document.wflow present) → usable bundle
        assert_eq!(classify(Some(true), true), IntegrationStatus::Installed);
        // Pre-fix layout: only the document — never registers.
        assert_eq!(classify(Some(false), true), IntegrationStatus::Stale);
        assert_eq!(classify(None, false), IntegrationStatus::NotInstalled);
    }

    use IntegrationStatus::*;
    fn classify(info: Option<bool>, doc: bool) -> IntegrationStatus {
        match (info, doc) {
            (Some(true), true) => Installed,
            (Some(false) | None, true) => Stale,
            _ => NotInstalled,
        }
    }
}
