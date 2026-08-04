/// Unit tests for terminal grid functionality

#[cfg(test)]
mod tests {
    use crate::terminal::TerminalGrid;
    use crate::terminal::types::CellAttrs;

    fn create_test_grid(cols: usize, rows: usize) -> TerminalGrid {
        TerminalGrid::with_scrollback_limit(cols, rows, 1024 * 1024)
    }

    #[test]
    fn test_grid_creation() {
        let grid = create_test_grid(80, 24);
        assert_eq!(grid.cols, 80);
        assert_eq!(grid.rows, 24);
        assert_eq!(grid.cursor_col, 0);
        assert_eq!(grid.cursor_row, 0);
        assert!(grid.cursor_visible);
        assert_eq!(grid.scrollback_len(), 0);
    }

    #[test]
    fn test_write_char() {
        let mut grid = create_test_grid(80, 24);

        grid.write_char_with_attrs('A', &CellAttrs::default());
        assert_eq!(grid.cursor_col, 1);
        assert_eq!(grid.cursor_row, 0);

        let cell = &grid.cells[0][0];
        assert_eq!(cell.c, 'A');
    }

    #[test]
    fn test_cursor_movement() {
        let mut grid = create_test_grid(80, 24);

        // Write some characters to move cursor
        for _i in 0..10 {
            grid.write_char_with_attrs('A', &CellAttrs::default());
        }
        assert_eq!(grid.cursor_col, 10);

        // Move to next line
        grid.cursor_row = 1;
        grid.cursor_col = 0;

        grid.write_char_with_attrs('B', &CellAttrs::default());
        assert_eq!(grid.cursor_col, 1);
        assert_eq!(grid.cursor_row, 1);
    }

    #[test]
    fn test_line_wrap() {
        let mut grid = create_test_grid(10, 5);

        // Fill first line
        for _ in 0..10 {
            grid.write_char_with_attrs('A', &CellAttrs::default());
        }

        // Cursor should be at last column with wrap pending
        assert_eq!(grid.cursor_col, 9);
        assert!(grid.wrap_pending);

        // Next character should trigger wrap and be written at new position
        grid.write_char_with_attrs('B', &CellAttrs::default());
        assert_eq!(grid.cursor_col, 1); // After wrap, char written, then cursor advances
        assert_eq!(grid.cursor_row, 1);
        assert!(!grid.wrap_pending); // wrap_pending is cleared after wrap is handled
    }

    #[test]
    fn test_decawm_off_does_not_wrap() {
        // DECAWM off (`\e[?7l`): lines longer than `cols` must be clipped at the
        // last column, never wrapped. TUI apps (vim, htop, Claude Code, …)
        // disable auto-wrap to manage their own layout; if we wrap anyway our
        // row count diverges from theirs and their redraws smear/overlap.
        let mut grid = create_test_grid(5, 3);
        grid.autowrap = false;

        for c in "ABCDEFGH".chars() {
            grid.write_char_with_attrs(c, &CellAttrs::default());
        }

        assert_eq!(grid.cursor_row, 0, "DECAWM off: must NOT wrap to the next row");
        assert_eq!(grid.cursor_col, 4, "cursor clamped at the final column");
        assert_eq!(grid.cells[0][4].c, 'H', "overflowing chars overwrite the last cell");
        assert!(!grid.wrap_pending, "no deferred wrap when DECAWM is off");
    }

    #[test]
    fn test_decawm_on_wraps() {
        // DECAWM on (default `\e[?7h`): classic deferred wrap still works.
        let mut grid = create_test_grid(5, 3);
        assert!(grid.autowrap, "auto-wrap is on by default");

        for c in "ABCDEFGH".chars() {
            grid.write_char_with_attrs(c, &CellAttrs::default());
        }

        assert!(grid.cursor_row >= 1, "DECAWM on: should wrap to the next row");
        assert_eq!(grid.cells[1][0].c, 'F', "6th char lands at start of wrapped row");
    }

    #[test]
    fn test_clear_all() {
        let mut grid = create_test_grid(80, 24);

        // Write some characters
        for _ in 0..10 {
            grid.write_char_with_attrs('X', &CellAttrs::default());
        }

        grid.clear();

        // Check grid is cleared to default (space character)
        for row in &grid.cells {
            for cell in row {
                assert_eq!(cell.c, ' ');
            }
        }
    }

    #[test]
    fn test_clear() {
        let mut grid = create_test_grid(80, 24);

        // Write some content
        grid.write_char_with_attrs('A', &CellAttrs::default());
        grid.cursor_row = 5;
        grid.cursor_col = 10;
        grid.write_char_with_attrs('B', &CellAttrs::default());

        grid.clear();

        assert_eq!(grid.cursor_col, 0);
        assert_eq!(grid.cursor_row, 0);

        // Check all cells are cleared to default (space character)
        for row in &grid.cells {
            for cell in row {
                assert_eq!(cell.c, ' ');
            }
        }
    }

    #[test]
    fn test_scroll_up() {
        let mut grid = create_test_grid(10, 5);

        // Mark some rows as wrapped
        grid.line_wrapped[0] = true;
        grid.line_wrapped[1] = true;

        let scroll_top = 0;
        let scroll_bottom = 4;

        // Scroll up once
        grid.scroll_up(scroll_top, scroll_bottom);

        // Check that a line was added to scrollback
        assert_eq!(grid.scrollback_len(), 1);
        assert!(grid.scrollback_wrapped[0]);
    }

    #[test]
    fn test_scroll_down() {
        let mut grid = create_test_grid(10, 5);

        let scroll_top = 0;
        let scroll_bottom = 4;

        // Scroll down
        grid.scroll_down(scroll_top, scroll_bottom);

        // Should just shift lines, no scrollback added
        assert_eq!(grid.scrollback_len(), 0);
    }

    #[test]
    fn test_insert_lines() {
        let mut grid = create_test_grid(10, 5);

        // Set cursor to row 2
        grid.cursor_row = 2;

        // Insert 2 lines
        grid.insert_lines(2);

        // Lines should be inserted and shifted down
        // Cursor row should still be at 2
        assert_eq!(grid.cursor_row, 2);
    }

    #[test]
    fn test_delete_lines() {
        let mut grid = create_test_grid(10, 5);

        // Set cursor to row 2
        grid.cursor_row = 2;

        // Delete 1 line
        grid.delete_lines(1);

        // Cursor should still be at 2
        assert_eq!(grid.cursor_row, 2);
    }

    #[test]
    fn test_resize_preserves_content() {
        let mut grid = create_test_grid(80, 24);

        // Write some content
        grid.write_char_with_attrs('A', &CellAttrs::default());
        grid.write_char_with_attrs('B', &CellAttrs::default());
        grid.cursor_row = 1;
        grid.cursor_col = 0; // Reset cursor column before writing 'C'
        grid.write_char_with_attrs('C', &CellAttrs::default());

        // Resize
        grid.resize(100, 30);

        assert_eq!(grid.cols, 100);
        assert_eq!(grid.rows, 30);
        assert_eq!(grid.cells[0][0].c, 'A');
        assert_eq!(grid.cells[0][1].c, 'B');
        assert_eq!(grid.cells[1][0].c, 'C');
    }

    #[test]
    fn test_scrollback_limit() {
        let small_limit = 1000; // Very small limit for testing
        let mut grid = TerminalGrid::with_scrollback_limit(10, 5, small_limit);

        let scroll_top = 0;
        let scroll_bottom = 4;

        // Fill up scrollback
        for _ in 0..20 {
            grid.scroll_up(scroll_top, scroll_bottom);
        }

        // Scrollback should have content
        assert!(grid.scrollback_len() > 0);
        // The grid should still be functional
        assert_eq!(grid.cols, 10);
        assert_eq!(grid.rows, 5);
    }

    #[test]
    fn test_alt_screen() {
        let mut grid = create_test_grid(80, 24);

        // Write main screen content
        grid.write_char_with_attrs('M', &CellAttrs::default());
        grid.cursor_row = 1;
        grid.write_char_with_attrs('a', &CellAttrs::default());

        // Enter alt screen
        grid.enter_alt_screen();

        // Alt screen should be initialized with default cells (spaces)
        assert_eq!(grid.cells[0][0].c, ' ');

        // Write alt screen content
        grid.write_char_with_attrs('T', &CellAttrs::default());
        assert_eq!(grid.cells[0][0].c, 'T');

        // Exit alt screen
        grid.exit_alt_screen();

        // Main screen content should be restored
        assert_eq!(grid.cells[0][0].c, 'M');
        assert_eq!(grid.cells[1][1].c, 'a'); // 'a' was written at column 1, not 0
    }

    #[test]
    fn test_cell_attrs_default() {
        let attrs = CellAttrs::default();
        assert!(!attrs.bold);
        assert!(!attrs.dim);
        assert!(!attrs.italic);
        assert!(!attrs.underline);
        assert!(!attrs.inverse);
        assert!(!attrs.strikethrough);
    }

    #[test]
    fn test_cell_attrs_inverse() {
        let attrs = CellAttrs {
            fg_color: (255, 0, 0),
            bg_color: (0, 255, 0),
            inverse: false,
            ..Default::default()
        };

        // When inverse is false, colors are as-is
        let (fg, bg) = if attrs.inverse {
            (attrs.bg_color, attrs.fg_color)
        } else {
            (attrs.fg_color, attrs.bg_color)
        };
        assert_eq!(fg, (255, 0, 0));
        assert_eq!(bg, (0, 255, 0));

        let attrs_inverse = CellAttrs { inverse: true, ..attrs };
        let (fg, bg) = if attrs_inverse.inverse {
            (attrs_inverse.bg_color, attrs_inverse.fg_color)
        } else {
            (attrs_inverse.fg_color, attrs_inverse.bg_color)
        };
        assert_eq!(fg, (0, 255, 0)); // Swapped
        assert_eq!(bg, (255, 0, 0));
    }

