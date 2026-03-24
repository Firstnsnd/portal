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
    ui.style_mut().visuals.window_fill = theme.menu_bg;
    ui.style_mut().visuals.widgets.inactive.bg_fill = egui::Color32::TRANSPARENT;
    ui.style_mut().visuals.widgets.hovered.bg_fill = theme.hover_bg;
    ui.style_mut().visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, theme.fg_primary);
    ui.style_mut().visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, theme.fg_primary);
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

/// Render a navigation sidebar button.
/// Used for navigation between main views (Hosts, Terminal, SFTP, etc.).
///
/// # Parameters
/// - `ui`: The UI context
/// - `icon`: The icon to display (e.g., "☰", ">_", "⇓")
/// - `label`: The text label for the button
/// - `active`: Whether this button is the currently active view
/// - `theme`: The theme colors
///
/// # Returns
/// - `true` if the button was clicked
pub fn nav_button(ui: &mut egui::Ui, icon: &str, label: &str, active: bool, theme: &ThemeColors) -> bool {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(width, 36.0),
        egui::Sense::click(),
    );

    let bg = if active {
        theme.accent_alpha(45)
    } else if resp.hovered() {
        theme.hover_bg
    } else {
        egui::Color32::TRANSPARENT
    };

    let shadow_color = if active {
        theme.accent_alpha(80)
    } else if resp.hovered() {
        theme.hover_shadow
    } else {
        egui::Color32::TRANSPARENT
    };

    // Draw shadow highlight at bottom
    ui.painter().rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(rect.min.x, rect.max.y - 1.0),
            rect.max,
        ),
        0.0,
        shadow_color,
    );

    // Draw main background
    ui.painter().rect_filled(rect, 0.0, bg);

    // Draw active indicator on left
    if active {
        ui.painter().rect_filled(
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + 3.0, rect.max.y)),
            egui::Rounding { nw: 0.0, ne: 2.0, sw: 0.0, se: 2.0 },
            theme.accent,
        );
    }

    // Draw icon and text
    let color = if active || resp.hovered() {
        theme.fg_primary
    } else {
        theme.fg_dim
    };
    ui.painter().text(
        egui::pos2(rect.min.x + 16.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        format!("{}  {}", icon, label),
        egui::FontId::proportional(13.0),
        color,
    );

    resp.clicked()
}
