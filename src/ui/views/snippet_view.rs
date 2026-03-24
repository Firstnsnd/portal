//! # Snippets View
//!
//! Command snippet management page with shadcn-style design.

use eframe::egui;
use egui::Widget;
use uuid::Uuid;

use crate::app::PortalApp;
use crate::config::{self, Snippet};
use crate::ui::{tokens::*, widgets};

impl PortalApp {
    /// Full snippets page content (used by both main and detached windows)
    pub fn show_snippets_page(&mut self, ctx: &egui::Context, _ui: &mut egui::Ui) {
        // Collect deferred actions
        let mut snippet_to_delete: Option<String> = None;
        let mut snippet_to_save: Option<Snippet> = None;
        let mut snippet_to_create: Option<Snippet> = None;

        // Top navigation bar (matching terminal tab bar style)
        egui::TopBottomPanel::top("snippets_nav_bar")
            .frame(egui::Frame {
                fill: self.theme.bg_secondary,
                inner_margin: egui::Margin::symmetric(8.0, 4.0),
                stroke: egui::Stroke::NONE,
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    // Left side: Snippets title
                    ui.label(egui::RichText::new(self.language.t("snippets"))
                        .color(self.theme.fg_dim)
                        .size(FONT_BASE)
                        .strong());

                    // Right side: New Snippet button
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(widgets::text_button(
                            self.language.t("new_snippet"),
                            self.theme.accent
                        )).clicked() {
                            self.snippet_view_state.open_new(
                                self.language.t("snippet_default_group")
                            );
                        }
                    });
                });
                ui.add_space(4.0);
            });

        egui::CentralPanel::default()
            .frame(egui::Frame {
                fill: self.theme.bg_primary,
                ..Default::default()
            })
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("snippets_page_scroll")
                    .show(ui, |ui| {
                        ui.add_space(12.0);

                        // Filter bar (right-aligned with margin)
                        ui.horizontal(|ui| {
                            // Match ComboBox closed-state background
                            let input_bg = ui.visuals().extreme_bg_color;
                            let border = self.theme.input_border;
                            ui.style_mut().visuals.widgets.inactive.bg_fill = input_bg;
                            ui.style_mut().visuals.widgets.inactive.bg_stroke =
                                egui::Stroke::new(1.0, border);
                            ui.style_mut().visuals.widgets.hovered.bg_fill = input_bg;
                            ui.style_mut().visuals.widgets.hovered.bg_stroke =
                                egui::Stroke::new(1.0, self.theme.focus_ring);
                            ui.style_mut().visuals.widgets.active.bg_fill = input_bg;
                            ui.style_mut().visuals.widgets.active.bg_stroke =
                                egui::Stroke::new(1.0, self.theme.accent);
                            ui.style_mut().visuals.widgets.open.bg_fill = input_bg;
                            ui.style_mut().visuals.widgets.open.bg_stroke =
                                egui::Stroke::new(1.0, self.theme.accent);

                            // Add left space to push content to the right
                            ui.add_space(ui.available_width() - 160.0);

                            // Group filter dropdown
                            let all_groups = self.collect_snippet_groups();
                            let group_label = if self.snippet_view_state.group_filter.is_empty() {
                                self.language.t("snippet_group").to_string()
                            } else {
                                self.snippet_view_state.group_filter.clone()
                            };

                            egui::ComboBox::from_id_salt("snippet_group_filter")
                                .selected_text(
                                    egui::RichText::new(&group_label)
                                        .color(self.theme.accent)
                                        .size(12.0)
                                )
                                .width(120.0)
                                .show_ui(ui, |ui| {
                                    widgets::style_dropdown(ui, &self.theme);

                                    // All option
                                    if egui::Button::new(
                                        egui::RichText::new(self.language.t("snippet_default_group"))
                                            .color(if self.snippet_view_state.group_filter.is_empty() { self.theme.accent } else { self.theme.fg_primary })
                                            .size(12.0)
                                    ).frame(false).ui(ui).clicked() {
                                        self.snippet_view_state.group_filter.clear();
                                        ui.close_menu();
                                    }

                                    for group in &all_groups {
                                        if egui::Button::new(
                                            egui::RichText::new(group)
                                                .color(if self.snippet_view_state.group_filter == *group { self.theme.accent } else { self.theme.fg_primary })
                                                .size(12.0)
                                        ).frame(false).ui(ui).clicked() {
                                            self.snippet_view_state.group_filter = group.clone();
                                            ui.close_menu();
                                        }
                                    }
                                });

                            ui.add_space(8.0);

                            // Clear button
                            if !self.snippet_view_state.group_filter.is_empty() {
                                if ui.add(widgets::text_button(
                                    self.language.t("clear_history"),
                                    self.theme.accent
                                )).clicked() {
                                    self.snippet_view_state.group_filter.clear();
                                }
                            }
                        });

                        ui.add_space(12.0);

                        // ── Empty state ──
                        let filtered_snippets = self.filter_snippets();
                        if filtered_snippets.is_empty() {
                            ui.add_space(SPACE_2XL);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    egui::RichText::new("\u{26A1}")
                                        .size(SPACE_2XL)
                                        .color(self.theme.fg_dim),
                                );
                                ui.add_space(SPACE_MD);
                                ui.label(
                                    egui::RichText::new(self.language.t("no_snippets"))
                                        .color(self.theme.fg_dim)
                                        .size(FONT_BASE),
                                );
                            });
                            return;
                        }

                        // ── Group snippets ──
                        let mut groups: std::collections::BTreeMap<String, Vec<Snippet>> =
                            std::collections::BTreeMap::new();
                        for snippet in filtered_snippets {
                            groups.entry(snippet.group.clone()).or_default().push(snippet);
                        }

                        // ── Render each group ──
                        for (group_name, snippets_in_group) in &groups {
                            // Section header
                            ui.horizontal(|ui| {
                                ui.add_space(24.0);
                                ui.label(
                                    egui::RichText::new(group_name.to_uppercase())
                                        .color(self.theme.fg_dim)
                                        .size(10.0)
                                        .strong(),
                                );
                            });
                            ui.add_space(4.0);

                            // Snippet rows
                            for snippet in snippets_in_group {
                                let row_h = 58.0;
                                let width = ui.available_width();
                                let (rect, resp): (egui::Rect, egui::Response) = ui.allocate_exact_size(
                                    egui::vec2(width, row_h),
                                    egui::Sense::click(),
                                );
                                let hovered = resp.hovered();

                                // Hover background
                                if hovered {
                                    ui.painter().rect_filled(
                                        egui::Rect::from_min_max(
                                            egui::pos2(rect.min.x, rect.max.y - 1.0),
                                            rect.max
                                        ),
                                        0.0,
                                        self.theme.hover_shadow,
                                    );
                                    ui.painter().rect_filled(rect, 0.0, self.theme.hover_bg);
                                }

                                // ── Snippet display row ──
                                // Icon
                                ui.painter().text(
                                    egui::pos2(rect.min.x + 24.0, rect.min.y + 18.0),
                                    egui::Align2::LEFT_CENTER,
                                    "\u{26A1}",  // Lightning bolt for snippets
                                    egui::FontId::proportional(12.0),
                                    self.theme.accent,
                                );

                                // Name
                                ui.painter().text(
                                    egui::pos2(rect.min.x + 46.0, rect.min.y + 18.0),
                                    egui::Align2::LEFT_CENTER,
                                    &snippet.name,
                                    egui::FontId::proportional(13.0),
                                    self.theme.fg_primary,
                                );

                                // Command preview (truncated, monospace)
                                let preview = if snippet.command.len() > 60 {
                                    format!("{}...", &snippet.command[..60])
                                } else {
                                    snippet.command.clone()
                                };
                                let preview = preview.replace('\n', " \\ ");
                                ui.painter().text(
                                    egui::pos2(rect.min.x + 46.0, rect.min.y + 34.0),
                                    egui::Align2::LEFT_CENTER,
                                    &preview,
                                    egui::FontId::monospace(10.0),
                                    self.theme.fg_dim,
                                );

                                // Right side: action buttons
                                let visible_right = ui.clip_rect().max.x;
                                let pointer_pos = ui.ctx().input(|i| i.pointer.hover_pos());

                                // Action buttons (only on hover)
                                if hovered {
                                    // Edit button only (run via Cmd+Shift+S in terminal)
                                    let edit_rect = egui::Rect::from_center_size(
                                        egui::pos2(visible_right - 40.0, rect.center().y),
                                        egui::vec2(52.0, 24.0),
                                    );
                                    let edit_hovered = pointer_pos.map_or(false, |p| edit_rect.contains(p));
                                    ui.painter().rect(
                                        edit_rect,
                                        RADIUS_SM,
                                        if edit_hovered {
                                            self.theme.hover_bg
                                        } else {
                                            self.theme.bg_elevated
                                        },
                                        egui::Stroke::new(1.0, self.theme.border),
                                    );
                                    ui.painter().text(
                                        edit_rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        self.language.t("edit_file"),
                                        egui::FontId::proportional(FONT_XS),
                                        self.theme.fg_dim,
                                    );

                                    // Handle clicks
                                    if let Some(pos) = pointer_pos {
                                        if resp.clicked() && edit_rect.contains(pos) {
                                            self.snippet_view_state.open_edit(
                                                snippet.id.clone(),
                                                &snippet.name,
                                                &snippet.command,
                                                &snippet.group
                                            );
                                        }
                                    }
                                }

                                ui.add_space(4.0);
                            }
                            ui.add_space(12.0);
                        }

                        ui.add_space(40.0);
                    });
            });

        // ── Show drawer for add/edit snippet ──
        if self.snippet_view_state.open {
            self.show_add_snippet_drawer(ctx, &mut snippet_to_create, &mut snippet_to_save);
        }

        // ── Delete confirmation dialog ──
        if let Some(delete_id) = &self.snippet_view_state.confirm_delete.clone() {
            let snippet_name = self.snippets.iter()
                .find(|s| s.id == *delete_id)
                .map(|s| s.name.clone())
                .unwrap_or_default();
            let mut open = true;
            egui::Window::new(self.language.t("delete"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .fixed_size([340.0, 0.0])
                .title_bar(false)
                .frame(egui::Frame {
                    fill: self.theme.bg_secondary,
                    rounding: egui::Rounding::same(8.0),
                    inner_margin: egui::Margin::same(20.0),
                    stroke: egui::Stroke::new(1.0, self.theme.border),
                    shadow: egui::epaint::Shadow {
                        offset: egui::vec2(0.0, 4.0),
                        blur: 20.0,
                        spread: 2.0,
                        color: egui::Color32::from_black_alpha(80),
                    },
                    ..Default::default()
                })
                .show(ctx, |ui| {
                    // Warning icon + title
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("\u{26A0}").size(18.0).color(self.theme.red));
                        ui.add_space(SPACE_XS);
                        ui.label(egui::RichText::new(self.language.t("delete")).size(15.0).color(self.theme.fg_primary).strong());
                    });
                    ui.add_space(10.0);

                    ui.label(
                        egui::RichText::new(self.language.tf("delete_confirm", &snippet_name))
                            .color(self.theme.fg_primary).size(FONT_BASE)
                    );
                    ui.add_space(SPACE_XS);
                    ui.label(
                        egui::RichText::new(self.language.t("confirm_delete"))
                            .color(self.theme.fg_dim).size(FONT_SM)
                    );
                    ui.add_space(SPACE_LG);

                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(widgets::danger_button(self.language.t("delete"), &self.theme)).clicked() {
                                snippet_to_delete = Some(delete_id.clone());
                            }
                            if ui.add(widgets::secondary_button(self.language.t("cancel"), &self.theme)).clicked() {
                                self.snippet_view_state.confirm_delete = None;
                            }
                        });
                    });
                });

            if !open {
                self.snippet_view_state.confirm_delete = None;
            }
        }

        // ── Apply deferred actions ──
        if let Some(snippet) = snippet_to_create {
            self.snippets.push(snippet);
            config::save_snippets(&self.snippets);
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
    }

    /// Right-side drawer for adding / editing a snippet (shown in Snippets view)
    pub fn show_add_snippet_drawer(
        &mut self,
        ctx: &egui::Context,
        snippet_to_create: &mut Option<Snippet>,
        snippet_to_save: &mut Option<Snippet>,
    ) {
        let theme = self.theme.clone();
        let lang = self.language;
        let drawer_width = ctx.screen_rect().width().min(DRAWER_WIDTH).max(280.0);

        let drawer_title = if self.snippet_view_state.editing.is_some() {
            lang.t("edit_snippet")
        } else {
            lang.t("new_snippet")
        };

        egui::SidePanel::right("snippet_drawer")
            .exact_width(drawer_width)
            .resizable(false)
            .frame(egui::Frame {
                fill: theme.bg_secondary,
                inner_margin: egui::Margin::same(20.0),
                stroke: egui::Stroke::new(1.0, theme.border),
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(drawer_title)
                        .color(theme.fg_primary)
                        .size(FONT_BASE)
                        .strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(
                            egui::Button::new(egui::RichText::new("\u{2715}").color(theme.fg_dim).size(FONT_MD))
                                .frame(false)
                        ).clicked() {
                            self.snippet_view_state.open = false;
                            self.snippet_view_state.editing = None;
                        }
                        if self.snippet_view_state.editing.is_some() {
                            if ui.add(
                                egui::Button::new(egui::RichText::new("\u{1F5D1}").size(FONT_BASE))
                                    .frame(false)
                            ).on_hover_text(lang.t("delete"))
                            .clicked() {
                                // Set confirm_delete to the editing snippet id and close drawer
                                if let Some(editing_id) = &self.snippet_view_state.editing {
                                    self.snippet_view_state.confirm_delete = Some(editing_id.clone());
                                }
                                self.snippet_view_state.open = false;
                            }
                        }
                    });
                });
                ui.add_space(SPACE_LG);

                // Name field
                ui.label(widgets::field_label(lang.t("snippet_name"), &theme));
                ui.add_space(SPACE_XS);
                ui.add(egui::TextEdit::singleline(&mut self.snippet_view_state.new_name)
                    .desired_width(f32::INFINITY)
                    .hint_text(lang.t("snippet_name"))
                    .font(egui::FontId::proportional(FONT_MD)));
                ui.add_space(SPACE_MD);

                // Group field
                ui.label(widgets::field_label(lang.t("snippet_group"), &theme));
                ui.add_space(SPACE_XS);
                ui.add(egui::TextEdit::singleline(&mut self.snippet_view_state.new_group)
                    .desired_width(f32::INFINITY)
                    .hint_text(lang.t("snippet_group"))
                    .font(egui::FontId::proportional(FONT_MD)));
                ui.add_space(SPACE_MD);

                // Command field
                ui.label(widgets::field_label(lang.t("snippet_command"), &theme));
                ui.add_space(SPACE_XS);
                ui.add(egui::TextEdit::multiline(&mut self.snippet_view_state.new_command)
                    .desired_width(f32::INFINITY)
                    .desired_rows(6)
                    .hint_text(lang.t("snippet_command"))
                    .font(egui::FontId::monospace(FONT_MD)));
                ui.add_space(SPACE_LG);

                // Action buttons
                ui.horizontal(|ui| {
                    // Save button
                    let can_save = !self.snippet_view_state.new_name.trim().is_empty();
                    if ui.add_enabled(
                        can_save,
                        widgets::primary_button(lang.t("save"), &theme)
                    ).clicked() && can_save {
                        let group = if self.snippet_view_state.new_group.trim().is_empty() {
                            lang.t("snippet_default_group").to_string()
                        } else {
                            self.snippet_view_state.new_group.trim().to_string()
                        };

                        if let Some(edit_id) = &self.snippet_view_state.editing {
                            // Update existing snippet
                            *snippet_to_save = Some(Snippet {
                                id: edit_id.clone(),
                                name: self.snippet_view_state.new_name.trim().to_string(),
                                command: self.snippet_view_state.new_command.clone(),
                                group,
                            });
                        } else {
                            // Create new snippet
                            *snippet_to_create = Some(Snippet {
                                id: Uuid::new_v4().to_string(),
                                name: self.snippet_view_state.new_name.trim().to_string(),
                                command: self.snippet_view_state.new_command.clone(),
                                group,
                            });
                        }
                        self.snippet_view_state.open = false;
                    }

                    ui.add_space(SPACE_SM);

                    // Cancel button
                    if ui.add(widgets::secondary_button(lang.t("cancel"), &theme)).clicked() {
                        self.snippet_view_state.open = false;
                        self.snippet_view_state.editing = None;
                    }
                });
            });
    }

    /// Collect all unique group names from snippets
    fn collect_snippet_groups(&self) -> Vec<String> {
        let mut groups = std::collections::BTreeSet::new();
        for snippet in &self.snippets {
            if !snippet.group.is_empty() {
                groups.insert(snippet.group.clone());
            }
        }
        groups.into_iter().collect()
    }

    /// Show snippet run drawer (triggered from terminal view with Cmd+Shift+S)
    /// Right-side drawer with clickable snippet list
    pub fn show_snippet_run_drawer(&mut self, ctx: &egui::Context) {
        let drawer_width = ctx.screen_rect().width().min(DRAWER_WIDTH).max(280.0);
        let theme = self.theme.clone();
        let lang = self.language;

        egui::SidePanel::right("snippet_run_drawer")
            .exact_width(drawer_width)
            .resizable(false)
            .frame(egui::Frame {
                fill: theme.bg_secondary,
                inner_margin: egui::Margin::same(20.0),
                stroke: egui::Stroke::new(1.0, theme.border),
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    // Header with title and close button
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("\u{26A1}") // Lightning bolt
                            .size(18.0)
                            .color(theme.accent));
                        ui.add_space(SPACE_XS);
                        ui.label(egui::RichText::new(lang.t("run_snippet"))
                            .color(theme.fg_primary)
                            .size(FONT_BASE)
                            .strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(
                                egui::Button::new(egui::RichText::new("\u{2715}").color(theme.fg_dim).size(FONT_MD))
                                    .frame(false)
                            ).clicked() {
                                self.snippet_view_state.quick_selector_open = false;
                                self.snippet_view_state.selected_snippet_index = None;
                            }
                        });
                    });
                    ui.add_space(SPACE_MD);

                    // Snippet list
                    if self.snippets.is_empty() {
                        ui.vertical_centered(|ui| {
                            ui.add_space(SPACE_XL);
                            ui.label(egui::RichText::new(lang.t("no_snippets"))
                                .color(theme.fg_dim)
                                .size(FONT_SM));
                        });
                    } else {
                        egui::ScrollArea::vertical()
                            .id_salt("snippet_run_drawer_scroll")
                            .show(ui, |ui| {
                                // Group snippets
                                let mut groups: std::collections::BTreeMap<String, Vec<(usize, Snippet)>> =
                                    std::collections::BTreeMap::new();
                                for (idx, snippet) in self.snippets.iter().enumerate() {
                                    groups.entry(snippet.group.clone()).or_default().push((idx, snippet.clone()));
                                }

                                for (group_name, snippets_in_group) in &groups {
                                    // Group header
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(group_name.to_uppercase())
                                            .color(theme.fg_dim)
                                            .size(10.0)
                                            .strong());
                                    });
                                    ui.add_space(SPACE_XS);

                                    for (_idx, snippet) in snippets_in_group {
                                        let item_height = 52.0;
                                        let (rect, resp) = ui.allocate_exact_size(
                                            egui::vec2(ui.available_width(), item_height),
                                            egui::Sense::click(),
                                        );

                                        // Hover background
                                        if resp.hovered() {
                                            ui.painter().rect_filled(rect, RADIUS_SM, theme.hover_bg);
                                        }

                                        // Icon
                                        ui.painter().text(
                                            egui::pos2(rect.min.x + 4.0, rect.min.y + 14.0),
                                            egui::Align2::LEFT_TOP,
                                            "\u{26A1}",
                                            egui::FontId::proportional(12.0),
                                            theme.accent,
                                        );

                                        // Name
                                        ui.painter().text(
                                            egui::pos2(rect.min.x + 24.0, rect.min.y + 12.0),
                                            egui::Align2::LEFT_TOP,
                                            &snippet.name,
                                            egui::FontId::proportional(FONT_SM),
                                            theme.fg_primary,
                                        );

                                        // Command preview
                                        let preview = if snippet.command.len() > 30 {
                                            format!("{}...", &snippet.command[..30])
                                        } else {
                                            snippet.command.clone()
                                        };
                                        let preview = preview.replace('\n', r" \ ");
                                        ui.painter().text(
                                            egui::pos2(rect.min.x + 24.0, rect.min.y + 30.0),
                                            egui::Align2::LEFT_TOP,
                                            &preview,
                                            egui::FontId::monospace(10.0),
                                            theme.fg_dim,
                                        );

                                        // Handle click - execute immediately
                                        if resp.clicked() {
                                            // Execute on current tab's sessions
                                            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                                                if tab.broadcast_enabled {
                                                    // Broadcast mode: send to all sessions
                                                    for session in &mut tab.sessions {
                                                        session.write(&format!("{}\n", snippet.command));
                                                    }
                                                } else {
                                                    // Normal mode: send only to focused session
                                                    if let Some(session) = tab.sessions.get_mut(tab.focused_session) {
                                                        session.write(&format!("{}\n", snippet.command));
                                                    }
                                                }
                                            }
                                            // Keep drawer open for more selections
                                        }

                                        ui.add_space(SPACE_XS);
                                    }
                                    ui.add_space(SPACE_SM);
                                }
                            });

                        // Footer hint
                        ui.add_space(SPACE_SM);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Click to run")
                                .size(11.0)
                                .color(theme.fg_dim));
                        });
                    }
                });
            });
    }

    /// Filter snippets based on current group filter
    fn filter_snippets(&self) -> Vec<Snippet> {
        self.snippets
            .iter()
            .filter(|s| {
                if self.snippet_view_state.group_filter.is_empty() {
                    return true;
                }
                s.group == self.snippet_view_state.group_filter
            })
            .cloned()
            .collect()
    }
}
