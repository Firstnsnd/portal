//! PTY session management with vte-based ANSI parsing

use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Mutex};
use unicode_width::UnicodeWidthChar;
use vte::{Params, Perform};

pub use super::{Pty, PtySize, Result};

#[cfg(unix)]
pub use super::UnixPty;

#[cfg(windows)]
pub use super::WindowsPty;

// Standard ANSI 16-color palette
const ANSI_COLORS: [(u8, u8, u8); 16] = [
    (0, 0, 0),         // 0  Black
    (205, 49, 49),      // 1  Red
    (13, 188, 121),     // 2  Green
    (229, 229, 16),     // 3  Yellow
    (36, 114, 200),     // 4  Blue
    (188, 63, 188),     // 5  Magenta
    (17, 168, 205),     // 6  Cyan
    (229, 229, 229),    // 7  White
    (102, 102, 102),    // 8  Bright Black
    (241, 76, 76),      // 9  Bright Red
    (35, 209, 139),     // 10 Bright Green
    (245, 245, 67),     // 11 Bright Yellow
    (59, 142, 234),     // 12 Bright Blue
    (214, 112, 214),    // 13 Bright Magenta
    (41, 184, 219),     // 14 Bright Cyan
    (255, 255, 255),    // 15 Bright White
];

/// Default foreground and background colors (Tokyo Night)
pub const DEFAULT_FG: (u8, u8, u8) = (220, 228, 255);
pub const DEFAULT_BG: (u8, u8, u8) = (26, 27, 38);

/// Convert 256-color index to RGB
fn color_256_to_rgb(idx: u16) -> (u8, u8, u8) {
    if idx < 16 {
        ANSI_COLORS[idx as usize]
    } else if idx < 232 {
        // 6x6x6 color cube (indices 16-231)
        let idx = idx - 16;
        let r = (idx / 36) as u8;
        let g = ((idx % 36) / 6) as u8;
        let b = (idx % 6) as u8;
        (
            if r > 0 { r * 40 + 55 } else { 0 },
            if g > 0 { g * 40 + 55 } else { 0 },
            if b > 0 { b * 40 + 55 } else { 0 },
        )
    } else if idx < 256 {
        // Grayscale (indices 232-255)
        let v = (idx - 232) as u8 * 10 + 8;
        (v, v, v)
    } else {
        DEFAULT_FG
    }
}

/// Terminal cell with styling information
#[derive(Debug, Clone, PartialEq)]
pub struct TerminalCell {
    pub c: char,
    pub fg_color: (u8, u8, u8),
    pub bg_color: (u8, u8, u8),
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub dim: bool,
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

/// Current SGR (text attribute) state
#[derive(Debug, Clone)]
pub struct CellAttrs {
    pub fg_color: (u8, u8, u8),
    pub bg_color: (u8, u8, u8),
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub dim: bool,
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

/// Terminal grid state
#[derive(Debug, Clone)]
pub struct TerminalGrid {
    pub cols: usize,
    pub rows: usize,
    pub cells: Vec<Vec<TerminalCell>>,
    pub cursor_col: usize,
    pub cursor_row: usize,
    pub cursor_visible: bool,
    saved_cursor: Option<(usize, usize)>,
    alt_screen: Option<(Vec<Vec<TerminalCell>>, Vec<bool>, usize, usize)>, // cells, line_wrapped, cursor_col, cursor_row
    pub scroll_top: usize,
    pub scroll_bottom: usize,
    /// Deferred line wrap: cursor hit last column, wrap on next printable char
    wrap_pending: bool,
    /// Scrollback history buffer
    pub scrollback: VecDeque<Vec<TerminalCell>>,
    /// Maximum scrollback lines
    max_scrollback: usize,
    /// Per-row flag: true if this row was soft-wrapped (auto-wrap at column boundary)
    pub line_wrapped: Vec<bool>,
    /// Per-scrollback-row wrapped flag
    pub scrollback_wrapped: VecDeque<bool>,
}

impl TerminalGrid {
    pub fn new(cols: usize, rows: usize) -> Self {
        let cells = vec![vec![TerminalCell::default(); cols]; rows];
        Self {
            cols,
            rows,
            cells,
            cursor_col: 0,
            cursor_row: 0,
            cursor_visible: true,
            saved_cursor: None,
            alt_screen: None,
            scroll_top: 0,
            scroll_bottom: rows.saturating_sub(1),
            wrap_pending: false,
            scrollback: VecDeque::new(),
            max_scrollback: 10000,
            line_wrapped: vec![false; rows],
            scrollback_wrapped: VecDeque::new(),
        }
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        if cols == self.cols && rows == self.rows {
            return;
        }
        if cols != self.cols && self.alt_screen.is_none() {
            self.resize_reflow(cols, rows);
        } else {
            self.resize_screen(cols, rows);
        }
    }

