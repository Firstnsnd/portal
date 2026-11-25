//! Owner-only permissions — macOS: chmod 0600 on the open file.

/// Restrict an open file to its owner (mode 0600). Best effort, like the
/// rest of the write path: `File::create` preserves the mode of an existing
/// file, so this also tightens an old world-readable copy.
pub fn restrict_to_owner(file: &std::fs::File) {
    use std::os::unix::fs::PermissionsExt;
    let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
}

/// Give a freshly created temp file the mode of the file it will replace, so
/// an atomic rewrite doesn't silently drop e.g. a script's +x bit. Best
/// effort: a missing destination leaves the default mode.
pub fn preserve_existing_mode(file: &std::fs::File, dest: &std::path::Path) {
    if let Ok(meta) = std::fs::metadata(dest) {
        let _ = file.set_permissions(meta.permissions());
    }
}
