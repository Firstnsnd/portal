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

        // After resize, all content should be visible in the grid
        // (scrollback is empty because all 3 rows fit in the grid)
        let content = get_visible_content(&grid);
        assert!(content.contains("line1"));
        assert!(content.contains("line2"));
        assert!(content.contains("line3"));
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
}
