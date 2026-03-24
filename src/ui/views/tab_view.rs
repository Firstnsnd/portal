//! # Tab Bar View Component
//!
//! This module contains the tab bar rendering logic for the terminal view.

use eframe::egui;
use crate::ui::pane::{Tab, TabDragState};
use crate::ui::theme::ThemeColors;
use crate::ui::i18n::Language;
use crate::ui::types::session::SessionBackend;
use crate::ssh::SshConnectionState;

/// Tab bar action result
#[derive(PartialEq)]
pub enum TabBarAction {
    ActivateTab(usize),
    CloseTab(usize),
    ReconnectTab(usize),
    DetachTab(usize),
    MergeTabs { src: usize, dst: usize },
    ReorderTab { src: usize, dst: usize, insert_before: bool },
    NewTab,
    ToggleBroadcast(usize),
    None,
}

/// Render the main tab bar
pub fn tab_bar(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    tabs: &[Tab],
    active_tab: usize,
    tab_drag: &mut TabDragState,
    theme: &ThemeColors,
    language: &Language,
    show_more_menu: &mut bool,
) -> TabBarAction {
    let buttons_width = 60.0;
    let available_width = ui.available_width();

    ui.horizontal(|ui| {
        ui.add_space(4.0);
        let full_tab_bar_rect = ui.max_rect();
        let tab_area_width = (available_width - buttons_width).max(100.0);

        let scroll_response = egui::ScrollArea::horizontal()
            .id_salt("tab_scroll")
            .auto_shrink([false, false])
            .max_width(tab_area_width)
            .scroll_bar_visibility(egui::containers::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                render_tabs_inner(ui, ctx, tabs, active_tab, tab_drag, theme, language, full_tab_bar_rect, false)
            });

        let scroll_result = scroll_response.inner;
        if scroll_result != TabBarAction::None {
            return scroll_result;
        }

        render_more_menu(ui, ctx, tabs, active_tab, theme, language, show_more_menu, egui::Id::new("tab_bar_more_menu"))
    }).inner
}

/// Render detached window tab bar
pub fn detached_tab_bar(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    tabs: &[Tab],
    active_tab: usize,
    tab_drag: &mut TabDragState,
    theme: &ThemeColors,
    language: &Language,
    show_more_menu: &mut bool,
    window_index: usize,
) -> TabBarAction {
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        let full_tab_bar_rect = ui.max_rect();

        let scroll_response = egui::ScrollArea::horizontal()
            .id_salt(egui::Id::new("detached_tab_scroll").with(window_index))
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::containers::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                render_tabs_inner(ui, ctx, tabs, active_tab, tab_drag, theme, language, full_tab_bar_rect, true)
            });

        let scroll_result = scroll_response.inner;
        if scroll_result != TabBarAction::None {
            return scroll_result;
        }

        render_more_menu(ui, ctx, tabs, active_tab, theme, language, show_more_menu, egui::Id::new("dw_tab_bar_more_menu").with(window_index))
    }).inner
}

