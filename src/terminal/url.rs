//! URL detection in terminal cell rows

use super::types::TerminalCell;

/// URL scheme prefixes to detect
const URL_PREFIXES: &[&str] = &[
    "https://",
    "http://",
    "ftp://",
    "ssh://",
    "git://",
];

/// Characters that terminate a URL
fn is_url_char(c: char) -> bool {
    // Allow most printable chars except common delimiters that typically
    // surround URLs rather than being part of them
    c.is_alphanumeric()
        || matches!(
            c,
            '.' | '-' | '_' | '~' | ':' | '/' | '?' | '#' | '[' | ']' | '@'
                | '!' | '$' | '&' | '\'' | '(' | ')' | '*' | '+' | ',' | ';' | '=' | '%'
        )
}

/// Scan a single row of terminal cells for URLs.
/// Returns `(start_col, end_col, url_string)` tuples.
pub fn scan_row_for_urls(cells: &[TerminalCell], _cols: usize) -> Vec<(usize, usize, String)> {
    let mut results = Vec::new();
    let text_len = cells.len();

    let mut i = 0;
    while i < text_len {
        // Skip wide continuation cells
        if cells[i].wide_continuation {
            i += 1;
            continue;
        }

        // Try matching each URL prefix at position i
        let mut matched_prefix: Option<&str> = None;
        for prefix in URL_PREFIXES {
            let prefix_len = prefix.len();
            if i + prefix_len > text_len {
                continue;
            }
            let mut ok = true;
            for (j, pc) in prefix.chars().enumerate() {
                let cell = &cells[i + j];
                if cell.wide_continuation || cell.c != pc {
                    ok = false;
                    break;
                }
            }
            if ok {
                matched_prefix = Some(prefix);
                break;
            }
        }

        if let Some(_prefix) = matched_prefix {
            let start = i;
            // Consume all valid URL characters
            let mut end = i;
            while end < text_len {
                let cell = &cells[end];
                if cell.wide_continuation {
                    end += 1;
                    continue;
                }
                if !is_url_char(cell.c) {
                    break;
                }
                end += 1;
            }

            // Trim trailing characters that are valid URL chars but usually not
            // part of the actual URL (punctuation that wraps URLs)
            while end > start + 8 {
                // Get the last real char (skip continuation cells)
                let last_real = if end > 0 && cells[end - 1].wide_continuation {
                    end - 2
                } else {
                    end - 1
                };
                if last_real < start {
                    break;
                }
                match cells[last_real].c {
                    '.' | ',' | ';' | '!' | '?' | ':' | '\'' => {
                        end = last_real;
                    }
                    ')' => {
                        // Keep ) if there's a matching ( before
                        let url_text: String = cells[start..end]
                            .iter()
                            .filter(|c| !c.wide_continuation)
                            .map(|c| c.c)
                            .collect();
                        if url_text.contains('(') {
                            break; // keep the )
                        }
                        end = last_real;
                    }
                    _ => break,
                }
            }

            // Extract the URL string
            let url: String = cells[start..end]
                .iter()
                .filter(|c| !c.wide_continuation)
                .map(|c| c.c)
                .collect();

            if url.len() >= 8 {
                // Minimum: "https://" + at least 1 char
                results.push((start, end, url));
            }
            i = end;
        } else {
            i += 1;
        }
    }

    results
}
