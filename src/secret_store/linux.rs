//! Linux backend: the freedesktop Secret Service (gnome-keyring, KWallet)
//! via the `keyring` crate, `sync-secret-service` backend.
//!
//! Dependency features (see Cargo.toml): `crypto-rust` keeps encryption
//! pure-Rust, `vendored` statically links libdbus so no system `-dev`
//! packages are needed at build time. At runtime a Secret Service daemon
//! must be present; on headless systems without one every operation fails
//! and callers degrade gracefully, same as any other desktop integration.

pub struct SecretServiceStore;

impl super::SecretStore for SecretServiceStore {
    fn name(&self) -> &'static str {
        "Secret Service"
    }

    fn set(&self, service: &str, account: &str, secret: &str) -> Result<(), String> {
        entry(service, account)?
            .set_password(secret)
            .map_err(|e| e.to_string())
    }

    fn get(&self, service: &str, account: &str) -> Result<Option<String>, String> {
        match entry(service, account)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), String> {
        match entry(service, account)?.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()), // already gone — deletion is idempotent
            Err(e) => Err(e.to_string()),
        }
    }
}

fn entry(service: &str, account: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(service, account).map_err(|e| e.to_string())
}
