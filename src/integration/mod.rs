//! # File-Manager Integration ("Open new Portal tab/window here")
//!
//! Installs the OS file-manager right-click actions that feed Portal's
//! single-instance IPC ([`crate::ipc`]). [`ShellIntegration`] is the
//! contract; each OS ships its own implementation in a sibling module:
//!
//! - **macOS Finder** ([`macos`]): Automator Quick Action bundles
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
//! - **Linux** ([`linux`]): Nautilus scripts (`~/.local/share/nautilus/scripts/`)
//!   and a Dolphin/KDE service menu (`~/.local/share/kio/servicemenus/`).
//! - **Windows Explorer** ([`windows`]): HKCU context-menu entries
//!   (`Directory\shell` + `Directory\Background\shell`) launching portal
//!   with `--open-tab` / `--open-window`.
//!
//! Every action ends in `--open-tab|--open-window <dir>`; a freshly launched
//! instance forwards the request to the running one over the IPC channel and
//! exits (see ipc.rs).

use std::path::PathBuf;

#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) mod linux;
#[cfg(target_os = "macos")]
pub(crate) mod macos;
#[cfg(windows)]
pub(crate) mod windows;

/// Whether the file-manager actions are installed — `Stale` means files from
/// an older layout exist that never register (e.g. pre-Info.plist bundles);
/// reinstalling fixes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegrationStatus {
    NotInstalled,
    Installed,
    Stale,
}

/// Platform shell integration: install/uninstall the right-click actions
/// and report their install state for the Settings UI.
pub trait ShellIntegration {
    /// Backend display name for log lines.
    fn name(&self) -> &'static str;
    fn install(&self) -> Result<(), String>;
    fn uninstall(&self) -> Result<(), String>;
    fn status(&self) -> IntegrationStatus;
}

fn platform_integration() -> &'static dyn ShellIntegration {
    #[cfg(target_os = "macos")]
    {
        &macos::FinderIntegration
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        &linux::DesktopIntegration
    }
    #[cfg(windows)]
    {
        &windows::ExplorerIntegration
    }
}

/// Install the file-manager actions for the current OS.
pub fn install() -> Result<(), String> {
    let it = platform_integration();
    it.install()
        .inspect(|_| log::info!("[{}] file-manager actions installed", it.name()))
}

/// Remove the file-manager actions.
pub fn uninstall() -> Result<(), String> {
    let it = platform_integration();
    it.uninstall()
        .inspect(|_| log::info!("[{}] file-manager actions removed", it.name()))
}

/// Current install state, for the Settings UI.
pub fn status() -> IntegrationStatus {
    let it = platform_integration();
    it.status()
}

/// The portal binary to invoke. `PORTAL_BIN` keeps dev builds working;
/// installed packages resolve to their installed path via `current_exe`.
/// (macOS deliberately routes through `open -nb` instead — see macos.rs.)
#[cfg(any(target_os = "linux", windows))]
pub(crate) fn portal_bin() -> String {
    std::env::var("PORTAL_BIN")
        .ok()
        .filter(|b| !b.is_empty())
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "portal".to_string())
}

pub(crate) fn home_dir() -> Result<PathBuf, String> {
    dirs::home_dir().ok_or_else(|| "cannot resolve home directory".to_string())
}
