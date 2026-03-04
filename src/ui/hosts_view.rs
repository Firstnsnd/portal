use eframe::egui;
use egui::Widget;
use std::sync::{Arc, Mutex};

use crate::PortalApp;
use crate::config::HostEntry;
use crate::ssh::test_connection;
use crate::ui::types::{AuthMethodChoice, TestConnState, AppView, KeySourceChoice, BatchTarget, BatchStatus, BatchResult, BatchUpdate};

impl PortalApp {
    /// Navigation strip on the left (always visible)
    pub fn show_nav_panel(&mut self, ctx: &egui::Context) {
        let theme = self.theme.clone();
        let language = self.language;
        let nav_width = (ctx.screen_rect().width() * 0.14).min(200.0).max(150.0);
        egui::SidePanel::left("nav")
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

                let nav_btn = |ui: &mut egui::Ui, icon: &str, label: &str, active: bool| -> bool {
                    let width = ui.available_width();
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(width, 42.0),
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
                    ui.painter().rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(rect.min.x, rect.max.y - 1.0),
                            rect.max,
                        ),
                        0.0,
                        shadow_color,
                    );
                    ui.painter().rect_filled(rect, 0.0, bg);
                    if active {
                        ui.painter().rect_filled(
                            egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + 3.0, rect.max.y)),
                            egui::Rounding { nw: 0.0, ne: 2.0, sw: 0.0, se: 2.0 },
                            theme.accent,
                        );
                    }
                    let color = if active { theme.fg_primary } else if resp.hovered() { theme.fg_primary } else { theme.fg_dim };
                    ui.painter().text(
                        egui::pos2(rect.min.x + 16.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        format!("{}  {}", icon, label),
                        egui::FontId::proportional(14.0),
                        color,
                    );
                    resp.clicked()
                };

                if nav_btn(ui, "☰", language.t("hosts"), self.current_view == AppView::Hosts) {
                    self.current_view = AppView::Hosts;
                }
                if nav_btn(ui, ">_", language.t("terminal"), self.current_view == AppView::Terminal) {
                    self.current_view = AppView::Terminal;
                }
                if nav_btn(ui, "\u{2195}", language.t("sftp"), self.current_view == AppView::Sftp) {
                    self.current_view = AppView::Sftp;
                }
                if nav_btn(ui, "⚡", "批量执行", self.current_view == AppView::Batch) {
                    self.current_view = AppView::Batch;
                }
                if nav_btn(ui, "\u{1f511}", language.t("keychain"), self.current_view == AppView::Keychain) {
                    self.current_view = AppView::Keychain;
                }

                // Settings button at bottom
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.add_space(8.0);
                    if nav_btn(ui, "⚙", language.t("settings"), self.current_view == AppView::Settings) {
                        self.current_view = AppView::Settings;
                    }
                });
            });
    }

    /// Full hosts page content (used by both main and detached windows)
    pub fn show_hosts_page(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        ui.add_space(0.0);
        let mut new_local_session = false;
        let mut connect_ssh_host: Option<usize> = None;
        let mut edit_host_index: Option<usize> = None;

        // Collect all unique groups and tags
        let mut all_groups: Vec<String> = Vec::new();
        let mut all_tags: Vec<String> = Vec::new();
        for host in &self.hosts {
            if !host.is_local {
                if !host.group.is_empty() && !all_groups.contains(&host.group) {
                    all_groups.push(host.group.clone());
                }
                for tag in &host.tags {
                    if !all_tags.contains(tag) {
                        all_tags.push(tag.clone());
                    }
                }
            }
        }
        all_groups.sort();
        all_tags.sort();

        // Top navigation bar (matching terminal tab bar style)
        egui::TopBottomPanel::top("hosts_nav_bar")
            .frame(egui::Frame {
                fill: self.theme.bg_secondary,
                inner_margin: egui::Margin::symmetric(16.0, 8.0),
                stroke: egui::Stroke::NONE,
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Left side: Hosts title
                    ui.label(egui::RichText::new(self.language.t("hosts")).color(self.theme.fg_dim).size(13.0).strong());

                    // Right side: New Host button
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(
                            egui::Button::new(egui::RichText::new(self.language.t("new_host")).color(self.theme.accent).size(12.0))
                                .frame(false)
                        ).clicked() {
                            self.add_host_dialog.open_new();
                        }
                    });
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame {
                fill: self.theme.bg_primary,
                ..Default::default()
            })
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("hosts_page_scroll")
                    .show(ui, |ui| {
                        ui.add_space(12.0);

                        // Filter bar (in content area, right-aligned with margin)
                        ui.horizontal(|ui| {
                            // Add left space to push content to the right
                            ui.add_space(ui.available_width() - 210.0); // Adjusted for narrower filters

                            // Group filter dropdown
                            let group_label = if self.host_filter.group.is_empty() {
                                "Group".to_string()
                            } else {
                                self.host_filter.group.clone()
                            };
                            egui::ComboBox::from_id_salt("group_filter")
                                .selected_text(egui::RichText::new(group_label).color(self.theme.accent).size(12.0))
                                .width(90.0)
                                .show_ui(ui, |ui| {
                                    ui.style_mut().visuals.widgets.inactive.bg_fill = self.theme.bg_elevated;
                                    ui.style_mut().visuals.widgets.hovered.bg_fill = self.theme.bg_primary;
                                    ui.style_mut().visuals.selection.bg_fill = self.theme.accent_alpha(30);
                                    // Hide checkbox for selected items
                                    ui.style_mut().visuals.selection.stroke = egui::Stroke::NONE;

                                    // All option
                                    if egui::Button::new(
                                        egui::RichText::new("All")
                                            .color(if self.host_filter.group.is_empty() { self.theme.accent } else { self.theme.fg_primary })
                                            .size(12.0)
                                    ).frame(false).ui(ui).clicked() {
                                        self.host_filter.group.clear();
                                        ui.close_menu();
                                    }

                                    for group in &all_groups {
                                        if egui::Button::new(
                                            egui::RichText::new(group)
                                                .color(if self.host_filter.group == *group { self.theme.accent } else { self.theme.fg_primary })
                                                .size(12.0)
                                        ).frame(false).ui(ui).clicked() {
                                            self.host_filter.group = group.clone();
                                            ui.close_menu();
                                        }
                                    }
                                });

                            ui.add_space(6.0);

                            // Tag filter dropdown
                            let tag_label = if self.host_filter.tag.is_empty() {
                                "Tag".to_string()
                            } else {
                                self.host_filter.tag.clone()
                            };
                            egui::ComboBox::from_id_salt("tag_filter")
                                .selected_text(egui::RichText::new(tag_label).color(self.theme.accent).size(12.0))
                                .width(90.0)
                                .show_ui(ui, |ui| {
                                    ui.style_mut().visuals.widgets.inactive.bg_fill = self.theme.bg_elevated;
                                    ui.style_mut().visuals.widgets.hovered.bg_fill = self.theme.bg_primary;
                                    ui.style_mut().visuals.selection.bg_fill = self.theme.accent_alpha(30);
                                    // Hide checkbox for selected items
                                    ui.style_mut().visuals.selection.stroke = egui::Stroke::NONE;

                                    // All option
                                    if egui::Button::new(
                                        egui::RichText::new("All")
                                            .color(if self.host_filter.tag.is_empty() { self.theme.accent } else { self.theme.fg_primary })
                                            .size(12.0)
                                    ).frame(false).ui(ui).clicked() {
                                        self.host_filter.tag.clear();
                                        ui.close_menu();
                                    }

                                    for tag in &all_tags {
                                        if egui::Button::new(
                                            egui::RichText::new(tag)
                                                .color(if self.host_filter.tag == *tag { self.theme.accent } else { self.theme.fg_primary })
                                                .size(12.0)
                                        ).frame(false).ui(ui).clicked() {
                                            self.host_filter.tag = tag.clone();
                                            ui.close_menu();
                                        }
                                    }
                                });

                            ui.add_space(8.0);

                            // Clear button
                            if self.host_filter.is_active() {
                                if ui.add(
                                    egui::Button::new(egui::RichText::new("Clear").color(self.theme.accent).size(12.0))
                                        .frame(false)
                                ).clicked() {
                                    self.host_filter.clear();
                                }
                            }
                        });

                        ui.add_space(12.0);

            // LOCAL section
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                ui.label(egui::RichText::new(self.language.t("local")).color(self.theme.fg_dim).size(10.0).strong());
            });
            ui.add_space(4.0);

            for (_i, host) in self.hosts.iter().enumerate() {
                if !host.is_local { continue; }
                // Apply filter (local hosts only filtered by search if we had search)
                let width = ui.available_width();
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(width, 36.0),
                    egui::Sense::click(),
                );
                if resp.hovered() {
                    ui.painter().rect_filled(
                        egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.max.y - 1.0), rect.max),
                        0.0, self.theme.hover_shadow,
                    );
                    ui.painter().rect_filled(rect, 0.0, self.theme.hover_bg);
                }
                ui.painter().text(
                    egui::pos2(rect.min.x + 24.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    ">_",
                    egui::FontId::proportional(12.0),
                    self.theme.green,
                );
                ui.painter().text(
                    egui::pos2(rect.min.x + 48.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    &host.name,
                    egui::FontId::proportional(13.0),
                    self.theme.fg_primary,
                );
                if resp.clicked() {
                    new_local_session = true;
                }
            }

            ui.add_space(20.0);

            // SSH HOSTS section
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                ui.label(egui::RichText::new(self.language.t("ssh_hosts")).color(self.theme.fg_dim).size(10.0).strong());
            });
            ui.add_space(4.0);

            // Collect groups (from filtered hosts)
            let mut groups: Vec<String> = Vec::new();
            for host in &self.hosts {
                if !host.is_local && !host.group.is_empty() && !groups.contains(&host.group) {
                    // Apply filter to group collection
                    if self.host_filter.matches(host) {
                        groups.push(host.group.clone());
                    }
                }
            }

            // Ungrouped SSH hosts
            for (i, host) in self.hosts.iter().enumerate() {
                if host.is_local || !host.group.is_empty() { continue; }
                // Apply filter
                if !self.host_filter.matches(host) {
                    continue;
                }
                let row_h = 44.0;
                let width = ui.available_width();
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(width, row_h),
                    egui::Sense::click(),
                );
                let hovered = resp.hovered();
                if hovered {
                    ui.painter().rect_filled(
                        egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.max.y - 1.0), rect.max),
                        0.0, self.theme.hover_shadow,
                    );
                    ui.painter().rect_filled(rect, 0.0, self.theme.hover_bg);
                }
                // Icon
                ui.painter().text(
                    egui::pos2(rect.min.x + 24.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    "@",
                    egui::FontId::proportional(12.0),
                    self.theme.accent,
                );
                // Host name
                ui.painter().text(
                    egui::pos2(rect.min.x + 46.0, rect.center().y - 7.0),
                    egui::Align2::LEFT_CENTER,
                    &host.name,
                    egui::FontId::proportional(13.0),
                    self.theme.fg_primary,
                );
                // Detail
                let detail = if host.username.is_empty() {
                    format!("{}:{}", host.host, host.port)
                } else {
                    format!("{}@{}:{}", host.username, host.host, host.port)
                };
                ui.painter().text(
                    egui::pos2(rect.min.x + 46.0, rect.center().y + 8.0),
                    egui::Align2::LEFT_CENTER,
                    detail,
                    egui::FontId::proportional(10.0),
                    self.theme.fg_dim,
                );

                // Tags (subtle, italic, displayed below host info)
                if !host.tags.is_empty() {
                    let tag_text = host.tags.join(", ");
                    // Use a small area to position the tag label
                    let tag_rect = egui::Rect::from_min_max(
                        egui::pos2(rect.min.x + 46.0, rect.max.y - 11.0),
                        egui::pos2(rect.max.x, rect.max.y - 2.0),
                    );
                    let _ = ui.allocate_ui_at_rect(tag_rect, |ui| {
                        ui.label(egui::RichText::new(format!("• {}", tag_text))
                            .italics()
                            .size(9.0)
                            .color(self.theme.fg_dim)
                        );
                    });
                }
                // Edit button (only visible on hover)
                // Use screen right edge for consistent positioning regardless of filter
                let screen_right = ui.ctx().input(|i| i.screen_rect().max.x);
                let btn_rect = egui::Rect::from_center_size(
                    egui::pos2(screen_right - 70.0, rect.center().y),
                    egui::vec2(56.0, 22.0),
                );
                if hovered {
                    let pointer_pos = ui.ctx().input(|i| i.pointer.hover_pos());
                    let over_btn = pointer_pos.map_or(false, |p| btn_rect.contains(p));
                    let btn_bg = if over_btn { self.theme.accent } else { self.theme.bg_elevated };
                    let btn_text_color = if over_btn { self.theme.bg_primary } else { self.theme.accent };
                    ui.painter().rect(btn_rect, 4.0, btn_bg, egui::Stroke::new(1.0, self.theme.accent));
                    ui.painter().text(
                        btn_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        self.language.t("edit_file"),
                        egui::FontId::proportional(11.0),
                        btn_text_color,
                    );
                }
                if resp.double_clicked() {
                    connect_ssh_host = Some(i);
                } else if resp.clicked() {
                    let click_pos = ui.ctx().input(|i| i.pointer.interact_pos());
                    if click_pos.map_or(false, |p| btn_rect.contains(p)) {
                        edit_host_index = Some(i);
                    }
                }
            }

            // Grouped SSH hosts
            for group in &groups {
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.label(egui::RichText::new(group).color(self.theme.fg_dim).size(10.0));
                });
                ui.add_space(2.0);
                for (i, host) in self.hosts.iter().enumerate() {
                    if host.is_local || host.group != *group { continue; }
                    // Apply filter
                    if !self.host_filter.matches(host) {
                        continue;
                    }
                    let row_h = 44.0;
                    let width = ui.available_width();
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(width, row_h),
                        egui::Sense::click(),
                    );
                    let hovered = resp.hovered();
                    if hovered {
                        ui.painter().rect_filled(
                            egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.max.y - 1.0), rect.max),
                            0.0, self.theme.hover_shadow,
                        );
                        ui.painter().rect_filled(rect, 0.0, self.theme.hover_bg);
                    }
                    ui.painter().text(
                        egui::pos2(rect.min.x + 24.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        "@",
                        egui::FontId::proportional(12.0),
                        self.theme.accent,
                    );
                    ui.painter().text(
                        egui::pos2(rect.min.x + 46.0, rect.center().y - 7.0),
                        egui::Align2::LEFT_CENTER,
                        &host.name,
                        egui::FontId::proportional(13.0),
                        self.theme.fg_primary,
                    );
                    let detail = if host.username.is_empty() {
                        format!("{}:{}", host.host, host.port)
                    } else {
                        format!("{}@{}:{}", host.username, host.host, host.port)
                    };
                    ui.painter().text(
                        egui::pos2(rect.min.x + 46.0, rect.center().y + 8.0),
                        egui::Align2::LEFT_CENTER,
                        detail,
                        egui::FontId::proportional(10.0),
                        self.theme.fg_dim,
                    );

                    // Tags (subtle, italic, displayed below host info)
                    if !host.tags.is_empty() {
                        let tag_text = host.tags.join(", ");
                        // Use a small area to position the tag label
                        let tag_rect = egui::Rect::from_min_max(
                            egui::pos2(rect.min.x + 46.0, rect.max.y - 11.0),
                            egui::pos2(rect.max.x, rect.max.y - 2.0),
                        );
                        let _ = ui.allocate_ui_at_rect(tag_rect, |ui| {
                            ui.label(egui::RichText::new(format!("• {}", tag_text))
                                .italics()
                                .size(9.0)
                                .color(self.theme.fg_dim)
                            );
                        });
                    }
                    // Edit button (only visible on hover)
                    // Use screen right edge for consistent positioning regardless of filter
                    let screen_right = ui.ctx().input(|i| i.screen_rect().max.x);
                    let btn_rect = egui::Rect::from_center_size(
                        egui::pos2(screen_right - 70.0, rect.center().y),
                        egui::vec2(56.0, 22.0),
                    );
                    if hovered {
                        let pointer_pos = ui.ctx().input(|i| i.pointer.hover_pos());
                        let over_btn = pointer_pos.map_or(false, |p| btn_rect.contains(p));
                        let btn_bg = if over_btn { self.theme.accent } else { self.theme.bg_elevated };
                        let btn_text_color = if over_btn { self.theme.bg_primary } else { self.theme.accent };
                        ui.painter().rect(btn_rect, 4.0, btn_bg, egui::Stroke::new(1.0, self.theme.accent));
                        ui.painter().text(
                            btn_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            self.language.t("edit_file"),
                            egui::FontId::proportional(11.0),
                            btn_text_color,
                        );
                    }
                    if resp.double_clicked() {
                        connect_ssh_host = Some(i);
                    } else if resp.clicked() {
                        let click_pos = ui.ctx().input(|i| i.pointer.interact_pos());
                        if click_pos.map_or(false, |p| btn_rect.contains(p)) {
                            edit_host_index = Some(i);
                        }
                    }
                }
            }

            ui.add_space(20.0);
        }); // end ScrollArea
    }); // end CentralPanel

        // Deferred actions (after borrows of self.hosts are done)
        if new_local_session {
            self.add_tab_local();
        }
        if let Some(idx) = connect_ssh_host {
            let host = self.hosts[idx].clone();
            self.add_tab_ssh(&host);
        }
        if let Some(idx) = edit_host_index {
            let host = self.hosts[idx].clone();
            self.add_host_dialog.open_edit(idx, &host);
        }
    }

    /// Right-side drawer for adding / editing a host (shown in Hosts view)
    pub fn show_add_host_drawer(&mut self, ctx: &egui::Context) {
        if !self.add_host_dialog.open {
            return;
        }

        // Poll the async test result
        let polled_result: Option<Result<String, String>> = self
            .add_host_dialog
            .test_conn_result
            .as_ref()
            .and_then(|arc| arc.lock().ok()?.take());
        if let Some(result) = polled_result {
            self.add_host_dialog.test_conn_state = match result {
                Ok(msg) => TestConnState::Success(msg),
                Err(msg) => TestConnState::Failed(msg),
            };
            self.add_host_dialog.test_conn_result = None;
        }

        let theme = self.theme.clone();
        let lang = self.language;
        let drawer_title = if self.add_host_dialog.edit_index.is_some() {
            lang.t("edit_host")
        } else {
            lang.t("new_host_title")
        };

        let mut save_clicked = false;
        let mut test_clicked = false;

        egui::SidePanel::right("add_host_drawer")
            .exact_width(340.0)
            .resizable(false)
            .frame(egui::Frame {
                fill: theme.bg_secondary,
                inner_margin: egui::Margin::same(0.0),
                stroke: egui::Stroke::NONE,
                ..Default::default()
            })
            .show(ctx, |ui| {
                egui::Frame {
                    fill: theme.bg_elevated,
                    inner_margin: egui::Margin::symmetric(20.0, 12.0),
                    ..Default::default()
                }
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(drawer_title).color(theme.fg_primary).size(16.0).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(
                                egui::Button::new(egui::RichText::new("✕").color(theme.fg_dim).size(16.0))
                                    .frame(false)
                            ).clicked() {
                                self.add_host_dialog.reset();
                            }
                            if self.add_host_dialog.edit_index.is_some() {
                                if ui.add(
                                    egui::Button::new(egui::RichText::new("\u{1F5D1}").size(14.0))
                                        .frame(false)
                                ).on_hover_text(lang.t("delete"))
                                .clicked() {
                                    self.confirm_delete_host = self.add_host_dialog.edit_index;
                                    self.add_host_dialog.reset();
                                }
                            }
                        });
                    });
                });

                ui.separator();

                egui::ScrollArea::vertical()
                    .id_salt("drawer_scroll")
                    .show(ui, |ui| {
                    ui.add_space(4.0);
                    egui::Frame {
                        inner_margin: egui::Margin::symmetric(20.0, 12.0),
                        ..Default::default()
                    }
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 8.0;

                        ui.label(egui::RichText::new(lang.t("label")).color(theme.fg_dim).size(12.0));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.add_host_dialog.name)
                                .hint_text(egui::RichText::new("My Server").color(theme.hint_color()).italics())
                                .desired_width(f32::INFINITY)
                                .text_color(theme.fg_primary)
                        );

                        ui.add_space(4.0);

                        ui.label(egui::RichText::new(lang.t("host_ip")).color(theme.fg_dim).size(12.0));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.add_host_dialog.host)
                                .hint_text(egui::RichText::new("192.168.1.1").color(theme.hint_color()).italics())
                                .desired_width(f32::INFINITY)
                                .text_color(theme.fg_primary)
                        );

                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(lang.t("port")).color(theme.fg_dim).size(12.0));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.add_host_dialog.port)
                                        .desired_width(70.0)
                                        .text_color(theme.fg_primary)
                                );
                            });
                            ui.add_space(8.0);
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(lang.t("group")).color(theme.fg_dim).size(12.0));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.add_host_dialog.group)
                                        .hint_text(egui::RichText::new("Production").color(theme.hint_color()).italics())
                                        .desired_width(f32::INFINITY)
                                        .text_color(theme.fg_primary)
                                );
                            });
                        });

                        ui.add_space(8.0);

                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new("Tags").color(theme.fg_dim).size(12.0));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.add_host_dialog.tags)
                                    .hint_text(egui::RichText::new("web, database, production").color(theme.hint_color()).italics())
                                    .desired_width(f32::INFINITY)
                                    .text_color(theme.fg_primary)
                            );
                        });

                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(8.0);

                        ui.label(egui::RichText::new(lang.t("authentication")).color(theme.fg_dim).size(12.0));
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let pw_color = if self.add_host_dialog.auth_method == AuthMethodChoice::Password { theme.accent } else { theme.fg_primary };
                            ui.selectable_value(
                                &mut self.add_host_dialog.auth_method,
                                AuthMethodChoice::Password,
                                egui::RichText::new(lang.t("password")).color(pw_color).size(12.0),
                            );
                            let key_color = if self.add_host_dialog.auth_method == AuthMethodChoice::Key { theme.accent } else { theme.fg_primary };
                            ui.selectable_value(
                                &mut self.add_host_dialog.auth_method,
                                AuthMethodChoice::Key,
                                egui::RichText::new(lang.t("ssh_key")).color(key_color).size(12.0),
                            );
                        });

                        ui.add_space(4.0);

                        match self.add_host_dialog.auth_method {
                            AuthMethodChoice::Password => {
                                ui.label(egui::RichText::new(lang.t("username")).color(theme.fg_dim).size(12.0));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.add_host_dialog.username)
                                        .hint_text(egui::RichText::new("root").color(theme.hint_color()).italics())
                                        .desired_width(f32::INFINITY)
                                        .text_color(theme.fg_primary)
                                );
                                ui.add_space(4.0);
                                ui.label(egui::RichText::new(lang.t("password")).color(theme.fg_dim).size(12.0));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.add_host_dialog.password)
                                        .password(true)
                                        .hint_text(egui::RichText::new("Enter password").color(theme.hint_color()).italics())
                                        .desired_width(f32::INFINITY)
                                        .text_color(theme.fg_primary)
                                );
                            }
                            AuthMethodChoice::Key => {
                                // Key source selection: Local file vs Import content
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Key Source:").color(theme.fg_dim).size(12.0));
                                    ui.add_space(8.0);

                                    let local_color = if self.add_host_dialog.key_source == KeySourceChoice::LocalFile { theme.accent } else { theme.fg_dim };
                                    if ui.add(
                                        egui::Button::new(
                                            egui::RichText::new("Local File").color(local_color).size(12.0)
                                        )
                                        .stroke(egui::Stroke::NONE)
                                        .fill(egui::Color32::TRANSPARENT)
                                    ).clicked() {
                                        self.add_host_dialog.key_source = KeySourceChoice::LocalFile;
                                    }

                                    ui.add_space(8.0);

                                    let import_color = if self.add_host_dialog.key_source == KeySourceChoice::ImportContent { theme.accent } else { theme.fg_dim };
                                    if ui.add(
                                        egui::Button::new(
                                            egui::RichText::new("Import Content").color(import_color).size(12.0)
                                        )
                                        .stroke(egui::Stroke::NONE)
                                        .fill(egui::Color32::TRANSPARENT)
                                    ).clicked() {
                                        self.add_host_dialog.key_source = KeySourceChoice::ImportContent;
                                    }
                                });
                                ui.add_space(8.0);

                                match self.add_host_dialog.key_source {
                                    KeySourceChoice::LocalFile => {
                                        ui.label(egui::RichText::new(lang.t("key_path")).color(theme.fg_dim).size(12.0));
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.add_host_dialog.key_path)
                                                .hint_text(egui::RichText::new("~/.ssh/id_rsa").color(theme.hint_color()).italics())
                                                .desired_width(f32::INFINITY)
                                                .text_color(theme.fg_primary)
                                        );
                                    }
                                    KeySourceChoice::ImportContent => {
                                        ui.label(egui::RichText::new("Private Key:").color(theme.fg_dim).size(12.0));
                                        ui.add(
                                            egui::TextEdit::multiline(&mut self.add_host_dialog.key_content)
                                                .id(egui::Id::new("import_private_key"))
                                                .hint_text(egui::RichText::new("Paste your private key content here...\n-----BEGIN OPENSSH PRIVATE KEY-----\n...\n-----END OPENSSH PRIVATE KEY-----").color(theme.hint_color()).italics())
                                                .font(egui::TextStyle::Monospace)
                                                .desired_width(f32::INFINITY)
                                                .desired_rows(6)
                                                .frame(true)
                                                .text_color(theme.fg_primary)
                                        );
                                        ui.add_space(4.0);
                                        ui.label(egui::RichText::new("Public Key (optional):").color(theme.fg_dim).size(12.0));
                                        ui.add(
                                            egui::TextEdit::multiline(&mut self.add_host_dialog.key_path)
                                                .hint_text(egui::RichText::new("ssh-rsa AAAA... user@host").color(theme.hint_color()).italics())
                                                .font(egui::TextStyle::Monospace)
                                                .desired_width(f32::INFINITY)
                                                .desired_rows(2)
                                                .frame(true)
                                                .text_color(theme.fg_primary)
                                        );
                                    }
                                }

                                if self.add_host_dialog.key_in_keychain {
                                    ui.label(egui::RichText::new(lang.t("key_stored_in_keychain")).color(theme.green).size(11.0));
                                }
                                ui.add_space(4.0);
                                ui.label(egui::RichText::new(lang.t("key_passphrase")).color(theme.fg_dim).size(12.0));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.add_host_dialog.key_passphrase)
                                        .password(true)
                                        .hint_text(egui::RichText::new("Leave empty if none").color(theme.hint_color()).italics())
                                        .desired_width(f32::INFINITY)
                                        .text_color(theme.fg_primary)
                                );
                            }
                        }

                        ui.add_space(8.0);

                        if !self.add_host_dialog.error.is_empty() {
                            ui.label(egui::RichText::new(&self.add_host_dialog.error).color(theme.red).size(12.0));
                        }

                        match &self.add_host_dialog.test_conn_state {
                            TestConnState::Idle => {}
                            TestConnState::Testing => {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label(egui::RichText::new(lang.t("testing")).color(theme.fg_dim).size(12.0));
                                });
                            }
                            TestConnState::Success(msg) => {
                                ui.label(egui::RichText::new(format!("✓ {}", msg)).color(theme.green).size(12.0));
                            }
                            TestConnState::Failed(msg) => {
                                ui.label(egui::RichText::new(format!("✗ {}", msg)).color(theme.red).size(12.0));
                            }
                        }

                        ui.add_space(16.0);
                        ui.separator();
                        ui.add_space(12.0);

                        ui.horizontal(|ui| {
                            if ui.add(
                                egui::Button::new(egui::RichText::new(lang.t("save")).color(theme.bg_primary).size(12.0))
                                    .fill(theme.accent)
                                    .rounding(4.0)
                                    .min_size(egui::vec2(70.0, 28.0))
                            ).clicked() {
                                save_clicked = true;
                            }

                            ui.add_space(8.0);

                            let is_testing = matches!(self.add_host_dialog.test_conn_state, TestConnState::Testing);
                            if ui.add_enabled(
                                !is_testing,
                                egui::Button::new(egui::RichText::new(lang.t("test")).color(theme.fg_primary).size(12.0))
                                    .fill(theme.bg_elevated)
                                    .rounding(4.0)
                                    .min_size(egui::vec2(70.0, 28.0))
                            ).clicked() {
                                test_clicked = true;
                            }
                        });
                    });
                });
            });

        if test_clicked {
            let host = self.add_host_dialog.host.trim().to_owned();
            let port: u16 = self.add_host_dialog.port.trim().parse().unwrap_or(22);
            let username = self.add_host_dialog.username.trim().to_owned();

            if host.is_empty() {
                self.add_host_dialog.error = "Host is required for testing.".to_owned();
            } else {
                let auth = match self.add_host_dialog.auth_method {
                    AuthMethodChoice::Password => {
                        let pw = self.add_host_dialog.password.clone();
                        if pw.is_empty() {
                            crate::config::AuthMethod::None
                        } else {
                            crate::config::AuthMethod::Password { password: pw }
                        }
                    }
                    AuthMethodChoice::Key => {
                        let (path, key_content) = match self.add_host_dialog.key_source {
                            KeySourceChoice::LocalFile => (
                                self.add_host_dialog.key_path.trim().to_owned(),
                                String::new()
                            ),
                            KeySourceChoice::ImportContent => (
                                String::new(), // Empty path when importing content
                                self.add_host_dialog.key_content.trim().to_owned()
                            ),
                        };
                        crate::config::AuthMethod::Key {
                            key_path: path,
                            key_content,
                            passphrase: self.add_host_dialog.key_passphrase.clone(),
                            key_in_keychain: self.add_host_dialog.key_in_keychain,
                        }
                    }
                };

                let result_arc: Arc<Mutex<Option<Result<String, String>>>> =
                    Arc::new(Mutex::new(None));
                self.add_host_dialog.test_conn_result = Some(Arc::clone(&result_arc));
                self.add_host_dialog.test_conn_state = TestConnState::Testing;
                self.add_host_dialog.error.clear();

                let name = self.add_host_dialog.name.trim().to_owned();
                self.runtime.spawn(async move {
                    let result = test_connection(host, port, username, auth, name).await;
                    if let Ok(mut guard) = result_arc.lock() {
                        *guard = Some(result);
                    }
                });
            }
        }

        if save_clicked {
            // Validate
            let name = self.add_host_dialog.name.trim().to_owned();
            let host = self.add_host_dialog.host.trim().to_owned();
            let port: u16 = self.add_host_dialog.port.trim().parse().unwrap_or(22);

            if name.is_empty() || host.is_empty() {
                self.add_host_dialog.error = "Label and Host are required.".to_owned();
                return;
            }

            let auth = match self.add_host_dialog.auth_method {
                AuthMethodChoice::Password => {
                    let pw = self.add_host_dialog.password.clone();
                    if pw.is_empty() {
                        crate::config::AuthMethod::None
                    } else {
                        crate::config::AuthMethod::Password { password: pw }
                    }
                }
                AuthMethodChoice::Key => {
                    let (path, key_content) = match self.add_host_dialog.key_source {
                        KeySourceChoice::LocalFile => {
                            let path = self.add_host_dialog.key_path.trim().to_owned();
                            if path.is_empty() {
                                self.add_host_dialog.error = "Key path is required.".to_owned();
                                return;
                            }
                            (path, String::new())
                        }
                        KeySourceChoice::ImportContent => {
                            let key_content = self.add_host_dialog.key_content.trim().to_owned();
                            if key_content.is_empty() {
                                self.add_host_dialog.error = "Private key content is required.".to_owned();
                                return;
                            }
                            // Store optional public key in path field for reference
                            (self.add_host_dialog.key_path.trim().to_owned(), key_content)
                        }
                    };
                    crate::config::AuthMethod::Key {
                        key_path: path,
                        key_content,
                        passphrase: self.add_host_dialog.key_passphrase.clone(),
                        key_in_keychain: self.add_host_dialog.key_in_keychain,
                    }
                }
            };

            let entry = HostEntry::new_ssh(
                name,
                host,
                port,
                self.add_host_dialog.username.trim().to_owned(),
                self.add_host_dialog.group.trim().to_owned(),
                auth,
            );

            // Parse tags from comma-separated string
            let tags: Vec<String> = self.add_host_dialog.tags
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            // Create entry with tags
            let mut entry = entry;
            entry.tags = tags;

            if let Some(idx) = self.add_host_dialog.edit_index {
                if idx < self.hosts.len() {
                    self.hosts[idx] = entry;
                }
            } else {
                self.hosts.push(entry);
            }

            self.save_hosts();
            self.add_host_dialog.reset();
        }
    }

    /// Render batch execution panel (right-side drawer, shadcn-style)
    #[allow(dead_code)]
    pub fn show_batch_execution_panel(&mut self, ctx: &egui::Context) {
        if !self.batch_execution.show_panel {
            return;
        }

        let panel_width = 420.0;
        egui::SidePanel::right("batch_execution_panel")
            .exact_width(panel_width)
            .resizable(false)
            .frame(egui::Frame {
                fill: self.theme.bg_secondary,
                inner_margin: egui::Margin::same(0.0),
                stroke: egui::Stroke::NONE,
                ..Default::default()
            })
            .show(ctx, |ui| {
                // Header with title and close button
                egui::Frame {
                    fill: self.theme.bg_elevated,
                    inner_margin: egui::Margin::symmetric(20.0, 16.0),
                    ..Default::default()
                }
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("⚡ 批量执行").color(self.theme.fg_primary).size(17.0).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(
                                egui::Button::new(egui::RichText::new("✕").color(self.theme.fg_dim).size(18.0))
                                    .frame(false)
                            ).clicked() {
                                self.batch_execution.show_panel = false;
                            }
                        });
                    });
                });

                egui::ScrollArea::vertical()
                    .id_salt("batch_panel_scroll")
                    .show(ui, |ui| {
                        ui.add_space(16.0);

                        // ── Target Machines Section ──
                        egui::Frame {
                            fill: self.theme.bg_elevated,
                            rounding: egui::Rounding::same(8.0),
                            inner_margin: egui::Margin::symmetric(16.0, 12.0),
                            stroke: egui::Stroke::new(1.0, self.theme.border),
                            ..Default::default()
                        }
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("目标机器").color(self.theme.fg_dim).size(11.0).strong());
                                ui.label(
                                    egui::RichText::new(format!("{}", self.batch_execution.targets.len()))
                                        .color(self.theme.fg_dim).size(11.0)
                                );
                            });
                            ui.add_space(8.0);

                            if self.batch_execution.targets.is_empty() {
                                ui.vertical_centered(|ui| {
                                    ui.add_space(20.0);
                                    ui.label(egui::RichText::new("暂无目标").color(self.theme.fg_dim).size(12.0).italics());
                                    ui.add_space(20.0);
                                });
                            } else {
                                let mut target_to_remove: Option<usize> = None;
                                for (i, target) in self.batch_execution.targets.iter().enumerate() {
                                    egui::Frame {
                                        fill: self.theme.bg_secondary,
                                        rounding: egui::Rounding::same(6.0),
                                        inner_margin: egui::Margin::symmetric(10.0, 8.0),
                                        ..Default::default()
                                    }
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            // Status indicator
                                            let is_connected = target.tab_idx != usize::MAX;
                                            ui.label(
                                                egui::RichText::new(if is_connected { "●" } else { "○" })
                                                    .color(if is_connected { self.theme.green } else { self.theme.fg_dim })
                                                    .size(10.0)
                                            );
                                            ui.add_space(6.0);

                                            ui.label(egui::RichText::new(&target.name).color(self.theme.fg_primary).size(13.0));

                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                if ui.add(
                                                    egui::Button::new(
                                                        egui::RichText::new("✕").color(self.theme.fg_dim).size(14.0)
                                                    )
                                                    .frame(false)
                                                    .rounding(4.0)
                                                ).clicked() {
                                                    target_to_remove = Some(i);
                                                }
                                            });
                                        });
                                    });
                                    ui.add_space(4.0);
                                }
                                if let Some(i) = target_to_remove {
                                    self.batch_execution.targets.remove(i);
                                }
                            }

                            ui.add_space(8.0);

                            // Add targets from hosts
                            if ui.add_sized(
                                [ui.available_width(), 32.0],
                                egui::Button::new(
                                    egui::RichText::new("+ 从主机列表添加").color(self.theme.accent).size(12.0)
                                )
                                .stroke(egui::Stroke::new(1.0, self.theme.accent))
                                .fill(egui::Color32::TRANSPARENT)
                                .rounding(6.0)
                            ).clicked() {
                                for host in &self.hosts {
                                    if !host.is_local {
                                        self.batch_execution.targets.push(BatchTarget {
                                            tab_idx: usize::MAX,
                                            session_idx: usize::MAX,
                                            global_id: usize::MAX,
                                            name: host.name.clone(),
                                        });
                                    }
                                }
                            }
                        });

                        ui.add_space(16.0);

                        // ── Command Input Section ──
                        egui::Frame {
                            fill: self.theme.bg_elevated,
                            rounding: egui::Rounding::same(8.0),
                            inner_margin: egui::Margin::symmetric(16.0, 12.0),
                            stroke: egui::Stroke::new(1.0, self.theme.border),
                            ..Default::default()
                        }
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("执行命令").color(self.theme.fg_dim).size(11.0).strong());

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    // History dropdown button
                                    if !self.batch_execution.command_history.is_empty() {
                                        let history_btn = egui::Button::new(
                                            egui::RichText::new("🕒").size(14.0)
                                        )
                                        .frame(false)
                                        .rounding(4.0);
                                        if ui.add(history_btn).clicked() {
                                            self.batch_execution.show_history = !self.batch_execution.show_history;
                                        }
                                    }
                                });
                            });
                            ui.add_space(8.0);

                            // Command history dropdown
                            if self.batch_execution.show_history {
                                egui::Frame {
                                    fill: self.theme.bg_secondary,
                                    rounding: egui::Rounding::same(6.0),
                                    inner_margin: egui::Margin::symmetric(8.0, 6.0),
                                    stroke: egui::Stroke::new(1.0, self.theme.border),
                                    ..Default::default()
                                }
                                .show(ui, |ui| {
                                    ui.add_space(4.0);
                                    ui.spacing_mut().item_spacing.y = 4.0;
                                    for cmd in self.batch_execution.command_history.iter() {
                                        if ui.add_sized(
                                            [ui.available_width(), 24.0],
                                            egui::Button::new(
                                                egui::RichText::new(cmd.lines().next().unwrap_or(cmd))
                                                    .color(self.theme.fg_primary).size(12.0)
                                            )
                                            .frame(false)
                                            .fill(egui::Color32::TRANSPARENT)
                                            .rounding(4.0)
                                        ).clicked() {
                                            self.batch_execution.command = cmd.clone();
                                            self.batch_execution.show_history = false;
                                        }
                                    }
                                    ui.add_space(4.0);
                                });
                                ui.add_space(8.0);
                            }

                            // Command input
                            let command_label: &str = if self.batch_execution.command.is_empty() {
                                "输入命令，例如: ls -la /tmp"
                            } else {
                                ""
                            };

                            ui.add(
                                egui::TextEdit::multiline(&mut self.batch_execution.command)
                                    .hint_text(egui::RichText::new(command_label)
                                        .color(self.theme.hint_color())
                                        .italics())
                                    .desired_rows(4)
                                    .frame(true)
                                    .text_color(self.theme.fg_primary)
                            );

                            ui.add_space(12.0);

                            // Execute button with better styling
                            let can_execute = !self.batch_execution.command.trim().is_empty()
                                && !self.batch_execution.targets.is_empty()
                                && !self.batch_execution.executing;

                            let has_results = !self.batch_execution.results.is_empty();

                            ui.horizontal(|ui| {
                                if ui.add_enabled(
                                    can_execute,
                                    egui::Button::new(
                                        egui::RichText::new("▶ 执行").color(egui::Color32::WHITE).size(13.0)
                                    )
                                    .fill(self.theme.accent)
                                    .rounding(6.0)
                                    .min_size(egui::vec2(80.0, 36.0))
                                ).clicked() {
                                    self.execute_batch_command(ctx);
                                }

                                // Re-execute button (only show if has results)
                                if has_results && !self.batch_execution.executing {
                                    if ui.add(
                                        egui::Button::new(
                                            egui::RichText::new("↻ 重新执行").color(self.theme.fg_primary).size(13.0)
                                        )
                                        .stroke(egui::Stroke::new(1.0, self.theme.border))
                                        .fill(egui::Color32::TRANSPARENT)
                                        .rounding(6.0)
                                        .min_size(egui::vec2(100.0, 36.0))
                                    ).clicked() {
                                        self.execute_batch_command(ctx);
                                    }
                                }

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if self.batch_execution.executing {
                                        ui.spinner();
                                        ui.label(egui::RichText::new("执行中...").color(self.theme.fg_dim).size(12.0));
                                    }
                                });
                            });
                        });

                        ui.add_space(16.0);

                        // ── Results Section ──
                        if !self.batch_execution.results.is_empty() {
                            egui::Frame {
                                fill: self.theme.bg_elevated,
                                rounding: egui::Rounding::same(8.0),
                                inner_margin: egui::Margin::symmetric(16.0, 12.0),
                                stroke: egui::Stroke::new(1.0, self.theme.border),
                                ..Default::default()
                            }
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("执行结果").color(self.theme.fg_dim).size(11.0).strong());
                                    ui.label(
                                        egui::RichText::new(format!("{}", self.batch_execution.results.len()))
                                            .color(self.theme.fg_dim).size(11.0)
                                    );
                                });
                                ui.add_space(8.0);

                                ui.spacing_mut().item_spacing.y = 8.0;

                                // Collect results data to avoid borrow issues
                                let results_data: Vec<(usize, BatchStatus, String, std::time::Instant, String)> = self.batch_execution.results
                                    .iter()
                                    .enumerate()
                                    .map(|(idx, r)| (idx, r.status.clone(), r.target.name.clone(), r.timestamp, r.output.clone()))
                                    .collect();
                                let expanded_set: std::collections::HashSet<usize> = self.batch_execution.expanded_results.iter().copied().collect();

                                for (result_idx, status, target_name, timestamp, output_text) in results_data {
                                    let is_expanded = expanded_set.contains(&result_idx);
                                    let (icon, icon_color) = match status {
                                        BatchStatus::Pending => ("⏳", self.theme.fg_dim),
                                        BatchStatus::Running => ("⟳", self.theme.accent),
                                        BatchStatus::Success => ("✓", self.theme.green),
                                        BatchStatus::Failed(_) => ("✗", self.theme.red),
                                    };

                                    egui::Frame {
                                        fill: self.theme.bg_secondary,
                                        rounding: egui::Rounding::same(6.0),
                                        inner_margin: egui::Margin::symmetric(12.0, 10.0),
                                        stroke: egui::Stroke::new(1.0, egui::Color32::TRANSPARENT),
                                        ..Default::default()
                                    }
                                    .show(ui, |ui| {
                                        // Main result row
                                        ui.horizontal(|ui| {
                                            // Expand/collapse button
                                            if ui.add(
                                                egui::Button::new(
                                                    egui::RichText::new(if is_expanded { "▼" } else { "▶" })
                                                        .color(self.theme.fg_dim).size(10.0)
                                                )
                                                .frame(false)
                                            ).clicked() {
                                                if is_expanded {
                                                    self.batch_execution.expanded_results.retain(|&x| x != result_idx);
                                                } else {
                                                    self.batch_execution.expanded_results.push(result_idx);
                                                }
                                            }

                                            ui.add_space(4.0);

                                            // Status icon
                                            ui.label(egui::RichText::new(icon).color(icon_color).size(14.0));

                                            ui.add_space(6.0);

                                            // Target name
                                            ui.label(egui::RichText::new(&target_name).color(self.theme.fg_primary).size(13.0));

                                            // Get the actual result for accessing target data
                                            let can_jump = self.batch_execution.results.get(result_idx)
                                                .and_then(|r| if r.target.tab_idx != usize::MAX { Some(true) } else { Some(false) })
                                                .unwrap_or(false);

                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                // Jump to session button (only for completed results)
                                                if matches!(status, BatchStatus::Success | BatchStatus::Failed(_)) {
                                                    if ui.add_enabled(
                                                        can_jump,
                                                        egui::Button::new(
                                                            egui::RichText::new("↗").color(self.theme.fg_dim).size(14.0)
                                                        )
                                                        .frame(false)
                                                        .rounding(4.0)
                                                    ).on_hover_text("跳转到会话").clicked() {
                                                        // TODO: Implement jump to session
                                                        // Switch to terminal view and focus the target tab/session
                                                    }

                                                    // Re-execute individual command (only for failed results)
                                                    if matches!(status, BatchStatus::Failed(_)) {
                                                        if ui.add(
                                                            egui::Button::new(
                                                                egui::RichText::new("↻").color(self.theme.fg_dim).size(12.0)
                                                            )
                                                            .frame(false)
                                                            .rounding(4.0)
                                                        ).on_hover_text("重新执行此命令").clicked() {
                                                            // TODO: Re-execute single command
                                                        }
                                                    }
                                                }
                                            });
                                        });

                                        // Expanded output section
                                        if is_expanded {
                                            ui.add_space(8.0);

                                            egui::Frame {
                                                fill: self.theme.bg_elevated,
                                                rounding: egui::Rounding::same(4.0),
                                                inner_margin: egui::Margin::symmetric(10.0, 8.0),
                                                ..Default::default()
                                            }
                                            .show(ui, |ui| {
                                                // Output with monospace font
                                                if output_text.is_empty() {
                                                    ui.label(
                                                        egui::RichText::new("无输出")
                                                            .color(self.theme.fg_dim).size(11.0).italics()
                                                    );
                                                } else {
                                                    // Create a static reference for display
                                                    let output_lines = output_text.lines().count();
                                                    let display_output = if output_lines > 5 {
                                                        output_text.lines().take(5).collect::<Vec<&str>>().join("\n")
                                                    } else {
                                                        output_text.clone()
                                                    };
                                                    ui.label(
                                                        egui::RichText::new(display_output)
                                                            .text_style(egui::TextStyle::Monospace)
                                                            .color(self.theme.fg_primary).size(11.0)
                                                    );
                                                    if output_lines > 5 {
                                                        ui.label(
                                                            egui::RichText::new(format!("... ({} more lines)", output_lines - 5))
                                                                .color(self.theme.fg_dim).size(10.0).italics()
                                                        );
                                                    }
                                                }

                                                // Timestamp
                                                ui.add_space(4.0);
                                                let elapsed = timestamp.elapsed();
                                                ui.label(
                                                    egui::RichText::new(format!("{} ago", self.format_duration(elapsed)))
                                                        .color(self.theme.fg_dim).size(10.0)
                                                );
                                            });
                                        }
                                    });
                                }
                            });
                        }

                        ui.add_space(16.0);
                    });
            });
    }

    /// Format duration for display
    fn format_duration(&self, duration: std::time::Duration) -> String {
        let secs = duration.as_secs();
        if secs < 60 {
            format!("{}s", secs)
        } else if secs < 3600 {
                            format!("{}m {}s", secs / 60, secs % 60)
                        } else {
                            format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
                        }
    }

    /// Execute batch command on all targets in parallel
    fn execute_batch_command(&mut self, _ctx: &egui::Context) {
        if self.batch_execution.targets.is_empty() {
            return;
        }

        let command = self.batch_execution.command.trim().to_string();
        if command.is_empty() {
            return;
        }

        // Add to history
        if !self.batch_execution.command_history.contains(&command) {
            self.batch_execution.command_history.push(command.clone());
        }

        // Create channel for result updates
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        self.batch_execution.result_rx = Some(result_rx);

        // Create pending results for all targets
        self.batch_execution.results.clear();
        self.batch_execution.expanded_results.clear();
        for target in self.batch_execution.targets.clone() {
            self.batch_execution.results.push(BatchResult {
                target: target.clone(),
                status: BatchStatus::Pending,
                output: String::new(),
                timestamp: std::time::Instant::now(),
            });
        }

        self.batch_execution.executing = true;

        // Collect session references before spawning tasks
        let session_refs: Vec<(usize, Option<Arc<Mutex<crate::terminal::TerminalGrid>>>, String)> = self.batch_execution.targets
            .iter()
            .enumerate()
            .map(|(idx, target)| {
                let session = if target.tab_idx != usize::MAX && target.tab_idx < self.tabs.len() {
                    let tab = &self.tabs[target.tab_idx];
                    if target.session_idx < tab.sessions.len() {
                        Some(tab.sessions[target.session_idx].grid.clone())
                    } else {
                        None
                    }
                } else {
                    None
                };
                (idx, session, target.name.clone())
            })
            .collect();

        // Spawn parallel execution tasks
        let runtime = self.runtime.handle();
        for (idx, session_ref, target_name) in session_refs {
            let tx = result_tx.clone();

            if session_ref.is_none() {
                // Target not connected to a session - mark as failed
                let _ = tx.send(BatchUpdate::StatusChanged {
                    index: idx,
                    status: BatchStatus::Failed("No active session".to_string()),
                });
                continue;
            }

            // Spawn async task to execute command and capture output
            let _ = runtime.spawn(async move {
                // Mark as running
                let _ = tx.send(BatchUpdate::StatusChanged {
                    index: idx,
                    status: BatchStatus::Running,
                });

                // Send command to session
                if let Some(session_ref) = session_ref {
                    // Lock the grid to check if we can access it
                    let can_access = session_ref.lock().is_ok();

                    if can_access {
                        // For now, we'll simulate execution
                        // In a real implementation, we'd need to:
                        // 1. Send the command bytes
                        // 2. Wait for execution
                        // 3. Capture the output from the grid

                        // TODO: Implement actual command execution and output capture
                        // For now, mark as success with placeholder output
                        tokio::time::sleep(tokio::time::Duration::from_millis(500 + (idx as u64 * 100))).await;

                        let _ = tx.send(BatchUpdate::StatusChanged {
                            index: idx,
                            status: BatchStatus::Success,
                        });

                        let _ = tx.send(BatchUpdate::Output {
                            index: idx,
                            output: format!("Command executed on {}\nOutput placeholder", target_name),
                        });
                    } else {
                        let _ = tx.send(BatchUpdate::StatusChanged {
                            index: idx,
                            status: BatchStatus::Failed("Failed to lock session".to_string()),
                        });
                    }
                }
            });
        }

        // Drop the original sender so the channel closes when all tasks complete
        drop(result_tx);
    }

    /// Check for batch execution result updates
    pub fn check_batch_execution_updates(&mut self) {
        if let Some(ref rx) = self.batch_execution.result_rx {
            while let Ok(update) = rx.try_recv() {
                match update {
                    BatchUpdate::StatusChanged { index, status } => {
                        if index < self.batch_execution.results.len() {
                            self.batch_execution.results[index].status = status;
                        }
                    }
                    BatchUpdate::Output { index, output } => {
                        if index < self.batch_execution.results.len() {
                            self.batch_execution.results[index].output = output;
                        }
                    }
                }
            }

            // Check if all executions are complete
            let all_complete = self.batch_execution.results.iter().all(|r| {
                matches!(r.status, BatchStatus::Success | BatchStatus::Failed(_))
            });

            if all_complete {
                self.batch_execution.executing = false;
                self.batch_execution.result_rx = None;
            }
        }
    }

    /// Show batch execution page (main view)
    pub fn show_batch_page(&mut self, ctx: &egui::Context) {
        // Main content area with command input and results
        egui::CentralPanel::default()
            .frame(egui::Frame {
                fill: self.theme.bg_secondary,
                ..Default::default()
            })
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("batch_page_scroll")
                    .show(ui, |ui| {
                        ui.add_space(20.0);

                        // Page header (inline, like hosts page)
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            ui.label(egui::RichText::new(self.language.t("batch")).color(self.theme.fg_dim).size(12.0).strong());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.add_space(24.0);
                            });
                        });
                        ui.add_space(16.0);

                        // Targets summary card
                        egui::Frame {
                            fill: self.theme.bg_elevated,
                            rounding: egui::Rounding::same(8.0),
                            inner_margin: egui::Margin::symmetric(16.0, 12.0),
                            stroke: egui::Stroke::new(1.0, self.theme.border),
                            ..Default::default()
                        }
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(format!("{} {}", self.language.t("batch_targets"), self.batch_execution.targets.len()))
                                        .color(self.theme.fg_dim)
                                        .size(11.0)
                                        .strong()
                                );

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    // Select hosts button
                                    if ui.add(
                                        egui::Button::new(
                                            egui::RichText::new(self.language.t("batch_select_hosts"))
                                                .color(self.theme.fg_primary)
                                                .size(12.0)
                                        )
                                        .fill(self.theme.accent)
                                        .rounding(4.0)
                                        .min_size(egui::vec2(100.0, 28.0))
                                    ).clicked() {
                                        self.batch_execution.show_hosts_drawer = true;
                                    }

                                    // Clear all button
                                    if !self.batch_execution.targets.is_empty() {
                                        if ui.add(
                                            egui::Button::new(
                                                egui::RichText::new(self.language.t("batch_clear_all"))
                                                    .color(self.theme.fg_dim)
                                                    .size(12.0)
                                            )
                                            .stroke(egui::Stroke::new(1.0, self.theme.border))
                                            .fill(egui::Color32::TRANSPARENT)
                                            .rounding(4.0)
                                            .min_size(egui::vec2(70.0, 28.0))
                                        ).clicked() {
                                            self.batch_execution.targets.clear();
                                            self.batch_execution.results.clear();
                                        }
                                    }
                                });
                            });

                            // Selected targets preview
                            if !self.batch_execution.targets.is_empty() {
                                ui.add_space(8.0);
                                let target_count = self.batch_execution.targets.len().min(5);
                                for (i, target) in self.batch_execution.targets.iter().take(target_count).enumerate() {
                                    let is_connected = target.tab_idx != usize::MAX;
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(if is_connected { "●" } else { "○" })
                                                .color(if is_connected { self.theme.green } else { self.theme.fg_dim })
                                                .size(10.0)
                                        );
                                        ui.add_space(6.0);
                                        ui.label(
                                            egui::RichText::new(&target.name)
                                                .color(self.theme.fg_primary)
                                                .size(12.0)
                                        );
                                    });
                                    if i < target_count - 1 {
                                        ui.add_space(4.0);
                                    }
                                }
                                if self.batch_execution.targets.len() > 5 {
                                    ui.add_space(4.0);
                                    ui.label(
                                        egui::RichText::new(format!("+{}", self.batch_execution.targets.len() - 5))
                                            .color(self.theme.fg_dim)
                                            .size(11.0)
                                            .italics()
                                    );
                                }
                            }
                        });

                        ui.add_space(16.0);

                        // Command input card
                        egui::Frame {
                            fill: self.theme.bg_elevated,
                            rounding: egui::Rounding::same(8.0),
                            inner_margin: egui::Margin::symmetric(16.0, 12.0),
                            stroke: egui::Stroke::new(1.0, self.theme.border),
                            ..Default::default()
                        }
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(self.language.t("batch_command"))
                                        .color(self.theme.fg_dim)
                                        .size(11.0)
                                        .strong()
                                );

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    // History dropdown button
                                    if !self.batch_execution.command_history.is_empty() {
                                        if ui.add(
                                            egui::Button::new(
                                                egui::RichText::new("🕒").size(12.0)
                                            )
                                            .frame(false)
                                            .rounding(4.0)
                                        ).clicked() {
                                            self.batch_execution.show_history = !self.batch_execution.show_history;
                                        }
                                    }
                                });
                            });

                            ui.add_space(6.0);

                            // Command history dropdown
                            if self.batch_execution.show_history {
                                egui::Frame {
                                    fill: self.theme.bg_secondary,
                                    rounding: egui::Rounding::same(6.0),
                                    inner_margin: egui::Margin::symmetric(8.0, 6.0),
                                    stroke: egui::Stroke::new(1.0, self.theme.border),
                                    ..Default::default()
                                }
                                .show(ui, |ui| {
                                    ui.add_space(4.0);
                                    ui.spacing_mut().item_spacing.y = 4.0;
                                    for cmd in self.batch_execution.command_history.iter() {
                                        if ui.add_sized(
                                            [ui.available_width(), 24.0],
                                            egui::Button::new(
                                                egui::RichText::new(cmd.lines().next().unwrap_or(cmd))
                                                    .color(self.theme.fg_primary)
                                                    .size(12.0)
                                            )
                                            .frame(false)
                                            .fill(egui::Color32::TRANSPARENT)
                                            .rounding(4.0)
                                        ).clicked() {
                                            self.batch_execution.command = cmd.clone();
                                            self.batch_execution.show_history = false;
                                        }
                                    }
                                    ui.add_space(4.0);
                                });
                                ui.add_space(8.0);
                            }

                            // Command input
                            let command_label: &str = if self.batch_execution.command.is_empty() {
                                "输入命令，例如: ls -la /tmp"
                            } else {
                                ""
                            };

                            ui.add_sized(
                                [ui.available_width(), 80.0],
                                egui::TextEdit::multiline(&mut self.batch_execution.command)
                                    .hint_text(egui::RichText::new(command_label)
                                        .color(self.theme.hint_color())
                                        .italics())
                                    .desired_rows(3)
                                    .frame(true)
                                    .text_color(self.theme.fg_primary)
                            );

                            ui.add_space(10.0);

                            // Execute buttons
                            let can_execute = !self.batch_execution.command.trim().is_empty()
                                && !self.batch_execution.targets.is_empty()
                                && !self.batch_execution.executing;

                            let has_results = !self.batch_execution.results.is_empty();

                            ui.horizontal(|ui| {
                                if ui.add_enabled(
                                    can_execute,
                                    egui::Button::new(
                                        egui::RichText::new(format!("▶ {}", self.language.t("batch_execute")))
                                            .color(egui::Color32::WHITE)
                                            .size(12.0)
                                    )
                                    .fill(self.theme.accent)
                                    .rounding(4.0)
                                    .min_size(egui::vec2(80.0, 28.0))
                                ).clicked() {
                                    self.execute_batch_command(ctx);
                                }

                                // Re-execute button
                                if has_results && !self.batch_execution.executing {
                                    if ui.add(
                                        egui::Button::new(
                                            egui::RichText::new(format!("↻ {}", self.language.t("batch_reexecute")))
                                                .color(self.theme.fg_primary)
                                                .size(12.0)
                                        )
                                        .stroke(egui::Stroke::new(1.0, self.theme.border))
                                        .fill(egui::Color32::TRANSPARENT)
                                        .rounding(4.0)
                                        .min_size(egui::vec2(100.0, 28.0))
                                    ).clicked() {
                                        self.execute_batch_command(ctx);
                                    }
                                }

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if self.batch_execution.executing {
                                        ui.spinner();
                                        ui.label(
                                            egui::RichText::new(self.language.t("batch_executing"))
                                                .color(self.theme.fg_dim)
                                                .size(11.0)
                                        );
                                    }
                                });
                            });
                        });

                        // Results section
                        if !self.batch_execution.results.is_empty() {
                            ui.add_space(16.0);

                            egui::Frame {
                                fill: self.theme.bg_elevated,
                                rounding: egui::Rounding::same(8.0),
                                inner_margin: egui::Margin::symmetric(16.0, 12.0),
                                stroke: egui::Stroke::new(1.0, self.theme.border),
                                ..Default::default()
                            }
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(self.language.t("batch_results"))
                                            .color(self.theme.fg_dim)
                                            .size(11.0)
                                            .strong()
                                    );
                                    ui.label(
                                        egui::RichText::new(format!("{}", self.batch_execution.results.len()))
                                            .color(self.theme.fg_dim)
                                            .size(11.0)
                                    );
                                });
                                ui.add_space(10.0);

                                ui.spacing_mut().item_spacing.y = 8.0;

                                // Collect results data to avoid borrow issues
                                let results_data: Vec<(usize, BatchStatus, String, std::time::Instant, String)> = self.batch_execution.results
                                    .iter()
                                    .enumerate()
                                    .map(|(idx, r)| (idx, r.status.clone(), r.target.name.clone(), r.timestamp, r.output.clone()))
                                    .collect();
                                let expanded_set: std::collections::HashSet<usize> = self.batch_execution.expanded_results.iter().copied().collect();

                                for (result_idx, status, target_name, timestamp, output_text) in results_data {
                                    let (icon, icon_color) = match status {
                                        BatchStatus::Pending => ("⏳", self.theme.fg_dim),
                                        BatchStatus::Running => ("⟳", self.theme.accent),
                                        BatchStatus::Success => ("✓", self.theme.green),
                                        BatchStatus::Failed(_) => ("✗", self.theme.red),
                                    };

                                    let is_expanded = expanded_set.contains(&result_idx);

                                    egui::Frame {
                                        fill: self.theme.bg_secondary,
                                        rounding: egui::Rounding::same(6.0),
                                        inner_margin: egui::Margin::symmetric(12.0, 10.0),
                                        stroke: egui::Stroke::new(1.0, egui::Color32::TRANSPARENT),
                                        ..Default::default()
                                    }
                                    .show(ui, |ui| {
                                        // Main result row
                                        ui.horizontal(|ui| {
                                            // Expand/collapse button
                                            if ui.add(
                                                egui::Button::new(
                                                    egui::RichText::new(if is_expanded { "▼" } else { "▶" })
                                                        .color(self.theme.fg_dim)
                                                        .size(10.0)
                                                )
                                                .frame(false)
                                            ).clicked() {
                                                if is_expanded {
                                                    self.batch_execution.expanded_results.retain(|&x| x != result_idx);
                                                } else {
                                                    self.batch_execution.expanded_results.push(result_idx);
                                                }
                                            }

                                            ui.add_space(4.0);

                                            // Status icon
                                            ui.label(
                                                egui::RichText::new(icon)
                                                    .color(icon_color)
                                                    .size(14.0)
                                            );

                                            ui.add_space(6.0);

                                            // Target name
                                            ui.label(
                                                egui::RichText::new(&target_name)
                                                    .color(self.theme.fg_primary)
                                                    .size(12.0)
                                                    .strong()
                                            );

                                            // Status text
                                            let status_text = match status {
                                                BatchStatus::Pending => self.language.t("batch_waiting"),
                                                BatchStatus::Running => self.language.t("batch_running"),
                                                BatchStatus::Success => self.language.t("batch_success"),
                                                BatchStatus::Failed(_) => self.language.t("batch_failed"),
                                            };
                                            ui.label(
                                                egui::RichText::new(status_text)
                                                    .color(icon_color)
                                                    .size(11.0)
                                            );

                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                // Jump to session button
                                                let can_jump = self.batch_execution.results.get(result_idx)
                                                    .and_then(|r| if r.target.tab_idx != usize::MAX { Some(true) } else { Some(false) })
                                                    .unwrap_or(false);

                                                if matches!(status, BatchStatus::Success | BatchStatus::Failed(_)) {
                                                    if ui.add_enabled(
                                                        can_jump,
                                                        egui::Button::new(
                                                            egui::RichText::new("↗")
                                                                .color(self.theme.fg_dim)
                                                                .size(14.0)
                                                        )
                                                        .frame(false)
                                                        .rounding(4.0)
                                                    ).on_hover_text(self.language.t("batch_jump_to_session")).clicked() {
                                                        // TODO: Implement jump to session
                                                    }

                                                    // Re-execute individual command
                                                    if matches!(status, BatchStatus::Failed(_)) {
                                                        if ui.add(
                                                            egui::Button::new(
                                                                egui::RichText::new("↻")
                                                                    .color(self.theme.fg_dim)
                                                                    .size(12.0)
                                                            )
                                                            .frame(false)
                                                            .rounding(4.0)
                                                        ).on_hover_text(self.language.t("batch_reexecute_single")).clicked() {
                                                            // TODO: Re-execute single command
                                                        }
                                                    }
                                                }
                                            });
                                        });

                                        // Expanded output section
                                        if is_expanded {
                                            ui.add_space(8.0);

                                            egui::Frame {
                                                fill: self.theme.bg_elevated,
                                                rounding: egui::Rounding::same(4.0),
                                                inner_margin: egui::Margin::symmetric(10.0, 8.0),
                                                ..Default::default()
                                            }
                                            .show(ui, |ui| {
                                                // Output with monospace font
                                                if output_text.is_empty() {
                                                    ui.label(
                                                        egui::RichText::new(self.language.t("batch_no_output"))
                                                            .color(self.theme.fg_dim)
                                                            .size(11.0)
                                                            .italics()
                                                    );
                                                } else {
                                                    let output_lines = output_text.lines().count();
                                                    let display_output = if output_lines > 10 {
                                                        output_text.lines().take(10).collect::<Vec<&str>>().join("\n")
                                                    } else {
                                                        output_text.clone()
                                                    };
                                                    ui.label(
                                                        egui::RichText::new(display_output)
                                                            .text_style(egui::TextStyle::Monospace)
                                                            .color(self.theme.fg_primary)
                                                            .size(11.0)
                                                    );
                                                    if output_lines > 10 {
                                                        ui.add_space(4.0);
                                                        ui.label(
                                                            egui::RichText::new(format!("... ({} more lines)", output_lines - 10))
                                                                .color(self.theme.fg_dim)
                                                                .size(10.0)
                                                                .italics()
                                                        );
                                                    }
                                                }

                                                // Timestamp
                                                ui.add_space(4.0);
                                                let elapsed = timestamp.elapsed();
                                                ui.label(
                                                    egui::RichText::new(format!("{} {}", self.format_duration(elapsed), self.language.t("batch_ago")))
                                                        .color(self.theme.fg_dim)
                                                        .size(10.0)
                                                );
                                            });
                                        }
                                    });
                                }
                            });
                        }

                        ui.add_space(20.0);
                    });
            });

        // Show hosts selection drawer
        self.show_batch_hosts_drawer(ctx);
    }

    /// Show hosts selection drawer (right side)
    pub fn show_batch_hosts_drawer(&mut self, ctx: &egui::Context) {
        if !self.batch_execution.show_hosts_drawer {
            return;
        }

        let drawer_width = 320.0;
        egui::SidePanel::right("batch_hosts_drawer")
            .exact_width(drawer_width)
            .resizable(false)
            .frame(egui::Frame {
                fill: self.theme.bg_secondary,
                inner_margin: egui::Margin::same(0.0),
                stroke: egui::Stroke::NONE,
                ..Default::default()
            })
            .show(ctx, |ui| {
                // Header
                egui::Frame {
                    fill: self.theme.bg_elevated,
                    inner_margin: egui::Margin::symmetric(20.0, 16.0),
                    ..Default::default()
                }
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(self.language.t("batch_select_hosts"))
                                .color(self.theme.fg_primary)
                                .size(17.0)
                                .strong()
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(
                                egui::Button::new(
                                    egui::RichText::new("✕")
                                        .color(self.theme.fg_dim)
                                        .size(18.0)
                                )
                                .frame(false)
                            ).clicked() {
                                self.batch_execution.show_hosts_drawer = false;
                            }
                        });
                    });
                });

                egui::ScrollArea::vertical()
                    .id_salt("batch_hosts_drawer_scroll")
                    .show(ui, |ui| {
                        ui.add_space(4.0);

                        // SSH hosts section
                        ui.label(
                            egui::RichText::new("SSH 主机")
                                .color(self.theme.fg_dim)
                                .size(10.0)
                                .strong()
                        );
                        ui.add_space(4.0);

                        let mut hosts_to_add: Vec<HostEntry> = vec![];
                        let mut hosts_to_remove: Vec<usize> = vec![];

                        for (_host_idx, host) in self.hosts.iter().enumerate() {
                            if host.is_local {
                                continue;
                            }

                            // Check if already in targets
                            let is_selected = self.batch_execution.targets.iter()
                                .any(|t| t.name == host.name);

                            egui::Frame {
                                fill: if is_selected { self.theme.bg_elevated } else { egui::Color32::TRANSPARENT },
                                rounding: egui::Rounding::same(6.0),
                                inner_margin: egui::Margin::symmetric(10.0, 8.0),
                                stroke: if is_selected {
                                    egui::Stroke::new(1.0, self.theme.accent)
                                } else {
                                    egui::Stroke::new(1.0, self.theme.border)
                                },
                                ..Default::default()
                            }
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // Checkbox
                                    if ui.add(
                                        egui::Button::new(
                                            egui::RichText::new(if is_selected { "☑" } else { "☐" })
                                                .size(14.0)
                                        )
                                        .frame(false)
                                    ).clicked() {
                                        if is_selected {
                                            // Find and mark for removal
                                            if let Some(idx) = self.batch_execution.targets.iter()
                                                .position(|t| t.name == host.name) {
                                                hosts_to_remove.push(idx);
                                            }
                                        } else {
                                            hosts_to_add.push(host.clone());
                                        }
                                    }

                                    ui.add_space(6.0);

                                    // Host info
                                    ui.vertical(|ui| {
                                        ui.label(
                                            egui::RichText::new(&host.name)
                                                .color(self.theme.fg_primary)
                                                .size(12.0)
                                                .strong()
                                        );
                                        ui.label(
                                            egui::RichText::new(format!("{}@{}:{}", host.username, host.host, host.port))
                                                .color(self.theme.fg_dim)
                                                .size(10.0)
                                        );
                                        if !host.group.is_empty() {
                                            ui.label(
                                                egui::RichText::new(&host.group)
                                                    .color(self.theme.fg_dim)
                                                    .size(9.0)
                                            );
                                        }
                                    });
                                });
                            });

                            ui.add_space(4.0);
                        }

                        // Apply removals (in reverse order to maintain indices)
                        for idx in hosts_to_remove.into_iter().rev() {
                            if idx < self.batch_execution.targets.len() {
                                self.batch_execution.targets.remove(idx);
                            }
                        }

                        // Apply additions
                        for host in hosts_to_add {
                            self.batch_execution.targets.push(BatchTarget {
                                tab_idx: usize::MAX,
                                session_idx: usize::MAX,
                                global_id: usize::MAX,
                                name: host.name.clone(),
                            });
                        }

                        ui.add_space(12.0);

                        // Quick actions
                        ui.separator();
                        ui.add_space(8.0);

                        if ui.add_sized(
                            [ui.available_width(), 28.0],
                            egui::Button::new(
                                egui::RichText::new(self.language.t("batch_add_from_hosts"))
                                    .color(self.theme.accent)
                                    .size(11.0)
                            )
                            .stroke(egui::Stroke::new(1.0, self.theme.accent))
                            .fill(egui::Color32::TRANSPARENT)
                            .rounding(4.0)
                        ).clicked() {
                            for host in &self.hosts {
                                if !host.is_local && !self.batch_execution.targets.iter().any(|t| t.name == host.name) {
                                    self.batch_execution.targets.push(BatchTarget {
                                        tab_idx: usize::MAX,
                                        session_idx: usize::MAX,
                                        global_id: usize::MAX,
                                        name: host.name.clone(),
                                    });
                                }
                            }
                        }

                        ui.add_space(12.0);
                    });
            });
    }
}
