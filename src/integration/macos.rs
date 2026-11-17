//! macOS backend: Finder Quick Actions (Automator workflow bundles in
//! `~/Library/Services`), registered with the `pbs` Services daemon.

use std::path::PathBuf;

use super::{home_dir, ShellIntegration};

const BUNDLE_ID: &str = "com.portal.app";

pub(crate) struct FinderIntegration;

impl ShellIntegration for FinderIntegration {
    fn name(&self) -> &'static str {
        "Finder"
    }

    fn install(&self) -> Result<(), String> {
        install_macos()?;
        refresh_services_cache();
        Ok(())
    }

    fn uninstall(&self) -> Result<(), String> {
        let dir = services_dir()?;
        for name in ["New Portal Tab Here", "New Portal Window Here"] {
            let _ = std::fs::remove_dir_all(dir.join(format!("{name}.workflow")));
        }
        refresh_services_cache();
        Ok(())
    }

    fn status(&self) -> super::IntegrationStatus {
        let Ok(home) = home_dir() else {
            return super::IntegrationStatus::NotInstalled;
        };
        let contents = home
            .join("Library/Services/New Portal Tab Here.workflow")
            .join("Contents");
        match (contents.join("Info.plist").exists(), contents.join("document.wflow").exists()) {
            // Both files: a bundle the Services daemon can actually register.
            (true, true) => super::IntegrationStatus::Installed,
            // Old layout (document.wflow only): silently ignored by pbs.
            (false, true) => super::IntegrationStatus::Stale,
            _ => super::IntegrationStatus::NotInstalled,
        }
    }
}

fn services_dir() -> Result<PathBuf, String> {
    Ok(home_dir()?.join("Library/Services"))
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
fn enable_service(name: &str, bundle_id: &str) {
    let key = format!("{bundle_id} - {name} - runWorkflowAsService");
    let value = "{ enabled_context_menu = 1; enabled_services_menu = 1; \
                  presentation_modes = { ContextMenu = 1; ServicesMenu = 1; }; }";
    let _ = std::process::Command::new("defaults")
        .args(["write", "pbs", "NSServicesStatus", "-dict-add", &key, value])
        .output();
}

/// Kick the Services daemon so the new actions appear without re-login.
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

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::IntegrationStatus;

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
