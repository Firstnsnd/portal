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
//! - **terminal**: Terminal content rendering and text selection
//! - **views**: View-specific UI implementations (SFTP, Hosts, Settings, etc.)
//! - **widgets**: Reusable UI widgets
//! - **formatting**: Formatting utilities for display
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

pub mod tokens;
pub mod theme;
pub mod i18n;
pub mod fonts;
pub mod input;
pub mod types;
pub mod pane;
pub mod terminal;
pub mod views;
pub mod widgets;
pub mod formatting;

// Re-export all public types for convenient access via `use ui::*`
pub use theme::{ThemeColors, ThemePreset};
pub use i18n::Language;
pub use types::{SessionBackend, TerminalSession, AppView, BroadcastState};
pub use pane::{SplitDirection, PaneNode, PaneAction, Tab, DetachedWindow, TabDragState};
pub use terminal::render_pane_tree;
pub use views::tab_view::{TabBarAction, tab_bar, detached_tab_bar};
pub use widgets::nav_button;
pub use tokens::STATUS_BAR_HEIGHT;
