//! # Terminal Text Selection
//!
//! This module contains text selection logic for terminal emulation,
//! including word boundary detection and selection state management.

use crate::terminal;

/// Check if a Unicode scalar value is in a CJK block.
pub fn is_cjk(cp: u32) -> bool {
    matches!(cp,
        0x4E00..=0x9FFF   | // CJK Unified Ideographs
        0x3400..=0x4DBF   | // CJK Extension A
        0x20000..=0x2A6DF | // CJK Extension B
        0x2A700..=0x2B73F | // CJK Extension C
        0x2B740..=0x2B81F | // CJK Extension D
        0x2B820..=0x2CEAF | // CJK Extension E
        0xF900..=0xFAFF   | // CJK Compatibility Ideographs
        0x2F800..=0x2FA1F   // CJK Compatibility Supplement
    )
}

/// Find word boundaries at the given grid position.
///
/// Returns (start_col, end_col) for the word containing the cursor.
/// Handles ASCII words, CJK ideographs, and wide character continuation cells.
///
/// # Arguments
///
/// * `grid` - The terminal grid
/// * `row` - Row index
/// * `col` - Column index
///
/// # Behavior
///
/// - For ASCII words: extends to include adjacent alphanumeric characters and underscores
/// - For CJK: each ideograph is its own word unit (including continuation cells)
/// - For non-word characters: returns (col, col)
///
/// # Fix Notes
///
/// Previously, the right-walk loop had a bug where it would continue after
/// encountering a continuation cell, potentially over-extending the selection
/// into adjacent characters. The fix is to include the continuation cell and
/// immediately break, as continuation cells mark the right edge of wide characters.
pub fn find_word_boundaries(grid: &terminal::TerminalGrid, row: usize, col: usize) -> (usize, usize) {
    if row >= grid.rows {
        return (col, col);
    }
    let cells = &grid.cells[row];
    let cell = cells.get(col);

    // If we landed on a continuation cell, walk back to the owning main cell
    let actual_col = if let Some(c) = cell {
        if c.wide_continuation && col > 0 {
            let mut search = col - 1;
            while search > 0 && cells[search].wide_continuation {
                search -= 1;
            }
            search
        } else {
            col
        }
    } else {
        col
    };

    let ch = cells.get(actual_col).map(|c| c.c).unwrap_or(' ');

    let is_word_char = |c: char| -> bool {
        if c.is_ascii_alphanumeric() || c == '_' { return true; }
        is_cjk(c as u32)
    };

    if !is_word_char(ch) {
        return (col, col);
    }

    // CJK fast path: each ideograph is its own word unit
    if is_cjk(ch as u32) {
        let mut end = actual_col;
        if end + 1 < cells.len() && cells[end + 1].wide_continuation {
            end += 1;
        }
        return (actual_col, end);
    }

    // Walk left for ASCII word
    let mut start = actual_col;
    while start > 0 {
        let prev_idx = start - 1;
        let prev_cell = &cells[prev_idx];

        // A continuation cell means the character before it is a wide (CJK)
        // char — that is a word boundary for ASCII words.
        if prev_cell.wide_continuation {
            break;
        }

        let prev_cp = prev_cell.c as u32;
        if !is_word_char(prev_cell.c) || is_cjk(prev_cp) {
            break;
        }

        start = prev_idx;
    }

    // Walk right for ASCII word
    let mut end = actual_col;
    while end + 1 < cells.len().min(grid.cols) {
        let next_idx = end + 1;
        let next_cell = &cells[next_idx];

        // A continuation cell is the right half of a wide character.
        // Include it in `end` and then STOP — do not continue the loop,
        // as that would cause the selection to extend past the CJK character.
        if next_cell.wide_continuation {
            end = next_idx;
            break;
        }

        let next_cp = next_cell.c as u32;
        if !is_word_char(next_cell.c) || is_cjk(next_cp) {
            break;
        }

        end = next_idx;

        // If this char is itself wide, absorb its continuation and stop
        if end + 1 < cells.len() && cells[end + 1].wide_continuation {
            end += 1;
            break;
        }
    }

    (start, end)
}
