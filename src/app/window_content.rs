//! Window content rendering - unified for both main and detached windows
//!
//! This module provides a single `render_window_content()` function that
//! renders all UI elements for any window (main or detached).
//!
//! When adding new features, you only need to modify this file and the
//! AppWindow data structure - no need to update separate rendering logic.

use eframe::egui;

use crate::config;
use crate::ssh::SshConnectionState;
use crate::ui::*;
use crate::ui::views::tab_view::{TabBarAction, render_tab_bar};
use crate::ui::types::dialogs::AppView;
use crate::ui::pane::{PaneNode, PaneAction, SplitDirection, Tab};
use crate::ui::types::session::TerminalSession;
use crate::ui::types::BroadcastState;

use super::PortalApp;

/// Pending actions from window content rendering that need deferred processing
#[derive(Default)]
pub struct WindowContentResult {
    /// Tabs to detach from windows (window_idx, tab_idx)
    pub pending_detach: Vec<(usize, usize)>,
}

impl PortalApp {
    /// Render window content - unified for both main and detached windows
    ///
    /// This is the single entry point for rendering any window's content.
    /// All windows (main and detached) call this function.
    ///
    /// # Arguments
    /// * `ctx` - The egui context
    /// * `window_idx` - Index of the window in self.windows
    /// * `is_detached` - Whether this is a detached window (affects ID generation)
    ///
    /// # Returns
    /// Deferred actions that need processing after rendering completes
    pub fn render_window_content(
        &mut self,
        ctx: &egui::Context,
        window_idx: usize,
        is_detached: bool,
    ) -> WindowContentResult {
        let mut result = WindowContentResult::default();

        // ── Keyboard shortcuts (terminal view only) ─────────────────────
        let current_view = self.windows[window_idx].current_view;
        if current_view == AppView::Terminal {
            // Cmd+D → split horizontally
            if ctx.input(|i| i.key_pressed(egui::Key::D) && i.modifiers.command && !i.modifiers.shift) {
                self.split_focused_pane_in_window(window_idx, SplitDirection::Horizontal);
            }
            // Cmd+Shift+D → split vertically
            if ctx.input(|i| i.key_pressed(egui::Key::D) && i.modifiers.command && i.modifiers.shift) {
                self.split_focused_pane_in_window(window_idx, SplitDirection::Vertical);
            }
            // Cmd+Shift+S → toggle snippet drawer
            if ctx.input(|i| i.key_pressed(egui::Key::S) && i.modifiers.command && i.modifiers.shift) {
                let active_tab = self.windows[window_idx].active_tab;
                if let Some(tab) = self.windows[window_idx].tabs.get_mut(active_tab) {
                    tab.snippet_drawer_open = !tab.snippet_drawer_open;
                }
            }
        }

        // ── Sidebar (Navigation) ──
        let current_view = self.windows[window_idx].current_view;
        let nav_id = if is_detached {
            Some(egui::Id::new("detached_nav").with(window_idx))
        } else {
            None
        };
        if let Some(clicked_view) = crate::ui::views::nav_panel::show_nav_panel(
            ctx, current_view, &self.theme, &self.language, nav_id
        ) {
            self.windows[window_idx].current_view = clicked_view;
        }

        // ── Tab Bar (terminal view only) ──
        if self.windows[window_idx].current_view == AppView::Terminal {
            let tab_bar_id = if is_detached {
                egui::Id::new("detached_tab_bar").with(window_idx)
            } else {
                egui::Id::new("terminal_tab_bar")
            };
            let more_menu_id = if is_detached {
                egui::Id::new("dw_tab_bar_more_menu").with(window_idx)
            } else {
                egui::Id::new("tab_bar_more_menu")
            };

            egui::TopBottomPanel::top(tab_bar_id)
                .frame(egui::Frame {
                    fill: self.theme.bg_secondary,
                    inner_margin: egui::Margin::symmetric(8.0, if is_detached { 4.0 } else { 8.0 }),
                    stroke: egui::Stroke::NONE,
                    ..Default::default()
                })
                .show(ctx, |ui| {
                    let mut show_more_menu = ctx.data_mut(|d| *d.get_temp_mut_or_default::<bool>(more_menu_id));

                    let window = &mut self.windows[window_idx];
                    let action = render_tab_bar(
                        ui, ctx, &window.tabs, window.active_tab, &mut window.tab_drag,
                        &self.theme, &self.language, &mut show_more_menu, window_idx
                    );

                    ctx.data_mut(|d| d.insert_temp(more_menu_id, show_more_menu));

                    // Handle tab bar actions
                    self.handle_tab_bar_action(window_idx, action, &mut result, is_detached, ctx);
                });
        }

        // ── Status Bar (terminal view only) ──
        if self.windows[window_idx].current_view == AppView::Terminal {
            self.render_status_bar(ctx, window_idx, is_detached);
        }

        // ── Drawers (right panels, before CentralPanel) ──
        let current_view = self.windows[window_idx].current_view;
        if current_view == AppView::Hosts && self.add_host_dialog.open {
            self.show_add_host_drawer(ctx);
        }
        if current_view == AppView::Keychain && self.credential_dialog.open {
            self.show_credential_drawer(ctx);
        }
        if current_view == AppView::Snippets && self.snippet_view_state.open {
            let mut snippet_to_create: Option<crate::config::Snippet> = None;
            let mut snippet_to_save: Option<crate::config::Snippet> = None;
            self.show_add_snippet_drawer(ctx, &mut snippet_to_create, &mut snippet_to_save);
            if let Some(snippet) = snippet_to_create {
                self.snippets.push(snippet);
                crate::config::save_snippets(&self.snippets);
            }
            if let Some(updated) = snippet_to_save {
                if let Some(s) = self.snippets.iter_mut().find(|s| s.id == updated.id) {
                    s.name = updated.name;
                    s.command = updated.command;
                    s.group = updated.group;
                }
                crate::config::save_snippets(&self.snippets);
                self.snippet_view_state.editing = None;
            }
        }

        // ── Central Panel ──
        let current_view = self.windows[window_idx].current_view;
        egui::CentralPanel::default()
            .frame(egui::Frame {
                fill: self.theme.bg_primary,
                inner_margin: egui::Margin::same(0.0),
                outer_margin: egui::Margin::same(0.0),
                ..Default::default()
            })
            .show(ctx, |ui| {
                match current_view {
                    AppView::Terminal => {
                        self.render_terminal_content(ui, ctx, window_idx);
                    }
                    AppView::Hosts => {
                        self.show_hosts_page(ctx, ui);
                    }
                    AppView::Sftp => {
                        self.show_sftp_view(ui, window_idx);
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

        // ── Snippet run drawer (terminal view only) ──
        let snippet_drawer_open = self.windows[window_idx].tabs.get(self.windows[window_idx].active_tab)
            .map(|tab| tab.snippet_drawer_open)
            .unwrap_or(false);
        if snippet_drawer_open && self.windows[window_idx].current_view == AppView::Terminal {
            self.show_snippet_run_drawer(ctx);
        }

        result
    }

    /// Handle tab bar actions
    fn handle_tab_bar_action(
        &mut self,
        window_idx: usize,
        action: TabBarAction,
        result: &mut WindowContentResult,
        is_detached: bool,
        ctx: &egui::Context,
    ) {
        let more_menu_id = if is_detached {
            egui::Id::new("dw_tab_bar_more_menu").with(window_idx)
        } else {
            egui::Id::new("tab_bar_more_menu")
        };

        match action {
            TabBarAction::ActivateTab(ti) => {
                self.windows[window_idx].active_tab = ti;
            }
            TabBarAction::CloseTab(ti) => {
                let window = &mut self.windows[window_idx];
                if window.tabs.len() > 1 {
                    window.tabs.remove(ti);
                    if window.active_tab >= window.tabs.len() {
                        window.active_tab = window.tabs.len() - 1;
                    } else if window.active_tab > ti {
                        window.active_tab -= 1;
                    }
                }
            }
            TabBarAction::ReconnectTab(ti) => {
                let window = &mut self.windows[window_idx];
                let si = window.tabs[ti].focused_session;
                window.tabs[ti].sessions[si].reconnect_ssh(&self.runtime, None);
            }
            TabBarAction::DetachTab(ti) => {
                if self.windows[window_idx].tabs.len() > 1 {
                    result.pending_detach.push((window_idx, ti));
                }
            }
            TabBarAction::MergeTabs { src, dst } => {
                let window = &mut self.windows[window_idx];
                let mut src_tab = window.tabs.remove(src);
                let dst = if src < dst { dst - 1 } else { dst };
                let dst_tab = &mut window.tabs[dst];
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
                if window.active_tab == src {
                    window.active_tab = dst;
                } else if window.active_tab > src && window.active_tab > 0 {
                    window.active_tab -= 1;
                }
                if window.active_tab >= window.tabs.len() {
                    window.active_tab = window.tabs.len().saturating_sub(1);
                }
            }
            TabBarAction::ReorderTab { src, dst, insert_before } => {
                let window = &mut self.windows[window_idx];
                let src_tab = window.tabs.remove(src);
                let insert_idx = if insert_before {
                    if src < dst { dst.saturating_sub(1) } else { dst }
                } else {
                    if src < dst { dst } else { (dst + 1).min(window.tabs.len()) }
                };
                window.tabs.insert(insert_idx, src_tab);
                window.active_tab = insert_idx;
            }
            TabBarAction::NewTab => {
                let window = &mut self.windows[window_idx];
                let id = window.next_id;
                window.next_id += 1;
                let new_tab = Tab {
                    title: format!("Terminal {}", id),
                    sessions: vec![TerminalSession::new_local(id, &self.selected_shell)],
                    layout: PaneNode::Terminal(0),
                    focused_session: 0,
                    broadcast_enabled: false,
                    snippet_drawer_open: false,
                };
                window.tabs.push(new_tab);
                window.active_tab = window.tabs.len() - 1;
            }
            TabBarAction::ToggleBroadcast(ti) => {
                let window = &mut self.windows[window_idx];
                if ti < window.tabs.len() {
                    window.tabs[ti].broadcast_enabled = !window.tabs[ti].broadcast_enabled;
                }
                ctx.data_mut(|d| d.insert_temp(more_menu_id, false));
            }
            TabBarAction::OpenSnippets => {
                let window = &mut self.windows[window_idx];
                let active_tab = window.active_tab;
                if let Some(tab) = window.tabs.get_mut(active_tab) {
                    tab.snippet_drawer_open = true;
                }
                ctx.data_mut(|d| d.insert_temp(more_menu_id, false));
            }
            TabBarAction::None => {}
        }
    }

    /// Render status bar for a window
    fn render_status_bar(&mut self, ctx: &egui::Context, window_idx: usize, is_detached: bool) {
        let window = &self.windows[window_idx];

        let conn_type = window.tabs.get(window.active_tab)
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
        let shell_label = window.tabs.get(window.active_tab)
            .and_then(|tab| tab.sessions.get(tab.focused_session))
            .map(|s| s.shell_name())
            .unwrap_or_else(|| "—".to_string());
        let encoding_label = self.selected_encoding.clone();

        let uptime_label = window.tabs.get(window.active_tab)
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

        let status_bar_id = if is_detached {
            egui::Id::new("detached_status_bar").with(window_idx)
        } else {
            egui::Id::new("terminal_status_bar")
        };

        let sep_color = self.theme.border;
        let conn_color = if conn_type == "Local" { self.theme.green } else { self.theme.accent };
        let broadcast_enabled = self.windows[window_idx].tabs.get(self.windows[window_idx].active_tab)
            .map(|t| t.broadcast_enabled)
            .unwrap_or(false);

        egui::TopBottomPanel::bottom(status_bar_id)
            .exact_height(STATUS_BAR_HEIGHT)
            .frame(egui::Frame {
                fill: self.theme.bg_secondary,
                inner_margin: egui::Margin::symmetric(12.0, if is_detached { 0.0 } else { 8.0 }),
                outer_margin: egui::Margin::symmetric(0.0, 0.0),
                stroke: egui::Stroke::NONE,
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let font_size = if is_detached { 12.0 } else { 11.0 };

                    let status_btn = |ui: &mut egui::Ui, text: &str, color: egui::Color32| {
                        ui.add(egui::Button::new(
                            egui::RichText::new(text).color(color).size(font_size)
                        ).frame(false).rounding(0.0).min_size(egui::vec2(0.0, if is_detached { 24.0 } else { 20.0 })))
                    };
                    status_btn(ui, &conn_type, conn_color);

                    // Broadcast indicator
                    if broadcast_enabled {
                        ui.add_space(12.0);
                        ui.label(egui::RichText::new("|").color(sep_color).size(font_size));
                        ui.add_space(12.0);
                        ui.label(egui::RichText::new(self.language.t("broadcast")).color(self.theme.accent).size(font_size));
                    }
                    ui.add_space(12.0);
                    ui.label(egui::RichText::new("|").color(sep_color).size(font_size));
                    ui.add_space(12.0);

                    // Shell display
                    ui.label(egui::RichText::new(&shell_label)
                        .color(if is_local_session { self.theme.fg_primary } else { self.theme.fg_dim })
                        .size(font_size));
                    ui.add_space(12.0);
                    ui.label(egui::RichText::new("|").color(sep_color).size(font_size));
                    ui.add_space(12.0);

                    // Encoding display
                    ui.label(egui::RichText::new(&encoding_label)
                        .color(self.theme.fg_dim)
                        .size(font_size));

                    if !uptime_label.is_empty() {
                        ui.add_space(12.0);
                        ui.label(egui::RichText::new("|").color(sep_color).size(font_size));
                        ui.add_space(12.0);
                        ui.label(egui::RichText::new(&uptime_label).color(self.theme.fg_dim).size(font_size));
                    }

                    // Show detached windows count (main window only)
                    if !is_detached {
                        let n = self.windows.len().saturating_sub(1);
                        if n > 0 {
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new("|").color(sep_color).size(11.0));
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new(format!("Windows: {}", n))
                                .color(self.theme.green).size(11.0));
                        }
                    }
                });
            });
    }

