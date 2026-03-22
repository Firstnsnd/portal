use eframe::egui;
use super::theme::ThemeColors;

/// Primary action button (accent-colored fill, white text).
/// Used for "Save", "Create", and other main actions.
pub fn primary_button<'a>(text: &'a str, theme: &'a ThemeColors) -> egui::Button<'a> {
    egui::Button::new(
        egui::RichText::new(text)
            .color(egui::Color32::WHITE)
            .size(13.0),
    )
    .fill(theme.accent)
    .rounding(6.0)
    .min_size(egui::vec2(70.0, 32.0))
}

/// Secondary / cancel button (elevated bg, dim text).
/// Used for "Cancel" and other secondary actions.
pub fn secondary_button<'a>(text: &'a str, theme: &'a ThemeColors) -> egui::Button<'a> {
    egui::Button::new(
        egui::RichText::new(text)
            .color(theme.fg_dim)
            .size(13.0),
    )
    .fill(theme.bg_elevated)
    .rounding(6.0)
    .min_size(egui::vec2(70.0, 32.0))
}

/// Danger / destructive action button (red fill, white text).
/// Used for "Delete" confirmations.
pub fn danger_button<'a>(text: &'a str, theme: &'a ThemeColors) -> egui::Button<'a> {
    egui::Button::new(
        egui::RichText::new(text)
            .color(egui::Color32::WHITE)
            .size(13.0),
    )
    .fill(theme.red)
    .rounding(6.0)
    .min_size(egui::vec2(70.0, 32.0))
}

/// Text-only button with no background frame.
/// Used for inline actions like "New Host", "Delete All", navigation links.
pub fn text_button(text: &str, color: egui::Color32) -> egui::Button<'_> {
    egui::Button::new(
        egui::RichText::new(text)
            .color(color)
            .size(12.0),
    )
    .frame(false)
}

/// Standard dialog/modal frame with shadow.
/// Used for confirmation dialogs, rename dialogs, delete dialogs.
pub fn dialog_frame(theme: &ThemeColors) -> egui::Frame {
    egui::Frame {
        fill: theme.bg_secondary,
        rounding: egui::Rounding::same(10.0),
        inner_margin: egui::Margin::same(20.0),
        stroke: egui::Stroke::new(1.0, theme.border),
        shadow: egui::epaint::Shadow {
            offset: egui::vec2(0.0, 4.0),
            blur: 20.0,
            spread: 2.0,
            color: egui::Color32::from_black_alpha(80),
        },
        ..Default::default()
    }
}

/// Style a dropdown/combobox popup for consistent appearance.
/// Sets window fill, inactive/hovered bg, selection colors.
pub fn style_dropdown(ui: &mut egui::Ui, theme: &ThemeColors) {
    ui.style_mut().visuals.window_fill = ui.visuals().extreme_bg_color;
    ui.style_mut().visuals.widgets.inactive.bg_fill = theme.bg_elevated;
    ui.style_mut().visuals.widgets.hovered.bg_fill = theme.bg_primary;
    ui.style_mut().visuals.selection.bg_fill = theme.accent_alpha(30);
    ui.style_mut().visuals.selection.stroke = egui::Stroke::NONE;
}

/// Section header label (dim, small, strong).
/// Used for page titles like "Keychain", "Snippets", "Tunnels".
pub fn section_header(text: &str, theme: &ThemeColors) -> egui::RichText {
    egui::RichText::new(text)
        .color(theme.fg_dim)
        .size(12.0)
        .strong()
}

/// Form field label (dim, small).
/// Used for labels above text inputs in forms.
pub fn field_label(text: &str, theme: &ThemeColors) -> egui::RichText {
    egui::RichText::new(text)
        .color(theme.fg_dim)
        .size(12.0)
}
