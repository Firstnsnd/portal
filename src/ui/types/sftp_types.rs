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

impl SftpPanel {
    #[allow(dead_code)]
    pub fn is_local(self) -> bool {
        matches!(self, SftpPanel::LeftLocal | SftpPanel::RightLocal)
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

/// SFTP editor dialog state
pub struct SftpEditorDialog {
    pub panel: SftpPanel,
    pub file_path: String,
    pub file_name: String,
    pub directory: String,
    pub content: String,
    pub original_content: String,
    pub loading: bool,
    pub is_new_file: bool,
    pub error: String,
    pub save_as_name: String,
}

use eframe::egui;
