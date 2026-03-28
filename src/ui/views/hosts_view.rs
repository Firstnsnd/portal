//! # Hosts View
//!
//! Rendering for the hosts management page.

use eframe::egui;
use std::sync::{Arc, Mutex};

// These types are defined in pane_view.rs
use crate::ui::pane_view::{WindowContext, ViewActions};
use crate::ui::pane::AppWindow;
use crate::config::HostEntry;
use crate::ssh::test_connection;
use crate::ui::types::dialogs::{AuthMethodChoice, TestConnState, KeySourceChoice, CredentialMode};
use crate::ui::i18n::format_time_ago;
use crate::ui::tokens::*;
use crate::ui::widgets;

/// Render hosts view for this window
pub fn render_hosts_view(
    window: &mut AppWindow,
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    cx: &mut WindowContext,
) -> ViewActions {
    let mut actions = ViewActions::default();
    let mut connect_ssh_host_idx: Option<usize> = None;
    let mut edit_host_index: Option<usize> = None;
    let mut connect_history_host: Option<HostEntry> = None;

    ui.add_space(0.0);

    // Collect all unique groups and tags
    let mut all_groups: Vec<String> = Vec::new();
    let mut all_tags: Vec<String> = Vec::new();
    for host in cx.hosts.iter() {
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

    // Top navigation bar
    egui::TopBottomPanel::top("hosts_nav_bar")
        .frame(egui::Frame {
            fill: cx.theme.bg_secondary,
            inner_margin: egui::Margin::symmetric(8.0, 8.0),
            stroke: egui::Stroke::NONE,
            ..Default::default()
        })
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(cx.language.t("hosts")).color(cx.theme.fg_dim).size(FONT_BASE).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(widgets::text_button(cx.language.t("new_host"), cx.theme.accent)).clicked() {
                        let current_time = ctx.input(|i| i.time);
                        window.add_host_dialog.open_new(current_time);
                    }
                });
            });
            ui.add_space(4.0);
        });

    egui::CentralPanel::default()
        .frame(egui::Frame {
            fill: cx.theme.bg_primary,
            ..Default::default()
        })
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("hosts_page_scroll")
                .show(ui, |ui| {
                    ui.add_space(SPACE_SM);

                    // Filter bar
                    ui.horizontal(|ui| {
                        let input_bg = ui.visuals().extreme_bg_color;
                        let border = cx.theme.input_border;
                        ui.style_mut().visuals.widgets.inactive.bg_fill = input_bg;
                        ui.style_mut().visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, border);
                        ui.style_mut().visuals.widgets.hovered.bg_fill = input_bg;
                        ui.style_mut().visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, cx.theme.focus_ring);
                        ui.style_mut().visuals.widgets.active.bg_fill = input_bg;
                        ui.style_mut().visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, cx.theme.accent);
                        ui.style_mut().visuals.widgets.open.bg_fill = input_bg;
                        ui.style_mut().visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0, cx.theme.accent);

                        ui.add_space(ui.available_width() - 210.0);

                        // Group filter
                        let group_label = if window.host_filter.group.is_empty() {
                            cx.language.t("group").to_string()
                        } else {
                            window.host_filter.group.clone()
                        };
                        egui::ComboBox::from_id_salt("group_filter")
                            .selected_text(egui::RichText::new(group_label).color(cx.theme.accent).size(FONT_MD))
                            .width(90.0)
                            .show_ui(ui, |ui| {
                                widgets::style_dropdown(ui, cx.theme);
                                if ui.add(egui::Button::new(
                                    egui::RichText::new(cx.language.t("snippet_default_group"))
                                        .color(if window.host_filter.group.is_empty() { cx.theme.accent } else { cx.theme.fg_primary })
                                        .size(FONT_MD)
                                ).frame(false)).clicked() {
                                    window.host_filter.group.clear();
                                    ui.close_menu();
                                }
                                for group in &all_groups {
                                    if ui.add(egui::Button::new(
                                        egui::RichText::new(group)
                                            .color(if window.host_filter.group == *group { cx.theme.accent } else { cx.theme.fg_primary })
                                            .size(FONT_MD)
                                    ).frame(false)).clicked() {
                                        window.host_filter.group = group.clone();
                                        ui.close_menu();
                                    }
                                }
                            });

                        ui.add_space(SPACE_SM - 2.0);

                        // Tag filter
                        let tag_label = if window.host_filter.tag.is_empty() {
                            cx.language.t("tag").to_string()
                        } else {
                            window.host_filter.tag.clone()
                        };
                        egui::ComboBox::from_id_salt("tag_filter")
                            .selected_text(egui::RichText::new(tag_label).color(cx.theme.accent).size(FONT_MD))
                            .width(90.0)
                            .show_ui(ui, |ui| {
                                widgets::style_dropdown(ui, cx.theme);
                                if ui.add(egui::Button::new(
                                    egui::RichText::new(cx.language.t("snippet_default_group"))
                                        .color(if window.host_filter.tag.is_empty() { cx.theme.accent } else { cx.theme.fg_primary })
                                        .size(FONT_MD)
                                ).frame(false)).clicked() {
                                    window.host_filter.tag.clear();
                                    ui.close_menu();
                                }
                                for tag in &all_tags {
                                    if ui.add(egui::Button::new(
                                        egui::RichText::new(tag)
                                            .color(if window.host_filter.tag == *tag { cx.theme.accent } else { cx.theme.fg_primary })
                                            .size(FONT_MD)
                                    ).frame(false)).clicked() {
                                        window.host_filter.tag = tag.clone();
                                        ui.close_menu();
                                    }
                                }
                            });

                        ui.add_space(SPACE_SM);

                        if window.host_filter.is_active() {
                            if ui.add(widgets::text_button(cx.language.t("clear_history"), cx.theme.accent)).clicked() {
                                window.host_filter.clear();
                            }
                        }
                    });

                    ui.add_space(SPACE_SM);

                    // RECENT CONNECTIONS section
                    {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        let history = &cx.connection_history;
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
                                ui.label(egui::RichText::new(cx.language.t("recent_connections")).color(cx.theme.fg_dim).size(FONT_XS).strong());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.add_space(SPACE_XL);
                                    if ui.add(
                                        egui::Button::new(egui::RichText::new(cx.language.t("clear_history")).color(cx.theme.fg_dim).size(10.0))
                                            .frame(false)
                                    ).clicked() {
                                        actions.clear_history = true;
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
                                        0.0, cx.theme.hover_shadow,
                                    );
                                    ui.painter().rect_filled(rect, 0.0, cx.theme.hover_bg);
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
                                    cx.theme.accent,
                                );
                                ui.painter().text(
                                    egui::pos2(rect.min.x + 46.0, rect.min.y + 14.0),
                                    egui::Align2::LEFT_CENTER,
                                    &display_name,
                                    egui::FontId::proportional(13.0),
                                    cx.theme.fg_primary,
                                );
                                let detail = format!("{}@{}:{}", record.username, record.host, record.port);
                                ui.painter().text(
                                    egui::pos2(rect.min.x + 46.0, rect.min.y + 30.0),
                                    egui::Align2::LEFT_CENTER,
                                    &detail,
                                    egui::FontId::proportional(10.0),
                                    cx.theme.fg_dim,
                                );
                                let secs_ago = now.saturating_sub(record.timestamp);
                                let time_text = format_time_ago(secs_ago, &cx.language);
                                let visible_right = ui.clip_rect().max.x;

                                let time_galley = ui.painter().layout_no_wrap(
                                    time_text.clone(),
                                    egui::FontId::proportional(10.0),
                                    cx.theme.fg_dim,
                                );
                                let time_width = time_galley.size().x;
                                let time_x = visible_right - 24.0;
                                ui.painter().text(
                                    egui::pos2(time_x, rect.min.y + 22.0),
                                    egui::Align2::RIGHT_CENTER,
                                    &time_text,
                                    egui::FontId::proportional(10.0),
                                    cx.theme.fg_dim,
                                );

                                if hovered {
                                    let btn_width = 56.0;
                                    let btn_x = time_x - time_width - 8.0;
                                    let btn_rect = egui::Rect::from_center_size(
                                        egui::pos2(btn_x - btn_width / 2.0, rect.min.y + 22.0),
                                        egui::vec2(btn_width, 20.0),
                                    );
                                    let pointer_pos = ui.ctx().input(|i| i.pointer.hover_pos());
                                    let over_btn = pointer_pos.map_or(false, |p| btn_rect.contains(p));
                                    let btn_bg = if over_btn { cx.theme.accent } else { cx.theme.bg_elevated };
                                    let btn_text_color = if over_btn { cx.theme.bg_primary } else { cx.theme.accent };
                                    ui.painter().rect(btn_rect, 4.0, btn_bg, egui::Stroke::new(1.0, cx.theme.accent));
                                    ui.painter().text(
                                        btn_rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        cx.language.t("connect"),
                                        egui::FontId::proportional(10.0),
                                        btn_text_color,
                                    );
                                    if resp.clicked() && over_btn {
                                        if let Some(host_entry) = cx.hosts.iter().find(|h| {
                                            !h.is_local && h.host == record.host && h.port == record.port && h.username == record.username
                                        }).cloned() {
                                            connect_history_host = Some(host_entry);
                                        } else {
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
                                    if let Some(host_entry) = cx.hosts.iter().find(|h| {
                                        !h.is_local && h.host == record.host && h.port == record.port && h.username == record.username
                                    }).cloned() {
                                        connect_history_host = Some(host_entry);
                                    } else {
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
                        ui.label(egui::RichText::new(cx.language.t("local")).color(cx.theme.fg_dim).size(FONT_XS).strong());
                    });
                    ui.add_space(SPACE_XS);

                    for host in cx.hosts.iter() {
                        if !host.is_local { continue; }
                        let width = ui.available_width();
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(width, 36.0),
                            egui::Sense::click(),
                        );
                        if resp.hovered() {
                            ui.painter().rect_filled(
                                egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.max.y - 1.0), rect.max),
                                0.0, cx.theme.hover_shadow,
                            );
                            ui.painter().rect_filled(rect, 0.0, cx.theme.hover_bg);
                        }
                        ui.painter().text(
                            egui::pos2(rect.min.x + 24.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            ">_",
                            egui::FontId::proportional(12.0),
                            cx.theme.green,
                        );
                        ui.painter().text(
                            egui::pos2(rect.min.x + 48.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            &host.name,
                            egui::FontId::proportional(13.0),
                            cx.theme.fg_primary,
                        );
                        if resp.clicked() {
                            actions.add_local_tab = true;
                        }
                    }

                    ui.add_space(SPACE_XL);

                    // SSH HOSTS section
                    ui.horizontal(|ui| {
                        ui.add_space(SPACE_XL);
                        ui.label(egui::RichText::new(cx.language.t("ssh_hosts")).color(cx.theme.fg_dim).size(FONT_XS).strong());
                    });
                    ui.add_space(SPACE_XS);

                    // Collect groups
                    let mut groups: Vec<String> = Vec::new();
                    for host in cx.hosts.iter() {
                        if !host.is_local && !host.group.is_empty() && !groups.contains(&host.group) {
                            if window.host_filter.matches(host) {
                                groups.push(host.group.clone());
                            }
                        }
                    }

                    // Ungrouped SSH hosts
                    for (i, host) in cx.hosts.iter().enumerate() {
                        if host.is_local || !host.group.is_empty() { continue; }
                        if !window.host_filter.matches(host) { continue; }

                        let row_h = 58.0;
                        let width = ui.available_width();
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(width, row_h),
                            egui::Sense::click(),
                        );
                        let hovered = resp.hovered();
                        if hovered {
                            ui.painter().rect_filled(
                                egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.max.y - 1.0), rect.max),
                                0.0, cx.theme.hover_shadow,
                            );
                            ui.painter().rect_filled(rect, 0.0, cx.theme.hover_bg);
                        }
                        ui.painter().text(
                            egui::pos2(rect.min.x + 24.0, rect.min.y + 18.0),
                            egui::Align2::LEFT_CENTER,
                            "@",
                            egui::FontId::proportional(12.0),
                            cx.theme.accent,
                        );
                        ui.painter().text(
                            egui::pos2(rect.min.x + 46.0, rect.min.y + 18.0),
                            egui::Align2::LEFT_CENTER,
                            &host.name,
                            egui::FontId::proportional(13.0),
                            cx.theme.fg_primary,
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
                            cx.theme.fg_dim,
                        );

                        if !host.tags.is_empty() {
                            let tag_text = format!("tag: {}", host.tags.join(", "));
                            let tag_clip_rect = egui::Rect::from_min_max(
                                egui::pos2(rect.min.x + 46.0, rect.min.y + 44.0),
                                egui::pos2(rect.max.x - 80.0, rect.min.y + row_h - 4.0),
                            );
                            ui.painter().with_clip_rect(tag_clip_rect).text(
                                egui::pos2(tag_clip_rect.min.x, tag_clip_rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                tag_text,
                                egui::FontId::proportional(9.0),
                                cx.theme.fg_dim,
                            );
                        }

                        let visible_right = ui.clip_rect().max.x;
                        let edit_btn_rect = egui::Rect::from_center_size(
                            egui::pos2(visible_right - 40.0, rect.min.y + 26.0),
                            egui::vec2(56.0, 22.0),
                        );
                        if hovered {
                            let pointer_pos = ui.ctx().input(|i| i.pointer.hover_pos());
                            let over_edit = pointer_pos.map_or(false, |p| edit_btn_rect.contains(p));

                            let edit_bg = if over_edit { cx.theme.accent } else { cx.theme.bg_elevated };
                            let edit_text = if over_edit { cx.theme.bg_primary } else { cx.theme.accent };
                            ui.painter().rect(edit_btn_rect, 4.0, edit_bg, egui::Stroke::new(1.0, cx.theme.accent));
                            ui.painter().text(
                                edit_btn_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                cx.language.t("edit_file"),
                                egui::FontId::proportional(11.0),
                                edit_text,
                            );
                        }
                        if resp.double_clicked() {
                            connect_ssh_host_idx = Some(i);
                        } else if resp.clicked() {
                            let click_pos = ui.ctx().input(|i| i.pointer.interact_pos());
                            if click_pos.map_or(false, |p| edit_btn_rect.contains(p)) {
                                edit_host_index = Some(i);
                            }
                        }
                    }

                    // Grouped SSH hosts
                    for group in &groups {
                        ui.add_space(SPACE_MD);
                        ui.horizontal(|ui| {
                            ui.add_space(SPACE_XL);
                            ui.label(widgets::section_header(group, cx.theme));
                        });
                        ui.add_space(SPACE_XS);
                        for (i, host) in cx.hosts.iter().enumerate() {
                            if host.is_local || host.group != *group { continue; }
                            if !window.host_filter.matches(host) { continue; }

                            let row_h = 58.0;
                            let width = ui.available_width();
                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(width, row_h),
                                egui::Sense::click(),
                            );
                            let hovered = resp.hovered();
                            if hovered {
                                ui.painter().rect_filled(
                                    egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.max.y - 1.0), rect.max),
                                    0.0, cx.theme.hover_shadow,
                                );
                                ui.painter().rect_filled(rect, 0.0, cx.theme.hover_bg);
                            }
                            ui.painter().text(
                                egui::pos2(rect.min.x + 24.0, rect.min.y + 18.0),
                                egui::Align2::LEFT_CENTER,
                                "@",
                                egui::FontId::proportional(12.0),
                                cx.theme.accent,
                            );
                            ui.painter().text(
                                egui::pos2(rect.min.x + 46.0, rect.min.y + 18.0),
                                egui::Align2::LEFT_CENTER,
                                &host.name,
                                egui::FontId::proportional(13.0),
                                cx.theme.fg_primary,
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
                                cx.theme.fg_dim,
                            );

                            if !host.tags.is_empty() {
                                let tag_text = format!("tag: {}", host.tags.join(", "));
                                let tag_clip_rect = egui::Rect::from_min_max(
                                    egui::pos2(rect.min.x + 46.0, rect.min.y + 44.0),
                                    egui::pos2(rect.max.x - 80.0, rect.min.y + row_h - 4.0),
                                );
                                ui.painter().with_clip_rect(tag_clip_rect).text(
                                    egui::pos2(tag_clip_rect.min.x, tag_clip_rect.center().y),
                                    egui::Align2::LEFT_CENTER,
                                    tag_text,
                                    egui::FontId::proportional(9.0),
                                    cx.theme.fg_dim,
                                );
                            }

                            let visible_right = ui.clip_rect().max.x;
                            let edit_btn_rect = egui::Rect::from_center_size(
                                egui::pos2(visible_right - 40.0, rect.min.y + 26.0),
                                egui::vec2(56.0, 22.0),
                            );
                            if hovered {
                                let pointer_pos = ui.ctx().input(|i| i.pointer.hover_pos());
                                let over_edit = pointer_pos.map_or(false, |p| edit_btn_rect.contains(p));

                                let edit_bg = if over_edit { cx.theme.accent } else { cx.theme.bg_elevated };
                                let edit_text = if over_edit { cx.theme.bg_primary } else { cx.theme.accent };
                                ui.painter().rect(edit_btn_rect, 4.0, edit_bg, egui::Stroke::new(1.0, cx.theme.accent));
                                ui.painter().text(
                                    edit_btn_rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    cx.language.t("edit_file"),
                                    egui::FontId::proportional(11.0),
                                    edit_text,
                                );
                            }
                            if resp.double_clicked() {
                                connect_ssh_host_idx = Some(i);
                            } else if resp.clicked() {
                                let click_pos = ui.ctx().input(|i| i.pointer.interact_pos());
                                if click_pos.map_or(false, |p| edit_btn_rect.contains(p)) {
                                    edit_host_index = Some(i);
                                }
                            }
                        }
                    }

                    ui.add_space(SPACE_XL);
                });
        });

    // Delete confirmation dialog
    if let Some(idx) = window.confirm_delete_host {
        let host_name = cx.hosts.get(idx).map(|h| h.name.clone()).unwrap_or_default();
        let mut open = true;
        egui::Window::new(cx.language.t("delete_host"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .fixed_size([340.0, 0.0])
            .title_bar(false)
            .frame(widgets::dialog_frame(cx.theme))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("\u{26A0}").size(18.0).color(cx.theme.red));
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(cx.language.t("delete_host")).size(15.0).color(cx.theme.fg_primary).strong());
                });
                ui.add_space(10.0);

                ui.label(
                    egui::RichText::new(cx.language.tf("delete_confirm", &host_name))
                        .color(cx.theme.fg_primary).size(13.0)
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(cx.language.t("confirm_delete"))
                        .color(cx.theme.fg_dim).size(11.0)
                );
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(widgets::danger_button(cx.language.t("delete"), cx.theme)).clicked() {
                            actions.delete_host = Some(idx);
                            window.confirm_delete_host = None;
                        }
                        if ui.add(widgets::secondary_button(cx.language.t("cancel"), cx.theme)).clicked() {
                            window.confirm_delete_host = None;
                        }
                    });
                });
            });
        if !open {
            window.confirm_delete_host = None;
        }
    }

    // Handle deferred actions
    if let Some(idx) = connect_ssh_host_idx {
        if idx < cx.hosts.len() {
            actions.connect_ssh = Some(cx.hosts[idx].clone());
        }
    }
    if let Some(idx) = edit_host_index {
        if idx < cx.hosts.len() {
            let host = cx.hosts[idx].clone();
            let current_time = ctx.input(|i| i.time);
            window.add_host_dialog.open_edit(idx, &host, current_time);
        }
    }
    if let Some(host) = connect_history_host {
        actions.connect_ssh = Some(host);
    }

    actions
}

