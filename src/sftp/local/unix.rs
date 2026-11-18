//! Local-directory entry metadata — unix: mode bits + dotfile convention.

use std::fs::Metadata;

/// Unix permission mode bits, rendered as rwx in the permissions column.
pub fn entry_permissions(meta: Option<&Metadata>) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    meta.map(|m| m.permissions().mode())
}

/// Hidden = leading dot (the unix convention; there is no attribute bit).
pub fn is_hidden(name: &str, _meta: Option<&Metadata>) -> bool {
    name.starts_with('.')
}
