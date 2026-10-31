//! # Terminal UI Rendering
//!
//! This module contains the terminal rendering system for the Portal application.
//! It handles all terminal visualization including text rendering, mouse interaction,
//! keyboard input, and IME support for CJK languages.
//!
//! ## Architecture
//!
//! ```text
//! render_pane_tree()
//!     └── render_terminal_pane()
//!             └── render_terminal_session()
//!                     ├── Text rendering (grid + scrollback)
//!                     ├── Mouse selection
//!                     ├── Keyboard input
//!                     └── IME support
//! ```
//!
//! ## Modules
//!
//! - **render**: Main terminal rendering functions
//! - **selection**: Text selection and word boundary detection
//! - **background**: User-configurable background image layer

pub mod render;
pub mod selection;
pub mod auth_prompt_ui;
pub mod background;

// Re-export commonly used functions
pub use render::render_pane_tree;
pub use background::TerminalBackground;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
