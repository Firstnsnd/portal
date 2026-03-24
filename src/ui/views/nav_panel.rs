//! # Navigation Panel
//!
//! Left navigation strip for switching between application views.

use eframe::egui;

use crate::ui::types::dialogs::AppView;
use crate::ui::theme::ThemeColors;
use crate::ui::i18n::Language;

/// Render the left navigation panel.
///
/// Returns the view that was clicked (if any).
///
/// # Arguments
///
/// * `ctx` - The egui context
/// * `current_view` - The currently active view
/// * `theme` - The theme colors
/// * `language` - The language settings
/// * `id` - Optional unique ID (for detached windows)
pub fn show_nav_panel(
    ctx: &egui::Context,
    current_view: AppView,
    theme: &ThemeColors,
    language: &Language,
    id: Option<egui::Id>,
) -> Option<AppView> {
    let nav_width = (ctx.screen_rect().width() * 0.14).min(200.0).max(150.0);
    let mut clicked_view = None;

    let panel_id = id.unwrap_or_else(|| egui::Id::new("nav"));
    egui::SidePanel::left(panel_id)
        .exact_width(nav_width)
        .resizable(false)
        .frame(egui::Frame {
            fill: theme.bg_secondary,
            inner_margin: egui::Margin::same(0.0),
            stroke: egui::Stroke::NONE,
            ..Default::default()
        })
        .show(ctx, |ui| {
            ui.add_space(32.0);

            if nav_button(ui, "☰", language.t("hosts"), current_view == AppView::Hosts, theme) {
                clicked_view = Some(AppView::Hosts);
            }
            if nav_button(ui, ">_", language.t("terminal"), current_view == AppView::Terminal, theme) {
                clicked_view = Some(AppView::Terminal);
            }
            if nav_button(ui, "\u{2195}", language.t("sftp"), current_view == AppView::Sftp, theme) {
                clicked_view = Some(AppView::Sftp);
            }
            if nav_button(ui, "\u{1f511}", language.t("keychain"), current_view == AppView::Keychain, theme) {
                clicked_view = Some(AppView::Keychain);
            }
            if nav_button(ui, "\u{2318}", language.t("snippets"), current_view == AppView::Snippets, theme) {
                clicked_view = Some(AppView::Snippets);
            }
            if nav_button(ui, "\u{1f310}", language.t("tunnels"), current_view == AppView::Tunnels, theme) {
                clicked_view = Some(AppView::Tunnels);
            }

            // Settings button at bottom - fill remaining space to reach window bottom
            let available_size = ui.available_size();
            egui::Frame::none()
                .fill(theme.bg_secondary)
                .show(ui, |ui| {
                    ui.allocate_ui_with_layout(
                        available_size,
                        egui::Layout::bottom_up(egui::Align::LEFT),
                        |ui| {
                            ui.add_space(8.0);
                            if nav_button(ui, "⚙", language.t("settings"), current_view == AppView::Settings, theme) {
                                clicked_view = Some(AppView::Settings);
                            }
                        },
                    );
                });
        });

    clicked_view
}

/// Navigation button for the left panel.
fn nav_button(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    is_active: bool,
    theme: &ThemeColors,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 36.0),
        egui::Sense::click(),
    );

    // Background for active state
    if is_active {
        ui.painter().rect_filled(
            egui::Rect::from_min_size(rect.min, egui::vec2(4.0, 36.0)),
            0.0,
            theme.accent,
        );
    }

    // Hover effect
    if response.hovered() {
        ui.painter().rect_filled(
            egui::Rect::from_min_size(rect.min, egui::vec2(ui.available_width(), 36.0)),
            0.0,
            theme.hover_bg,
        );
    }

    // Icon and label
    let icon_color = if is_active { theme.accent } else { theme.fg_dim };
    let label_color = if is_active { theme.fg_primary } else { theme.fg_dim };

    ui.painter().text(
        egui::pos2(rect.min.x + 16.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        icon,
        egui::FontId::proportional(14.0),
        icon_color,
    );

    ui.painter().text(
        egui::pos2(rect.min.x + 40.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.0),
        label_color,
    );

    response.clicked()
}
