//! # SFTP View Types
//!
//! This module contains types specific to the SFTP view,
//! including drag-and-drop payloads and selection actions.

/// A single entry in a drag payload.
#[derive(Clone)]
pub struct DragEntry {
    pub full_path: String,
    pub entry_name: String,
    pub is_dir: bool,
}

/// Drag-and-drop payload for SFTP panel file transfers (supports multi-select).
#[derive(Clone)]
pub struct DragPayload {
    pub is_local: bool,
    pub entries: Vec<DragEntry>,
}

/// Actions produced by render_file_panel for the caller to apply to FileSelection.
pub enum SelectionAction {
    Single(usize),
    Toggle(usize),
    Range(usize),
    SelectAll,
    FocusMove(usize),
    FocusExtend(usize),
    DeselectAll,
}

/// Request to move entries to a target directory (produced by drag-to-folder).
pub struct MoveToDirRequest {
    pub source_entries: Vec<DragEntry>,
    pub target_dir: String,
}
