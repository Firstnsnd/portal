//! # Terminal background image
//!
//! User-configurable image painted behind the terminal grid. The decoded
//! texture travels down the pane-render call chain as an owned
//! [`TerminalBackground`] (`TextureHandle` clones are cheap — passing it owned
//! avoids borrowing conflicts at the `render_pane_tree` call site).
//!
//! The image is viewport-fixed (scrolling never moves it) and painted with a
//! premultiplied-alpha tint so it fades toward the pane's solid fill while
//! ANSI-colored cell backgrounds stay opaque on top of it.

use eframe::egui;

/// Prepared background-image layer passed down the pane-render call chain.
#[derive(Clone)]
pub struct TerminalBackground {
    /// Decoded texture; `None` = disabled or last load attempt failed.
    pub texture: Option<egui::TextureHandle>,
    /// Blend strength, 0.0–1.0 (applied via `gamma_multiply` at paint time).
    pub opacity: f32,
}

/// Center-crop UV so the image fills `rect_size` (CSS `cover`): aspect ratio
/// preserved, overflow cropped. Degenerate sizes fall back to the full 0..1
/// UV rect (painter clamps anyway).
pub(crate) fn cover_uv(tex: egui::Vec2, rect_size: egui::Vec2) -> egui::Rect {
    if tex.x <= 0.0 || tex.y <= 0.0 || rect_size.x <= 0.0 || rect_size.y <= 0.0 {
        return egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    }
    let tex_ar = tex.x / tex.y;
    let rect_ar = rect_size.x / rect_size.y;
    if rect_ar > tex_ar {
        // Rect is relatively wider: fill width, crop top/bottom.
        let visible = (tex_ar / rect_ar).min(1.0);
        let y0 = (1.0 - visible) / 2.0;
        egui::Rect::from_min_max(egui::pos2(0.0, y0), egui::pos2(1.0, y0 + visible))
    } else {
        // Rect is relatively taller (or equal): fill height, crop sides.
        let visible = (rect_ar / tex_ar).min(1.0);
        let x0 = (1.0 - visible) / 2.0;
        egui::Rect::from_min_max(egui::pos2(x0, 0.0), egui::pos2(x0 + visible, 1.0))
    }
}

/// Decode an image file into an egui texture payload. The format is sniffed
/// from the file content (never trusted from the extension). Animated images
/// (GIF) decode to their first frame. Oversized images are downsampled so the
/// GPU texture stays within safe limits.
pub(crate) fn load_background_image(path: &str) -> Result<egui::ColorImage, String> {
    let img = image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?;
    // 4096 px per side keeps the texture well inside wgpu device limits;
    // larger source files are silently downsampled.
    let img = if img.width().max(img.height()) > 4096 {
        img.thumbnail(4096, 4096)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uv(x0: f32, y0: f32, x1: f32, y1: f32) -> egui::Rect {
        egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1))
    }

    #[test]
    fn wide_image_in_square_rect_crops_sides() {
        // 2:1 texture in a 1:1 pane: keep the middle half horizontally.
        assert_eq!(
            cover_uv(egui::vec2(200.0, 100.0), egui::vec2(100.0, 100.0)),
            uv(0.25, 0.0, 0.75, 1.0)
        );
    }

    #[test]
    fn tall_image_in_square_rect_crops_top_bottom() {
        // 1:2 texture in a 1:1 pane: keep the middle half vertically.
        assert_eq!(
            cover_uv(egui::vec2(100.0, 200.0), egui::vec2(100.0, 100.0)),
            uv(0.0, 0.25, 1.0, 0.75)
        );
    }

    #[test]
    fn matching_aspect_uses_full_uv() {
        assert_eq!(
            cover_uv(egui::vec2(800.0, 600.0), egui::vec2(400.0, 300.0)),
            uv(0.0, 0.0, 1.0, 1.0)
        );
        assert_eq!(
            cover_uv(egui::vec2(100.0, 100.0), egui::vec2(500.0, 500.0)),
            uv(0.0, 0.0, 1.0, 1.0)
        );
    }

    #[test]
    fn wide_pane_with_wide_image_fills_width() {
        // 16:9 texture in a 21:9 pane: fill width, crop some height.
        let got = cover_uv(egui::vec2(1920.0, 1080.0), egui::vec2(2100.0, 900.0));
        let visible = ((1920.0f32 / 1080.0) / (2100.0 / 900.0)).min(1.0);
        let y0 = (1.0 - visible) / 2.0;
        assert_eq!(got, uv(0.0, y0, 1.0, y0 + visible));
    }

    #[test]
    fn degenerate_sizes_fall_back_to_full_uv() {
        let full = uv(0.0, 0.0, 1.0, 1.0);
        assert_eq!(cover_uv(egui::Vec2::ZERO, egui::vec2(100.0, 100.0)), full);
        assert_eq!(cover_uv(egui::vec2(100.0, 100.0), egui::Vec2::ZERO), full);
        assert_eq!(cover_uv(egui::vec2(-1.0, 100.0), egui::vec2(10.0, 10.0)), full);
    }

    #[test]
    fn decodes_png_from_content_not_extension() {
        // Real 2x2 PNG written with a bogus extension: the decoder must still
        // sniff the format from the bytes.
        let img = image::RgbaImage::from_raw(2, 2, [10u8, 20, 30, 255].repeat(4)).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("not-really.dat");
        img.write_to(
            &mut std::io::BufWriter::new(std::fs::File::create(&path).unwrap()),
            image::ImageFormat::Png,
        )
        .unwrap();
        let decoded =
            load_background_image(path.to_str().unwrap()).expect("decode should succeed");
        assert_eq!(decoded.size, [2, 2]);
    }

    #[test]
    fn missing_file_is_an_error_not_a_panic() {
        assert!(load_background_image("/nonexistent/bg.png").is_err());
    }
}
