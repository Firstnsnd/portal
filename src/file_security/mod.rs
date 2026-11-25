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

#[cfg(all(unix, not(target_os = "macos")))]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(all(unix, not(target_os = "macos")))]
pub use linux::{preserve_existing_mode, restrict_to_owner};
#[cfg(target_os = "macos")]
pub use macos::{preserve_existing_mode, restrict_to_owner};
#[cfg(windows)]
pub use windows::{preserve_existing_mode, restrict_to_owner};

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

    #[cfg(unix)]
    #[test]
    fn preserve_existing_mode_keeps_the_destination_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("script.sh");
        std::fs::write(&dest, b"old").unwrap();
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755)).unwrap();
        // The temp replacement inherits the destination's mode…
        let temp = dir.path().join("script.sh.tmp");
        let file = std::fs::File::create(&temp).unwrap();
        super::preserve_existing_mode(&file, &dest);
        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o755);
        // …whereas a fresh destination leaves the default (0644 minus umask).
        let fresh = dir.path().join("other.tmp");
        let file = std::fs::File::create(&fresh).unwrap();
        let default = file.metadata().unwrap().permissions().mode();
        super::preserve_existing_mode(&file, &dir.path().join("never-existed"));
        assert_eq!(file.metadata().unwrap().permissions().mode(), default);
    }
}
