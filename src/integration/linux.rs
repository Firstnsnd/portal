//! Linux backend: Nautilus scripts and a Dolphin/KDE service menu.

use super::{home_dir, portal_bin, ShellIntegration};

pub(crate) struct DesktopIntegration;

impl ShellIntegration for DesktopIntegration {
    fn name(&self) -> &'static str {
        "Nautilus/Dolphin"
    }

    fn install(&self) -> Result<(), String> {
        install_linux()
    }

    fn uninstall(&self) -> Result<(), String> {
        let home = home_dir()?;
        let _ =
            std::fs::remove_file(home.join(".local/share/nautilus/scripts/New Portal Tab Here"));
        let _ =
            std::fs::remove_file(home.join(".local/share/nautilus/scripts/New Portal Window Here"));
        let _ =
            std::fs::remove_file(home.join(".local/share/kio/servicemenus/portal_here.desktop"));
        let _ = std::fs::remove_file(
            home.join(".local/share/kservices5/ServiceMenus/portal_here.desktop"),
        );
        Ok(())
    }

    fn status(&self) -> super::IntegrationStatus {
        let Ok(home) = home_dir() else {
            return super::IntegrationStatus::NotInstalled;
        };
        let nautilus = home.join(".local/share/nautilus/scripts/New Portal Tab Here");
        let dolphin = home.join(".local/share/kio/servicemenus/portal_here.desktop");
        if nautilus.exists() || dolphin.exists() {
            super::IntegrationStatus::Installed
        } else {
            super::IntegrationStatus::NotInstalled
        }
    }
}

fn install_linux() -> Result<(), String> {
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
        // Atomic write: a crash mid-install leaves the previous script (still
        // executable) in place instead of a truncated non-executable one.
        crate::config::atomic_write_file_preserving_mode(&path, script.as_bytes())
            .map_err(|e| format!("{}: {}", path.display(), e))?;
        // Scripts must stay executable — chmod after the rename, since a fresh
        // install gets the default (non-executable) mode.
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
    crate::config::atomic_write_file_preserving_mode(
        &menu_dir.join("portal_here.desktop"),
        desktop_entry.as_bytes(),
    )
    .map_err(|e| format!("portal_here.desktop: {}", e))?;

    // Reload Nautilus' script list if it's running.
    let _ = std::process::Command::new("nautilus").arg("-q").output();

    Ok(())
}
