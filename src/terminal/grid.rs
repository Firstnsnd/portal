//! # Terminal Grid

//! This module contains the terminal grid implementation that manages
//! the visible screen content, cursor position, scrollback history,
//! and related terminal state.

use std::collections::VecDeque;
use unicode_width::UnicodeWidthChar;

use super::types::TerminalCell;
use super::types::CellAttrs;

/// Terminal grid state representing the visible screen content.
///
/// The grid contains the current screen content as a 2D array of cells,
/// plus scrollback history for content that has scrolled off the top.
pub struct TerminalGrid {
    /// Number of columns in the grid
    pub cols: usize,
    /// Number of rows in the grid
    pub rows: usize,
    /// 2D array of terminal cells [row][col]
    pub cells: Vec<Vec<TerminalCell>>,
    /// Current cursor column position (0-based)
    pub cursor_col: usize,
    /// Current cursor row position (0-based)
    pub cursor_row: usize,
    /// Whether the cursor is visible
    pub cursor_visible: bool,
    /// Saved cursor position for DECSC/DECRC
    pub saved_cursor: Option<(usize, usize)>,
    /// Alternate screen buffer state (cells, line_wrapped, cursor_col, cursor_row)
    alt_screen: Option<(Vec<Vec<TerminalCell>>, Vec<bool>, usize, usize)>,
    /// Top row of scrolling region (inclusive)
    pub scroll_top: usize,
    /// Bottom row of scrolling region (inclusive)
    pub scroll_bottom: usize,
    /// Deferred line wrap: cursor hit last column, wrap on next printable char
    pub wrap_pending: bool,
    /// Scrollback history buffer (oldest at front)
    pub scrollback: VecDeque<Vec<TerminalCell>>,
    /// Maximum scrollback memory in bytes (default: 100MB)
    max_scrollback_bytes: usize,
    /// Current scrollback memory usage in bytes
    current_scrollback_bytes: usize,
    /// Per-row flag: true if this row was soft-wrapped (auto-wrap at column boundary)
    pub line_wrapped: Vec<bool>,
    /// Per-scrollback-row wrapped flag
    pub scrollback_wrapped: VecDeque<bool>,
    /// Current working directory (updated via OSC 7 sequence)
    pub cwd: Option<String>,
    /// Counter for protecting against shell erase_after immediately after resize
    /// Only protects the FIRST erase_below call after each resize
    resize_protection_count: u8,
}

impl TerminalGrid {
    /// Default scrollback limit: 100MB
    pub const DEFAULT_MAX_SCROLLBACK_BYTES: usize = 100 * 1024 * 1024;

    /// Create a new grid with default scrollback limit.
    #[allow(dead_code)]
    pub fn new(cols: usize, rows: usize) -> Self {
        Self::with_scrollback_limit(cols, rows, Self::DEFAULT_MAX_SCROLLBACK_BYTES)
    }