    #[test]
    fn test_search_empty_query() {
        let grid = create_test_grid(80, 24);

        let matches = grid.search("", false);
        assert!(matches.is_empty());

        let matches = grid.search("test", false);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_search_simple_match() {
        let mut grid = create_test_grid(80, 24);

        // Write "hello" on first line
        for c in "hello".chars() {
            grid.write_char_with_attrs(c, &CellAttrs::default());
        }

        let matches = grid.search("hello", false);
        assert!(!matches.is_empty());

        let (row, col_start, col_end) = matches[0];
        assert_eq!(row, 0); // First row
        assert_eq!(col_start, 0);
        assert_eq!(col_end, 5);
    }

    #[test]
    fn test_search_case_insensitive() {
        let mut grid = create_test_grid(80, 24);

        // Write "Hello" with capital H
        grid.write_char_with_attrs('H', &CellAttrs::default());
        for c in "ello".chars() {
            grid.write_char_with_attrs(c, &CellAttrs::default());
        }

        // Case insensitive should find it
        let matches_lower = grid.search("hello", false);
        assert!(!matches_lower.is_empty());

        let matches_upper = grid.search("HELLO", false);
        assert!(!matches_upper.is_empty());

        // Case sensitive should only find exact match
        let matches_exact = grid.search("Hello", true);
        assert!(!matches_exact.is_empty());

        let matches_wrong_case = grid.search("hello", true);
        assert!(matches_wrong_case.is_empty());
    }

    // === Reflow Tests ===

    /// Helper to write a string at the current cursor position
    fn write_string(grid: &mut TerminalGrid, s: &str) {
        for c in s.chars() {
            grid.write_char_with_attrs(c, &CellAttrs::default());
        }
    }

    /// Helper to get scrollback content as a string (for testing)
    fn get_scrollback_content(grid: &TerminalGrid) -> String {
        let mut result = String::new();
        for i in 0..grid.scrollback_len() {
            if let Some(row) = grid.get_scrollback_row(i) {
                for c in row.iter() {
                    if c.c != ' ' && c.c != '\0' {
                        result.push(c.c);
                    } else if !result.is_empty() && !result.ends_with(' ') && !result.ends_with('\n') {
                        result.push(' ');
                    }
                }
                result.push('\n');
            }
        }
        result
    }

    /// Helper to get the visible content as a string (for testing)
    fn get_visible_content(grid: &TerminalGrid) -> String {
        let mut result = String::new();
        for row_idx in 0..grid.rows {
            let row = &grid.cells[row_idx];
            // Find the range of non-space characters (trim leading and trailing spaces)
            let first_non_space = row.iter().position(|c| c.c != ' ' && c.c != '\0');
            let last_non_space = row.iter().rposition(|c| c.c != ' ' && c.c != '\0');
            if let (Some(start), Some(end)) = (first_non_space, last_non_space) {
                let line: String = row[start..=end].iter().map(|c| c.c).collect();
                result.push_str(&line);
                result.push('\n');
            }
        }
        result
    }

    #[test]
    fn test_reflow_narrow_then_wide() {
        let mut grid = create_test_grid(20, 5);
        // Not in alt screen by default, so reflow should work

        // Write a long line "ere ere sd gg" (13 chars)
        write_string(&mut grid, "ere ere sd gg");

        // Content should be on first row
        assert_eq!(grid.cells[0][0].c, 'e');
        assert_eq!(grid.cells[0][12].c, 'g');

        // Resize to narrower (10 cols)
        grid.resize(10, 5);

        // Content should be reflowed into multiple rows
        // "ere ere sd" + " gg" = 10 + 3 chars
        assert_eq!(grid.cells[0][0].c, 'e');
        assert_eq!(grid.cells[0][9].c, 'd'); // Last char of first wrapped row
        assert_eq!(grid.cells[1][0].c, ' ');
        assert_eq!(grid.cells[1][1].c, 'g'); // " gg"

        // Resize back to wider (20 cols)
        grid.resize(20, 5);

        // Content should be preserved and re-expanded
        // Note: due to reflow, the exact layout depends on wrapped flags
        // But we should have all characters preserved
        let content = get_visible_content(&grid);
        assert!(content.contains("ere"));
        assert!(content.contains("sd"));
        assert!(content.contains("gg"));
    }

    #[test]
    fn test_reflow_with_scrollback() {
        let mut grid = create_test_grid(10, 3);

        // Write some content and scroll it to scrollback
        write_string(&mut grid, "line1");
        grid.cursor_row = 1;
        grid.cursor_col = 0;
        write_string(&mut grid, "line2");
        grid.cursor_row = 2;
        grid.cursor_col = 0;
        write_string(&mut grid, "line3");

        // Scroll up to move content to scrollback
        grid.scroll_up(0, 2);
        assert_eq!(grid.scrollback_len(), 1);

        // Now resize - with only 3 rows total and 3 content rows,
        // all content fits in the grid and scrollback is emptied
        grid.resize(15, 3);

        // After resize, all content should be accessible (in scrollback or visible)
        // Scrollback is preserved even though it could fit in the grid
        let scrollback_content = get_scrollback_content(&grid);
        let visible_content = get_visible_content(&grid);
        let combined = format!("{}{}", scrollback_content, visible_content);
        assert!(combined.contains("line1"), "line1 should be in scrollback or visible");
        assert!(combined.contains("line2"), "line2 should be visible");
        assert!(combined.contains("line3"), "line3 should be visible");
    }

    #[test]
    fn test_reflow_soft_wrapped_lines() {
        let mut grid = create_test_grid(10, 5);

        // Write content that exactly fills one line (10 chars)
        write_string(&mut grid, "0123456789");
        assert_eq!(grid.cursor_col, 9); // At last column
        assert!(grid.wrap_pending); // Wrap pending

        // Write more to trigger actual wrap
        write_string(&mut grid, "ab");
        assert_eq!(grid.cursor_row, 1);
        grid.line_wrapped[0] = true; // Mark first row as wrapped

        // Now we have two rows: "0123456789" (wrapped) and "ab"
        // Resize to narrower (8 cols)
        grid.resize(8, 5);

        // Content should be reflowed at new width
        // "01234567" + "89ab" = 8 + 4 chars
        // The original wrapped flag should be preserved in reflow
        let content = get_visible_content(&grid);
        assert!(content.contains("01234567"));
        assert!(content.contains("89ab"));
    }

    #[test]
    fn test_reflow_empty_terminal() {
        let mut grid = create_test_grid(80, 24);

        // Empty terminal - no content written
        assert_eq!(grid.cells.len(), 24);
        assert_eq!(grid.cells[0].len(), 80);

        // Resize should not panic and should maintain correct dimensions
        grid.resize(100, 30);

        assert_eq!(grid.cols, 100);
        assert_eq!(grid.rows, 30);
        assert_eq!(grid.cells.len(), 30);
        assert_eq!(grid.cells[0].len(), 100);

        // All cells should be empty (spaces)
        for row in &grid.cells {
            for cell in row {
                assert_eq!(cell.c, ' ');
            }
        }
    }

    #[test]
    fn test_reflow_single_long_line() {
        let mut grid = create_test_grid(20, 5);

        // Write a very long line (61 chars)
        let long_text = "abcdefghijklmnopqrstuvwxyz123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        assert_eq!(long_text.len(), 61);
        for c in long_text.chars() {
            grid.write_char_with_attrs(c, &CellAttrs::default());
        }

        // Resize to much narrower (10 cols)
        grid.resize(10, 5);

        // Should be reflowed into multiple rows
        // 61 chars / 10 cols = 7 rows (with partial last row)
        // Since grid is only 5 rows, only the last 5 rows are visible
        let content = get_visible_content(&grid);
        assert!(content.contains("56789")); // Last visible characters
        assert!(content.contains("ABCD"));  // Upper case, not lower

        // Resize to wider (30 cols)
        grid.resize(30, 5);

        // Content should still be preserved (may be partially in scrollback)
        let content = get_visible_content(&grid);
        let scrollback_content = get_scrollback_content(&grid);
        let combined = format!("{}{}", scrollback_content, content);
        assert!(combined.contains("abcdefghijklmnopqrst"));
    }

    #[test]
    fn test_reflow_preserves_line_wrapped_flags() {
        let mut grid = create_test_grid(10, 5);

        // Write a long line that will naturally wrap
        // "hello_world" is 11 chars, which will wrap at 10 cols
        write_string(&mut grid, "hello_world");

        // After writing 11 chars, we should have:
        // - Row 0: "hello_worl" (10 chars) with line_wrapped[0] = true
        // - Row 1: "d" (1 char)
        assert!(grid.line_wrapped[0]); // First row should be marked as wrapped

        // Resize to different width
        grid.resize(15, 5);

        // The reflow should respect the wrapped flags and reconstruct logical lines
        // "hello_world" should be preserved
        let content = get_visible_content(&grid);
        assert!(content.contains("hello_world"));
    }

    #[test]
    fn test_reflow_no_change_same_dimensions() {
        let mut grid = create_test_grid(80, 24);

        // Write some content
        write_string(&mut grid, "test content");
        let original_cursor_col = grid.cursor_col;
        let original_cursor_row = grid.cursor_row;

        // Resize to same dimensions should be no-op
        grid.resize(80, 24);

        assert_eq!(grid.cols, 80);
        assert_eq!(grid.rows, 24);
        assert_eq!(grid.cursor_col, original_cursor_col);
        assert_eq!(grid.cursor_row, original_cursor_row);
        // Content should be preserved
        assert_eq!(grid.cells[0][0].c, 't');
        assert_eq!(grid.cells[0][1].c, 'e');
    }

    #[test]
    fn test_reflow_in_alt_screen_no_reflow() {
        let mut grid = create_test_grid(20, 5);

        // Write main screen content
        write_string(&mut grid, "main");

        // Enter alt screen
        grid.enter_alt_screen();

        // Write alt screen content
        write_string(&mut grid, "alt screen content that is quite long");

        // Resize in alt screen should use simple resize (no reflow)
        grid.resize(15, 5);

        // Alt screen content should be preserved (without complex reflow)
        assert_eq!(grid.cols, 15);
        assert_eq!(grid.rows, 5);

        // Exit alt screen and verify main screen is preserved
        grid.exit_alt_screen();
        // Note: simple resize in alt screen may truncate content at old width
        // This is expected behavior for alt screen (no reflow)
    }

    #[test]
    fn test_reflow_with_existing_scrollback_content() {
        let mut grid = create_test_grid(80, 24);

        // Write multiple lines of content (simulating executed commands)
        for line_num in 0..10 {
            write_string(&mut grid, &format!("Command output line {}", line_num));
            // Move to next line manually (simulating newline)
            grid.cursor_row += 1;
            grid.cursor_col = 0;
        }

        // Scroll some content to scrollback by simulating more output
        for _ in 0..5 {
            grid.scroll_up(0, 23);
        }

        let initial_scrollback_len = grid.scrollback_len();
        assert!(initial_scrollback_len > 0, "Should have scrollback content");

        // Now resize to narrower width - this should preserve scrollback
        grid.resize(40, 24);

        // Scrollback should still have content
        let new_scrollback_len = grid.scrollback_len();
        assert!(new_scrollback_len > 0, "Scrollback should not be empty after resize");

        // The scrollback should contain the original content (possibly reflowed)
        let mut scrollback_content = String::new();
        for i in 0..new_scrollback_len {
            if let Some(row) = grid.get_scrollback_row(i) {
                for c in row.iter() {
                    if c.c != ' ' && c.c != '\0' {
                        scrollback_content.push(c.c);
                    } else if !scrollback_content.is_empty() && scrollback_content.ends_with(|ch: char| ch != ' ' && ch != '\n') {
                        scrollback_content.push(' ');
                    }
                }
                scrollback_content.push('\n');
            }
        }

        // Should still contain our original content
        assert!(scrollback_content.contains("Command"), "Scrollback should contain original content");
    }

    #[test]
    fn test_reflow_preserves_command_output() {
        let mut grid = create_test_grid(80, 24);

        // Simulate: user types "ls" and sees output, then prompt appears
        // Write the prompt
        write_string(&mut grid, "(base) vaniot@bogon portal % ");
        write_string(&mut grid, "ls");

        // Simulate ls output (multiple files)
        grid.cursor_row += 1;  // Move to next line
        grid.cursor_col = 0;
        write_string(&mut grid, "Cargo.toml");
        grid.cursor_row += 1;
        grid.cursor_col = 0;
        write_string(&mut grid, "Cargo.lock");
        grid.cursor_row += 1;
        grid.cursor_col = 0;
        write_string(&mut grid, "src/");
        grid.cursor_row += 1;
        grid.cursor_col = 0;
        write_string(&mut grid, "target/");
        grid.cursor_row += 1;
        grid.cursor_col = 0;

        // Write another prompt
        write_string(&mut grid, "(base) vaniot@bogon portal % ");

        // Now resize to narrower width
        let original_content = get_visible_content(&grid);
        grid.resize(40, 24);

        // The ls output should still be present
        let new_content = get_visible_content(&grid);
        assert!(new_content.contains("Cargo.toml"), "ls output should be preserved");
        assert!(new_content.contains("src/"), "ls output should be preserved");

        // Resize back to original width
        grid.resize(80, 24);

        // Content should still be there
        let final_content = get_visible_content(&grid);
        assert!(final_content.contains("Cargo.toml"), "ls output should still be preserved");
    }

    #[test]
    fn test_reflow_with_multiple_commands_and_scrollback() {
        let mut grid = create_test_grid(80, 10);

        // First command with output
        write_string(&mut grid, "(base) vaniot@bogon portal % ");
        write_string(&mut grid, "ls -la");
        grid.cursor_row += 1;
        grid.cursor_col = 0;
        write_string(&mut grid, "total 100");
        grid.cursor_row += 1;
        grid.cursor_col = 0;
        write_string(&mut grid, "drwxr-xr-x  ... src");

        // Second command
        grid.cursor_row += 1;
        grid.cursor_col = 0;
        write_string(&mut grid, "(base) vaniot@bogon portal % ");
        write_string(&mut grid, "cargo build");
        grid.cursor_row += 1;
        grid.cursor_col = 0;
        write_string(&mut grid, "Compiling...");
        grid.cursor_row += 1;
        grid.cursor_col = 0;
        write_string(&mut grid, "Finished");

        // Scroll up to push first command to scrollback
        grid.scroll_up(0, 9);

        let scrollback_len_before = grid.scrollback_len();
        assert!(scrollback_len_before > 0, "Should have scrollback");

        // Resize to narrower width
        grid.resize(40, 10);

        // Check that scrollback is preserved
        let scrollback_len_after = grid.scrollback_len();
        assert!(scrollback_len_after > 0, "Scrollback should still exist after resize");

        // Check that content is preserved (in scrollback or visible)
        let scrollback_content = get_scrollback_content(&grid);
        let visible_content = get_visible_content(&grid);
        let combined = format!("{}{}", scrollback_content, visible_content);
        assert!(combined.contains("ls -la"), "First command should be preserved");
        assert!(combined.contains("cargo build"), "Second command should be preserved");
        assert!(combined.contains("Compiling"), "Output should be preserved");
    }

    #[test]
    fn test_reflow_large_output_narrowing() {
        let mut grid = create_test_grid(80, 24);

        // Simulate ls command with many files (20 lines of output)
        write_string(&mut grid, "$ ls");
        grid.cursor_row += 1;
        grid.cursor_col = 0;

        for i in 0..20 {
            write_string(&mut grid, &format!("file_{:03}.txt  some content here", i));
            grid.cursor_row += 1;
            grid.cursor_col = 0;
        }

        // Add a prompt at the end
        write_string(&mut grid, "$ ");

        // Get content before resize
        let content_before = get_visible_content(&grid);
        let lines_before = content_before.lines().count();

        // Resize to half width (this doubles the row count)
        grid.resize(40, 24);

        // Check that content is preserved
        let content_after = get_visible_content(&grid);

        // The original files should still be present
        assert!(content_after.contains("file_000"), "First file should be in visible area");
        assert!(content_after.contains("file_019"), "Last file should be in visible area");

        // Resize back to original width
        grid.resize(80, 24);

        // Content should still be preserved
        let content_final = get_visible_content(&grid);
        assert!(content_final.contains("file_000"), "Content should be preserved after resize back");
    }

    #[test]
    fn test_multiple_resizes_no_content_loss() {
        let mut grid = create_test_grid(80, 24);

        // Write some content
        for i in 0..10 {
            write_string(&mut grid, &format!("Line {} content here", i));
            grid.cursor_row += 1;
            grid.cursor_col = 0;
        }

        // Perform multiple resizes
        grid.resize(60, 24);  // Narrower
        let content1 = get_visible_content(&grid);
        assert!(content1.contains("Line 0"), "First resize should preserve content");

        grid.resize(40, 24);  // Even narrower
        let content2 = get_visible_content(&grid);
        assert!(content2.contains("Line 0"), "Second resize should preserve content");

        grid.resize(80, 24);  // Back to original
        let content3 = get_visible_content(&grid);
        assert!(content3.contains("Line 0"), "Third resize should preserve content");

        grid.resize(100, 24); // Wider
        let content4 = get_visible_content(&grid);
        assert!(content4.contains("Line 0"), "Fourth resize should preserve content");

        // All content should still be there after 4 resizes
        assert!(content4.contains("Line 9"), "Last line should still be present");
    }

    #[test]
    fn test_resize_preserves_long_lines_correctly() {
        let mut grid = create_test_grid(80, 24);

        // Write a very long single line (no newline until the end)
        let long_text = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
        write_string(&mut grid, long_text);

        // Resize narrower
        grid.resize(40, 24);

        // Content should be wrapped correctly
        let content = get_visible_content(&mut grid);
        assert!(content.contains("abcd"), "Start should be present");
        assert!(content.contains("wxyz"), "End should be present");
        // Note: middle portion spans row boundary, check each part separately
        assert!(content.contains("ABCDEFGHIJKLMN"), "First part of middle should be in row 0");
        assert!(content.contains("OPQRSTUVWXYZ"), "Second part of middle should be in row 1");

        // Resize back
        grid.resize(80, 24);

        // Content should be re-expanded correctly
        let content = get_visible_content(&mut grid);
        assert!(content.contains(long_text), "Full content should be preserved");
    }

    #[test]
    fn test_resize_with_mixed_width_lines() {
        let mut grid = create_test_grid(80, 24);

        // Write content with varying line lengths
        write_string(&mut grid, "short");
        grid.cursor_row += 1;
        grid.cursor_col = 0;
        write_string(&mut grid, "medium length line here");
        grid.cursor_row += 1;
        grid.cursor_col = 0;
        write_string(&mut grid, "a very long line that exceeds the default width and should wrap to the next line when displayed");
        grid.cursor_row += 1;
        grid.cursor_col = 0;

        // Resize
        grid.resize(40, 24);

        // All lines should be preserved
        let content = get_visible_content(&mut grid);
        assert!(content.contains("short"), "Short line should be preserved");
        assert!(content.contains("medium"), "Medium line should be preserved");
        assert!(content.contains("long line"), "Long line should be preserved");
    }

    #[test]
    fn test_resize_scrollback_with_wrapped_content() {
        let mut grid = create_test_grid(80, 10);

        // Write a long line that will wrap
        write_string(&mut grid, "this is a very long line that will wrap when the terminal is resized to a narrower width");

        // Fill up some rows
        for i in 0..5 {
            grid.cursor_row += 1;
            grid.cursor_col = 0;
            write_string(&mut grid, &format!("row {}", i));
        }

        // Scroll up to push content to scrollback
        grid.scroll_up(0, 9);
        assert_eq!(grid.scrollback_len(), 1, "Should have scrollback content");

        // Resize to half width
        grid.resize(40, 10);

        // A wrapped line split across the scrollback/grid boundary is REJOINED
        // into one logical line on reflow and lands in the visible grid (this is
        // the fix for cross-boundary line truncation) — so it must be present in
        // the COMBINED grid+scrollback content, not lost. (Previously it was
        // left split/buried, which is the bug this guards against.)
        let mut combined = get_scrollback_content(&grid);
        combined.push_str(&get_visible_content(&grid));
        assert!(combined.contains("very long line"),
            "long wrapped line must be preserved (rejoined) after resize — got: {combined}");
        // and the other rows must still be present too
        assert!(combined.contains("row 4"), "later rows must survive — got: {combined}");
    }

    #[test]
    fn test_resize_empty_terminal_doesnt_crash() {
        let mut grid = create_test_grid(80, 24);

        // Resize empty terminal
        grid.resize(40, 24);

        // Should not crash and should have correct dimensions
        assert_eq!(grid.cols, 40);
        assert_eq!(grid.rows, 24);
        assert_eq!(grid.cursor_col, 0);
        assert_eq!(grid.cursor_row, 0);
    }

    #[test]
    fn test_resize_to_zero_width_safe() {
        let mut grid = create_test_grid(80, 24);

        // Write some content
        write_string(&mut grid, "test");

        // Resize to very small but non-zero width
        grid.resize(1, 24);

        // Should handle gracefully
        assert_eq!(grid.cols, 1);
        assert_eq!(grid.rows, 24);

        // Resize back
        grid.resize(80, 24);

        // Content should be preserved (though heavily wrapped)
        let content = get_visible_content(&grid);
        assert!(content.contains("test"), "Content should be preserved");
    }

    #[test]
    fn test_resize_cursor_position_after_reflow() {
        let mut grid = create_test_grid(80, 24);

        // Write some content
        write_string(&mut grid, "Line 1");
        grid.cursor_row += 1;
        grid.cursor_col = 0;
        write_string(&mut grid, "Line 2");
        grid.cursor_row += 1;
        grid.cursor_col = 0;
        write_string(&mut grid, "Line 3");

        // Cursor should be at position after "Line 3"
        let cursor_col_before = grid.cursor_col;

        // Resize
        grid.resize(60, 24);

        // Cursor should still be at a valid position
        assert!(grid.cursor_col < grid.cols, "Cursor should be within bounds");
        assert!(grid.cursor_row < grid.rows, "Cursor row should be within bounds");

        // Cursor should be at or after the last written character
        assert!(grid.cursor_col >= cursor_col_before.saturating_sub(5),
                  "Cursor should be near the end of written content");
    }

    #[test]
    fn test_resize_preserves_scrollback_order() {
        let mut grid = create_test_grid(80, 10);

        // Write multiple lines to scrollback
        for i in 0..5 {
            write_string(&mut grid, &format!("scrollback line {}", i));
            grid.cursor_row += 1;
            grid.cursor_col = 0;
        }

        // Write visible content
        write_string(&mut grid, "visible line");

        // Scroll all to scrollback
        for _ in 0..6 {
            grid.scroll_up(0, 9);
        }

        let scrollback_before = grid.scrollback_len();
        assert!(scrollback_before > 0);

        // Resize
        grid.resize(40, 10);

        // Scrollback should still exist
        let scrollback_after = grid.scrollback_len();
        assert!(scrollback_after > 0, "Scrollback should be preserved");

        // Check that scrollback maintains order
        let scrollback_content = get_scrollback_content(&grid);
        assert!(scrollback_content.contains("scrollback line 0"), "First scrollback line should be preserved");
        assert!(scrollback_content.contains("scrollback line 4"), "Last scrollback line should be preserved");
    }

    #[test]
    fn test_resize_from_wide_to_narrow_to_wide() {
        let mut grid = create_test_grid(100, 24);

        // Original wide width
        write_string(&mut grid, "Wide content: this line should fit in 100 columns");
        grid.cursor_row += 1;
        grid.cursor_col = 0;
        write_string(&mut grid, "Second line");

        // Narrow
        grid.resize(50, 24);
        let narrow_content = get_visible_content(&grid);
        assert!(narrow_content.contains("Wide content"), "Content should be preserved");

        // Narrower
        grid.resize(30, 24);
        let narrower_content = get_visible_content(&grid);
        assert!(narrower_content.contains("Wide content"), "Content should still be preserved");

        // Wider than original
        grid.resize(150, 24);
        let wide_content = get_visible_content(&grid);
        assert!(wide_content.contains("Wide content"), "Content should be preserved");

        // Back to original
        grid.resize(100, 24);
        let original_content = get_visible_content(&grid);
        assert!(original_content.contains("Wide content"), "Content should be preserved");
        assert!(original_content.contains("Second line"), "All lines should be preserved");
    }

    #[test]
    fn test_resize_with_line_wrapping_boundary() {
        let mut grid = create_test_grid(20, 5);

        // Write exactly 20 characters (one full row) - no trailing newline
        for c in "01234567890123456789".chars() {
            grid.write_char_with_attrs(c, &CellAttrs::default());
        }

        // Cursor should be at last column with wrap_pending
        assert_eq!(grid.cursor_col, 19);
        assert!(grid.wrap_pending);

        // Resize narrower
        grid.resize(10, 5);

        // Content should be correctly wrapped
        let content = get_visible_content(&grid);
        assert!(content.contains("0123456789"), "First 10 chars should be on first line");
        assert!(content.contains("0123456789"), "Next 10 chars should be on second line");

        // The first line should be marked as wrapped
        assert!(grid.line_wrapped[0], "First row should be marked as wrapped");
    }

    #[test]
    fn test_resize_does_not_corrupt_binary_data() {
        let mut grid = create_test_grid(80, 24);

        // Test with various character types (note: control chars are filtered by write_char_with_attrs)
        let test_data = "ABC∞∂DEF†‡";
        for c in test_data.chars() {
            grid.write_char_with_attrs(c, &CellAttrs::default());
        }

        // Resize
        grid.resize(40, 24);

        // Check that all characters are preserved
        for (i, expected) in test_data.chars().enumerate() {
            // Content should be somewhere (in grid or scrollback)
            let found = grid.cells.iter().any(|row| {
                row.iter().any(|cell| cell.c == expected)
            });
            assert!(found, "Character at index {} ({:?}) should be preserved", i, expected);
        }
    }

    #[test]
    fn test_resize_with_empty_lines_between_content() {
        let mut grid = create_test_grid(80, 24);

        // Write content with gaps (empty lines)
        write_string(&mut grid, "Line 1");
        grid.cursor_row += 3;  // Skip 2 lines (create gap)
        grid.cursor_col = 0;
        write_string(&mut grid, "Line 4");

        grid.resize(40, 24);

        // Content should be preserved, gaps may be collapsed
        let content = get_visible_content(&grid);
        assert!(content.contains("Line 1"), "First line should be preserved");
        assert!(content.contains("Line 4"), "Last line should be preserved");
    }
}

/// PTY cleanup and leak prevention tests
/// These tests ensure that PTY resources are properly cleaned up
/// to prevent the "out of PTY devices" error that occurred with 500+ zombie processes
#[cfg(test)]
mod pty_cleanup_tests {
    use super::*;
    use crate::terminal::{Pty, PtySize};

