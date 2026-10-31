//! # SFTP Types
//!
//! This module contains types related to SFTP file browser functionality.

/// Which SFTP panel an operation targets
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SftpPanel {
    LeftLocal,
    RightLocal,
    LeftRemote,
    RightRemote,
}

/// Which half of the SFTP view a panel lives in (embedded editors occupy a half).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SftpSide {
    Left,
    Right,
}

impl SftpPanel {
    #[allow(dead_code)]
    pub fn is_local(self) -> bool {
        matches!(self, SftpPanel::LeftLocal | SftpPanel::RightLocal)
    }

    /// Half of the view this panel belongs to. Drives editor placement,
    /// per-half keyboard gating, and OS-drop exclusion.
    pub fn side(self) -> SftpSide {
        if matches!(self, SftpPanel::LeftLocal | SftpPanel::LeftRemote) {
            SftpSide::Left
        } else {
            SftpSide::Right
        }
    }
}

/// SFTP right-click context menu state (supports multi-selection)
pub struct SftpContextMenu {
    pub pos: egui::Pos2,
    pub panel: SftpPanel,
    pub entry_indices: Vec<usize>,
    pub entry_names: Vec<String>,
    pub all_dirs: bool,
    pub any_dirs: bool,
}

/// SFTP rename dialog state
pub struct SftpRenameDialog {
    pub panel: SftpPanel,
    pub old_name: String,
    pub new_name: String,
    pub error: String,
}

/// SFTP new folder dialog state
pub struct SftpNewFolderDialog {
    pub panel: SftpPanel,
    pub name: String,
    pub error: String,
}

/// SFTP new file dialog state
pub struct SftpNewFileDialog {
    pub panel: SftpPanel,
    pub name: String,
    pub error: String,
}

/// SFTP delete confirmation dialog state (supports multi-file delete)
pub struct SftpConfirmDelete {
    pub panel: SftpPanel,
    pub names: Vec<String>,
}

/// SFTP error dialog state
pub struct SftpErrorDialog {
    pub title: String,
    pub message: String,
}

/// SFTP file editor embedded in one half of the SFTP view. While open it
/// replaces that half's file list; `panel` is fixed at open time and routes
/// read/write I/O to the owning browser.
pub struct SftpEmbeddedEditor {
    pub panel: SftpPanel,
    pub file_path: String,
    pub file_name: String,
    pub directory: String,
    pub content: String,
    pub original_content: String,
    pub loading: bool,
    pub error: String,
    /// Two-step close guard: the first ✕ press while dirty arms this flag
    /// (✕ turns red); the second press actually closes.
    pub confirm_close: bool,
}

impl SftpEmbeddedEditor {
    pub fn is_dirty(&self) -> bool {
        self.content != self.original_content
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_panel_side_mapping() {
        assert_eq!(SftpPanel::LeftLocal.side(), SftpSide::Left);
        assert_eq!(SftpPanel::LeftRemote.side(), SftpSide::Left);
        assert_eq!(SftpPanel::RightLocal.side(), SftpSide::Right);
        assert_eq!(SftpPanel::RightRemote.side(), SftpSide::Right);
    }

    #[test]
    fn test_editor_dirty_detection() {
        let editor = SftpEmbeddedEditor {
            panel: SftpPanel::LeftLocal,
            file_path: "/tmp/a.txt".to_string(),
            file_name: "a.txt".to_string(),
            directory: "/tmp".to_string(),
            content: "hello".to_string(),
            original_content: "hello".to_string(),
            loading: false,
            error: String::new(),
            confirm_close: false,
        };
        assert!(!editor.is_dirty());
        let mut edited = editor;
        edited.content.push('!');
        assert!(edited.is_dirty());
    }
}
