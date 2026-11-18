//! Platform fonts — Windows: Consolas monospace + system CJK fallbacks.

use eframe::egui;

use super::{install_cjk_fallback, push_font};

/// Register Windows fonts: Consolas as the stock terminal monospace, then
/// the first readable system CJK font as the fallback for both families.
pub fn install_platform_fonts(fonts: &mut egui::FontDefinitions) {
    // Consolas: the stock terminal monospace, always present.
    let system_root =
        std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    push_font(
        fonts,
        "Consolas",
        &format!("{}\\Fonts\\consola.ttf", system_root),
        &[egui::FontFamily::Monospace],
    );

    // 微软雅黑 ships with every Windows 10/11; the others cover stripped
    //-down installs and add Korean/Japanese coverage.
    let cjk_paths = [
        format!("{system_root}\\Fonts\\msyh.ttc"),
        format!("{system_root}\\Fonts\\msyhbd.ttc"),
        format!("{system_root}\\Fonts\\simhei.ttf"),
        format!("{system_root}\\Fonts\\malgun.ttf"),
        format!("{system_root}\\Fonts\\meiryo.ttc"),
    ];
    install_cjk_fallback(fonts, &cjk_paths);
}