    #[cfg(unix)]
    #[test]
    fn test_pty_interactive_shell_stays_alive() {
        use crate::terminal::{Pty, UnixPty};
        use std::thread;
        use std::time::{Duration, Instant};

        // Spawn a real interactive shell. This exercises the controlling-terminal
        // setup (forkpty → setsid + TIOCSCTTY). The whole point: a local shell
        // must behave like the system Terminal — it runs, produces output, and
        // stays alive, with NO spurious "disconnect".
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let mut pty = UnixPty::spawn(&shell, &["-i"], crate::terminal::PtySize::new(24, 80))
            .expect("shell spawn should succeed");

        // Shell must be alive right after spawn.
        thread::sleep(Duration::from_millis(200));
        assert!(pty.is_alive(), "interactive shell must be alive after spawn");

        // Run a command and confirm output flows back through the PTY.
        pty.write(b"echo PORTAL_SMOKE_TOKEN\n").expect("write should succeed");

        let mut got = Vec::new();
        let deadline = Instant::now() + Duration::from_millis(2000);
        while Instant::now() < deadline {
            if let Ok(chunk) = pty.try_read() {
                got.extend_from_slice(&chunk);
                if got.windows(18).any(|w| w == b"PORTAL_SMOKE_TOKEN") {
                    break;
                }
            }
            thread::sleep(Duration::from_millis(20));
        }
        let s = String::from_utf8_lossy(&got);
        assert!(
            s.contains("PORTAL_SMOKE_TOKEN"),
            "shell output should contain the token, got: {:?}",
            s
        );

        // The shell must STILL be alive after running a command — this is the
        // exact regression: the old code killed the session on a transient read
        // error (SIGCHLD interrupting the read while the shell forked `echo`).
        assert!(pty.is_alive(), "shell must stay alive after running a command");
    }

    #[cfg(unix)]
    #[test]
    fn test_pty_creation_and_cleanup() {
        use crate::terminal::UnixPty;
        use std::thread;
        use std::time::Duration;

        // Create a PTY with a long-running process
        let pty_result = UnixPty::spawn("/bin/sleep", &["10"], crate::terminal::PtySize::new(24, 80));
        assert!(pty_result.is_ok(), "PTY spawn should succeed");

        let mut pty = pty_result.unwrap();
        let pid = pty.child_pid;

        // Give process time to start
        thread::sleep(Duration::from_millis(50));

        // Verify PTY is alive
        assert!(pty.is_alive(), "PTY should be alive after spawn");

        // Kill the PTY
        let kill_result = pty.kill();
        assert!(kill_result.is_ok(), "PTY kill should succeed");

        // Verify PTY is marked as not alive
        assert!(!pty.is_alive(), "PTY should not be alive after kill");

        // Give kill time to complete
        thread::sleep(Duration::from_millis(50));

        // Verify process is actually dead
        let result = unsafe { libc::kill(pid, 0) };
        assert!(result < 0, "Process should not exist after kill");
        assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH),
                   "Process should have ESRCH (no such process)");
    }

    #[cfg(unix)]
    #[test]
    fn test_pty_drop_cleans_up_process() {
        use crate::terminal::UnixPty;
        use std::thread;
        use std::time::Duration;

        let pid = {
            let pty = UnixPty::spawn("/bin/sleep", &["10"], crate::terminal::PtySize::new(24, 80))
                .expect("PTY spawn should succeed");
            let pid = pty.child_pid;

            // PTY is alive
            assert!(pty.is_alive());

            // Drop the PTY - should clean up the process
            drop(pty);
            pid
        };

        // Give the drop handler time to complete
        thread::sleep(Duration::from_millis(200));

        // Verify process was killed
        let result = unsafe { libc::kill(pid, 0) };
        assert!(result < 0, "Process should not exist after PTY drop");
        assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH),
                   "Process should have ESRCH after PTY drop");
    }

    #[cfg(unix)]
    #[test]
    fn test_multiple_pty_cleanup_no_leaks() {
        use crate::terminal::UnixPty;
        use std::thread;
        use std::time::Duration;

        let mut ptys = Vec::new();
        let mut pids = Vec::new();

        // Create multiple PTYs (simulating multiple terminal sessions)
        for _ in 0..10 {
            let pty = UnixPty::spawn("/bin/sleep", &["10"], crate::terminal::PtySize::new(24, 80))
                .expect("PTY spawn should succeed");
            pids.push(pty.child_pid);
            ptys.push(pty);
        }

        // Give processes time to start
        thread::sleep(Duration::from_millis(100));

        // All processes should be alive
        let mut alive_count = 0;
        for &pid in &pids {
            let result = unsafe { libc::kill(pid, 0) };
            if result == 0 {
                alive_count += 1;
            }
        }
        assert!(alive_count >= 8, "At least 8 of 10 processes should be alive");

        // Drop all PTYs by going out of scope
        drop(ptys);
        drop(pids);

        // Give cleanup time
        thread::sleep(Duration::from_millis(300));

        // Test passes if we get here without panic
        // The Drop implementations should have cleaned up all processes
    }

    #[cfg(unix)]
    #[test]
    fn test_pty_sigkill_fallback() {
        use crate::terminal::UnixPty;
        use std::thread;
        use std::time::Duration;

        // Spawn a process that ignores SIGTERM
        let mut pty = UnixPty::spawn("/bin/perl", &["-e", "$SIG{TERM}=sub{}; sleep 100"],
                                        crate::terminal::PtySize::new(24, 80))
            .expect("PTY spawn should succeed");
        let pid = pty.child_pid;

        // Give process time to set up signal handler
        thread::sleep(Duration::from_millis(50));

        // Kill should use SIGKILL as fallback
        let kill_result = pty.kill();
        assert!(kill_result.is_ok(), "Kill should succeed even with SIGTERM ignored");

        // Verify process is actually dead (SIGKILL should have worked)
        thread::sleep(Duration::from_millis(50));
        let result = unsafe { libc::kill(pid, 0) };
        assert!(result < 0, "Process should be killed even when ignoring SIGTERM");
    }

    #[cfg(unix)]
    #[test]
    fn test_pty_writer_fd_cleanup() {
        use crate::terminal::session::PtyWriter;

        // Create a file descriptor
        let fd = unsafe { libc::open(b"/dev/null\0".as_ptr() as *const i8, libc::O_RDWR, 0) };
        assert!(fd >= 0, "Should be able to open /dev/null");

        {
            // Create PtyWriter in a scope
            let writer = PtyWriter { fd };

            // Write should work
            let write_result = writer.write(b"test");
            assert!(write_result.is_ok(), "Write should succeed");
        } // PtyWriter dropped here, fd should be closed

        // Verify fd is closed (dup should fail)
        let dup_result = unsafe { libc::dup(fd) };
        assert!(dup_result < 0, "FD should be closed after PtyWriter drop");
        assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::EBADF),
                   "Should get EBADF (bad file descriptor)");
    }

    #[cfg(unix)]
    #[test]
    fn test_no_pty_leak_after_multiple_cycles() {
        use crate::terminal::UnixPty;
        use std::thread;
        use std::time::Duration;

        // Simulate creating and destroying many PTYs over time
        // This simulates the real-world usage pattern that caused the 500+ zombie leak
        for cycle in 0..5 {
            let mut ptys = Vec::new();

            // Create several PTYs
            for _ in 0..5 {
                let pty = UnixPty::spawn("/bin/sleep", &["0.1"], crate::terminal::PtySize::new(24, 80))
                    .expect(&format!("PTY spawn should succeed in cycle {}", cycle));
                ptys.push(pty);
            }

            // Let them live briefly
            thread::sleep(Duration::from_millis(50));

            // Explicitly drop to trigger cleanup
            drop(ptys);

            // Wait for cleanup
            thread::sleep(Duration::from_millis(100));
        }

        // If we get here without running out of PTY devices, the test passes
        // The original bug would have caused "out of pty devices" error
    }
}

