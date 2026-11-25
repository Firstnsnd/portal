//! # SFTP View Components
//!
//! This module contains components for the SFTP file browser view.
//! It is organized into submodules for better maintainability.

pub mod drop_logic;
pub mod editor;
pub mod format;
pub mod panel;
pub mod paste;
pub mod progress;
pub mod types;

// Re-export commonly used types
pub use panel::{apply_selection_action, render_breadcrumbs, render_file_panel};
pub use progress::render_transfer_progress;
pub use types::{ClipboardRequest, DragEntry, DragPayload, MoveToDirRequest, SelectionAction};
