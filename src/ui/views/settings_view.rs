use eframe::egui;

use crate::app::PortalApp;
use crate::app::SettingsSection;
use crate::ui::theme::ThemePreset;
use crate::ui::i18n::Language;
use crate::ui::tokens::*;
use crate::ui::widgets;
use crate::ui::fonts;

impl PortalApp {
    pub fn apply_visuals(&self, ctx: &egui::Context) {
        // Use light or dark base visuals depending on theme
        let is_light_theme = matches!(self.theme_preset,
            ThemePreset::SolarizedLight | ThemePreset::GitHubLight | ThemePreset::OneLight
        );
        let mut visuals = if is_light_theme {
            egui::Visuals::light()
        } else {
            egui::Visuals::dark()
        };

        visuals.panel_fill = self.theme.bg_primary;
        visuals.window_fill = self.theme.menu_bg;
        visuals.extreme_bg_color = self.theme.input_bg;
        visuals.faint_bg_color = self.theme.bg_elevated;

        // Border color derived from theme
        let border = self.theme.input_border;

        // Non-interactive widgets (labels, separators)
        visuals.widgets.noninteractive.bg_fill = self.theme.bg_secondary;
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, self.theme.fg_primary);

        // Inactive widgets (text inputs, buttons not hovered)
        visuals.widgets.inactive.bg_fill = self.theme.bg_secondary;
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, border);
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, self.theme.fg_primary);

        // Hovered widgets
        visuals.widgets.hovered.bg_fill = self.theme.bg_elevated;
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, self.theme.focus_ring);
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

        // Don't override text color - let individual widgets control it
        visuals.override_text_color = None;

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
        let fonts = fonts::load_fonts(&self.custom_font_path);
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
            ssh_keepalive_interval: self.ssh_keepalive_interval,
            keyboard_shortcuts: self.shortcut_resolver.bindings().to_vec(),
            check_for_updates: self.check_for_updates,
            last_dismissed_update_version: self.last_dismissed_update_version.clone(),
            auto_record_sessions: self.auto_record_sessions,
            recordings_retention_days: self.recordings_retention_days,
            recordings_max_total_mb: self.recordings_max_total_mb,
            restore_sessions: self.restore_sessions,
            zmodem_download_dir: if self.zmodem_download_dir.trim().is_empty() {
                None
            } else {
                Some(self.zmodem_download_dir.trim().to_string())
            },
            terminal_background_image: if self.terminal_background_image.is_empty() {
                None
            } else {
                Some(self.terminal_background_image.clone())
            },
            terminal_background_opacity: self.terminal_background_opacity,
            llm: self.llm_config.clone(),
        };
        crate::config::save_settings(&settings);
    }

    pub fn show_settings_view(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let theme = self.theme.clone();
        let lang = self.language;
        let mut changed = false;

        ui.add_space(SPACE_XL);
        ui.horizontal(|ui| {
            ui.add_space(SPACE_XL);
            ui.label(widgets::section_header(lang.t("settings"), &theme));
        });
        ui.add_space(SPACE_LG);

        // Two-column layout: a fixed-width vertical nav on the left, the active
        // section's content fills the remaining width on the right (macOS System
        // Settings style). The right column is given an explicit width so its
        // vertical ScrollArea and containing fields lay out as a normal page
        // instead of being squeezed into a single row by the outer horizontal.
        const NAV_WIDTH: f32 = 176.0;
        ui.horizontal_top(|ui| {
            ui.add_space(SPACE_XL);
            let remaining = ui.available_width();

            // ── Left navigation ──
            ui.allocate_ui_with_layout(
                egui::vec2(NAV_WIDTH, ui.available_height()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let sections: [(SettingsSection, &str); 5] = [
                        (SettingsSection::General, "general"),
                        (SettingsSection::Appearance, "appearance"),
                        (SettingsSection::Terminal, "terminal"),
                        (SettingsSection::Ai, "ai"),
                        (SettingsSection::Shortcuts, "shortcuts"),
                    ];
                    for (section, key) in sections {
                        let selected = self.settings_section == section;
                        let text = if selected {
                            egui::RichText::new(lang.t(key)).color(theme.fg_primary).size(FONT_BASE).strong()
                        } else {
                            egui::RichText::new(lang.t(key)).color(theme.fg_dim).size(FONT_BASE)
                        };
                        let resp = ui.add_sized(
                            [NAV_WIDTH - 16.0, 26.0],
                            egui::SelectableLabel::new(selected, text),
                        );
                        if resp.clicked() {
                            self.settings_section = section;
                        }
                    }
                },
            );

            // ── Right content (fills the remaining width) ──
            ui.add_space(SPACE_MD);
            ui.separator();
            ui.add_space(SPACE_MD);

            let content_width = (remaining - NAV_WIDTH - 3.0 * SPACE_MD - 8.0).max(240.0);
            ui.allocate_ui_with_layout(
                egui::vec2(content_width, ui.available_height()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            egui::Frame {
                                inner_margin: egui::Margin::symmetric(SPACE_LG, SPACE_MD),
                                ..Default::default()
                            }
                            .show(ui, |ui| {
                                ui.set_min_width(content_width - 2.0 * SPACE_LG);
                                ui.spacing_mut().item_spacing.y = 12.0;
                                match self.settings_section {
                                    SettingsSection::Appearance => {
                                        self.render_appearance_section(ctx, ui, &theme, lang, &mut changed);
                                    }
                                    SettingsSection::Terminal => {
                                        self.render_terminal_section(ui, &theme, lang, &mut changed);
                                    }
                                    SettingsSection::Ai => {
                                        self.render_ai_section(ui, &theme, lang, &mut changed);
                                    }
                                    SettingsSection::Shortcuts => {
                                        self.render_shortcuts_section(ctx, ui, &theme, lang, &mut changed);
                                    }
                                    SettingsSection::General => {
                                        self.render_general_section(ui, &theme, lang, &mut changed);
                                    }
                                }
                            });
                        });
                },
            );
        });

        if changed {
            self.save_settings_to_disk();
        }
    }

    /// Appearance section: font, theme, and language.
    fn render_appearance_section(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        theme: &crate::ui::theme::ThemeColors,
        lang: crate::ui::i18n::Language,
        changed: &mut bool,
    ) {
        ui.label(egui::RichText::new(lang.t("font")).color(theme.fg_primary).size(14.0).strong());
        ui.add_space(SPACE_XS);

        ui.horizontal(|ui| {
            ui.label(widgets::field_label(lang.t("font_size"), theme));
            ui.add_space(SPACE_SM);
            let slider = ui.add(
                egui::Slider::new(&mut self.font_size, 8.0..=32.0)
                    .step_by(1.0)
                    .text("px")
            );
            if slider.changed() {
                *changed = true;
            }
        });

        ui.horizontal(|ui| {
            ui.label(widgets::field_label(lang.t("custom_font"), theme));
            ui.add_space(SPACE_SM);
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.custom_font_path)
                    .hint_text(egui::RichText::new("/path/to/font.ttf").color(theme.hint_color()).italics())
                    .desired_width(300.0)
                    .font(egui::FontId::proportional(13.0))
                    .text_color(theme.fg_primary)
            );
            if resp.lost_focus() {
                self.fonts_dirty = true;
                *changed = true;
            }
        });

        ui.add_space(SPACE_LG);
        ui.separator();
        ui.add_space(SPACE_SM);

        ui.label(egui::RichText::new(lang.t("theme")).color(theme.fg_primary).size(14.0).strong());
        ui.add_space(SPACE_XS);

        for preset in ThemePreset::all() {
            if ui.selectable_value(&mut self.theme_preset, *preset, preset.label()).clicked() {
                self.theme = self.theme_preset.colors();
                self.apply_visuals(ctx);
                *changed = true;
            }
        }

        ui.add_space(SPACE_LG);
        ui.separator();
        ui.add_space(SPACE_SM);

        // ── Background image ──
        // The texture itself loads lazily in render_terminal_content when the
        // user returns to a terminal view; here we only edit the settings.
        ui.label(egui::RichText::new(lang.t("terminal_background")).color(theme.fg_primary).size(14.0).strong());
        ui.add_space(SPACE_XS);
        ui.label(egui::RichText::new(lang.t("terminal_background_desc")).color(theme.fg_dim).size(FONT_SM));

        ui.horizontal(|ui| {
            ui.label(widgets::field_label(lang.t("terminal_background_image"), theme));
            ui.add_space(SPACE_SM);
            if ui.button(lang.t("choose_image")).clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("images", &["png", "jpg", "jpeg", "gif", "webp", "bmp"])
                    .pick_file()
                {
                    self.terminal_background_image = path.to_string_lossy().to_string();
                    *changed = true;
                }
            }
            if !self.terminal_background_image.is_empty()
                && ui.button(lang.t("clear_image")).clicked()
            {
                self.terminal_background_image.clear();
                *changed = true;
            }
        });
        if !self.terminal_background_image.is_empty() {
            ui.label(
                egui::RichText::new(&self.terminal_background_image)
                    .color(theme.hint_color())
                    .size(FONT_SM),
            );
        }

        ui.horizontal(|ui| {
            ui.label(widgets::field_label(lang.t("background_opacity"), theme));
            ui.add_space(SPACE_SM);
            let slider = ui.add(
                egui::Slider::new(&mut self.terminal_background_opacity, 0.0..=1.0)
                    .custom_formatter(|v, _| format!("{}%", (v * 100.0).round() as i32))
                    .custom_parser(|s| {
                        s.trim().trim_end_matches('%').parse::<f64>().ok().map(|v| v / 100.0)
                    }),
            );
            if slider.changed() {
                *changed = true;
            }
        });
    }

    /// Terminal section: scrollback and SSH keepalive.
    fn render_terminal_section(
        &mut self,
        ui: &mut egui::Ui,
        theme: &crate::ui::theme::ThemeColors,
        lang: crate::ui::i18n::Language,
        changed: &mut bool,
    ) {
        ui.label(egui::RichText::new(lang.t("terminal")).color(theme.fg_primary).size(14.0).strong());
        ui.add_space(SPACE_XS);

        ui.horizontal(|ui| {
            ui.label(widgets::field_label(lang.t("scrollback_limit"), theme));
            ui.add_space(SPACE_SM);
            let slider = ui.add(
                egui::Slider::new(&mut self.scrollback_limit_mb, 10..=1000)
                    .step_by(10.0)
                    .text("MB")
            );
            if slider.changed() {
                *changed = true;
            }
        });

        // Auto-record: every new session records as asciicast from byte zero.
        ui.add_space(SPACE_SM);
        if ui.checkbox(&mut self.auto_record_sessions, lang.t("auto_record")).changed() {
            *changed = true;
        }
        ui.label(egui::RichText::new(lang.t("auto_record_desc")).color(theme.fg_dim).size(FONT_SM));

        // Recordings: retention policy — the safety net for auto-record,
        // which otherwise accumulates .cast files forever.
        ui.add_space(SPACE_LG);
        ui.separator();
        ui.add_space(SPACE_SM);

        ui.label(egui::RichText::new(lang.t("recordings_section")).color(theme.fg_primary).size(14.0).strong());
        ui.add_space(SPACE_XS);

        ui.horizontal(|ui| {
            ui.label(widgets::field_label(lang.t("recordings_retention"), theme));
            ui.add_space(SPACE_SM);
            let slider = ui.add(
                egui::Slider::new(&mut self.recordings_retention_days, 0..=365)
                    .step_by(1.0)
                    .text(lang.t("unit_days"))
            );
            if slider.changed() {
                *changed = true;
            }
        });
        ui.label(egui::RichText::new(lang.t("recordings_retention_desc")).color(theme.fg_dim).size(FONT_SM));

        ui.add_space(SPACE_SM);
        ui.horizontal(|ui| {
            ui.label(widgets::field_label(lang.t("recordings_max_total"), theme));
            ui.add_space(SPACE_SM);
            let slider = ui.add(
                egui::Slider::new(&mut self.recordings_max_total_mb, 0..=10240)
                    .step_by(100.0)
                    .text("MB")
            );
            if slider.changed() {
                *changed = true;
            }
        });
        ui.label(egui::RichText::new(lang.t("recordings_max_total_desc")).color(theme.fg_dim).size(FONT_SM));

        // Current usage + a shortcut to the folder (the replay library is the
        // content manager; this is the policy page's reality check).
        ui.add_space(SPACE_SM);
        ui.horizontal(|ui| {
            let dir = portal::terminal::cast::recordings_dir();
            if ui.button(lang.t("replay_open_folder")).clicked() {
                #[cfg(target_os = "macos")]
                let _ = std::process::Command::new("open").arg(&dir).spawn();
                #[cfg(not(target_os = "macos"))]
                let _ = std::process::Command::new("xdg-open").arg(&dir).spawn();
            }
            let mut count = 0usize;
            let mut total: u64 = 0;
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for e in entries.flatten() {
                    if e.path().extension().and_then(|x| x.to_str()) == Some("cast") {
                        if let Ok(m) = e.metadata() {
                            count += 1;
                            total += m.len();
                        }
                    }
                }
            }
            ui.label(
                egui::RichText::new(format!(
                    "{} · {}",
                    lang.tf("recordings_files", &count.to_string()),
                    crate::ui::views::replay_view::fmt_size(total)
                ))
                .color(theme.fg_dim)
                .size(FONT_SM),
            );
        });

        ui.add_space(SPACE_LG);
        ui.separator();
        ui.add_space(SPACE_SM);

        ui.label(egui::RichText::new("SSH").color(theme.fg_primary).size(14.0).strong());
        ui.add_space(SPACE_XS);

        ui.horizontal(|ui| {
            ui.label(widgets::field_label(lang.t("ssh_keepalive"), theme));
            ui.add_space(SPACE_SM);
            let slider = ui.add(
                egui::Slider::new(&mut self.ssh_keepalive_interval, 0..=300)
                    .step_by(5.0)
                    .text("s")
            );
            if slider.changed() {
                *changed = true;
            }
        });
        ui.label(egui::RichText::new(lang.t("ssh_keepalive_desc")).color(theme.fg_dim).size(FONT_SM));
    }

    /// AI section: LLM provider, base URL, model, and API key.
    fn render_ai_section(
        &mut self,
        ui: &mut egui::Ui,
        theme: &crate::ui::theme::ThemeColors,
        lang: crate::ui::i18n::Language,
        changed: &mut bool,
    ) {
        ui.label(egui::RichText::new(lang.t("llm")).color(theme.fg_primary).size(14.0).strong());
        ui.add_space(SPACE_XS);

        // Provider
        ui.horizontal(|ui| {
            ui.label(widgets::field_label(lang.t("llm_provider"), theme));
            ui.add_space(SPACE_SM);
            let backend = self.llm_config.backend;
            let selected = match backend {
                crate::llm::types::LlmBackend::OpenAiCompatible => lang.t("llm_provider_openai"),
                crate::llm::types::LlmBackend::Anthropic => lang.t("llm_provider_anthropic"),
            };
            widgets::style_combo_button(ui, theme);
            egui::ComboBox::from_id_salt("llm_provider_combo")
                .selected_text(egui::RichText::new(selected).color(theme.accent).size(FONT_MD))
                .width(220.0)
                .show_ui(ui, |ui| {
                    widgets::style_dropdown(ui, theme);
                    if widgets::dropdown_item(ui, theme, lang.t("llm_provider_openai"), matches!(backend, crate::llm::types::LlmBackend::OpenAiCompatible)).clicked() {
                        self.llm_config.backend = crate::llm::types::LlmBackend::OpenAiCompatible;
                        *changed = true;
                        ui.close_menu();
                    }
                    if widgets::dropdown_item(ui, theme, lang.t("llm_provider_anthropic"), matches!(backend, crate::llm::types::LlmBackend::Anthropic)).clicked() {
                        self.llm_config.backend = crate::llm::types::LlmBackend::Anthropic;
                        *changed = true;
                        ui.close_menu();
                    }
                });
        });

        // Base URL — required for OpenAI-compatible, optional for Anthropic
        // (blank falls back to https://api.anthropic.com).
        let base_url_hint: &str = if self.llm_config.backend == crate::llm::types::LlmBackend::OpenAiCompatible {
            "https://api.openai.com/v1"
        } else {
            "https://api.anthropic.com"
        };
        ui.horizontal(|ui| {
            ui.label(widgets::field_label(lang.t("llm_base_url"), theme));
            ui.add_space(SPACE_SM);
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.llm_config.base_url)
                    .hint_text(base_url_hint)
                    .desired_width(280.0)
                    .font(egui::FontId::proportional(FONT_BASE))
                    .text_color(theme.fg_primary),
            );
            if resp.lost_focus() { *changed = true; }
        });

        // Model
        ui.horizontal(|ui| {
            ui.label(widgets::field_label(lang.t("llm_model"), theme));
            ui.add_space(SPACE_SM);
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.llm_config.model)
                    .hint_text("gpt-4o / claude-sonnet-4-5")
                    .desired_width(280.0)
                    .font(egui::FontId::proportional(FONT_BASE))
                    .text_color(theme.fg_primary),
            );
            if resp.lost_focus() { *changed = true; }
        });

        // API key (stored in keychain, never echoed back)
        ui.horizontal(|ui| {
            ui.label(widgets::field_label(lang.t("llm_api_key"), theme));
            ui.add_space(SPACE_SM);
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.llm_key_draft)
                    .password(true)
                    .hint_text(if self.llm_config.has_api_key { "••••••••" } else { "" })
                    .desired_width(220.0)
                    .font(egui::FontId::proportional(FONT_BASE))
                    .text_color(theme.fg_primary),
            );
            let save_clicked = ui.add(
                egui::Button::new(egui::RichText::new(lang.t("llm_api_key_save")).color(theme.fg_primary).size(FONT_BASE))
                    .min_size(egui::vec2(52.0, 26.0)),
            ).clicked();
            if save_clicked && !self.llm_key_draft.trim().is_empty() {
                let backend = match self.llm_config.backend {
                    crate::llm::types::LlmBackend::OpenAiCompatible => "openai",
                    crate::llm::types::LlmBackend::Anthropic => "anthropic",
                };
                if crate::config::store_llm_api_key(backend, self.llm_key_draft.trim()) {
                    self.llm_config.has_api_key = true;
                    self.llm_key_draft.clear();
                    *changed = true;
                }
            }
            let _ = resp;
        });

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(lang.t("llm_api_key_desc")).color(theme.fg_dim).size(FONT_SM));
            if self.llm_config.has_api_key
                && ui.add(egui::Button::new(egui::RichText::new(lang.t("llm_api_key_clear")).color(theme.fg_dim).size(FONT_SM)).frame(false)).clicked()
            {
                let backend = match self.llm_config.backend {
                    crate::llm::types::LlmBackend::OpenAiCompatible => "openai",
                    crate::llm::types::LlmBackend::Anthropic => "anthropic",
                };
                crate::config::delete_llm_api_key(backend);
                self.llm_config.has_api_key = false;
                *changed = true;
            }
        });
    }

    /// Shortcuts section: per-binding recorders + reset-to-defaults.
    fn render_shortcuts_section(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        theme: &crate::ui::theme::ThemeColors,
        lang: crate::ui::i18n::Language,
        changed: &mut bool,
    ) {
        ui.label(egui::RichText::new(lang.t("keyboard_shortcuts")).color(theme.fg_primary).size(14.0).strong());
        ui.add_space(SPACE_SM);

        use crate::config::ShortcutAction;
        use crate::ui::input::ShortcutResolver;

        let actions = [
            (ShortcutAction::SplitHorizontal, "shortcut_split_h"),
            (ShortcutAction::SplitVertical, "shortcut_split_v"),
            (ShortcutAction::NewTab, "shortcut_new_tab"),
            (ShortcutAction::CloseTab, "shortcut_close_tab"),
            (ShortcutAction::ClosePane, "shortcut_close_pane"),
            (ShortcutAction::NextTab, "shortcut_next_tab"),
            (ShortcutAction::PrevTab, "shortcut_prev_tab"),
            (ShortcutAction::ToggleBroadcast, "shortcut_broadcast"),
            (ShortcutAction::Search, "shortcut_search"),
            (ShortcutAction::Copy, "shortcut_copy"),
            (ShortcutAction::Paste, "shortcut_paste"),
            (ShortcutAction::SelectAll, "shortcut_select_all"),
        ];

        let is_recording = self.recording_shortcut.is_some();

        for (action, label_key) in &actions {
            let binding = self.shortcut_resolver.bindings().iter()
                .find(|b| b.action == *action)
                .cloned();
            let display = binding.as_ref()
                .map(ShortcutResolver::display_binding)
                .unwrap_or_else(|| "—".to_string());

            let is_this_recording = self.recording_shortcut.as_ref() == Some(action);

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(lang.t(label_key)).color(theme.fg_primary).size(FONT_BASE));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if is_this_recording {
                        ui.label(egui::RichText::new(lang.t("press_key")).color(theme.accent).size(FONT_BASE).strong());
                    } else {
                        let edit_enabled = !is_recording;
                        if ui.add_enabled(edit_enabled, egui::Button::new(
                            egui::RichText::new(&display).color(theme.fg_dim).size(FONT_BASE)
                        ).min_size(egui::vec2(80.0, 24.0))).clicked() {
                            self.recording_shortcut = Some(action.clone());
                        }
                    }
                });
            });
            ui.add_space(SPACE_XS / 2.0);
        }

        // Capture key combination when recording
        if let Some(ref recording_action) = self.recording_shortcut.clone() {
            let captured = ctx.input(|i| {
                for event in &i.events {
                    if let egui::Event::Key { key, pressed: true, modifiers, .. } = event {
                        if matches!(key,
                            egui::Key::ArrowUp | egui::Key::ArrowDown |
                            egui::Key::ArrowLeft | egui::Key::ArrowRight
                        ) && !modifiers.command && !modifiers.ctrl && !modifiers.alt {
                            continue;
                        }
                        if *key == egui::Key::Escape {
                            return Some(None);
                        }
                        let key_str = ShortcutResolver::key_to_string(*key);
                        return Some(Some(crate::config::KeyBinding {
                            action: recording_action.clone(),
                            key: key_str,
                            ctrl: modifiers.ctrl,
                            alt: modifiers.alt,
                            shift: modifiers.shift,
                            command: modifiers.command,
                        }));
                    }
                }
                None
            });

            if let Some(result) = captured {
                if let Some(new_binding) = result {
                    let mut bindings: Vec<crate::config::KeyBinding> = self.shortcut_resolver.bindings().to_vec();
                    if let Some(existing) = bindings.iter_mut().find(|b| b.action == *recording_action) {
                        existing.key = new_binding.key;
                        existing.ctrl = new_binding.ctrl;
                        existing.alt = new_binding.alt;
                        existing.shift = new_binding.shift;
                        existing.command = new_binding.command;
                    }
                    self.shortcut_resolver.update_bindings(bindings);
                    *changed = true;
                }
                self.recording_shortcut = None;
            }
        }

        ui.add_space(SPACE_MD);
        if ui.add(
            egui::Button::new(egui::RichText::new(lang.t("reset_defaults")).color(theme.fg_dim).size(FONT_BASE))
                .rounding(6.0)
                .min_size(egui::vec2(140.0, 30.0))
        ).clicked() {
            self.shortcut_resolver.update_bindings(crate::config::default_shortcuts());
            *changed = true;
        }
    }

    /// General section: language and update checking.
    fn render_general_section(
        &mut self,
        ui: &mut egui::Ui,
        theme: &crate::ui::theme::ThemeColors,
        lang: crate::ui::i18n::Language,
        changed: &mut bool,
    ) {
        ui.label(egui::RichText::new(lang.t("language_label")).color(theme.fg_primary).size(14.0).strong());
        ui.add_space(SPACE_XS);
        for lang_opt in Language::all() {
            if ui.selectable_value(&mut self.language, *lang_opt, lang_opt.label()).clicked() {
                *changed = true;
            }
        }

        ui.add_space(SPACE_LG);
        ui.separator();
        ui.add_space(SPACE_SM);

        ui.label(egui::RichText::new(lang.t("updates")).color(theme.fg_primary).size(14.0).strong());
        ui.add_space(SPACE_XS);
        let check = ui.checkbox(&mut self.check_for_updates, lang.t("check_for_updates"));
        if check.changed() {
            *changed = true;
            if self.check_for_updates {
                self.next_update_check = std::time::Instant::now();
            }
        }
        ui.label(egui::RichText::new(lang.t("check_for_updates_desc")).color(theme.fg_dim).size(FONT_SM));

        ui.add_space(SPACE_LG);
        ui.separator();
        ui.add_space(SPACE_SM);

        ui.label(egui::RichText::new(lang.t("restore_sessions")).color(theme.fg_primary).size(14.0).strong());
        ui.add_space(SPACE_XS);
        let restore = ui.checkbox(&mut self.restore_sessions, lang.t("restore_sessions"));
        if restore.changed() {
            *changed = true;
        }
        ui.label(egui::RichText::new(lang.t("restore_sessions_desc")).color(theme.fg_dim).size(FONT_SM));

        ui.add_space(SPACE_LG);
        ui.separator();
        ui.add_space(SPACE_SM);

        ui.label(egui::RichText::new(lang.t("zmodem_download_dir")).color(theme.fg_primary).size(14.0).strong());
        ui.add_space(SPACE_XS);
        ui.horizontal(|ui| {
            ui.label(widgets::field_label(lang.t("zmodem_download_dir"), theme));
            ui.add_space(SPACE_SM);
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.zmodem_download_dir)
                    .hint_text(egui::RichText::new("~/Downloads").color(theme.hint_color()).italics())
                    .desired_width(300.0)
                    .font(egui::FontId::proportional(13.0))
                    .text_color(theme.fg_primary)
            );
            if resp.changed() || resp.lost_focus() {
                *changed = true;
            }
        });
        ui.label(egui::RichText::new(lang.t("zmodem_download_dir_desc")).color(theme.fg_dim).size(FONT_SM));
    }
}
