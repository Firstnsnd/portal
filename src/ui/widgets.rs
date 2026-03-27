use eframe::egui;
use super::theme::ThemeColors;

// ============================================================================
// Design Tokens for Forms
// ============================================================================

/// Drawer width (all drawers should use this)
pub const DRAWER_WIDTH: f32 = 380.0;

/// Form label width (fixed, for alignment)
pub const LABEL_WIDTH: f32 = 80.0;

/// Font sizes
pub const FONT_SIZE_LABEL: f32 = 12.0;
pub const FONT_SIZE_INPUT: f32 = 12.0;
pub const FONT_SIZE_TITLE: f32 = 14.0;

/// Spacing
pub const SPACING_FIELD: f32 = 8.0;       // Between fields
pub const SPACING_LABEL: f32 = 4.0;       // Between label and input
pub const SPACING_SECTION: f32 = 0.0;     // Between sections (before footer)
pub const SPACING_INLINE: f32 = 8.0;      // Between inline fields

/// Input dimensions
pub const INPUT_HEIGHT: f32 = 20.0;       // Standard input height (compact)
pub const INPUT_ROUNDING: f32 = 6.0;      // Standard border radius
pub const INPUT_PADDING_X: f32 = 8.0;
pub const INPUT_PADDING_Y: f32 = 3.0;

/// Form layout
pub const FORM_LEFT_MARGIN: f32 = 6.0;   // Left margin for form content

// ============================================================================
// Basic Buttons
// ============================================================================

/// Primary action button (accent-colored fill, white text).
pub fn primary_button<'a>(text: &'a str, theme: &'a ThemeColors) -> egui::Button<'a> {
    egui::Button::new(
        egui::RichText::new(text)
            .color(egui::Color32::WHITE)
            .size(13.0),
    )
    .fill(theme.accent)
    .rounding(6.0)
    .min_size(egui::vec2(80.0, 32.0))
}

/// Secondary / cancel button (elevated bg, dim text).
pub fn secondary_button<'a>(text: &'a str, theme: &'a ThemeColors) -> egui::Button<'a> {
    egui::Button::new(
        egui::RichText::new(text)
            .color(theme.fg_dim)
            .size(13.0),
    )
    .fill(theme.bg_elevated)
    .rounding(6.0)
    .min_size(egui::vec2(80.0, 32.0))
}

/// Danger / destructive action button (red fill, white text).
pub fn danger_button<'a>(text: &'a str, theme: &'a ThemeColors) -> egui::Button<'a> {
    egui::Button::new(
        egui::RichText::new(text)
            .color(egui::Color32::WHITE)
            .size(13.0),
    )
    .fill(theme.red)
    .rounding(6.0)
    .min_size(egui::vec2(80.0, 32.0))
}

/// Text-only button with no background frame.
pub fn text_button(text: &str, color: egui::Color32) -> egui::Button<'_> {
    egui::Button::new(
        egui::RichText::new(text)
            .color(color)
            .size(12.0),
    )
    .frame(false)
}

// ============================================================================
// Dialog Frame
// ============================================================================

/// Standard dialog/modal frame with shadow.
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
pub fn style_dropdown(ui: &mut egui::Ui, theme: &ThemeColors) {
    ui.style_mut().visuals.window_fill = theme.menu_bg;
    ui.style_mut().visuals.widgets.inactive.bg_fill = egui::Color32::TRANSPARENT;
    ui.style_mut().visuals.widgets.hovered.bg_fill = theme.hover_bg;
    ui.style_mut().visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, theme.fg_primary);
    ui.style_mut().visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, theme.fg_primary);
    ui.style_mut().visuals.selection.bg_fill = theme.accent_alpha(30);
    ui.style_mut().visuals.selection.stroke = egui::Stroke::NONE;
}

// ============================================================================
// Form Components - Professional Layout
// ============================================================================

/// Hint text style: italic with transparency
fn hint_text_style(text: &str, theme: &ThemeColors) -> egui::RichText {
    egui::RichText::new(text.to_string())
        .color(theme.hint_color())
        .size(FONT_SIZE_INPUT)
        .italics()
}

/// Form field label (fixed width for alignment)
pub fn form_label(ui: &mut egui::Ui, text: &str, required: bool, theme: &ThemeColors) {
    let label = egui::RichText::new(text)
        .color(theme.fg_dim)
        .size(FONT_SIZE_LABEL);

    ui.horizontal(|ui| {
        if required {
            ui.label(egui::RichText::new("*").color(theme.red).size(FONT_SIZE_LABEL));
        }
        ui.label(label);
    });
}

