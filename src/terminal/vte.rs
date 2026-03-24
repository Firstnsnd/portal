//! # VTE (Virtual Terminal Emulator) Handler
//!
//! This module contains the VTE parser implementation that processes
//! ANSI escape sequences and updates the terminal grid state.

use vte::{Params, Perform};

use super::color::{DEFAULT_FG, DEFAULT_BG, ANSI_COLORS, parse_extended_color};
use super::types::TerminalCell;
use super::grid::TerminalGrid;
use super::types::CellAttrs;

/// Get CSI parameter value, defaulting to 1 if 0
#[inline]
pub fn param_or_one(p: u16) -> usize {
    if p == 0 { 1 } else { p as usize }
}

/// Get CSI parameter value, defaulting to 1 if 0 (usize version)
#[inline]
pub fn param_or_one_usize(p: usize) -> usize {
    if p == 0 { 1 } else { p }
}

/// URL decode a percent-encoded string
pub fn urlencoding_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
            } else {
                result.push('%');
                result.push_str(&hex);
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// VTE handler that implements vte::Perform trait.
///
/// This struct processes ANSI escape sequences and updates the terminal
/// grid state accordingly. It borrows mutable references to the grid
/// and current cell attributes.
pub struct VteHandler<'a> {
    /// Reference to the terminal grid being updated
    pub grid: &'a mut TerminalGrid,
    /// Reference to current cell attributes
    pub attrs: &'a mut CellAttrs,
}

impl<'a> VteHandler<'a> {
    /// Parse SGR (Select Graphic Rendition) parameters and update attrs.
    fn handle_sgr(&mut self, params: &Params) {
        let mut iter = params.iter();
        // If no params, treat as reset
        let first = match iter.next() {
            Some(p) => p,
            None => {
                *self.attrs = CellAttrs::default();
                return;
            }
        };

        // Process all param groups
        self.process_sgr_param(first, &mut iter);
        while let Some(param) = iter.next() {
            self.process_sgr_param(param, &mut iter);
        }
    }

    /// Process a single SGR parameter group.
    fn process_sgr_param<'b, I>(&mut self, param: &[u16], iter: &mut I)
    where
        I: Iterator<Item = &'b [u16]>,
    {
        let val = param.first().copied().unwrap_or(0);
        match val {
            0 => *self.attrs = CellAttrs::default(),
            1 => self.attrs.bold = true,
            2 => self.attrs.dim = true,
            3 => self.attrs.italic = true,
            4 => self.attrs.underline = true,
            7 => self.attrs.inverse = true,
            9 => self.attrs.strikethrough = true,
            22 => { self.attrs.bold = false; self.attrs.dim = false; }
            23 => self.attrs.italic = false,
            24 => self.attrs.underline = false,
            27 => self.attrs.inverse = false,
            29 => self.attrs.strikethrough = false,
            // Standard foreground colors
            30..=37 => {
                self.attrs.fg_color = ANSI_COLORS[(val - 30) as usize];
            }
            38 => {
                self.attrs.fg_color = parse_extended_color(param, iter, DEFAULT_FG);
            }
            39 => self.attrs.fg_color = DEFAULT_FG,
            // Standard background colors
            40..=47 => {
                self.attrs.bg_color = ANSI_COLORS[(val - 40) as usize];
            }
            48 => {
                self.attrs.bg_color = parse_extended_color(param, iter, DEFAULT_BG);
            }
            49 => self.attrs.bg_color = DEFAULT_BG,
            // Bright foreground colors
            90..=97 => {
                self.attrs.fg_color = ANSI_COLORS[(val - 90 + 8) as usize];
            }
            // Bright background colors
            100..=107 => {
                self.attrs.bg_color = ANSI_COLORS[(val - 100 + 8) as usize];
            }
            _ => {}
        }
    }
}

