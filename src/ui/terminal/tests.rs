//! # Word Selection Tests
//!
//! Tests for word boundary selection in terminal.
//! These tests ensure that double-click word selection works correctly
//! for both visible grid rows and scrollback rows.

mod tests {
    use crate::terminal::{TerminalGrid, TerminalCell, CellAttrs};
    use crate::ui::terminal::selection::{find_word_boundaries_in_row, find_word_boundaries};

    fn create_test_grid(cols: usize, rows: usize) -> TerminalGrid {
        TerminalGrid::with_scrollback_limit(cols, rows, 1024 * 1024)
    }

    fn write_string(grid: &mut TerminalGrid, s: &str) {
        for c in s.chars() {
            grid.write_char_with_attrs(c, &CellAttrs::default());
        }
    }

    /// Create a test cell row with the given string
    fn create_test_row(text: &str, cols: usize) -> Vec<TerminalCell> {
        let mut row = vec![TerminalCell::default(); cols];
        for (i, c) in text.chars().enumerate() {
            if i < cols {
                row[i] = TerminalCell {
                    c,
                    fg_color: (255, 255, 255),
                    bg_color: (0, 0, 0),
                    wide_continuation: false,
                    ..TerminalCell::default()
                };
            }
        }
        row
    }

    #[test]
    fn test_word_selection_ascii_word() {
        let row = create_test_row("hello world test", 20);
        let (start, end) = find_word_boundaries_in_row(&row, 20, 6); // Click on 'w' in "world"
        assert_eq!(start, 6);  // Start of "world"
        assert_eq!(end, 10);   // End of "world" (inclusive)
    }

    #[test]
    fn test_word_selection_with_underscore() {
        let row = create_test_row("my_variable_name", 20);
        let (start, end) = find_word_boundaries_in_row(&row, 20, 3); // Click on 'v'
        assert_eq!(start, 0);
        assert_eq!(end, 15); // Entire identifier including underscores (inclusive)
    }

    #[test]
    fn test_word_selection_space_returns_single_pos() {
        let row = create_test_row("hello world", 15);
        let (start, end) = find_word_boundaries_in_row(&row, 15, 5); // Click on space
        assert_eq!(start, 5);
        assert_eq!(end, 5);
    }

    #[test]
    fn test_word_selection_single_char() {
        let row = create_test_row("a b c", 10);
        let (start, end) = find_word_boundaries_in_row(&row, 10, 0); // Click on 'a'
        assert_eq!(start, 0);
        assert_eq!(end, 0); // Single char, inclusive
    }

    #[test]
    fn test_word_selection_cjk_single_character() {
        let row = create_test_row("中文测试", 10);
        let (start, end) = find_word_boundaries_in_row(&row, 10, 0);
        assert_eq!(start, 0);
        assert_eq!(end, 0); // CJK single char selects itself
    }

    #[test]
    fn test_word_selection_out_of_bounds() {
        let row = create_test_row("hello", 10);
        let (start, end) = find_word_boundaries_in_row(&row, 10, 50); // Click beyond row
        assert_eq!(start, 50);
        assert_eq!(end, 50);
    }

    #[test]
    fn test_word_selection_empty_row() {
        let row = vec![TerminalCell::default(); 10];
        let (start, end) = find_word_boundaries_in_row(&row, 10, 5);
        assert_eq!(start, 5);
        assert_eq!(end, 5);
    }

    #[test]
    fn test_word_selection_grid_row_integration() {
        let mut grid = create_test_grid(20, 5);
        write_string(&mut grid, "test_function(arg)");

        let (start, end) = find_word_boundaries(&grid, 0, 5);
        assert!(start <= 5);
        assert!(end >= 5);
    }

    #[test]
    fn test_word_selection_after_scrollback() {
        let mut grid = create_test_grid(20, 5);

        write_string(&mut grid, "scrollback_content");
        grid.cursor_row = 1;
        grid.cursor_col = 0;
        write_string(&mut grid, "visible_content");

        grid.scroll_up(0, 4);
        assert_eq!(grid.scrollback_len(), 1);

        // Test selection in scrollback
        if let Some(row) = grid.get_scrollback_row(0) {
            let (start, end) = find_word_boundaries_in_row(row, grid.cols, 9);
            assert!(start <= 9);
            assert!(end >= 9);
        }
    }

    #[test]
    fn test_word_selection_empty_grid() {
        let grid = create_test_grid(80, 24);
        let (start, end) = find_word_boundaries(&grid, 0, 10);
        assert_eq!(start, 10);
        assert_eq!(end, 10);
    }

    #[test]
    fn test_word_selection_row_out_of_bounds() {
        let grid = create_test_grid(80, 24);
        let (start, end) = find_word_boundaries(&grid, 100, 10);
        assert_eq!(start, 10);
        assert_eq!(end, 10);
    }

    #[test]
    fn test_word_selection_after_resize_narrow_to_wide() {
        let mut grid = create_test_grid(80, 24);
        write_string(&mut grid, "hello_world_test");

        // Narrow then wide
        grid.resize(40, 24);
        let (start1, end1) = find_word_boundaries(&grid, 0, 5);
        assert!(start1 <= 5);
        assert!(end1 >= 5);

        grid.resize(80, 24);
        let (start2, end2) = find_word_boundaries(&grid, 0, 5);
        assert!(start2 <= 5);
        assert!(end2 >= 5);
    }

    #[test]
    fn test_word_selection_with_wrapped_lines() {
        let mut grid = create_test_grid(10, 5);

        // Write content that fills exactly one row and triggers wrap_pending
        for c in "0123456789".chars() {
            grid.write_char_with_attrs(c, &CellAttrs::default());
        }
        // Cursor should be at last column with wrap_pending
        assert_eq!(grid.cursor_col, 9);
        assert!(grid.wrap_pending);

        // Write one more char to trigger actual wrap
        grid.write_char_with_attrs('a', &CellAttrs::default());
        assert_eq!(grid.cursor_row, 1);
        // First row should be marked as wrapped now
        grid.line_wrapped[0] = true;

        let (start, end) = find_word_boundaries(&grid, 0, 2);
        assert!(start <= 2);
        assert!(end >= 2);
    }

    #[test]
    fn test_word_selection_multiple_scrollback_rows() {
        let mut grid = create_test_grid(20, 5);

        for i in 0..3 {
            write_string(&mut grid, &format!("line{}", i));
            grid.cursor_row += 1;
            grid.cursor_col = 0;
        }

        for _ in 0..3 {
            grid.scroll_up(0, 4);
        }

        assert!(grid.scrollback_len() >= 2);

        for i in 0..grid.scrollback_len().min(3) {
            if let Some(row) = grid.get_scrollback_row(i) {
                let (start, end) = find_word_boundaries_in_row(row, grid.cols, 0);
                assert!(start <= 4); // "lineX"
                assert!(end >= start);
            }
        }
    }
}
