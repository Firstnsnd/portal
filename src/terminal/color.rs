//! # Terminal Color Handling
//!
//! This module contains color-related functionality for terminal emulation,
//! including ANSI color palettes and color conversion utilities.

/// Default foreground color (Tokyo Night theme)
pub const DEFAULT_FG: (u8, u8, u8) = (220, 228, 255);

/// Default background color (Tokyo Night theme)
pub const DEFAULT_BG: (u8, u8, u8) = (26, 27, 38);

/// Standard ANSI 16-color palette
pub const ANSI_COLORS: [(u8, u8, u8); 16] = [
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

/// Convert a 256-color index to RGB tuple.
///
/// Supports three color ranges:
/// - 0-15: Standard ANSI colors
/// - 16-231: 6x6x6 color cube
/// - 232-255: Grayscale ramp
pub fn color_256_to_rgb(idx: u16) -> (u8, u8, u8) {
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

/// Parse extended color from SGR parameters (38 or 48).
///
/// Handles both subparam form (38:5:n or 38:2:r:g:b) and
/// next-params form (38 ; 5 ; n or 38 ; 2 ; r ; g ; b).
///
/// Returns the RGB color tuple, or the default if parsing fails.
pub fn parse_extended_color<'b, I>(param: &[u16], iter: &mut I, default: (u8, u8, u8)) -> (u8, u8, u8)
where
    I: Iterator<Item = &'b [u16]>,
{
    // Check subparam form first: 38:5:n or 38:2:r:g:b
    if param.len() >= 3 && param[1] == 5 {
        // 256-color mode: 38:5:n
        return color_256_to_rgb(param[2]);
    }
    if param.len() >= 5 && param[1] == 2 {
        // RGB mode: 38:2:r:g:b
        return (param[2] as u8, param[3] as u8, param[4] as u8);
    }

    // Try next-params form: 38 ; 5 ; n or 38 ; 2 ; r ; g ; b
    if let Some(mode) = iter.next() {
        let mode_val = mode.first().copied().unwrap_or(0);
        if mode_val == 5 {
            // 256-color mode
            if let Some(color) = iter.next() {
                return color_256_to_rgb(color.first().copied().unwrap_or(0));
            }
        } else if mode_val == 2 {
            // RGB mode
            let r = iter.next().and_then(|p| p.first().copied()).unwrap_or(0) as u8;
            let g = iter.next().and_then(|p| p.first().copied()).unwrap_or(0) as u8;
            let b = iter.next().and_then(|p| p.first().copied()).unwrap_or(0) as u8;
            return (r, g, b);
        }
    }

    default
}
