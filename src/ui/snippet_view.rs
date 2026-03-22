use eframe::egui;
use uuid::Uuid;

use crate::app::PortalApp;
use crate::config::{self, Snippet};
use crate::ui::theme::brighter;
use crate::ui::types::AppView;

impl PortalApp {
    pub fn show_snippets_view(&mut self, _ctx: &egui::Context, ui: &mut egui::Ui) {
        let theme = self.theme.clone();
        let lang = self.language;

        // Collect deferred actions
        let mut snippet_to_delete: Option<String> = None;
        let mut snippet_to_save: Option<Snippet> = None;
        let mut snippet_to_create: Option<Snippet> = None;
        let mut command_to_run: Option<String> = None;

        egui::ScrollArea::vertical()
            .id_salt("snippets_page_scroll")
            .show(ui, |ui| {
                ui.add_space(20.0);

                // ── Page header ──
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.label(
                        egui::RichText::new(lang.t("snippets"))
                            .color(theme.fg_dim)
                            .size(12.0)
                            .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(24.0);
                        if ui.add(
                            egui::Button::new(
                                egui::RichText::new(lang.t("new_snippet"))
                                    .color(theme.accent)
                                    .size(12.0),
                            )
                            .frame(false)
                        ).clicked() {
                            self.snippet_view_state.show_new = true;
                            self.snippet_view_state.new_name.clear();
                            self.snippet_view_state.new_command.clear();
                            self.snippet_view_state.new_group = lang.t("snippet_default_group").to_string();
                        }
                    });
                });
                ui.add_space(8.0);

