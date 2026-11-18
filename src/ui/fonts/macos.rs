//! Platform fonts — macOS: Monaco monospace + system CJK fallbacks.

use eframe::egui;

use super::{install_cjk_fallback, push_font};

/// Register macOS fonts: Monaco as the stock monospace, then the first
/// readable system CJK font as the fallback for both families.
pub fn install_platform_fonts(fonts: &mut egui::FontDefinitions) {
    // Monaco font for macOS
    push_font(
        fonts,
        "Monaco",
        "/System/Library/Fonts/Monaco.dfont",
        &[egui::FontFamily::Monospace],
    );

    let cjk_paths = [
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        "/System/Library/Fonts/STHeiti Medium.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
    ]
    .iter()
    .map(|p| p.to_string())
    .collect::<Vec<_>>();
    install_cjk_fallback(fonts, &cjk_paths);
}
