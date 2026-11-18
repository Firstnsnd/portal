//! Platform fonts — Linux: DejaVu Sans Mono monospace + distro CJK fallbacks.

use eframe::egui;

use super::{install_cjk_fallback, push_font};

/// Register Linux fonts: DejaVu Sans Mono as the stock monospace, then the
/// first readable system CJK font as the fallback for both families.
pub fn install_platform_fonts(fonts: &mut egui::FontDefinitions) {
    // DejaVu Sans Mono is the near-universal Linux monospace.
    push_font(
        fonts,
        "DejaVuSansMono",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        &[egui::FontFamily::Monospace],
    );

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
    install_cjk_fallback(fonts, &cjk_paths);
}