/// Standard text input field
pub fn text_input(
    ui: &mut egui::Ui,
    content: &mut String,
    hint: &str,
    theme: &ThemeColors,
) -> egui::Response {
    egui::Frame::none()
        .fill(theme.bg_secondary)
        .stroke(egui::Stroke::new(1.0, theme.input_border))
        .rounding(INPUT_ROUNDING)
        .inner_margin(egui::Margin::symmetric(INPUT_PADDING_X, INPUT_PADDING_Y))
        .show(ui, |ui| {
            let available_width = ui.available_width();
            ui.add_sized([available_width, INPUT_HEIGHT],
                egui::TextEdit::singleline(content)
                    .hint_text(hint_text_style(hint, theme))
                    .frame(false)
            )
        }).inner
}

/// Password input field
pub fn password_input(
    ui: &mut egui::Ui,
    content: &mut String,
    hint: &str,
    theme: &ThemeColors,
) -> egui::Response {
    egui::Frame::none()
        .fill(theme.bg_secondary)
        .stroke(egui::Stroke::new(1.0, theme.input_border))
        .rounding(INPUT_ROUNDING)
        .inner_margin(egui::Margin::symmetric(INPUT_PADDING_X, INPUT_PADDING_Y))
        .show(ui, |ui| {
            let available_width = ui.available_width();
            ui.add_sized([available_width, INPUT_HEIGHT],
                egui::TextEdit::singleline(content)
                    .password(true)
                    .hint_text(hint_text_style(hint, theme))
                    .frame(false)
            )
        }).inner
}

/// Multiline text area
pub fn text_area(
    ui: &mut egui::Ui,
    content: &mut String,
    hint: &str,
    height: f32,
    theme: &ThemeColors,
) {
    egui::Frame::none()
        .fill(theme.bg_secondary)
        .stroke(egui::Stroke::new(1.0, theme.input_border))
        .rounding(INPUT_ROUNDING)
        .inner_margin(egui::Margin::symmetric(INPUT_PADDING_X, INPUT_PADDING_Y))
        .show(ui, |ui| {
            ui.add_sized([ui.available_width(), height],
                egui::TextEdit::multiline(content)
                    .hint_text(hint_text_style(hint, theme))
                    .frame(false)
            );
        });
}

/// Fixed-width input for inline use
pub fn fixed_input(
    ui: &mut egui::Ui,
    content: &mut String,
    hint: &str,
    width: f32,
    theme: &ThemeColors,
) {
    egui::Frame::none()
        .fill(theme.bg_secondary)
        .stroke(egui::Stroke::new(1.0, theme.input_border))
        .rounding(INPUT_ROUNDING)
        .inner_margin(egui::Margin::symmetric(INPUT_PADDING_X, INPUT_PADDING_Y))
        .show(ui, |ui| {
            ui.add_sized([width, INPUT_HEIGHT],
                egui::TextEdit::singleline(content)
                    .hint_text(hint_text_style(hint, theme))
                    .frame(false)
            );
        });
}

/// Fixed-width password input for inline use
pub fn fixed_password_input(
    ui: &mut egui::Ui,
    content: &mut String,
    hint: &str,
    width: f32,
    theme: &ThemeColors,
) {
    egui::Frame::none()
        .fill(theme.bg_secondary)
        .stroke(egui::Stroke::new(1.0, theme.input_border))
        .rounding(INPUT_ROUNDING)
        .inner_margin(egui::Margin::symmetric(INPUT_PADDING_X, INPUT_PADDING_Y))
        .show(ui, |ui| {
            ui.add_sized([width, INPUT_HEIGHT],
                egui::TextEdit::singleline(content)
                    .password(true)
                    .hint_text(hint_text_style(hint, theme))
                    .frame(false)
            );
        });
}

// ============================================================================
// Form Row Layouts
// ============================================================================

/// Single form field: label above, input below (fills width)
pub fn form_field(
    ui: &mut egui::Ui,
    label: &str,
    required: bool,
    content: &mut String,
    hint: &str,
    theme: &ThemeColors,
) {
    ui.vertical(|ui| {
        form_label(ui, label, required, theme);
        ui.add_space(SPACING_LABEL);
        text_input(ui, content, hint, theme);
    });
}

/// Password form field: label above, input below
pub fn form_field_password(
    ui: &mut egui::Ui,
    label: &str,
    required: bool,
    content: &mut String,
    hint: &str,
    theme: &ThemeColors,
) {
    ui.vertical(|ui| {
        form_label(ui, label, required, theme);
        ui.add_space(SPACING_LABEL);
        password_input(ui, content, hint, theme);
    });
}

/// Two fields in one row: each with label above, input below
pub fn form_field_2col(
    ui: &mut egui::Ui,
    label1: &str,
    required1: bool,
    content1: &mut String,
    hint1: &str,
    width1: f32,
    label2: &str,
    required2: bool,
    content2: &mut String,
    hint2: &str,
    width2: f32,
    theme: &ThemeColors,
) {
    ui.horizontal(|ui| {
        // First field: label above, input below
        ui.vertical(|ui| {
            form_label(ui, label1, required1, theme);
            ui.add_space(SPACING_LABEL);
            fixed_input(ui, content1, hint1, width1, theme);
        });

        ui.add_space(SPACING_INLINE);

        // Second field: label above, input below
        ui.vertical(|ui| {
            form_label(ui, label2, required2, theme);
            ui.add_space(SPACING_LABEL);
            fixed_input(ui, content2, hint2, width2, theme);
        });
    });
}

