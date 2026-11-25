//! Process-level platform hooks run at app startup.
//!
//! Two concerns, each with a per-OS answer behind one facade:
//!
//! - [`install_signal_cleanup`]: exit-time PTY cleanup on termination
//!   signals. Windows has no POSIX signals — the ConPTY handles are torn
//!   down by the OS when the process dies, so nothing to install there.
//! - [`activate_gui`]: give the freshly launched app GUI presence (macOS:
//!   Dock icon + activation policy). Other platforms show the launched
//!   window as-is.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use unix::install_signal_cleanup;
#[cfg(windows)]
pub use windows::install_signal_cleanup;

#[cfg(target_os = "macos")]
pub use macos::activate_gui;
/// No GUI-presence step is needed: the window manager already shows the
/// launched window on Linux and Windows.
#[cfg(not(target_os = "macos"))]
pub fn activate_gui() {}
