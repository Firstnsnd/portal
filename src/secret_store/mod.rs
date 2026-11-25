//! # System secret store
//!
//! Platform-agnostic access to the OS keychain. [`SecretStore`] is the
//! contract; each OS ships its own implementation in a sibling module:
//!
//! - [`macos`]:   macOS Keychain, via the `security` command-line tool
//! - [`windows`]: Windows Credential Manager, via CredReadW/CredWriteW/CredDeleteW
//! - [`linux`]:   freedesktop Secret Service, via the `keyring` crate
//!
//! Callers go through [`platform_store()`] and never know which backend is
//! live. The naming scheme (which service/account strings stand for which
//! entity) lives in `config`, not here.

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(windows)]
pub mod windows;

pub trait SecretStore {
    /// Backend display name for log lines.
    fn name(&self) -> &'static str;

    /// Store (or overwrite) the secret for `(service, account)`.
    fn set(&self, service: &str, account: &str, secret: &str) -> Result<(), String>;

    /// Read the secret for `(service, account)`; `Ok(None)` when absent.
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, String>;

    /// Remove the entry for `(service, account)`. Removing an absent entry is
    /// `Ok(())` — deletion is idempotent on every backend.
    fn delete(&self, service: &str, account: &str) -> Result<(), String>;
}

/// The live platform backend.
pub fn platform_store() -> &'static dyn SecretStore {
    #[cfg(target_os = "macos")]
    {
        &macos::KeychainCli
    }
    #[cfg(windows)]
    {
        &windows::CredentialManager
    }
    #[cfg(target_os = "linux")]
    {
        &linux::SecretServiceStore
    }
}
