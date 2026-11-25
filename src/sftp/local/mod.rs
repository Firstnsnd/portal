//! # Local File Browser
//!
//! Synchronous local filesystem browser implementation.
//!
//! Per-OS differences (permission bits, the hidden-file convention) live in
//! sibling modules behind [`entry_permissions`] / [`is_hidden`]:

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use unix::{entry_permissions, is_hidden};
#[cfg(windows)]
pub use windows::{entry_permissions, is_hidden};

use crate::sftp::types::{SftpEntry, SftpEntryKind};

/// Synchronous local filesystem browser (left panel).
pub struct LocalBrowser {
    pub current_path: String,
    pub entries: Vec<SftpEntry>,
    pub selection: crate::sftp::selection::FileSelection,
    pub path_input: String,
    pub show_hidden_files: bool,
}

impl LocalBrowser {
    // `new()` performs filesystem I/O (`refresh`), so a `Default` impl
    // would be misleading rather than trivial.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let home = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("/"))
            .to_string_lossy()
            .to_string();
        let mut browser = Self {
            current_path: home.clone(),
            entries: Vec::new(),
            selection: crate::sftp::selection::FileSelection::default(),
            path_input: home,
            show_hidden_files: false,
        };
        browser.refresh();
        browser
    }

    /// Re-read the current directory.
    pub fn refresh(&mut self) {
        let mut entries = Vec::new();
        if let Ok(read_dir) = std::fs::read_dir(&self.current_path) {
            for dir_entry in read_dir.flatten() {
                let name = dir_entry.file_name().to_string_lossy().to_string();
                let meta = dir_entry.metadata().ok();
                // Hidden entries are dropped here (not at render time) so the
                // per-OS predicate has the metadata it needs.
                if !self.show_hidden_files && is_hidden(&name, meta.as_ref()) {
                    continue;
                }
                let kind = match meta.as_ref().map(|m| m.file_type()) {
                    Some(ft) if ft.is_dir() => SftpEntryKind::Directory,
                    Some(ft) if ft.is_symlink() => SftpEntryKind::Symlink,
                    Some(ft) if ft.is_file() => SftpEntryKind::File,
                    _ => SftpEntryKind::Other,
                };
                let size = meta.as_ref().map(|m| m.len());
                let modified = meta.as_ref().and_then(|m| m.modified().ok());
                let permissions = entry_permissions(meta.as_ref());
                entries.push(SftpEntry {
                    name,
                    kind,
                    size,
                    permissions,
                    modified,
                });
            }
        }
        // Sort: directories first, then alphabetical
        entries.sort_by(|a, b| {
            let a_dir = a.kind == SftpEntryKind::Directory;
            let b_dir = b.kind == SftpEntryKind::Directory;
            b_dir
                .cmp(&a_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        self.entries = entries;
        self.selection.clear();
    }

    /// Navigate to an absolute path.
    pub fn navigate(&mut self, path: &str) {
        let p = std::path::Path::new(path);
        if p.is_dir() {
            self.current_path = p.to_string_lossy().to_string();
            self.path_input = self.current_path.clone();
            self.refresh();
        }
    }

    /// Navigate to the parent directory.
    pub fn navigate_up(&mut self) {
        if let Some(parent) = std::path::Path::new(&self.current_path).parent() {
            let p = parent.to_string_lossy().to_string();
            self.navigate(&p);
        }
    }

    /// Rename a local file or directory.
    pub fn rename(&mut self, old_name: &str, new_name: &str) -> Result<(), String> {
        let old_path = format!("{}/{}", self.current_path.trim_end_matches('/'), old_name);
        let new_path = format!("{}/{}", self.current_path.trim_end_matches('/'), new_name);
        std::fs::rename(&old_path, &new_path).map_err(|e| format!("Rename failed: {}", e))?;
        self.refresh();
        Ok(())
    }

    /// Delete a local file or directory.
    pub fn delete(&mut self, name: &str) -> Result<(), String> {
        let path = format!("{}/{}", self.current_path.trim_end_matches('/'), name);
        let p = std::path::Path::new(&path);
        if p.is_dir() {
            std::fs::remove_dir_all(&path).map_err(|e| format!("Delete failed: {}", e))?;
        } else {
            std::fs::remove_file(&path).map_err(|e| format!("Delete failed: {}", e))?;
        }
        self.refresh();
        Ok(())
    }

    /// Create a new directory inside the current path.
    pub fn create_dir(&mut self, name: &str) -> Result<(), String> {
        let path = format!("{}/{}", self.current_path.trim_end_matches('/'), name);
        std::fs::create_dir(&path).map_err(|e| format!("Create dir failed: {}", e))?;
        self.refresh();
        Ok(())
    }

    /// Read a local file for the editor (≤10MB, UTF-8).
    pub fn read_file(&self, name: &str) -> Result<String, String> {
        let path = format!("{}/{}", self.current_path.trim_end_matches('/'), name);
        let meta = std::fs::metadata(&path).map_err(|e| format!("Cannot stat file: {}", e))?;
        if meta.len() > 10 * 1024 * 1024 {
            return Err(format!("File too large: {} bytes (max 10 MB)", meta.len()));
        }
        let data = std::fs::read(&path).map_err(|e| format!("Cannot read file: {}", e))?;
        String::from_utf8(data).map_err(|_| "Not valid UTF-8".to_string())
    }

    /// Write content to a local file from the editor. Atomic (temp + fsync +
    /// rename): a crash mid-save leaves the old content intact, never a
    /// truncated file. The file's existing mode is preserved.
    pub fn write_file(&mut self, name: &str, content: &str) -> Result<(), String> {
        let path = format!("{}/{}", self.current_path.trim_end_matches('/'), name);
        crate::config::atomic_write_file_preserving_mode(path.as_ref(), content.as_bytes())
            .map_err(|e| format!("Cannot write file: {}", e))?;
        self.refresh();
        Ok(())
    }

    /// Toggle visibility of hidden files (dotfiles on unix, the Win32
    /// hidden attribute on Windows) and re-read the directory.
    pub fn toggle_hidden_files(&mut self) {
        self.show_hidden_files = !self.show_hidden_files;
        self.refresh();
    }

    /// Entries for the panel. The hidden filter is applied by [`Self::refresh`]
    /// (it needs per-OS metadata), so this is the full current listing.
    pub fn filtered_entries(&self) -> Vec<SftpEntry> {
        self.entries.clone()
    }
}