    pub fn resize_screen(&mut self, cols: usize, rows: usize) {
        if cols == self.cols && rows == self.rows {
            return;
        }
        let mut new_cells = vec![vec![TerminalCell::default(); cols]; rows];
        let copy_rows = rows.min(self.rows);
        let copy_cols = cols.min(self.cols);
        for r in 0..copy_rows {
            for c in 0..copy_cols {
                new_cells[r][c] = self.cells[r][c].clone();
            }
        }
        // Rebuild line_wrapped for new row count
        let mut new_wrapped = vec![false; rows];
        for r in 0..rows.min(self.rows) {
            new_wrapped[r] = self.line_wrapped[r];
        }
        self.line_wrapped = new_wrapped;
        self.cells = new_cells;
        self.cols = cols;
        self.rows = rows;
        self.scroll_bottom = rows.saturating_sub(1);
        self.cursor_col = self.cursor_col.min(cols.saturating_sub(1));
        self.cursor_row = self.cursor_row.min(rows.saturating_sub(1));
    }

    pub fn resize_reflow(&mut self, new_cols: usize, new_rows: usize) {
        let old_cols = self.cols;
        if old_cols == new_cols {
            self.resize_screen(new_cols, new_rows);
            return;
        }

        // 1. Merge scrollback + screen into one list
        let sb_len = self.scrollback.len();
        let cursor_abs_row = sb_len + self.cursor_row;
        let cursor_abs_col = self.cursor_col;

        let mut all_rows: Vec<Vec<TerminalCell>> = Vec::new();
        let mut all_wrapped: Vec<bool> = Vec::new();
        for i in 0..sb_len {
            all_rows.push(self.scrollback[i].clone());
            all_wrapped.push(self.scrollback_wrapped[i]);
        }
        for i in 0..self.rows {
            all_rows.push(self.cells[i].clone());
            all_wrapped.push(self.line_wrapped[i]);
        }

        // 2. Trim trailing blank rows (below cursor) to avoid cursor-in-middle bug
        let mut last_content = cursor_abs_row;
        for (i, row) in all_rows.iter().enumerate() {
            if row.iter().any(|c| c.c != ' ' && c.c != '\0') {
                last_content = i;
            }
        }
        let trim_to = last_content.max(cursor_abs_row) + 1;
        all_rows.truncate(trim_to);
        all_wrapped.truncate(trim_to);

        // 3. Join wrapped rows into logical lines, tracking cursor
        let mut logical_lines: Vec<Vec<TerminalCell>> = Vec::new();
        let mut current_line: Vec<TerminalCell> = Vec::new();
        let mut cursor_logical = 0;
        let mut cursor_offset = 0;

        for (i, row) in all_rows.iter().enumerate() {
            if i == cursor_abs_row {
                cursor_logical = logical_lines.len();
                cursor_offset = current_line.len() + cursor_abs_col;
            }
            current_line.extend_from_slice(row);
            if i >= all_wrapped.len() || !all_wrapped[i] {
                logical_lines.push(std::mem::take(&mut current_line));
            }
        }
        if !current_line.is_empty() {
            logical_lines.push(current_line);
        }

        // 4. Re-wrap each logical line to new_cols
        let mut new_rows_vec: Vec<Vec<TerminalCell>> = Vec::new();
        let mut new_wrapped_vec: Vec<bool> = Vec::new();
        let mut new_cursor_row = 0;
        let mut new_cursor_col = 0;

        for (li, logical) in logical_lines.iter().enumerate() {
            // Find content length (trim trailing blanks for wrap calculation)
            let content_len = logical.iter().rposition(|c| c.c != ' ' && c.c != '\0')
                .map(|p| p + 1).unwrap_or(0);

            if content_len == 0 {
                // Empty logical line -> single blank row
                new_rows_vec.push(vec![TerminalCell::default(); new_cols]);
                new_wrapped_vec.push(false);
                if li == cursor_logical {
                    new_cursor_row = new_rows_vec.len() - 1;
                    new_cursor_col = cursor_offset.min(new_cols.saturating_sub(1));
                }
                continue;
            }

            let mut offset = 0;
            while offset < content_len || offset == 0 {
                let end = (offset + new_cols).min(logical.len());
                let mut row = vec![TerminalCell::default(); new_cols];
                for j in 0..(end - offset).min(new_cols) {
                    row[j] = logical[offset + j].clone();
                }

                let is_last = offset + new_cols >= content_len;

                if li == cursor_logical && cursor_offset >= offset
                    && (cursor_offset < offset + new_cols || is_last) {
                    new_cursor_row = new_rows_vec.len();
                    new_cursor_col = (cursor_offset - offset).min(new_cols.saturating_sub(1));
                }

                new_rows_vec.push(row);
                new_wrapped_vec.push(!is_last);
                offset += new_cols;
            }
        }

        // 5. Split into scrollback + screen
        let total = new_rows_vec.len();
        if total <= new_rows {
            self.scrollback.clear();
            self.scrollback_wrapped.clear();
            self.cells = new_rows_vec;
            self.line_wrapped = new_wrapped_vec;
            while self.cells.len() < new_rows {
                self.cells.push(vec![TerminalCell::default(); new_cols]);
                self.line_wrapped.push(false);
            }
            self.cursor_row = new_cursor_row;
        } else {
            let overflow = total - new_rows;
            self.scrollback.clear();
            self.scrollback_wrapped.clear();
            for i in 0..overflow {
                self.scrollback.push_back(new_rows_vec[i].clone());
                self.scrollback_wrapped.push_back(new_wrapped_vec[i]);
            }
            self.cells = new_rows_vec[overflow..].to_vec();
            self.line_wrapped = new_wrapped_vec[overflow..].to_vec();
            self.cursor_row = new_cursor_row.saturating_sub(overflow);
        }

        // Enforce scrollback limit
        while self.scrollback.len() > self.max_scrollback {
            self.scrollback.pop_front();
            self.scrollback_wrapped.pop_front();
        }

        self.cols = new_cols;
        self.rows = new_rows;
        self.scroll_bottom = new_rows.saturating_sub(1);
        self.cursor_col = new_cursor_col.min(new_cols.saturating_sub(1));
        self.cursor_row = self.cursor_row.min(new_rows.saturating_sub(1));
    }