/// Two fields in one row: first text, second password (each with label above)
pub fn form_field_2col_mixed(
    ui: &mut egui::Ui,
    label1: &str,
    required1: bool,
    content1: &mut String,
    hint1: &str,
    width1: f32,
    label2: &str,
    required2: bool,
    content2: &mut String,
    hint2: &str,
    width2: f32,
    theme: &ThemeColors,
) {
    ui.horizontal(|ui| {
        // First field: label above, input below
        ui.vertical(|ui| {
            form_label(ui, label1, required1, theme);
            ui.add_space(SPACING_LABEL);
            fixed_input(ui, content1, hint1, width1, theme);
        });

        ui.add_space(SPACING_INLINE);

        // Second field: label above, password input below
        ui.vertical(|ui| {
            form_label(ui, label2, required2, theme);
            ui.add_space(SPACING_LABEL);
            fixed_password_input(ui, content2, hint2, width2, theme);
        });
    });
}

/// Textarea form field: label above, textarea below
pub fn form_field_textarea(
    ui: &mut egui::Ui,
    label: &str,
    required: bool,
    content: &mut String,
    hint: &str,
    height: f32,
    theme: &ThemeColors,
) {
    ui.vertical(|ui| {
        form_label(ui, label, required, theme);
        ui.add_space(SPACING_LABEL);
        text_area(ui, content, hint, height, theme);
    });
}

/// Form section header with optional separator
pub fn form_section(ui: &mut egui::Ui, title: &str, theme: &ThemeColors) {
    ui.add_space(SPACING_SECTION);
    ui.label(egui::RichText::new(title)
        .color(theme.fg_primary)
        .size(FONT_SIZE_TITLE)
        .strong());
    ui.add_space(SPACING_FIELD);
}

/// Form separator line
pub fn form_separator(ui: &mut egui::Ui) {
    ui.add_space(8.0);
    ui.add(egui::Separator::default().spacing(0.0).horizontal());
    ui.add_space(8.0);
}

/// Section header label (for list headers, not form sections)
pub fn section_header(text: &str, theme: &ThemeColors) -> egui::RichText {
    egui::RichText::new(text)
        .color(theme.fg_dim)
        .size(12.0)
        .strong()
}

/// Form field label (alias for compatibility)
pub fn field_label(text: &str, theme: &ThemeColors) -> egui::RichText {
    egui::RichText::new(text)
        .color(theme.fg_dim)
        .size(12.0)
}

// ============================================================================
// Legacy Compatibility (deprecated, will remove later)
// ============================================================================

#[allow(dead_code)]
pub fn shadcn_input<'a>(
    ui: &mut egui::Ui,
    label: &'a str,
    required: bool,
    content: &'a mut String,
    hint: &'a str,
    width: f32,
    theme: &'a ThemeColors,
) {
    ui.vertical(|ui| {
        form_label(ui, label, required, theme);
        ui.add_space(SPACING_LABEL);
        egui::Frame::none()
            .fill(theme.bg_secondary)
            .stroke(egui::Stroke::new(1.0, theme.input_border))
            .rounding(INPUT_ROUNDING)
            .inner_margin(egui::Margin::symmetric(INPUT_PADDING_X, INPUT_PADDING_Y))
            .show(ui, |ui| {
                ui.add_sized([width, INPUT_HEIGHT], egui::TextEdit::singleline(content)
                    .hint_text(hint_text_style(hint, theme))
                    .frame(false)
                );
            });
    });
}

#[allow(dead_code)]
pub fn shadcn_textarea<'a>(
    ui: &mut egui::Ui,
    label: &'a str,
    required: bool,
    content: &'a mut String,
    hint: &'a str,
    width: f32,
    height: f32,
    theme: &'a ThemeColors,
) {
    ui.vertical(|ui| {
        form_label(ui, label, required, theme);
        ui.add_space(SPACING_LABEL);
        egui::Frame::none()
            .fill(theme.bg_secondary)
            .stroke(egui::Stroke::new(1.0, theme.input_border))
            .rounding(INPUT_ROUNDING)
            .inner_margin(egui::Margin::symmetric(INPUT_PADDING_X, INPUT_PADDING_Y))
            .show(ui, |ui| {
                ui.add_sized([width, height], egui::TextEdit::multiline(content)
                    .hint_text(hint_text_style(hint, theme))
                    .frame(false)
                );
            });
    });
}

// Legacy aliases
pub use form_field as form_row;
pub use fixed_input as compact_input;
pub use fixed_password_input as compact_password_input;
