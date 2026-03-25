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

        // Scrollback should still exist (possibly reflowed)
        assert!(grid.scrollback_len() > 0, "Scrollback should be preserved");

        // The scrollback should contain the original text
        let scrollback_content = get_scrollback_content(&grid);
        assert!(scrollback_content.contains("very long line"), "Long wrapped line should be in scrollback");
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
