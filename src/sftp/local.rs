//! # Local File Browser
//!
//! Synchronous local filesystem browser implementation.

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
                let kind = match meta.as_ref().map(|m| m.file_type()) {
                    Some(ft) if ft.is_dir() => SftpEntryKind::Directory,
                    Some(ft) if ft.is_symlink() => SftpEntryKind::Symlink,
                    Some(ft) if ft.is_file() => SftpEntryKind::File,
                    _ => SftpEntryKind::Other,
                };
                let size = meta.as_ref().map(|m| m.len());
                #[cfg(unix)]
                let permissions = {
                    use std::os::unix::fs::PermissionsExt;
                    meta.as_ref().map(|m| m.permissions().mode())
                };
                #[cfg(not(unix))]
                let permissions = None;
                entries.push(SftpEntry {
                    name,
                    kind,
                    size,
                    permissions,
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
        std::fs::rename(&old_path, &new_path)
            .map_err(|e| format!("Rename failed: {}", e))?;
        self.refresh();
        Ok(())
    }

    /// Delete a local file or directory.
    pub fn delete(&mut self, name: &str) -> Result<(), String> {
        let path = format!("{}/{}", self.current_path.trim_end_matches('/'), name);
        let p = std::path::Path::new(&path);
        if p.is_dir() {
            std::fs::remove_dir_all(&path)
                .map_err(|e| format!("Delete failed: {}", e))?;
        } else {
            std::fs::remove_file(&path)
                .map_err(|e| format!("Delete failed: {}", e))?;
        }
        self.refresh();
        Ok(())
    }

    /// Create a new directory inside the current path.
    pub fn create_dir(&mut self, name: &str) -> Result<(), String> {
        let path = format!("{}/{}", self.current_path.trim_end_matches('/'), name);
        std::fs::create_dir(&path)
            .map_err(|e| format!("Create dir failed: {}", e))?;
        self.refresh();
        Ok(())
    }

    /// Read a local file for the editor (≤10MB, UTF-8).
    pub fn read_file(&self, name: &str) -> Result<String, String> {
        let path = format!("{}/{}", self.current_path.trim_end_matches('/'), name);
        let meta = std::fs::metadata(&path)
            .map_err(|e| format!("Cannot stat file: {}", e))?;
        if meta.len() > 10 * 1024 * 1024 {
            return Err(format!("File too large: {} bytes (max 10 MB)", meta.len()));
        }
        let data = std::fs::read(&path)
            .map_err(|e| format!("Cannot read file: {}", e))?;
        String::from_utf8(data)
            .map_err(|_| "Not valid UTF-8".to_string())
    }

    /// Write content to a local file from the editor.
    pub fn write_file(&mut self, name: &str, content: &str) -> Result<(), String> {
        let path = format!("{}/{}", self.current_path.trim_end_matches('/'), name);
        std::fs::write(&path, content.as_bytes())
            .map_err(|e| format!("Cannot write file: {}", e))?;
        self.refresh();
        Ok(())
    }

    /// Toggle visibility of hidden files (files starting with .).
    pub fn toggle_hidden_files(&mut self) {
        self.show_hidden_files = !self.show_hidden_files;
        self.selection.clear();
    }

    /// Get filtered entries (hides dotfiles if show_hidden_files is false).
    pub fn filtered_entries(&self) -> Vec<SftpEntry> {
        if self.show_hidden_files {
            self.entries.clone()
        } else {
            self.entries.iter()
                .filter(|e| !e.name.starts_with('.'))
                .cloned()
                .collect()
        }
    }
}
