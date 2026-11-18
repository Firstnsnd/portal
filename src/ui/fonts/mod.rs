//! # Font loading utilities
//!
//! This module provides shared font loading functionality used during
//! app initialization and when font settings change.
//!
//! The platform-specific font sets (stock monospace + CJK fallback paths)
//! live in per-OS sibling modules behind [`install_platform_fonts`]:

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "macos")]
pub use macos::install_platform_fonts;
#[cfg(target_os = "linux")]
pub use linux::install_platform_fonts;
#[cfg(windows)]
pub use windows::install_platform_fonts;

use eframe::egui;

/// Probe `path` and, if readable, register it under `name` as a fallback for
/// `families` (appended — existing fonts keep priority, this only fills gaps).
fn push_font(fonts: &mut egui::FontDefinitions, name: &str, path: &str, families: &[egui::FontFamily]) {
    if let Ok(font_data) = std::fs::read(path) {
        fonts.font_data.insert(name.to_owned(), egui::FontData::from_owned(font_data));
        for family in families {
            fonts
                .families
                .entry(family.clone())
                .or_insert_with(Vec::new)
                .push(name.to_owned());
        }
    }
}

/// Load the first readable path as the "CJK" fallback for both font families.
/// egui's bundled fonts contain no CJK glyphs, and it never reads system font
/// registries — without this, Chinese/Japanese/Korean renders as tofu boxes.
fn install_cjk_fallback(fonts: &mut egui::FontDefinitions, candidates: &[String]) {
    let families = [egui::FontFamily::Monospace, egui::FontFamily::Proportional];
    for path in candidates {
        if std::fs::metadata(path).is_ok() {
            push_font(fonts, "CJK", path, &families);
            return;
        }
    }
}

/// Load fonts based on the current configuration.
///
/// This function creates a `FontDefinitions` with:
/// - Default fonts
/// - Custom font if `custom_font_path` is provided
/// - A platform monospace font (Monaco/Consolas/DejaVu Sans Mono)
/// - CJK fallback fonts for Chinese/Japanese/Korean characters
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

    // Platform monospace + CJK fallback (per-OS sibling module).
    install_platform_fonts(&mut fonts);

    fonts
}
