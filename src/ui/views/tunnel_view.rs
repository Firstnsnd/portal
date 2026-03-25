//! SSH Tunnels (Port Forwarding) View

use crate::app::PortalApp;
use crate::config::{ForwardKind, PortForwardConfig};
use crate::ui::types::session::SessionBackend;
use crate::ui::tokens::*;
use crate::ui::widgets;
use eframe::egui;
use std::collections::HashMap;

impl PortalApp {
    /// Tunnels page with list of host port forwards and add drawer
    pub fn show_tunnels_page(&mut self, ctx: &egui::Context, _ui: &mut egui::Ui) {
        let theme = self.theme.clone();
        let lang = self.language;

        // Top navigation bar (matching terminal tab bar style)
        egui::TopBottomPanel::top("tunnels_nav_bar")
            .frame(egui::Frame {
                fill: theme.bg_secondary,
                inner_margin: egui::Margin::symmetric(8.0, 4.0),
                stroke: egui::Stroke::NONE,
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(lang.t("tunnels"))
                            .color(theme.fg_dim)
                            .size(FONT_BASE)
                            .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(widgets::text_button(lang.t("new_tunnel"), theme.accent)).clicked() {
                            // Reset and select first available host
                            self.add_tunnel_dialog.reset();
                            if let Some(first_host_idx) = self.hosts.iter().position(|h| !h.is_local) {
                                self.add_tunnel_dialog.selected_host_idx = Some(first_host_idx);
                            }
                            self.add_tunnel_dialog.open_drawer();
                        }
                    });
                });
                ui.add_space(4.0);
            });

        // Main content area
        egui::CentralPanel::default()
            .frame(egui::Frame {
                fill: theme.bg_primary,
                ..Default::default()
            })
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("tunnels_page_scroll")
                    .show(ui, |ui| {
                        ui.add_space(SPACE_MD);

                        let mut tunnel_to_delete: Option<(usize, usize)> = None;
                        let mut tunnel_to_start: Option<(usize, usize)> = None;
                        let mut tunnel_to_stop: Option<(usize, usize)> = None;

                        // Collect active tunnel states from SSH sessions
                        let mut active_states: HashMap<(usize, usize), crate::ssh::port_forward::ForwardState> =
                            HashMap::new();

                        for tab in &self.tabs {
                            for session in &tab.sessions {
                                if let Some(SessionBackend::Ssh(ssh_session)) = &session.session {
                                    if let Some(host_entry) = &session.ssh_host {
                                        let states = ssh_session.get_port_forward_states();
                                        if let Some(host_idx) = self.hosts.iter().position(|h| h.name == host_entry.name) {
                                            for (config, state) in states {
                                                if let Some(tunnel_idx) = host_entry.port_forwards.iter().position(|t| {
                                                    t.kind == config.kind &&
                                                    t.local_host == config.local_host &&
                                                    t.local_port == config.local_port &&
                                                    t.remote_host == config.remote_host &&
                                                    t.remote_port == config.remote_port
                                                }) {
                                                    active_states.insert((host_idx, tunnel_idx), state);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Group hosts by their tunnels
                        let hosts_with_tunnels: Vec<(usize, crate::config::HostEntry)> = self.hosts.iter()
                            .enumerate()
                            .filter(|(_, h)| !h.is_local && !h.port_forwards.is_empty())
                            .map(|(i, h)| (i, h.clone()))
                            .collect();

                        let has_tunnels = !hosts_with_tunnels.is_empty();

                        // Section header for tunnels list
                        if has_tunnels {
                            ui.horizontal(|ui| {
                                ui.add_space(SPACE_XL);
                                ui.label(widgets::section_header(lang.t("ssh_tunnels"), &theme));
                            });
                            ui.add_space(SPACE_XS);
                        }

                        // Render each host with its tunnels
                        for (host_idx, host) in &hosts_with_tunnels {
                            // Host section header
                            ui.horizontal(|ui| {
                                ui.add_space(SPACE_XL);
                                ui.label(
                                    egui::RichText::new(&host.name)
                                        .color(theme.fg_dim)
                                        .size(FONT_XS)
                                        .strong(),
                                );
                                ui.label(
                                    egui::RichText::new(format!("{}:{}", host.host, host.port))
                                        .color(theme.fg_dim)
                                        .size(FONT_XS),
                                );
                            });
                            ui.add_space(SPACE_XS);

                            // Tunnels list
                            for (tunnel_idx, tunnel) in host.port_forwards.iter().enumerate() {
                                let row_h = LIST_ROW_HEIGHT;
                                let width = ui.available_width();
                                let (rect, resp): (egui::Rect, egui::Response) = ui.allocate_exact_size(
                                    egui::vec2(width, row_h),
                                    egui::Sense::click(),
                                );

                                let hovered = resp.hovered();
                                let state = active_states.get(&(*host_idx, tunnel_idx));
                                let is_confirming_delete =
                                    self.add_tunnel_dialog.confirm_delete == Some((*host_idx, tunnel_idx));

                                // Background hover effect
                                if hovered {
                                    ui.painter().rect_filled(
                                        egui::Rect::from_min_max(
                                            egui::pos2(rect.min.x + SPACE_XL, rect.min.y),
                                            egui::pos2(rect.max.x - SPACE_MD, rect.max.y),
                                        ),
                                        RADIUS_SM,
                                        theme.hover_bg,
                                    );
                                }

                                // Status indicator (colored circle)
                                let status_color = match state {
                                    Some(crate::ssh::port_forward::ForwardState::Active) => theme.green,
                                    Some(crate::ssh::port_forward::ForwardState::Starting) => theme.accent,
                                    Some(crate::ssh::port_forward::ForwardState::Error(_)) => theme.red,
                                    Some(crate::ssh::port_forward::ForwardState::Stopped) | None => theme.fg_dim,
                                };

                                ui.painter().circle_filled(
                                    egui::pos2(rect.min.x + SPACE_XL + 8.0, rect.center().y),
                                    3.0,
                                    status_color,
                                );

                                // Kind badge
                                let kind_badge = match tunnel.kind {
                                    ForwardKind::Local => "L",
                                    ForwardKind::Remote => "R",
                                };
                                ui.painter().text(
                                    egui::pos2(rect.min.x + SPACE_XL + 20.0, rect.min.y + 14.0),
                                    egui::Align2::LEFT_TOP,
                                    kind_badge,
                                    egui::FontId::proportional(FONT_XS),
                                    theme.accent,
                                );

                                // Tunnel details (using monospace for addresses)
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
                                    theme.fg_primary,
                                );

                                // Status text
                                let status_text = match state {
                                    Some(crate::ssh::port_forward::ForwardState::Active) => lang.t("tunnel_status_active"),
                                    Some(crate::ssh::port_forward::ForwardState::Starting) => lang.t("tunnel_status_starting"),
                                    Some(crate::ssh::port_forward::ForwardState::Error(e)) => {
                                        ui.painter().text(
                                            egui::pos2(rect.min.x + SPACE_XL + 38.0, rect.min.y + 34.0),
                                            egui::Align2::LEFT_TOP,
                                            e,
                                            egui::FontId::proportional(FONT_XS),
                                            theme.red,
                                        );
                                        lang.t("tunnel_status_error")
                                    }
                                    Some(crate::ssh::port_forward::ForwardState::Stopped) => lang.t("tunnel_status_stopped"),
                                    None => lang.t("tunnel_status_inactive"),
                                };

                                ui.painter().text(
                                    egui::pos2(rect.min.x + SPACE_XL + 38.0, rect.min.y + 34.0),
                                    egui::Align2::LEFT_TOP,
                                    status_text,
                                    egui::FontId::proportional(FONT_XS),
                                    status_color,
                                );

                                // Action buttons on hover
                                if hovered {
                                    let visible_right = rect.max.x - SPACE_MD;
                                    let pointer_pos = ui.ctx().input(|i| i.pointer.hover_pos());

                                    if is_confirming_delete {
                                        // Delete confirmation
                                        let cancel_rect = egui::Rect::from_center_size(
                                            egui::pos2(visible_right - 50.0, rect.center().y),
                                            egui::vec2(50.0, 22.0),
                                        );
                                        let delete_rect = egui::Rect::from_center_size(
                                            egui::pos2(visible_right - 106.0, rect.center().y),
                                            egui::vec2(50.0, 22.0),
                                        );

                                        // Cancel button
                                        ui.painter().rect(
                                            cancel_rect,
                                            RADIUS_SM,
                                            theme.bg_elevated,
                                            egui::Stroke::new(1.0, theme.border),
                                        );
                                        ui.painter().text(
                                            cancel_rect.center(),
                                            egui::Align2::CENTER_CENTER,
                                            lang.t("cancel"),
                                            egui::FontId::proportional(FONT_XS),
                                            theme.fg_dim,
                                        );

                                        // Delete button
                                        ui.painter().rect(
                                            delete_rect,
                                            RADIUS_SM,
                                            theme.red,
                                            egui::Stroke::NONE,
                                        );
                                        ui.painter().text(
                                            delete_rect.center(),
                                            egui::Align2::CENTER_CENTER,
                                            lang.t("delete"),
                                            egui::FontId::proportional(FONT_XS),
                                            theme.bg_primary,
                                        );

                                        // Handle clicks
                                        if let Some(pos) = pointer_pos {
                                            if resp.clicked() {
                                                if cancel_rect.contains(pos) {
                                                    self.add_tunnel_dialog.confirm_delete = None;
                                                } else if delete_rect.contains(pos) {
                                                    tunnel_to_delete = Some((*host_idx, tunnel_idx));
                                                }
                                            }
                                        }
                                    } else {
                                        // Start/Stop button
                                        let is_active = matches!(state, Some(crate::ssh::port_forward::ForwardState::Active));
                                        let action_rect = egui::Rect::from_center_size(
                                            egui::pos2(visible_right - 34.0, rect.center().y),
                                            egui::vec2(50.0, 24.0),
                                        );

                                        let over_action = pointer_pos.map_or(false, |p| action_rect.contains(p));
                                        let action_bg = if over_action {
                                            if is_active { theme.red } else { theme.accent }
                                        } else {
                                            theme.bg_elevated
                                        };

                                        ui.painter().rect(
                                            action_rect,
                                            RADIUS_SM,
                                            action_bg,
                                            egui::Stroke::new(1.0, theme.border),
                                        );

                                        let action_label = if is_active { lang.t("tunnel_stop") } else { lang.t("tunnel_start") };
                                        ui.painter().text(
                                            action_rect.center(),
                                            egui::Align2::CENTER_CENTER,
                                            action_label,
                                            egui::FontId::proportional(FONT_XS),
                                            if over_action { theme.bg_elevated } else { theme.fg_dim },
                                        );

                                        // Delete button
                                        let del_rect = egui::Rect::from_center_size(
                                            egui::pos2(visible_right - 98.0, rect.center().y),
                                            egui::vec2(50.0, 24.0),
                                        );

                                        let over_del = pointer_pos.map_or(false, |p| del_rect.contains(p));
                                        let del_bg = if over_del { theme.red } else { theme.bg_elevated };

                                        ui.painter().rect(
                                            del_rect,
                                            RADIUS_SM,
                                            del_bg,
                                            egui::Stroke::new(1.0, if over_del {
                                                theme.red
                                            } else {
                                                theme.border
                                            }),
                                        );
                                        ui.painter().text(
                                            del_rect.center(),
                                            egui::Align2::CENTER_CENTER,
                                            lang.t("delete"),
                                            egui::FontId::proportional(FONT_XS),
                                            if over_del { theme.bg_elevated } else { theme.red },
                                        );

                                        // Handle clicks
                                        if let Some(pos) = pointer_pos {
                                            if resp.clicked() {
                                                if over_action {
                                                    if is_active {
                                                        tunnel_to_stop = Some((*host_idx, tunnel_idx));
                                                    } else {
                                                        tunnel_to_start = Some((*host_idx, tunnel_idx));
                                                    }
                                                } else if del_rect.contains(pos) {
                                                    self.add_tunnel_dialog.confirm_delete = Some((*host_idx, tunnel_idx));
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            ui.add_space(SPACE_MD);
                        }

                        // Empty state
                        if hosts_with_tunnels.is_empty() {
                            ui.add_space(60.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    egui::RichText::new("\u{1F510}")
                                        .size(SPACE_2XL)
                                        .color(theme.fg_dim),
                                );
                                ui.add_space(SPACE_MD);
                                ui.label(
                                    egui::RichText::new(lang.t("no_tunnels"))
                                        .color(theme.fg_dim)
                                        .size(FONT_BASE),
                                );
                                ui.add_space(SPACE_SM);
                                ui.label(
                                    egui::RichText::new(lang.t("no_tunnels_hint"))
                                        .color(theme.fg_dim)
                                        .size(FONT_SM),
                                );
                            });
                        }

                        // Apply deferred actions
                        if let Some((host_idx, tunnel_idx)) = tunnel_to_delete {
                            if let Some(host) = self.hosts.get_mut(host_idx) {
                                host.port_forwards.remove(tunnel_idx);
                                self.save_hosts();
                            }
                            self.add_tunnel_dialog.confirm_delete = None;
                        }

                        if let Some((host_idx, tunnel_idx)) = tunnel_to_start {
                            self.start_tunnel(host_idx, tunnel_idx, ctx);
                        }

                        if let Some((host_idx, tunnel_idx)) = tunnel_to_stop {
                            self.stop_tunnel(host_idx, tunnel_idx);
                        }
                    });
            });

        // ── Show drawer for add tunnel ──
        if self.add_tunnel_dialog.open {
            self.show_add_tunnel_drawer(ctx);
        }
    }

    fn show_add_tunnel_drawer(&mut self, ctx: &egui::Context) {
        let theme = self.theme.clone();
        let lang = self.language;
        let drawer_width = ctx.screen_rect().width().min(DRAWER_WIDTH).max(280.0);

        let mut save_clicked = false;
        let mut close_clicked = false;

        egui::SidePanel::right("add_tunnel_drawer")
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
                    ui.label(egui::RichText::new(lang.t("new_tunnel"))
                        .color(theme.fg_primary)
                        .size(FONT_BASE)
                        .strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(
                            egui::Button::new(egui::RichText::new("\u{2715}").color(theme.fg_dim).size(FONT_MD))
                                .frame(false)
                        ).clicked() {
                            close_clicked = true;
                        }
                    });
                });
                ui.add_space(SPACE_LG);

                // Host selector
                let ssh_hosts: Vec<(usize, String)> = self.hosts.iter()
                    .enumerate()
                    .filter(|(_, h)| !h.is_local)
                    .map(|(i, h)| (i, h.name.clone()))
                    .collect();

                if ssh_hosts.is_empty() {
                    ui.label(egui::RichText::new(lang.t("no_ssh_hosts"))
                        .color(theme.red)
                        .size(FONT_SM));
                } else {
                    ui.label(widgets::field_label(lang.t("tunnel_ssh_host"), &theme));
                    ui.add_space(SPACE_XS);

                    if self.add_tunnel_dialog.selected_host_idx.is_none() {
                        self.add_tunnel_dialog.selected_host_idx = Some(ssh_hosts[0].0);
                    }

                    let current_selection = self.add_tunnel_dialog.selected_host_idx.unwrap_or(ssh_hosts[0].0);
                    let selected_name = ssh_hosts.iter()
                        .find(|(idx, _)| *idx == current_selection)
                        .map(|(_, name)| name.clone())
                        .unwrap_or_else(|| ssh_hosts[0].1.clone());

                    egui::ComboBox::from_id_salt("tunnel_host_selector")
                        .selected_text(egui::RichText::new(&selected_name)
                            .color(theme.fg_primary)
                            .size(FONT_MD))
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            widgets::style_dropdown(ui, &self.theme);
                            for (idx, name) in &ssh_hosts {
                                if ui.add(
                                    egui::Button::new(
                                        egui::RichText::new(name)
                                            .color(if self.add_tunnel_dialog.selected_host_idx == Some(*idx) {
                                                theme.accent
                                            } else {
                                                theme.fg_primary
                                            })
                                            .size(FONT_MD)
                                    )
                                    .frame(false)
                                ).clicked() {
                                    self.add_tunnel_dialog.selected_host_idx = Some(*idx);
                                    ui.close_menu();
                                }
                            }
                        });
                    ui.add_space(SPACE_MD);
                }

                // Forward kind
                ui.label(widgets::field_label(lang.t("forward_kind"), &theme));
                ui.add_space(SPACE_XS);
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.add_tunnel_dialog.forward_kind,
                        ForwardKind::Local,
                        egui::RichText::new(lang.t("local_forward")).size(FONT_MD),
                    );
                    ui.selectable_value(
                        &mut self.add_tunnel_dialog.forward_kind,
                        ForwardKind::Remote,
                        egui::RichText::new(lang.t("remote_forward")).size(FONT_MD),
                    );
                });
                ui.add_space(SPACE_MD);

                // Local host
                ui.label(widgets::field_label(lang.t("tunnel_local_host"), &theme));
                ui.add_space(SPACE_XS);
                ui.add(egui::TextEdit::singleline(&mut self.add_tunnel_dialog.local_host)
                    .desired_width(ui.available_width())
                    .hint_text(egui::RichText::new(lang.t("tunnel_local_host_placeholder")).color(theme.hint_color()).italics())
                    .font(egui::FontId::proportional(FONT_MD))
                    .text_color(theme.fg_primary));
                ui.add_space(SPACE_MD);

                // Local port
                ui.label(widgets::field_label(lang.t("tunnel_local_port"), &theme));
                ui.add_space(SPACE_XS);
                ui.add(egui::TextEdit::singleline(&mut self.add_tunnel_dialog.local_port)
                    .desired_width(ui.available_width())
                    .hint_text(egui::RichText::new(lang.t("tunnel_local_port_placeholder")).color(theme.hint_color()).italics())
                    .font(egui::FontId::proportional(FONT_MD))
                    .text_color(theme.fg_primary));
                ui.add_space(SPACE_MD);

                // Remote host
                ui.label(widgets::field_label(lang.t("tunnel_remote_host"), &theme));
                ui.add_space(SPACE_XS);
                ui.add(egui::TextEdit::singleline(&mut self.add_tunnel_dialog.remote_host)
                    .desired_width(ui.available_width())
                    .hint_text(egui::RichText::new(lang.t("tunnel_remote_host_placeholder")).color(theme.hint_color()).italics())
                    .font(egui::FontId::proportional(FONT_MD))
                    .text_color(theme.fg_primary));
                ui.add_space(SPACE_MD);

                // Remote port
                ui.label(widgets::field_label(lang.t("tunnel_remote_port"), &theme));
                ui.add_space(SPACE_XS);
                ui.add(egui::TextEdit::singleline(&mut self.add_tunnel_dialog.remote_port)
                    .desired_width(ui.available_width())
                    .hint_text(egui::RichText::new(lang.t("tunnel_remote_port_placeholder")).color(theme.hint_color()).italics())
                    .font(egui::FontId::proportional(FONT_MD))
                    .text_color(theme.fg_primary));
                ui.add_space(SPACE_LG);

                // Error message
                if !self.add_tunnel_dialog.error.is_empty() {
                    ui.label(egui::RichText::new(&self.add_tunnel_dialog.error).color(theme.red).size(FONT_SM));
                    ui.add_space(SPACE_MD);
                }

                // Action buttons
                ui.horizontal(|ui| {
                    if ui.add(widgets::primary_button(lang.t("save"), &theme)).clicked() {
                        save_clicked = true;
                    }
                    ui.add_space(SPACE_SM);
                    if ui.add(widgets::secondary_button(lang.t("cancel"), &theme)).clicked() {
                        close_clicked = true;
                    }
                });
            });

        if close_clicked {
            self.add_tunnel_dialog.close_drawer();
        }

        if save_clicked {
            self.save_tunnel_from_dialog();
        }
    }

    fn save_tunnel_from_dialog(&mut self) {
        // Validate inputs
        let local_port: u16 = match self.add_tunnel_dialog.local_port.parse() {
            Ok(p) => p,
            Err(_) => {
                self.add_tunnel_dialog.error = "Invalid local port".to_string();
                return;
            }
        };

        let remote_port: u16 = match self.add_tunnel_dialog.remote_port.parse() {
            Ok(p) => p,
            Err(_) => {
                self.add_tunnel_dialog.error = "Invalid remote port".to_string();
                return;
            }
        };

        // Get selected host
        let host_idx = match self.add_tunnel_dialog.selected_host_idx {
            Some(idx) => idx,
            None => {
                self.add_tunnel_dialog.error = "Please select a host".to_string();
                return;
            }
        };

        let tunnel = PortForwardConfig {
            kind: self.add_tunnel_dialog.forward_kind.clone(),
            local_host: self.add_tunnel_dialog.local_host.clone(),
            local_port,
            remote_host: self.add_tunnel_dialog.remote_host.clone(),
            remote_port,
        };

        if let Some(host) = self.hosts.get_mut(host_idx) {
            host.port_forwards.push(tunnel);
            self.save_hosts();
        } else {
            self.add_tunnel_dialog.error = "Selected host not found".to_string();
            return;
        }

        self.add_tunnel_dialog.reset();
    }

    fn start_tunnel(&mut self, host_idx: usize, tunnel_idx: usize, ctx: &egui::Context) {
        if let Some(host) = self.hosts.get(host_idx) {
            if let Some(tunnel) = host.port_forwards.get(tunnel_idx) {
                for tab in &mut self.tabs {
                    for session in &mut tab.sessions {
                        if let Some(SessionBackend::Ssh(ssh_session)) = &session.session {
                            if let Some(host_entry) = &session.ssh_host {
                                if host_entry.name == host.name {
                                    ssh_session.start_port_forward(tunnel.clone());
                                    ctx.request_repaint();
                                    return;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn stop_tunnel(&mut self, host_idx: usize, tunnel_idx: usize) {
        if let Some(host) = self.hosts.get(host_idx) {
            if let Some(tunnel) = host.port_forwards.get(tunnel_idx) {
                for tab in &mut self.tabs {
                    for session in &mut tab.sessions {
                        if let Some(SessionBackend::Ssh(ssh_session)) = &session.session {
                            if let Some(host_entry) = &session.ssh_host {
                                if host_entry.name == host.name {
                                    // Build the config to match
                                    let config = crate::config::PortForwardConfig {
                                        kind: tunnel.kind.clone(),
                                        local_host: tunnel.local_host.clone(),
                                        local_port: tunnel.local_port,
                                        remote_host: tunnel.remote_host.clone(),
                                        remote_port: tunnel.remote_port,
                                    };
                                    ssh_session.stop_port_forward(config);
                                    return;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