    /// Write a character at cursor position with given attributes (deferred wrap)
    pub fn write_char_with_attrs(&mut self, c: char, attrs: &CellAttrs) {
        // If wrap is pending, perform the deferred line wrap now
        if self.wrap_pending {
            self.wrap_pending = false;
            self.line_wrapped[self.cursor_row] = true;
            self.cursor_col = 0;
            self.cursor_row += 1;
            if self.cursor_row > self.scroll_bottom {
                self.scroll_up(self.scroll_top, self.scroll_bottom);
                self.cursor_row = self.scroll_bottom;
            }
        }

        if self.cursor_col < self.cols && self.cursor_row < self.rows {
            let char_width = UnicodeWidthChar::width(c).unwrap_or(1);

            // For wide chars at the last column, wrap first
            if char_width == 2 && self.cursor_col + 1 >= self.cols {
                // Can't fit wide char — wrap to next line
                self.cursor_col = 0;
                self.cursor_row += 1;
                if self.cursor_row > self.scroll_bottom {
                    self.scroll_up(self.scroll_top, self.scroll_bottom);
                    self.cursor_row = self.scroll_bottom;
                }
            }

            let (fg, bg) = if attrs.inverse {
                (attrs.bg_color, attrs.fg_color)
            } else {
                (attrs.fg_color, attrs.bg_color)
            };
            self.cells[self.cursor_row][self.cursor_col] = TerminalCell {
                c,
                fg_color: fg,
                bg_color: bg,
                bold: attrs.bold,
                italic: attrs.italic,
                underline: attrs.underline,
                inverse: false, // already applied
                dim: attrs.dim,
                strikethrough: attrs.strikethrough,
                wide_continuation: false,
            };
            self.cursor_col += 1;

            // For wide (CJK) characters, place a continuation placeholder in the next cell
            if char_width == 2 && self.cursor_col < self.cols {
                self.cells[self.cursor_row][self.cursor_col] = TerminalCell {
                    c: ' ',
                    fg_color: fg,
                    bg_color: bg,
                    wide_continuation: true,
                    ..TerminalCell::default()
                };
                self.cursor_col += 1;
            }

            // At last column: don't wrap yet, set pending flag
            if self.cursor_col >= self.cols {
                self.cursor_col = self.cols.saturating_sub(1);
                self.wrap_pending = true;
            }
        }
    }

