//! # SFTP Transfer Progress Bar
//!
//! This module contains the transfer progress bar rendering logic for the SFTP view.

use eframe::egui;
use crate::sftp::TransferProgress;
use crate::ui::theme::ThemeColors;
use crate::ui::i18n::Language;
use crate::ui::views::sftp::format::{format_transfer_speed, format_file_size};

/// Render the transfer progress bar.
///
/// Returns `true` if the user clicked the stop button (cancel request).
pub fn render_transfer_progress(
    ui: &mut egui::Ui,
    progress: &TransferProgress,
    available: egui::Rect,
    theme: &ThemeColors,
    language: &Language,
) -> bool {
    let label = if progress.is_upload {
        language.tf("uploading", &progress.filename)
    } else {
        language.tf("downloading", &progress.filename)
    };
    let pct = if progress.total_bytes > 0 {
        progress.bytes_transferred as f32 / progress.total_bytes as f32
    } else {
        0.0
    };

    // Format speed
    let speed = progress.speed_bps();
    let speed_str = format_transfer_speed(speed);
    // Format transferred / total
    let size_str = format!(
        "{} / {}",
        format_file_size(progress.bytes_transferred),
        format_file_size(progress.total_bytes),
    );

    let bar_h = 40.0;
    let bar_area = egui::Rect::from_min_size(
        egui::pos2(available.min.x, available.max.y - bar_h),
        egui::vec2(available.width(), bar_h),
    );
    ui.painter().rect_filled(bar_area, 0.0, theme.bg_elevated);

    let pad_x = 12.0;
    let stop_size = 20.0;
    let stop_right_pad = 10.0;
    let content_right = bar_area.max.x - stop_size - stop_right_pad * 2.0;

    // ── Row 1: label + info text + stop button ──
    let row1_cy = bar_area.min.y + bar_h * 0.30;

    // Label: "Uploading filename"
    let label_galley = ui.painter().layout_no_wrap(
        label.clone(),
        egui::FontId::proportional(12.0),
        theme.fg_primary,
    );
    let label_w = label_galley.size().x;
    ui.painter().galley(
        egui::pos2(bar_area.min.x + pad_x, row1_cy - label_galley.size().y / 2.0),
        label_galley,
        theme.fg_primary,
    );

    // Info text (right-aligned, left of stop button): %  speed  size  ETA
    let eta_str = progress.eta_string().unwrap_or_else(|| "--:--".to_string());
    let info_text = format!("{:.0}%   {}   {}   {}", pct * 100.0, speed_str, size_str, eta_str);
    let info_galley = ui.painter().layout_no_wrap(
        info_text,
        egui::FontId::proportional(11.0),
        theme.fg_dim,
    );
    let info_x = (content_right - info_galley.size().x).max(bar_area.min.x + pad_x + label_w + 12.0);
    ui.painter().galley(
        egui::pos2(info_x, row1_cy - info_galley.size().y / 2.0),
        info_galley,
        theme.fg_dim,
    );

    // Stop button (vertically centered in bar)
    let stop_rect = egui::Rect::from_min_size(
        egui::pos2(bar_area.max.x - stop_size - stop_right_pad, bar_area.center().y - stop_size / 2.0),
        egui::vec2(stop_size, stop_size),
    );
    let stop_resp = ui.allocate_rect(stop_rect, egui::Sense::click());
    let stop_color = if stop_resp.hovered() { theme.red } else { theme.fg_dim };
    ui.painter().text(
        stop_rect.center(),
        egui::Align2::CENTER_CENTER,
        "\u{25A0}",
        egui::FontId::proportional(14.0),
        stop_color,
    );
    let should_cancel = stop_resp.on_hover_text(language.t("stop_transfer")).clicked();

    // ── Row 2: full-width progress bar ──
    let row2_cy = bar_area.min.y + bar_h * 0.75;
    let pb_h = 6.0;
    let pb_rect = egui::Rect::from_min_size(
        egui::pos2(bar_area.min.x + pad_x, row2_cy - pb_h / 2.0),
        egui::vec2(content_right - bar_area.min.x - pad_x, pb_h),
    );
    ui.painter().rect_filled(pb_rect, 3.0, theme.border);
    let filled = egui::Rect::from_min_size(
        pb_rect.min,
        egui::vec2(pb_rect.width() * pct, pb_rect.height()),
    );
    ui.painter().rect_filled(filled, 3.0, theme.accent);

    should_cancel
}
