use eframe::egui;

use crate::app::PortalApp;
use crate::ui::theme::ThemePreset;
use crate::ui::theme::{darker, brighter};
use crate::ui::i18n::Language;

impl PortalApp {
    pub fn apply_visuals(&self, ctx: &egui::Context) {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = self.theme.bg_primary;
        visuals.window_fill = self.theme.bg_secondary;
        visuals.extreme_bg_color = darker(self.theme.bg_secondary, 10);
        visuals.faint_bg_color = self.theme.bg_elevated;

        // Border color derived from theme
        let border = brighter(self.theme.bg_elevated, 20);

        // Non-interactive widgets (labels, separators)
        visuals.widgets.noninteractive.bg_fill = self.theme.bg_secondary;
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, self.theme.fg_primary);

        // Inactive widgets (text inputs, buttons not hovered)
        visuals.widgets.inactive.bg_fill = self.theme.bg_secondary;
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, border);
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, self.theme.fg_primary);

        // Hovered widgets
        visuals.widgets.hovered.bg_fill = self.theme.bg_elevated;
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, self.theme.accent_alpha(120));
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, self.theme.fg_primary);

        // Active / focused widgets
        visuals.widgets.active.bg_fill = self.theme.bg_elevated;
        visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, self.theme.accent);
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, self.theme.fg_primary);

        // Selection highlight
        visuals.selection.bg_fill = self.theme.accent_alpha(60);
        visuals.selection.stroke = egui::Stroke::new(1.0, self.theme.accent);

        // Override text cursor color
        visuals.text_cursor.stroke = egui::Stroke::new(2.0, self.theme.fg_primary);

        visuals.override_text_color = Some(self.theme.fg_primary);

        // Unified rounding (shadcn: 6px buttons/inputs, 8px windows)
        visuals.widgets.noninteractive.rounding = egui::Rounding::same(6.0);
        visuals.widgets.inactive.rounding = egui::Rounding::same(6.0);
        visuals.widgets.hovered.rounding = egui::Rounding::same(6.0);
        visuals.widgets.active.rounding = egui::Rounding::same(6.0);
        visuals.window_rounding = egui::Rounding::same(8.0);

        // Non-interactive border from theme
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::NONE;

        ctx.set_visuals(visuals);
    }

    pub fn apply_fonts(&mut self, ctx: &egui::Context) {
        let mut fonts = egui::FontDefinitions::default();

        if !self.custom_font_path.is_empty() {
            if let Ok(font_data) = std::fs::read(&self.custom_font_path) {
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
            if let Ok(font_data) = std::fs::read("/System/Library/Fonts/Monaco.dfont") {
                fonts.font_data.insert(
                    "Monaco".to_owned(),
                    egui::FontData::from_owned(font_data),
                );
                fonts.families
                    .entry(egui::FontFamily::Monospace)
                    .or_insert_with(Vec::new)
                    .push("Monaco".to_owned());
            }
            let cjk_paths = [
                "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
                "/System/Library/Fonts/STHeiti Medium.ttc",
                "/System/Library/Fonts/Hiragino Sans GB.ttc",
            ];
            for path in &cjk_paths {
                if let Ok(font_data) = std::fs::read(path) {
                    fonts.font_data.insert(
                        "CJK".to_owned(),
                        egui::FontData::from_owned(font_data),
                    );
                    fonts.families
                        .entry(egui::FontFamily::Monospace)
                        .or_insert_with(Vec::new)
                        .push("CJK".to_owned());
                    fonts.families
                        .entry(egui::FontFamily::Proportional)
                        .or_insert_with(Vec::new)
                        .push("CJK".to_owned());
                    break;
                }
            }
        }

        ctx.set_fonts(fonts);
        self.fonts_dirty = false;
    }

    pub fn save_settings_to_disk(&self) {
        let settings = crate::config::PortalSettings {
            font_size: self.font_size,
            custom_font_path: if self.custom_font_path.is_empty() {
                None
            } else {
                Some(self.custom_font_path.clone())
            },
            language: self.language.id().to_string(),
            scrollback_limit_mb: self.scrollback_limit_mb,
        };
        crate::config::save_settings(&settings);
    }

    pub fn show_settings_view(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let theme = self.theme.clone();
        let lang = self.language;
        let mut changed = false;

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(20.0);
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                ui.label(egui::RichText::new(lang.t("settings")).color(theme.fg_dim).size(12.0).strong());
            });
            ui.add_space(16.0);

            egui::Frame {
                inner_margin: egui::Margin::symmetric(24.0, 16.0),
                ..Default::default()
            }
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 12.0;

                // ── Font section ──
                ui.label(egui::RichText::new(lang.t("font")).color(theme.fg_primary).size(14.0).strong());
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(lang.t("font_size")).color(theme.fg_dim).size(12.0));
                    ui.add_space(8.0);
                    let slider = ui.add(
                        egui::Slider::new(&mut self.font_size, 8.0..=32.0)
                            .step_by(1.0)
                            .text("px")
                    );
                    if slider.changed() {
                        changed = true;
                    }
                });

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(lang.t("custom_font")).color(theme.fg_dim).size(12.0));
                    ui.add_space(8.0);
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.custom_font_path)
                            .hint_text(egui::RichText::new("/path/to/font.ttf").color(theme.hint_color()).italics())
                            .desired_width(300.0)
                    );
                    if resp.lost_focus() {
                        self.fonts_dirty = true;
                        changed = true;
                    }
                });

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(8.0);

                // ── Terminal section ──
                ui.label(egui::RichText::new(lang.t("terminal")).color(theme.fg_primary).size(14.0).strong());
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(lang.t("scrollback_limit")).color(theme.fg_dim).size(12.0));
                    ui.add_space(8.0);
                    let slider = ui.add(
                        egui::Slider::new(&mut self.scrollback_limit_mb, 10..=1000)
                            .step_by(10.0)
                            .text("MB")
                    );
                    if slider.changed() {
                        changed = true;
                    }
                });

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(8.0);

                // ── Theme section ──
                ui.label(egui::RichText::new(lang.t("theme")).color(theme.fg_primary).size(14.0).strong());
                ui.add_space(4.0);

                for preset in ThemePreset::all() {
                    if ui.selectable_value(&mut self.theme_preset, *preset, preset.label()).clicked() {
                        self.theme = self.theme_preset.colors();
                        self.apply_visuals(ctx);
                        changed = true;
                    }
                }

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(8.0);

                // ── Language section ──
                ui.label(egui::RichText::new(lang.t("language_label")).color(theme.fg_primary).size(14.0).strong());
                ui.add_space(4.0);
                for lang_opt in Language::all() {
                    if ui.selectable_value(&mut self.language, *lang_opt, lang_opt.label()).clicked() {
                        changed = true;
                    }
                }
            });
        });

        if changed {
            self.save_settings_to_disk();
        }
    }
}
