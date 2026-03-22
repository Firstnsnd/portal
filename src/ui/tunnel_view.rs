use eframe::egui;

use crate::app::PortalApp;
use crate::config::{ForwardKind, PortForwardConfig};
use crate::ssh::port_forward::ForwardState;
use crate::ui::tokens::*;
use crate::ui::types::SessionBackend;
use crate::ui::widgets;

/// Information about a single tunnel gathered from session state
struct TunnelInfo {
    kind: ForwardKind,
    local_host: String,
    local_port: u16,
    remote_host: String,
    remote_port: u16,
    host_name: String,
    state: ForwardState,
    /// Tab index in app.tabs
    tab_idx: usize,
    /// Session index within that tab
    session_idx: usize,
    /// Index of this forward within the session's port_forwards vec
    forward_idx: usize,
}

impl PortalApp {
    pub fn show_tunnels_view(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let theme = self.theme.clone();
        let lang = self.language;

        // Collect all tunnel info from all SSH sessions across all tabs
        let tunnels = self.collect_tunnel_info();

        egui::ScrollArea::vertical()
            .id_salt("tunnels_page_scroll")
            .show(ui, |ui| {
                ui.add_space(SPACE_2XL / 2.0);

                // ── Page header ──
                ui.horizontal(|ui| {
                    ui.add_space(SPACE_XL);
                    ui.label(widgets::section_header(lang.t("tunnels"), &theme));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(SPACE_XL);
                        if ui
                            .add(widgets::text_button(&format!("+ {}", lang.t("add_tunnel")), theme.accent))
                            .clicked()
                        {
                            self.add_tunnel_dialog.reset();
                            self.add_tunnel_dialog.open = true;
                        }
                    });
                });
                ui.add_space(SPACE_LG);

                if tunnels.is_empty() {
                    // ── Empty state ──
                    ui.add_space(60.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("\u{1f310}") // globe icon
                                .size(32.0)
                                .color(theme.fg_dim),
                        );
                        ui.add_space(12.0);
                        ui.label(
                            egui::RichText::new(lang.t("no_tunnels"))
                                .color(theme.fg_dim)
                                .size(13.0),
                        );
                    });
                    ui.add_space(60.0);
                } else {
                    // ── Table header ──
                    ui.horizontal(|ui| {
                        ui.add_space(SPACE_XL);
                        let header_color = theme.fg_dim;
                        let header_size = FONT_XS;
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width() - SPACE_XL, 20.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.allocate_ui(egui::vec2(40.0, 20.0), |ui| {
                                    ui.label(egui::RichText::new("TYPE").color(header_color).size(header_size).strong());
                                });
                                ui.allocate_ui(egui::vec2(140.0, 20.0), |ui| {
                                    ui.label(egui::RichText::new("LOCAL").color(header_color).size(header_size).strong());
                                });
                                ui.allocate_ui(egui::vec2(140.0, 20.0), |ui| {
                                    ui.label(egui::RichText::new("REMOTE").color(header_color).size(header_size).strong());
                                });
                                ui.allocate_ui(egui::vec2(120.0, 20.0), |ui| {
                                    ui.label(egui::RichText::new("HOST").color(header_color).size(header_size).strong());
                                });
                                ui.allocate_ui(egui::vec2(80.0, 20.0), |ui| {
                                    ui.label(egui::RichText::new("STATUS").color(header_color).size(header_size).strong());
                                });
                            },
                        );
                    });
                    ui.add_space(4.0);

                    // Collect actions to apply after iteration (to avoid borrow conflicts)
                    let mut stop_actions: Vec<(usize, usize, usize)> = Vec::new();
                    let mut delete_actions: Vec<(usize, usize, usize)> = Vec::new();

                    // ── Tunnel rows ──
                    for tunnel in &tunnels {
                        ui.horizontal(|ui| {
                            ui.add_space(SPACE_XL);
                            let row_width = ui.available_width() - SPACE_XL;
                            let (rect, _resp) = ui.allocate_exact_size(
                                egui::vec2(row_width, TABLE_ROW_HEIGHT),
                                egui::Sense::hover(),
                            );

                            // Row background (keep as painter for hover effect)
                            if _resp.hovered() {
                                ui.painter().rect_filled(rect, RADIUS_SM, theme.hover_bg);
                            }

                            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
                            child.add_space(SPACE_XS);

                            // Type badge (keep as painter for custom styling)
                            child.allocate_ui(egui::vec2(40.0, TABLE_ROW_HEIGHT), |ui| {
                                let (type_label, type_color) = match tunnel.kind {
                                    ForwardKind::Local => ("L", theme.accent),
                                    ForwardKind::Remote => ("R", theme.green),
                                };
                                let badge_rect = egui::Rect::from_min_size(
                                    egui::pos2(ui.min_rect().min.x, ui.min_rect().center().y - 9.0),
                                    egui::vec2(22.0, 18.0),
                                );
                                ui.painter().rect_filled(badge_rect, 3.0, type_color);
                                ui.painter().text(
                                    badge_rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    type_label,
                                    egui::FontId::monospace(FONT_SM),
                                    egui::Color32::BLACK,
                                );
                            });

                            // Local host:port
                            child.allocate_ui(egui::vec2(140.0, TABLE_ROW_HEIGHT), |ui| {
                                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                    ui.label(
                                        egui::RichText::new(format!("{}:{}", tunnel.local_host, tunnel.local_port))
                                            .family(egui::FontFamily::Monospace)
                                            .size(FONT_MD)
                                            .color(theme.fg_primary),
                                    );
                                });
                            });

                            // Remote host:port
                            child.allocate_ui(egui::vec2(140.0, TABLE_ROW_HEIGHT), |ui| {
                                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                    ui.label(
                                        egui::RichText::new(format!("{}:{}", tunnel.remote_host, tunnel.remote_port))
                                            .family(egui::FontFamily::Monospace)
                                            .size(FONT_MD)
                                            .color(theme.fg_primary),
                                    );
                                });
                            });

                            // Host name
                            child.allocate_ui(egui::vec2(120.0, TABLE_ROW_HEIGHT), |ui| {
                                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                    ui.label(
                                        egui::RichText::new(&tunnel.host_name)
                                            .size(FONT_MD)
                                            .color(theme.fg_dim),
                                    );
                                });
                            });

                            // Status badge
                            child.allocate_ui(egui::vec2(80.0, TABLE_ROW_HEIGHT), |ui| {
                                let yellow = egui::Color32::from_rgb(224, 175, 50);
                                let gray = theme.fg_dim;
                                let (status_text, status_color) = match &tunnel.state {
                                    ForwardState::Active => (lang.t("tunnel_status_active"), theme.green),
                                    ForwardState::Starting => (lang.t("tunnel_status_starting"), yellow),
                                    ForwardState::Stopped => (lang.t("tunnel_status_stopped"), gray),
                                    ForwardState::Error(_) => (lang.t("tunnel_status_error"), theme.red),
                                };
                                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                    ui.label(
                                        egui::RichText::new(status_text)
                                            .size(FONT_SM)
                                            .color(status_color),
                                    );
                                });
                            });

                            // Action buttons — use egui Button widgets
                            let is_active = matches!(tunnel.state, ForwardState::Active | ForwardState::Starting);
                            let stop_label = if is_active { "\u{25a0}" } else { "\u{25b6}" }; // stop / play

                            let stop_resp = child.add(
                                egui::Button::new(
                                    egui::RichText::new(stop_label).size(FONT_MD).color(theme.fg_dim)
                                )
                                .frame(false)
                                .min_size(egui::vec2(40.0, 20.0)),
                            );
                            if stop_resp.clicked() {
                                stop_actions.push((tunnel.tab_idx, tunnel.session_idx, tunnel.forward_idx));
                            }

                            let del_resp = child.add(
                                egui::Button::new(
                                    egui::RichText::new("\u{2715}").size(FONT_MD).color(theme.fg_dim)
                                )
                                .frame(false)
                                .min_size(egui::vec2(40.0, 20.0)),
                            );
                            if del_resp.clicked() {
                                delete_actions.push((tunnel.tab_idx, tunnel.session_idx, tunnel.forward_idx));
                            }
                        });
                        ui.add_space(2.0);
                    }

                    // Apply stop/start actions
                    for (tab_idx, session_idx, forward_idx) in stop_actions {
                        if let Some(tab) = self.tabs.get_mut(tab_idx) {
                            if let Some(session) = tab.sessions.get_mut(session_idx) {
                                if let Some(SessionBackend::Ssh(ssh)) = &session.session {
                                    let states = ssh.get_port_forward_states();
                                    if let Some((_, state)) = states.get(forward_idx) {
                                        match state {
                                            ForwardState::Active | ForwardState::Starting => {
                                                ssh.stop_port_forward(forward_idx);
                                            }
                                            ForwardState::Stopped | ForwardState::Error(_) => {
                                                // Restart: get config and start again
                                                let config = states[forward_idx].0.clone();
                                                ssh.start_port_forward(config);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Apply delete actions (stop then remove from the vec)
                    // Process in reverse order to keep indices valid
                    let mut sorted_deletes = delete_actions;
                    sorted_deletes.sort_by(|a, b| b.2.cmp(&a.2));
                    for (tab_idx, session_idx, forward_idx) in sorted_deletes {
                        if let Some(tab) = self.tabs.get_mut(tab_idx) {
                            if let Some(session) = tab.sessions.get_mut(session_idx) {
                                if let Some(SessionBackend::Ssh(ssh)) = &session.session {
                                    ssh.stop_port_forward(forward_idx);
                                    if let Ok(mut fwds) = ssh.port_forwards.lock() {
                                        if forward_idx < fwds.len() {
                                            fwds.remove(forward_idx);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            });

        // ── Add Tunnel dialog ──
        if self.add_tunnel_dialog.open {
            self.show_add_tunnel_dialog(ctx);
        }
    }

    /// Collect tunnel information from all SSH sessions across all tabs
    fn collect_tunnel_info(&self) -> Vec<TunnelInfo> {
        let mut tunnels = Vec::new();
        for (tab_idx, tab) in self.tabs.iter().enumerate() {
            for (session_idx, session) in tab.sessions.iter().enumerate() {
                if let Some(SessionBackend::Ssh(ssh)) = &session.session {
                    let host_name = session
                        .ssh_host
                        .as_ref()
                        .map(|h| h.name.clone())
                        .unwrap_or_else(|| "SSH".to_string());

                    let states = ssh.get_port_forward_states();
                    for (forward_idx, (config, state)) in states.iter().enumerate() {
                        tunnels.push(TunnelInfo {
                            kind: config.kind.clone(),
                            local_host: config.local_host.clone(),
                            local_port: config.local_port,
                            remote_host: config.remote_host.clone(),
                            remote_port: config.remote_port,
                            host_name: host_name.clone(),
                            state: state.clone(),
                            tab_idx,
                            session_idx,
                            forward_idx,
                        });
                    }
                }
            }
        }
        tunnels
    }

    /// Show the "Add Tunnel" dialog
    fn show_add_tunnel_dialog(&mut self, ctx: &egui::Context) {
        let theme = self.theme.clone();
        let lang = self.language;

        // Collect SSH session info before the closure to avoid borrow issues
        let ssh_sessions: Vec<(usize, usize, String)> = self
            .tabs
            .iter()
            .enumerate()
            .flat_map(|(tab_idx, tab)| {
                tab.sessions
                    .iter()
                    .enumerate()
                    .filter_map(move |(sess_idx, session)| {
                        if let Some(SessionBackend::Ssh(ssh)) = &session.session {
                            if ssh.connection_state().is_connected() {
                                let name = session
                                    .ssh_host
                                    .as_ref()
                                    .map(|h| h.name.clone())
                                    .unwrap_or_else(|| format!("SSH #{}", sess_idx));
                                return Some((tab_idx, sess_idx, name));
                            }
                        }
                        None
                    })
            })
            .collect();

        let mut should_close = false;
        let mut create_forward: Option<(usize, usize, PortForwardConfig)> = None;

        egui::Window::new(lang.t("add_tunnel"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .min_size([280.0, 0.0])
            .default_size([DIALOG_WIDTH_LG, 320.0])
            .frame(widgets::dialog_frame(&theme))
            .show(ctx, |ui| {
                ui.add_space(4.0);

                // Session selector
                ui.label(widgets::field_label(lang.t("select_session"), &theme));
                ui.add_space(4.0);

                let current_label = if let (Some(tab_idx), Some(sess_idx)) =
                    (self.add_tunnel_dialog.selected_tab_idx, self.add_tunnel_dialog.selected_session_idx)
                {
                    ssh_sessions
                        .iter()
                        .find(|(t, s, _)| *t == tab_idx && *s == sess_idx)
                        .map(|(_, _, name)| name.as_str())
                        .unwrap_or("--")
                } else {
                    "--"
                };

                egui::ComboBox::from_id_salt("tunnel_session_select")
                    .selected_text(current_label)
                    .width(320.0)
                    .show_ui(ui, |ui| {
                        for (tab_idx, sess_idx, name) in &ssh_sessions {
                            let is_selected = self.add_tunnel_dialog.selected_tab_idx == Some(*tab_idx)
                                && self.add_tunnel_dialog.selected_session_idx == Some(*sess_idx);
                            if ui.selectable_label(is_selected, name).clicked() {
                                self.add_tunnel_dialog.selected_tab_idx = Some(*tab_idx);
                                self.add_tunnel_dialog.selected_session_idx = Some(*sess_idx);
                            }
                        }
                    });
                ui.add_space(12.0);

                // Forward type toggle
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.add_tunnel_dialog.forward_kind,
                        ForwardKind::Local,
                        egui::RichText::new("Local (L)").size(12.0),
                    );
                    ui.selectable_value(
                        &mut self.add_tunnel_dialog.forward_kind,
                        ForwardKind::Remote,
                        egui::RichText::new("Remote (R)").size(12.0),
                    );
                });
                ui.add_space(12.0);

                // Local host:port
                ui.horizontal(|ui| {
                    ui.label(widgets::field_label("Local Host", &theme));
                    ui.add_space(40.0);
                    ui.label(widgets::field_label("Local Port", &theme));
                });
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.add_tunnel_dialog.local_host)
                            .desired_width(180.0)
                            .font(egui::FontId::monospace(12.0)),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.add_tunnel_dialog.local_port)
                            .desired_width(80.0)
                            .font(egui::FontId::monospace(12.0)),
                    );
                });
                ui.add_space(8.0);

                // Remote host:port
                ui.horizontal(|ui| {
                    ui.label(widgets::field_label("Remote Host", &theme));
                    ui.add_space(32.0);
                    ui.label(widgets::field_label("Remote Port", &theme));
                });
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.add_tunnel_dialog.remote_host)
                            .desired_width(180.0)
                            .font(egui::FontId::monospace(12.0)),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.add_tunnel_dialog.remote_port)
                            .desired_width(80.0)
                            .font(egui::FontId::monospace(12.0)),
                    );
                });
                ui.add_space(8.0);

                // Error message
                if !self.add_tunnel_dialog.error.is_empty() {
                    ui.label(
                        egui::RichText::new(&self.add_tunnel_dialog.error)
                            .color(theme.red)
                            .size(11.0),
                    );
                    ui.add_space(4.0);
                }

                ui.add_space(8.0);

                // Buttons
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(widgets::secondary_button(lang.t("cancel"), &theme)).clicked()
                        {
                            should_close = true;
                        }

                        ui.add_space(8.0);

                        if ui.add(widgets::primary_button(lang.t("create"), &theme)).clicked()
                        {
                            // Validate
                            if self.add_tunnel_dialog.selected_tab_idx.is_none()
                                || self.add_tunnel_dialog.selected_session_idx.is_none()
                            {
                                self.add_tunnel_dialog.error =
                                    lang.t("select_session").to_string();
                            } else if self.add_tunnel_dialog.local_port.is_empty()
                                || self.add_tunnel_dialog.remote_port.is_empty()
                            {
                                self.add_tunnel_dialog.error = "Port is required.".to_string();
                            } else {
                                match (
                                    self.add_tunnel_dialog.local_port.parse::<u16>(),
                                    self.add_tunnel_dialog.remote_port.parse::<u16>(),
                                ) {
                                    (Ok(lp), Ok(rp)) => {
                                        let config = PortForwardConfig {
                                            kind: self.add_tunnel_dialog.forward_kind.clone(),
                                            local_host: self
                                                .add_tunnel_dialog
                                                .local_host
                                                .clone(),
                                            local_port: lp,
                                            remote_host: self
                                                .add_tunnel_dialog
                                                .remote_host
                                                .clone(),
                                            remote_port: rp,
                                        };
                                        create_forward = Some((
                                            self.add_tunnel_dialog.selected_tab_idx.unwrap(),
                                            self.add_tunnel_dialog.selected_session_idx.unwrap(),
                                            config,
                                        ));
                                        should_close = true;
                                    }
                                    _ => {
                                        self.add_tunnel_dialog.error =
                                            "Invalid port number.".to_string();
                                    }
                                }
                            }
                        }
                    });
                });
            });

        if should_close {
            self.add_tunnel_dialog.open = false;
        }

        // Send the start command to the SSH session
        if let Some((tab_idx, session_idx, config)) = create_forward {
            if let Some(tab) = self.tabs.get_mut(tab_idx) {
                if let Some(session) = tab.sessions.get_mut(session_idx) {
                    if let Some(SessionBackend::Ssh(ssh)) = &session.session {
                        ssh.start_port_forward(config);
                    }
                }
            }
        }
    }
}

/// Helper trait to check connected state
trait SshConnectionStateExt {
    fn is_connected(&self) -> bool;
}

impl SshConnectionStateExt for crate::ssh::SshConnectionState {
    fn is_connected(&self) -> bool {
        matches!(self, crate::ssh::SshConnectionState::Connected)
    }
}