    pub fn clear(&mut self) {
        self.cells = vec![vec![TerminalCell::default(); self.cols]; self.rows];
        self.line_wrapped = vec![false; self.rows];
        self.cursor_col = 0;
        self.cursor_row = 0;
    }

    /// Erase from cursor to end of display
    pub fn erase_below(&mut self) {
        // Erase from cursor to end of current line
        for c in self.cursor_col..self.cols {
            self.cells[self.cursor_row][c] = TerminalCell::default();
        }
        // Erase all lines below
        for r in (self.cursor_row + 1)..self.rows {
            for c in 0..self.cols {
                self.cells[r][c] = TerminalCell::default();
            }
        }
        // Clear line_wrapped flags for erased rows
        if self.cursor_col == 0 && self.cursor_row > 0 {
            self.line_wrapped[self.cursor_row - 1] = false;
        }
        for r in self.cursor_row..self.rows {
            self.line_wrapped[r] = false;
        }
    }

    /// Erase from start of display to cursor
    pub fn erase_above(&mut self) {
        // Erase all lines above
        for r in 0..self.cursor_row {
            for c in 0..self.cols {
                self.cells[r][c] = TerminalCell::default();
            }
        }
        // Erase from start of current line to cursor
        for c in 0..=self.cursor_col.min(self.cols - 1) {
            self.cells[self.cursor_row][c] = TerminalCell::default();
        }
    }

    /// Erase from cursor to end of line
    pub fn erase_line_right(&mut self) {
        if self.cursor_row < self.rows {
            for c in self.cursor_col..self.cols {
                self.cells[self.cursor_row][c] = TerminalCell::default();
            }
        }
    }

    /// Erase from start of line to cursor
    pub fn erase_line_left(&mut self) {
        if self.cursor_row < self.rows {
            for c in 0..=self.cursor_col.min(self.cols - 1) {
                self.cells[self.cursor_row][c] = TerminalCell::default();
            }
        }
    }

    /// Erase entire current line
    pub fn erase_line_all(&mut self) {
        if self.cursor_row < self.rows {
            for c in 0..self.cols {
                self.cells[self.cursor_row][c] = TerminalCell::default();
            }
            self.line_wrapped[self.cursor_row] = false;
            // If previous row was wrapped into this one, clear its flag too
            if self.cursor_row > 0 {
                self.line_wrapped[self.cursor_row - 1] = false;
            }
        }
    }

