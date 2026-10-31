//! # Terminal Type Definitions
//!
//! This module contains core type definitions for terminal emulation,
//! including terminal cells and cell attributes.

use super::color::{DEFAULT_FG, DEFAULT_BG};

/// A single terminal cell with character and styling information.
#[derive(Debug, Clone, PartialEq)]
pub struct TerminalCell {
    /// The character displayed in this cell
    pub c: char,
    /// Foreground color as RGB tuple
    pub fg_color: (u8, u8, u8),
    /// Background color as RGB tuple
    pub bg_color: (u8, u8, u8),
    /// Bold text attribute
    pub bold: bool,
    /// Italic text attribute
    pub italic: bool,
    /// Underline text attribute
    pub underline: bool,
    /// Inverse video (swap foreground/background)
    pub inverse: bool,
    /// Dim/faint text attribute
    pub dim: bool,
    /// Strikethrough text attribute
    pub strikethrough: bool,
    /// True if this cell is a placeholder for the second half of a wide (CJK) character
    pub wide_continuation: bool,
}

impl Default for TerminalCell {
    fn default() -> Self {
        Self {
            c: ' ',
            fg_color: DEFAULT_FG,
            bg_color: DEFAULT_BG,
            bold: false,
            italic: false,
            underline: false,
            inverse: false,
            dim: false,
            strikethrough: false,
            wide_continuation: false,
        }
    }
}

/// Current SGR (Select Graphic Rendition) text attribute state.
///
/// This represents the accumulated text styling attributes that
/// should be applied to newly written characters.
#[derive(Debug, Clone)]
pub struct CellAttrs {
    /// Foreground color as RGB tuple
    pub fg_color: (u8, u8, u8),
    /// Background color as RGB tuple
    pub bg_color: (u8, u8, u8),
    /// Bold text attribute
    pub bold: bool,
    /// Italic text attribute
    pub italic: bool,
    /// Underline text attribute
    pub underline: bool,
    /// Inverse video (swap foreground/background)
    pub inverse: bool,
    /// Dim/faint text attribute
    pub dim: bool,
    /// Strikethrough text attribute
    pub strikethrough: bool,
}

impl Default for CellAttrs {
    fn default() -> Self {
        Self {
            fg_color: DEFAULT_FG,
            bg_color: DEFAULT_BG,
            bold: false,
            italic: false,
            underline: false,
            inverse: false,
            dim: false,
            strikethrough: false,
        }
    }
}