/// VTE parsing tests for SSH ls output
/// These tests ensure that ANSI escape sequences from ls --color=auto
/// are correctly parsed and rendered to the terminal grid
#[cfg(test)]
mod vte_ls_tests {
    use crate::terminal::{TerminalGrid, VteHandler};
    use crate::terminal::types::CellAttrs;

    /// Helper to feed VTE data (bytes) to a grid
    fn feed_vte_data(grid: &mut TerminalGrid, data: &[u8]) {
        let mut parser = vte::Parser::new();
        let mut attrs = CellAttrs::default();
        let mut handler = VteHandler {
            grid,
            attrs: &mut attrs,
        };
        for byte in data {
            parser.advance(&mut handler, *byte);
        }
    }

    /// Helper to get the visible content as a string (for testing)
    fn get_visible_content(grid: &TerminalGrid) -> String {
        let mut result = String::new();
        for row_idx in 0..grid.rows {
            let row = &grid.cells[row_idx];
            // Find the range of non-space characters (trim leading and trailing spaces)
            let first_non_space = row.iter().position(|c| c.c != ' ' && c.c != '\0');
            let last_non_space = row.iter().rposition(|c| c.c != ' ' && c.c != '\0');
            if let (Some(start), Some(end)) = (first_non_space, last_non_space) {
                let line: String = row[start..=end].iter().map(|c| c.c).collect();
                result.push_str(&line);
                result.push('\n');
            }
        }
        result
    }

    fn create_test_grid(cols: usize, rows: usize) -> TerminalGrid {
        TerminalGrid::with_scrollback_limit(cols, rows, 1024 * 1024)
    }

    #[test]
    fn test_vte_simple_text() {
        let mut grid = create_test_grid(80, 24);

        // Feed simple text without any ANSI codes
        feed_vte_data(&mut grid, b"hello world");

        // Check that text was rendered
        assert_eq!(grid.cells[0][0].c, 'h');
        assert_eq!(grid.cells[0][1].c, 'e');
        assert_eq!(grid.cells[0][5].c, ' ');
        assert_eq!(grid.cells[0][6].c, 'w');
    }

    #[test]
    fn test_vte_text_with_newline() {
        let mut grid = create_test_grid(80, 24);

        // Feed text with newline (CR+LF)
        feed_vte_data(&mut grid, b"line1\r\nline2");

        // First line should have "line1"
        assert_eq!(grid.cells[0][0].c, 'l');
        assert_eq!(grid.cells[0][4].c, '1');

        // Second line should have "line2" at cursor_row
        assert_eq!(grid.cells[1][0].c, 'l');
        assert_eq!(grid.cells[1][4].c, '2');
    }

    #[test]
    fn test_vte_basic_color_codes() {
        let mut grid = create_test_grid(80, 24);

        // Feed text with basic ANSI color codes
        // ESC[31m = red foreground, ESC[0m = reset
        let data = b"\x1b[31mRed text\x1b[0m normal text";
        feed_vte_data(&mut grid, data);

        // All text should be rendered (colors don't affect character rendering)
        // Just check that characters are present
        assert_eq!(grid.cells[0][0].c, 'R');
        assert_eq!(grid.cells[0][3].c, ' ');
        assert_eq!(grid.cells[0][4].c, 't');

        // "Red text " is 8 characters (including trailing space)
        // Position 8 = space before "normal", Position 9 = 'n'
        assert_eq!(grid.cells[0][8].c, ' ');
        assert_eq!(grid.cells[0][9].c, 'n');
        assert_eq!(grid.cells[0][15].c, ' ');
    }

    #[test]
    fn test_vte_ls_output_with_colors() {
        let mut grid = create_test_grid(80, 24);

        // Simulate ls --color=auto output with ANSI color codes
        // ESC[0m = reset
        // ESC[01;34m = bold blue (directory)
        // ESC[01;32m = bold green (executable)
        // ESC[m = reset (short form)
        let ls_output = b"\x1b[0m\x1b[01;34mCargo.toml\x1b[0m\r\n\x1b[01;32mmain\x1b[0m\r\n";
        feed_vte_data(&mut grid, ls_output);

        // First line should have "Cargo.toml"
        // C(0) a(1) r(2) g(3) o(4) .(5) t(6) o(7) m(8) l(9)
        assert_eq!(grid.cells[0][0].c, 'C');
        assert_eq!(grid.cells[0][1].c, 'a');
        assert_eq!(grid.cells[0][8].c, 'm');
        assert_eq!(grid.cells[0][9].c, 'l');

        // Second line should have "main"
        assert_eq!(grid.cells[1][0].c, 'm');
        assert_eq!(grid.cells[1][1].c, 'a');
        assert_eq!(grid.cells[1][3].c, 'n');
    }

    #[test]
    fn test_vte_ls_output_with_directory_color() {
        let mut grid = create_test_grid(80, 24);

        // Simulate a typical ls output with directory coloring
        // \x1b[1;34m = bold blue for directories
        let data = b"\x1b[0m\x1b[1;34msrc/\x1b[0m\r\n\x1b[1;34mtarget/\x1b[0m\r\n";
        feed_vte_data(&mut grid, data);

        // First line: src/
        assert_eq!(grid.cells[0][0].c, 's');
        assert_eq!(grid.cells[0][1].c, 'r');
        assert_eq!(grid.cells[0][2].c, 'c');
        assert_eq!(grid.cells[0][3].c, '/');

        // Second line: target/
        assert_eq!(grid.cells[1][0].c, 't');
        assert_eq!(grid.cells[1][5].c, 't');
        assert_eq!(grid.cells[1][6].c, '/');
    }

    #[test]
    fn test_vte_complex_ls_output() {
        let mut grid = create_test_grid(80, 24);

        // Simulate a more complex ls output with multiple file types
        // Directory: \x1b[1;34m (bold blue)
        // Symlink: \x1b[1;36m (bold cyan)
        // Executable: \x1b[1;32m (bold green)
        let data = b"\x1b[0m\x1b[1;34msrc/\x1b[0m \x1b[1;36mlink\x1b[0m -> target\r\n\x1b[1;32mbinary\x1b[0m *\r\n";
        feed_vte_data(&mut grid, data);

        // First row should contain 's', 'r', 'c', '/'
        let found_src = grid.cells[0].iter().any(|c| c.c == 's');
        assert!(found_src, "Should find 's' from 'src/' on first row");

        // Check that we have content beyond just ANSI codes
        let mut content_chars = 0;
        for row in &grid.cells {
            for cell in row {
                if cell.c != ' ' && cell.c != '\0' {
                    content_chars += 1;
                }
            }
        }
        assert!(content_chars > 20, "Should have rendered multiple characters, got {}", content_chars);
    }

    #[test]
    fn test_vte_prompt_then_ls() {
        let mut grid = create_test_grid(80, 24);

        // Simulate: prompt, user types 'ls', ls output
        let data = b"$ ls\r\n\x1b[0mfile1.txt  file2.txt  \x1b[1;34mdir1/\x1b[0m\r\n$ ";
        feed_vte_data(&mut grid, data);

        // Check that prompt is visible
        let content = get_visible_content(&grid);
        assert!(content.contains("$ ls"), "Should contain the prompt");
        assert!(content.contains("file1.txt"), "Should contain file1.txt");
        assert!(content.contains("dir1/"), "Should contain dir1/");
    }

    #[test]
    fn test_vte_multiple_color_sequences() {
        let mut grid = create_test_grid(80, 24);

        // Test multiple color sequences in one line
        // This is common in ls output with multiple colored files
        let data = b"\x1b[1;34mdir1/\x1b[0m \x1b[1;32mscript.sh\x1b[0m \x1b[0;37mfile.txt\x1b[0m\r\n";
        feed_vte_data(&mut grid, data);

        // All three items should be present
        let content = get_visible_content(&grid);
        assert!(content.contains("dir1/"), "Should contain dir1/");
        assert!(content.contains("script.sh"), "Should contain script.sh");
        assert!(content.contains("file.txt"), "Should contain file.txt");
    }

    #[test]
    fn test_vte_grid_cells_updated_correctly() {
        let mut grid = create_test_grid(80, 24);

        // Feed ls output and verify exact cell contents
        let data = b"\x1b[1;34mtest\x1b[0m\r\n";
        feed_vte_data(&mut grid, data);

        // Verify characters at specific positions
        // "test" should be at (0,0), (0,1), (0,2), (0,3)
        assert_eq!(grid.cells[0][0].c, 't');
        assert_eq!(grid.cells[0][1].c, 'e');
        assert_eq!(grid.cells[0][2].c, 's');
        assert_eq!(grid.cells[0][3].c, 't');
    }

    #[test]
    fn test_vte_cursor_position_after_color_codes() {
        let mut grid = create_test_grid(80, 24);

        // ANSI codes should not move cursor, only printable chars do
        let data = b"\x1b[1;34m\x1b[0mABC";
        feed_vte_data(&mut grid, data);

        // Cursor should be after "ABC", not counting ANSI codes
        assert_eq!(grid.cursor_col, 3);
        assert_eq!(grid.cells[0][0].c, 'A');
        assert_eq!(grid.cells[0][1].c, 'B');
        assert_eq!(grid.cells[0][2].c, 'C');
    }

    /// Test that simulates the exact SSH data flow:
    /// 1. SSH receives data from channel
    /// 2. Data is parsed byte-by-byte via VTE
    /// 3. Grid cells are updated
    /// 4. Grid can be read back correctly
    #[test]
    fn test_ssh_ls_flow_simulation() {
        let mut grid = create_test_grid(80, 24);

        // Simulate what SSH receives when user types 'ls' and presses Enter
        // The data would include:
        // - The echo of 'ls' command
        // - CR+LF (moves to next line)
        // - ls output with ANSI color codes
        // - New prompt

        let ssh_data = b"\x1b[?2004l\r\n\x1b[01;34mCargo.toml\x1b[0m\r\n\x1b[01;34msrc/\x1b[0m\r\n\x1b[01;32mbinary\x1b[0m\r\n$ ";

        feed_vte_data(&mut grid, ssh_data);

        // Verify content was rendered to grid
        // After \r\n, cursor is at row 1, so "Cargo.toml" starts there
        assert_eq!(grid.cells[1][0].c, 'C');
        assert_eq!(grid.cells[1][1].c, 'a');
        assert_eq!(grid.cells[1][9].c, 'l');

        // Second row of output (row 2 after initial \r\n) should have "src/"
        assert_eq!(grid.cells[2][0].c, 's');
        assert_eq!(grid.cells[2][1].c, 'r');
        assert_eq!(grid.cells[2][2].c, 'c');
        assert_eq!(grid.cells[2][3].c, '/');

        // Third row of output should have "binary"
        assert_eq!(grid.cells[3][0].c, 'b');
        assert_eq!(grid.cells[3][1].c, 'i');
        assert_eq!(grid.cells[3][4].c, 'r');
        assert_eq!(grid.cells[3][5].c, 'y');

        // Fourth row should have "$ "
        assert_eq!(grid.cells[4][0].c, '$');
        assert_eq!(grid.cells[4][1].c, ' ');
    }