    /// Scroll up within a region: remove top line, add blank at bottom
    /// When scrolling from the absolute top (top == 0), save the removed line to scrollback.
    pub fn scroll_up(&mut self, top: usize, bottom: usize) {
        if top < bottom && bottom < self.rows {
            // Save to scrollback only when scrolling from absolute top and not in alt screen
            if top == 0 && self.alt_screen.is_none() {
                let removed = self.cells.remove(top);
                let wrapped = self.line_wrapped.remove(top);
                self.scrollback.push_back(removed);
                self.scrollback_wrapped.push_back(wrapped);
                if self.scrollback.len() > self.max_scrollback {
                    self.scrollback.pop_front();
                    self.scrollback_wrapped.pop_front();
                }
            } else {
                self.cells.remove(top);
                self.line_wrapped.remove(top);
            }
            self.cells.insert(bottom, vec![TerminalCell::default(); self.cols]);
            self.line_wrapped.insert(bottom, false);
        }
    }

    /// Number of lines in scrollback history
    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

    /// Get a scrollback row by index (0 = oldest)
    pub fn get_scrollback_row(&self, idx: usize) -> Option<&Vec<TerminalCell>> {
        self.scrollback.get(idx)
    }

    /// Scroll down within a region: remove bottom line, add blank at top
    pub fn scroll_down(&mut self, top: usize, bottom: usize) {
        if top < bottom && bottom < self.rows {
            self.cells.remove(bottom);
            self.line_wrapped.remove(bottom);
            self.cells.insert(top, vec![TerminalCell::default(); self.cols]);
            self.line_wrapped.insert(top, false);
        }
    }

    /// Insert n blank lines at cursor row
    pub fn insert_lines(&mut self, n: usize) {
        let top = self.cursor_row;
        let bottom = self.scroll_bottom;
        for _ in 0..n {
            if top <= bottom && bottom < self.rows {
                self.cells.remove(bottom);
                self.line_wrapped.remove(bottom);
                self.cells.insert(top, vec![TerminalCell::default(); self.cols]);
                self.line_wrapped.insert(top, false);
            }
        }
    }

    /// Delete n lines at cursor row
    pub fn delete_lines(&mut self, n: usize) {
        let top = self.cursor_row;
        let bottom = self.scroll_bottom;
        for _ in 0..n {
            if top <= bottom && bottom < self.rows {
                self.cells.remove(top);
                self.line_wrapped.remove(top);
                self.cells.insert(bottom, vec![TerminalCell::default(); self.cols]);
                self.line_wrapped.insert(bottom, false);
            }
        }
    }

    /// Insert n blank characters at cursor position
    pub fn insert_chars(&mut self, n: usize) {
        if self.cursor_row < self.rows {
            let row = &mut self.cells[self.cursor_row];
            for _ in 0..n {
                if self.cursor_col < self.cols {
                    row.pop();
                    row.insert(self.cursor_col, TerminalCell::default());
                }
            }
        }
    }

    /// Delete n characters at cursor position
    pub fn delete_chars(&mut self, n: usize) {
        if self.cursor_row < self.rows {
            let row = &mut self.cells[self.cursor_row];
            for _ in 0..n {
                if self.cursor_col < row.len() {
                    row.remove(self.cursor_col);
                    row.push(TerminalCell::default());
                }
            }
        }
    }

    /// Enter alternate screen buffer
    pub fn enter_alt_screen(&mut self) {
        let saved_cells = self.cells.clone();
        let saved_wrapped = self.line_wrapped.clone();
        self.alt_screen = Some((saved_cells, saved_wrapped, self.cursor_col, self.cursor_row));
        self.cells = vec![vec![TerminalCell::default(); self.cols]; self.rows];
        self.line_wrapped = vec![false; self.rows];
        self.cursor_col = 0;
        self.cursor_row = 0;
    }

