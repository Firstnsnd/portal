//! # UI Type Definitions
//!
//! This module re-exports all UI type definitions from submodules.

// Session and terminal types
pub mod session;
pub use session::TerminalSession;

// Layout and window types
pub mod layout;

// Dialog types
pub mod dialogs;
pub use dialogs::{
    AppView, BroadcastState,
    AddHostDialog, CredentialDialog, SnippetViewState, HostFilter,
};

// SFTP types
pub mod sftp_types;
pub use sftp_types::SftpPanel;
