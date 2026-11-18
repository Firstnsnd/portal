//! # OS-shell opener
//!
//! Platform-agnostic "open things" capability: hand a URL/file/folder to the
//! OS default handler, reveal a path in the file manager (selecting it where
//! the OS supports that), and deep-link authorization settings. [`Opener`] is
//! the contract; each OS ships its own implementation in a sibling module:
//!
//! - [`macos`]:   `open` / `open -R`, TCC pane via `x-apple.systempreferences:`
//! - [`windows`]: `cmd /C start` + `explorer /select,`
//! - [`linux`]:   `xdg-open` (reveal degrades to opening the parent folder)
//!
//! Callers go through the free functions below and never know which backend
//! is live (same shape as `secret_store::platform_store`).

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(windows)]
pub mod windows;

use std::path::Path;

pub trait Opener: Sync {
    /// Backend display name for log lines.
    fn name(&self) -> &'static str;

    /// Open a URL in the default browser.
    fn open_url(&self, url: &str) -> Result<(), String>;

    /// Open a filesystem path with its default handler (installer, archive…).
    fn open_path(&self, path: &Path) -> Result<(), String>;

    /// Open a directory in the OS file manager.
    fn open_folder(&self, dir: &Path) -> Result<(), String>;

    /// Reveal a path in the file manager, selecting it where the OS supports
    /// it; degrades to opening the parent directory elsewhere.
    fn reveal_path(&self, path: &Path) -> Result<(), String>;

    /// Whether this OS has an authorization/privacy settings surface worth
    /// deep-linking (macOS TCC). Gates the related UI buttons — on platforms
    /// returning `false` there is nothing meaningful to open.
    fn has_privacy_settings(&self) -> bool;

    /// Open the OS authorization/privacy settings pane. Only meaningful when
    /// [`Opener::has_privacy_settings`] is true.
    fn open_privacy_settings(&self) -> Result<(), String>;
}

/// The live platform backend.
pub fn platform_opener() -> &'static dyn Opener {
    #[cfg(target_os = "macos")]
    {
        &macos::MacosOpener
    }
    #[cfg(windows)]
    {
        &windows::WindowsOpener
    }
    #[cfg(target_os = "linux")]
    {
        &linux::LinuxOpener
    }
}

/// Route a call through the platform backend, logging failures with the
/// backend name (the log line is the only place the live OS shows through).
fn dispatch<F>(what: &str, f: F) -> Result<(), String>
where
    F: FnOnce(&'static dyn Opener) -> Result<(), String>,
{
    let it = platform_opener();
    f(it).inspect_err(|e| log::warn!("[{}] {what}: {e}", it.name()))
}

/// Open a URL in the default browser.
pub fn open_url(url: &str) -> Result<(), String> {
    dispatch("open_url", |it| it.open_url(url))
}

/// Open a filesystem path with its default handler.
pub fn open_path(path: &Path) -> Result<(), String> {
    let path = path.to_path_buf();
    dispatch("open_path", move |it| it.open_path(&path))
}

/// Open a directory in the OS file manager.
pub fn open_folder(dir: &Path) -> Result<(), String> {
    let dir = dir.to_path_buf();
    dispatch("open_folder", move |it| it.open_folder(&dir))
}

/// Reveal a path in the OS file manager.
pub fn reveal_path(path: &Path) -> Result<(), String> {
    let path = path.to_path_buf();
    dispatch("reveal_path", move |it| it.reveal_path(&path))
}

/// Whether the current OS has an authorization settings pane to deep-link.
pub fn has_privacy_settings() -> bool {
    platform_opener().has_privacy_settings()
}

/// Open the OS authorization settings pane (macOS only).
pub fn open_privacy_settings() -> Result<(), String> {
    dispatch("open_privacy_settings", |it| it.open_privacy_settings())
}