    /// Exit alternate screen buffer
    pub fn exit_alt_screen(&mut self) {
        if let Some((cells, wrapped, col, row)) = self.alt_screen.take() {
            self.cells = cells;
            self.line_wrapped = wrapped;
            self.cursor_col = col;
            self.cursor_row = row;
        }
    }
}

/// VTE handler that implements vte::Perform
pub struct VteHandler<'a> {
    pub grid: &'a mut TerminalGrid,
    pub attrs: &'a mut CellAttrs,
}

impl<'a> VteHandler<'a> {
    /// Parse SGR parameters and update attrs
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
                // Extended foreground: check subparams first, then next params
                if param.len() >= 3 && param[1] == 5 {
                    // Subparam form: 38:5:n
                    self.attrs.fg_color = color_256_to_rgb(param[2]);
                } else if param.len() >= 5 && param[1] == 2 {
                    // Subparam form: 38:2:r:g:b
                    self.attrs.fg_color = (param[2] as u8, param[3] as u8, param[4] as u8);
                } else if let Some(mode) = iter.next() {
                    let mode_val = mode.first().copied().unwrap_or(0);
                    if mode_val == 5 {
                        if let Some(color) = iter.next() {
                            self.attrs.fg_color = color_256_to_rgb(color.first().copied().unwrap_or(0));
                        }
                    } else if mode_val == 2 {
                        let r = iter.next().and_then(|p| p.first().copied()).unwrap_or(0) as u8;
                        let g = iter.next().and_then(|p| p.first().copied()).unwrap_or(0) as u8;
                        let b = iter.next().and_then(|p| p.first().copied()).unwrap_or(0) as u8;
                        self.attrs.fg_color = (r, g, b);
                    }
                }
            }
            39 => self.attrs.fg_color = DEFAULT_FG,
            // Standard background colors
            40..=47 => {
                self.attrs.bg_color = ANSI_COLORS[(val - 40) as usize];
            }
            48 => {
                // Extended background
                if param.len() >= 3 && param[1] == 5 {
                    self.attrs.bg_color = color_256_to_rgb(param[2]);
                } else if param.len() >= 5 && param[1] == 2 {
                    self.attrs.bg_color = (param[2] as u8, param[3] as u8, param[4] as u8);
                } else if let Some(mode) = iter.next() {
                    let mode_val = mode.first().copied().unwrap_or(0);
                    if mode_val == 5 {
                        if let Some(color) = iter.next() {
                            self.attrs.bg_color = color_256_to_rgb(color.first().copied().unwrap_or(0));
                        }
                    } else if mode_val == 2 {
                        let r = iter.next().and_then(|p| p.first().copied()).unwrap_or(0) as u8;
                        let g = iter.next().and_then(|p| p.first().copied()).unwrap_or(0) as u8;
                        let b = iter.next().and_then(|p| p.first().copied()).unwrap_or(0) as u8;
                        self.attrs.bg_color = (r, g, b);
                    }
                }
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
                let n = if p1 == 0 { 1 } else { p1 as usize };
                self.grid.cursor_row = self.grid.cursor_row.saturating_sub(n);
            }
            'B' => {
                // CUD - Cursor Down
                let n = if p1 == 0 { 1 } else { p1 as usize };
                self.grid.cursor_row = (self.grid.cursor_row + n).min(self.grid.rows.saturating_sub(1));
            }
            'C' => {
                // CUF - Cursor Forward
                let n = if p1 == 0 { 1 } else { p1 as usize };
                self.grid.cursor_col = (self.grid.cursor_col + n).min(self.grid.cols.saturating_sub(1));
            }
            'D' => {
                // CUB - Cursor Back
                let n = if p1 == 0 { 1 } else { p1 as usize };
                self.grid.cursor_col = self.grid.cursor_col.saturating_sub(n);
            }
            'E' => {
                // CNL - Cursor Next Line
                let n = if p1 == 0 { 1 } else { p1 as usize };
                self.grid.cursor_col = 0;
                self.grid.cursor_row = (self.grid.cursor_row + n).min(self.grid.rows.saturating_sub(1));
            }
            'F' => {
                // CPL - Cursor Previous Line
                let n = if p1 == 0 { 1 } else { p1 as usize };
                self.grid.cursor_col = 0;
                self.grid.cursor_row = self.grid.cursor_row.saturating_sub(n);
            }
            'G' => {
                // CHA - Cursor Horizontal Absolute
                let col = if p1 == 0 { 1 } else { p1 as usize };
                self.grid.cursor_col = (col - 1).min(self.grid.cols.saturating_sub(1));
            }
            'H' | 'f' => {
                // CUP/HVP - Cursor Position
                let row = if p1 == 0 { 1 } else { p1 as usize };
                let col = param_list.get(1).copied().unwrap_or(1).max(1) as usize;
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
                let n = if p1 == 0 { 1 } else { p1 as usize };
                self.grid.insert_lines(n);
            }
            'M' => {
                // DL - Delete Lines
                let n = if p1 == 0 { 1 } else { p1 as usize };
                self.grid.delete_lines(n);
            }
            '@' => {
                // ICH - Insert Characters
                let n = if p1 == 0 { 1 } else { p1 as usize };
                self.grid.insert_chars(n);
            }
            'P' => {
                // DCH - Delete Characters
                let n = if p1 == 0 { 1 } else { p1 as usize };
                self.grid.delete_chars(n);
            }
            'S' => {
                // SU - Scroll Up
                let n = if p1 == 0 { 1 } else { p1 as usize };
                for _ in 0..n {
                    self.grid.scroll_up(self.grid.scroll_top, self.grid.scroll_bottom);
                }
            }
            'T' => {
                // SD - Scroll Down
                let n = if p1 == 0 { 1 } else { p1 as usize };
                for _ in 0..n {
                    self.grid.scroll_down(self.grid.scroll_top, self.grid.scroll_bottom);
                }
            }
            'X' => {
                // ECH - Erase Characters
                let n = if p1 == 0 { 1 } else { p1 as usize };
                for i in 0..n {
                    let col = self.grid.cursor_col + i;
                    if col < self.grid.cols && self.grid.cursor_row < self.grid.rows {
                        self.grid.cells[self.grid.cursor_row][col] = TerminalCell::default();
                    }
                }
            }
            'd' => {
                // VPA - Vertical Position Absolute
                let row = if p1 == 0 { 1 } else { p1 as usize };
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
                    let top = if p1 == 0 { 1 } else { p1 as usize };
                    let bottom = param_list.get(1).copied().unwrap_or(self.grid.rows as u16).max(1) as usize;
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

    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {
        // OSC sequences (window title, etc.) — ignore for now
    }
}

/// Safe wrapper for writing to a raw fd without closing it on drop
#[cfg(unix)]
struct PtyWriter {
    fd: std::os::unix::io::RawFd,
}

#[cfg(unix)]
impl PtyWriter {
    fn write(&self, data: &[u8]) -> io::Result<()> {
        use std::mem::ManuallyDrop;
        use std::os::unix::io::FromRawFd;
        let mut file = ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(self.fd) });
        std::io::Write::write_all(&mut *file, data)
    }
}

