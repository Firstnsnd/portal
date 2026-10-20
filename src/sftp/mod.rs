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
// Drag-and-drop glue is consumed only by `src/ui` (the bin crate). It is
// compiled into the lib so `pub mod sftp` (lib.rs) links, but it has no
// lib-side callers — hence the dead_code allowance.
#[allow(dead_code)]
pub(crate) mod drop_planner;
#[allow(dead_code)]
pub(crate) mod drag_out;

// Re-export public types
// The lib crate re-exports these for integration tests (`portal::sftp::…`);
// the bin compiles this module privately and reaches the types through
// `crate::sftp::types`/`crate::sftp::task` directly, so the re-exports are
// legitimately unused there.
#[allow(unused_imports)]
pub use types::{SftpEntry, SftpEntryKind, SftpConnectionState, TransferProgress, SftpCommand, SftpResponse};
pub use selection::FileSelection;
pub use local::LocalBrowser;
pub use browser::SftpBrowser;
// Task entry points (integration tests drive these directly with the
// AcceptAll host-key policy against mock servers).
#[allow(unused_imports)]
pub use task::{sftp_task, sftp_task_with_initial_path};