/// Inner function to render tabs (shared between main and detached)
fn render_tabs_inner(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    tabs: &[Tab],
    active_tab: usize,
    tab_drag: &mut TabDragState,
    theme: &ThemeColors,
    language: &Language,
    full_tab_bar_rect: egui::Rect,
    is_detached: bool,
) -> TabBarAction {
    let mut result = TabBarAction::None;

    ui.horizontal(|ui| {
        ui.add_space(4.0);

        let mut tab_to_activate: Option<usize> = None;
        let mut tab_to_close: Option<usize> = None;
        let mut tab_to_reconnect: Option<usize> = None;
        let mut tab_rects: Vec<egui::Rect> = Vec::with_capacity(tabs.len());

        tab_drag.ensure_size(tabs.len());
        let current_time = ctx.input(|i| i.time);
        let tab_width = tab_drag.ghost_size.x.max(100.0);

        // Calculate target offsets for drag animation
        if let (Some(source), Some(target)) = (tab_drag.source_index, tab_drag.target_index) {
            if !tab_drag.is_merge && source != target {
                for ti in 0..tabs.len() {
                    if ti == source {
                        tab_drag.set_target_offset(ti, 0.0, current_time);
                    } else if tab_drag.insert_before {
                        if ti >= target && ti < source {
                            tab_drag.set_target_offset(ti, tab_width + 8.0, current_time);
                        } else if ti > source && ti < target {
                            tab_drag.set_target_offset(ti, -(tab_width + 8.0), current_time);
                        } else {
                            tab_drag.set_target_offset(ti, 0.0, current_time);
                        }
                    } else {
                        if ti > target && ti < source {
                            tab_drag.set_target_offset(ti, tab_width + 8.0, current_time);
                        } else if ti > source && ti <= target {
                            tab_drag.set_target_offset(ti, -(tab_width + 8.0), current_time);
                        } else {
                            tab_drag.set_target_offset(ti, 0.0, current_time);
                        }
                    }
                }
            }
        } else {
            for ti in 0..tabs.len() {
                tab_drag.set_target_offset(ti, 0.0, current_time);
            }
        }

        // Render each tab
        for (i, tab) in tabs.iter().enumerate() {
            let is_active = i == active_tab;
            let is_drag_target = tab_drag.source_index.is_some() && tab_drag.target_index == Some(i);
            let is_broadcasting = tab.broadcast_enabled;

            let offset = tab_drag.get_offset(i, current_time);
            ui.add_space(offset);

            let tab_fill = if is_active {
                theme.bg_elevated
            } else if is_broadcasting {
                egui::Color32::from_rgba_unmultiplied(60, 40, 100, 255)
            } else {
                egui::Color32::TRANSPARENT
            };

            let mut close_btn_rect: Option<egui::Rect> = None;
            let tab_resp = egui::Frame {
                fill: tab_fill,
                rounding: egui::Rounding::same(8.0),
                inner_margin: egui::Margin::symmetric(12.0, 4.0),
                ..Default::default()
            }
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;

                    // Status dot
                    let dot_color = tab.sessions
                        .get(tab.focused_session)
                        .map(|s| match &s.session {
                            Some(sb) if sb.is_connected() => theme.green,
                            Some(SessionBackend::Ssh(ssh)) => match ssh.connection_state() {
                                SshConnectionState::Connecting | SshConnectionState::Authenticating => theme.accent,
                                _ => theme.red,
                            },
                            _ => theme.red,
                        })
                        .unwrap_or(theme.fg_dim);
                    ui.label(egui::RichText::new("●").color(dot_color).size(8.0));

                    // Broadcast indicator
                    if is_broadcasting {
                        ui.label(egui::RichText::new("◉").color(theme.accent).size(11.0));
                    }

                    // Tab title
                    let title_color = if is_active { theme.fg_primary } else { theme.fg_dim };
                    let display_title = tab.sessions
                        .get(tab.focused_session)
                        .and_then(|s| s.cwd.as_ref())
                        .and_then(|cwd| {
                            std::path::Path::new(cwd)
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                        })
                        .unwrap_or_else(|| tab.title.clone());
                    ui.label(egui::RichText::new(display_title).color(title_color).size(13.0));

                    // Close button
                    if tabs.len() > 1 {
                        let close_resp = ui.add(
                            egui::Button::new(
                                egui::RichText::new("×").color(theme.fg_dim).size(14.0)
                            ).frame(false)
                        );
                        close_btn_rect = Some(close_resp.rect);
                    }
                });
            });

            let tab_rect = tab_resp.response.rect;
            tab_rects.push(tab_rect);

            // Draw merge indicator
            if is_drag_target {
                ui.painter().rect_stroke(
                    tab_rect,
                    8.0,
                    egui::Stroke::new(2.0, theme.accent),
                );
            }

            // Handle interaction
            let drag_id = if is_detached {
                egui::Id::new(("detached_tab_drag", i))
            } else {
                egui::Id::new(("tab_drag", i))
            };
            let sense_resp = ui.interact(tab_rect, drag_id, egui::Sense::click_and_drag());

            if sense_resp.clicked() {
                let click_pos = ui.ctx().input(|inp| inp.pointer.interact_pos());
                let on_close = close_btn_rect.map_or(false, |r| click_pos.map_or(false, |p| r.contains(p)));
                if on_close {
                    tab_to_close = Some(i);
                } else {
                    tab_to_activate = Some(i);
                    if !is_detached && tab.sessions
                        .get(tab.focused_session)
                        .map(|s| s.needs_reconnect())
                        .unwrap_or(false)
                    {
                        tab_to_reconnect = Some(i);
                    }
                }
            }

            if sense_resp.drag_started() {
                tab_drag.source_index = Some(i);
                tab_drag.ghost_size = tab_rect.size();
                let display_title = tab.sessions
                    .get(tab.focused_session)
                    .and_then(|s| s.cwd.as_ref())
                    .and_then(|cwd| {
                        std::path::Path::new(cwd)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                    })
                    .unwrap_or_else(|| tab.title.clone());
                tab_drag.ghost_title = display_title;
                tab_drag.target_index = None;
                tab_drag.is_merge = false;
            }

            // Context menu
            let tab_count = tabs.len();
            sense_resp.context_menu(|ui| {
                if ui.add_enabled(tab_count > 1, egui::Button::new(language.t("close_tab"))).clicked() {
                    tab_to_close = Some(i);
                    ui.close_menu();
                }
            });
        }

        // Handle drag state
        if let Some(src) = tab_drag.source_index {
            if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                tab_drag.target_index = None;
                tab_drag.insert_before = true;
                tab_drag.is_merge = false;

                for (i, rect) in tab_rects.iter().enumerate() {
                    if Some(i) == tab_drag.source_index {
                        continue;
                    }
                    if rect.contains(pos) {
                        tab_drag.target_index = Some(i);
                        let center_x = rect.center().x;
                        let edge_threshold = rect.width() * 0.25;
                        if pos.x >= rect.min.x + edge_threshold && pos.x <= rect.max.x - edge_threshold {
                            tab_drag.is_merge = true;
                        } else {
                            tab_drag.insert_before = pos.x < center_x;
                        }
                        break;
                    }
                }

                // Find closest tab if not hovering over any
                if tab_drag.target_index.is_none() && full_tab_bar_rect.contains(pos) {
                    let mut closest_idx = None;
                    let mut closest_dist = f32::MAX;
                    for (i, rect) in tab_rects.iter().enumerate() {
                        if Some(i) == tab_drag.source_index {
                            continue;
                        }
                        let center_x = rect.center().x;
                        let dist = (pos.x - center_x).abs();
                        if dist < closest_dist {
                            closest_dist = dist;
                            closest_idx = Some(i);
                        }
                    }
                    if let Some(i) = closest_idx {
                        tab_drag.target_index = Some(i);
                        let center_x = tab_rects[i].center().x;
                        tab_drag.insert_before = pos.x < center_x;
                    }
                }

                // Draw ghost tab
                let ghost_rect = egui::Rect::from_center_size(pos, tab_drag.ghost_size);
                let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::new("tab_ghost")));
                painter.rect_filled(
                    ghost_rect,
                    egui::Rounding::same(8.0),
                    egui::Color32::from_rgba_unmultiplied(40, 40, 50, 200)
                );
                painter.rect_stroke(
                    ghost_rect,
                    egui::Rounding::same(8.0),
                    egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(150, 150, 170, 150))
                );

                // Draw ghost text
                let text_pos = egui::pos2(ghost_rect.min.x + 12.0, ghost_rect.center().y - 7.0);
                painter.text(
                    text_pos,
                    egui::Align2::LEFT_CENTER,
                    &tab_drag.ghost_title,
                    egui::FontId::new(13.0, egui::FontFamily::Monospace),
                    egui::Color32::from_rgba_unmultiplied(220, 228, 255, 180)
                );

                // Draw insertion indicator
                if let Some(dst_idx) = tab_drag.target_index {
                    if !tab_drag.is_merge && dst_idx < tab_rects.len() {
                        draw_insertion_indicator(ui, tab_rects[dst_idx], tab_drag.insert_before, current_time, theme);
                    }
                }
            }

            // Handle drop
            if ctx.input(|i| i.pointer.any_released()) {
                if let Some(dst) = tab_drag.target_index {
                    if src != dst && src < tabs.len() && dst < tabs.len() {
                        if tab_drag.is_merge {
                            result = TabBarAction::MergeTabs { src, dst };
                        } else {
                            result = TabBarAction::ReorderTab { src, dst, insert_before: tab_drag.insert_before };
                        }
                    }
                } else if !full_tab_bar_rect.contains(ctx.input(|i| i.pointer.hover_pos()).unwrap_or_default()) {
                    if src < tabs.len() {
                        result = TabBarAction::DetachTab(src);
                    }
                }
                tab_drag.reset();
            }
        }

        // New tab button
        ui.add_space(4.0);
        if ui.add(
            egui::Button::new(egui::RichText::new("+").color(theme.fg_dim).size(16.0))
                .frame(false)
        ).clicked() {
            result = TabBarAction::NewTab;
        }

        // Apply immediate actions
        if let TabBarAction::None = result {
            if let Some(i) = tab_to_activate {
                result = TabBarAction::ActivateTab(i);
            } else if let Some(i) = tab_to_reconnect {
                result = TabBarAction::ReconnectTab(i);
            } else if let Some(i) = tab_to_close {
                result = TabBarAction::CloseTab(i);
            }
        }
    });

    result
}

