use eframe::egui;
use std::sync::{Arc, Mutex};

use crate::app::PortalApp;
use crate::config::HostEntry;
use crate::ssh::test_connection;
use crate::ui::types::dialogs::{AuthMethodChoice, TestConnState, KeySourceChoice, CredentialMode};
use crate::ui::i18n::format_time_ago;
use crate::ui::tokens::*;
use crate::ui::views::nav_panel;
use crate::ui::widgets;

impl PortalApp {
    /// Navigation strip on the left (always visible)
    pub fn show_nav_panel(&mut self, ctx: &egui::Context) {
        if let Some(clicked_view) = nav_panel::show_nav_panel(
            ctx,
            self.current_view,
            &self.theme,
            &self.language,
            None,
        ) {
            self.current_view = clicked_view;
        }
    }

    /// Full hosts page content (used by both main and detached windows)
    pub fn show_hosts_page(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        ui.add_space(0.0);
        let mut new_local_session = false;
        let mut connect_ssh_host: Option<usize> = None;
        let mut edit_host_index: Option<usize> = None;
        let mut connect_history_host: Option<HostEntry> = None;
        let mut clear_history = false;

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
                inner_margin: egui::Margin::symmetric(8.0, 8.0),
                stroke: egui::Stroke::NONE,
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Left side: Hosts title
                    ui.label(egui::RichText::new(self.language.t("hosts")).color(self.theme.fg_dim).size(FONT_BASE).strong());

                    // Right side: New Host button
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(widgets::text_button(self.language.t("new_host"), self.theme.accent)).clicked() {
                            let current_time = ctx.input(|i| i.time);
                            self.add_host_dialog.open_new(current_time);
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
                    .id_salt("hosts_page_scroll")
                    .show(ui, |ui| {
                        ui.add_space(SPACE_MD);

                        // Filter bar (in content area, right-aligned with margin)
                        ui.horizontal(|ui| {
                            // Match ComboBox closed-state background to New Host TextEdit input
                            let input_bg = ui.visuals().extreme_bg_color;
                            let border = self.theme.input_border;
                            ui.style_mut().visuals.widgets.inactive.bg_fill = input_bg;
                            ui.style_mut().visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, border);
                            ui.style_mut().visuals.widgets.hovered.bg_fill = input_bg;
                            ui.style_mut().visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, self.theme.focus_ring);
                            ui.style_mut().visuals.widgets.active.bg_fill = input_bg;
                            ui.style_mut().visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, self.theme.accent);
                            ui.style_mut().visuals.widgets.open.bg_fill = input_bg;
                            ui.style_mut().visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0, self.theme.accent);

                            // Add left space to push content to the right
                            ui.add_space(ui.available_width() - 210.0); // Adjusted for narrower filters

                            // Group filter dropdown
                            let group_label = if self.host_filter.group.is_empty() {
                                self.language.t("group").to_string()
                            } else {
                                self.host_filter.group.clone()
                            };
                            egui::ComboBox::from_id_salt("group_filter")
                                .selected_text(egui::RichText::new(group_label).color(self.theme.accent).size(FONT_MD))
                                .width(90.0)
                                .show_ui(ui, |ui| {
                                    widgets::style_dropdown(ui, &self.theme);

                                    // All option
                                    if ui.add(egui::Button::new(
                                        egui::RichText::new(self.language.t("snippet_default_group"))
                                            .color(if self.host_filter.group.is_empty() { self.theme.accent } else { self.theme.fg_primary })
                                            .size(FONT_MD)
                                    ).frame(false)).clicked() {
                                        self.host_filter.group.clear();
                                        ui.close_menu();
                                    }

                                    for group in &all_groups {
                                        if ui.add(egui::Button::new(
                                            egui::RichText::new(group)
                                                .color(if self.host_filter.group == *group { self.theme.accent } else { self.theme.fg_primary })
                                                .size(FONT_MD)
                                        ).frame(false)).clicked() {
                                            self.host_filter.group = group.clone();
                                            ui.close_menu();
                                        }
                                    }
                                });

                            ui.add_space(SPACE_SM - 2.0);

                            // Tag filter dropdown
                            let tag_label = if self.host_filter.tag.is_empty() {
                                self.language.t("tag").to_string()
                            } else {
                                self.host_filter.tag.clone()
                            };
                            egui::ComboBox::from_id_salt("tag_filter")
                                .selected_text(egui::RichText::new(tag_label).color(self.theme.accent).size(FONT_MD))
                                .width(90.0)
                                .show_ui(ui, |ui| {
                                    widgets::style_dropdown(ui, &self.theme);

                                    // All option
                                    if ui.add(egui::Button::new(
                                        egui::RichText::new(self.language.t("snippet_default_group"))
                                            .color(if self.host_filter.tag.is_empty() { self.theme.accent } else { self.theme.fg_primary })
                                            .size(FONT_MD)
                                    ).frame(false)).clicked() {
                                        self.host_filter.tag.clear();
                                        ui.close_menu();
                                    }

                                    for tag in &all_tags {
                                        if ui.add(egui::Button::new(
                                            egui::RichText::new(tag)
                                                .color(if self.host_filter.tag == *tag { self.theme.accent } else { self.theme.fg_primary })
                                                .size(FONT_MD)
                                        ).frame(false)).clicked() {
                                            self.host_filter.tag = tag.clone();
                                            ui.close_menu();
                                        }
                                    }
                                });

                            ui.add_space(SPACE_SM);

                            // Clear button
                            if self.host_filter.is_active() {
                                if ui.add(widgets::text_button(self.language.t("clear_history"), self.theme.accent)).clicked() {
                                    self.host_filter.clear();
                                }
                            }
                        });

                        ui.add_space(SPACE_MD);

