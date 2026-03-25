//! Portal - Modern Terminal Emulator with egui
//! Termius-inspired UI with native terminal input

#![allow(unexpected_cfgs)]  // Suppress warnings from objc crate's macros

mod app;
mod config;
mod sftp;
mod ssh;
mod terminal;
mod ui;

use app::PortalApp;
use std::time::Duration;

use ui::*;
use ssh::SshConnectionState;

impl eframe::App for PortalApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(16));

        // Update window title based on current view and active tab
        let title = match self.current_view {
            AppView::Terminal => {
                if let Some(tab) = self.tabs.get(self.active_tab) {
                    tab.title.clone()
                } else {
                    "Terminal".to_string()
                }
            }
            AppView::Hosts => self.language.t("hosts").to_string(),
            AppView::Sftp => self.language.t("sftp").to_string(),
            AppView::Keychain => self.language.t("keychain").to_string(),
            AppView::Settings => self.language.t("settings").to_string(),
            AppView::Snippets => "Snippets".to_string(),
            AppView::Tunnels => "Tunnels".to_string(),
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        // Apply visuals/fonts if dirty
        if self.visuals_dirty {
            self.apply_visuals(ctx);
            self.visuals_dirty = false;
        }
        if self.fonts_dirty {
            self.apply_fonts(ctx);
        }

        // ── Main window close handling ──
        // If the user closes the main window but detached windows still exist,
        // hide the main window instead of exiting. Exit only when all windows are gone.
        if ctx.input(|i| i.viewport().close_requested()) {
            if !self.detached_windows.is_empty() {
                // Cancel the close and hide the main window
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.main_window_hidden = true;
            } else {
                // No detached windows - clean up and exit
                // Clean up all sessions to prevent PTY leaks
                self.cleanup_sessions();
                // Let the default close proceed (app exits)
            }
        }

        // ── Render detached tab windows (full UI) ─────────────────────────
        // Collect tabs to detach from detached windows (deferred to avoid borrow conflicts)
        let mut pending_detach: Vec<(usize, usize)> = Vec::new(); // (window_index, tab_index)
        for i in 0..self.detached_windows.len() {
            let viewport_id = self.detached_windows[i].viewport_id;
            let builder = egui::ViewportBuilder::default()
                .with_title("Portal") // Initial title
                .with_inner_size([1200.0, 800.0])
                .with_min_inner_size([600.0, 400.0]);
            ctx.show_viewport_immediate(viewport_id, builder, |ctx, _class| {
                // Update window title based on current view and active tab
                let dw = &self.detached_windows[i];
                let title = match dw.current_view {
                    AppView::Terminal => {
                        if let Some(tab) = dw.tabs.get(dw.active_tab) {
                            tab.title.clone()
                        } else {
                            "Terminal".to_string()
                        }
                    }
                    AppView::Hosts => self.language.t("hosts").to_string(),
                    AppView::Sftp => self.language.t("sftp").to_string(),
                    AppView::Keychain => self.language.t("keychain").to_string(),
                    AppView::Settings => self.language.t("settings").to_string(),
                    AppView::Snippets => "Snippets".to_string(),
                    AppView::Tunnels => "Tunnels".to_string(),
                };
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

                ctx.request_repaint_after(Duration::from_millis(16));

                if ctx.input(|i| i.viewport().close_requested()) {
                    self.detached_windows[i].close_requested = true;
                }

                let dw = &mut self.detached_windows[i];

                // ── Keyboard shortcuts ──
                if dw.current_view == AppView::Terminal {
                    if ctx.input(|i| i.key_pressed(egui::Key::D) && i.modifiers.command && !i.modifiers.shift) {
                        // Split horizontal in detached window
                        let active = dw.active_tab;
                        let tab = &dw.tabs[active];
                        let old_idx = tab.focused_session;
                        let ssh_host = tab.sessions.get(old_idx).and_then(|s| s.ssh_host.clone());
                        let resolved_auth = tab.sessions.get(old_idx).and_then(|s| s.resolved_auth.clone());
                        let new_session = if let Some(host) = &ssh_host {
                            let auth = resolved_auth.unwrap_or(config::resolve_auth(host, &self.credentials));
                            TerminalSession::new_ssh(host, auth, &self.runtime, None)
                        } else {
                            let id = dw.next_id;
                            dw.next_id += 1;
                            TerminalSession::new_local(id, &self.selected_shell)
                        };
                        let tab = &mut dw.tabs[active];
                        tab.sessions.push(new_session);
                        let new_idx = tab.sessions.len() - 1;
                        tab.layout.replace(old_idx, PaneNode::Split {
                            direction: SplitDirection::Horizontal,
                            ratio: 0.5,
                            first: Box::new(PaneNode::Terminal(old_idx)),
                            second: Box::new(PaneNode::Terminal(new_idx)),
                        });
                        tab.focused_session = new_idx;
                    }
                    if ctx.input(|i| i.key_pressed(egui::Key::D) && i.modifiers.command && i.modifiers.shift) {
                        // Split vertical in detached window
                        let active = dw.active_tab;
                        let tab = &dw.tabs[active];
                        let old_idx = tab.focused_session;
                        let ssh_host = tab.sessions.get(old_idx).and_then(|s| s.ssh_host.clone());
                        let resolved_auth = tab.sessions.get(old_idx).and_then(|s| s.resolved_auth.clone());
                        let new_session = if let Some(host) = &ssh_host {
                            let auth = resolved_auth.unwrap_or(config::resolve_auth(host, &self.credentials));
                            TerminalSession::new_ssh(host, auth, &self.runtime, None)
                        } else {
                            let id = dw.next_id;
                            dw.next_id += 1;
                            TerminalSession::new_local(id, &self.selected_shell)
                        };
                        let tab = &mut dw.tabs[active];
                        tab.sessions.push(new_session);
                        let new_idx = tab.sessions.len() - 1;
                        tab.layout.replace(old_idx, PaneNode::Split {
                            direction: SplitDirection::Vertical,
                            ratio: 0.5,
                            first: Box::new(PaneNode::Terminal(old_idx)),
                            second: Box::new(PaneNode::Terminal(new_idx)),
                        });
                        tab.focused_session = new_idx;
                    }
                }

                // ── Sidebar ──
                let nav_width = (ctx.screen_rect().width() * 0.14).min(200.0).max(150.0);
                egui::SidePanel::left(egui::Id::new("detached_nav").with(i))
                    .exact_width(nav_width)
                    .resizable(false)
                    .frame(egui::Frame {
                        fill: self.theme.bg_secondary,
                        inner_margin: egui::Margin::same(0.0),
                        stroke: egui::Stroke::NONE,
                        ..Default::default()
                    })
                    .show(ctx, |ui| {
                        ui.add_space(32.0);
                        let dw = &mut self.detached_windows[i];
                        if nav_button(ui, "☰", self.language.t("hosts"), dw.current_view == AppView::Hosts, &self.theme) {
                            dw.current_view = AppView::Hosts;
                        }
                        if nav_button(ui, ">_", self.language.t("terminal"), dw.current_view == AppView::Terminal, &self.theme) {
                            dw.current_view = AppView::Terminal;
                        }
                        if nav_button(ui, "\u{2195}", self.language.t("sftp"), dw.current_view == AppView::Sftp, &self.theme) {
                            dw.current_view = AppView::Sftp;
                        }
                        if nav_button(ui, "\u{1f511}", self.language.t("keychain"), dw.current_view == AppView::Keychain, &self.theme) {
                            dw.current_view = AppView::Keychain;
                        }

                        // Settings button at bottom - fill remaining space to reach window bottom
                        let available_size = ui.available_size();
                        egui::Frame::none()
                            .fill(self.theme.bg_secondary)
                            .show(ui, |ui| {
                                ui.allocate_ui_with_layout(
                                    available_size,
                                    egui::Layout::bottom_up(egui::Align::LEFT),
                                    |ui| {
                                        ui.add_space(8.0);
                                        if nav_button(ui, "\u{2699}", self.language.t("settings"), dw.current_view == AppView::Settings, &self.theme) {
                                            dw.current_view = AppView::Settings;
                                        }
                                    },
                                );
                            });
                    });

                let dw = &mut self.detached_windows[i];

                // ── Tab Bar (terminal view) ──
                if dw.current_view == AppView::Terminal {
                    use crate::ui::views::tab_view::{detached_tab_bar, TabBarAction};

                    egui::TopBottomPanel::top(egui::Id::new("detached_tab_bar").with(i))
                        .frame(egui::Frame {
                            fill: self.theme.bg_secondary,
                            inner_margin: egui::Margin::symmetric(8.0, 4.0),
                            stroke: egui::Stroke::NONE,
                            ..Default::default()
                        })
                        .show(ctx, |ui| {
                            let more_menu_id = egui::Id::new("dw_tab_bar_more_menu").with(i);
                            let mut show_more_menu = ctx.data_mut(|d| *d.get_temp_mut_or_default::<bool>(more_menu_id));

                            let action = detached_tab_bar(
                                ui, ctx, &dw.tabs, dw.active_tab, &mut dw.tab_drag,
                                &self.theme, &self.language, &mut show_more_menu, i
                            );

                            // Store menu state for next frame
                            ctx.data_mut(|d| d.insert_temp(more_menu_id, show_more_menu));

                            // Handle tab bar actions
                            match action {
                                TabBarAction::ActivateTab(ti) => {
                                    dw.active_tab = ti;
                                }
                                TabBarAction::CloseTab(ti) => {
                                    if dw.tabs.len() > 1 {
                                        dw.tabs.remove(ti);
                                        if dw.active_tab >= dw.tabs.len() {
                                            dw.active_tab = dw.tabs.len() - 1;
                                        } else if dw.active_tab > ti {
                                            dw.active_tab -= 1;
                                        }
                                    }
                                }
                                TabBarAction::DetachTab(ti) => {
                                    if dw.tabs.len() > 1 {
                                        pending_detach.push((i, ti));
                                    }
                                }
                                TabBarAction::MergeTabs { src, dst } => {
                                    let mut src_tab = dw.tabs.remove(src);
                                    let dst = if src < dst { dst - 1 } else { dst };
                                    let dst_tab = &mut dw.tabs[dst];
                                    let offset = dst_tab.sessions.len();
                                    src_tab.layout.offset_indices(offset);
                                    dst_tab.sessions.extend(src_tab.sessions);
                                    let old_layout = std::mem::replace(&mut dst_tab.layout, PaneNode::Terminal(0));
                                    dst_tab.layout = PaneNode::Split {
                                        direction: SplitDirection::Horizontal,
                                        ratio: 0.5,
                                        first: Box::new(old_layout),
                                        second: Box::new(src_tab.layout),
                                    };
                                    if dw.active_tab == src {
                                        dw.active_tab = dst;
                                    } else if dw.active_tab > src && dw.active_tab > 0 {
                                        dw.active_tab -= 1;
                                    }
                                    if dw.active_tab >= dw.tabs.len() {
                                        dw.active_tab = dw.tabs.len().saturating_sub(1);
                                    }
                                }
                                TabBarAction::ReorderTab { src, dst, insert_before } => {
                                    let src_tab = dw.tabs.remove(src);
                                    let insert_idx = if insert_before {
                                        if src < dst { dst.saturating_sub(1) } else { dst }
                                    } else {
                                        if src < dst { dst } else { (dst + 1).min(dw.tabs.len()) }
                                    };
                                    dw.tabs.insert(insert_idx, src_tab);
                                    dw.active_tab = insert_idx;
                                }
                                TabBarAction::NewTab => {
                                    let id = dw.next_id;
                                    dw.next_id += 1;
                                    let new_tab = Tab {
                                        title: format!("Terminal {}", id),
                                        sessions: vec![TerminalSession::new_local(id, &self.selected_shell)],
                                        layout: PaneNode::Terminal(0),
                                        focused_session: 0,
                                        broadcast_enabled: false,
                                    };
                                    dw.tabs.push(new_tab);
                                    dw.active_tab = dw.tabs.len() - 1;
                                }
                                TabBarAction::ToggleBroadcast(ti) => {
                                    if ti < dw.tabs.len() {
                                        dw.tabs[ti].broadcast_enabled = !dw.tabs[ti].broadcast_enabled;
                                    }
                                    ctx.data_mut(|d| d.insert_temp(more_menu_id, false));
                                }
                                TabBarAction::OpenSnippets => {
                                    // Snippets drawer is part of main window, no-op in detached windows
                                    ctx.data_mut(|d| d.insert_temp(more_menu_id, false));
                                }
                                TabBarAction::None | TabBarAction::ReconnectTab(_) => {}
                            }
                        });
                }

                // ── Status Bar (terminal view) ──
                if dw.current_view == AppView::Terminal {
                    let conn_type = dw.tabs.get(dw.active_tab)
                        .and_then(|tab| tab.sessions.get(tab.focused_session))
                        .map(|s| match &s.session {
                            Some(SessionBackend::Ssh(ssh)) => {
                                let host = s.ssh_host.as_ref()
                                    .map(|h| format!("{}@{}:{}", h.username, h.host, h.port))
                                    .unwrap_or_else(|| "SSH".to_string());
                                match ssh.connection_state() {
                                    SshConnectionState::Connected => format!("SSH  {}", host),
                                    SshConnectionState::Connecting => format!("SSH  {} (connecting…)", host),
                                    SshConnectionState::Authenticating => format!("SSH  {} (authenticating…)", host),
                                    SshConnectionState::Disconnected(_) => format!("SSH  {} (disconnected)", host),
                                    SshConnectionState::Error(_) => format!("SSH  {}", host),
                                }
                            }
                            _ => "Local".to_string(),
                        })
                        .unwrap_or_else(|| "Local".to_string());

                    let is_local_session = conn_type == "Local";
                    let shell_label = dw.tabs.get(dw.active_tab)
                        .and_then(|tab| tab.sessions.get(tab.focused_session))
                        .map(|s| s.shell_name())
                        .unwrap_or_else(|| "—".to_string());
                    let encoding_label = self.selected_encoding.clone();
                    let uptime_label = dw.tabs.get(dw.active_tab)
                        .and_then(|tab| tab.sessions.get(tab.focused_session))
                        .map(|s| {
                            let elapsed = s.created_at.elapsed().as_secs();
                            let hours = elapsed / 3600;
                            let minutes = (elapsed % 3600) / 60;
                            let seconds = elapsed % 60;
                            if hours > 0 { format!("{:02}:{:02}:{:02}", hours, minutes, seconds) }
                            else { format!("{:02}:{:02}", minutes, seconds) }
                        })
                        .unwrap_or_default();

                    let sep_color = self.theme.border;
                    let conn_color = if conn_type == "Local" { self.theme.green } else { self.theme.accent };

                    egui::TopBottomPanel::bottom(egui::Id::new("detached_status_bar").with(i))
                        .exact_height(STATUS_BAR_HEIGHT)
                        .frame(egui::Frame {
                            fill: self.theme.bg_secondary,
                            inner_margin: egui::Margin::symmetric(12.0, 0.0),
                            outer_margin: egui::Margin::symmetric(0.0, 0.0),
                            stroke: egui::Stroke::NONE,
                            ..Default::default()
                        })
                        .show(ctx, |ui| {
                            ui.horizontal_centered(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                let status_btn = |ui: &mut egui::Ui, text: &str, color: egui::Color32| {
                                    ui.add(egui::Button::new(
                                        egui::RichText::new(text).color(color).size(12.0)
                                    ).frame(false).rounding(0.0).min_size(egui::vec2(0.0, 24.0)))
                                };
                                status_btn(ui, &conn_type, conn_color);
                                // Broadcast indicator
                                if dw.tabs[dw.active_tab].broadcast_enabled {
                                    ui.add_space(12.0);
                                    ui.label(egui::RichText::new("|").color(sep_color).size(12.0));
                                    ui.add_space(12.0);
                                    ui.label(egui::RichText::new(self.language.t("broadcast")).color(self.theme.accent).size(12.0));
                                }
                                ui.add_space(12.0);
                                ui.label(egui::RichText::new("|").color(sep_color).size(12.0));
                                ui.add_space(12.0);
                                // Shell display (non-interactive)
                                ui.label(egui::RichText::new(&shell_label)
                                    .color(if is_local_session { self.theme.fg_primary } else { self.theme.fg_dim })
                                    .size(12.0));
                                ui.add_space(12.0);
                                ui.label(egui::RichText::new("|").color(sep_color).size(12.0));
                                ui.add_space(12.0);
                                // Encoding display (non-interactive)
                                ui.label(egui::RichText::new(&encoding_label)
                                    .color(self.theme.fg_dim)
                                    .size(12.0));
                                if !uptime_label.is_empty() {
                                    ui.add_space(12.0);
                                    ui.label(egui::RichText::new("|").color(sep_color).size(12.0));
                                    ui.add_space(12.0);
                                    ui.label(egui::RichText::new(&uptime_label).color(self.theme.fg_dim).size(12.0));
                                }
                            });
                        });
                }

                // ── Add/Edit Host Drawer & Delete Dialog (Hosts view, before CentralPanel) ──
                let dw_view = self.detached_windows[i].current_view;
                if dw_view == AppView::Hosts && self.add_host_dialog.open {
                    self.show_add_host_drawer(ctx);
                }

                // ── Central Panel ──
                let dw_view = self.detached_windows[i].current_view;
                egui::CentralPanel::default()
                    .frame(egui::Frame {
                        fill: self.theme.bg_primary,
                        inner_margin: egui::Margin::same(0.0),
                        outer_margin: egui::Margin::same(0.0),
                        ..Default::default()
                    })
                    .show(ctx, |ui| {
                        match dw_view {
                            AppView::Terminal => {
                                let dw = &mut self.detached_windows[i];
                                ui.style_mut().spacing.item_spacing = egui::vec2(0.0, 0.0);
                                let available = ui.available_rect_before_wrap();
                                let active = dw.active_tab;
                                let focused = dw.tabs[active].focused_session;
                                let can_close = dw.tabs.len() > 1 || dw.tabs[active].sessions.len() > 1;
                                let pane_result = {
                                    let tab = &mut dw.tabs[active];
                                    // Create a temporary broadcast state from the tab's broadcast_enabled flag
                                    let temp_broadcast = BroadcastState {
                                        enabled: tab.broadcast_enabled,
                                    };
                                    render_pane_tree(
                                        ui, ctx,
                                        &mut tab.layout,
                                        available,
                                        &mut tab.sessions,
                                        focused,
                                        &temp_broadcast,
                                        &mut dw.ime_composing,
                                        &mut dw.ime_preedit,
                                        can_close,
                                        &self.theme,
                                        self.font_size,
                                        &self.language,
                                        &self.shortcut_resolver,
                                    )
                                };
                                if let Some((idx, action, input_bytes)) = pane_result {
                                    dw.tabs[active].focused_session = idx;
                                    // Broadcast input to all sessions in current tab
                                    if dw.tabs[active].broadcast_enabled && !input_bytes.is_empty() {
                                        for (sess_idx, session) in dw.tabs[active].sessions.iter_mut().enumerate() {
                                            // Skip focused session (already handled)
                                            if sess_idx == idx {
                                                continue;
                                            }
                                            if let Some(ref mut backend) = session.session {
                                                if backend.is_connected() {
                                                    let _ = backend.write(&input_bytes);
                                                }
                                            }
                                        }
                                    }
                                    match action {
                                        PaneAction::Focus => {}
                                        PaneAction::SplitHorizontal => {
                                            let old_idx = idx;
                                            let ssh_host = dw.tabs[active].sessions.get(old_idx).and_then(|s| s.ssh_host.clone());
                                            let resolved_auth = dw.tabs[active].sessions.get(old_idx).and_then(|s| s.resolved_auth.clone());
                                            let new_session = if let Some(host) = &ssh_host {
                                                let auth = resolved_auth.unwrap_or(config::resolve_auth(host, &self.credentials));
                                                TerminalSession::new_ssh(host, auth, &self.runtime, None)
                                            } else {
                                                let id = dw.next_id;
                                                dw.next_id += 1;
                                                TerminalSession::new_local(id, &self.selected_shell)
                                            };
                                            let tab = &mut dw.tabs[active];
                                            tab.sessions.push(new_session);
                                            let new_idx = tab.sessions.len() - 1;
                                            tab.layout.replace(old_idx, PaneNode::Split {
                                                direction: SplitDirection::Horizontal, ratio: 0.5,
                                                first: Box::new(PaneNode::Terminal(old_idx)),
                                                second: Box::new(PaneNode::Terminal(new_idx)),
                                            });
                                            tab.focused_session = new_idx;
                                        }
                                        PaneAction::SplitVertical => {
                                            let old_idx = idx;
                                            let ssh_host = dw.tabs[active].sessions.get(old_idx).and_then(|s| s.ssh_host.clone());
                                            let resolved_auth = dw.tabs[active].sessions.get(old_idx).and_then(|s| s.resolved_auth.clone());
                                            let new_session = if let Some(host) = &ssh_host {
                                                let auth = resolved_auth.unwrap_or(config::resolve_auth(host, &self.credentials));
                                                TerminalSession::new_ssh(host, auth, &self.runtime, None)
                                            } else {
                                                let id = dw.next_id;
                                                dw.next_id += 1;
                                                TerminalSession::new_local(id, &self.selected_shell)
                                            };
                                            let tab = &mut dw.tabs[active];
                                            tab.sessions.push(new_session);
                                            let new_idx = tab.sessions.len() - 1;
                                            tab.layout.replace(old_idx, PaneNode::Split {
                                                direction: SplitDirection::Vertical, ratio: 0.5,
                                                first: Box::new(PaneNode::Terminal(old_idx)),
                                                second: Box::new(PaneNode::Terminal(new_idx)),
                                            });
                                            tab.focused_session = new_idx;
                                        }
                                        PaneAction::ClosePane => {
                                            let tab = &mut dw.tabs[active];
                                            if tab.sessions.len() <= 1 {
                                                if dw.tabs.len() > 1 {
                                                    dw.tabs.remove(active);
                                                    if dw.active_tab >= dw.tabs.len() {
                                                        dw.active_tab = dw.tabs.len().saturating_sub(1);
                                                    }
                                                }
                                            } else {
                                                let old_layout = tab.layout.clone();
                                                if let Some(new_layout) = old_layout.remove(idx) {
                                                    tab.layout = new_layout;
                                                }
                                                tab.layout.decrement_indices_above(idx);
                                                tab.sessions.remove(idx);
                                                if tab.focused_session >= tab.sessions.len() {
                                                    tab.focused_session = tab.sessions.len().saturating_sub(1);
                                                } else if tab.focused_session == idx && idx > 0 {
                                                    tab.focused_session = idx - 1;
                                                }
                                            }
                                        }
                                        PaneAction::ToggleBroadcast => {
                                            dw.tabs[active].broadcast_enabled = !dw.tabs[active].broadcast_enabled;
                                        }
                                        PaneAction::RemoveHostKey => {
                                            // Remove old SSH host key and reconnect
                                            if let Some(host) = dw.tabs[active].sessions.get(idx).and_then(|s| s.ssh_host.clone()) {
                                                let _ = crate::ssh::remove_known_hosts_key(&host.host, host.port);
                                                // Reconnect the SSH session
                                                dw.tabs[active].sessions[idx].reconnect_ssh(&self.runtime, None);
                                            }
                                        }
                                    }
                                }
                            }
                            AppView::Hosts => {
                                self.show_hosts_page(ctx, ui);
                            }
                            AppView::Sftp => {
                                self.show_sftp_view(ui);
                            }
                            AppView::Keychain => {
                                self.show_keychain_view(ctx, ui);
                            }
                            AppView::Settings => {
                                self.show_settings_view(ctx, ui);
                            }
                            AppView::Snippets => {
                                self.show_snippets_page(ctx, ui);
                            }
                            AppView::Tunnels => {
                                self.show_tunnels_page(ctx, ui);
                            }
                        }
                    });
            });
        }
        self.detached_windows.retain(|w| !w.close_requested);

        // Process deferred tab detaches from detached windows
        // Process in reverse order so indices stay valid
        for &(win_idx, tab_idx) in pending_detach.iter().rev() {
            if win_idx < self.detached_windows.len() {
                let dw = &mut self.detached_windows[win_idx];
                if dw.tabs.len() > 1 && tab_idx < dw.tabs.len() {
                    let tab = dw.tabs.remove(tab_idx);
                    if dw.active_tab >= dw.tabs.len() {
                        dw.active_tab = dw.tabs.len().saturating_sub(1);
                    }
                    let id_val = self.next_viewport_id;
                    self.next_viewport_id += 1;
                    let viewport_id = egui::ViewportId::from_hash_of(format!("detached_{}", id_val));
                    let next_id = dw.next_id;
                    dw.next_id += 100;
                    self.detached_windows.push(DetachedWindow {
                        viewport_id,
                        title: tab.title.clone(),
                        tabs: vec![tab],
                        active_tab: 0,
                        current_view: AppView::Terminal,
                        close_requested: false,
                        ime_composing: false,
                        ime_preedit: String::new(),
                        next_id,
                        tab_drag: TabDragState::default(),
                        broadcast_state: BroadcastState::default(),
                    });
                }
            }
        }

        // If main window is hidden and all detached windows are closed, exit the app
        if self.main_window_hidden && self.detached_windows.is_empty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // If main window is hidden, skip rendering the rest (only detached windows are active)
        if self.main_window_hidden {
            return;
        }

        // ── Split keyboard shortcuts (terminal view only) ─────────────────────
        // Cmd+D  → split horizontally (left | right)
        // Cmd+Shift+D → split vertically (top / bottom)
        if self.current_view == AppView::Terminal {
            if ctx.input(|i| i.key_pressed(egui::Key::D) && i.modifiers.command && !i.modifiers.shift) {
                self.split_focused_pane(SplitDirection::Horizontal);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::D) && i.modifiers.command && i.modifiers.shift) {
                self.split_focused_pane(SplitDirection::Vertical);
            }
            // Cmd+Shift+S → open snippet quick selector
            if ctx.input(|i| i.key_pressed(egui::Key::S) && i.modifiers.command && i.modifiers.shift) {
                self.snippet_view_state.quick_selector_open = true;
                self.snippet_view_state.selected_snippet_index = if !self.snippets.is_empty() { Some(0) } else { None };
            }
        }

        // ── Nav panel (narrow, always shown first to get full height) ──
        self.show_nav_panel(ctx);

        // ── Tab Bar and Status Bar (only in terminal view) ─────────────────────
        // Collect terminal info for status bar (used across terminal view)
        let (conn_type, shell_label, encoding_label, uptime_label) = if self.current_view == AppView::Terminal {
            let conn_type = self.tabs.get(self.active_tab)
                .and_then(|tab| tab.sessions.get(tab.focused_session))
                .map(|s| match &s.session {
                    Some(SessionBackend::Ssh(ssh)) => {
                        let host = s.ssh_host.as_ref()
                            .map(|h| format!("{}@{}:{}", h.username, h.host, h.port))
                            .unwrap_or_else(|| "SSH".to_string());
                        match ssh.connection_state() {
                            SshConnectionState::Connected => format!("SSH  {}", host),
                            SshConnectionState::Connecting => format!("SSH  {} (connecting…)", host),
                            SshConnectionState::Authenticating => format!("SSH  {} (authenticating…)", host),
                            SshConnectionState::Disconnected(_) => format!("SSH  {} (disconnected)", host),
                            SshConnectionState::Error(_) => format!("SSH  {}", host),
                        }
                    }
                    _ => "Local".to_string(),
                })
                .unwrap_or_else(|| "Local".to_string());

            let shell_label = self.tabs.get(self.active_tab)
                .and_then(|tab| tab.sessions.get(tab.focused_session))
                .map(|s| s.shell_name())
                .unwrap_or_else(|| "—".to_string());
            let encoding_label = self.selected_encoding.clone();

            let uptime_label = self.tabs.get(self.active_tab)
                .and_then(|tab| tab.sessions.get(tab.focused_session))
                .map(|s| {
                    let elapsed = s.created_at.elapsed().as_secs();
                    let hours = elapsed / 3600;
                    let minutes = (elapsed % 3600) / 60;
                    let seconds = elapsed % 60;
                    if hours > 0 {
                        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
                    } else {
                        format!("{:02}:{:02}", minutes, seconds)
                    }
                })
                .unwrap_or_default();

            (Some(conn_type), shell_label, encoding_label, uptime_label)
        } else {
            (None, String::new(), String::new(), String::new())
        };

        // ── Add/Edit Host Drawer (right panel, Hosts view only, before CentralPanel) ──
        if self.current_view == AppView::Hosts && self.add_host_dialog.open {
            self.show_add_host_drawer(ctx);
        }

        // ── Delete Confirmation Dialog ──────────────────
        if let Some(idx) = self.confirm_delete_host {
            let host_name = self.hosts.get(idx).map(|h| h.name.clone()).unwrap_or_default();
            let mut open = true;
            egui::Window::new(self.language.t("delete_host"))
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
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(self.language.t("delete_host")).size(15.0).color(self.theme.fg_primary).strong());
                    });
                    ui.add_space(10.0);

                    ui.label(
                        egui::RichText::new(self.language.tf("delete_confirm", &host_name))
                            .color(self.theme.fg_primary).size(13.0)
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(self.language.t("confirm_delete"))
                            .color(self.theme.fg_dim).size(11.0)
                    );
                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(
                                egui::Button::new(egui::RichText::new(self.language.t("delete")).color(egui::Color32::WHITE).size(13.0))
                                    .fill(self.theme.red)
                                    .rounding(6.0)
                                    .min_size(egui::vec2(70.0, 32.0))
                            ).clicked() {
                                self.host_to_delete = Some(idx);
                                self.confirm_delete_host = None;
                            }
                            if ui.add(
                                egui::Button::new(egui::RichText::new(self.language.t("cancel")).color(self.theme.fg_dim).size(13.0))
                                    .fill(self.theme.bg_elevated)
                                    .rounding(6.0)
                                    .min_size(egui::vec2(70.0, 32.0))
                            ).clicked() {
                                self.confirm_delete_host = None;
                            }
                        });
                    });
                });
            if !open {
                self.confirm_delete_host = None;
            }
        }

        // Handle deferred host deletion
        if let Some(idx) = self.host_to_delete.take() {
            if idx < self.hosts.len() && !self.hosts[idx].is_local {
                config::delete_host_credentials(&self.hosts[idx]);
                self.hosts.remove(idx);
                self.save_hosts();
            }
        }

        // ── Poll SFTP browser ──────────────────────────────────────────────
        if let Some(ref mut browser) = self.sftp_browser {
            let had_transfer = browser.transfer.is_some();
            let was_download = browser.transfer.as_ref().map_or(false, |t| !t.is_upload);
            browser.poll();
            // Auto-refresh local browser after download completes
            if had_transfer && browser.transfer.is_none() && was_download {
                self.local_browser_right.refresh();
            }
            // Handle async file read results for the editor
            if let Some((path, data)) = browser.pending_file_content.take() {
                if let Some(ref mut dialog) = self.sftp_editor_dialog {
                    if dialog.loading && dialog.panel == ui::types::SftpPanel::RightRemote {
                        match String::from_utf8(data) {
                            Ok(text) => {
                                dialog.content = text.clone();
                                dialog.original_content = text;
                                dialog.loading = false;
                                dialog.file_path = path;
                            }
                            Err(_) => {
                                dialog.error = "Not valid UTF-8".to_string();
                                dialog.loading = false;
                            }
                        }
                    }
                }
            }
            if let Some((_path, size)) = browser.pending_file_too_large.take() {
                if let Some(ref mut dialog) = self.sftp_editor_dialog {
                    if dialog.loading && dialog.panel == ui::types::SftpPanel::RightRemote {
                        dialog.loading = false;
                        dialog.error = format!("File too large ({} bytes)", size);
                    }
                }
            }
        }

        // ── Poll left SFTP browser ──────────────────────────────────────────
        if let Some(ref mut browser) = self.sftp_browser_left {
            let had_transfer = browser.transfer.is_some();
            let was_download = browser.transfer.as_ref().map_or(false, |t| !t.is_upload);
            browser.poll();
            // Auto-refresh local browser after download completes
            if had_transfer && browser.transfer.is_none() && was_download {
                self.local_browser_left.refresh();
            }
            // Handle async file read results for the editor
            if let Some((path, data)) = browser.pending_file_content.take() {
                if let Some(ref mut dialog) = self.sftp_editor_dialog {
                    if dialog.loading && dialog.panel == ui::types::SftpPanel::LeftRemote {
                        match String::from_utf8(data) {
                            Ok(text) => {
                                dialog.content = text.clone();
                                dialog.original_content = text;
                                dialog.loading = false;
                                dialog.file_path = path;
                            }
                            Err(_) => {
                                dialog.error = "Not valid UTF-8".to_string();
                                dialog.loading = false;
                            }
                        }
                    }
                }
            }
            if let Some((_path, size)) = browser.pending_file_too_large.take() {
                if let Some(ref mut dialog) = self.sftp_editor_dialog {
                    if dialog.loading && dialog.panel == ui::types::SftpPanel::LeftRemote {
                        dialog.loading = false;
                        dialog.error = format!("File too large ({} bytes)", size);
                    }
                }
            }
        }

        // ── Terminal Tab Bar (top panel, only for terminal view) ─────────────
        if self.current_view == AppView::Terminal {
            egui::TopBottomPanel::top("terminal_tab_bar")
                .frame(egui::Frame {
                    fill: self.theme.bg_secondary,
                    inner_margin: egui::Margin::symmetric(8.0, 8.0),
                    stroke: egui::Stroke::NONE,
                    ..Default::default()
                })
                .show(ctx, |ui| {
                    use crate::ui::views::tab_view::{tab_bar, TabBarAction};

                    let more_menu_id = egui::Id::new("tab_bar_more_menu");
                    let mut show_more_menu = ctx.data_mut(|d| *d.get_temp_mut_or_default::<bool>(more_menu_id));

                    let action = tab_bar(
                        ui, ctx, &self.tabs, self.active_tab, &mut self.tab_drag,
                        &self.theme, &self.language, &mut show_more_menu
                    );

                    ctx.data_mut(|d| d.insert_temp(more_menu_id, show_more_menu));

                    match action {
                        TabBarAction::ActivateTab(i) => { self.active_tab = i; }
                        TabBarAction::CloseTab(i) => {
                            if self.tabs.len() > 1 {
                                self.tabs.remove(i);
                                if self.active_tab >= self.tabs.len() {
                                    self.active_tab = self.tabs.len() - 1;
                                } else if self.active_tab > i {
                                    self.active_tab -= 1;
                                }
                            }
                        }
                        TabBarAction::ReconnectTab(i) => {
                            let si = self.tabs[i].focused_session;
                            self.tabs[i].sessions[si].reconnect_ssh(&self.runtime, None);
                        }
                        TabBarAction::DetachTab(i) => { self.detach_tab(i); }
                        TabBarAction::MergeTabs { src, dst } => {
                            let mut src_tab = self.tabs.remove(src);
                            let dst = if src < dst { dst - 1 } else { dst };
                            let dst_tab = &mut self.tabs[dst];
                            let offset = dst_tab.sessions.len();
                            src_tab.layout.offset_indices(offset);
                            dst_tab.sessions.extend(src_tab.sessions);
                            let old_layout = std::mem::replace(&mut dst_tab.layout, PaneNode::Terminal(0));
                            dst_tab.layout = PaneNode::Split {
                                direction: SplitDirection::Horizontal,
                                ratio: 0.5,
                                first: Box::new(old_layout),
                                second: Box::new(src_tab.layout),
                            };
                            if self.active_tab == src {
                                self.active_tab = dst;
                            } else if self.active_tab > src && self.active_tab > 0 {
                                self.active_tab -= 1;
                            }
                            if self.active_tab >= self.tabs.len() {
                                self.active_tab = self.tabs.len().saturating_sub(1);
                            }
                        }
                        TabBarAction::ReorderTab { src, dst, insert_before } => {
                            let src_tab = self.tabs.remove(src);
                            let insert_idx = if insert_before {
                                if src < dst { dst.saturating_sub(1) } else { dst }
                            } else {
                                if src < dst { dst } else { (dst + 1).min(self.tabs.len()) }
                            };
                            self.tabs.insert(insert_idx, src_tab);
                            self.active_tab = insert_idx;
                        }
                        TabBarAction::NewTab => { self.add_tab_local(); }
                        TabBarAction::ToggleBroadcast(i) => {
                            if let Some(tab) = self.tabs.get_mut(i) {
                                tab.broadcast_enabled = !tab.broadcast_enabled;
                            }
                            ctx.data_mut(|d| d.insert_temp(more_menu_id, false));
                        }
                        TabBarAction::OpenSnippets => {
                            self.snippet_view_state.quick_selector_open = true;
                            self.snippet_view_state.selected_snippet_index = if !self.snippets.is_empty() { Some(0) } else { None };
                            ctx.data_mut(|d| d.insert_temp(more_menu_id, false));
                        }
                        TabBarAction::None => {}
                    }
                });
        }

        // ── Terminal Status Bar (bottom panel, only for terminal view) ──────
        if self.current_view == AppView::Terminal {
            if let (Some(conn_type), _, _, _) = (&conn_type, &shell_label, &encoding_label, &uptime_label) {
                egui::TopBottomPanel::bottom("terminal_status_bar")
                    .frame(egui::Frame {
                        fill: self.theme.bg_secondary,
                        inner_margin: egui::Margin::symmetric(12.0, 8.0),
                        stroke: egui::Stroke::NONE,
                        ..Default::default()
                    })
                    .show(ctx, |ui| {
                        ui.horizontal_centered(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;

                            let sep_color = self.theme.border;
                            let is_local_session = conn_type == "Local";
                            let conn_color = if *conn_type == "Local" { self.theme.green } else { self.theme.accent };

                            let status_btn = |ui: &mut egui::Ui, text: &str, color: egui::Color32| {
                                ui.add(egui::Button::new(
                                    egui::RichText::new(text).color(color).size(11.0)
                                ).frame(false).rounding(0.0).min_size(egui::vec2(0.0, 20.0)))
                            };

                            status_btn(ui, conn_type, conn_color);

                            let is_broadcasting = self.tabs.get(self.active_tab)
                                .map(|t| t.broadcast_enabled)
                                .unwrap_or(false);
                            if is_broadcasting {
                                ui.add_space(8.0);
                                ui.label(egui::RichText::new("|").color(sep_color).size(11.0));
                                ui.add_space(8.0);
                                ui.label(egui::RichText::new(self.language.t("broadcast")).color(self.theme.accent).size(11.0));
                            }

                            ui.add_space(8.0);
                            ui.label(egui::RichText::new("|").color(sep_color).size(11.0));
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new(&shell_label)
                                .color(if is_local_session { self.theme.fg_primary } else { self.theme.fg_dim })
                                .size(11.0));

                            ui.add_space(8.0);
                            ui.label(egui::RichText::new("|").color(sep_color).size(11.0));
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new(&encoding_label).color(self.theme.fg_dim).size(11.0));

                            if !uptime_label.is_empty() {
                                ui.add_space(8.0);
                                ui.label(egui::RichText::new("|").color(sep_color).size(11.0));
                                ui.add_space(8.0);
                                ui.label(egui::RichText::new(&uptime_label).color(self.theme.fg_dim).size(11.0));
                            }

                            let n = self.detached_windows.len();
                            if n > 0 {
                                ui.add_space(8.0);
                                ui.label(egui::RichText::new("|").color(sep_color).size(11.0));
                                ui.add_space(8.0);
                                ui.label(egui::RichText::new(format!("Detached: {}", n))
                                    .color(self.theme.green).size(11.0));
                            }
                        });
                    });
            }
        }

        // ── Central Panel: Main content area ──────────────────────────
        egui::CentralPanel::default()
            .frame(egui::Frame {
                fill: self.theme.bg_primary,
                inner_margin: egui::Margin::same(0.0),
                outer_margin: egui::Margin::same(0.0),
                ..Default::default()
            })
            .show(ctx, |ui| {
                match self.current_view {
                    AppView::Terminal => {
                        // ── Terminal Content (only, panels are outside) ───────
                        ui.style_mut().spacing.item_spacing = egui::vec2(0.0, 0.0);
                        let available = ui.available_rect_before_wrap();
                        let active = self.active_tab;
                        let focused = self.tabs[active].focused_session;
                        let can_close = self.tabs.len() > 1 || self.tabs[active].sessions.len() > 1;
                        let pane_result = {
                            let tab = &mut self.tabs[active];
                            let temp_broadcast = BroadcastState { enabled: tab.broadcast_enabled };
                            render_pane_tree(
                                ui, ctx,
                                &mut tab.layout,
                                available,
                                &mut tab.sessions,
                                focused,
                                &temp_broadcast,
                                &mut self.ime_composing,
                                &mut self.ime_preedit,
                                can_close,
                                &self.theme,
                                self.font_size,
                                &self.language,
                                &self.shortcut_resolver,
                            )
                        };
                        if let Some((idx, action, input_bytes)) = pane_result {
                            self.tabs[active].focused_session = idx;
                            if self.tabs[active].broadcast_enabled && !input_bytes.is_empty() {
                                for (sess_idx, session) in self.tabs[active].sessions.iter_mut().enumerate() {
                                    if sess_idx == idx { continue; }
                                    if let Some(ref mut backend) = session.session {
                                        if backend.is_connected() {
                                            let _ = backend.write(&input_bytes);
                                        }
                                    }
                                }
                            }
                            match action {
                                PaneAction::Focus => {}
                                PaneAction::SplitHorizontal => self.split_focused_pane(SplitDirection::Horizontal),
                                PaneAction::SplitVertical   => self.split_focused_pane(SplitDirection::Vertical),
                                PaneAction::ClosePane       => self.close_pane(idx),
                                PaneAction::ToggleBroadcast => {
                                    self.tabs[active].broadcast_enabled = !self.tabs[active].broadcast_enabled;
                                }
                                PaneAction::RemoveHostKey => {
                                    if let Some(host) = self.tabs[active].sessions.get(idx).and_then(|s| s.ssh_host.clone()) {
                                        let _ = crate::ssh::remove_known_hosts_key(&host.host, host.port);
                                        self.tabs[active].sessions[idx].reconnect_ssh(&self.runtime, None);
                                    }
                                }
                            }
                        }
                    }

                    AppView::Sftp => {
                        self.show_sftp_view(ui);
                    }

                    AppView::Settings => {
                        self.show_settings_view(ctx, ui);
                    }

                    AppView::Hosts => {
                        self.show_hosts_page(ctx, ui);
                    }

                    AppView::Snippets => {
                        self.show_snippets_page(ctx, ui);
                    }

                    AppView::Keychain => {
                        self.show_keychain_view(ctx, ui);
                    }

                    AppView::Tunnels => {
                        self.show_tunnels_page(ctx, ui);
                    }
                }
            });

        // ── Snippet run drawer (shown from terminal view only) ─────────────────────
        if self.snippet_view_state.quick_selector_open && self.current_view == AppView::Terminal {
            self.show_snippet_run_drawer(ctx);
        }
    }
}

