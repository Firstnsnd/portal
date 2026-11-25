//! # Local owner verification
//!
//! Platform-agnostic "prove you own this computer" checks guarding protected
//! hosts. [`OwnerAuth`] is the contract; each OS ships its own implementation
//! in a sibling module:
//!
//! - `macos`:   LocalAuthentication (`LAContext`, Touch ID with automatic
//!   password fallback via the system sheet)
//! - `linux`:   PAM (`pam_authenticate`, service "login")
//! - `windows`: Win32 `LogonUserW` against the current user's password
//!
//! Callers go through [`begin_system_prompt`]/[`verify_owner_password`] and
//! never know which backend is live. Two collection styles exist: macOS owns
//! its system prompt (async), Linux/Windows verify a password the caller
//! collected in an app-drawn field. `verify_owner_password` may block for
//! seconds — never call it on the UI thread.

use std::sync::mpsc;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(windows)]
pub mod windows;

/// One-shot callback for the system-prompt verification result.
pub type OwnerReply = Box<dyn FnOnce(bool) + Send>;

pub trait OwnerAuth {
    /// Backend display name for log lines.
    fn name(&self) -> &'static str;

    /// Verify a caller-collected password. `Ok(false)` = wrong password;
    /// `Err` = backend error (or "not this platform's style", on macOS).
    fn verify_password(&self, password: &str) -> Result<bool, String>;

    /// Start the system-owned prompt (macOS sheet). `Err` = this platform
    /// has no system prompt — the caller should fall back to `verify_password`.
    /// `reply` fires exactly once, from an arbitrary thread.
    fn verify_system_prompt(&self, reason: &str, reply: OwnerReply) -> Result<(), String>;
}

/// True on platforms whose backend owns a system-prompt UI (macOS). Lets
/// callers tell "this platform collects passwords itself" apart from "the
/// sheet exists but failed to start" — both come back `None` from
/// [`begin_system_prompt`], but only the latter is a dead end.
pub fn has_system_prompt() -> bool {
    cfg!(target_os = "macos")
}

/// The live platform backend.
pub fn platform_auth() -> &'static dyn OwnerAuth {
    #[cfg(target_os = "macos")]
    {
        &macos::LocalAuthentication
    }
    #[cfg(windows)]
    {
        &windows::Win32Logon
    }
    #[cfg(target_os = "linux")]
    {
        &linux::PamAuth
    }
}

/// Try the platform system prompt (macOS). `None` = no system prompt on this
/// platform — collect a password and call [`verify_owner_password`] instead.
/// When the prompt resolves, the result is on the channel and the UI has been
/// asked to repaint.
pub fn begin_system_prompt(reason: &str) -> Option<mpsc::Receiver<Result<bool, String>>> {
    let (tx, rx) = mpsc::channel();
    let started = platform_auth().verify_system_prompt(
        reason,
        Box::new(move |ok| {
            let _ = tx.send(Ok(ok));
            crate::repaint::notify_repaint();
        }),
    );
    match started {
        Ok(()) => Some(rx),
        Err(e) => {
            log::warn!(
                "owner auth ({}): system prompt unavailable: {e}",
                platform_auth().name()
            );
            None
        }
    }
}

/// Blocking password check (PAM on Linux, `LogonUserW` on Windows).
/// `Ok(false)` = wrong password. Must run on a background thread.
pub fn verify_owner_password(password: &str) -> Result<bool, String> {
    platform_auth().verify_password(password)
}

// Only meaningful off macOS; on macOS the import would be the sole content
// of the module and warn as unused.
#[cfg(all(test, not(target_os = "macos")))]
mod tests {
    use super::*;

    #[test]
    fn no_system_prompt_off_macos() {
        assert!(begin_system_prompt("test").is_none());
    }
}