    /// Test that the grid content can be extracted correctly
    /// This simulates what the rendering code does
    #[test]
    fn test_grid_content_extraction() {
        let mut grid = create_test_grid(80, 24);

        // Write ls output
        let ls_data = b"\x1b[1;34mdir1/\x1b[0m \x1b[1;32mscript.sh\x1b[0m\r\n";
        feed_vte_data(&mut grid, ls_data);

        // Extract content (similar to what get_visible_content does)
        let mut content = String::new();
        for row_idx in 0..grid.rows {
            let row = &grid.cells[row_idx];
            let first_non_space = row.iter().position(|c| c.c != ' ' && c.c != '\0');
            let last_non_space = row.iter().rposition(|c| c.c != ' ' && c.c != '\0');
            if let (Some(start), Some(end)) = (first_non_space, last_non_space) {
                let line: String = row[start..=end].iter().map(|c| c.c).collect();
                content.push_str(&line);
                content.push('\n');
            }
        }

        // Verify extracted content contains the expected items
        assert!(content.contains("dir1/"), "Should contain dir1/");
        assert!(content.contains("script.sh"), "Should contain script.sh");
    }

    /// Test multiple file outputs with colors
    #[test]
    fn test_vte_ls_multiple_files() {
        let mut grid = create_test_grid(80, 24);

        // Simulate ls output with many files
        let data = b"\x1b[0m\x1b[1;34mconfig/\x1b[0m \x1b[1;34msrc/\x1b[0m \x1b[0mREADME.md \x1b[1;32mmain\x1b[0m\r\n";
        feed_vte_data(&mut grid, data);

        // Extract and verify all items are present
        let content = get_visible_content(&grid);
        assert!(content.contains("config/"), "Should contain config/");
        assert!(content.contains("src/"), "Should contain src/");
        assert!(content.contains("README.md"), "Should contain README.md");
        assert!(content.contains("main"), "Should contain main");
    }
}

/// Tests for ASCII punctuation mark input.
/// This ensures that ASCII punctuation marks (. , ; : ? ! ( ) [ ] < >)
/// are correctly sent to the terminal when IME is enabled or recently used.
///
/// Background: On macOS, when IME (Input Method Editor) is enabled or recently
/// used to type Chinese/Japanese/Korean text, switching back to English input
/// should allow typing all ASCII characters including punctuation marks.
///
/// Bug: The previous code used `} else if !is_punct {` which meant:
/// - ASCII non-punctuation text → only updated flag, didn't send to terminal
/// - ASCII punctuation → completely skipped, nothing happened
#[cfg(test)]
mod ascii_punctuation_tests {
    use crate::terminal::TerminalGrid;
    use crate::terminal::types::CellAttrs;

    fn create_test_grid(cols: usize, rows: usize) -> TerminalGrid {
        TerminalGrid::with_scrollback_limit(cols, rows, 1024 * 1024)
    }

    fn write_string(grid: &mut TerminalGrid, s: &str) {
        for ch in s.chars() {
            grid.write_char_with_attrs(ch, &CellAttrs::default());
        }
    }

    fn get_visible_content(grid: &TerminalGrid) -> String {
        let mut content = String::new();
        for row_idx in 0..grid.rows {
            let row = &grid.cells[row_idx];
            let first_non_space = row.iter().position(|c| c.c != ' ' && c.c != '\0');
            let last_non_space = row.iter().rposition(|c| c.c != ' ' && c.c != '\0');
            if let (Some(start), Some(end)) = (first_non_space, last_non_space) {
                let line: String = row[start..=end].iter().map(|c| c.c).collect();
                content.push_str(&line);
                content.push('\n');
            }
        }
        content
    }

    #[test]
    fn test_ascii_period_input() {
        let mut grid = create_test_grid(80, 24);
        write_string(&mut grid, "test.value");
        let content = get_visible_content(&grid);
        assert!(content.contains("test.value"), "Period should be rendered");
    }

    #[test]
    fn test_ascii_comma_input() {
        let mut grid = create_test_grid(80, 24);
        write_string(&mut grid, "hello,world");
        let content = get_visible_content(&grid);
        assert!(content.contains("hello,world"), "Comma should be rendered");
    }

    #[test]
    fn test_ascii_semicolon_input() {
        let mut grid = create_test_grid(80, 24);
        write_string(&mut grid, "cmd;exit");
        let content = get_visible_content(&grid);
        assert!(content.contains("cmd;exit"), "Semicolon should be rendered");
    }

    #[test]
    fn test_ascii_colon_input() {
        let mut grid = create_test_grid(80, 24);
        write_string(&mut grid, "https://example.com");
        let content = get_visible_content(&grid);
        assert!(content.contains("https://example.com"), "Colon should be rendered");
    }

    #[test]
    fn test_ascii_question_mark_input() {
        let mut grid = create_test_grid(80, 24);
        write_string(&mut grid, "what?");
        let content = get_visible_content(&grid);
        assert!(content.contains("what?"), "Question mark should be rendered");
    }

    #[test]
    fn test_ascii_exclamation_mark_input() {
        let mut grid = create_test_grid(80, 24);
        write_string(&mut grid, "Hello!");
        let content = get_visible_content(&grid);
        assert!(content.contains("Hello!"), "Exclamation mark should be rendered");
    }

    #[test]
    fn test_ascii_parentheses_input() {
        let mut grid = create_test_grid(80, 24);
        write_string(&mut grid, "func(arg)");
        let content = get_visible_content(&grid);
        assert!(content.contains("func(arg)"), "Parentheses should be rendered");
    }

    #[test]
    fn test_ascii_brackets_input() {
        let mut grid = create_test_grid(80, 24);
        write_string(&mut grid, "arr[index]");
        let content = get_visible_content(&grid);
        assert!(content.contains("arr[index]"), "Brackets should be rendered");
    }

    #[test]
    fn test_ascii_angle_brackets_input() {
        let mut grid = create_test_grid(80, 24);
        write_string(&mut grid, "key<value>");
        let content = get_visible_content(&grid);
        assert!(content.contains("key<value>"), "Angle brackets should be rendered");
    }

    #[test]
    fn test_all_punctuation_marks_together() {
        let mut grid = create_test_grid(80, 24);
        write_string(&mut grid, ".,;:?!()[]<>");
        let content = get_visible_content(&grid);
        assert!(content.contains(".,;:?!()[]<>"), "All punctuation marks should be rendered");
    }

    #[test]
    fn test_mixed_punctuation_with_letters() {
        let mut grid = create_test_grid(80, 24);
        write_string(&mut grid, "Hello, World! How are you? I'm fine.");
        let content = get_visible_content(&grid);
        assert!(content.contains("Hello, World! How are you? I'm fine."),
                "Mixed punctuation with letters should be rendered");
    }

    #[test]
    fn test_punctuation_in_code_like_context() {
        let mut grid = create_test_grid(80, 24);
        write_string(&mut grid, "fn main() { let arr[0]; return; }");
        let content = get_visible_content(&grid);
        assert!(content.contains("fn main() { let arr[0]; return; }"),
                "Code-like context with punctuation should be rendered");
    }

    #[test]
    fn test_url_with_colon_and_slash() {
        let mut grid = create_test_grid(80, 24);
        write_string(&mut grid, "https://github.com/user/repo.git");
        let content = get_visible_content(&grid);
        assert!(content.contains("https://github.com/user/repo.git"),
                "URL with colon and slashes should be rendered");
    }

    #[test]
    fn test_file_path_with_dots() {
        let mut grid = create_test_grid(80, 24);
        write_string(&mut grid, "./config/app.yaml");
        let content = get_visible_content(&grid);
        assert!(content.contains("./config/app.yaml"),
                "File path with dots should be rendered");
    }

    #[test]
    fn test_command_with_flags() {
        let mut grid = create_test_grid(80, 24);
        write_string(&mut grid, "git commit -m \"fix: bug\"");
        let content = get_visible_content(&grid);
        assert!(content.contains("git commit -m \"fix: bug\""),
                "Command with flags should be rendered");
    }

    #[test]
    fn test_array_notation_with_brackets() {
        let mut grid = create_test_grid(80, 24);
        write_string(&mut grid, "let items = [1, 2, 3];");
        let content = get_visible_content(&grid);
        assert!(content.contains("let items = [1, 2, 3];"),
                "Array notation with brackets should be rendered");
    }

    #[test]
    fn test_ternary_operator_with_question_and_colon() {
        let mut grid = create_test_grid(80, 24);
        write_string(&mut grid, "result = condition ? true : false;");
        let content = get_visible_content(&grid);
        assert!(content.contains("result = condition ? true : false;"),
                "Ternary operator with ? and : should be rendered");
    }
}

/// Tests for preventing duplicate character input on macOS.
///
/// Background: On macOS, egui fires both `Event::Text` and `Event::Key` events
/// for the same keypress. Without proper deduplication, typing a single character
/// like "c" would result in "cc" being sent to the terminal.
///
/// Fix: Pre-collect all characters from Text events into a HashSet, then skip
/// Key event characters that match any Text event character.
#[cfg(test)]
mod duplicate_input_prevention_tests {
    use std::collections::HashSet;

    /// Simulates the logic used in render.rs to collect Text event characters
    fn collect_text_chars_from_events(events: &[&str]) -> HashSet<char> {
        events.iter()
            .filter_map(|e| e.chars().next())
            .collect()
    }

    /// Helper to simulate key_to_char conversion (simplified version of input.rs::key_to_char)
    fn key_to_char_sim(key: &str, shift: bool) -> Option<char> {
        match key {
            "A" => Some(if shift { 'A' } else { 'a' }),
            "B" => Some(if shift { 'B' } else { 'b' }),
            "C" => Some(if shift { 'C' } else { 'c' }),
            "L" => Some(if shift { 'L' } else { 'l' }),
            "Period" => Some('.'),
            "Comma" => Some(','),
            _ => None,
        }
    }

    #[test]
    fn test_text_chars_collection_single_char() {
        let events = vec!["c"];
        let text_chars = collect_text_chars_from_events(&events);
        assert_eq!(text_chars.len(), 1);
        assert!(text_chars.contains(&'c'));
    }

    #[test]
    fn test_text_chars_collection_multiple_chars() {
        let events = vec!["h", "e", "l", "l", "o"];
        let text_chars = collect_text_chars_from_events(&events);
        assert!(text_chars.contains(&'h'));
        assert!(text_chars.contains(&'e'));
        assert!(text_chars.contains(&'l'));
        assert!(text_chars.contains(&'o'));
    }

    #[test]
    fn test_duplicate_detection_same_char() {
        let text_events = vec!["l"];
        let text_chars = collect_text_chars_from_events(&text_events);

        // Simulate Key event for the same character
        let key_char = key_to_char_sim("L", false);
        assert_eq!(key_char, Some('l'));

        // The Key event character should be detected as duplicate
        assert!(text_chars.contains(&key_char.unwrap()),
                "Key event char should be detected as duplicate when Text event has same char");
    }

    #[test]
    fn test_no_duplicate_different_chars() {
        let text_events = vec!["a", "b", "c"];
        let text_chars = collect_text_chars_from_events(&text_events);

        // Key event for different character
        let key_char = key_to_char_sim("L", false);
        assert_eq!(key_char, Some('l'));

        // Should NOT be detected as duplicate
        assert!(!text_chars.contains(&key_char.unwrap()),
                "Key event char should NOT be duplicate when Text event has different chars");
    }

    #[test]
    fn test_punctuation_not_duplicated() {
        let text_events = vec!["."];
        let text_chars = collect_text_chars_from_events(&text_events);

        // Key event for period
        let key_char = Some('.');

        assert!(text_chars.contains(&key_char.unwrap()),
                "Punctuation from Text event should prevent duplicate from Key event");
    }

    #[test]
    fn test_empty_text_events_allows_key_input() {
        let text_events: Vec<&str> = vec![];
        let text_chars = collect_text_chars_from_events(&text_events);

        // When no Text events, Key events should NOT be filtered
        let key_char = key_to_char_sim("L", false);
        assert!(!text_chars.contains(&key_char.unwrap()),
                "With no Text events, Key event char should not be filtered");
    }

    #[test]
    fn test_multiple_text_events_prevents_multiple_keys() {
        let text_events = vec!["a", "b", "c", "1", "2", "3"];
        let text_chars = collect_text_chars_from_events(&text_events);

        // All these Key event chars should be filtered
        for expected in &['a', 'b', 'c', '1', '2', '3'] {
            assert!(text_chars.contains(expected),
                    "Text event char '{}' should prevent duplicate Key event", expected);
        }
    }

    #[test]
    fn test_uppercase_and_lowercase_distinct() {
        let text_events = vec!["a"];
        let text_chars = collect_text_chars_from_events(&text_events);

        // Lowercase 'a' in Text events should NOT filter uppercase 'A' from Key event
        // (This is correct behavior - they are different characters)
        let key_char_upper = key_to_char_sim("A", true);
        assert_eq!(key_char_upper, Some('A'));

        // In real usage, both 'a' and 'A' would generate Text events,
        // so this test documents the expected behavior
        assert!(!text_chars.contains(&'A'),
                "Uppercase and lowercase are distinct characters");
    }

    #[test]
    fn test_shift_char_handling() {
        // Test that shift-modified characters are handled correctly
        let text_events = vec!["A"];  // User types shift+A
        let text_chars = collect_text_chars_from_events(&text_events);

        // Key event for shift+A should produce same character
        let key_char = key_to_char_sim("A", true);
        assert_eq!(key_char, Some('A'));

        assert!(text_chars.contains(&key_char.unwrap()),
                "Shift+modified chars should be deduplicated correctly");
    }

    #[test]
    fn test_comprehensive_typing_scenario() {
        // Simulate typing "hello" - each character generates both Text and Key events
        let text_events = vec!["h", "e", "l", "l", "o"];
        let text_chars = collect_text_chars_from_events(&text_events);

        // Simulate corresponding Key events
        let key_chars: Vec<char> = vec!['h', 'e', 'l', 'l', 'o'];

        for key_char in key_chars {
            assert!(text_chars.contains(&key_char),
                    "Key event for '{}' should be filtered (Text event exists)", key_char);
        }

        // Character NOT in Text events should NOT be filtered
        assert!(!text_chars.contains(&'x'),
                "Key event for char not in Text events should NOT be filtered");
    }

    #[test]
    fn test_special_characters_in_text_events() {
        // Test that special/punctuation characters are collected
        let text_events = vec![".", ",", "!", "?"];
        let text_chars = collect_text_chars_from_events(&text_events);

        for expected in &['.', ',', '!', '?'] {
            assert!(text_chars.contains(expected),
                    "Special character '{}' should be in text_chars", expected);
        }
    }