/// Render the more menu (⋯) button
fn render_more_menu(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    tabs: &[Tab],
    active_tab: usize,
    theme: &ThemeColors,
    language: &Language,
    show_more_menu: &mut bool,
    more_menu_id: egui::Id,
) -> TabBarAction {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let btn_color = if *show_more_menu { theme.accent } else { theme.fg_dim };
        let more_resp = ui.add(
            egui::Button::new(egui::RichText::new("⋯").color(btn_color).size(16.0))
                .frame(false)
        );
        if more_resp.clicked() {
            *show_more_menu = !*show_more_menu;
        }

        if *show_more_menu {
            let popup_pos = egui::pos2(more_resp.rect.min.x, more_resp.rect.max.y + 2.0);
            egui::Area::new(more_menu_id.with("popup"))
                .order(egui::Order::Foreground)
                .fixed_pos(popup_pos)
                .show(ctx, |ui| {
                    egui::Frame {
                        fill: theme.bg_elevated,
                        rounding: egui::Rounding::same(6.0),
                        inner_margin: egui::Margin::same(8.0),
                        stroke: egui::Stroke::new(1.0, theme.border),
                        ..Default::default()
                    }
                    .show(ui, |ui| {
                        ui.set_min_width(200.0);
                        ui.separator();
                        ui.add_space(4.0);
                        let current_tab_broadcast = active_tab < tabs.len() && tabs[active_tab].broadcast_enabled;
                        let broadcast_label = if current_tab_broadcast {
                            format!("◉ {}  ⌘⇧I", language.t("broadcast_off"))
                        } else {
                            format!("○ {}  ⌘⇧I", language.t("broadcast_on"))
                        };
                        if ui.add(
                            egui::Button::new(
                                egui::RichText::new(&broadcast_label)
                                    .color(if current_tab_broadcast { theme.accent } else { theme.fg_primary })
                                    .size(13.0)
                            )
                            .frame(false)
                        ).clicked() {
                            return TabBarAction::ToggleBroadcast(active_tab);
                        }
                        TabBarAction::None
                    });
                });

            // Close menu when clicking outside
            if ctx.input(|i| i.pointer.any_pressed()) {
                if let Some(_pos) = ctx.input(|i| i.pointer.interact_pos()) {
                    // This will be handled by the caller
                }
            }
        }

        TabBarAction::None
    }).inner
}