impl<'a> Perform for VteHandler<'a> {
    fn print(&mut self, c: char) {
        self.grid.write_char_with_attrs(c, self.attrs);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            0x07 => { /* BEL - bell, ignore */ }
            0x08 => {
                // BS - backspace
                self.grid.wrap_pending = false;
                self.grid.cursor_col = self.grid.cursor_col.saturating_sub(1);
            }
            0x09 => {
                // HT - horizontal tab
                self.grid.wrap_pending = false;
                self.grid.cursor_col = ((self.grid.cursor_col / 8) + 1) * 8;
                if self.grid.cursor_col >= self.grid.cols {
                    self.grid.cursor_col = self.grid.cols.saturating_sub(1);
                }
            }
            0x0A | 0x0B | 0x0C => {
                // LF, VT, FF - line feed
                self.grid.wrap_pending = false;
                self.grid.cursor_row += 1;
                if self.grid.cursor_row > self.grid.scroll_bottom {
                    self.grid.scroll_up(self.grid.scroll_top, self.grid.scroll_bottom);
                    self.grid.cursor_row = self.grid.scroll_bottom;
                }
            }
            0x0D => {
                // CR - carriage return: stay on current row, go to col 0
                self.grid.wrap_pending = false;
                self.grid.cursor_col = 0;
            }
            _ => {}
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        // Parse OSC sequences
        // OSC 7: file://hostname/path - current directory (used by shell integration)
        if params.len() >= 2 {
            let cmd = params[0];
            if cmd == b"7" || cmd == b"7;" {
                // OSC 7: set current directory
                if let Ok(url) = std::str::from_utf8(params[1]) {
                    // Parse file://hostname/path
                    if let Some(path) = url.strip_prefix("file://") {
                        // Skip hostname part
                        if let Some(slash_pos) = path.find('/') {
                            let path = &path[slash_pos..];
                            // URL decode the path
                            let decoded = urlencoding_decode(path);
                            self.grid.cwd = Some(decoded);
                        }
                    }
                }
            }
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        let is_private = intermediates.first() == Some(&b'?');

        // Any CSI sequence clears the pending wrap state
        self.grid.wrap_pending = false;

        // Collect params into a vec for indexed access
        let param_list: Vec<u16> = params.iter()
            .map(|p| p.first().copied().unwrap_or(0))
            .collect();

        let p1 = param_list.first().copied().unwrap_or(0);

        match action {
            // Cursor movement
            'A' => {
                // CUU - Cursor Up
                self.grid.cursor_row = self.grid.cursor_row.saturating_sub(param_or_one(p1));
            }
            'B' => {
                // CUD - Cursor Down
                self.grid.cursor_row = (self.grid.cursor_row + param_or_one(p1)).min(self.grid.rows.saturating_sub(1));
            }
            'C' => {
                // CUF - Cursor Forward
                self.grid.cursor_col = (self.grid.cursor_col + param_or_one(p1)).min(self.grid.cols.saturating_sub(1));
            }
            'D' => {
                // CUB - Cursor Back
                self.grid.cursor_col = self.grid.cursor_col.saturating_sub(param_or_one(p1));
            }
            'E' => {
                // CNL - Cursor Next Line
                self.grid.cursor_col = 0;
                self.grid.cursor_row = (self.grid.cursor_row + param_or_one(p1)).min(self.grid.rows.saturating_sub(1));
            }
            'F' => {
                // CPL - Cursor Previous Line
                self.grid.cursor_col = 0;
                self.grid.cursor_row = self.grid.cursor_row.saturating_sub(param_or_one(p1));
            }
            'G' => {
                // CHA - Cursor Horizontal Absolute
                let col = param_or_one_usize(p1 as usize);
                self.grid.cursor_col = (col - 1).min(self.grid.cols.saturating_sub(1));
            }
            'H' | 'f' => {
                // CUP/HVP - Cursor Position
                let row = param_or_one_usize(p1 as usize);
                let col = param_or_one_usize(param_list.get(1).copied().unwrap_or(1) as usize);
                self.grid.cursor_row = (row - 1).min(self.grid.rows.saturating_sub(1));
                self.grid.cursor_col = (col - 1).min(self.grid.cols.saturating_sub(1));
            }
            'J' => {
                // ED - Erase in Display
                match p1 {
                    0 => self.grid.erase_below(),
                    1 => self.grid.erase_above(),
                    2 | 3 => self.grid.clear(),
                    _ => {}
                }
            }
            'K' => {
                // EL - Erase in Line
                match p1 {
                    0 => self.grid.erase_line_right(),
                    1 => self.grid.erase_line_left(),
                    2 => self.grid.erase_line_all(),
                    _ => {}
                }
            }
            'L' => {
                // IL - Insert Lines
                self.grid.insert_lines(param_or_one(p1));
            }
            'M' => {
                // DL - Delete Lines
                self.grid.delete_lines(param_or_one(p1));
            }
            '@' => {
                // ICH - Insert Characters
                self.grid.insert_chars(param_or_one(p1));
            }
            'P' => {
                // DCH - Delete Characters
                self.grid.delete_chars(param_or_one(p1));
            }
            'S' => {
                // SU - Scroll Up
                for _ in 0..param_or_one(p1) {
                    self.grid.scroll_up(self.grid.scroll_top, self.grid.scroll_bottom);
                }
            }
            'T' => {
                // SD - Scroll Down
                for _ in 0..param_or_one(p1) {
                    self.grid.scroll_down(self.grid.scroll_top, self.grid.scroll_bottom);
                }
            }
            'X' => {
                // ECH - Erase Characters
                for i in 0..param_or_one(p1) {
                    let col = self.grid.cursor_col + i;
                    if col < self.grid.cols && self.grid.cursor_row < self.grid.rows {
                        self.grid.cells[self.grid.cursor_row][col] = TerminalCell::default();
                    }
                }
            }
            'd' => {
                // VPA - Vertical Position Absolute
                let row = param_or_one_usize(p1 as usize);
                self.grid.cursor_row = (row - 1).min(self.grid.rows.saturating_sub(1));
            }
            'm' => {
                // SGR - Select Graphic Rendition
                self.handle_sgr(params);
            }
            's' => {
                // Save cursor position
                if !is_private {
                    self.grid.saved_cursor = Some((self.grid.cursor_col, self.grid.cursor_row));
                }
            }
            'u' => {
                // Restore cursor position
                if let Some((col, row)) = self.grid.saved_cursor {
                    self.grid.cursor_col = col;
                    self.grid.cursor_row = row;
                }
            }
            'r' => {
                // DECSTBM - Set Scrolling Region
                if !is_private {
                    let top = param_or_one_usize(p1 as usize);
                    let bottom = param_or_one_usize(param_list.get(1).copied().unwrap_or(self.grid.rows as u16) as usize).max(1);
                    self.grid.scroll_top = (top - 1).min(self.grid.rows.saturating_sub(1));
                    self.grid.scroll_bottom = (bottom - 1).min(self.grid.rows.saturating_sub(1));
                    // Move cursor to home after setting scroll region
                    self.grid.cursor_col = 0;
                    self.grid.cursor_row = 0;
                }
            }
            'h' => {
                // SM / DECSET
                if is_private {
                    match p1 {
                        25 => self.grid.cursor_visible = true,
                        1049 => self.grid.enter_alt_screen(),
                        _ => {}
                    }
                }
            }
            'l' => {
                // RM / DECRST
                if is_private {
                    match p1 {
                        25 => self.grid.cursor_visible = false,
                        1049 => self.grid.exit_alt_screen(),
                        _ => {}
                    }
                }
            }
            'n' => {
                // DSR - Device Status Report (we just ignore the request)
            }
            'c' => {
                // DA - Device Attributes (ignore)
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        match (intermediates, byte) {
            ([], b'7') | ([b'('], _) => {
                // DECSC or charset selection — save cursor
                self.grid.saved_cursor = Some((self.grid.cursor_col, self.grid.cursor_row));
            }
            ([], b'8') => {
                // DECRC — restore cursor
                if let Some((col, row)) = self.grid.saved_cursor {
                    self.grid.cursor_col = col;
                    self.grid.cursor_row = row;
                }
            }
            ([], b'D') => {
                // IND - Index (move cursor down, scroll if needed)
                self.grid.cursor_row += 1;
                if self.grid.cursor_row > self.grid.scroll_bottom {
                    self.grid.scroll_up(self.grid.scroll_top, self.grid.scroll_bottom);
                    self.grid.cursor_row = self.grid.scroll_bottom;
                }
            }
            ([], b'M') => {
                // RI - Reverse Index (move cursor up, scroll if needed)
                if self.grid.cursor_row == self.grid.scroll_top {
                    self.grid.scroll_down(self.grid.scroll_top, self.grid.scroll_bottom);
                } else {
                    self.grid.cursor_row = self.grid.cursor_row.saturating_sub(1);
                }
            }
            ([], b'E') => {
                // NEL - Next Line
                self.grid.cursor_col = 0;
                self.grid.cursor_row += 1;
                if self.grid.cursor_row > self.grid.scroll_bottom {
                    self.grid.scroll_up(self.grid.scroll_top, self.grid.scroll_bottom);
                    self.grid.cursor_row = self.grid.scroll_bottom;
                }
            }
            _ => {}
        }
    }
}
