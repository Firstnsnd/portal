//! Owner-only permissions — Windows: covered by the user profile's NTFS ACLs.

/// Files Portal writes (config, recordings) live under the user profile,
/// whose inherited NTFS ACLs already exclude other standard users — there
/// is no chmod to apply, and hand-writing a restrictive DACL
/// (`SetSecurityInfo` + SID plumbing) is out of proportion for the threat
/// model. Documented no-op.
pub fn restrict_to_owner(_file: &std::fs::File) {}

/// Mode preservation — Windows: the temp file inherits the directory's NTFS
/// ACLs; a user file's explicit ACLs are rare here and `CopyFileSecurity`
/// plumbing is out of proportion for the editor-save path. Documented no-op.
pub fn preserve_existing_mode(_file: &std::fs::File, _dest: &std::path::Path) {}