/// Render the add/edit host drawer
pub fn render_add_host_drawer(window: &mut AppWindow, ctx: &egui::Context, cx: &mut WindowContext) {
    // Poll the async test result
    let polled_result: Option<Result<String, String>> = window
        .add_host_dialog
        .test_conn_result
        .as_ref()
        .and_then(|arc| arc.lock().ok()?.take());
    if let Some(result) = polled_result {
        window.add_host_dialog.test_conn_state = match result {
            Ok(msg) => {
                window.add_host_dialog.show_remove_key_button = false;
                TestConnState::Success(msg)
            }
            Err(msg) => {
                let is_key_error = msg.contains("Host key verification failed") ||
                                  msg.contains("MITM attack");
                window.add_host_dialog.show_remove_key_button = is_key_error;
                TestConnState::Failed(msg)
            }
        };
        window.add_host_dialog.test_conn_result = None;
    }

    let drawer_title = if window.add_host_dialog.edit_index.is_some() {
        cx.language.t("edit_host")
    } else {
        cx.language.t("new_host_title")
    };

    let mut save_clicked = false;
    let mut test_clicked = false;

    egui::SidePanel::right("add_host_drawer")
        .default_width(widgets::DRAWER_WIDTH)
        .frame(egui::Frame {
            fill: cx.theme.bg_secondary,
            inner_margin: egui::Margin::ZERO,
            rounding: egui::Rounding::ZERO,
            shadow: egui::epaint::Shadow {
                offset: egui::vec2(-4.0, 0.0),
                blur: 20.0,
                spread: 0.0,
                color: egui::Color32::from_black_alpha(20),
            },
            stroke: egui::Stroke::NONE,
            ..Default::default()
        })
        .show(ctx, |ui| {
            // Header
            egui::Frame::none()
                .inner_margin(egui::Margin::symmetric(widgets::FORM_LEFT_MARGIN, 16.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(drawer_title)
                            .size(widgets::FONT_SIZE_TITLE).strong().color(cx.theme.fg_primary));

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            // Close button
                            if ui.add(
                                egui::Button::new(egui::RichText::new("×").size(20.0).color(cx.theme.fg_dim))
                                    .frame(false)
                            ).clicked() {
                                window.add_host_dialog.reset();
                            }
                            // Delete button (edit mode only)
                            if window.add_host_dialog.edit_index.is_some() {
                                if ui.add(
                                    egui::Button::new(egui::RichText::new("\u{1F5D1}").size(FONT_BASE))
                                        .frame(false)
                                ).on_hover_text(cx.language.t("delete"))
                                .clicked() {
                                    window.confirm_delete_host = window.add_host_dialog.edit_index;
                                    window.add_host_dialog.open = false;
                                }
                            }
                        });
                    });
                });

            widgets::form_separator(ui);

            // Content with left margin
            egui::ScrollArea::vertical()
                .id_salt("add_host_drawer_scroll")
                .show(ui, |ui| {
                    egui::Frame::none()
                        .inner_margin(egui::Margin::symmetric(widgets::FORM_LEFT_MARGIN, 0.0))
                        .show(ui, |ui| {
                            // Name field - full width
                            widgets::form_field(ui, cx.language.t("label"), true,
                                &mut window.add_host_dialog.name,
                                cx.language.t("host_label_hint"), cx.theme);
                            ui.add_space(widgets::SPACING_FIELD);

                            // Host + Port in same row
                            widgets::form_field_2col(
                                ui,
                                cx.language.t("host_ip"), true,
                                &mut window.add_host_dialog.host,
                                cx.language.t("host_ip_hint"), 170.0,
                                cx.language.t("port"), true,
                                &mut window.add_host_dialog.port,
                                cx.language.t("port_hint"), 80.0,
                                cx.theme
                            );
                            ui.add_space(widgets::SPACING_FIELD);

                            // Username + Group in same row
                            widgets::form_field_2col(
                                ui,
                                cx.language.t("username"), true,
                                &mut window.add_host_dialog.username,
                                cx.language.t("username_hint"), 125.0,
                                cx.language.t("group"), false,
                                &mut window.add_host_dialog.group,
                                cx.language.t("group_hint"), 125.0,
                                cx.theme
                            );
                            ui.add_space(widgets::SPACING_FIELD);

                            // Tags - full width
                            widgets::form_field(ui, cx.language.t("tag"), false,
                                &mut window.add_host_dialog.tags,
                                cx.language.t("tag_hint"), cx.theme);

                            widgets::form_separator(ui);

                            // Authentication section
                            ui.label(egui::RichText::new(cx.language.t("authentication"))
                                .color(cx.theme.fg_primary)
                                .size(widgets::FONT_SIZE_TITLE)
                                .strong());
                            ui.add_space(widgets::SPACING_FIELD);

                            // Credential mode tabs
                            let has_credentials = !cx.credentials.is_empty();
                            ui.horizontal(|ui| {
                                let modes: [(CredentialMode, &str); 3] = [
                                    (CredentialMode::None, cx.language.t("credential_none")),
                                    (CredentialMode::Existing, cx.language.t("credential_existing")),
                                    (CredentialMode::Inline, cx.language.t("credential_inline")),
                                ];
                                for (mode, text) in &modes {
                                    if *mode == CredentialMode::Existing && !has_credentials {
                                        continue;
                                    }
                                    let is_selected = window.add_host_dialog.credential_mode == *mode;
                                    let text_color = if is_selected { cx.theme.accent } else { cx.theme.fg_dim };
                                    let bg_color = if is_selected { cx.theme.accent_alpha(15) } else { egui::Color32::TRANSPARENT };
                                    let btn = egui::Button::new(
                                        egui::RichText::new(*text).size(12.0).color(text_color)
                                    )
                                    .fill(bg_color)
                                    .rounding(4.0)
                                    .stroke(egui::Stroke::new(1.0, if is_selected { cx.theme.accent } else { egui::Color32::TRANSPARENT }));
                                    if ui.add(btn).clicked() {
                                        window.add_host_dialog.credential_mode = *mode;
                                    }
                                    ui.add_space(4.0);
                                }
                            });

                            ui.add_space(widgets::SPACING_FIELD);

                            // Existing credential selector
                            if window.add_host_dialog.credential_mode == CredentialMode::Existing {
                                ui.vertical(|ui| {
                                    widgets::form_label(ui, cx.language.t("select_credential"), true, cx.theme);
                                    ui.add_space(widgets::SPACING_LABEL);
                                    let selected_id = window.add_host_dialog.selected_credential_id.as_ref();
                                    let selected_text = selected_id
                                        .and_then(|id| cx.credentials.iter().find(|c| &c.id == id))
                                        .map(|c| c.name.clone())
                                        .unwrap_or_else(|| cx.language.t("select_credential").to_string());
                                    egui::ComboBox::from_id_salt("existing_credential")
                                        .selected_text(egui::RichText::new(selected_text).size(widgets::FONT_SIZE_INPUT).color(cx.theme.fg_primary))
                                        .width(ui.available_width())
                                        .show_ui(ui, |ui| {
                                            widgets::style_dropdown(ui, cx.theme);
                                            for cred in cx.credentials.iter() {
                                                let is_selected = window.add_host_dialog.selected_credential_id.as_ref() == Some(&cred.id);
                                                let type_label = match &cred.credential_type {
                                                    crate::config::CredentialType::Password { .. } => cx.language.t("credential_password"),
                                                    crate::config::CredentialType::SshKey { .. } => cx.language.t("credential_private_key"),
                                                };
                                                let label = format!("{} ({})", cred.name, type_label);
                                                if ui.selectable_label(is_selected, &label).clicked() {
                                                    window.add_host_dialog.selected_credential_id = Some(cred.id.clone());
                                                }
                                            }
                                        });
                                });
                            }

                            // Inline credential fields
                            if window.add_host_dialog.credential_mode == CredentialMode::Inline {
                                ui.horizontal(|ui| {
                                    ui.selectable_value(
                                        &mut window.add_host_dialog.auth_method,
                                        AuthMethodChoice::Password,
                                        egui::RichText::new(cx.language.t("password")).size(12.0),
                                    );
                                    ui.selectable_value(
                                        &mut window.add_host_dialog.auth_method,
                                        AuthMethodChoice::Key,
                                        egui::RichText::new(cx.language.t("ssh_key")).size(12.0),
                                    );
                                });
                                ui.add_space(widgets::SPACING_FIELD);

                                match window.add_host_dialog.auth_method {
                                    AuthMethodChoice::Password => {
                                        widgets::form_field_password(ui, cx.language.t("password"), true,
                                            &mut window.add_host_dialog.password,
                                            cx.language.t("password_hint"), cx.theme);
                                    }
                                    AuthMethodChoice::Key => {
                                        // Key source selector
                                        ui.horizontal(|ui| {
                                            let local_color = if window.add_host_dialog.key_source == KeySourceChoice::LocalFile { cx.theme.accent } else { cx.theme.fg_dim };
                                            if ui.add(
                                                egui::Button::new(egui::RichText::new(cx.language.t("key_source_path")).color(local_color).size(12.0))
                                                    .stroke(egui::Stroke::NONE).fill(egui::Color32::TRANSPARENT)
                                            ).clicked() {
                                                window.add_host_dialog.key_source = KeySourceChoice::LocalFile;
                                            }
                                            ui.add_space(SPACE_SM);
                                            let import_color = if window.add_host_dialog.key_source == KeySourceChoice::ImportContent { cx.theme.accent } else { cx.theme.fg_dim };
                                            if ui.add(
                                                egui::Button::new(egui::RichText::new(cx.language.t("import_key")).color(import_color).size(12.0))
                                                    .stroke(egui::Stroke::NONE).fill(egui::Color32::TRANSPARENT)
                                            ).clicked() {
                                                window.add_host_dialog.key_source = KeySourceChoice::ImportContent;
                                            }
                                        });
                                        ui.add_space(widgets::SPACING_FIELD);

                                        if window.add_host_dialog.key_source == KeySourceChoice::LocalFile {
                                            widgets::form_field(ui, cx.language.t("key_path"), true,
                                                &mut window.add_host_dialog.key_path,
                                                cx.language.t("key_path_hint"), cx.theme);
                                        } else {
                                            widgets::form_field_textarea(ui, cx.language.t("key_content"), true,
                                                &mut window.add_host_dialog.key_content,
                                                cx.language.t("key_content_hint"), 80.0, cx.theme);
                                        }

                                        if window.add_host_dialog.key_in_keychain {
                                            ui.label(egui::RichText::new(cx.language.t("key_stored_in_keychain")).color(cx.theme.green).size(11.0));
                                        }
                                        ui.add_space(widgets::SPACING_FIELD);
                                        widgets::form_field_password(ui, cx.language.t("key_passphrase"), false,
                                            &mut window.add_host_dialog.key_passphrase,
                                            cx.language.t("key_passphrase_hint"), cx.theme);
                                    }
                                }
                            }

                            // Error message
                            if !window.add_host_dialog.error.is_empty() {
                                ui.add_space(widgets::SPACING_FIELD);
                                ui.label(egui::RichText::new(&window.add_host_dialog.error)
                                    .color(cx.theme.red).size(widgets::FONT_SIZE_INPUT));
                            }

                            // Test connection state
                            match &window.add_host_dialog.test_conn_state {
                                TestConnState::Idle => {}
                                TestConnState::Testing => {
                                    ui.add_space(widgets::SPACING_FIELD);
                                    ui.horizontal(|ui| {
                                        ui.spinner();
                                        ui.label(egui::RichText::new(cx.language.t("testing")).color(cx.theme.fg_dim).size(12.0));
                                    });
                                }
                                TestConnState::Success(msg) => {
                                    ui.add_space(widgets::SPACING_FIELD);
                                    ui.label(egui::RichText::new(format!("✓ {}", msg)).color(cx.theme.green).size(12.0));
                                }
                                TestConnState::Failed(msg) => {
                                    ui.add_space(widgets::SPACING_FIELD);
                                    ui.label(egui::RichText::new(format!("✗ {}", msg)).color(cx.theme.red).size(12.0));
                                }
                            }

                        });
                });

            // Footer (fixed at bottom)
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.add_space(widgets::FORM_LEFT_MARGIN);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let can_save = !window.add_host_dialog.name.trim().is_empty()
                        && !window.add_host_dialog.host.trim().is_empty()
                        && !window.add_host_dialog.port.trim().is_empty()
                        && !window.add_host_dialog.username.trim().is_empty();

                    let is_testing = matches!(window.add_host_dialog.test_conn_state, TestConnState::Testing);
                    if ui.add_enabled(!is_testing, widgets::primary_button(cx.language.t("test"), cx.theme)).clicked() {
                        test_clicked = true;
                    }
                    ui.add_space(8.0);
                    if ui.add(widgets::primary_button(cx.language.t("save"), cx.theme)).clicked() && can_save {
                        save_clicked = true;
                    }
                    ui.add_space(8.0);
                    if ui.add(widgets::secondary_button(cx.language.t("cancel"), cx.theme)).clicked() {
                        window.add_host_dialog.reset();
                    }
                });
            });
        });

    // Handle test connection
    if test_clicked {
        let host = window.add_host_dialog.host.trim().to_owned();
        let port: u16 = window.add_host_dialog.port.trim().parse().unwrap_or(22);
        let username = window.add_host_dialog.username.trim().to_owned();

        if host.is_empty() {
            window.add_host_dialog.error = cx.language.t("host_required").to_string();
        } else {
            let resolved = match window.add_host_dialog.credential_mode {
                CredentialMode::None => crate::config::ResolvedAuth::None,
                CredentialMode::Existing => {
                    if let Some(ref cid) = window.add_host_dialog.selected_credential_id {
                        if let Some(cred) = cx.credentials.iter().find(|c| c.id == *cid) {
                            crate::config::resolve_credential(cred)
                        } else {
                            crate::config::ResolvedAuth::None
                        }
                    } else {
                        crate::config::ResolvedAuth::None
                    }
                }
                CredentialMode::Inline => {
                    match window.add_host_dialog.auth_method {
                        AuthMethodChoice::Password => {
                            let pw = window.add_host_dialog.password.clone();
                            if pw.is_empty() {
                                crate::config::ResolvedAuth::None
                            } else {
                                crate::config::ResolvedAuth::Password { password: pw }
                            }
                        }
                        AuthMethodChoice::Key => {
                            let key_content = match window.add_host_dialog.key_source {
                                KeySourceChoice::LocalFile => {
                                    let key_path = window.add_host_dialog.key_path.trim().to_owned();
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
                                    window.add_host_dialog.key_content.trim().to_owned()
                                }
                            };
                            let passphrase = if window.add_host_dialog.key_passphrase.is_empty() {
                                None
                            } else {
                                Some(window.add_host_dialog.key_passphrase.clone())
                            };
                            crate::config::ResolvedAuth::Key { key_content, passphrase }
                        }
                    }
                }
            };

            let result_arc: Arc<Mutex<Option<Result<String, String>>>> =
                Arc::new(Mutex::new(None));
            window.add_host_dialog.test_conn_result = Some(Arc::clone(&result_arc));
            window.add_host_dialog.test_conn_state = TestConnState::Testing;
            window.add_host_dialog.error.clear();

            let agent_fwd = window.add_host_dialog.agent_forwarding;
            let rt = cx.runtime;
            rt.spawn(async move {
                let settings = crate::config::load_settings();
                let result = test_connection(host, port, username, resolved, settings.ssh_keepalive_interval, agent_fwd).await;
                if let Ok(mut guard) = result_arc.lock() {
                    *guard = Some(result);
                }
            });
        }
    }

    // Handle save
    if save_clicked {
        let name = window.add_host_dialog.name.trim().to_owned();
        let host = window.add_host_dialog.host.trim().to_owned();
        let port: u16 = window.add_host_dialog.port.trim().parse().unwrap_or(22);

        if name.is_empty() || host.is_empty() {
            window.add_host_dialog.error = cx.language.t("label_required").to_string();
            return;
        }

        let credential_id = match window.add_host_dialog.credential_mode {
            CredentialMode::None => None,
            CredentialMode::Existing => window.add_host_dialog.selected_credential_id.clone(),
            CredentialMode::Inline => {
                let cred = match window.add_host_dialog.auth_method {
                    AuthMethodChoice::Password => {
                        let pw = window.add_host_dialog.password.clone();
                        if pw.is_empty() {
                            None
                        } else {
                            let cred = crate::config::Credential::new_password(
                                format!("{} (password)", name),
                                window.add_host_dialog.username.trim().to_owned(),
                            );
                            crate::config::store_credential_secret(&cred.id, &cred.name, "password", &pw);
                            Some(cred)
                        }
                    }
                    AuthMethodChoice::Key => {
                        let key_content = match window.add_host_dialog.key_source {
                            KeySourceChoice::LocalFile => {
                                let key_path = window.add_host_dialog.key_path.trim();
                                if key_path.is_empty() {
                                    window.add_host_dialog.error = cx.language.t("key_required").to_string();
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
                                let kc = window.add_host_dialog.key_content.trim().to_owned();
                                if kc.is_empty() {
                                    window.add_host_dialog.error = "Private key content is required.".to_string();
                                    return;
                                }
                                kc
                            }
                        };
                        let has_passphrase = !window.add_host_dialog.key_passphrase.is_empty();
                        let mut cred = crate::config::Credential::new_ssh_key(
                            format!("{} (key)", name),
                            window.add_host_dialog.key_path.trim().to_owned(),
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
                            crate::config::store_credential_secret(&cred.id, &cred.name, "passphrase", &window.add_host_dialog.key_passphrase);
                        }
                        Some(cred)
                    }
                };

                if let Some(cred) = cred {
                    let id = cred.id.clone();
                    cx.credentials.push(cred);
                    // Note: save_credentials will be handled by the caller
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
            window.add_host_dialog.username.trim().to_owned(),
            window.add_host_dialog.group.trim().to_owned(),
            credential_id,
            window.add_host_dialog.startup_commands
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect(),
        );

        entry.agent_forwarding = window.add_host_dialog.agent_forwarding;
        entry.jump_host = window.add_host_dialog.jump_host.clone();
        entry.port_forwards = window.add_host_dialog.port_forwards.clone();

        let tags: Vec<String> = window.add_host_dialog.tags
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        entry.tags = tags;

        if let Some(idx) = window.add_host_dialog.edit_index {
            if idx < cx.hosts.len() {
                cx.hosts[idx] = entry;
            }
        } else {
            cx.hosts.push(entry);
        }

        // Note: save_hosts will be handled by the caller
        window.add_host_dialog.reset();
    }
}
