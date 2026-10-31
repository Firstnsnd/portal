//! Window content rendering - unified for both main and detached windows
//!
//! This module provides a single `render_window_content()` function that
//! renders all UI elements for any window (main or detached).
//!
//! When adding new features, you only need to modify this file and the
//! AppWindow data structure - no need to update separate rendering logic.

use eframe::egui;

use crate::config;
use crate::config::ShortcutAction;
use crate::ssh::{SshConnectionState, NotificationLevel};
use crate::ui::*;
use crate::ui::views::tab_view::{TabBarAction, render_tab_bar};
use crate::ui::types::dialogs::AppView;
use crate::ui::pane::{PaneNode, PaneAction, SplitDirection, Tab};
use crate::ui::types::session::{TerminalSession, SessionKind};
use crate::ui::types::BroadcastState;
use crate::ui::pane_view::{WindowContext, ViewActions};

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

        // ── Remote metrics polling gate ─────────────────────────────────
        // SSH sessions poll remote /proc stats only while the Metrics tab
        // of the active tab's tools drawer is visible (and only for the
        // focused pane — the one actually displayed). Synced every frame
        // so closing the drawer or leaving the Terminal view stops the
        // remote traffic immediately. On slow links a standing 5 s exec
        // loop competes with interactive I/O and makes vim etc. feel stuck.
        {
            let window = &self.windows[window_idx];
            for (ti, tab) in window.tabs.iter().enumerate() {
                let want = ti == window.active_tab
                    && current_view == AppView::Terminal
                    && tab.tools_drawer_open
                    && tab.tools_tab == 0;
                for (si, session) in tab.sessions.iter().enumerate() {
                    if let SessionKind::Ssh(ssh, _, _) = &session.kind {
                        ssh.set_metrics_enabled(want && si == tab.focused_session);
                    }
                }
            }
        }

        if current_view == AppView::Terminal {
            let sr = self.shortcut_resolver.clone();

            if sr.matches(ShortcutAction::SplitHorizontal, ctx) {
                self.split_focused_pane_in_window(window_idx, SplitDirection::Horizontal);
            }
            if sr.matches(ShortcutAction::SplitVertical, ctx) {
                self.split_focused_pane_in_window(window_idx, SplitDirection::Vertical);
            }
            if sr.matches(ShortcutAction::NewTab, ctx) {
                self.add_tab_local_to_window(window_idx);
            }
            if sr.matches(ShortcutAction::CloseTab, ctx) {
                let window = &mut self.windows[window_idx];
                let active = window.active_tab;
                let tab = &mut window.tabs[active];
                if tab.sessions.len() > 1 {
                    // Multiple panes — close focused pane
                    let focused = tab.focused_session;
                    let old_layout = tab.layout.clone();
                    if let Some(new_layout) = old_layout.remove(focused) {
                        tab.layout = new_layout;
                    }
                    tab.layout.decrement_indices_above(focused);
                    tab.sessions.remove(focused);
                    if tab.focused_session >= tab.sessions.len() {
                        tab.focused_session = tab.sessions.len().saturating_sub(1);
                    } else if tab.focused_session == focused && focused > 0 {
                        tab.focused_session = focused - 1;
                    }
                } else if window.tabs.len() > 1 {
                    // Single pane — close the tab
                    let removed_idx = active;
                    window.tabs.remove(active);
                    if window.active_tab >= window.tabs.len() {
                        window.active_tab = window.tabs.len().saturating_sub(1);
                    }
                    result.pending_detach.retain(|&(w, t)| w != window_idx || t != removed_idx);
                }
                self.mark_sessions_dirty();
            }
            if sr.matches(ShortcutAction::ClosePane, ctx) {
                let window = &mut self.windows[window_idx];
                let active = window.active_tab;
                let tab = &mut window.tabs[active];
                if tab.sessions.len() > 1 {
                    let focused = tab.focused_session;
                    let old_layout = tab.layout.clone();
                    if let Some(new_layout) = old_layout.remove(focused) {
                        tab.layout = new_layout;
                    }
                    tab.layout.decrement_indices_above(focused);
                    tab.sessions.remove(focused);
                    if tab.focused_session >= tab.sessions.len() {
                        tab.focused_session = tab.sessions.len().saturating_sub(1);
                    } else if tab.focused_session == focused && focused > 0 {
                        tab.focused_session = focused - 1;
                    }
                    self.mark_sessions_dirty();
                }
            }
            if sr.matches(ShortcutAction::NextTab, ctx) {
                let window = &mut self.windows[window_idx];
                if window.tabs.len() > 1 {
                    window.active_tab = (window.active_tab + 1) % window.tabs.len();
                }
            }
            if sr.matches(ShortcutAction::PrevTab, ctx) {
                let window = &mut self.windows[window_idx];
                if window.tabs.len() > 1 {
                    window.active_tab = if window.active_tab == 0 {
                        window.tabs.len() - 1
                    } else {
                        window.active_tab - 1
                    };
                }
            }
            // Cmd+Shift+M → open tools drawer with Metrics tab
            if ctx.input(|i| i.key_pressed(egui::Key::M) && i.modifiers.command && i.modifiers.shift) {
                let at = self.windows[window_idx].active_tab;
                if let Some(tab) = self.windows[window_idx].tabs.get_mut(at) {
                    if tab.tools_drawer_open && tab.tools_tab == 0 {
                        tab.tools_drawer_open = false; // toggle off if already on Metrics
                    } else {
                        tab.tools_drawer_open = true;
                        tab.tools_tab = 0;
                        if let Ok(mut ai) = tab.ai_chat.lock() {
                            ai.open = false;
                        }
                    }
                }
            }
            // Cmd+Shift+S → open tools drawer with Snippets tab
            if ctx.input(|i| i.key_pressed(egui::Key::S) && i.modifiers.command && i.modifiers.shift) {
                let at = self.windows[window_idx].active_tab;
                if let Some(tab) = self.windows[window_idx].tabs.get_mut(at) {
                    if tab.tools_drawer_open && tab.tools_tab == 1 {
                        tab.tools_drawer_open = false; // toggle off if already on Snippets
                    } else {
                        tab.tools_drawer_open = true;
                        tab.tools_tab = 1;
                        if let Ok(mut ai) = tab.ai_chat.lock() {
                            ai.open = false;
                        }
                    }
                }
            }
        }

        // ── Execute pending snippet (after drawer closed and PTY resized) ──
        if current_view == AppView::Terminal {
            let active_tab = self.windows[window_idx].active_tab;
            if let Some(tab) = self.windows[window_idx].tabs.get_mut(active_tab) {
                if let Some(cmd) = tab.pending_snippet.take() {
                    let focused = tab.focused_session;
                    if tab.broadcast_enabled {
                        for session in &mut tab.sessions {
                            session.write(&cmd);
                            session.write("\r");
                        }
                    } else if let Some(session) = tab.sessions.get_mut(focused) {
                        session.write(&cmd);
                        session.write("\r");
                    }
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

        // ── Poll session notifications ──
        {
            let window = &mut self.windows[window_idx];
            for tab in &window.tabs {
                for session in &tab.sessions {
                    let new_notifications = session.kind.drain_notifications();
                    window.notifications.extend(new_notifications);
                }
            }
        }

        // ── Toast notifications (shadcn/ui style, top-right) ──
        {
            let window = &mut self.windows[window_idx];
            window.notifications.retain(|n| !n.is_expired(5));

            if !window.notifications.is_empty() {
                let area_id = if is_detached {
                    egui::Id::new("detached_toasts").with(window_idx)
                } else {
                    egui::Id::new("app_toasts")
                };

                let notifications_count = window.notifications.len();
                let theme = &self.theme;

                egui::Area::new(area_id)
                    .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-16.0, 16.0))
                    .order(egui::Order::Foreground)
                    .interactable(true)
                    .show(ctx, |ui| {
                        ui.set_width(380.0);
                        ui.vertical(|ui| {
                            ui.add_space(0.0);
                            let mut to_remove = Vec::new();
                            for (i, notification) in window.notifications.iter().enumerate() {
                                // shadcn/ui toast: bg_elevated, thin border, subtle shadow, rounded
                                let (icon, icon_color, border_tint) = match notification.level {
                                    NotificationLevel::Info => {
                                        ("\u{2139}", theme.accent, theme.accent)
                                    }
                                    NotificationLevel::Warning => {
                                        ("\u{26A0}", self.theme.warning, self.theme.warning)
                                    }
                                    NotificationLevel::Error => {
                                        ("\u{274C}", theme.red, theme.red)
                                    }
                                };

                                let border_color = egui::Color32::from_rgba_unmultiplied(
                                    border_tint.r(), border_tint.g(), border_tint.b(), 60,
                                );

                                let frame = egui::Frame {
                                    fill: theme.bg_elevated,
                                    rounding: egui::Rounding::same(8.0),
                                    inner_margin: egui::Margin::same(14.0),
                                    stroke: egui::Stroke::new(1.0, border_color),
                                    shadow: egui::epaint::Shadow {
                                        offset: egui::vec2(0.0, 4.0),
                                        blur: 12.0,
                                        spread: 0.0,
                                        color: egui::Color32::from_black_alpha(40),
                                    },
                                    ..Default::default()
                                };

                                frame.show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        // Icon
                                        ui.label(
                                            egui::RichText::new(icon)
                                                .size(15.0)
                                                .color(icon_color),
                                        );
                                        ui.add_space(6.0);

                                        // Message
                                        ui.with_layout(
                                            egui::Layout::left_to_right(egui::Align::Center)
                                                .with_main_wrap(true),
                                            |ui| {
                                                ui.label(
                                                    egui::RichText::new(&notification.message)
                                                        .size(13.0)
                                                        .color(theme.fg_primary),
                                                );
                                            },
                                        );

                                        // Close button (shadcn: ghost button, rounded)
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                let close_resp = ui.add(
                                                    egui::Button::new(
                                                        egui::RichText::new("\u{00D7}")
                                                            .size(16.0)
                                                            .color(theme.fg_dim),
                                                    )
                                                    .frame(false)
                                                    .rounding(egui::Rounding::same(4.0)),
                                                );
                                                if close_resp.clicked() {
                                                    to_remove.push(i);
                                                }
                                                close_resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                                            },
                                        );
                                    });
                                });

                                // Gap between toasts
                                if i < notifications_count - 1 {
                                    ui.add_space(8.0);
                                }
                            }
                            for i in to_remove.into_iter().rev() {
                                window.notifications.remove(i);
                            }
                        });
                    });
            }
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
                    let mut showmore_menu_id = ctx.data_mut(|d| *d.get_temp_mut_or_default::<bool>(more_menu_id));

                    let window = &mut self.windows[window_idx];
                    let action = render_tab_bar(
                        ui, ctx, &window.tabs, window.active_tab, &mut window.tab_drag,
                        &self.theme, &self.language, &mut showmore_menu_id, window_idx
                    );

                    ctx.data_mut(|d| d.insert_temp(more_menu_id, showmore_menu_id));

                    // Handle tab bar actions
                    self.handle_tab_bar_action(window_idx, action, &mut result, is_detached, ctx);
                });
        }

        // ── Status Bar (terminal view only) ──
        if self.windows[window_idx].current_view == AppView::Terminal {
            self.render_status_bar(ctx, window_idx, is_detached);
        }

        // ── Drawers (right panels) ──
        // IMPORTANT: SidePanels must be created BEFORE CentralPanel
        // because CentralPanel takes remaining space
        {
            let mut cx = WindowContext::new(
                &mut self.hosts,
                &mut self.credentials,
                &mut self.snippets,
                &mut self.connection_history,
                &self.hosts_file,
                &self.credentials_file,
                &self.theme,
                self.language,
                &self.runtime,
                self.font_size,
                &self.llm_config,
                &self.host_sftp_support,
            );
            self.windows[window_idx].render_drawers(ctx, &mut cx);
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
                        // Use new per-window rendering with WindowContext
                        let view_actions = {
                            let mut cx = WindowContext::new(
                                &mut self.hosts,
                                &mut self.credentials,
                                &mut self.snippets,
                                &mut self.connection_history,
                                &self.hosts_file,
                                &self.credentials_file,
                                &self.theme,
                                self.language,
                                &self.runtime,
                                self.font_size,
                                &self.llm_config,
                                &self.host_sftp_support,
                            );
                            self.windows[window_idx].render_central_content(ctx, ui, &mut cx)
                        };
                        // Handle view actions
                        self.handle_view_actions(view_actions, window_idx, ctx);
                    }
                    AppView::Sftp => {
                        let view_actions = {
                            let mut cx = WindowContext::new(
                                &mut self.hosts,
                                &mut self.credentials,
                                &mut self.snippets,
                                &mut self.connection_history,
                                &self.hosts_file,
                                &self.credentials_file,
                                &self.theme,
                                self.language,
                                &self.runtime,
                                self.font_size,
                                &self.llm_config,
                                &self.host_sftp_support,
                            );
                            self.windows[window_idx].render_central_content(ctx, ui, &mut cx)
                        };
                        self.handle_view_actions(view_actions, window_idx, ctx);
                    }
                    AppView::Keychain => {
                        let view_actions = {
                            let mut cx = WindowContext::new(
                                &mut self.hosts,
                                &mut self.credentials,
                                &mut self.snippets,
                                &mut self.connection_history,
                                &self.hosts_file,
                                &self.credentials_file,
                                &self.theme,
                                self.language,
                                &self.runtime,
                                self.font_size,
                                &self.llm_config,
                                &self.host_sftp_support,
                            );
                            self.windows[window_idx].render_central_content(ctx, ui, &mut cx)
                        };
                        self.handle_view_actions(view_actions, window_idx, ctx);
                    }
                    AppView::Settings => {
                        self.show_settings_view(ctx, ui);
                    }
                    AppView::Snippets => {
                        let view_actions = {
                            let mut cx = WindowContext::new(
                                &mut self.hosts,
                                &mut self.credentials,
                                &mut self.snippets,
                                &mut self.connection_history,
                                &self.hosts_file,
                                &self.credentials_file,
                                &self.theme,
                                self.language,
                                &self.runtime,
                                self.font_size,
                                &self.llm_config,
                                &self.host_sftp_support,
                            );
                            self.windows[window_idx].render_central_content(ctx, ui, &mut cx)
                        };
                        self.handle_view_actions(view_actions, window_idx, ctx);
                    }
                    AppView::Replay => {
                        crate::ui::views::replay_view::render_replay_view(
                            &mut self.windows[window_idx],
                            ctx,
                            ui,
                            &self.theme,
                            self.font_size,
                            self.language,
                        );
                    }
                }
            });

        // Update-available dialog is app-global: render it only in the root
        // window so detached windows never duplicate it.
        if window_idx == 0 {
            self.render_update_dialog(ctx);
        }

        result
    }

    /// Render the update-check prompt/download progress dialog, dispatching on
    /// the shared `update_state`. Non-modal: it does not block input elsewhere.
    fn render_update_dialog(&mut self, ctx: &egui::Context) {
        use crate::update::UpdateState;
        use crate::ui::widgets;

        // Snapshot the state; all mutations go through methods that take &mut
        // self (which also manage persisted dismissals and thread spawns).
        let state = self.update_state.lock().expect("update_state lock").clone();

        match state {
            UpdateState::Idle => {}
            // Version check runs silently in the background — no UI. The user
            // only sees anything once a newer version is actually found.
            UpdateState::Checking => {}
            UpdateState::Available { version, .. } => {
                // Never re-prompt for a version the user already dismissed.
                if self.last_dismissed_update_version.as_deref() == Some(version.as_str()) {
                    if let Ok(mut g) = self.update_state.lock() {
                        *g = UpdateState::Idle;
                    }
                    return;
                }
                let mut act = None;
                egui::Window::new("update_available")
                    .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -16.0))
                    .title_bar(false)
                    .resizable(false)
                    .collapsible(false)
                    .frame(widgets::dialog_frame(&self.theme))
                    .show(ctx, |ui| {
                        ui.label(egui::RichText::new(self.language.tf("update_available", &version))
                            .size(15.0).strong().color(self.theme.fg_primary));
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(format!("{} → {}", env!("CARGO_PKG_VERSION"), version))
                            .size(12.0).color(self.theme.fg_dim));
                        ui.add_space(16.0);
                        ui.horizontal(|ui| {
                            if ui.add(widgets::primary_button(self.language.t("update_download"), &self.theme)).clicked() {
                                act = Some(true);
                            }
                            if ui.add(widgets::secondary_button(self.language.t("update_dismiss"), &self.theme)).clicked() {
                                act = Some(false);
                            }
                        });
                    });
                match act {
                    Some(true) => self.begin_update_download(),
                    Some(false) => self.dismiss_update(&version),
                    None => {}
                }
            }
            UpdateState::Downloading { version, asset_name, received, total } => {
                egui::Window::new("update_downloading")
                    .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -16.0))
                    .title_bar(false)
                    .resizable(false)
                    .collapsible(false)
                    .frame(widgets::dialog_frame(&self.theme))
                    .show(ctx, |ui| {
                        ui.label(egui::RichText::new(format!("{} v{}", self.language.t("update_downloading"), version))
                            .size(14.0).strong().color(self.theme.fg_primary));
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(&asset_name).size(12.0).color(self.theme.fg_dim));
                        ui.add_space(12.0);
                        match total {
                            Some(t) if t > 0 => {
                                let pct = received as f32 / t as f32;
                                ui.add(egui::ProgressBar::new(pct).text(format!("{:.0}%", pct * 100.0)));
                            }
                            _ => {
                                ui.horizontal(|ui| {
                                    ui.add(egui::Spinner::new());
                                    ui.add_space(8.0);
                                    ui.label(self.language.t("update_downloading"));
                                });
                            }
                        }
                    });
            }
            UpdateState::Ready { version, path } => {
                let mut act = None;
                egui::Window::new("update_ready")
                    .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -16.0))
                    .title_bar(false)
                    .resizable(false)
                    .collapsible(false)
                    .frame(widgets::dialog_frame(&self.theme))
                    .show(ctx, |ui| {
                        ui.label(egui::RichText::new(self.language.tf("update_ready", &version))
                            .size(14.0).strong().color(self.theme.fg_primary));
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(path.display().to_string()).size(12.0).color(self.theme.fg_dim));
                        ui.add_space(16.0);
                        ui.horizontal(|ui| {
                            if ui.add(widgets::primary_button(self.language.t("update_open"), &self.theme)).clicked() {
                                act = Some(true);
                            }
                            if ui.add(widgets::secondary_button(self.language.t("update_close"), &self.theme)).clicked() {
                                act = Some(false);
                            }
                        });
                    });
                if let Some(true) = act {
                    // Re-open the already-revealed path (background, non-blocking).
                    let arc = std::sync::Arc::clone(&self.update_state);
                    let p = path.clone();
                    std::thread::spawn(move || {
                        let _ = crate::update::open_revealed(&p);
                        if let Ok(mut g) = arc.lock() {
                            *g = UpdateState::Idle;
                        }
                    });
                } else if let Some(false) = act {
                    if let Ok(mut g) = self.update_state.lock() {
                        *g = UpdateState::Idle;
                    }
                }
            }
            UpdateState::Error(e) => {
                let mut close = false;
                egui::Window::new("update_error")
                    .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -16.0))
                    .title_bar(false)
                    .resizable(false)
                    .collapsible(false)
                    .frame(widgets::dialog_frame(&self.theme))
                    .show(ctx, |ui| {
                        ui.label(egui::RichText::new(self.language.t("update_check_error"))
                            .size(14.0).strong().color(self.theme.fg_primary));
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(&e).size(12.0).color(self.theme.fg_dim));
                        ui.add_space(16.0);
                        if ui.add(widgets::secondary_button(self.language.t("update_close"), &self.theme)).clicked() {
                            close = true;
                        }
                    });
                if close {
                    if let Ok(mut g) = self.update_state.lock() {
                        *g = UpdateState::Idle;
                    }
                }
            }
        }
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
        let _ = ctx;
        let _more_menu_id = if is_detached {
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
                    self.mark_sessions_dirty();
                }
            }
            TabBarAction::ReconnectTab(ti) => {
                let window = &mut self.windows[window_idx];
                let si = window.tabs[ti].focused_session;
                // Only SSH sessions support reconnection; Local sessions have no
                // connection concept and must never reach reconnect_ssh.
                if matches!(&window.tabs[ti].sessions[si].kind, SessionKind::Ssh(_, _, _)) {
                    window.tabs[ti].sessions[si].reconnect_ssh(&self.runtime, None);
                }
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
                self.mark_sessions_dirty();
            }
            TabBarAction::ReorderTab { src, dst, insert_before } => {
                let window = &mut self.windows[window_idx];
                let src_tab = window.tabs.remove(src);
                let insert_idx = if insert_before {
                    if src < dst { dst.saturating_sub(1) } else { dst }
                } else if src < dst { dst } else { (dst + 1).min(window.tabs.len()) };
                window.tabs.insert(insert_idx, src_tab);
                window.active_tab = insert_idx;
                self.mark_sessions_dirty();
            }
            TabBarAction::NewTab => {
                let window = &mut self.windows[window_idx];
                let id = window.next_id;
                window.next_id += 1;
                let default_shell = std::env::var("SHELL")
                    .unwrap_or_else(|_| "/bin/zsh".to_string());
                let new_tab = Tab {
                    title: format!("Terminal {}", id),
                    sessions: vec![TerminalSession::new_local(id, &default_shell)],
                    layout: PaneNode::Terminal(0),
                    focused_session: 0,
                    broadcast_enabled: false,
                    tools_drawer_open: false,
                    tools_tab: 0,
                    pending_snippet: None,
                    ai_chat: std::sync::Arc::new(std::sync::Mutex::new(
                        crate::ui::state::ai_chat::AiChatState::default(),
                    )),
                    pending_ai_command: None,
                };
                window.tabs.push(new_tab);
                window.active_tab = window.tabs.len() - 1;
                self.mark_sessions_dirty();
            }
            TabBarAction::ToggleToolsDrawer => {
                let window = &mut self.windows[window_idx];
                let active_tab = window.active_tab;
                if let Some(tab) = window.tabs.get_mut(active_tab) {
                    if tab.tools_drawer_open {
                        tab.tools_drawer_open = false;
                    } else {
                        tab.tools_drawer_open = true;
                        tab.tools_tab = 0; // default to Metrics
                        // The tools drawer and AI drawer share the right-hand
                        // rail — opening tools dismisses the AI drawer.
                        if let Ok(mut ai) = tab.ai_chat.lock() {
                            ai.open = false;
                        }
                    }
                }
            }
            TabBarAction::None => {}
        }
    }

    /// Render status bar for a window
    fn render_status_bar(&mut self, ctx: &egui::Context, window_idx: usize, is_detached: bool) {
        let window = &self.windows[window_idx];

        let conn_type = window.tabs.get(window.active_tab)
            .and_then(|tab| tab.sessions.get(tab.focused_session))
            .map(|s| match &s.kind {
                SessionKind::Ssh(ssh, ssh_host, _) => {
                    let host = format!("{}@{}:{}", ssh_host.username, ssh_host.host, ssh_host.port);
                    match ssh.connection_state() {
                        SshConnectionState::Connected => format!("SSH  {}", host),
                        SshConnectionState::Connecting => format!("SSH  {} (connecting…)", host),
                        SshConnectionState::Authenticating => format!("SSH  {} (authenticating…)", host),
                        SshConnectionState::Disconnected(_) => format!("SSH  {} (disconnected)", host),
                        SshConnectionState::Error(_) => format!("SSH  {}", host),
                    }
                }
                SessionKind::Local(_, _) => "Local".to_string(),
            })
            .unwrap_or_else(|| "Local".to_string());

        let is_local_session = conn_type == "Local";
        let shell_label = window.tabs.get(window.active_tab)
            .and_then(|tab| tab.sessions.get(tab.focused_session))
            .map(|s| s.shell_name())
            .unwrap_or_else(|| "—".to_string());

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
        // Recording state of the focused pane (for the REC status indicator).
        let recording_active = self.windows[window_idx].tabs
            .get(self.windows[window_idx].active_tab)
            .and_then(|t| t.sessions.get(t.focused_session))
            .map(|s| s.kind.is_recording())
            .unwrap_or(false);

        egui::TopBottomPanel::bottom(status_bar_id)
            .exact_height(STATUS_BAR_HEIGHT)
            .frame(egui::Frame {
                fill: self.theme.bg_secondary,
                // No vertical inner margin: it would shrink the content rect
                // below the text height and break `horizontal_centered`'s
                // vertical centering (text ended up glued to the bottom).
                inner_margin: egui::Margin::symmetric(12.0, 0.0),
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
                    // Recording indicator (focused pane) — clickable, jumps to
                    // this host's recordings in the replay library.
                    if recording_active {
                        ui.add_space(12.0);
                        ui.label(egui::RichText::new("|").color(sep_color).size(font_size));
                        ui.add_space(12.0);
                        // Red REC label; keep it ticking so the dot can't appear stale.
                        let rec_host = self.windows[window_idx].tabs
                            .get(self.windows[window_idx].active_tab)
                            .and_then(|t| t.sessions.get(t.focused_session))
                            .map(|s| s.kind.recording_host());
                        ui.label(egui::RichText::new("●").color(self.theme.red).size(font_size));
                        ui.add_space(4.0);
                        let rec_resp = status_btn(ui, self.language.t("recording"), self.theme.red);
                        if rec_resp.clicked() {
                            if let Some(host) = rec_host {
                                let w = &mut self.windows[window_idx];
                                w.replay_host_filter = Some(host);
                                w.replay = None;
                                w.current_view = AppView::Replay;
                            }
                        }
                        ctx.request_repaint_after(std::time::Duration::from_millis(500));
                    }
                    // Recording failure toast (5s, right in the status bar)
                    if let Some((err, at)) = &self.windows[window_idx].recording_error {
                        if at.elapsed() < std::time::Duration::from_secs(5) {
                            ui.add_space(12.0);
                            ui.label(egui::RichText::new("|").color(sep_color).size(font_size));
                            ui.add_space(12.0);
                            ui.label(egui::RichText::new(
                                self.language.tf("recording_failed", err),
                            ).color(self.theme.red).size(font_size));
                            ctx.request_repaint_after(std::time::Duration::from_millis(
                                5000 - at.elapsed().as_millis().min(4999) as u64 + 1,
                            ));
                        } else {
                            self.windows[window_idx].recording_error = None;
                        }
                    }
                    // Recording saved notice (8s): click to open the replay
                    // player on the file that was just written.
                    if let Some((path, at)) = &self.windows[window_idx].recording_saved {
                        let (saved_path, saved_at) = (path.clone(), *at);
                        if saved_at.elapsed() < std::time::Duration::from_secs(8) {
                            ui.add_space(12.0);
                            ui.label(egui::RichText::new("|").color(sep_color).size(font_size));
                            ui.add_space(12.0);
                            let name = saved_path.file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default();
                            let saved = status_btn(
                                ui,
                                &self.language.tf("recording_saved", &name),
                                self.theme.green,
                            ).on_hover_text(self.language.t("replay"));
                            if saved.clicked() {
                                let w = &mut self.windows[window_idx];
                                w.replay = Some(crate::ui::views::replay_view::ReplayState::load(&saved_path));
                                w.replay_host_filter = None;
                                w.current_view = AppView::Replay;
                                w.recording_saved = None;
                            }
                            ctx.request_repaint_after(std::time::Duration::from_millis(
                                8000 - saved_at.elapsed().as_millis().min(7999) as u64 + 1,
                            ));
                        } else {
                            self.windows[window_idx].recording_saved = None;
                        }
                    }
                    // ZMODEM transfer chip (focused pane): live byte progress
                    // while active (click cancels); final states stay until
                    // clicked (dismiss).
                    #[cfg(unix)]
                    {
                        use crate::terminal::zmodem::{Direction, ZmodemState};
                        fn format_transfer_bytes(bytes: u64) -> String {
                            const KB: f64 = 1024.0;
                            const MB: f64 = 1024.0 * 1024.0;
                            if bytes as f64 >= MB {
                                format!("{:.1} MB", bytes as f64 / MB)
                            } else if bytes as f64 >= KB {
                                format!("{:.0} KB", bytes as f64 / KB)
                            } else {
                                format!("{bytes} B")
                            }
                        }
                        let active_tab_i = self.windows[window_idx].active_tab;
                        let zt = self.windows[window_idx]
                            .tabs
                            .get(active_tab_i)
                            .and_then(|t| t.sessions.get(t.focused_session))
                            .map(|s| (s.kind.zmodem_transfer(), s.kind.zmodem_bytes()));
                        if let Some((Some(transfer), live_bytes)) = zt {
                            ui.add_space(12.0);
                            ui.label(
                                egui::RichText::new("|").color(sep_color).size(font_size),
                            );
                            ui.add_space(12.0);
                            let bytes = if transfer.is_active() && live_bytes > 0 {
                                live_bytes
                            } else {
                                match &transfer.state {
                                    ZmodemState::Done { bytes, .. } => *bytes,
                                    _ => 0,
                                }
                            };
                            let (text, color) = match (&transfer.state, transfer.direction)
                            {
                                (ZmodemState::Handshaking, Direction::Upload) => (
                                    self.language.t("zmodem_sending").to_string(),
                                    self.theme.accent,
                                ),
                                (ZmodemState::Handshaking, Direction::Download) => (
                                    self.language.t("zmodem_receiving").to_string(),
                                    self.theme.accent,
                                ),
                                (ZmodemState::Transferring { file }, dir) => {
                                    let verb = match dir {
                                        Direction::Upload => self.language.t("zmodem_sending"),
                                        Direction::Download => self.language.t("zmodem_receiving"),
                                    };
                                    let label = if file.is_empty() {
                                        verb.to_string()
                                    } else {
                                        format!("{verb} {file}")
                                    };
                                    (label, self.theme.accent)
                                }
                                (ZmodemState::Finishing, _) => (
                                    self.language.t("zmodem_sending").to_string(),
                                    self.theme.accent,
                                ),
                                (ZmodemState::Done { .. }, _) => (
                                    self.language.t("zmodem_complete").to_string(),
                                    self.theme.green,
                                ),
                                (ZmodemState::Failed(e), _) => (
                                    self.language.tf("zmodem_failed", e),
                                    self.theme.red,
                                ),
                                (ZmodemState::Cancelled, _) => (
                                    self.language.t("zmodem_cancelled").to_string(),
                                    self.theme.fg_dim,
                                ),
                            };
                            let text = if bytes > 0 {
                                format!("{text} • {}", format_transfer_bytes(bytes))
                            } else {
                                text
                            };
                            let hover = if transfer.is_active() {
                                self.language.t("zmodem_cancel").to_string()
                            } else if let ZmodemState::Done { files, .. } = &transfer.state {
                                files.join(", ")
                            } else {
                                String::new()
                            };
                            let mut chip = status_btn(ui, &text, color);
                            if !hover.is_empty() {
                                chip = chip.on_hover_text(hover);
                            }
                            if chip.clicked() {
                                let w = &mut self.windows[window_idx];
                                if let Some(s) = w.tabs
                                    .get_mut(w.active_tab)
                                    .and_then(|t| t.sessions.get_mut(t.focused_session))
                                {
                                    if transfer.is_active() {
                                        s.kind.cancel_zmodem();
                                    } else {
                                        s.kind.clear_zmodem_state();
                                    }
                                }
                            }
                            if transfer.is_active() {
                                // Keep the byte counter animating.
                                ctx.request_repaint_after(std::time::Duration::from_millis(
                                    250,
                                ));
                            }
                        }
                    }
                    ui.add_space(12.0);
                    ui.label(egui::RichText::new("|").color(sep_color).size(font_size));
                    ui.add_space(12.0);

                    // Shell display
                    ui.label(egui::RichText::new(&shell_label)
                        .color(if is_local_session { self.theme.fg_primary } else { self.theme.fg_dim })
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

        // Owned value, computed before the pane block below borrows
        // `self.windows` mutably (TextureHandle clones are cheap).
        let background = self.ensure_background_texture(ctx);

        let pane_result = {
            let window = &mut self.windows[window_idx];
            let ai_composer_id = window.ai_composer_id();
            let tab = &mut window.tabs[active];
            let temp_broadcast = BroadcastState {
                enabled: tab.broadcast_enabled,
            };
            // The AI composer's TextEdit must be able to hold keyboard
            // focus; while it does, the terminal panes stand down from
            // re-claiming focus every frame (see render_terminal_session).
            // A click on a pane takes focus back regardless.
            let ai_chat_open = tab
                .ai_chat
                .lock()
                .map(|s| s.open)
                .unwrap_or(false);
            let ai_composer_holds_focus = ai_chat_open
                && ui.memory(|mem| mem.has_focus(ai_composer_id));
            render_pane_tree(
                ui, ctx,
                &mut tab.layout,
                available,
                &mut tab.sessions,
                focused,
                ai_composer_holds_focus,
                &temp_broadcast,
                &mut window.ime_composing,
                &mut window.ime_preedit,
                &mut window.key_char_suppress,
                can_close,
                &self.theme,
                self.font_size,
                &self.language,
                &self.shortcut_resolver,
                background,
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
                    let live = match &session.kind {
                        SessionKind::Local(pty, _) => !pty.has_exited(),
                        SessionKind::Ssh(ssh, _, _) => matches!(
                            ssh.connection_state(),
                            SshConnectionState::Connected
                        ),
                    };
                    if live {
                        let _ = session.kind.write(&input_bytes);
                    }
                }
            }

            match action {
                PaneAction::Focus => {}
                PaneAction::SplitHorizontal => {
                    let window = &mut self.windows[window_idx];
                    let old_idx = idx;
                    let new_session = match &window.tabs[active].sessions.get(old_idx).map(|s| &s.kind) {
                        Some(SessionKind::Ssh(_, host, auth)) => {
                            TerminalSession::new_ssh(host, auth.clone(), &self.runtime, None)
                        }
                        _ => {
                            let default_shell = std::env::var("SHELL")
                                .unwrap_or_else(|_| "/bin/zsh".to_string());
                            let id = window.next_id;
                            window.next_id += 1;
                            TerminalSession::new_local(id, &default_shell)
                        }
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
                    self.mark_sessions_dirty();
                }
                PaneAction::SplitVertical => {
                    let window = &mut self.windows[window_idx];
                    let old_idx = idx;
                    let new_session = match &window.tabs[active].sessions.get(old_idx).map(|s| &s.kind) {
                        Some(SessionKind::Ssh(_, host, auth)) => {
                            TerminalSession::new_ssh(host, auth.clone(), &self.runtime, None)
                        }
                        _ => {
                            let default_shell = std::env::var("SHELL")
                                .unwrap_or_else(|_| "/bin/zsh".to_string());
                            let id = window.next_id;
                            window.next_id += 1;
                            TerminalSession::new_local(id, &default_shell)
                        }
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
                    self.mark_sessions_dirty();
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
                            self.mark_sessions_dirty();
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
                        self.mark_sessions_dirty();
                    }
                }
                PaneAction::ToggleBroadcast => {
                    self.windows[window_idx].tabs[active].broadcast_enabled =
                        !self.windows[window_idx].tabs[active].broadcast_enabled;
                }
                PaneAction::ToggleRecording => {
                    let window = &mut self.windows[window_idx];
                    if let Some(session) = window.tabs[active].sessions.get_mut(idx) {
                        if session.kind.is_recording() {
                            // Surface the finished file: the status bar shows a
                            // clickable "saved" notice that opens the player.
                            if let Some(path) = session.kind.stop_recording() {
                                window.recording_saved =
                                    Some((path, std::time::Instant::now()));
                            }
                        } else {
                            let label = session.kind.recording_label();
                            match crate::terminal::cast::new_recording_path(&label)
                                .and_then(|path| {
                                    session.kind.start_recording(&path)?;
                                    Ok(path)
                                }) {
                                Ok(_path) => {}
                                Err(e) => {
                                    window.recording_error =
                                        Some((e.to_string(), std::time::Instant::now()));
                                }
                            }
                        }
                    }
                }
                PaneAction::ViewHostRecordings => {
                    let window = &mut self.windows[window_idx];
                    if let Some(host) = window.tabs[active]
                        .sessions
                        .get(idx)
                        .map(|s| s.kind.recording_host())
                    {
                        window.replay_host_filter = Some(host);
                        window.replay = None; // library, not the player
                        window.current_view = AppView::Replay;
                    }
                }
                #[cfg(unix)]
                PaneAction::ZmodemUpload => {
                    let window = &mut self.windows[window_idx];
                    if let Some(session) = window.tabs[active].sessions.get(idx) {
                        if let Some(path) = rfd::FileDialog::new().pick_file() {
                            if let Err(e) = session.kind.start_zmodem_upload(&path) {
                                window.recording_error =
                                    Some((e.to_string(), std::time::Instant::now()));
                            }
                        }
                    }
                }
                #[cfg(unix)]
                PaneAction::ZmodemDownload => {
                    let window = &mut self.windows[window_idx];
                    window.zmodem_download_dialog = Some(String::new());
                }
                #[cfg(unix)]
                PaneAction::ZmodemCancel => {
                    let window = &mut self.windows[window_idx];
                    if let Some(session) = window.tabs[active].sessions.get(idx) {
                        let transfer = session.kind.zmodem_transfer();
                        if transfer.is_some_and(|t| t.is_active()) {
                            session.kind.cancel_zmodem();
                        } else {
                            session.kind.clear_zmodem_state();
                        }
                    }
                }
                PaneAction::SendSelectionToAi => {
                    let window = &mut self.windows[window_idx];
                    if let Some(session) = window.tabs[active].sessions.get(idx) {
                        if let Some(text) =
                            crate::ui::terminal::render::selected_text_of(session)
                        {
                            let tab = &mut window.tabs[active];
                            {
                                let mut ai = tab.ai_chat.lock().unwrap();
                                // Append to any existing draft so a
                                // half-typed question survives.
                                if !ai.input.is_empty() {
                                    ai.input.push('\n');
                                }
                                ai.input.push_str(&text);
                                ai.open = true;
                            }
                            // The AI drawer and tools drawer share the
                            // right-hand rail — same exclusion as orb click.
                            tab.tools_drawer_open = false;
                        }
                    }
                }
                PaneAction::RemoveHostKey => {
                    let window = &mut self.windows[window_idx];
                    let host_opt = match &window.tabs[active].sessions.get(idx).map(|s| &s.kind) {
                        Some(SessionKind::Ssh(_, host, _)) => Some(host.clone()),
                        _ => None,
                    };
                    if let Some(host) = host_opt {
                        let _ = crate::ssh::remove_known_hosts_key(&host.host, host.port);
                        window.tabs[active].sessions[idx].reconnect_ssh(&self.runtime, None);
                    }
                }
                PaneAction::Reconnect => {
                    let window = &mut self.windows[window_idx];
                    // Only SSH sessions support reconnection.
                    if matches!(&window.tabs[active].sessions[idx].kind, SessionKind::Ssh(_, _, _)) {
                        // An active ZMODEM transfer dies with the old connection —
                        // say so instead of letting the chip vanish silently.
                        #[cfg(unix)]
                        if window.tabs[active].sessions[idx]
                            .kind
                            .zmodem_transfer()
                            .is_some_and(|t| t.is_active())
                        {
                            window.recording_error = Some((
                                self.language.t("zmodem_transfer_interrupted").to_string(),
                                std::time::Instant::now(),
                            ));
                        }
                        window.tabs[active].sessions[idx].reconnect_ssh(&self.runtime, None);
                    }
                }
            }
        }

        // ── ZMODEM download: remote-path prompt (unix only) ──
        // Opening it takes the buffer out; submitting starts the transfer
        // (the buffer stays taken = dialog closed) or puts it back.
        #[cfg(unix)]
        if self.windows[window_idx].zmodem_download_dialog.is_some() {
            let t_download = self.language.t("zmodem_download").to_string();
            let t_remote_path = self.language.t("zmodem_remote_path").to_string();
            let t_cancel = self.language.t("cancel").to_string();
            let mut open = true;
            let mut submitted = false;
            let mut cancel_clicked = false;
            let mut buffer = self
                .windows[window_idx]
                .zmodem_download_dialog
                .take()
                .unwrap_or_default();
            egui::Window::new(egui::RichText::new(t_download).heading())
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(t_remote_path);
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut buffer)
                            .desired_width(360.0)
                            .hint_text("/var/log/app/error.log"),
                    );
                    if field.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    {
                        submitted = true;
                    }
                    field.request_focus();
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(self.language.t("save")).clicked() {
                            submitted = true;
                        }
                        if ui.button(t_cancel).clicked() {
                            cancel_clicked = true;
                        }
                    });
                });
            let remote = buffer.trim().to_string();
            if submitted && !remote.is_empty() {
                let dir = config::zmodem_download_dir();
                let window = &mut self.windows[window_idx];
                if let Some(session) = window.tabs
                    .get_mut(window.active_tab)
                    .and_then(|t| t.sessions.get_mut(t.focused_session))
                {
                    if let Err(e) = session.kind.start_zmodem_download(&remote, &dir) {
                        window.recording_error =
                            Some((e.to_string(), std::time::Instant::now()));
                    }
                }
            } else if open && !cancel_clicked {
                self.windows[window_idx].zmodem_download_dialog = Some(buffer);
            }
        }

        // ── AI orb (one per tab) + pending command execution ──
        let tab = &mut self.windows[window_idx].tabs[active];
        let prev_offset = tab.ai_chat.lock().map(|s| s.orb_offset).unwrap_or_default();
        // Hide the orb while the drawer is open (it overlaps the drawer area and
        // its Foreground Area would otherwise steal the drawer's input).
        let drawer_open = tab.ai_chat.lock().map(|s| s.open).unwrap_or(false);
        let (orb_clicked, new_offset) = if drawer_open {
            (false, prev_offset)
        } else {
            crate::ui::views::ai_drawer::render_orb(ctx, available, &self.theme, prev_offset)
        };
        {
            let mut ai = tab.ai_chat.lock().unwrap();
            if new_offset != ai.orb_offset {
                ai.orb_offset = new_offset;
            }
            if orb_clicked {
                ai.open = true;
            }
        }
        if orb_clicked {
            // The AI drawer and tools drawer share the right-hand rail —
            // opening the AI drawer dismisses the tools drawer.
            tab.tools_drawer_open = false;
        }

        // Execute a confirmed AI command in the focused terminal (single frame
        // after the confirmation modal, so PTY resize/settle has happened).
        if let Some(cmd) = tab.pending_ai_command.take() {
            let focused = tab.focused_session;
            if tab.broadcast_enabled {
                for session in &mut tab.sessions {
                    session.write(&cmd);
                    session.write("\r");
                }
            } else if let Some(session) = tab.sessions.get_mut(focused) {
                // Feed the result back to the AI: wrap the command in sentinel
                // markers (shell echo can't match them — they go through
                // printf templates), tap the session's output stream, and let
                // a watcher thread collect the output. The command still runs
                // interactively exactly as before, visible in the terminal.
                // The manual path (Copy button) never taps anything.
                let mut env = session.kind.environment_summary();
                if let Some(cwd) = &session.cwd {
                    env.cwd = cwd.clone();
                }
                let capture = std::sync::Arc::new(std::sync::Mutex::new(
                    crate::terminal::ai_capture::AiCapture::new(),
                ));
                let slot = session.kind.ai_capture_slot();
                session.kind.set_ai_capture(Some(std::sync::Arc::clone(&capture)));
                let wrapped = crate::terminal::ai_capture::wrap_command(&cmd);
                session.write(&wrapped);
                session.write("\r");

                let ai = std::sync::Arc::clone(&tab.ai_chat);
                let llm_config = self.llm_config.clone();
                // The drawer shares the right rail with the tools drawer; the
                // feedback will re-open the AI drawer when it lands.
                tab.tools_drawer_open = false;
                let _ = std::thread::Builder::new()
                    .name("ai-run-feedback".to_string())
                    .spawn(move || {
                        use crate::terminal::ai_capture::FEEDBACK_TIMEOUT_SECS;
                        let deadline = std::time::Instant::now()
                            + std::time::Duration::from_secs(FEEDBACK_TIMEOUT_SECS);
                        let timed_out;
                        loop {
                            let done = capture.lock().map(|c| c.done).unwrap_or(false);
                            if done || std::time::Instant::now() >= deadline {
                                timed_out = !done;
                                break;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(100));
                        }
                        // Detach the tap — but only if no newer Run already
                        // replaced this capture.
                        if let Ok(mut slot) = slot.lock() {
                            if slot.as_ref().is_some_and(|c| std::sync::Arc::ptr_eq(c, &capture)) {
                                *slot = None;
                            }
                        }
                        let (output, exit_code) = match capture.lock() {
                            Ok(mut c) => (c.take_output(), c.exit_code),
                            Err(_) => (String::new(), None),
                        };
                        let feedback = crate::terminal::ai_capture::feedback_text(
                            &cmd, &output, exit_code, timed_out,
                        );
                        let messages = {
                            let mut st = ai.lock().unwrap();
                            st.push_user(feedback);
                            // Surface the reply even if the drawer was closed.
                            st.open = true;
                            st.messages.clone()
                        };
                        crate::ui::state::ai_chat::spawn_chat(
                            llm_config, String::new(), messages, env, ai,
                        );
                        crate::repaint::notify_repaint();
                    });
            }
        }
    }

    /// Handle actions returned from view rendering
    fn handle_view_actions(&mut self, actions: ViewActions, _window_idx: usize, _ctx: &egui::Context) {
        if actions.add_local_tab {
            self.add_tab_local();
        }
        if let Some(host) = actions.connect_ssh {
            self.add_tab_ssh(&host);
        }
        if actions.clear_history {
            self.connection_history.clear();
            config::save_history(&[]);
        }
        if actions.save_hosts {
            config::save_hosts(&self.hosts_file, &self.hosts);
        }
        if actions.save_credentials {
            config::save_credentials(&self.credentials_file, &self.credentials);
        }
        if let Some(idx) = actions.delete_host {
            if idx < self.hosts.len() && !self.hosts[idx].is_local {
                config::delete_host_credentials(&self.hosts[idx]);
                self.hosts.remove(idx);
                self.save_hosts();
            }
        }
        if let Some(id) = &actions.delete_credential {
            self.credentials.retain(|c| c.id != **id);
            config::delete_credential_secrets(id, id.as_str());
            config::save_credentials(&self.credentials_file, &self.credentials);
        }
    }
}
