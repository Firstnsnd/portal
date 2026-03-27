//! # Tunnels View
//!
//! Rendering for the SSH tunnel management page.

use eframe::egui;

// These types are defined in pane_view.rs
use crate::ui::pane_view::{WindowContext, ViewActions};
use crate::ui::pane::AppWindow;
use crate::ui::tokens::*;
use crate::ui::widgets;
use crate::config::ForwardKind;

/// Render tunnels view for this window
pub fn render_tunnels_view(
    window: &mut AppWindow,
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    cx: &mut WindowContext,
) -> ViewActions {
    // Top navigation bar
    egui::TopBottomPanel::top("tunnels_nav_bar")
        .frame(egui::Frame {
            fill: cx.theme.bg_secondary,
            inner_margin: egui::Margin::symmetric(8.0, 8.0),
            stroke: egui::Stroke::NONE,
            ..Default::default()
        })
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(cx.language.t("tunnels"))
                        .color(cx.theme.fg_dim)
                        .size(FONT_BASE)
                        .strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(widgets::text_button(cx.language.t("new_tunnel"), cx.theme.accent)).clicked() {
                        window.add_tunnel_dialog.reset();
                        if let Some(first_host_idx) = cx.hosts.iter().position(|h| !h.is_local) {
                            window.add_tunnel_dialog.selected_host_idx = Some(first_host_idx);
                        }
                        window.add_tunnel_dialog.open_drawer();
                    }
                });
            });
            ui.add_space(4.0);
        });

    // Main content area
    egui::CentralPanel::default()
        .frame(egui::Frame {
            fill: cx.theme.bg_primary,
            ..Default::default()
        })
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("tunnels_page_scroll")
                .show(ui, |ui| {
                    ui.add_space(SPACE_MD);

                    // Group hosts by their tunnels
                    let hosts_with_tunnels: Vec<(usize, crate::config::HostEntry)> = cx.hosts.iter()
                        .enumerate()
                        .filter(|(_, h)| !h.is_local && !h.port_forwards.is_empty())
                        .map(|(i, h)| (i, h.clone()))
                        .collect();

                    let has_tunnels = !hosts_with_tunnels.is_empty();

                    if has_tunnels {
                        ui.horizontal(|ui| {
                            ui.add_space(SPACE_XL);
                            ui.label(widgets::section_header(cx.language.t("ssh_tunnels"), cx.theme));
                        });
                        ui.add_space(SPACE_XS);
                    }

                    for (host_idx, host) in &hosts_with_tunnels {
                        ui.horizontal(|ui| {
                            ui.add_space(SPACE_XL);
                            ui.label(
                                egui::RichText::new(&host.name)
                                    .color(cx.theme.fg_dim)
                                    .size(FONT_XS)
                                    .strong(),
                            );
                            ui.label(
                                egui::RichText::new(format!("{}:{}", host.host, host.port))
                                    .color(cx.theme.fg_dim)
                                    .size(FONT_XS),
                            );
                        });
                        ui.add_space(SPACE_XS);

                        for (tunnel_idx, tunnel) in host.port_forwards.iter().enumerate() {
                            let row_h = LIST_ROW_HEIGHT;
                            let width = ui.available_width();
                            let (rect, resp): (egui::Rect, egui::Response) = ui.allocate_exact_size(
                                egui::vec2(width, row_h),
                                egui::Sense::click(),
                            );

                            let hovered = resp.hovered();

                            if hovered {
                                ui.painter().rect_filled(
                                    egui::Rect::from_min_max(
                                        egui::pos2(rect.min.x + SPACE_XL, rect.min.y),
                                        egui::pos2(rect.max.x - SPACE_MD, rect.max.y),
                                    ),
                                    RADIUS_SM,
                                    cx.theme.hover_bg,
                                );
                            }

                            // Status indicator (gray for now - active state needs session access)
                            ui.painter().circle_filled(
                                egui::pos2(rect.min.x + SPACE_XL + 8.0, rect.center().y),
                                3.0,
                                cx.theme.fg_dim,
                            );

                            let kind_badge = match tunnel.kind {
                                ForwardKind::Local => "L",
                                ForwardKind::Remote => "R",
                            };
                            ui.painter().text(
                                egui::pos2(rect.min.x + SPACE_XL + 20.0, rect.min.y + 14.0),
                                egui::Align2::LEFT_TOP,
                                kind_badge,
                                egui::FontId::proportional(FONT_XS),
                                cx.theme.accent,
                            );

                            let detail_text = match tunnel.kind {
                                ForwardKind::Local => {
                                    format!("{}:{} → {}:{}",
                                        tunnel.local_host, tunnel.local_port,
                                        tunnel.remote_host, tunnel.remote_port
                                    )
                                }
                                ForwardKind::Remote => {
                                    format!("{}:{} → {}:{}",
                                        tunnel.remote_host, tunnel.remote_port,
                                        tunnel.local_host, tunnel.local_port
                                    )
                                }
                            };

                            ui.painter().text(
                                egui::pos2(rect.min.x + SPACE_XL + 38.0, rect.min.y + 14.0),
                                egui::Align2::LEFT_TOP,
                                &detail_text,
                                egui::FontId::monospace(FONT_MD),
                                cx.theme.fg_primary,
                            );

                            ui.painter().text(
                                egui::pos2(rect.min.x + SPACE_XL + 38.0, rect.min.y + 34.0),
                                egui::Align2::LEFT_TOP,
                                cx.language.t("tunnel_status_inactive"),
                                egui::FontId::proportional(FONT_XS),
                                cx.theme.fg_dim,
                            );

                            // Click to open edit drawer
                            if resp.clicked() {
                                window.add_tunnel_dialog.open_edit(*host_idx, tunnel_idx, tunnel);
                            }
                        }

                        ui.add_space(SPACE_MD);
                    }

                    if hosts_with_tunnels.is_empty() {
                        ui.add_space(60.0);
                        ui.vertical_centered(|ui| {
                            ui.label(egui::RichText::new("\u{1F510}").size(SPACE_2XL).color(cx.theme.fg_dim));
                            ui.add_space(SPACE_MD);
                            ui.label(egui::RichText::new(cx.language.t("no_tunnels")).color(cx.theme.fg_dim).size(FONT_BASE));
                            ui.add_space(SPACE_SM);
                            ui.label(egui::RichText::new(cx.language.t("no_tunnels_hint")).color(cx.theme.fg_dim).size(FONT_SM));
                        });
                    }
                });
        });

    ViewActions::default()
}
/// Render the add/edit tunnel drawer (shadcn/ui style)
pub fn render_tunnel_drawer(window: &mut AppWindow, ctx: &egui::Context, cx: &mut WindowContext) {
    use crate::config::PortForwardConfig;

    if !window.add_tunnel_dialog.open {
        return;
    }

    let is_editing = window.add_tunnel_dialog.edit_index.is_some();
    let drawer_title = if is_editing {
        cx.language.t("edit_tunnel")
    } else {
        cx.language.t("add_tunnel")
    };

    egui::SidePanel::right("tunnel_drawer")
        .default_width(400.0)
        .frame(egui::Frame {
            fill: cx.theme.bg_elevated,
            inner_margin: egui::Margin::ZERO,
            ..Default::default()
        })
        .show(ctx, |ui| {
            // Header
            egui::TopBottomPanel::top("tunnel_drawer_header")
                .exact_height(56.0)
                .frame(egui::Frame {
                    fill: cx.theme.bg_elevated,
                    inner_margin: egui::Margin { left: 24.0, right: 16.0, top: 16.0, bottom: 16.0 },
                    ..Default::default()
                })
                .show_inside(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(drawer_title)
                            .size(16.0).strong().color(cx.theme.fg_primary));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            if ui.add(
                                egui::Button::new(egui::RichText::new("×").size(20.0).color(cx.theme.fg_dim))
                                    .frame(false)
                                    .rounding(4.0)
                                    .min_size(egui::vec2(32.0, 32.0))
                            ).clicked() {
                                window.add_tunnel_dialog.close_drawer();
                                window.add_tunnel_dialog.edit_index = None;
                            }
                            if is_editing {
                                if ui.add(
                                    egui::Button::new(egui::RichText::new("\u{1F5D1}").size(FONT_BASE))
                                        .frame(false)
                                        .rounding(4.0)
                                        .min_size(egui::vec2(28.0, 28.0))
                                ).on_hover_text(cx.language.t("delete"))
                                .clicked() {
                                    window.add_tunnel_dialog.confirm_delete = window.add_tunnel_dialog.edit_index;
                                    window.add_tunnel_dialog.open = false;
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
                .id_salt("tunnel_drawer_scroll")
                .show(ui, |ui| {
                    egui::Frame::none()
                        .inner_margin(egui::Margin::symmetric(widgets::FORM_LEFT_MARGIN, 0.0))
                        .show(ui, |ui| {
                            // SSH Host selector
                            ui.vertical(|ui| {
                                widgets::form_label(ui, cx.language.t("tunnel_ssh_host"), true, cx.theme);
                                ui.add_space(widgets::SPACING_LABEL);
                                let host_items: Vec<String> = cx.hosts.iter()
                                    .filter(|h| !h.is_local)
                                    .map(|h| h.name.clone())
                                    .collect();
                                let selected_text = window.add_tunnel_dialog.selected_host_idx
                                    .and_then(|i| host_items.get(i).cloned())
                                    .unwrap_or_else(|| cx.language.t("select_session").to_string());
                                egui::ComboBox::from_id_salt("tunnel_ssh_host")
                                    .selected_text(egui::RichText::new(selected_text).size(widgets::FONT_SIZE_INPUT).color(cx.theme.fg_primary))
                                    .width(ui.available_width())
                                    .show_ui(ui, |ui| {
                                        widgets::style_dropdown(ui, cx.theme);
                                        for (idx, item) in host_items.iter().enumerate() {
                                            if ui.selectable_label(
                                                window.add_tunnel_dialog.selected_host_idx == Some(idx),
                                                item
                                            ).clicked() {
                                                window.add_tunnel_dialog.selected_host_idx = Some(idx);
                                            }
                                        }
                                    });
                            });
                            ui.add_space(widgets::SPACING_FIELD);

                            // Forward type selector
                            ui.vertical(|ui| {
                                widgets::form_label(ui, cx.language.t("forward_kind"), true, cx.theme);
                                ui.add_space(widgets::SPACING_LABEL);
                                let selected_text = match window.add_tunnel_dialog.forward_kind {
                                    crate::config::ForwardKind::Local => cx.language.t("tunnel_local_forward"),
                                    crate::config::ForwardKind::Remote => cx.language.t("tunnel_remote_forward"),
                                };
                                egui::ComboBox::from_id_salt("forward_kind")
                                    .selected_text(egui::RichText::new(selected_text).size(widgets::FONT_SIZE_INPUT).color(cx.theme.fg_primary))
                                    .width(ui.available_width())
                                    .show_ui(ui, |ui| {
                                        widgets::style_dropdown(ui, cx.theme);
                                        if ui.selectable_label(
                                            matches!(window.add_tunnel_dialog.forward_kind, crate::config::ForwardKind::Local),
                                            cx.language.t("tunnel_local_forward")
                                        ).clicked() {
                                            window.add_tunnel_dialog.forward_kind = crate::config::ForwardKind::Local;
                                        }
                                        if ui.selectable_label(
                                            matches!(window.add_tunnel_dialog.forward_kind, crate::config::ForwardKind::Remote),
                                            cx.language.t("tunnel_remote_forward")
                                        ).clicked() {
                                            window.add_tunnel_dialog.forward_kind = crate::config::ForwardKind::Remote;
                                        }
                                    });
                            });
                            ui.add_space(widgets::SPACING_FIELD);

                            // Local host + port in same row
                            widgets::form_field_2col(
                                ui,
                                cx.language.t("tunnel_local_host"), true,
                                &mut window.add_tunnel_dialog.local_host,
                                cx.language.t("tunnel_local_host_placeholder"), 170.0,
                                cx.language.t("tunnel_local_port"), true,
                                &mut window.add_tunnel_dialog.local_port,
                                cx.language.t("tunnel_local_port_placeholder"), 70.0,
                                cx.theme
                            );
                            ui.add_space(widgets::SPACING_FIELD);

                            // Remote host + port in same row
                            widgets::form_field_2col(
                                ui,
                                cx.language.t("tunnel_remote_host"), true,
                                &mut window.add_tunnel_dialog.remote_host,
                                cx.language.t("tunnel_remote_host_placeholder"), 170.0,
                                cx.language.t("tunnel_remote_port"), true,
                                &mut window.add_tunnel_dialog.remote_port,
                                cx.language.t("tunnel_remote_port_placeholder"), 70.0,
                                cx.theme
                            );

                            // Error message
                            if !window.add_tunnel_dialog.error.is_empty() {
                                ui.add_space(widgets::SPACING_FIELD);
                                ui.label(egui::RichText::new(&window.add_tunnel_dialog.error)
                                    .color(cx.theme.red).size(12.0));
                            }

                            ui.add_space(widgets::SPACING_SECTION);

                    // Footer buttons
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let can_save = window.add_tunnel_dialog.selected_host_idx.is_some()
                            && !window.add_tunnel_dialog.local_host.trim().is_empty()
                            && !window.add_tunnel_dialog.local_port.trim().is_empty()
                            && !window.add_tunnel_dialog.remote_host.trim().is_empty()
                            && !window.add_tunnel_dialog.remote_port.trim().is_empty();

                        let button_text = if is_editing {
                            cx.language.t("save")
                        } else {
                            cx.language.t("add_tunnel")
                        };

                        if ui.add(widgets::primary_button(button_text, cx.theme)).clicked() && can_save {
                            if let Some(host_idx) = window.add_tunnel_dialog.selected_host_idx {
                                if host_idx < cx.hosts.len() {
                                    let local_port = window.add_tunnel_dialog.local_port.trim().parse::<u16>();
                                    let remote_port = window.add_tunnel_dialog.remote_port.trim().parse::<u16>();

                                    if let (Ok(lp), Ok(rp)) = (local_port, remote_port) {
                                        let forward = PortForwardConfig {
                                            kind: window.add_tunnel_dialog.forward_kind.clone(),
                                            local_host: window.add_tunnel_dialog.local_host.trim().to_string(),
                                            local_port: lp,
                                            remote_host: window.add_tunnel_dialog.remote_host.trim().to_string(),
                                            remote_port: rp,
                                        };

                                        if let Some((edit_host_idx, edit_tunnel_idx)) = window.add_tunnel_dialog.edit_index {
                                            // Edit mode: update existing tunnel
                                            if edit_host_idx < cx.hosts.len() {
                                                if let Some(existing) = cx.hosts[edit_host_idx].port_forwards.get_mut(edit_tunnel_idx) {
                                                    *existing = forward;
                                                }
                                            }
                                        } else {
                                            // Add mode: create new tunnel
                                            cx.hosts[host_idx].port_forwards.push(forward);
                                        }
                                        window.add_tunnel_dialog.reset();
                                    } else {
                                        window.add_tunnel_dialog.error = "Invalid port numbers".to_string();
                                    }
                                }
                            }
                        }
                        ui.add_space(8.0);
                        if ui.add(widgets::secondary_button(cx.language.t("cancel"), cx.theme)).clicked() {
                            window.add_tunnel_dialog.close_drawer();
                            window.add_tunnel_dialog.edit_index = None;
                        }
                    });
                });
            ui.add_space(24.0);
        });
    });

    // Delete confirmation dialog
    if let Some((host_idx, tunnel_idx)) = window.add_tunnel_dialog.confirm_delete {
        let tunnel_name = cx.hosts.get(host_idx)
            .and_then(|h| h.port_forwards.get(tunnel_idx))
            .map(|t| {
                format!("{}:{}", t.local_host, t.local_port)
            })
            .unwrap_or_default();
        let mut open = true;
        egui::Window::new("delete_tunnel")
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
                    ui.label(egui::RichText::new(cx.language.t("delete_tunnel")).size(15.0).color(cx.theme.fg_primary).strong());
                });
                ui.add_space(10.0);
                ui.label(egui::RichText::new(cx.language.tf("delete_confirm", &tunnel_name)).color(cx.theme.fg_primary).size(FONT_BASE));
                ui.add_space(SPACE_XS);
                ui.label(egui::RichText::new(cx.language.t("confirm_delete")).color(cx.theme.fg_dim).size(FONT_SM));
                ui.add_space(SPACE_LG);

                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(widgets::danger_button(cx.language.t("delete"), cx.theme)).clicked() {
                            if let Some(host) = cx.hosts.get_mut(host_idx) {
                                host.port_forwards.remove(tunnel_idx);
                            }
                            window.add_tunnel_dialog.confirm_delete = None;
                            window.add_tunnel_dialog.edit_index = None;
                        }
                        if ui.add(widgets::secondary_button(cx.language.t("cancel"), cx.theme)).clicked() {
                            window.add_tunnel_dialog.confirm_delete = None;
                        }
                    });
                });
            });

        if !open {
            window.add_tunnel_dialog.confirm_delete = None;
        }
    }
}
