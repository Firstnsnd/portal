//! Owner-only permissions — Linux: chmod 0600 on the open file.

/// Restrict an open file to its owner (mode 0600). Best effort, like the
/// rest of the write path: `File::create` preserves the mode of an existing
/// file, so this also tightens an old world-readable copy.
pub fn restrict_to_owner(file: &std::fs::File) {
    use std::os::unix::fs::PermissionsExt;
    let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
}
