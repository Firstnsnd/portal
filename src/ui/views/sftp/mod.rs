//! # SFTP View Components
//!
//! This module contains components for the SFTP file browser view.
//! It is organized into submodules for better maintainability.

pub mod types;
pub mod format;
pub mod panel;
pub mod progress;
pub mod drop_logic;
pub mod editor;

// Re-export commonly used types
pub use types::{DragEntry, DragPayload, SelectionAction, MoveToDirRequest};
pub use panel::{render_breadcrumbs, render_file_panel, apply_selection_action};
pub use progress::render_transfer_progress;