    /// Create a new grid with a specific scrollback memory limit.
    pub fn with_scrollback_limit(cols: usize, rows: usize, max_bytes: usize) -> Self {
        let cells = vec![vec![TerminalCell::default(); cols]; rows];
        // Get initial cwd
        let cwd = std::env::current_dir().ok().map(|p| p.to_string_lossy().to_string());
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
            max_scrollback_bytes: max_bytes,
            current_scrollback_bytes: 0,
            line_wrapped: vec![false; rows],
            scrollback_wrapped: VecDeque::new(),
            cwd,
            resize_protection_count: 0,
        }
    }

    /// Update the scrollback memory limit (e.g., from settings change).
    #[allow(dead_code)]
    pub fn set_scrollback_limit(&mut self, max_bytes: usize) {
        self.max_scrollback_bytes = max_bytes;
        // Trim if currently over the new limit
        while self.current_scrollback_bytes > self.max_scrollback_bytes {
            if let Some(oldest) = self.scrollback.pop_front() {
                let oldest_bytes = Self::row_memory_usage(&oldest);
                self.current_scrollback_bytes -= oldest_bytes;
            }
            self.scrollback_wrapped.pop_front();
        }
    }

    /// Resize the grid to new dimensions.
    ///
    /// Uses reflow logic when column count changes (not in alt screen).
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

    /// Resize the grid without reflow (simple resize).
    fn resize_screen(&mut self, cols: usize, rows: usize) {
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

    /// Write a character to the grid at the current cursor position with given attributes.
    pub fn write_char_with_attrs(&mut self, c: char, attrs: &CellAttrs) {
        // Clear resize protection when new input arrives
        // This means the resize operation is complete and shell has finished redrawing
        self.resize_protection_count = 0;

        // Handle control characters
        if c < ' ' && c != '\t' && c != '\n' && c != '\r' {
            return; // Skip other control characters
        }

        // Get character width (1 or 2 for CJK)
        let char_width = c.width().unwrap_or(1);

        // Handle pending wrap from hitting last column
        if self.wrap_pending {
            // Mark the previous row as wrapped
            if self.cursor_row < self.rows {
                self.line_wrapped[self.cursor_row] = true;
            }
            self.cursor_col = 0;
            self.cursor_row += 1;
            if self.cursor_row > self.scroll_bottom {
                self.scroll_up(self.scroll_top, self.scroll_bottom);
                self.cursor_row = self.scroll_bottom;
            }
            self.wrap_pending = false;
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

    /// Clear the entire grid and reset cursor to home position.
    pub fn clear(&mut self) {
        self.cells = vec![vec![TerminalCell::default(); self.cols]; self.rows];
        self.line_wrapped = vec![false; self.rows];
        self.cursor_col = 0;
        self.cursor_row = 0;
    }

    /// Erase from cursor to end of display.
    pub fn erase_below(&mut self) {
        // If we have resize protection active, decrement the counter and skip erasing
        // This prevents the shell's SIGWINCH response (erase_below) from destroying
        // our reflowed content, but only for the first call after each resize
        if self.resize_protection_count > 0 {
            self.resize_protection_count -= 1;
            return;
        }

        // When erasing from column 0, also clear wrapped continuation rows above
        // the cursor that are part of the same logical line (created by reflow).
        if self.cursor_col == 0 {
            let mut r = self.cursor_row;
            while r > 0 && self.line_wrapped[r - 1] {
                r -= 1;
                self.clear_row(r);
                self.line_wrapped[r] = false;
            }
            if r > 0 {
                self.line_wrapped[r - 1] = false;
            }
        }

        // Erase from cursor to end of current line
        self.clear_row_range(self.cursor_row, self.cursor_col, self.cols);
        // Erase all lines below
        for r in (self.cursor_row + 1)..self.rows {
            self.clear_row(r);
        }
        // Clear line_wrapped flags for erased rows
        for r in self.cursor_row..self.rows {
            self.line_wrapped[r] = false;
        }
    }

    /// Erase from start of display to cursor.
    pub fn erase_above(&mut self) {
        // Erase all lines above
        for r in 0..self.cursor_row {
            self.clear_row(r);
        }
        // Erase from start of current line to cursor
        self.clear_row_range(self.cursor_row, 0, self.cursor_col + 1);
    }

    /// Erase from cursor to end of line.
    pub fn erase_line_right(&mut self) {
        self.clear_row_range(self.cursor_row, self.cursor_col, self.cols);
    }

    /// Erase from start of line to cursor.
    pub fn erase_line_left(&mut self) {
        self.clear_row_range(self.cursor_row, 0, self.cursor_col + 1);
    }

    /// Erase entire current line.
    pub fn erase_line_all(&mut self) {
        if self.cursor_row < self.rows {
            self.clear_row(self.cursor_row);
            self.line_wrapped[self.cursor_row] = false;
            // If previous row was wrapped into this one, clear its flag too
            if self.cursor_row > 0 {
                self.line_wrapped[self.cursor_row - 1] = false;
            }
        }
    }

    /// Calculate the memory usage of a row in bytes.
    fn row_memory_usage(row: &[TerminalCell]) -> usize {
        row.len() * std::mem::size_of::<TerminalCell>()
    }

    /// Clear a row to default cells.
    fn clear_row(&mut self, row: usize) {
        if row < self.rows {
            for c in 0..self.cols {
                self.cells[row][c] = TerminalCell::default();
            }
        }
    }

    /// Clear a range of columns in a row.
    fn clear_row_range(&mut self, row: usize, col_start: usize, col_end: usize) {
        if row < self.rows {
            let end = col_end.min(self.cols);
            for c in col_start..end {
                self.cells[row][c] = TerminalCell::default();
            }
        }
    }

    /// Remove a row at one index and insert a blank row at another.
    fn remove_and_insert_row(&mut self, remove_idx: usize, insert_idx: usize) {
        if remove_idx < self.cells.len() {
            self.cells.remove(remove_idx);
            self.line_wrapped.remove(remove_idx);
        }
        if insert_idx <= self.cells.len() {
            self.cells.insert(insert_idx, vec![TerminalCell::default(); self.cols]);
            self.line_wrapped.insert(insert_idx, false);
        }
    }

    /// Scroll up within a region: remove top line, add blank at bottom.
    /// When scrolling from absolute top (top == 0), save to scrollback.
    pub fn scroll_up(&mut self, top: usize, bottom: usize) {
        if top < bottom && bottom < self.rows {
            // Save to scrollback only when scrolling from absolute top and not in alt screen
            if top == 0 && self.alt_screen.is_none() {
                let removed = self.cells.remove(top);
                let wrapped = self.line_wrapped.remove(top);
                let removed_bytes = Self::row_memory_usage(&removed);

                self.scrollback.push_back(removed);
                self.scrollback_wrapped.push_back(wrapped);
                self.current_scrollback_bytes += removed_bytes;

                // Remove oldest entries if we exceed the memory limit
                while self.current_scrollback_bytes > self.max_scrollback_bytes {
                    if let Some(oldest) = self.scrollback.pop_front() {
                        let oldest_bytes = Self::row_memory_usage(&oldest);
                        self.current_scrollback_bytes -= oldest_bytes;
                    }
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

    /// Number of lines in scrollback history.
    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

    /// Get a scrollback row by index (0 = oldest).
    pub fn get_scrollback_row(&self, idx: usize) -> Option<&Vec<TerminalCell>> {
        self.scrollback.get(idx)
    }

    /// Scroll down within a region: remove bottom line, add blank at top.
    pub fn scroll_down(&mut self, top: usize, bottom: usize) {
        if top < bottom && bottom < self.rows {
            self.remove_and_insert_row(bottom, top);
        }
    }

    /// Insert n blank lines at cursor row.
    pub fn insert_lines(&mut self, n: usize) {
        let top = self.cursor_row;
        let bottom = self.scroll_bottom;
        for _ in 0..n {
            if top <= bottom && bottom < self.rows {
                self.remove_and_insert_row(bottom, top);
            }
        }
    }

    /// Delete n lines at cursor row.
    pub fn delete_lines(&mut self, n: usize) {
        let top = self.cursor_row;
        let bottom = self.scroll_bottom;
        for _ in 0..n {
            if top <= bottom && bottom < self.rows {
                self.remove_and_insert_row(top, bottom);
            }
        }
    }

    /// Insert n blank characters at cursor position.
    pub fn insert_chars(&mut self, n: usize) {
        let row = self.cursor_row;
        let col = self.cursor_col;
        if row < self.rows {
            let shift = n.min(self.cols - col);
            for _ in 0..shift {
                self.cells[row].insert(col, TerminalCell::default());
                self.cells[row].pop();
            }
        }
    }

    /// Delete n characters at cursor position.
    pub fn delete_chars(&mut self, n: usize) {
        let row = self.cursor_row;
        let col = self.cursor_col;
        if row < self.rows {
            let remaining = self.cols.saturating_sub(col);
            let to_delete = n.min(remaining);
            for _ in 0..to_delete {
                if col < self.cells[row].len() {
                    self.cells[row].remove(col);
                    self.cells[row].push(TerminalCell::default());
                }
            }
        }
    }

    /// Enter alternate screen mode (save current screen and switch to alt buffer).
    pub fn enter_alt_screen(&mut self) {
        if self.alt_screen.is_none() {
            let saved_cells = std::mem::replace(&mut self.cells, vec![vec![TerminalCell::default(); self.cols]; self.rows]);
            let saved_wrapped = std::mem::replace(&mut self.line_wrapped, vec![false; self.rows]);
            self.alt_screen = Some((saved_cells, saved_wrapped, self.cursor_col, self.cursor_row));
            self.clear();
        }
    }

    /// Exit alternate screen mode (restore saved screen).
    pub fn exit_alt_screen(&mut self) {
        if let Some((cells, wrapped, col, row)) = self.alt_screen.take() {
            self.cells = cells;
            self.line_wrapped = wrapped;
            self.cursor_col = col;
            self.cursor_row = row;
        }
    }

    /// Search terminal content (scrollback + grid) for all occurrences of a query.
    ///
    /// Returns matches sorted by row then column. Each match contains a global row
    /// index (scrollback rows 0..scrollback_len, grid rows scrollback_len..scrollback_len+rows)
    /// and column range (col_start inclusive, col_end exclusive).
    pub fn search(&self, query: &str, case_sensitive: bool) -> Vec<(usize, usize, usize)> {
        if query.is_empty() {
            return Vec::new();
        }

        let query_lower: String;
        let search_query = if case_sensitive {
            query
        } else {
            query_lower = query.to_lowercase();
            &query_lower
        };

        let sb_len = self.scrollback.len();
        let total_rows = sb_len + self.rows;
        let mut matches = Vec::new();

        for global_row in 0..total_rows {
            let cells = if global_row < sb_len {
                match self.scrollback.get(global_row) {
                    Some(row) => row,
                    None => continue,
                }
            } else {
                let grid_row = global_row - sb_len;
                if grid_row >= self.rows {
                    continue;
                }
                &self.cells[grid_row]
            };

            // Build searchable string, tracking mapping from string index to cell column
            let mut text = String::new();
            let mut col_map: Vec<usize> = Vec::new();
            for (col, cell) in cells.iter().enumerate() {
                if !cell.wide_continuation {
                    col_map.push(col);
                    text.push(cell.c);
                }
            }

            let search_text: String;
            let haystack = if case_sensitive {
                &text
            } else {
                search_text = text.to_lowercase();
                &search_text
            };

            // Find all occurrences
            let mut start = 0;
            while let Some(pos) = haystack[start..].find(search_query) {
                let char_start = haystack[..start + pos].chars().count();
                let char_end = char_start + search_query.chars().count();

                if char_start < col_map.len() && char_end > 0 {
                    let col_start = col_map[char_start];
                    let col_end = if char_end < col_map.len() {
                        col_map[char_end]
                    } else if let Some(&last) = col_map.last() {
                        (last + 1).min(cells.len())
                    } else {
                        cells.len()
                    };
                    matches.push((global_row, col_start, col_end));
                }

                start += pos + search_query.len().max(1);
                if start >= haystack.len() {
                    break;
                }
            }
        }

        matches
    }

    /// Resize with reflow logic (for column changes when not in alt screen).
    /// Re-wraps all content at the new column boundary to preserve text.
    fn resize_reflow(&mut self, new_cols: usize, new_rows: usize) {
        let old_cols = self.cols;
        if old_cols == new_cols {
            self.resize_screen(new_cols, new_rows);
            return;
        }

        // Step 1: Collect logical lines separately from scrollback and grid
        let mut scrollback_logical_lines: Vec<Vec<TerminalCell>> = Vec::new();
        let mut grid_logical_lines: Vec<Vec<TerminalCell>> = Vec::new();
        let mut current_line: Vec<TerminalCell> = Vec::new();

        // Process scrollback first
        for row_idx in 0..self.scrollback.len() {
            let row = &self.scrollback[row_idx];
            let was_wrapped = self.scrollback_wrapped.get(row_idx).copied().unwrap_or(false);
            current_line.extend(row.iter().cloned());
            if !was_wrapped {
                scrollback_logical_lines.push(std::mem::take(&mut current_line));
            }
        }
        // Add any remaining line from scrollback
        if !current_line.is_empty() {
            scrollback_logical_lines.push(std::mem::take(&mut current_line));
        }

        // Process visible grid
        for row_idx in 0..self.rows {
            let row = &self.cells[row_idx];
            let was_wrapped = self.line_wrapped.get(row_idx).copied().unwrap_or(false);
            current_line.extend(row.iter().cloned());

            // If this row was not wrapped OR it's the last row, end the logical line
            if !was_wrapped || row_idx == self.rows - 1 {
                // Don't add if it's empty and we already have content
                if !current_line.is_empty() {
                    grid_logical_lines.push(std::mem::take(&mut current_line));
                }
            }
        }

        // Step 2: Re-wrap scrollback logical lines at new column width
        let reflow_line = |logical_lines: &[Vec<TerminalCell>], new_cols: usize| -> VecDeque<Vec<TerminalCell>> {
            let mut result = VecDeque::new();
            for logical_line in logical_lines {
                // Find actual content length (skip trailing spaces/nuls)
                let line_len = logical_line.iter().rposition(|c| c.c != ' ' && c.c != '\0')
                    .map(|p| p + 1).unwrap_or(0);

                if line_len == 0 {
                    continue;
                }

                // Split logical line into chunks of new_cols
                let chunks = logical_line[..line_len].chunks(new_cols).collect::<Vec<_>>();
                for chunk in chunks {
                    let mut row = vec![TerminalCell::default(); new_cols];
                    for (i, cell) in chunk.iter().enumerate() {
                        if i < new_cols {
                            row[i] = cell.clone();
                        }
                    }
                    result.push_back(row);
                }
            }
            result
        };

        // Re-wrap scrollback content
        let mut new_scrollback = reflow_line(&scrollback_logical_lines, new_cols);
        // Calculate wrapped flags for scrollback
        let scrollback_row_counts: Vec<usize> = scrollback_logical_lines.iter()
            .map(|line| {
                let line_len = line.iter().rposition(|c| c.c != ' ' && c.c != '\0').map(|p| p + 1).unwrap_or(0);
                if line_len == 0 { 0 } else { (line_len + new_cols - 1) / new_cols }
            })
            .collect();

        // Re-wrap grid content
        let mut new_grid_content = reflow_line(&grid_logical_lines, new_cols);
        // Calculate wrapped flags for grid
        let grid_row_counts: Vec<usize> = grid_logical_lines.iter()
            .map(|line| {
                let line_len = line.iter().rposition(|c| c.c != ' ' && c.c != '\0').map(|p| p + 1).unwrap_or(0);
                if line_len == 0 { 0 } else { (line_len + new_cols - 1) / new_cols }
            })
            .collect();

        // Helper to calculate wrapped flags from row counts
        let calc_wrapped_flags = |row_counts: &[usize]| -> VecDeque<bool> {
            let mut flags = VecDeque::new();
            for &count in row_counts {
                for i in 0..count {
                    flags.push_back(i < count.saturating_sub(1));
                }
            }
            flags
        };

        // Step 3: Distribute rows between scrollback and visible grid
        let scrollback_rows = new_scrollback.len();
        let grid_rows = new_grid_content.len();
        let total_rows = scrollback_rows + grid_rows;

        if total_rows == 0 {
            // Empty terminal - create all empty rows
            self.cells = vec![vec![TerminalCell::default(); new_cols]; new_rows];
            self.line_wrapped = vec![false; new_rows];
            self.scrollback.clear();
            self.scrollback_wrapped.clear();
        } else if total_rows <= new_rows {
            // All content (scrollback + grid) fits in visible grid
            // Keep scrollback separate to preserve history structure
            // Even if scrollback is small, users expect to scroll up to see it
            self.scrollback = new_scrollback;
            self.scrollback_wrapped = calc_wrapped_flags(&scrollback_row_counts).into_iter().collect();

            self.cells = new_grid_content.into_iter().collect();
            self.line_wrapped = calc_wrapped_flags(&grid_row_counts).into_iter().collect();

            // Fill remaining rows
            while self.cells.len() < new_rows {
                self.cells.push(vec![TerminalCell::default(); new_cols]);
                self.line_wrapped.push(false);
            }
        } else {
            // total_rows > new_rows: need to keep scrollback
            // Most recent new_rows content goes to grid, rest to scrollback
            let rows_for_grid = new_rows;

            // Pop rows for the visible grid (from the back, so we get the most recent content)
            self.cells = Vec::new();
            self.line_wrapped = Vec::new();
            for _ in 0..rows_for_grid {
                if let Some(row) = new_grid_content.pop_back() {
                    self.cells.push(row);
                }
            }
            // Reverse since we popped from the back
            self.cells.reverse();
            self.line_wrapped.reverse();

            // Fill remaining rows with empty cells if needed
            while self.cells.len() < new_rows {
                self.cells.push(vec![TerminalCell::default(); new_cols]);
                self.line_wrapped.push(false);
            }

            // Remaining grid content + scrollback goes to scrollback
            while let Some(row) = new_grid_content.pop_front() {
                new_scrollback.push_back(row);
            }
            self.scrollback = new_scrollback;

            // Set wrapped flags for scrollback (scrollback content + overflow from grid)
            let mut scrollback_wrapped = calc_wrapped_flags(&scrollback_row_counts);
            // Add wrapped flags for overflow grid content (first rows that didn't fit)
            let mut grid_row_idx = 0;
            for &count in &grid_row_counts {
                for i in 0..count {
                    if grid_row_idx < grid_rows - rows_for_grid {
                        scrollback_wrapped.push_back(i < count.saturating_sub(1));
                    }
                    grid_row_idx += 1;
                }
            }
            self.scrollback_wrapped = scrollback_wrapped;
        }

        // Update grid dimensions
        self.cols = new_cols;
        self.rows = new_rows;
        self.scroll_bottom = new_rows.saturating_sub(1);

        // Position cursor at the end of actual content (not at old clamped position)
        // Find the last non-empty row to place the cursor correctly
        let mut last_content_row = 0;
        for row_idx in (0..self.cells.len()).rev() {
            let has_content = self.cells[row_idx].iter().any(|c| c.c != ' ' && c.c != '\0');
            if has_content {
                last_content_row = row_idx;
                break;
            }
        }

        // Find the last non-empty column in that row
        let mut last_content_col = 0;
        for col_idx in (0..self.cols).rev() {
            if self.cells[last_content_row][col_idx].c != ' ' && self.cells[last_content_row][col_idx].c != '\0' {
                last_content_col = col_idx;
                break;
            }
        }

        // Set cursor at the end of content
        self.cursor_row = last_content_row;
        self.cursor_col = if last_content_col < self.cols.saturating_sub(1) {
            last_content_col + 1
        } else {
            self.cols.saturating_sub(1)
        };

        // Recalculate scrollback memory usage
        self.current_scrollback_bytes = self.scrollback.iter()
            .map(|row| Self::row_memory_usage(row))
            .sum();

        // Trim scrollback if needed (after reflow we might exceed limit)
        while self.current_scrollback_bytes > self.max_scrollback_bytes {
            if let Some(oldest) = self.scrollback.pop_front() {
                let oldest_bytes = Self::row_memory_usage(&oldest);
                self.current_scrollback_bytes -= oldest_bytes;
            }
            self.scrollback_wrapped.pop_front();
        }

        // Set protection count - this will protect against the next erase_below call
        // (shell's response to PTY resize), but not against subsequent erases
        self.resize_protection_count = 1;
    }
}
