//! # SFTP View Components
//!
//! This module contains components for the SFTP file browser view.
//! It is organized into submodules for better maintainability.

#![allow(dead_code)]
#![allow(unused_imports)]

pub mod types;
pub mod format;
pub mod panel;
pub mod progress;

// Re-export commonly used types
pub use types::{DragEntry, DragPayload, SelectionAction, MoveToDirRequest};
pub use format::{format_transfer_speed, format_file_size, format_permissions};
pub use panel::{render_breadcrumbs, render_file_panel, apply_selection_action};
pub use progress::render_transfer_progress;