                // ── Search bar ──
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    let search_resp = ui.add_sized(
                        [ui.available_width() - 48.0, 28.0],
                        egui::TextEdit::singleline(&mut self.snippet_view_state.search_query)
                            .hint_text(lang.t("search_placeholder"))
                            .text_color(theme.fg_primary)
                            .font(egui::FontId::proportional(13.0)),
                    );
                    let _ = search_resp;
                    ui.add_space(24.0);
                });
                ui.add_space(12.0);

                // ── New snippet form ──
                if self.snippet_view_state.show_new {
                    ui.horizontal(|ui| {
                        ui.add_space(24.0);
                        egui::Frame {
                            fill: theme.bg_elevated,
                            rounding: egui::Rounding::same(8.0),
                            inner_margin: egui::Margin::same(16.0),
                            stroke: egui::Stroke::new(1.0, theme.accent),
                            ..Default::default()
                        }
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width() - 48.0);

                            ui.label(
                                egui::RichText::new(lang.t("new_snippet"))
                                    .color(theme.fg_primary)
                                    .size(14.0)
                                    .strong(),
                            );
                            ui.add_space(8.0);

                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(lang.t("snippet_name"))
                                        .color(theme.fg_dim)
                                        .size(12.0),
                                );
                                ui.add_sized(
                                    [200.0, 24.0],
                                    egui::TextEdit::singleline(&mut self.snippet_view_state.new_name)
                                        .text_color(theme.fg_primary)
                                        .font(egui::FontId::proportional(13.0)),
                                );
                                ui.add_space(16.0);
                                ui.label(
                                    egui::RichText::new(lang.t("snippet_group"))
                                        .color(theme.fg_dim)
                                        .size(12.0),
                                );
                                ui.add_sized(
                                    [120.0, 24.0],
                                    egui::TextEdit::singleline(&mut self.snippet_view_state.new_group)
                                        .text_color(theme.fg_primary)
                                        .font(egui::FontId::proportional(13.0)),
                                );
                            });
                            ui.add_space(4.0);

                            ui.label(
                                egui::RichText::new(lang.t("snippet_command"))
                                    .color(theme.fg_dim)
                                    .size(12.0),
                            );
                            ui.add_sized(
                                [ui.available_width(), 60.0],
                                egui::TextEdit::multiline(&mut self.snippet_view_state.new_command)
                                    .text_color(theme.fg_primary)
                                    .font(egui::FontId::monospace(13.0))
                                    .desired_rows(3),
                            );
                            ui.add_space(8.0);

                            ui.horizontal(|ui| {
                                if ui.add(
                                    egui::Button::new(
                                        egui::RichText::new(lang.t("save"))
                                            .color(egui::Color32::WHITE)
                                            .size(12.0),
                                    )
                                    .fill(theme.accent)
                                    .rounding(6.0)
                                    .min_size(egui::vec2(60.0, 28.0))
                                ).clicked() && !self.snippet_view_state.new_name.trim().is_empty() {
                                    let group = if self.snippet_view_state.new_group.trim().is_empty() {
                                        lang.t("snippet_default_group").to_string()
                                    } else {
                                        self.snippet_view_state.new_group.trim().to_string()
                                    };
                                    snippet_to_create = Some(Snippet {
                                        id: Uuid::new_v4().to_string(),
                                        name: self.snippet_view_state.new_name.trim().to_string(),
                                        command: self.snippet_view_state.new_command.clone(),
                                        group,
                                    });
                                }
                                ui.add_space(8.0);
                                if ui.add(
                                    egui::Button::new(
                                        egui::RichText::new(lang.t("cancel"))
                                            .color(theme.fg_dim)
                                            .size(12.0),
                                    )
                                    .fill(theme.bg_secondary)
                                    .rounding(6.0)
                                    .min_size(egui::vec2(60.0, 28.0))
                                ).clicked() {
                                    self.snippet_view_state.show_new = false;
                                }
                            });
                        });
                    });
                    ui.add_space(12.0);
                }

                // ── Empty state ──
                if self.snippets.is_empty() && !self.snippet_view_state.show_new {
                    ui.add_space(60.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("\u{2318}")
                                .size(32.0)
                                .color(theme.fg_dim),
                        );
                        ui.add_space(12.0);
                        ui.label(
                            egui::RichText::new(lang.t("snippets"))
                                .color(theme.fg_dim)
                                .size(14.0),
                        );
                    });
                    return;
                }

                // ── Filter snippets ──
                let query = self.snippet_view_state.search_query.to_lowercase();
                let filtered: Vec<&Snippet> = self.snippets.iter()
                    .filter(|s| {
                        if query.is_empty() {
                            return true;
                        }
                        s.name.to_lowercase().contains(&query) ||
                        s.command.to_lowercase().contains(&query)
                    })
                    .collect();

                // ── Group snippets ──
                let mut groups: std::collections::BTreeMap<&str, Vec<&Snippet>> =
                    std::collections::BTreeMap::new();
                for snippet in &filtered {
                    groups.entry(snippet.group.as_str()).or_default().push(snippet);
                }

                // ── Render groups ──
                for (group_name, snippets_in_group) in &groups {
                    ui.horizontal(|ui| {
                        ui.add_space(24.0);
                        let header_id = ui.make_persistent_id(format!("snippet_group_{}", group_name));
                        let mut open = ui.data(|d| d.get_temp::<bool>(header_id).unwrap_or(true));
                        let arrow = if open { "\u{25BC}" } else { "\u{25B6}" };
                        let header_resp = ui.add(
                            egui::Button::new(
                                egui::RichText::new(format!("{} {}", arrow, group_name))
                                    .color(theme.fg_dim)
                                    .size(11.0)
                                    .strong(),
                            )
                            .frame(false),
                        );
                        if header_resp.clicked() {
                            open = !open;
                            ui.data_mut(|d| d.insert_temp(header_id, open));
                        }
                    });
                    ui.add_space(4.0);

                    let header_id = ui.make_persistent_id(format!("snippet_group_{}", group_name));
                    let open = ui.data(|d| d.get_temp::<bool>(header_id).unwrap_or(true));
                    if !open {
                        continue;
                    }

                    for snippet in snippets_in_group {
                        let is_editing = self.snippet_view_state.editing.as_deref() == Some(&snippet.id);
                        let is_confirming_delete = self.snippet_view_state.confirm_delete.as_deref() == Some(&snippet.id);

                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            egui::Frame {
                                fill: if is_editing { theme.bg_elevated } else { brighter(theme.bg_primary, 8) },
                                rounding: egui::Rounding::same(6.0),
                                inner_margin: egui::Margin::same(12.0),
                                stroke: if is_editing {
                                    egui::Stroke::new(1.0, theme.accent)
                                } else {
                                    egui::Stroke::new(1.0, theme.border)
                                },
                                ..Default::default()
                            }
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width() - 48.0);

                                if is_editing {
                                    // ── Inline edit form ──
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(lang.t("snippet_name"))
                                                .color(theme.fg_dim)
                                                .size(12.0),
                                        );
                                        ui.add_sized(
                                            [200.0, 24.0],
                                            egui::TextEdit::singleline(&mut self.snippet_view_state.edit_name)
                                                .text_color(theme.fg_primary)
                                                .font(egui::FontId::proportional(13.0)),
                                        );
                                        ui.add_space(16.0);
                                        ui.label(
                                            egui::RichText::new(lang.t("snippet_group"))
                                                .color(theme.fg_dim)
                                                .size(12.0),
                                        );
                                        ui.add_sized(
                                            [120.0, 24.0],
                                            egui::TextEdit::singleline(&mut self.snippet_view_state.edit_group)
                                                .text_color(theme.fg_primary)
                                                .font(egui::FontId::proportional(13.0)),
                                        );
                                    });
                                    ui.add_space(4.0);

                                    ui.label(
                                        egui::RichText::new(lang.t("snippet_command"))
                                            .color(theme.fg_dim)
                                            .size(12.0),
                                    );
                                    ui.add_sized(
                                        [ui.available_width(), 60.0],
                                        egui::TextEdit::multiline(&mut self.snippet_view_state.edit_command)
                                            .text_color(theme.fg_primary)
                                            .font(egui::FontId::monospace(13.0))
                                            .desired_rows(3),
                                    );
                                    ui.add_space(8.0);

                                    ui.horizontal(|ui| {
                                        if ui.add(
                                            egui::Button::new(
                                                egui::RichText::new(lang.t("save"))
                                                    .color(egui::Color32::WHITE)
                                                    .size(12.0),
                                            )
                                            .fill(theme.accent)
                                            .rounding(6.0)
                                            .min_size(egui::vec2(60.0, 28.0))
                                        ).clicked() && !self.snippet_view_state.edit_name.trim().is_empty() {
                                            let group = if self.snippet_view_state.edit_group.trim().is_empty() {
                                                lang.t("snippet_default_group").to_string()
                                            } else {
                                                self.snippet_view_state.edit_group.trim().to_string()
                                            };
                                            snippet_to_save = Some(Snippet {
                                                id: snippet.id.clone(),
                                                name: self.snippet_view_state.edit_name.trim().to_string(),
                                                command: self.snippet_view_state.edit_command.clone(),
                                                group,
                                            });
                                        }
                                        ui.add_space(8.0);
                                        if ui.add(
                                            egui::Button::new(
                                                egui::RichText::new(lang.t("cancel"))
                                                    .color(theme.fg_dim)
                                                    .size(12.0),
                                            )
                                            .fill(theme.bg_secondary)
                                            .rounding(6.0)
                                            .min_size(egui::vec2(60.0, 28.0))
                                        ).clicked() {
                                            self.snippet_view_state.editing = None;
                                        }
                                    });
                                } else {
                                    // ── Snippet display row ──
                                    ui.horizontal(|ui| {
                                        // Left side: name + command preview
                                        ui.vertical(|ui| {
                                            ui.label(
                                                egui::RichText::new(&snippet.name)
                                                    .color(theme.fg_primary)
                                                    .size(13.0)
                                                    .strong(),
                                            );
                                            // Command preview (truncated, monospace)
                                            let preview = if snippet.command.len() > 80 {
                                                format!("{}...", &snippet.command[..80])
                                            } else {
                                                snippet.command.clone()
                                            };
                                            // Replace newlines for single-line preview
                                            let preview = preview.replace('\n', " \u{21B5} ");
                                            ui.label(
                                                egui::RichText::new(preview)
                                                    .color(theme.fg_dim)
                                                    .size(11.0)
                                                    .family(egui::FontFamily::Monospace),
                                            );
                                        });

                                        // Right side: action buttons
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            // Delete button / confirmation
                                            if is_confirming_delete {
                                                if ui.add(
                                                    egui::Button::new(
                                                        egui::RichText::new(lang.t("cancel"))
                                                            .color(theme.fg_dim)
                                                            .size(11.0),
                                                    )
                                                    .frame(false)
                                                ).clicked() {
                                                    self.snippet_view_state.confirm_delete = None;
                                                }
                                                if ui.add(
                                                    egui::Button::new(
                                                        egui::RichText::new(lang.t("delete"))
                                                            .color(theme.red)
                                                            .size(11.0),
                                                    )
                                                    .frame(false)
                                                ).clicked() {
                                                    snippet_to_delete = Some(snippet.id.clone());
                                                }
                                                ui.label(
                                                    egui::RichText::new(lang.t("confirm_delete"))
                                                        .color(theme.red)
                                                        .size(11.0),
                                                );
                                            } else {
                                                if ui.add(
                                                    egui::Button::new(
                                                        egui::RichText::new(lang.t("delete"))
                                                            .color(theme.fg_dim)
                                                            .size(11.0),
                                                    )
                                                    .frame(false)
                                                ).clicked() {
                                                    self.snippet_view_state.confirm_delete = Some(snippet.id.clone());
                                                }

                                                // Edit button
                                                if ui.add(
                                                    egui::Button::new(
                                                        egui::RichText::new(lang.t("edit_file"))
                                                            .color(theme.fg_dim)
                                                            .size(11.0),
                                                    )
                                                    .frame(false)
                                                ).clicked() {
                                                    self.snippet_view_state.editing = Some(snippet.id.clone());
                                                    self.snippet_view_state.edit_name = snippet.name.clone();
                                                    self.snippet_view_state.edit_command = snippet.command.clone();
                                                    self.snippet_view_state.edit_group = snippet.group.clone();
                                                }

                                                // Run button
                                                if ui.add(
                                                    egui::Button::new(
                                                        egui::RichText::new(lang.t("run_snippet"))
                                                            .color(theme.accent)
                                                            .size(11.0)
                                                            .strong(),
                                                    )
                                                    .fill(theme.accent_alpha(25))
                                                    .rounding(4.0)
                                                    .min_size(egui::vec2(48.0, 24.0))
                                                ).clicked() {
                                                    command_to_run = Some(snippet.command.clone());
                                                }
                                            }
                                        });
                                    });
                                }
                            });
                        });
                        ui.add_space(4.0);
                    }
                    ui.add_space(8.0);
                }

                ui.add_space(40.0);
            });

        // ── Apply deferred actions ──

        if let Some(snippet) = snippet_to_create {
            self.snippets.push(snippet);
            config::save_snippets(&self.snippets);
            self.snippet_view_state.show_new = false;
            self.snippet_view_state.new_name.clear();
            self.snippet_view_state.new_command.clear();
            self.snippet_view_state.new_group.clear();
        }

        if let Some(updated) = snippet_to_save {
            if let Some(s) = self.snippets.iter_mut().find(|s| s.id == updated.id) {
                s.name = updated.name;
                s.command = updated.command;
                s.group = updated.group;
            }
            config::save_snippets(&self.snippets);
            self.snippet_view_state.editing = None;
        }

        if let Some(id) = snippet_to_delete {
            self.snippets.retain(|s| s.id != id);
            config::save_snippets(&self.snippets);
            self.snippet_view_state.confirm_delete = None;
        }

        if let Some(command) = command_to_run {
            // Write command + newline to the focused terminal session
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                if let Some(session) = tab.sessions.get_mut(tab.focused_session) {
                    session.write(&format!("{}\n", command));
                }
            }
            // Switch to terminal view so the user can see the output
            self.current_view = AppView::Terminal;
        }
    }
}
