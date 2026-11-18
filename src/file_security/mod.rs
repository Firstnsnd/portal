//! Owner-only file permissions — per-OS implementations behind one call.
//!
//! Config files and session recordings may carry secrets (a keychain-store
//! failure leaves passwords in the config JSON as a documented fallback, and
//! recordings persist every echoed output byte), so everything Portal writes
//! to disk must be readable by the owning user only. Unix tightens the mode
//! to 0600; Windows relies on the NTFS ACLs the user profile already
//! applies (see [`windows`]).
//!
//! The shared write paths that route through this module:
//! `config::write_private_file` (all on-disk config/session files) and
//! `terminal::cast::CastRecorder::create` (session recordings).

#[cfg(target_os = "macos")]
mod macos;
#[cfg(all(unix, not(target_os = "macos")))]
mod linux;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "macos")]
pub use macos::restrict_to_owner;
#[cfg(all(unix, not(target_os = "macos")))]
pub use linux::restrict_to_owner;
#[cfg(windows)]
pub use windows::restrict_to_owner;

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[test]
    fn restrict_to_owner_sets_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.conf");
        let file = std::fs::File::create(&path).unwrap();
        super::restrict_to_owner(&file);
        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    }
}
