//! # User Interface Module
//!
//! This module contains all egui-based UI components for the terminal emulator.
//!
//! ## Module Organization
//!
//! - **theme**: Color schemes and visual styling
//! - **i18n**: Internationalization (EN/ZH/JA/KO)
//! - **input**: Keyboard input handling and shortcuts
//! - **types**: Shared data structures (sessions, panes, dialogs)
//! - **pane**: Split pane layout management
//! - **terminal_render**: Terminal content rendering and text selection
//! - **sftp_view**: SFTP file browser UI
//! - **hosts_view**: SSH host management UI
//! - **settings_view**: Application settings UI
//! - **keychain_view**: System keychain management UI
//!
//! ## Key Concepts
//!
//! ### Pane Tree
//!
//! Terminal sessions are organized in a tree structure that supports:
//! - **Leaf nodes**: Individual terminal sessions
//! - **Split nodes**: Horizontal or vertical splits with adjustable ratios
//! - **Recursive nesting**: Unlimited split depth
//!
//! ### Terminal Sessions
//!
//! Each terminal session (`TerminalSession`) maintains:
//! - Terminal grid (character cells + attributes)
//! - Scrollback buffer (historical content)
//! - Selection state (mouse drag, double-click, triple-click)
//! - SSH session or local PTY
//!
//! ### Session Backend
//!
//! Sessions can be backed by different implementations:
//! - **Local**: Unix PTY for local shell
//! - **SSH**: Remote SSH connection
//! - Future: Telnet, serial, etc.

pub mod theme;
pub mod i18n;
pub mod input;
pub mod types;
pub mod pane;
pub mod terminal_render;
pub mod sftp_view;
pub mod hosts_view;
pub mod settings_view;
pub mod keychain_view;

// Re-export all public types for convenient access via `use ui::*`
pub use theme::{ThemeColors, ThemePreset};
pub use i18n::Language;
pub use types::{SessionBackend, TerminalSession, AddHostDialog, AppView, KeychainDeleteRequest, SftpContextMenu, SftpRenameDialog, SftpNewFolderDialog, SftpNewFileDialog, SftpConfirmDelete, SftpEditorDialog, SftpErrorDialog, BroadcastState};
pub use pane::{SplitDirection, PaneNode, PaneAction, Tab, DetachedWindow, TabDragState};
pub use terminal_render::render_pane_tree;