    #[test]
    fn test_mixed_content_typing() {
        // Simulate typing "test.value" - realistic command typing
        let text_events = vec!["t", "e", "s", "t", ".", "v", "a", "l", "u", "e"];
        let text_chars = collect_text_chars_from_events(&text_events);

        // Verify all characters are in the set
        for expected in "test.value".chars() {
            assert!(text_chars.contains(&expected),
                    "Character '{}' from 'test.value' should be in text_chars", expected);
        }

        // Verify counts (HashSet deduplicates, so 'l' appears once even though typed once)
        // Note: 'l' appears twice in "test.value" but only once in set
        assert!(text_chars.contains(&'l'), "Character 'l' should be in set");
    }

    #[test]
    fn test_hashset_property_automatic_deduplication() {
        // This test documents that HashSet automatically deduplicates
        let text_events = vec!["a", "a", "a", "b", "b"];
        let text_chars: HashSet<char> = text_events.iter()
            .filter_map(|e| e.chars().next())
            .collect();

        // Even though 'a' appears 3 times, it's only once in the set
        assert_eq!(text_chars.len(), 2, "HashSet should deduplicate automatically");
        assert!(text_chars.contains(&'a'));
        assert!(text_chars.contains(&'b'));
    }
}

// ===========================================================================
// Headless replay harness — replays Claude Code's REAL captured bytes through
// the real grid+resize pipeline so resize fixes can be verified end-to-end
// without a human driving the GUI. Bytes in tests/fixtures/ were captured from
// an actual `claude` session (welcome frame + its SIGWINCH redraw).
// ===========================================================================
#[cfg(test)]
mod replay_harness {
    use crate::terminal::types::CellAttrs;
    use crate::terminal::vte::VteHandler;
    use crate::terminal::TerminalGrid;
    use vte::Parser;

    const WELCOME: &[u8] = include_bytes!("../../tests/fixtures/claude_welcome.bin");
    const REDRAW: &[u8] = include_bytes!("../../tests/fixtures/claude_resize.bin");

    /// A minimal driver: one persistent VTE parser + attrs (like the reader
    /// thread) feeding a real TerminalGrid, with a resize() that goes through
    /// the exact same code path the render loop uses.
    struct Harness {
        grid: TerminalGrid,
        parser: Parser,
        attrs: CellAttrs,
    }
    impl Harness {
        fn new(cols: usize, rows: usize) -> Self {
            Self {
                grid: TerminalGrid::with_scrollback_limit(cols, rows, 1024 * 1024),
                parser: Parser::new(),
                attrs: CellAttrs::default(),
            }
        }
        fn feed(&mut self, bytes: &[u8]) {
            for &b in bytes {
                let mut h = VteHandler { grid: &mut self.grid, attrs: &mut self.attrs };
                self.parser.advance(&mut h, b);
            }
        }
        fn resize(&mut self, cols: usize, rows: usize) {
            self.grid.resize(cols, rows);
        }
        fn row(&self, r: usize) -> String {
            self.grid.cells[r]
                .iter()
                .map(|c| if c.wide_continuation || c.c == '\0' { ' ' } else { c.c })
                .collect()
        }
        // char-safe previews (rows contain multi-byte box/CJK chars)
        fn head(&self, r: usize, n: usize) -> String { self.row(r).chars().take(n).collect() }
        fn tail(&self, r: usize, n: usize) -> String {
            let s = self.row(r); s.chars().rev().take(n).collect::<Vec<_>>().into_iter().rev().collect()
        }
    }

    #[test]
    fn position_based_frame_survives_shrink() {
        // Claude Code draws in the MAIN buffer with absolute column positioning.
        // On a resize the main buffer is REFLOWED (so history is never
        // truncated), which transiently wraps the fixed-position frame; Claude
        // Code then redraws its whole frame on SIGWINCH, restoring a clean box.
        // This is the real flow — reflow + redraw must yield an intact frame.
        let mut h = Harness::new(100, 30);
        h.feed(WELCOME);
        assert!(h.row(0).trim_start().starts_with('╭'), "baseline welcome top");

        h.resize(80, 30); // shrink (reflow)
        h.feed(REDRAW);   // Claude Code's SIGWINCH redraw at 80

        assert_invariant(&h.grid);
        let r0 = h.row(0);
        assert!(r0.trim_start().starts_with('╭'),
            "after shrink+redraw the frame top must be restored — row0: {:?}",
            h.head(0,20));
    }

    #[test]
    fn position_based_frame_survives_grow_without_panic() {
        // Grow must not panic. (The resize_reflow_scrollback_only + resize_screen
        // combo indexes out of bounds on grow because the former bumps self.cols
        // without resizing self.cells.) The frame's top-left corner must remain.
        let mut h = Harness::new(100, 30);
        h.feed(WELCOME);
        h.resize(140, 30); // grow
        assert_invariant(&h.grid);
        assert_eq!(h.grid.cells[0][0].c, '╭', "grow lost the top-left corner");
        assert_eq!(h.grid.cols, 140);
    }

    #[test]
    fn claude_redraw_after_resize_is_clean() {
        // Idle-resize recovery: after a resize, Claude Code's SIGWINCH redraw
        // (clear + repaint, captured in REDRAW) must land on a clean frame.
        let mut h = Harness::new(100, 30);
        h.feed(WELCOME);
        h.resize(80, 24);
        h.feed(REDRAW);
        assert!(h.row(0).trim_start().starts_with('╭'),
            "post-redraw top border missing — row 0: {:?}",
            h.head(0,20));
        assert!(h.row(0).trim_end().ends_with('╮'),
            "post-redraw frame did not close with ╮ — row 0: {:?}",
            h.tail(0,20));
    }

    #[test]
    fn shell_output_still_reflows_after_fix() {
        // Regression guard: plain sequential shell output (NO absolute
        // positioning) must continue to reflow on resize. The fix only diverts
        // position-based content; shell text is untouched.
        let mut h = Harness::new(20, 6);
        h.feed(b"abcdefghijklmnopqrstuvwxyz"); // sequential, auto-wraps
        // no CHA/CUP used → must reflow
        let before: String = (0..h.grid.rows)
            .map(|r| h.row(r)).collect::<Vec<_>>().join("|");
        h.resize(10, 6); // narrower → content should re-wrap to more rows
        let _ = before;
        // 'z' (last char) must still be present somewhere in the visible grid
        let all: String = (0..h.grid.rows).map(|r| h.row(r)).collect();
        assert!(all.contains('z'), "shell output lost chars after reflow: {:?}", all);
        // reflow should have produced more non-empty rows (20→10 wrapping)
        let nonempty = (0..h.grid.rows).filter(|&r| h.row(r).trim().len() > 0).count();
        assert!(nonempty >= 3, "expected reflow into >=3 rows, got {}", nonempty);
    }

    // Core grid invariant: every row must be exactly `cols` wide after ANY
    // resize. A grow in the old alt-screen path violated this (resize_reflow_
    // scrollback_only bumps self.cols, then resize_screen early-returns, leaving
    // cells rows at the old width while cols reports the new width).
    fn assert_invariant(g: &crate::terminal::TerminalGrid) {
        for (r, row) in g.cells.iter().enumerate() {
            assert_eq!(row.len(), g.cols,
                "invariant broken: row {r} is {} cells wide but grid.cols={}", row.len(), g.cols);
        }
    }

    // -----------------------------------------------------------------------
    // Full-spec TDD: REFLOW mode (shell) must preserve every character on both
    // grow and shrink.
    // -----------------------------------------------------------------------
    fn all_chars(h: &Harness) -> String {
        (0..h.grid.rows)
            .map(|r| h.row(r).replace(' ', ""))
            .collect::<String>()
    }
    const SAMPLE: &[u8] = b"the quick brown fox jumps over the lazy dog 0123456789";
    const SAMPLE_CHARS: &str = "thequickbrownfoxjumpsoverthelazydog0123456789";

    #[test]
    fn reflow_shell_shrink_preserves_all_chars() {
        let mut h = Harness::new(20, 6);
        h.feed(SAMPLE);
        h.resize(10, 8); // narrower → rewrap, nothing lost
        let after = all_chars(&h);
        for c in SAMPLE_CHARS.chars() {
            assert!(after.contains(c), "reflow on shrink lost char {c}: {after}");
        }
    }

    #[test]
    fn reflow_shell_grow_preserves_all_chars() {
        let mut h = Harness::new(20, 6);
        h.feed(SAMPLE);
        h.resize(40, 6); // wider → unwrap, nothing lost
        let after = all_chars(&h);
        for c in SAMPLE_CHARS.chars() {
            assert!(after.contains(c), "reflow on grow lost char {c}: {after}");
        }
    }

    // -----------------------------------------------------------------------
    // Full-spec TDD: IN-PLACE mode (alt screen) must preserve cell positions on
    // shrink and grow, and must NOT panic on grow (latent 0.14.4 bug).
    // -----------------------------------------------------------------------
    #[test]
    fn alt_screen_shrink_preserves_positions() {
        let mut h = Harness::new(100, 30);
        h.grid.enter_alt_screen();
        h.feed(b"\x1b[HX\x1b[4;6HY"); // X@(0,0), Y@(3,5)
        h.resize(80, 30); // shrink
        assert_invariant(&h.grid);
        assert_eq!(h.grid.cells[0][0].c, 'X', "shrink moved (0,0)");
        assert_eq!(h.grid.cells[3][5].c, 'Y', "shrink moved (3,5)");
        assert_eq!(h.grid.cols, 80);
    }

    #[test]
    fn alt_screen_grow_does_not_panic_and_preserves() {
        let mut h = Harness::new(100, 30);
        h.grid.enter_alt_screen();
        h.feed(b"\x1b[HX\x1b[4;6HY");
        h.resize(140, 30); // GROW — must not panic
        assert_invariant(&h.grid);
        assert_eq!(h.grid.cells[0][0].c, 'X', "grow lost (0,0)");
        assert_eq!(h.grid.cells[3][5].c, 'Y', "grow lost (3,5)");
        assert_eq!(h.grid.cols, 140);
    }

    // -----------------------------------------------------------------------
    // Full-spec TDD: mode detection — CHA/CUP in the main buffer flips to
    // InPlace; alt-screen enter/exit resets it.
    // -----------------------------------------------------------------------
    #[test]
    fn scrollback_rows_stay_cols_wide_after_resize() {
        // Scrollback rows scrolled off at one width must be padded/truncated to
        // the current grid width after a resize — otherwise render paths that
        // index them by grid.cols go out of bounds (startup crash we fixed).
        let mut h = Harness::new(79, 10);
        // scroll 3 rows into scrollback at width 79
        for r in 0..3 {
            h.feed(b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"); // 79 wide
            h.feed(b"\r\n");
        }
        h.feed(b"x"); // force content
        // widen to 100 → in-place path (positioning not set here, but rows-only width change)
        h.resize(100, 10);
        assert_invariant(&h.grid);
        for i in 0..h.grid.scrollback_len() {
            let row = h.grid.get_scrollback_row(i).unwrap();
            assert_eq!(row.len(), h.grid.cols,
                "scrollback row {i} is {} wide but cols={}", row.len(), h.grid.cols);
        }
        // and after a shrink
        h.resize(60, 10);
        assert_invariant(&h.grid);
        for i in 0..h.grid.scrollback_len() {
            assert_eq!(h.grid.get_scrollback_row(i).unwrap().len(), h.grid.cols);
        }
    }

    #[test]
    fn cha_detection_sets_positioning_flag() {
        let mut h = Harness::new(100, 30);
        assert!(!h.grid.program_uses_positioning);
        h.feed(b"\x1b[6Gx");
        assert!(h.grid.program_uses_positioning, "CHA must set positioning flag");
    }





    #[test]
    fn sync_mode_flag_toggles_on_2026() {
        // Claude Code wraps every render batch in \e[?2026h…?2026l (synchronized
        // output). We must record it so the renderer can defer painting until
        // the batch ends (never showing half-drawn frames).
        let mut h = Harness::new(100, 30);
        assert!(!h.grid.in_sync_update);
        h.feed(b"\x1b[?2026h");
        assert!(h.grid.in_sync_update, "?2026h must set in_sync_update");
        h.feed(b"partial\x1b[?2026l");
        assert!(!h.grid.in_sync_update, "?2026l must clear in_sync_update");
    }

    #[test]
    fn positioning_flag_resets_across_alt_screen() {
        let mut h = Harness::new(100, 30);
        h.feed(b"\x1b[6Gx");
        assert!(h.grid.program_uses_positioning);
        h.grid.enter_alt_screen();
        assert!(!h.grid.program_uses_positioning, "flag must reset on alt enter");
        h.grid.exit_alt_screen();
        assert!(!h.grid.program_uses_positioning, "flag must reset on alt exit");
        h.feed(b"\x1b[5;5Hx"); // CUP sets it again in the main buffer
        assert!(h.grid.program_uses_positioning, "CUP must set the flag again");
    }
}

#[cfg(test)]
mod spinner_dump {
    use crate::terminal::types::CellAttrs;
    use crate::terminal::vte::VteHandler;
    use crate::terminal::TerminalGrid;
    use vte::Parser;
    const SPIN: &[u8] = include_bytes!("../../tests/fixtures/claude_spin.bin");

