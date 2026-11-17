//! # Font loading utilities
//!
//! This module provides shared font loading functionality used during
//! app initialization and when font settings change.

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

    #[cfg(target_os = "macos")]
    {
        // Monaco font for macOS
        push_font(&mut fonts, "Monaco", "/System/Library/Fonts/Monaco.dfont", &[egui::FontFamily::Monospace]);

        let cjk_paths = [
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
            "/System/Library/Fonts/STHeiti Medium.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
        ]
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>();
        install_cjk_fallback(&mut fonts, &cjk_paths);
    }

    #[cfg(windows)]
    {
        // Consolas: the stock terminal monospace, always present.
        push_font(
            &mut fonts,
            "Consolas",
            &format!("{}\\Fonts\\consola.ttf", std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string())),
            &[egui::FontFamily::Monospace],
        );

        // 微软雅黑 ships with every Windows 10/11; the others cover stripped
        //-down installs and add Korean/Japanese coverage.
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
        let cjk_paths = [
            format!("{system_root}\\Fonts\\msyh.ttc"),
            format!("{system_root}\\Fonts\\msyhbd.ttc"),
            format!("{system_root}\\Fonts\\simhei.ttf"),
            format!("{system_root}\\Fonts\\malgun.ttf"),
            format!("{system_root}\\Fonts\\meiryo.ttc"),
        ];
        install_cjk_fallback(&mut fonts, &cjk_paths);
    }

    #[cfg(target_os = "linux")]
    {
        // DejaVu Sans Mono is the near-universal Linux monospace.
        push_font(&mut fonts, "DejaVuSansMono", "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf", &[egui::FontFamily::Monospace]);

        // Path probes across the major distro layouts: Debian/Ubuntu
        // (fonts-noto-cjk, fonts-wqy-microhei), Arch, and Fedora.
        let cjk_paths = [
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-sans-cjk-fonts/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
            "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
        ]
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>();
        install_cjk_fallback(&mut fonts, &cjk_paths);
    }

    fonts
}