    /// Render terminal content (tab panes)
    fn render_terminal_content(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, window_idx: usize) {
        ui.style_mut().spacing.item_spacing = egui::vec2(0.0, 0.0);
        let available = ui.available_rect_before_wrap();

        let active = self.windows[window_idx].active_tab;
        let focused = self.windows[window_idx].tabs[active].focused_session;
        let can_close = self.windows[window_idx].tabs.len() > 1
            || self.windows[window_idx].tabs[active].sessions.len() > 1;

        let pane_result = {
            let window = &mut self.windows[window_idx];
            let tab = &mut window.tabs[active];
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
                &mut window.ime_composing,
                &mut window.ime_preedit,
                can_close,
                &self.theme,
                self.font_size,
                &self.language,
                &self.shortcut_resolver,
            )
        };

        if let Some((idx, action, input_bytes)) = pane_result {
            self.windows[window_idx].tabs[active].focused_session = idx;

            // Broadcast input to all sessions
            if self.windows[window_idx].tabs[active].broadcast_enabled && !input_bytes.is_empty() {
                let window = &mut self.windows[window_idx];
                for (sess_idx, session) in window.tabs[active].sessions.iter_mut().enumerate() {
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
                    let window = &mut self.windows[window_idx];
                    let old_idx = idx;
                    let ssh_host = window.tabs[active].sessions.get(old_idx).and_then(|s| s.ssh_host.clone());
                    let resolved_auth = window.tabs[active].sessions.get(old_idx).and_then(|s| s.resolved_auth.clone());
                    let new_session = if let Some(host) = &ssh_host {
                        let auth = resolved_auth.unwrap_or(config::resolve_auth(host, &self.credentials));
                        TerminalSession::new_ssh(host, auth, &self.runtime, None)
                    } else {
                        let id = window.next_id;
                        window.next_id += 1;
                        TerminalSession::new_local(id, &self.selected_shell)
                    };
                    let tab = &mut window.tabs[active];
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
                    let window = &mut self.windows[window_idx];
                    let old_idx = idx;
                    let ssh_host = window.tabs[active].sessions.get(old_idx).and_then(|s| s.ssh_host.clone());
                    let resolved_auth = window.tabs[active].sessions.get(old_idx).and_then(|s| s.resolved_auth.clone());
                    let new_session = if let Some(host) = &ssh_host {
                        let auth = resolved_auth.unwrap_or(config::resolve_auth(host, &self.credentials));
                        TerminalSession::new_ssh(host, auth, &self.runtime, None)
                    } else {
                        let id = window.next_id;
                        window.next_id += 1;
                        TerminalSession::new_local(id, &self.selected_shell)
                    };
                    let tab = &mut window.tabs[active];
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
                    let window = &mut self.windows[window_idx];
                    let tab = &mut window.tabs[active];
                    if tab.sessions.len() <= 1 {
                        if window.tabs.len() > 1 {
                            window.tabs.remove(active);
                            if window.active_tab >= window.tabs.len() {
                                window.active_tab = window.tabs.len().saturating_sub(1);
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
                    self.windows[window_idx].tabs[active].broadcast_enabled =
                        !self.windows[window_idx].tabs[active].broadcast_enabled;
                }
                PaneAction::RemoveHostKey => {
                    let window = &mut self.windows[window_idx];
                    if let Some(host) = window.tabs[active].sessions.get(idx).and_then(|s| s.ssh_host.clone()) {
                        let _ = crate::ssh::remove_known_hosts_key(&host.host, host.port);
                        window.tabs[active].sessions[idx].reconnect_ssh(&self.runtime, None);
                    }
                }
                PaneAction::Reconnect => {
                    let window = &mut self.windows[window_idx];
                    window.tabs[active].sessions[idx].reconnect_ssh(&self.runtime, None);
                }
            }
        }
    }
}
