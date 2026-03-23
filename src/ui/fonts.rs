//! # Font loading utilities
//!
//! This module provides shared font loading functionality used during
//! app initialization and when font settings change.

use eframe::egui;

/// Load fonts based on the current configuration.
///
/// This function creates a `FontDefinitions` with:
/// - Default fonts
/// - Custom font if `custom_font_path` is provided
/// - Monaco font on macOS
/// - CJK fallback fonts on macOS for Chinese/Japanese/Korean characters
pub fn load_fonts(custom_font_path: &str) -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();

    // Load custom font if specified
    if !custom_font_path.is_empty() {
        if let Ok(font_data) = std::fs::read(custom_font_path) {
            fonts.font_data.insert(
                "CustomFont".to_owned(),
                egui::FontData::from_owned(font_data),
            );
            fonts.families
                .entry(egui::FontFamily::Monospace)
                .or_insert_with(Vec::new)
                .insert(0, "CustomFont".to_owned());
        }
    }

    #[cfg(target_os = "macos")]
    {
        // Monaco font for macOS
        if let Ok(font_data) = std::fs::read("/System/Library/Fonts/Monaco.dfont") {
            fonts.font_data.insert(
                "Monaco".to_owned(),
                egui::FontData::from_owned(font_data),
            );
            fonts.families
                .entry(egui::FontFamily::Monospace)
                .or_insert_with(Vec::new)
                .push("Monaco".to_owned());
        }

        // CJK fallback font for Chinese/Japanese/Korean characters
        let cjk_paths = [
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
            "/System/Library/Fonts/STHeiti Medium.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
        ];
        for path in &cjk_paths {
            if let Ok(font_data) = std::fs::read(path) {
                fonts.font_data.insert(
                    "CJK".to_owned(),
                    egui::FontData::from_owned(font_data),
                );
                fonts.families
                    .entry(egui::FontFamily::Monospace)
                    .or_insert_with(Vec::new)
                    .push("CJK".to_owned());
                fonts.families
                    .entry(egui::FontFamily::Proportional)
                    .or_insert_with(Vec::new)
                    .push("CJK".to_owned());
                break;
            }
        }
    }

    fonts
}