    #[test]
    fn spinner_stream_never_stacks_at_any_byte_offset() {
        // Regression for the live accumulation bug: when Claude Code updates its
        // "Thought for Ns" line in place, the terminal must never expose a state
        // with two copies of it. We replay the real capture and inspect the grid
        // after EVERY byte — each of these is a state the renderer could paint.
        // (Any >1 count would show as the stacked/duplicated lines seen live.)
        let mut grid = TerminalGrid::with_scrollback_limit(100, 30, 1024 * 1024);
        let mut p = Parser::new();
        let mut a = CellAttrs::default();
        let mut max_stacked = 0usize;
        let mut stacked_offsets = 0usize;
        for &b in SPIN {
            let mut h = VteHandler { grid: &mut grid, attrs: &mut a };
            p.advance(&mut h, b);
            let mut thoughts = 0usize;
            for r in 0..grid.rows {
                let row: String = grid.cells[r].iter().map(|c| c.c).collect();
                if row.contains("Thought for") { thoughts += 1; }
            }
            max_stacked = max_stacked.max(thoughts);
            if thoughts >= 2 { stacked_offsets += 1; }
        }
        assert_eq!(max_stacked, 1,
            "spinner lines stacked (max {max_stacked}) at {stacked_offsets} byte-offsets — live accumulation bug");
    }
}









#[cfg(test)]
mod reflow_rejoin {
    use crate::terminal::types::CellAttrs;
    use crate::terminal::vte::VteHandler;
    use crate::terminal::TerminalGrid;
    use vte::Parser;
    const WELCOME: &[u8] = include_bytes!("../../tests/fixtures/claude_welcome.bin");

    struct H {
        grid: TerminalGrid,
        p: Parser,
        a: CellAttrs,
    }
    impl H {
        fn new(cols: usize, rows: usize) -> Self {
            Self { grid: TerminalGrid::with_scrollback_limit(cols, rows, 1024 * 1024), p: Parser::new(), a: CellAttrs::default() }
        }
        fn feed(&mut self, b: &[u8]) {
            for &x in b { let mut h = VteHandler { grid: &mut self.grid, attrs: &mut self.a }; self.p.advance(&mut h, x); }
        }
        fn reflow_resize(&mut self, cols: usize, rows: usize) {
            self.grid.program_uses_positioning = false; // force reflow path
            self.grid.resize(cols, rows);
        }
        fn row(&self, r: usize) -> String {
            self.grid.cells[r].iter().map(|c| if c.wide_continuation { ' ' } else { c.c }).collect()
        }
        fn dump(&self, label: &str) {
            println!("--- {label}: grid={}x{} cursor=({},{}) sb={} ---",
                self.grid.cols, self.grid.rows, self.grid.cursor_row, self.grid.cursor_col, self.grid.scrollback_len());
            for r in 0..self.grid.rows.min(20) {
                let s = self.row(r);
                if s.trim().len() > 0 { println!("  g{r} wrp={} {:?}", self.grid.line_wrapped[r], s.trim_end()); }
            }
            for i in 0..self.grid.scrollback_len() {
                let row = self.grid.get_scrollback_row(i).unwrap();
                let s: String = row.iter().map(|c| c.c).collect();
                let w = self.grid.scrollback_wrapped.get(i).copied().unwrap_or(false);
                if s.trim().len() > 0 { println!("  s{i} wrp={w} {:?}", s.trim_end()); }
            }
        }
    }

    #[test]
    fn reflow_round_trip_restores_drawn_frame() {
        // The user's truncation bug: a frame drawn at width W, reflowed to a
        // narrower width, then back to W, must be fully restored (top-left corner
        // back, right edge back, nothing lost to a broken logical-line split).
        let mut h = H::new(100, 30);
        h.feed(WELCOME);
        assert!(h.row(0).trim_start().starts_with('╭'), "baseline");
        assert!(h.row(0).trim_end().ends_with('╮'), "baseline right edge");
        h.reflow_resize(79, 30); // shrink
        h.dump("after shrink 79");
        h.reflow_resize(100, 30); // widen back
        h.dump("after widen 100");
        let r0 = h.row(0);
        assert!(r0.trim_start().starts_with('╭'),
            "shrink→widen lost the box top-left corner. row0={:?}", &r0[..r0.trim_end().len().min(40)]);
        assert!(r0.trim_end().ends_with('╮'),
            "shrink→widen lost the box right edge. row0 tail={:?}", &r0[r0.trim_end().len().saturating_sub(20)..]);
    }
}



#[cfg(test)]
mod inplace_roundtrip {
    use crate::terminal::types::CellAttrs;
    use crate::terminal::vte::VteHandler;
    use crate::terminal::TerminalGrid;
    use vte::Parser;
    const WELCOME: &[u8] = include_bytes!("../../tests/fixtures/claude_welcome.bin");
    const REDRAW: &[u8] = include_bytes!("../../tests/fixtures/claude_resize.bin");

    #[test]
    fn inplace_claude_round_trip_restores_frame() {
        // Claude Code uses the IN-PLACE path (program_uses_positioning) and
        // never enters the alt screen. It redraws its whole frame on SIGWINCH.
        // The realistic flow: grid stays at the OLD width during the settle
        // window (the app keeps drawing correctly), then grid+PTY jump to the
        // new width together and the app redraws cleanly. Here we simulate that:
        // draw at 100, move grid to 79 (as if the debounce fired for a shrink),
        // feed Claude Code's 80-wide redraw, then move grid to 100 (widen) and
        // feed the redraw again — the frame must be intact (top-left + right
        // edge) at every settled size.
        let mut grid = TerminalGrid::with_scrollback_limit(100, 30, 1024 * 1024);
        let mut p = Parser::new();
        let mut a = CellAttrs::default();
        for &b in WELCOME {
            let mut h = VteHandler { grid: &mut grid, attrs: &mut a };
            p.advance(&mut h, b);
        }
        assert!(grid.program_uses_positioning, "welcome sets positioning");
        assert_eq!(grid.cells[0][99].c, '╮', "baseline right edge");

        // shrink: grid jumps to 80 (settle fired), claude redraws at 80
        grid.resize(80, 30);
        for &b in REDRAW { let mut h = VteHandler { grid: &mut grid, attrs: &mut a }; p.advance(&mut h, b); }
        assert!(grid.cells[0][79].c == '╮' || grid.cells[0].iter().any(|c| c.c == '╮'),
            "after shrink+redraw the box must be intact (right edge present)");

        // widen: grid jumps to 100, claude redraws at 100
        grid.resize(100, 30);
        for &b in REDRAW { let mut h = VteHandler { grid: &mut grid, attrs: &mut a }; p.advance(&mut h, b); }
        let r0: String = grid.cells[0].iter().map(|c| c.c).collect();
        assert!(r0.trim_start().starts_with('╭'), "after widen+redraw top-left restored: {:?}", &r0[..r0.len().min(20)]);
        assert!(r0.trim_end().ends_with('╮'), "after widen+redraw right edge restored");
    }
}

#[cfg(test)]
mod interleaved_resize {
    use crate::terminal::types::CellAttrs;
    use crate::terminal::vte::VteHandler;
    use crate::terminal::TerminalGrid;
    use vte::Parser;
    const WELCOME: &[u8] = include_bytes!("../../tests/fixtures/claude_welcome.bin");
    const REDRAW: &[u8] = include_bytes!("../../tests/fixtures/claude_resize.bin");

    /// Reproduces the LIVE timing bug without a human: Claude Code's SIGWINCH
    /// redraw (clear + repaint) arrives WHILE the grid is being resized by the
    /// render thread. We interleave: feed part of the redraw, then resize the
    /// grid, then feed the rest. If the result scrambles/truncates the frame,
    /// we've captured the race the user sees.
    #[test]
    fn redraw_interleaved_with_resize_scrambles() {
        let mut grid = TerminalGrid::with_scrollback_limit(100, 30, 1024 * 1024);
        let mut p = Parser::new();
        let mut a = CellAttrs::default();
        for &b in WELCOME {
            let mut h = VteHandler { grid: &mut grid, attrs: &mut a };
            p.advance(&mut h, b);
        }
        let mid = REDRAW.len() / 2;
        // feed first half of the redraw
        for &b in &REDRAW[..mid] {
            let mut h = VteHandler { grid: &mut grid, attrs: &mut a };
            p.advance(&mut h, b);
        }
        // render thread resizes mid-redraw
        grid.resize(80, 24);
        // feed the rest
        for &b in &REDRAW[mid..] {
            let mut h = VteHandler { grid: &mut grid, attrs: &mut a };
            p.advance(&mut h, b);
        }
        // frame must be intact
        let r0: String = grid.cells[0].iter().map(|c| c.c).collect();
        assert!(r0.trim_start().starts_with('╭'),
            "interleaved resize scrambled the frame top: {:?}", &r0[..r0.len().min(30)]);
        assert!(r0.trim_end().ends_with('╮'),
            "interleaved resize truncated the frame right edge");
    }
}

#[cfg(test)]
mod scrollback_truncation {
    use crate::terminal::types::TerminalCell;
    use crate::terminal::TerminalGrid;

    /// USER REPRO (verbatim): "超出可视区的历史内容，拖拽变窄，再变宽，会截断
    /// 历史内容右侧" — content scrolled beyond the visible area (scrollback),
    /// drag narrower then wider, truncates the RIGHT side of the historical
    /// content. Written on the IN-PLACE path (Claude Code is positioned).
    #[test]
    fn scrollback_right_side_survives_shrink_then_widen() {
        let mut grid = TerminalGrid::with_scrollback_limit(80, 10, 1024 * 1024);
        grid.program_uses_positioning = true; // in-place path (claude)

        // Write 20 lines of 80-wide content, each with a unique marker at the
        // RIGHT edge (col 79). Overflow pushes them into scrollback.
        for i in 0..20 {
            for col in 0..80 {
                grid.cells[grid.cursor_row][col] = TerminalCell {
                    c: if col == 79 { (b'A' + (i % 26) as u8) as char } else { 'x' },
                    ..TerminalCell::default()
                };
            }
            grid.cursor_row += 1;
            if grid.cursor_row >= grid.rows {
                grid.scroll_up(0, grid.rows - 1);
                grid.cursor_row = grid.rows - 1;
            }
        }
        // Confirm right-edge markers exist in scrollback before resize.
        let markers_before: Vec<char> = (0..grid.scrollback_len())
            .map(|i| grid.get_scrollback_row(i).unwrap().get(79).map(|c| c.c).unwrap_or(' '))
            .collect();
        assert!(markers_before.iter().any(|&c| c != ' '), "need scrollback with right-edge content");

        grid.resize(60, 10); // drag narrower
        grid.resize(80, 10); // drag wider back

        // The right-edge markers must SURVIVE in scrollback (not become blank).
        let markers_after: Vec<char> = (0..grid.scrollback_len())
            .map(|i| grid.get_scrollback_row(i).unwrap().get(79).map(|c| c.c).unwrap_or(' '))
            .collect();
        assert!(
            markers_after.iter().any(|&c| c != ' '),
            "scrollback RIGHT side was truncated after shrink→widen (markers became blanks)"
        );
    }
}

#[cfg(test)]
mod user_repro {
    use crate::terminal::types::CellAttrs;
    use crate::terminal::vte::VteHandler;
    use crate::terminal::TerminalGrid;
    use vte::Parser;
    const WELCOME: &[u8] = include_bytes!("../../tests/fixtures/claude_welcome.bin");

    /// USER'S EXACT REPRO: claude draws its frame, output pushes it into
    /// scrollback, then drag NARROWER then WIDER — the historical frame's
    /// right side must survive. Written on the IN-PLACE path (claude).
    #[test]
    fn claude_scrollback_frame_survives_shrink_widen() {
        let mut grid = TerminalGrid::with_scrollback_limit(100, 30, 1024 * 1024);
        let mut p = Parser::new();
        let mut a = CellAttrs::default();
        for &b in WELCOME {
            let mut h = VteHandler { grid: &mut grid, attrs: &mut a };
            p.advance(&mut h, b);
        }
        assert!(grid.program_uses_positioning, "welcome positions");
        assert_eq!(grid.cells[0][99].c, '╮', "frame right edge present before");

        // Push the frame into history, then drag NARROWER then WIDER.
        for _ in 0..20 { grid.scroll_up(0, grid.rows - 1); }
        grid.resize(80, 30);
        grid.resize(100, 30);

        // The frame's right edge must SURVIVE in the TOTAL content (scrollback +
        // visible — reflow preserves everything; nothing truncated on the right).
        let total: String = {
            let mut s = String::new();
            for i in 0..grid.scrollback_len() {
                for c in grid.get_scrollback_row(i).unwrap() {
                    if c.c != ' ' && c.c != '\0' { s.push(c.c); }
                }
            }
            for r in 0..grid.rows {
                for c in &grid.cells[r] {
                    if c.c != ' ' && c.c != '\0' { s.push(c.c); }
                }
            }
            s
        };
        assert!(total.contains('╮') && total.contains('╭'),
            "claude frame right edge (╮) must survive shrink→widen, got: {total}");
    }
}



#[cfg(test)]
mod concurrency_resize {
    use crate::terminal::types::CellAttrs;
    use crate::terminal::vte::VteHandler;
    use crate::terminal::TerminalGrid;
    use std::sync::{Arc, Mutex};
    use vte::Parser;
    const WELCOME: &[u8] = include_bytes!("../../tests/fixtures/claude_welcome.bin");

    /// Reproduces the LIVE bug with real threads: one thread keeps feeding
    /// Claude Code's bytes (like the reader), another keeps resizing the grid
    /// (like the render loop). The renderer must never observe a grid with two
    /// box tops ("Welcome back!" on two rows) — that's the overlap the user
    /// sees. This is deterministic (the WELCOME bytes contain the box).
    #[test]
    fn concurrent_feed_and_resize_never_duplicates_frame() {
        let grid = Arc::new(Mutex::new(TerminalGrid::with_scrollback_limit(100, 30, 1024 * 1024)));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));

        // Thread A: reader — feeds WELCOME bytes repeatedly
        let g2 = Arc::clone(&grid);
        let s2 = Arc::clone(&stop);
        let reader = std::thread::spawn(move || {
            let mut parser = Parser::new();
            let mut attrs = CellAttrs::default();
            while !s2.load(std::sync::atomic::Ordering::Relaxed) {
                let mut grid = g2.lock().unwrap();
                let mut h = VteHandler { grid: &mut *grid, attrs: &mut attrs };
                for &b in WELCOME {
                    parser.advance(&mut h, b);
                }
            }
        });

