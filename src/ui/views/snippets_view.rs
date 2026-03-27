//! # Snippets View
//!
//! Rendering for the command snippets management page.

use eframe::egui;

// These types are defined in pane_view.rs
use crate::ui::pane_view::{WindowContext, ViewActions};
use crate::ui::pane::AppWindow;
use crate::config::Snippet;
use crate::ui::tokens::*;
use crate::ui::widgets;

/// Render snippets view for this window
pub fn render_snippets_view(
    window: &mut AppWindow,
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    cx: &mut WindowContext,
) -> ViewActions {
    let mut snippet_to_delete: Option<String> = None;

    // Top navigation bar
    egui::TopBottomPanel::top("snippets_nav_bar")
        .frame(egui::Frame {
            fill: cx.theme.bg_secondary,
            inner_margin: egui::Margin::symmetric(8.0, 8.0),
            stroke: egui::Stroke::NONE,
            ..Default::default()
        })
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(cx.language.t("snippets"))
                    .color(cx.theme.fg_dim)
                    .size(FONT_BASE)
                    .strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(widgets::text_button(
                        cx.language.t("new_snippet"),
                        cx.theme.accent
                    )).clicked() {
                        window.snippet_view_state.open_new(cx.language.t("snippet_default_group"));
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
                .id_salt("snippets_page_scroll")
                .show(ui, |ui| {
                    ui.add_space(SPACE_MD);

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

                        ui.add_space(ui.available_width() - 110.0);

                        // Collect groups
                        let mut all_groups: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
                        for snippet in cx.snippets.iter() {
                            if !snippet.group.is_empty() {
                                all_groups.insert(snippet.group.clone());
                            }
                        }
                        let all_groups: Vec<String> = all_groups.into_iter().collect();

                        let group_label = if window.snippet_view_state.group_filter.is_empty() {
                            cx.language.t("snippet_group").to_string()
                        } else {
                            window.snippet_view_state.group_filter.clone()
                        };

                        egui::ComboBox::from_id_salt("snippet_group_filter")
                            .selected_text(egui::RichText::new(group_label).color(cx.theme.accent).size(FONT_MD))
                            .width(90.0)
                            .show_ui(ui, |ui| {
                                widgets::style_dropdown(ui, cx.theme);
                                if ui.add(egui::Button::new(
                                    egui::RichText::new(cx.language.t("snippet_default_group"))
                                        .color(if window.snippet_view_state.group_filter.is_empty() { cx.theme.accent } else { cx.theme.fg_primary })
                                        .size(FONT_MD)
                                ).frame(false)).clicked() {
                                    window.snippet_view_state.group_filter.clear();
                                    ui.close_menu();
                                }
                                for group in &all_groups {
                                    if ui.add(egui::Button::new(
                                        egui::RichText::new(group)
                                            .color(if window.snippet_view_state.group_filter == *group { cx.theme.accent } else { cx.theme.fg_primary })
                                            .size(FONT_MD)
                                    ).frame(false)).clicked() {
                                        window.snippet_view_state.group_filter = group.clone();
                                        ui.close_menu();
                                    }
                                }
                            });

                        ui.add_space(SPACE_SM);

                        if !window.snippet_view_state.group_filter.is_empty() {
                            if ui.add(widgets::text_button(cx.language.t("clear_history"), cx.theme.accent)).clicked() {
                                window.snippet_view_state.group_filter.clear();
                            }
                        }
                    });

                    ui.add_space(SPACE_MD);

                    // Filter snippets
                    let filtered_snippets: Vec<Snippet> = cx.snippets
                        .iter()
                        .filter(|s| {
                            if window.snippet_view_state.group_filter.is_empty() {
                                return true;
                            }
                            s.group == window.snippet_view_state.group_filter
                        })
                        .cloned()
                        .collect();

                    if filtered_snippets.is_empty() {
                        ui.add_space(SPACE_2XL);
                        ui.vertical_centered(|ui| {
                            ui.label(egui::RichText::new("\u{26A1}").size(SPACE_2XL).color(cx.theme.fg_dim));
                            ui.add_space(SPACE_MD);
                            ui.label(egui::RichText::new(cx.language.t("no_snippets")).color(cx.theme.fg_dim).size(FONT_BASE));
                        });
                    } else {
                        // Group snippets
                        let mut groups: std::collections::BTreeMap<String, Vec<Snippet>> = std::collections::BTreeMap::new();
                        for snippet in filtered_snippets {
                            groups.entry(snippet.group.clone()).or_default().push(snippet);
                        }

                        for (group_name, snippets_in_group) in &groups {
                            ui.horizontal(|ui| {
                                ui.add_space(24.0);
                                ui.label(egui::RichText::new(group_name.to_uppercase())
                                    .color(cx.theme.fg_dim)
                                    .size(10.0)
                                    .strong());
                            });
                            ui.add_space(4.0);

                            for snippet in snippets_in_group {
                                let row_h = 58.0;
                                let width = ui.available_width();
                                let (rect, resp): (egui::Rect, egui::Response) = ui.allocate_exact_size(
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
                                    "\u{26A1}",
                                    egui::FontId::proportional(12.0),
                                    cx.theme.accent,
                                );
                                ui.painter().text(
                                    egui::pos2(rect.min.x + 46.0, rect.min.y + 18.0),
                                    egui::Align2::LEFT_CENTER,
                                    &snippet.name,
                                    egui::FontId::proportional(13.0),
                                    cx.theme.fg_primary,
                                );

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
                                    cx.theme.fg_dim,
                                );

                                let visible_right = ui.clip_rect().max.x;
                                let pointer_pos = ui.ctx().input(|i| i.pointer.hover_pos());

                                if hovered {
                                    let btn_rect = egui::Rect::from_center_size(
                                        egui::pos2(visible_right - 40.0, rect.min.y + 26.0),
                                        egui::vec2(56.0, 22.0),
                                    );
                                    let over_btn = pointer_pos.map_or(false, |p| btn_rect.contains(p));
                                    let btn_bg = if over_btn { cx.theme.accent } else { cx.theme.bg_elevated };
                                    let btn_text_color = if over_btn { cx.theme.bg_primary } else { cx.theme.accent };
                                    ui.painter().rect(btn_rect, 4.0, btn_bg, egui::Stroke::new(1.0, cx.theme.accent));
                                    ui.painter().text(
                                        btn_rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        cx.language.t("edit_file"),
                                        egui::FontId::proportional(11.0),
                                        btn_text_color,
                                    );

                                    if over_btn && resp.clicked() {
                                        window.snippet_view_state.open_edit(
                                            snippet.id.clone(),
                                            &snippet.name,
                                            &snippet.command,
                                            &snippet.group
                                        );
                                    }
                                    ui.allocate_exact_size(egui::vec2(56.0, 22.0), egui::Sense::hover());
                                }
                            }
                            ui.add_space(12.0);
                        }
                    }
                    ui.add_space(40.0);
                });
        });

    // Delete confirmation dialog
    if let Some(delete_id) = &window.snippet_view_state.confirm_delete.clone() {
        let snippet_name = cx.snippets.iter()
            .find(|s| s.id == *delete_id)
            .map(|s| s.name.clone())
            .unwrap_or_default();
        let mut open = true;
        egui::Window::new(cx.language.t("delete"))
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
                    ui.add_space(SPACE_XS);
                    ui.label(egui::RichText::new(cx.language.t("delete")).size(15.0).color(cx.theme.fg_primary).strong());
                });
                ui.add_space(10.0);
                ui.label(egui::RichText::new(cx.language.tf("delete_confirm", &snippet_name)).color(cx.theme.fg_primary).size(FONT_BASE));
                ui.add_space(SPACE_XS);
                ui.label(egui::RichText::new(cx.language.t("confirm_delete")).color(cx.theme.fg_dim).size(FONT_SM));
                ui.add_space(SPACE_LG);

                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(widgets::danger_button(cx.language.t("delete"), cx.theme)).clicked() {
                            snippet_to_delete = Some(delete_id.clone());
                        }
                        if ui.add(widgets::secondary_button(cx.language.t("cancel"), cx.theme)).clicked() {
                            window.snippet_view_state.confirm_delete = None;
                        }
                    });
                });
            });

        if !open {
            window.snippet_view_state.confirm_delete = None;
        }
    }

    // Apply delete action
    if let Some(id) = snippet_to_delete {
        cx.snippets.retain(|s| s.id != id);
        window.snippet_view_state.confirm_delete = None;
        let mut actions = ViewActions::default();
        actions.save_hosts = true;
        return actions;
    }

    ViewActions::default()
}

