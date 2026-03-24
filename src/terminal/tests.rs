/// Unit tests for terminal grid functionality

#[cfg(test)]
mod tests {
    use crate::terminal::TerminalGrid;
    use crate::terminal::types::{TerminalCell, CellAttrs};

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
        for i in 0..10 {
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
}