            // RECENT CONNECTIONS section
            {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let history = &self.connection_history;
                let mut seen = std::collections::HashSet::new();
                let recent: Vec<_> = history.iter().rev()
                    .filter(|r| {
                        let key = (r.host.clone(), r.port, r.username.clone());
                        seen.insert(key)
                    })
                    .take(10)
                    .collect();

                if !recent.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add_space(SPACE_XL);
                        ui.label(egui::RichText::new(self.language.t("recent_connections")).color(self.theme.fg_dim).size(FONT_XS).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(SPACE_XL);
                            if ui.add(
                                egui::Button::new(egui::RichText::new(self.language.t("clear_history")).color(self.theme.fg_dim).size(10.0))
                                    .frame(false)
                            ).clicked() {
                                clear_history = true;
                            }
                        });
                    });
                    ui.add_space(SPACE_XS);

                    for record in &recent {
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
                        let display_name = if record.host_name.is_empty() {
                            format!("{}:{}", record.host, record.port)
                        } else {
                            record.host_name.clone()
                        };
                        ui.painter().text(
                            egui::pos2(rect.min.x + 24.0, rect.min.y + 14.0),
                            egui::Align2::LEFT_CENTER,
                            "@",
                            egui::FontId::proportional(12.0),
                            self.theme.accent,
                        );
                        ui.painter().text(
                            egui::pos2(rect.min.x + 46.0, rect.min.y + 14.0),
                            egui::Align2::LEFT_CENTER,
                            &display_name,
                            egui::FontId::proportional(13.0),
                            self.theme.fg_primary,
                        );
                        let detail = format!("{}@{}:{}", record.username, record.host, record.port);
                        ui.painter().text(
                            egui::pos2(rect.min.x + 46.0, rect.min.y + 30.0),
                            egui::Align2::LEFT_CENTER,
                            &detail,
                            egui::FontId::proportional(10.0),
                            self.theme.fg_dim,
                        );
                        let secs_ago = now.saturating_sub(record.timestamp);
                        let time_text = format_time_ago(secs_ago, &self.language);
                        let visible_right = ui.clip_rect().max.x;

                        // Layout time and connect button on the same line, vertically centered in row (row_h = 44, center = 22)
                        let time_galley = ui.painter().layout_no_wrap(
                            time_text.clone(),
                            egui::FontId::proportional(10.0),
                            self.theme.fg_dim,
                        );
                        let time_width = time_galley.size().x;

                        // Time text (right aligned, vertically centered at y + 22)
                        let time_x = visible_right - 24.0;
                        ui.painter().text(
                            egui::pos2(time_x, rect.min.y + 22.0),
                            egui::Align2::RIGHT_CENTER,
                            &time_text,
                            egui::FontId::proportional(10.0),
                            self.theme.fg_dim,
                        );

                        // Connect button (left of time text, only on hover, vertically centered)
                        if hovered {
                            let btn_width = 56.0;
                            let btn_x = time_x - time_width - 8.0; // 8px padding between time and button
                            let btn_rect = egui::Rect::from_center_size(
                                egui::pos2(btn_x - btn_width / 2.0, rect.min.y + 22.0),
                                egui::vec2(btn_width, 20.0),
                            );
                            let pointer_pos = ui.ctx().input(|i| i.pointer.hover_pos());
                            let over_btn = pointer_pos.map_or(false, |p| btn_rect.contains(p));
                            let btn_bg = if over_btn { self.theme.accent } else { self.theme.bg_elevated };
                            let btn_text_color = if over_btn { self.theme.bg_primary } else { self.theme.accent };
                            ui.painter().rect(btn_rect, 4.0, btn_bg, egui::Stroke::new(1.0, self.theme.accent));
                            ui.painter().text(
                                btn_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                self.language.t("connect"),
                                egui::FontId::proportional(10.0),
                                btn_text_color,
                            );
                            if resp.clicked() && over_btn {
                                // Look up the actual host entry from hosts list to preserve credentials
                                if let Some(host_entry) = self.hosts.iter().find(|h| {
                                    !h.is_local && h.host == record.host && h.port == record.port && h.username == record.username
                                }).cloned() {
                                    connect_history_host = Some(host_entry);
                                } else {
                                    // Fallback: create entry without credentials (will prompt for auth)
                                    connect_history_host = Some(HostEntry::new_ssh(
                                        record.host_name.clone(),
                                        record.host.clone(),
                                        record.port,
                                        record.username.clone(),
                                        String::new(),
                                        None,
                                        Vec::new(),
                                    ));
                                }
                            }
                        }
                        if resp.double_clicked() {
                            // Look up the actual host entry from hosts list to preserve credentials
                            if let Some(host_entry) = self.hosts.iter().find(|h| {
                                !h.is_local && h.host == record.host && h.port == record.port && h.username == record.username
                            }).cloned() {
                                connect_history_host = Some(host_entry);
                            } else {
                                // Fallback: create entry without credentials (will prompt for auth)
                                connect_history_host = Some(HostEntry::new_ssh(
                                    record.host_name.clone(),
                                    record.host.clone(),
                                    record.port,
                                    record.username.clone(),
                                    String::new(),
                                    None,
                                    Vec::new(),
                                ));
                            }
                        }
                    }

                    ui.add_space(SPACE_XL);
                }
            }

            // LOCAL section
            ui.horizontal(|ui| {
                ui.add_space(SPACE_XL);
                ui.label(egui::RichText::new(self.language.t("local")).color(self.theme.fg_dim).size(FONT_XS).strong());
            });
            ui.add_space(SPACE_XS);

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

            ui.add_space(SPACE_XL);

            // SSH HOSTS section
            ui.horizontal(|ui| {
                ui.add_space(SPACE_XL);
                ui.label(egui::RichText::new(self.language.t("ssh_hosts")).color(self.theme.fg_dim).size(FONT_XS).strong());
            });
            ui.add_space(SPACE_XS);

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
                let row_h = 58.0;  // Increased to accommodate tags without overflow
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
                    egui::pos2(rect.min.x + 24.0, rect.min.y + 18.0),
                    egui::Align2::LEFT_CENTER,
                    "@",
                    egui::FontId::proportional(12.0),
                    self.theme.accent,
                );
                // Host name
                ui.painter().text(
                    egui::pos2(rect.min.x + 46.0, rect.min.y + 18.0),
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
                    egui::pos2(rect.min.x + 46.0, rect.min.y + 34.0),
                    egui::Align2::LEFT_CENTER,
                    detail,
                    egui::FontId::proportional(10.0),
                    self.theme.fg_dim,
                );

                // Tags (subtle, italic, displayed below detail with shadcn-style spacing)
                if !host.tags.is_empty() {
                    let tag_text = host.tags.join(", ");
                    let full_tag_text = format!("tag: {}", tag_text);

                    // Define tag area with proper clipping (stay within row bounds)
                    let tag_y = rect.min.y + 44.0;
                    let tag_bottom = rect.min.y + row_h - 4.0;
                    let tag_clip_rect = egui::Rect::from_min_max(
                        egui::pos2(rect.min.x + 46.0, tag_y),
                        egui::pos2(rect.max.x - 80.0, tag_bottom),  // Leave space for edit button
                    );

                    // Check if text fits
                    let text_width = ui.fonts(|f| {
                        f.layout_no_wrap(full_tag_text.clone(), egui::FontId::proportional(9.0), self.theme.fg_dim)
                            .size().x
                    });

                    let display_text = if text_width > tag_clip_rect.width() {
                        // Text too long, truncate with ellipsis
                        let ellipsis = String::from("...");
                        let ellipsis_width = ui.fonts(|f| {
                            f.layout_no_wrap(ellipsis.clone(), egui::FontId::proportional(9.0), self.theme.fg_dim)
                                .size().x
                        });

                        // Find max characters that fit
                        let mut fitted = String::from("tag: ");
                        let mut current_width = ui.fonts(|f| {
                            f.layout_no_wrap(fitted.clone(), egui::FontId::proportional(9.0), self.theme.fg_dim)
                                .size().x
                        });

                        for ch in tag_text.chars() {
                            let char_width = ui.fonts(|f| {
                                f.layout_no_wrap(ch.to_string(), egui::FontId::proportional(9.0), self.theme.fg_dim)
                                    .size().x
                            });

                            if current_width + char_width + ellipsis_width <= tag_clip_rect.width() {
                                fitted.push(ch);
                                current_width += char_width;
                            } else {
                                break;
                            }
                        }

                        if current_width + ellipsis_width <= tag_clip_rect.width() {
                            fitted.push_str(&ellipsis);
                        }

                        fitted
                    } else {
                        full_tag_text
                    };

                    // Draw with clipping to prevent overflow
                    ui.painter().with_clip_rect(tag_clip_rect).text(
                        egui::pos2(tag_clip_rect.min.x, tag_clip_rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        display_text,
                        egui::FontId::proportional(9.0),
                        self.theme.fg_dim,
                    );
                }
                // Edit button (only visible on hover)
                // Use visible clip rect right edge to avoid being clipped by panel borders
                let visible_right = ui.clip_rect().max.x;
                let btn_rect = egui::Rect::from_center_size(
                    egui::pos2(visible_right - 40.0, rect.min.y + 26.0),
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
                ui.add_space(SPACE_MD);
                ui.horizontal(|ui| {
                    ui.add_space(SPACE_XL);
                    ui.label(widgets::section_header(group, &self.theme));
                });
                ui.add_space(SPACE_XS);
                for (i, host) in self.hosts.iter().enumerate() {
                    if host.is_local || host.group != *group { continue; }
                    // Apply filter
                    if !self.host_filter.matches(host) {
                        continue;
                    }
                    let row_h = 58.0;  // Increased to accommodate tags without overflow
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
                        egui::pos2(rect.min.x + 24.0, rect.min.y + 18.0),
                        egui::Align2::LEFT_CENTER,
                        "@",
                        egui::FontId::proportional(12.0),
                        self.theme.accent,
                    );
                    ui.painter().text(
                        egui::pos2(rect.min.x + 46.0, rect.min.y + 18.0),
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
                        egui::pos2(rect.min.x + 46.0, rect.min.y + 34.0),
                        egui::Align2::LEFT_CENTER,
                        detail,
                        egui::FontId::proportional(10.0),
                        self.theme.fg_dim,
                    );

                    // Tags (subtle, italic, displayed below detail with shadcn-style spacing)
                    if !host.tags.is_empty() {
                        let tag_text = host.tags.join(", ");
                        let full_tag_text = format!("tag: {}", tag_text);

                        // Define tag area with proper clipping (stay within row bounds)
                        let tag_y = rect.min.y + 44.0;
                        let tag_bottom = rect.min.y + row_h - 4.0;
                        let tag_clip_rect = egui::Rect::from_min_max(
                            egui::pos2(rect.min.x + 46.0, tag_y),
                            egui::pos2(rect.max.x - 80.0, tag_bottom),  // Leave space for edit button
                        );

                        // Check if text fits
                        let text_width = ui.fonts(|f| {
                            f.layout_no_wrap(full_tag_text.clone(), egui::FontId::proportional(9.0), self.theme.fg_dim)
                                .size().x
                        });

                        let display_text = if text_width > tag_clip_rect.width() {
                            // Text too long, truncate with ellipsis
                            let ellipsis = String::from("...");
                            let ellipsis_width = ui.fonts(|f| {
                                f.layout_no_wrap(ellipsis.clone(), egui::FontId::proportional(9.0), self.theme.fg_dim)
                                    .size().x
                            });

                            // Find max characters that fit
                            let mut fitted = String::from("tag: ");
                            let mut current_width = ui.fonts(|f| {
                                f.layout_no_wrap(fitted.clone(), egui::FontId::proportional(9.0), self.theme.fg_dim)
                                    .size().x
                            });

                            for ch in tag_text.chars() {
                                let char_width = ui.fonts(|f| {
                                    f.layout_no_wrap(ch.to_string(), egui::FontId::proportional(9.0), self.theme.fg_dim)
                                        .size().x
                                });

                                if current_width + char_width + ellipsis_width <= tag_clip_rect.width() {
                                    fitted.push(ch);
                                    current_width += char_width;
                                } else {
                                    break;
                                }
                            }

                            if current_width + ellipsis_width <= tag_clip_rect.width() {
                                fitted.push_str(&ellipsis);
                            }

                            fitted
                        } else {
                            full_tag_text
                        };

                        // Draw with clipping to prevent overflow
                        ui.painter().with_clip_rect(tag_clip_rect).text(
                            egui::pos2(tag_clip_rect.min.x, tag_clip_rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            display_text,
                            egui::FontId::proportional(9.0),
                            self.theme.fg_dim,
                        );
                    }
                    // Edit button (only visible on hover)
                    // Use visible clip rect right edge to avoid being clipped by panel borders
                    let visible_right = ui.clip_rect().max.x;
                    let btn_rect = egui::Rect::from_center_size(
                        egui::pos2(visible_right - 40.0, rect.min.y + 26.0),
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

            ui.add_space(SPACE_XL);
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
            let current_time = ctx.input(|i| i.time);
            self.add_host_dialog.open_edit(idx, &host, current_time);
        }
        if let Some(host) = connect_history_host {
            self.add_tab_ssh(&host);
        }
        if clear_history {
            self.connection_history.clear();
            crate::config::save_history(&[]);
        }
    }

    /// Right-side drawer for adding / editing a host (shown in Hosts view)
    pub fn show_add_host_drawer(&mut self, ctx: &egui::Context) {
        // Poll the async test result
        let polled_result: Option<Result<String, String>> = self
            .add_host_dialog
            .test_conn_result
            .as_ref()
            .and_then(|arc| arc.lock().ok()?.take());
        if let Some(result) = polled_result {
            self.add_host_dialog.test_conn_state = match result {
                Ok(msg) => {
                    self.add_host_dialog.show_remove_key_button = false;
                    TestConnState::Success(msg)
                }
                Err(msg) => {
                    // Check if this is a host key verification error
                    let is_key_error = msg.contains("Host key verification failed") ||
                                      msg.contains("MITM attack");
                    self.add_host_dialog.show_remove_key_button = is_key_error;
                    TestConnState::Failed(msg)
                }
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
        let drawer_width = ctx.screen_rect().width().min(DRAWER_WIDTH).max(280.0);

        egui::SidePanel::right("add_host_drawer")
            .exact_width(drawer_width)
            .resizable(false)
            .frame(egui::Frame {
                fill: theme.bg_secondary,
                inner_margin: egui::Margin::symmetric(SPACE_XL, SPACE_MD),
                stroke: egui::Stroke::new(1.0, theme.border),
                ..Default::default()
            })
            .show(ctx, |ui| {
                // Header with title and close button
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
                            self.add_host_dialog.open = false;
                        }
                        if self.add_host_dialog.edit_index.is_some() {
                            if ui.add(
                                egui::Button::new(egui::RichText::new("\u{1F5D1}").size(FONT_BASE))
                                    .frame(false)
                            ).on_hover_text(lang.t("delete"))
                            .clicked() {
                                self.confirm_delete_host = self.add_host_dialog.edit_index;
                                self.add_host_dialog.open = false;
                            }
                        }
                    });
                });
                ui.add_space(SPACE_LG);

                // Scrollable content area
                egui::ScrollArea::vertical()
                    .id_salt("drawer_scroll")
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = SPACE_MD;

                        ui.label(widgets::field_label(lang.t("label"), &theme));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.add_host_dialog.name)
                                .hint_text(egui::RichText::new("My Server").color(theme.hint_color()).italics())
                                .desired_width(f32::INFINITY)
                                .text_color(theme.fg_primary)
                                .font(egui::FontId::proportional(13.0))
                        );

                        ui.add_space(SPACE_XS);

                        ui.label(widgets::field_label(lang.t("host_ip"), &theme));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.add_host_dialog.host)
                                .hint_text(egui::RichText::new("192.168.1.1").color(theme.hint_color()).italics())
                                .desired_width(f32::INFINITY)
                                .text_color(theme.fg_primary)
                                .font(egui::FontId::proportional(13.0))
                        );

                        ui.add_space(SPACE_XS);

                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(widgets::field_label(lang.t("port"), &theme));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.add_host_dialog.port)
                                        .desired_width(70.0)
                                        .text_color(theme.fg_primary)
                                        .hint_text(egui::RichText::new("22").color(theme.hint_color()).italics())
                                        .font(egui::FontId::proportional(13.0))
                                );
                            });
                            ui.add_space(SPACE_SM);
                            ui.vertical(|ui| {
                                ui.label(widgets::field_label(lang.t("group"), &theme));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.add_host_dialog.group)
                                        .hint_text(egui::RichText::new("Production").color(theme.hint_color()).italics())
                                        .desired_width(f32::INFINITY)
                                        .text_color(theme.fg_primary)
                                        .font(egui::FontId::proportional(13.0))
                                );
                            });
                        });

                        ui.add_space(SPACE_SM);

                        ui.vertical(|ui| {
                            ui.label(widgets::field_label(lang.t("tag"), &theme));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.add_host_dialog.tags)
                                    .hint_text(egui::RichText::new("web, database, production").color(theme.hint_color()).italics())
                                    .desired_width(f32::INFINITY)
                                    .text_color(theme.fg_primary)
                                    .font(egui::FontId::proportional(13.0))
                            );
                        });

                        ui.add_space(SPACE_SM);
                        ui.separator();
                        ui.add_space(SPACE_SM);

                        // ── Credential selection ──
                        ui.label(widgets::field_label(lang.t("authentication"), &theme));
                        ui.add_space(SPACE_XS);

                        // Username (always visible for SSH login)
                        ui.label(widgets::field_label(lang.t("username"), &theme));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.add_host_dialog.username)
                                .hint_text(egui::RichText::new("root").color(theme.hint_color()).italics())
                                .desired_width(f32::INFINITY)
                                .text_color(theme.fg_primary)
                                .font(egui::FontId::proportional(13.0))
                        );
                        ui.add_space(SPACE_SM);

                        // Credential mode selector: None / Existing / New
                        ui.horizontal(|ui| {
                            ui.selectable_value(
                                &mut self.add_host_dialog.credential_mode,
                                CredentialMode::None,
                                egui::RichText::new(lang.t("credential_none")).size(12.0),
                            );
                            if !self.credentials.is_empty() {
                                ui.selectable_value(
                                    &mut self.add_host_dialog.credential_mode,
                                    CredentialMode::Existing,
                                    egui::RichText::new(lang.t("credential_existing")).size(12.0),
                                );
                            }
                            ui.selectable_value(
                                &mut self.add_host_dialog.credential_mode,
                                CredentialMode::Inline,
                                egui::RichText::new(lang.t("credential_inline")).size(12.0),
                            );
                        });
                        ui.add_space(SPACE_SM);

                        match self.add_host_dialog.credential_mode {
                            CredentialMode::None => {
                                // No authentication
                            }
                            CredentialMode::Existing => {
                                // Dropdown to pick an existing credential
                                ui.label(widgets::field_label(lang.t("select_credential"), &theme));
                                ui.add_space(SPACE_XS);
                                let current_label = self.add_host_dialog.selected_credential_id.as_ref()
                                    .and_then(|id| self.credentials.iter().find(|c| c.id == *id))
                                    .map(|c| c.name.clone())
                                    .unwrap_or_else(|| "---".to_string());
                                egui::ComboBox::from_id_salt("credential_selector")
                                    .selected_text(egui::RichText::new(&current_label).size(12.0))
                                    .width(ui.available_width() - 8.0)
                                    .show_ui(ui, |ui| {
                                        widgets::style_dropdown(ui, &self.theme);
                                        for cred in &self.credentials {
                                            let label = format!("{} ({})", cred.name, match &cred.credential_type {
                                                crate::config::CredentialType::Password { .. } => lang.t("password"),
                                                crate::config::CredentialType::SshKey { .. } => lang.t("ssh_key"),
                                            });
                                            let is_selected = self.add_host_dialog.selected_credential_id.as_ref() == Some(&cred.id);
                                            if ui.selectable_label(is_selected, egui::RichText::new(&label).size(12.0)).clicked() {
                                                self.add_host_dialog.selected_credential_id = Some(cred.id.clone());
                                            }
                                        }
                                    });
                            }
                            CredentialMode::Inline => {
                                // Inline credential creation (same fields as before)
                                ui.horizontal(|ui| {
                                    ui.selectable_value(
                                        &mut self.add_host_dialog.auth_method,
                                        AuthMethodChoice::Password,
                                        egui::RichText::new(lang.t("password")).size(12.0),
                                    );
                                    ui.selectable_value(
                                        &mut self.add_host_dialog.auth_method,
                                        AuthMethodChoice::Key,
                                        egui::RichText::new(lang.t("ssh_key")).size(12.0),
                                    );
                                });
                                ui.add_space(SPACE_XS);

                                match self.add_host_dialog.auth_method {
                                    AuthMethodChoice::Password => {
                                        ui.label(widgets::field_label(lang.t("password"), &theme));
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.add_host_dialog.password)
                                                .password(true)
                                                .hint_text(egui::RichText::new("Enter password").color(theme.hint_color()).italics())
                                                .desired_width(f32::INFINITY)
                                                .text_color(theme.fg_primary)
                                                .font(egui::FontId::proportional(13.0))
                                        );
                                    }
                                    AuthMethodChoice::Key => {
                                        ui.horizontal(|ui| {
                                            let local_color = if self.add_host_dialog.key_source == KeySourceChoice::LocalFile { theme.accent } else { theme.fg_dim };
                                            if ui.add(
                                                egui::Button::new(egui::RichText::new("Local File").color(local_color).size(12.0))
                                                    .stroke(egui::Stroke::NONE).fill(egui::Color32::TRANSPARENT)
                                            ).clicked() {
                                                self.add_host_dialog.key_source = KeySourceChoice::LocalFile;
                                            }
                                            ui.add_space(SPACE_SM);
                                            let import_color = if self.add_host_dialog.key_source == KeySourceChoice::ImportContent { theme.accent } else { theme.fg_dim };
                                            if ui.add(
                                                egui::Button::new(egui::RichText::new("Import Content").color(import_color).size(12.0))
                                                    .stroke(egui::Stroke::NONE).fill(egui::Color32::TRANSPARENT)
                                            ).clicked() {
                                                self.add_host_dialog.key_source = KeySourceChoice::ImportContent;
                                            }
                                        });
                                        ui.add_space(SPACE_XS);

                                        match self.add_host_dialog.key_source {
                                            KeySourceChoice::LocalFile => {
                                                ui.label(widgets::field_label(lang.t("key_path"), &theme));
                                                ui.add(
                                                    egui::TextEdit::singleline(&mut self.add_host_dialog.key_path)
                                                        .hint_text(egui::RichText::new("~/.ssh/id_rsa").color(theme.hint_color()).italics())
                                                        .desired_width(f32::INFINITY)
                                                        .text_color(theme.fg_primary)
                                                        .font(egui::FontId::proportional(13.0))
                                                );
                                            }
                                            KeySourceChoice::ImportContent => {
                                                ui.label(widgets::field_label("Private Key:", &theme));
                                                ui.add(
                                                    egui::TextEdit::multiline(&mut self.add_host_dialog.key_content)
                                                        .id(egui::Id::new("import_private_key"))
                                                        .hint_text(egui::RichText::new("-----BEGIN OPENSSH PRIVATE KEY-----\n...\n-----END OPENSSH PRIVATE KEY-----").color(theme.hint_color()).italics())
                                                        .font(egui::FontId::monospace(12.0))
                                                        .desired_width(f32::INFINITY)
                                                        .desired_rows(6)
                                                        .frame(true)
                                                        .text_color(theme.fg_primary)
                                                );
                                            }
                                        }

                                        if self.add_host_dialog.key_in_keychain {
                                            ui.label(egui::RichText::new(lang.t("key_stored_in_keychain")).color(theme.green).size(11.0));
                                        }
                                        ui.add_space(SPACE_XS);
                                        ui.label(widgets::field_label(lang.t("key_passphrase"), &theme));
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.add_host_dialog.key_passphrase)
                                                .password(true)
                                                .hint_text(egui::RichText::new("Leave empty if none").color(theme.hint_color()).italics())
                                                .desired_width(f32::INFINITY)
                                                .text_color(theme.fg_primary)
                                                .font(egui::FontId::proportional(13.0))
                                        );
                                    }
                                }
                            }
                        }

                        ui.add_space(SPACE_SM);

                        // Startup commands
                        ui.label(widgets::field_label(lang.t("startup_commands"), &theme));
                        ui.add(
                            egui::TextEdit::multiline(&mut self.add_host_dialog.startup_commands)
                                .desired_rows(3)
                                .desired_width(f32::INFINITY)
                                .font(egui::FontId::monospace(12.0))
                                .hint_text(egui::RichText::new(format!("{}\nexport PATH=/usr/local/bin:$PATH\nsource ~/.profile", lang.t("startup_commands_hint"))).color(theme.hint_color()).italics())
                                .text_color(theme.fg_primary)
                        );

                        ui.add_space(SPACE_SM);

                        // Agent forwarding
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.add_host_dialog.agent_forwarding, egui::RichText::new(lang.t("agent_forwarding")).color(theme.fg_primary));
                        });
                        ui.label(egui::RichText::new(lang.t("agent_forwarding_desc")).color(theme.fg_dim).size(11.0));

                        ui.add_space(SPACE_SM);

                        // Jump Host
                        ui.label(widgets::field_label(lang.t("jump_host"), &theme));
                        let current_label = self.add_host_dialog.jump_host
                            .as_deref()
                            .unwrap_or(lang.t("jump_host_none"));
                        let editing_name = self.add_host_dialog.name.clone();
                        egui::ComboBox::from_id_salt("jump_host_combo")
                            .selected_text(current_label)
                            .width(ui.available_width() - 8.0)
                            .show_ui(ui, |ui| {
                                widgets::style_dropdown(ui, &self.theme);
                                if ui.selectable_label(self.add_host_dialog.jump_host.is_none(), lang.t("jump_host_none")).clicked() {
                                    self.add_host_dialog.jump_host = None;
                                }
                                for h in &self.hosts {
                                    if h.is_local || h.name == editing_name {
                                        continue;
                                    }
                                    let selected = self.add_host_dialog.jump_host.as_deref() == Some(&h.name);
                                    if ui.selectable_label(selected, &h.name).clicked() {
                                        self.add_host_dialog.jump_host = Some(h.name.clone());
                                    }
                                }
                            });
                        ui.label(egui::RichText::new(lang.t("jump_host_desc")).color(theme.fg_dim).size(11.0));

                        ui.add_space(SPACE_SM);
                        ui.separator();
                        ui.add_space(SPACE_SM);

                        // Port forwards section
                        ui.label(widgets::field_label(lang.t("port_forwards"), &theme));
                        ui.add_space(SPACE_XS);

                        // List existing port forwards
                        let mut remove_fwd_idx = None;
                        for (fi, fwd) in self.add_host_dialog.port_forwards.iter().enumerate() {
                            ui.horizontal(|ui| {
                                let kind_label = match fwd.kind {
                                    crate::config::ForwardKind::Local => "L",
                                    crate::config::ForwardKind::Remote => "R",
                                };
                                ui.label(egui::RichText::new(kind_label).color(theme.accent).size(11.0).strong());
                                ui.label(egui::RichText::new(format!(
                                    "{}:{} -> {}:{}",
                                    fwd.local_host, fwd.local_port,
                                    fwd.remote_host, fwd.remote_port
                                )).color(theme.fg_primary).size(11.0));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.add(
                                        egui::Button::new(egui::RichText::new("✕").color(theme.red).size(11.0))
                                            .frame(false)
                                    ).clicked() {
                                        remove_fwd_idx = Some(fi);
                                    }
                                });
                            });
                        }
                        if let Some(idx) = remove_fwd_idx {
                            self.add_host_dialog.port_forwards.remove(idx);
                        }

                        ui.add_space(SPACE_XS);

                        // Add new forward form
                        ui.horizontal(|ui| {
                            ui.selectable_value(
                                &mut self.add_host_dialog.new_forward_kind,
                                crate::config::ForwardKind::Local,
                                egui::RichText::new(lang.t("local_forward")).size(11.0),
                            );
                            ui.selectable_value(
                                &mut self.add_host_dialog.new_forward_kind,
                                crate::config::ForwardKind::Remote,
                                egui::RichText::new(lang.t("remote_forward")).size(11.0),
                            );
                        });
                        ui.add_space(SPACE_XS);

                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(lang.t("forward_local_host")).color(theme.fg_dim).size(10.0));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.add_host_dialog.new_forward_local_host)
                                        .desired_width(100.0)
                                        .text_color(theme.fg_primary)
                                        .font(egui::FontId::proportional(12.0))
                                        .hint_text(egui::RichText::new("localhost").color(theme.hint_color()).italics())
                                );
                            });
                            ui.add_space(SPACE_XS);
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(lang.t("forward_local_port")).color(theme.fg_dim).size(10.0));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.add_host_dialog.new_forward_local_port)
                                        .desired_width(50.0)
                                        .text_color(theme.fg_primary)
                                        .font(egui::FontId::proportional(12.0))
                                        .hint_text(egui::RichText::new("8080").color(theme.hint_color()).italics())
                                );
                            });
                        });
                        ui.add_space(SPACE_XS / 2.0);
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(lang.t("forward_remote_host")).color(theme.fg_dim).size(10.0));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.add_host_dialog.new_forward_remote_host)
                                        .desired_width(100.0)
                                        .text_color(theme.fg_primary)
                                        .font(egui::FontId::proportional(12.0))
                                        .hint_text(egui::RichText::new("localhost").color(theme.hint_color()).italics())
                                );
                            });
                            ui.add_space(SPACE_XS);
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(lang.t("forward_remote_port")).color(theme.fg_dim).size(10.0));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.add_host_dialog.new_forward_remote_port)
                                        .desired_width(50.0)
                                        .text_color(theme.fg_primary)
                                        .font(egui::FontId::proportional(12.0))
                                        .hint_text(egui::RichText::new("3000").color(theme.hint_color()).italics())
                                );
                            });
                        });
                        ui.add_space(SPACE_XS);
                        if ui.add(widgets::text_button(lang.t("add_forward"), theme.accent)).clicked() {
                            let lp: u16 = self.add_host_dialog.new_forward_local_port.trim().parse().unwrap_or(0);
                            let rp: u16 = self.add_host_dialog.new_forward_remote_port.trim().parse().unwrap_or(0);
                            if lp > 0 && rp > 0 {
                                self.add_host_dialog.port_forwards.push(
                                    crate::config::PortForwardConfig {
                                        kind: self.add_host_dialog.new_forward_kind.clone(),
                                        local_host: self.add_host_dialog.new_forward_local_host.trim().to_owned(),
                                        local_port: lp,
                                        remote_host: self.add_host_dialog.new_forward_remote_host.trim().to_owned(),
                                        remote_port: rp,
                                    }
                                );
                                self.add_host_dialog.new_forward_local_port.clear();
                                self.add_host_dialog.new_forward_remote_port.clear();
                            }
                        }

                        ui.add_space(SPACE_SM);

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
                                // Show "Remove old key" button for host key verification failures
                                if self.add_host_dialog.show_remove_key_button {
                                    ui.add_space(SPACE_SM);
                                    if ui.add(
                                        egui::Button::new(egui::RichText::new("Remove old key").color(theme.fg_primary).size(11.0))
                                            .fill(theme.button_bg)
                                            .rounding(4.0)
                                            .stroke(egui::Stroke::new(1.0, theme.red))
                                            .min_size(egui::vec2(0.0, 24.0))
                                    ).clicked() {
                                        self.add_host_dialog.show_remove_key_button = false;
                                        let host = self.add_host_dialog.host.trim().to_owned();
                                        let port: u16 = self.add_host_dialog.port.trim().parse().unwrap_or(22);
                                        match crate::ssh::remove_known_hosts_key(&host, port) {
                                            Ok(count) => {
                                                self.add_host_dialog.remove_key_message =
                                                    format!("Removed {} old key(s) from known_hosts. Try again.", count);
                                            }
                                            Err(e) => {
                                                self.add_host_dialog.remove_key_message =
                                                    format!("Failed to remove key: {}", e);
                                            }
                                        }
                                    }
                                }
                                // Show message after key removal attempt
                                if !self.add_host_dialog.remove_key_message.is_empty() {
                                    ui.add_space(SPACE_XS);
                                    ui.label(egui::RichText::new(&self.add_host_dialog.remove_key_message)
                                        .color(theme.accent).size(11.0));
                                }
                            }
                        }

                        ui.add_space(SPACE_LG);
                        ui.separator();
                        ui.add_space(SPACE_MD);

                        ui.horizontal(|ui| {
                            let is_testing = matches!(self.add_host_dialog.test_conn_state, TestConnState::Testing);
                            if ui.add_enabled(
                                !is_testing,
                                widgets::primary_button(lang.t("test"), &theme)
                            ).clicked() {
                                test_clicked = true;
                            }

                            ui.add_space(SPACE_SM);

                            if ui.add(widgets::secondary_button(lang.t("cancel"), &theme)).clicked() {
                                self.add_host_dialog.open = false;
                            }

                            ui.add_space(SPACE_SM);

                            if ui.add(widgets::primary_button(lang.t("save"), &theme)).clicked() {
                                save_clicked = true;
                            }
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
                // Build ResolvedAuth based on credential mode
                let resolved = match self.add_host_dialog.credential_mode {
                    CredentialMode::None => crate::config::ResolvedAuth::None,
                    CredentialMode::Existing => {
                        if let Some(ref cid) = self.add_host_dialog.selected_credential_id {
                            if let Some(cred) = self.credentials.iter().find(|c| c.id == *cid) {
                                crate::config::resolve_credential(cred)
                            } else {
                                crate::config::ResolvedAuth::None
                            }
                        } else {
                            crate::config::ResolvedAuth::None
                        }
                    }
                    CredentialMode::Inline => {
                        match self.add_host_dialog.auth_method {
                            AuthMethodChoice::Password => {
                                let pw = self.add_host_dialog.password.clone();
                                if pw.is_empty() {
                                    crate::config::ResolvedAuth::None
                                } else {
                                    crate::config::ResolvedAuth::Password { password: pw }
                                }
                            }
                            AuthMethodChoice::Key => {
                                let key_content = match self.add_host_dialog.key_source {
                                    KeySourceChoice::LocalFile => {
                                        let key_path = self.add_host_dialog.key_path.trim().to_owned();
                                        if key_path.is_empty() {
                                            String::new()
                                        } else {
                                            let expanded = if key_path.starts_with('~') {
                                                if let Some(home) = dirs::home_dir() {
                                                    home.join(&key_path[2..]).to_string_lossy().to_string()
                                                } else {
                                                    key_path.clone()
                                                }
                                            } else {
                                                key_path
                                            };
                                            std::fs::read_to_string(&expanded).unwrap_or_default()
                                        }
                                    }
                                    KeySourceChoice::ImportContent => {
                                        self.add_host_dialog.key_content.trim().to_owned()
                                    }
                                };
                                let passphrase = if self.add_host_dialog.key_passphrase.is_empty() {
                                    None
                                } else {
                                    Some(self.add_host_dialog.key_passphrase.clone())
                                };
                                crate::config::ResolvedAuth::Key { key_content, passphrase }
                            }
                        }
                    }
                };

                let result_arc: Arc<Mutex<Option<Result<String, String>>>> =
                    Arc::new(Mutex::new(None));
                self.add_host_dialog.test_conn_result = Some(Arc::clone(&result_arc));
                self.add_host_dialog.test_conn_state = TestConnState::Testing;
                self.add_host_dialog.error.clear();

                let agent_fwd = self.add_host_dialog.agent_forwarding;
                self.runtime.spawn(async move {
                    let settings = crate::config::load_settings();
                    let result = test_connection(host, port, username, resolved, settings.ssh_keepalive_interval, agent_fwd).await;
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

            // Determine credential_id based on mode
            let credential_id = match self.add_host_dialog.credential_mode {
                CredentialMode::None => None,
                CredentialMode::Existing => {
                    self.add_host_dialog.selected_credential_id.clone()
                }
                CredentialMode::Inline => {
                    // Create a new credential entity from inline fields
                    let cred = match self.add_host_dialog.auth_method {
                        AuthMethodChoice::Password => {
                            let pw = self.add_host_dialog.password.clone();
                            if pw.is_empty() {
                                None  // No credential needed
                            } else {
                                let cred = crate::config::Credential::new_password(
                                    format!("{} (password)", name),
                                    self.add_host_dialog.username.trim().to_owned(),
                                );
                                crate::config::store_credential_secret(&cred.id, &cred.name, "password", &pw);
                                Some(cred)
                            }
                        }
                        AuthMethodChoice::Key => {
                            let key_content = match self.add_host_dialog.key_source {
                                KeySourceChoice::LocalFile => {
                                    let key_path = self.add_host_dialog.key_path.trim();
                                    if key_path.is_empty() {
                                        self.add_host_dialog.error = "Key path is required.".to_owned();
                                        return;
                                    }
                                    let expanded = if key_path.starts_with('~') {
                                        if let Some(home) = dirs::home_dir() {
                                            home.join(&key_path[2..]).to_string_lossy().to_string()
                                        } else {
                                            key_path.to_owned()
                                        }
                                    } else {
                                        key_path.to_owned()
                                    };
                                    std::fs::read_to_string(&expanded).unwrap_or_default()
                                }
                                KeySourceChoice::ImportContent => {
                                    let kc = self.add_host_dialog.key_content.trim().to_owned();
                                    if kc.is_empty() {
                                        self.add_host_dialog.error = "Private key content is required.".to_owned();
                                        return;
                                    }
                                    kc
                                }
                            };
                            let has_passphrase = !self.add_host_dialog.key_passphrase.is_empty();
                            let mut cred = crate::config::Credential::new_ssh_key(
                                format!("{} (key)", name),
                                self.add_host_dialog.key_path.trim().to_owned(),
                                false,
                                has_passphrase,
                            );
                            if !key_content.is_empty() {
                                crate::config::store_credential_secret(&cred.id, &cred.name, "privatekey", &key_content);
                                if let crate::config::CredentialType::SshKey { ref mut key_in_keychain, .. } = cred.credential_type {
                                    *key_in_keychain = true;
                                }
                            }
                            if has_passphrase {
                                crate::config::store_credential_secret(&cred.id, &cred.name, "passphrase", &self.add_host_dialog.key_passphrase);
                            }
                            Some(cred)
                        }
                    };

                    if let Some(cred) = cred {
                        let id = cred.id.clone();
                        self.credentials.push(cred);
                        self.save_credentials();
                        Some(id)
                    } else {
                        None
                    }
                }
            };

            let mut entry = HostEntry::new_ssh(
                name,
                host,
                port,
                self.add_host_dialog.username.trim().to_owned(),
                self.add_host_dialog.group.trim().to_owned(),
                credential_id,
                self.add_host_dialog.startup_commands
                    .lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect(),
            );

            entry.agent_forwarding = self.add_host_dialog.agent_forwarding;
            entry.jump_host = self.add_host_dialog.jump_host.clone();
            entry.port_forwards = self.add_host_dialog.port_forwards.clone();

            // Parse tags from comma-separated string
            let tags: Vec<String> = self.add_host_dialog.tags
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            entry.tags = tags;

            if let Some(idx) = self.add_host_dialog.edit_index {
                if idx < self.hosts.len() {
                    self.hosts[idx] = entry;
                }
            } else {
                self.hosts.push(entry);
            }

            self.save_hosts();
            self.add_host_dialog.open = false;
        }
    } // Close show_add_host_drawer

}