/// Draw animated insertion indicator
fn draw_insertion_indicator(
    ui: &mut egui::Ui,
    target_rect: egui::Rect,
    insert_before: bool,
    time: f64,
    theme: &ThemeColors,
) {
    let painter = ui.painter();

    let x_pos = if insert_before {
        target_rect.min.x
    } else {
        target_rect.max.x
    };

    let pulse = ((time * 3.0).sin() * 0.5 + 0.5) as f32;
    let alpha = ((time * 2.0).sin() * 0.3 + 0.7) as u8;
    let line_width = 4.0 + pulse * 2.0;
    let expand = pulse * 3.0;

    let line_rect = egui::Rect::from_min_max(
        egui::pos2(x_pos - line_width / 2.0 - expand, target_rect.min.y - 4.0 - expand),
        egui::pos2(x_pos + line_width / 2.0 + expand, target_rect.max.y + 4.0 + expand),
    );

    let (r, g, b) = (theme.accent.r(), theme.accent.g(), theme.accent.b());

    painter.rect_filled(
        line_rect.expand(3.0),
        egui::Rounding::same(4.0),
        egui::Color32::from_rgba_unmultiplied(r, g, b, (alpha as f32 / 255.0 * 80.0) as u8)
    );
    painter.rect_filled(
        line_rect.expand(1.0),
        egui::Rounding::same(3.0),
        egui::Color32::from_rgba_unmultiplied(r, g, b, (alpha as f32 / 255.0 * 150.0) as u8)
    );
    painter.rect_filled(
        line_rect,
        egui::Rounding::same(2.0),
        egui::Color32::from_rgba_unmultiplied(r, g, b, alpha)
    );
}