/// Real PTY session with background reader thread
pub struct RealPtySession {
    #[cfg(unix)]
    pty: Option<UnixPty>,
    #[cfg(unix)]
    writer: Option<PtyWriter>,
    #[cfg(windows)]
    pty: Option<WindowsPty>,
    pub grid: Arc<Mutex<TerminalGrid>>,
    alive: Arc<std::sync::atomic::AtomicBool>,
    _reader_thread: Option<std::thread::JoinHandle<()>>,
}

impl RealPtySession {
    #[cfg(unix)]
    pub fn new(id: usize, cols: u16, rows: u16, shell: &str) -> Result<Self> {
        use std::os::unix::io::{AsRawFd, FromRawFd};
        use std::sync::atomic::{AtomicBool, Ordering};
        let _ = id;

        let grid = Arc::new(Mutex::new(TerminalGrid::new(cols as usize, rows as usize)));
        let pty = UnixPty::spawn(shell, &["-l"], PtySize::new(rows, cols))?;

        // Dup the master fd: one for reading (background thread), one for writing (main thread)
        let master_fd = pty.as_raw_fd();
        let reader_fd = unsafe { libc::dup(master_fd) };
        if reader_fd < 0 {
            return Err(super::Error::SpawnFailed("Failed to dup fd for reader".to_string()));
        }
        let writer_fd = unsafe { libc::dup(master_fd) };
        if writer_fd < 0 {
            unsafe { libc::close(reader_fd); }
            return Err(super::Error::SpawnFailed("Failed to dup fd for writer".to_string()));
        }

        let alive = Arc::new(AtomicBool::new(true));
        let alive_clone = Arc::clone(&alive);
        let grid_clone = Arc::clone(&grid);

        // Spawn background reader thread
        let reader_thread = std::thread::spawn(move || {
            use std::io::Read;

            let mut reader_file = unsafe { std::fs::File::from_raw_fd(reader_fd) };
            let mut parser = vte::Parser::new();
            let mut attrs = CellAttrs::default();
            let mut buf = [0u8; 8192];

            // Set non-blocking mode
            unsafe {
                let flags = libc::fcntl(reader_fd, libc::F_GETFL);
                if flags >= 0 {
                    libc::fcntl(reader_fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
                }
            }

            loop {
                if !alive_clone.load(Ordering::Relaxed) {
                    break;
                }

                match reader_file.read(&mut buf) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        if let Ok(mut grid) = grid_clone.lock() {
                            let mut handler = VteHandler {
                                grid: &mut grid,
                                attrs: &mut attrs,
                            };
                            for byte in &buf[..n] {
                                parser.advance(&mut handler, *byte);
                            }
                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        // No data available, sleep briefly to avoid busy-wait
                        std::thread::sleep(std::time::Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            pty: Some(pty),
            writer: Some(PtyWriter { fd: writer_fd }),
            grid,
            alive,
            _reader_thread: Some(reader_thread),
        })
    }

    #[cfg(windows)]
    pub fn new(id: usize, cols: u16, rows: u16, _shell: &str) -> Result<Self> {
        let _ = id;
        let _grid = Arc::new(Mutex::new(TerminalGrid::new(cols as usize, rows as usize)));
        Err(super::Error::SpawnFailed("Windows ConPTY not yet implemented".to_string()))
    }

    pub fn write(&mut self, data: &[u8]) -> io::Result<()> {
        #[cfg(unix)]
        if let Some(ref writer) = self.writer {
            writer.write(data)?;
        }
        Ok(())
    }

    pub fn get_grid(&self) -> Arc<Mutex<TerminalGrid>> {
        Arc::clone(&self.grid)
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> io::Result<()> {
        #[cfg(unix)]
        if let Some(ref mut pty) = self.pty {
            pty.resize(PtySize::new(rows, cols))
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        }
        if let Ok(mut grid) = self.grid.lock() {
            grid.resize(cols as usize, rows as usize);
        }
        Ok(())
    }
}

impl Drop for RealPtySession {
    fn drop(&mut self) {
        // Signal reader thread to stop
        self.alive.store(false, std::sync::atomic::Ordering::Relaxed);

        // Close writer fd
        #[cfg(unix)]
        {
            if let Some(writer) = self.writer.take() {
                unsafe { libc::close(writer.fd); }
            }
        }

        // Kill the PTY child process
        if let Some(ref mut pty) = self.pty {
            let _ = pty.kill();
        }
    }
}