/// Render the add/edit snippet drawer (shadcn/ui style)
pub fn render_snippet_drawer(window: &mut AppWindow, ctx: &egui::Context, cx: &mut WindowContext) {
    use uuid::Uuid;

    if !window.snippet_view_state.open {
        return;
    }

    let is_editing = window.snippet_view_state.editing.is_some();

    egui::SidePanel::right("snippet_drawer")
        .default_width(400.0)
        .frame(egui::Frame {
            fill: cx.theme.bg_elevated,
            inner_margin: egui::Margin::ZERO,
            ..Default::default()
        })
        .show(ctx, |ui| {
            // Header
            egui::TopBottomPanel::top("snippet_drawer_header")
                .exact_height(56.0)
                .frame(egui::Frame {
                    fill: cx.theme.bg_elevated,
                    inner_margin: egui::Margin { left: 24.0, right: 16.0, top: 16.0, bottom: 16.0 },
                    ..Default::default()
                })
                .show_inside(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(
                            if is_editing { cx.language.t("edit_snippet") } else { cx.language.t("new_snippet") }
                        ).size(16.0).strong().color(cx.theme.fg_primary));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(
                                egui::Button::new(egui::RichText::new("×").size(20.0).color(cx.theme.fg_dim))
                                    .frame(false)
                                    .rounding(4.0)
                                    .min_size(egui::vec2(32.0, 32.0))
                            ).clicked() {
                                window.snippet_view_state.open = false;
                                window.snippet_view_state.editing = None;
                            }
                            if is_editing {
                                if ui.add(
                                    egui::Button::new(egui::RichText::new("\u{1F5D1}").size(FONT_BASE))
                                        .frame(false)
                                        .rounding(4.0)
                                        .min_size(egui::vec2(28.0, 28.0))
                                ).on_hover_text(cx.language.t("delete"))
                                .clicked() {
                                    window.snippet_view_state.confirm_delete = window.snippet_view_state.editing.clone();
                                    window.snippet_view_state.open = false;
                                }
                            }
                        });
                    });
                });

            // Divider
            ui.add_space(16.0);
            ui.add(egui::Separator::default().spacing(0.0));
            ui.add_space(24.0);

            // Content
            egui::ScrollArea::vertical()
                .id_salt("snippet_drawer_scroll")
                .show(ui, |ui| {
                    egui::Frame::none()
                        .inner_margin(egui::Margin::symmetric(widgets::FORM_LEFT_MARGIN, 0.0))
                        .show(ui, |ui| {
                            // Name + Group in same row
                            widgets::form_field_2col(
                                ui,
                                cx.language.t("name"), true,
                                &mut window.snippet_view_state.new_name,
                                cx.language.t("snippet_name_hint"), 170.0,
                                cx.language.t("group"), false,
                                &mut window.snippet_view_state.new_group,
                                cx.language.t("snippet_default_group"), 120.0,
                                cx.theme
                            );
                            ui.add_space(widgets::SPACING_FIELD);

                            // Command (required) - multiline
                            widgets::form_field_textarea(ui, cx.language.t("command"), true,
                                &mut window.snippet_view_state.new_command,
                                cx.language.t("command_hint"), 80.0, cx.theme);

                            ui.add_space(widgets::SPACING_SECTION);

                    // Footer buttons
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let can_save = !window.snippet_view_state.new_name.trim().is_empty();

                        if ui.add(widgets::primary_button(cx.language.t("save"), cx.theme)).clicked() && can_save {
                            let group = if window.snippet_view_state.new_group.trim().is_empty() {
                                cx.language.t("snippet_default_group").to_string()
                            } else {
                                window.snippet_view_state.new_group.trim().to_string()
                            };

                            if let Some(edit_id) = &window.snippet_view_state.editing {
                                if let Some(snippet) = cx.snippets.iter_mut().find(|s| s.id == *edit_id) {
                                    snippet.name = window.snippet_view_state.new_name.trim().to_string();
                                    snippet.command = window.snippet_view_state.new_command.clone();
                                    snippet.group = group;
                                }
                            } else {
                                let snippet = Snippet {
                                    id: Uuid::new_v4().to_string(),
                                    name: window.snippet_view_state.new_name.trim().to_string(),
                                    command: window.snippet_view_state.new_command.clone(),
                                    group,
                                };
                                cx.snippets.push(snippet);
                            }
                            window.snippet_view_state.open = false;
                            window.snippet_view_state.editing = None;
                        }
                        ui.add_space(8.0);
                        if ui.add(widgets::secondary_button(cx.language.t("cancel"), cx.theme)).clicked() {
                            window.snippet_view_state.open = false;
                            window.snippet_view_state.editing = None;
                        }
                    });
                });
            ui.add_space(24.0);
        });
    });
}