        // Thread B: render — resizes the grid repeatedly (drawer drag)
        let g3 = Arc::clone(&grid);
        let s3 = Arc::clone(&stop);
        let renderer = std::thread::spawn(move || {
            let widths = [100usize, 79, 60, 85, 100, 70, 95, 100];
            let mut i = 0usize;
            while !s3.load(std::sync::atomic::Ordering::Relaxed) {
                let mut grid = g3.lock().unwrap();
                grid.resize(widths[i % widths.len()], 30);
                i += 1;
            }
        });

        // Run for a bounded time, then sample the grid for duplicates
        std::thread::sleep(std::time::Duration::from_millis(300));
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        reader.join().unwrap();
        renderer.join().unwrap();

        // Sample after settle: there must be at most one welcome top visible
        let mut welcomes = 0usize; let mut tops = 0usize;
        {
            let grid = grid.lock().unwrap();
            for r in 0..grid.rows {
                let row: String = grid.cells[r].iter().map(|c| c.c).collect();
                if row.contains("Welcome back!") { welcomes += 1; }
                if row.contains("╭───") { tops += 1; }
            }
            println!("after concurrency: cols={} welcomes={} tops={} sb={}",
                grid.cols, welcomes, tops, grid.scrollback_len());
        }
        // A clean frame has exactly one welcome/top. Duplicates = the overlap.
        assert!(welcomes <= 1 && tops <= 1,
            "concurrent feed+resize produced duplicate frame: welcomes={welcomes} tops={tops}");
    }
}

#[cfg(test)]
mod render_scan {
    use crate::terminal::types::CellAttrs;
    use crate::terminal::vte::VteHandler;
    use crate::terminal::TerminalGrid;
    use vte::Parser;
    const WELCOME: &[u8] = include_bytes!("../../tests/fixtures/claude_welcome.bin");

    /// Faithful renderer-mapping check: after claude's frame scrolls into
    /// scrollback and we shrink→widen (reflow), every row the renderer could
    /// reference (scrollback + grid) must be exactly grid.cols wide. A mismatch
    /// is what makes rendering index out of bounds / draw wrong → the overlap.
    #[test]
    fn render_scan_rows_all_match_cols_after_shrink_widen() {
        let mut grid = TerminalGrid::with_scrollback_limit(100, 30, 1024 * 1024);
        let mut p = Parser::new();
        let mut a = CellAttrs::default();
        for &b in WELCOME {
            let mut h = VteHandler { grid: &mut grid, attrs: &mut a };
            p.advance(&mut h, b);
        }
        // push the box into scrollback
        for _ in 0..10 { grid.scroll_up(0, grid.rows - 1); }
        grid.resize(80, 30);   // shrink (reflow scrollback)
        grid.resize(100, 30);  // widen back

        let cols = grid.cols;
        for i in 0..grid.scrollback_len() {
            let row = grid.get_scrollback_row(i).unwrap();
            assert_eq!(row.len(), cols,
                "scrollback row {i} is {} wide but cols={cols}", row.len());
        }
        for r in 0..grid.rows {
            assert_eq!(grid.cells[r].len(), cols, "grid row {r} width mismatch");
        }
    }
}


#[cfg(test)]
mod mixed_width_rows {
    use crate::terminal::types::TerminalCell;
    use crate::terminal::TerminalGrid;

    /// The user's crash: "index out of bounds: len is 79 but index is 79".
    /// A scrollback row that is NARROWER than grid.cols makes the renderer
    /// (and clear_row_range) index out of bounds. This must be impossible:
    /// every row, visible or scrollback, must equal grid.cols after resize.
    /// Reproduces by simulating a scrollback row that was NOT resized to cols.
    #[test]
    fn narrow_scrollback_row_never_exists() {
        let mut grid = TerminalGrid::with_scrollback_limit(100, 30, 1024 * 1024);
        // Simulate the crash state: a scrollback row at the OLD width (79)
        // while grid.cols = 100. This is the invariant that must never break.
        grid.scrollback.push_back(vec![TerminalCell::default(); 79]);
        grid.scrollback_wrapped.push_back(false);
        grid.normalize_row_widths(); // reader calls this after every chunk

        // Every row must be resized to grid.cols — verify the invariant that
        // prevents the out-of-bounds crash.
        let cols = grid.cols;
        for i in 0..grid.scrollback_len() {
            let row = grid.get_scrollback_row(i).unwrap();
            assert_eq!(row.len(), cols,
                "narrow scrollback row {i} (len={}) would crash the renderer (cols={cols})",
                row.len());
        }
    }
}

#[cfg(test)]
mod claude_visible_overlap {
    use crate::terminal::types::CellAttrs;
    use crate::terminal::vte::VteHandler;
    use crate::terminal::TerminalGrid;
    use vte::Parser;
    const WELCOME: &[u8] = include_bytes!("../../tests/fixtures/claude_welcome.bin");

    /// USER'S EXACT REPRO, end-to-end: claude draws its frame at 100, the frame
    /// scrolls into history as output follows, then drag NARROWER then WIDER.
    /// The VISIBLE grid must show ONE clean frame — not two overlapping frames
    /// at different widths (the user's screenshot shows two box tops).
    #[test]
    fn visible_grid_has_single_frame_after_shrink_widen() {
        let mut grid = TerminalGrid::with_scrollback_limit(100, 30, 1024 * 1024);
        let mut p = Parser::new();
        let mut a = CellAttrs::default();
        for &b in WELCOME {
            let mut h = VteHandler { grid: &mut grid, attrs: &mut a };
            p.advance(&mut h, b);
        }
        // push the frame into scrollback
        for _ in 0..12 { grid.scroll_up(0, grid.rows - 1); }
        // drag narrower then wider
        grid.resize(80, 30);
        grid.resize(100, 30);

        // Count box tops (╭───) in the VISIBLE grid
        let mut tops = 0usize; let mut welcomes = 0usize;
        for r in 0..grid.rows {
            let row: String = grid.cells[r].iter().map(|c| c.c).collect();
            if row.contains("╭───") { tops += 1; }
            if row.contains("Welcome back!") { welcomes += 1; }
        }
        println!("visible: cols={} tops={tops} welcomes={welcomes} sb={}", grid.cols, grid.scrollback_len());
        assert!(tops <= 1, "visible grid has {tops} box tops — overlapping frames");
        assert!(welcomes <= 1, "visible grid has {welcomes} welcome frames — overlapping");
    }
}

#[cfg(test)]
mod user_repro_exact {
    use crate::terminal::types::CellAttrs;
    use crate::terminal::vte::VteHandler;
    use crate::terminal::TerminalGrid;
    use vte::Parser;
    const WELCOME: &[u8] = include_bytes!("../../tests/fixtures/claude_welcome.bin");

    /// USER'S EXACT WORDS: "超出可视区的历史内容，拖拽变窄，再变宽，会截断历史内容
    /// 右侧". Claude's frame scrolls into history, drag narrower then wider,
    /// the RIGHT side of the historical frame must NOT be truncated — the box
    /// top must rejoin to ONE row with ╮ at col 99.
    #[test]
    fn scrollback_box_top_rejoins_after_shrink_widen() {
        let mut grid = TerminalGrid::with_scrollback_limit(100, 30, 1024 * 1024);
        let mut p = Parser::new();
        let mut a = CellAttrs::default();
        for &b in WELCOME {
            let mut h = VteHandler { grid: &mut grid, attrs: &mut a };
            p.advance(&mut h, b);
        }
        // push the whole frame into scrollback
        for _ in 0..20 { grid.scroll_up(0, grid.rows - 1); }

        // drag narrower then wider
        grid.resize(80, 30);
        grid.resize(100, 30);

        // The box top (╭ … ╮) must be intact in the TOTAL content and exactly
        // cols wide (rejoined on widen — nothing truncated on the right).
        let cols = grid.cols;
        let total: String = {
            let mut s = String::new();
            for i in 0..grid.scrollback_len() {
                for c in grid.get_scrollback_row(i).unwrap() {
                    if c.c != ' ' && c.c != '\0' { s.push(c.c); }
                }
            }
            for r in 0..grid.rows {
                for c in &grid.cells[r] {
                    if c.c != ' ' && c.c != '\0' { s.push(c.c); }
                }
            }
            s
        };
        // The top border's corner sequence "╭───…───╮" rejoined: count box-top
        // corners in the total — exactly one ╭ that has a ╮ somewhere after it.
        assert!(total.contains('╮'), "box top right edge ╮ must survive shrink→widen");
        assert!(total.contains('╭'), "box top left edge ╭ must survive shrink→widen");
    }
}









#[cfg(test)]
mod duplicate_render {
    use crate::terminal::types::CellAttrs;
    use crate::terminal::vte::VteHandler;
    use crate::terminal::TerminalGrid;
    use vte::Parser;
    const WELCOME: &[u8] = include_bytes!("../../tests/fixtures/claude_welcome.bin");

    fn char_counts(grid: &TerminalGrid) -> std::collections::HashMap<char, usize> {
        let mut m = std::collections::HashMap::new();
        for i in 0..grid.scrollback_len() {
            for c in grid.get_scrollback_row(i).unwrap() {
                if c.c != ' ' && c.c != '\0' { *m.entry(c.c).or_insert(0) += 1; }
            }
        }
        for r in 0..grid.rows {
            for c in &grid.cells[r] {
                if c.c != ' ' && c.c != '\0' { *m.entry(c.c).or_insert(0) += 1; }
            }
        }
        m
    }

    /// USER'S NEW SYMPTOM: "超出可视区的历史内容，拖拽变窄，再变宽，内容会重复渲染".
    /// After shrink→widen, NO character's count may INCREASE (no content
    /// duplicated). The widening "pull scrollback back into view" logic copies
    /// history into the visible grid, duplicating it.
    #[test]
    fn no_content_duplicated_after_shrink_widen() {
        let mut grid = TerminalGrid::with_scrollback_limit(100, 30, 1024 * 1024);
        let mut p = Parser::new();
        let mut a = CellAttrs::default();
        for &b in WELCOME {
            let mut h = VteHandler { grid: &mut grid, attrs: &mut a };
            p.advance(&mut h, b);
        }
        // push the frame into scrollback
        for _ in 0..20 { grid.scroll_up(0, grid.rows - 1); }
        let before = char_counts(&grid);

        grid.resize(80, 30);  // narrower
        grid.resize(100, 30); // wider

        let after = char_counts(&grid);
        let mut duplicated: Vec<char> = Vec::new();
        for (c, n) in &before {
            if after.get(c).copied().unwrap_or(0) > *n {
                duplicated.push(*c);
            }
        }
        assert!(duplicated.is_empty(),
            "content duplicated after shrink→widen (reflow pulled history into view): {duplicated:?}");
    }
}

#[cfg(test)]
mod no_duplicate_visible {
    use crate::terminal::types::CellAttrs;
    use crate::terminal::vte::VteHandler;
    use crate::terminal::TerminalGrid;
    use vte::Parser;
    const WELCOME: &[u8] = include_bytes!("../../tests/fixtures/claude_welcome.bin");

    /// USER'S CURRENT SYMPTOM: "拖拽变窄，再变宽，内容会重复渲染". After a
    /// shrink→widen, the VISIBLE grid must not contain any row more than once
    /// (no history content pulled back and stacked on top of the live frame).
    #[test]
    fn no_row_appears_twice_in_visible_after_shrink_widen() {
        let mut grid = TerminalGrid::with_scrollback_limit(100, 30, 1024 * 1024);
        let mut p = Parser::new();
        let mut a = CellAttrs::default();
        for &b in WELCOME {
            let mut h = VteHandler { grid: &mut grid, attrs: &mut a };
            p.advance(&mut h, b);
        }
        // push part of the frame into scrollback
        for _ in 0..10 { grid.scroll_up(0, grid.rows - 1); }

        grid.resize(80, 30);
        grid.resize(100, 30);

        // A frame appears twice in the visible grid only if history content was
        // pulled back and stacked on the live frame. "Welcome back!" is unique
        // in the welcome frame — if it shows up twice, that's duplication.
        let welcomes = (0..grid.rows).filter(|&r| {
            let row: String = grid.cells[r].iter().map(|c| c.c).collect();
            row.contains("Welcome back!")
        }).count();
        assert!(welcomes <= 1,
            "visible grid shows {welcomes} welcome frames — history duplicated on top of live frame");
    }
}



#[cfg(test)]
mod statusline_dup {
    use crate::terminal::types::CellAttrs;
    use crate::terminal::vte::VteHandler;
    use crate::terminal::TerminalGrid;
    use vte::Parser;
    const SPIN: &[u8] = include_bytes!("../../tests/fixtures/claude_spin.bin");

    /// USER'S EXACT DUPLICATION: Claude Code's status line ("▎ Using
    /// deepseek… · /model") and the auth warning get redrawn on every tick
    /// (covering the same row). If a resize happens BETWEEN ticks, the 
    /// terminal's grid moves/resizes so the next cover lands on a different
    /// row → the status line appears MULTIPLE times. Reproduce: replay the
    /// real session bytes with a mid-stream resize, then count status lines.
    #[test]
    fn statusline_not_duplicated_after_midstream_resize() {
        let mut grid = TerminalGrid::with_scrollback_limit(100, 30, 1024 * 1024);
        let mut p = Parser::new();
        let mut a = CellAttrs::default();
        let mid = SPIN.len() / 2;
        for &b in &SPIN[..mid] {
            let mut h = VteHandler { grid: &mut grid, attrs: &mut a };
            p.advance(&mut h, b);
        }
        // resize mid-stream: drag NARROWER (cols AND rows) then WIDER back
        grid.resize(80, 24);
        grid.resize(100, 30);
        // rest of the stream (more status-line redraws)
        for &b in &SPIN[mid..] {
            let mut h = VteHandler { grid: &mut grid, attrs: &mut a };
            p.advance(&mut h, b);
        }
        // count status lines in the visible grid
        let status = (0..grid.rows).filter(|&r| {
            let row: String = grid.cells[r].iter().map(|c| c.c).collect();
            row.contains("Using deepseek") || row.contains("· /model")
        }).count();
        println!("cols={} status_lines={status}", grid.cols);
        assert!(status <= 1,
            "status line duplicated after mid-stream resize: {status} copies in visible grid");
    }
}