fn load_app_icon() -> Option<egui::IconData> {
    let png_bytes = include_bytes!("../assets/portal-icon-1024.png");
    let decoder = eframe::icon_data::from_png_bytes(png_bytes);
    decoder.ok()
}

fn main() -> eframe::Result<()> {
    env_logger::init();

    // Set up signal handler for clean PTY cleanup on exit
    // This prevents PTY resource leaks when the app is terminated
    #[cfg(unix)]
    {
        use std::sync::atomic::{AtomicBool, Ordering};

        // Flag to track if we've already cleaned up
        static CLEANUP_DONE: AtomicBool = AtomicBool::new(false);

        // Spawn a dedicated cleanup thread that waits for signals
        std::thread::spawn(|| {
            use signal_hook::consts::signal::*;
            use signal_hook::iterator::Signals;

            let signals = Signals::new([SIGTERM, SIGINT, SIGHUP]).ok();

            if let Some(mut sig) = signals {
                for _ in sig.forever() {
                    if !CLEANUP_DONE.swap(true, Ordering::SeqCst) {
                        // Kill all zsh -l processes spawned by portal
                        unsafe {
                            let cmd = std::ffi::CString::new("pkill").unwrap();
                            let arg1 = std::ffi::CString::new("-9").unwrap();
                            let arg2 = std::ffi::CString::new("-f").unwrap();
                            let arg3 = std::ffi::CString::new("/bin/zsh -l").unwrap();
                            libc::execvp(cmd.as_ptr(), [cmd.as_ptr(), arg1.as_ptr(), arg2.as_ptr(), arg3.as_ptr(), std::ptr::null()].as_ptr());
                        }
                    }
                    // Exit after cleanup
                    std::process::exit(0);
                }
            }
        });
    }

    // Set macOS activation policy to Regular so the app appears in Dock
    #[cfg(target_os = "macos")]
    {
        use cocoa::appkit::{NSApp, NSApplication, NSApplicationActivationPolicy};
        use cocoa::base::{id, YES};

        unsafe {
            let app: id = NSApp();
            app.setActivationPolicy_(NSApplicationActivationPolicy::NSApplicationActivationPolicyRegular);
            app.activateIgnoringOtherApps_(YES);
        }
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1400.0, 900.0])
        .with_min_inner_size([800.0, 600.0])
        .with_title("Portal");

    if let Some(icon) = load_app_icon() {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }

    let options = eframe::NativeOptions {
        viewport,
        centered: true,
        ..Default::default()
    };

    eframe::run_native(
        "Portal",
        options,
        Box::new(|cc| Ok(Box::new(PortalApp::new(cc)))),
    )
}
