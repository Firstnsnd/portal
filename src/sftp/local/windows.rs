//! Local-directory entry metadata — Windows: no mode bits; hidden =
//! the Win32 hidden attribute.

use std::fs::Metadata;

/// Windows has no rwx mode bits — the permissions column stays empty.
pub fn entry_permissions(_meta: Option<&Metadata>) -> Option<u32> {
    None
}

/// Hidden = the Win32 `FILE_ATTRIBUTE_HIDDEN` attribute (0x2). A leading
/// dot means nothing on Windows: `.gitignore` and friends are only hidden
/// when the attribute is set.
pub fn is_hidden(_name: &str, meta: Option<&Metadata>) -> bool {
    use std::os::windows::fs::MetadataExt;
    meta.map(|m| m.file_attributes() & 0x2 != 0)
        .unwrap_or(false)
}
