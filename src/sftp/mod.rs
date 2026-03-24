//! SFTP file browser support
//!
//! This module is organized into submodules for better maintainability:
//! - **types**: Common data types (entries, connection state, transfer progress)
//! - **selection**: Multi-selection state management
//! - **local**: Local filesystem browser
//! - **browser**: SFTP browser with async task management
//! - **task**: Async SFTP task running on tokio

mod types;
mod selection;
mod local;
mod browser;
mod task;

// Re-export public types
pub use types::{SftpEntry, SftpEntryKind, SftpConnectionState, TransferProgress};
pub use selection::FileSelection;
pub use local::LocalBrowser;
pub use browser::SftpBrowser;
