//! Portal - Modern Terminal Emulator with egui
//! Termius-inspired UI with native terminal input

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
            AppView::Batch => "Batch".to_string(),
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
            }
            // else: no detached windows → let the default close proceed (app exits)
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
                    AppView::Batch => "Batch".to_string(),
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
                            TerminalSession::new_ssh(host, auth, &self.runtime)
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
                            TerminalSession::new_ssh(host, auth, &self.runtime)
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
                        let nav_btn = |ui: &mut egui::Ui, icon: &str, label: &str, active: bool| -> bool {
                            let width = ui.available_width();
                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(width, 36.0), egui::Sense::click(),
                            );
                            let bg = if active {
                                self.theme.accent_alpha(45)
                            } else if resp.hovered() { self.theme.hover_bg } else { egui::Color32::TRANSPARENT };
                            let shadow_color = if active {
                                self.theme.accent_alpha(80)
                            } else if resp.hovered() { self.theme.hover_shadow } else { egui::Color32::TRANSPARENT };
                            ui.painter().rect_filled(
                                egui::Rect::from_min_max(
                                    egui::pos2(rect.min.x, rect.max.y - 1.0), rect.max,
                                ), 0.0, shadow_color,
                            );
                            ui.painter().rect_filled(rect, 0.0, bg);
                            if active {
                                ui.painter().rect_filled(
                                    egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + 3.0, rect.max.y)),
                                    egui::Rounding { nw: 0.0, ne: 2.0, sw: 0.0, se: 2.0 }, self.theme.accent,
                                );
                            }
                            let color = if active || resp.hovered() { self.theme.fg_primary } else { self.theme.fg_dim };
                            ui.painter().text(
                                egui::pos2(rect.min.x + 16.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                format!("{}  {}", icon, label),
                                egui::FontId::proportional(13.0), color,
                            );
                            resp.clicked()
                        };
                        let dw = &mut self.detached_windows[i];
                        if nav_btn(ui, "☰", self.language.t("hosts"), dw.current_view == AppView::Hosts) {
                            dw.current_view = AppView::Hosts;
                        }
                        if nav_btn(ui, ">_", self.language.t("terminal"), dw.current_view == AppView::Terminal) {
                            dw.current_view = AppView::Terminal;
                        }
                        if nav_btn(ui, "\u{2195}", self.language.t("sftp"), dw.current_view == AppView::Sftp) {
                            dw.current_view = AppView::Sftp;
                        }
                        if nav_btn(ui, "\u{1f511}", self.language.t("keychain"), dw.current_view == AppView::Keychain) {
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
                                        if nav_btn(ui, "\u{2699}", self.language.t("settings"), dw.current_view == AppView::Settings) {
                                            dw.current_view = AppView::Settings;
                                        }
                                    },
                                );
                            });
                    });

                let dw = &mut self.detached_windows[i];

                // ── Tab Bar (terminal view) ──
                if dw.current_view == AppView::Terminal {
                    egui::TopBottomPanel::top(egui::Id::new("detached_tab_bar").with(i))
                        .frame(egui::Frame {
                            fill: self.theme.bg_secondary,
                            inner_margin: egui::Margin::symmetric(8.0, 4.0),
                            stroke: egui::Stroke::NONE,
                            ..Default::default()
                        })
                        .show(ctx, |ui| {
                            ui.horizontal(|ui| {
                                // Get the actual tab bar area after horizontal layout
                                let full_tab_bar_rect = ui.max_rect(); // Full tab bar rect for drag-out detection

                                // Scrollable tab area (no scrollbar)
                                egui::ScrollArea::horizontal()
                                    .id_salt("detached_tab_scroll")
                                    .auto_shrink([false, false])
                                    .scroll_bar_visibility(egui::containers::scroll_area::ScrollBarVisibility::AlwaysHidden)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.add_space(4.0);
                                            let mut tab_to_activate: Option<usize> = None;
                                            let mut tab_to_close: Option<usize> = None;
                                            let mut tab_to_detach: Option<usize> = None;
                                            let mut tab_rects: Vec<egui::Rect> = Vec::with_capacity(dw.tabs.len());

                                            // Calculate tab displacement offset with smooth easing
                                            let displacement_offset = if let (Some(target), Some(start_time)) = (dw.tab_drag.target_index, dw.tab_drag.animation_start_time) {
                                                if !dw.tab_drag.is_merge && dw.tab_drag.source_index.is_some() {
                                                    let current_time = ctx.input(|i| i.time);
                                                    let elapsed = (current_time - start_time) as f32;
                                                    let animation_duration = 0.6; // 600ms for smooth animation
                                                    let progress = (elapsed / animation_duration).min(1.0);

                                                    // Enhanced ease-in-out (slow-fast-slow) for smooth tab displacement
                                                    // Using cubic easing for more pronounced slow-start and slow-end
                                                    let t = progress;
                                                    let eased = if t < 0.5 {
                                                        4.0 * t * t * t  // Ease in: slow → fast
                                                    } else {
                                                        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0  // Ease out: fast → slow
                                                    };

                                                    // Use actual tab width
                                                    let tab_width = if dw.tab_drag.ghost_size.x > 0.0 {
                                                        dw.tab_drag.ghost_size.x
                                                    } else {
                                                        150.0
                                                    };

                                                    // Mark animation as completed for this target
                                                    if progress >= 1.0 {
                                                        dw.tab_drag.animation_completed_target = Some(target);
                                                    }

                                                    Some(eased * tab_width)
                                                } else {
                                                    None
                                                }
                                            } else if let (Some(target), Some(completed_target)) = (dw.tab_drag.target_index, dw.tab_drag.animation_completed_target) {
                                                // Animation completed, keep the final displacement
                                                if !dw.tab_drag.is_merge && dw.tab_drag.source_index.is_some() && target == completed_target {
                                                    let tab_width = if dw.tab_drag.ghost_size.x > 0.0 {
                                                        dw.tab_drag.ghost_size.x
                                                    } else {
                                                        150.0
                                                    };
                                                    Some(tab_width)
                                                } else {
                                                    None
                                                }
                                            } else {
                                                None
                                            };

                                            for (ti, tab) in dw.tabs.iter().enumerate() {
                                                let is_active = ti == dw.active_tab;
                                                let is_drag_target = dw.tab_drag.source_index.is_some() && dw.tab_drag.target_index == Some(ti);
                                                let tab_fill = if is_active { self.theme.bg_elevated } else { egui::Color32::TRANSPARENT };

                                                // Apply smooth displacement animation to tabs that should move
                                                if let (Some(target), Some(offset)) = (dw.tab_drag.target_index, displacement_offset) {
                                                    // If this tab should be displaced, add space before it
                                                    let should_displace = if dw.tab_drag.insert_before {
                                                        // Displace target and all tabs after it
                                                        ti >= target
                                                    } else {
                                                        // Displace only tabs after the target
                                                        ti > target
                                                    };
                                                    if should_displace && dw.tab_drag.source_index != Some(ti) {
                                                        ui.add_space(offset);
                                                    }
                                                }

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
                                                        let dot_color = tab.sessions
                                                            .get(tab.focused_session)
                                                            .map(|s| match &s.session {
                                                                Some(sb) if sb.is_connected() => self.theme.green,
                                                                Some(SessionBackend::Ssh(ssh)) => match ssh.connection_state() {
                                                                    SshConnectionState::Connecting | SshConnectionState::Authenticating => self.theme.accent,
                                                                    _ => self.theme.red,
                                                                },
                                                                _ => self.theme.red,
                                                            })
                                                            .unwrap_or(self.theme.fg_dim);
                                                        ui.label(egui::RichText::new("●").color(dot_color).size(8.0));
                                                        if tab.broadcast_enabled {
                                                            ui.label(egui::RichText::new("◉").color(self.theme.accent).size(11.0));
                                                        }
                                                        // Tab title - show focused pane's cwd folder name
                                                        let title_color = if is_active { self.theme.fg_primary } else { self.theme.fg_dim };
                                                        let display_title = tab.sessions
                                                            .get(tab.focused_session)
                                                            .and_then(|s| s.cwd.as_ref())
                                                            .and_then(|cwd| {
                                                                std::path::Path::new(cwd)
                                                                    .file_name()
                                                                    .map(|n| n.to_string_lossy().to_string())
                                                            })
                                                            .unwrap_or_else(|| tab.title.clone());
                                                        ui.label(egui::RichText::new(&display_title).color(title_color).size(13.0));
                                                        if dw.tabs.len() > 1 {
                                                            let close_resp = ui.add(
                                                                egui::Button::new(egui::RichText::new("×").color(self.theme.fg_dim).size(14.0))
                                                                    .frame(false)
                                                            );
                                                            close_btn_rect = Some(close_resp.rect);
                                                        }
                                                    });
                                                });

                                                let tab_rect = tab_resp.response.rect;
                                                tab_rects.push(tab_rect);

                                                if is_drag_target {
                                                    ui.painter().rect_stroke(tab_rect, 8.0, egui::Stroke::new(2.0, self.theme.accent));
                                                }

                                                let sense_resp = ui.interact(tab_rect, egui::Id::new(("detached_tab_drag", i, ti)), egui::Sense::click_and_drag());
                                                if sense_resp.clicked() {
                                                    let click_pos = ui.ctx().input(|inp| inp.pointer.interact_pos());
                                                    let on_close = close_btn_rect.map_or(false, |r| click_pos.map_or(false, |p| r.contains(p)));
                                                    if on_close {
                                                        tab_to_close = Some(ti);
                                                    } else {
                                                        tab_to_activate = Some(ti);
                                                    }
                                                }
                                                if sense_resp.drag_started() {
                                                    dw.tab_drag.source_index = Some(ti);
                                                    dw.tab_drag.ghost_title = tab.title.clone();
                                                    dw.tab_drag.ghost_size = tab_rect.size();
                                                }
                                                // Context menu for detached window tabs (only close tab, drag to detach)
                                                let tab_count = dw.tabs.len();
                                                sense_resp.context_menu(|ui| {
                                                    if ui.add_enabled(tab_count > 1, egui::Button::new(self.language.t("close_tab"))).clicked() {
                                                        tab_to_close = Some(ti);
                                                        ui.close_menu();
                                                    }
                                                });
                                            }

                                            // Draw drag ghost and handle reorder
                                            if let Some(src) = dw.tab_drag.source_index {
                                                if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                                                    // Save previous target for animation reset logic
                                                    let prev_target = dw.tab_drag.target_index;

                                                    dw.tab_drag.target_index = None;
                                                    dw.tab_drag.insert_before = true;
                                                    dw.tab_drag.is_merge = false;

                                                    let mut new_target = None;

                                                    for (ti, rect) in tab_rects.iter().enumerate() {
                                                        if Some(ti) == dw.tab_drag.source_index {
                                                            continue;
                                                        }
                                                        if rect.contains(pos) {
                                                            dw.tab_drag.target_index = Some(ti);
                                                            new_target = Some(ti);
                                                            // Check if in center region (merge) or edge region (reorder)
                                                            let center_x = rect.center().x;
                                                            let edge_threshold = rect.width() * 0.25;
                                                            if pos.x >= rect.min.x + edge_threshold && pos.x <= rect.max.x - edge_threshold {
                                                                // In center region → merge mode
                                                                dw.tab_drag.is_merge = true;
                                                            } else {
                                                                // In edge region → reorder mode
                                                                dw.tab_drag.insert_before = pos.x < center_x;
                                                            }
                                                            break;
                                                        }
                                                    }

                                                    // If not hovering over any tab but still within tab bar area,
                                                    // find the closest insertion point (reorder only)
                                                    if dw.tab_drag.target_index.is_none() && full_tab_bar_rect.contains(pos) {
                                                        let mut closest_idx = None;
                                                        let mut closest_dist = f32::MAX;
                                                        for (ti, rect) in tab_rects.iter().enumerate() {
                                                            if Some(ti) == dw.tab_drag.source_index {
                                                                continue;
                                                            }
                                                            let center_x = rect.center().x;
                                                            let dist = (pos.x - center_x).abs();
                                                            if dist < closest_dist {
                                                                closest_dist = dist;
                                                                closest_idx = Some(ti);
                                                            }
                                                        }
                                                        if let Some(ti) = closest_idx {
                                                            dw.tab_drag.target_index = Some(ti);
                                                            new_target = Some(ti);
                                                            let center_x = tab_rects[ti].center().x;
                                                            dw.tab_drag.insert_before = pos.x < center_x;
                                                        }
                                                    }

                                                    // Reset animation ONLY when target changes from Some(a) to Some(b) where a != b
                                                    // Don't reset when going None -> Some or staying on same target (prevents looping)
                                                    let should_reset = match (prev_target, new_target) {
                                                        (Some(a), Some(b)) if a != b => true,
                                                        _ => false,
                                                    };
                                                    if should_reset {
                                                        dw.tab_drag.animation_start_time = Some(ctx.input(|i| i.time));
                                                        dw.tab_drag.animation_completed_target = None;
                                                    }

                                                    // Draw ghost tab at cursor position
                                                    let ghost_rect = egui::Rect::from_center_size(
                                                        pos,
                                                        dw.tab_drag.ghost_size
                                                    );
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
                                                    let text_pos = egui::pos2(
                                                        ghost_rect.min.x + 12.0,
                                                        ghost_rect.center().y - 7.0
                                                    );
                                                    painter.text(
                                                        text_pos,
                                                        egui::Align2::LEFT_CENTER,
                                                        &dw.tab_drag.ghost_title,
                                                        egui::FontId::new(13.0, egui::FontFamily::Monospace),
                                                        egui::Color32::from_rgba_unmultiplied(220, 228, 255, 180)
                                                    );

                                                    // Draw insertion indicator when in reorder mode
                                                    if let Some(dst_idx) = dw.tab_drag.target_index {
                                                        if !dw.tab_drag.is_merge && dst_idx < tab_rects.len() {
                                                            let indicator_rect = tab_rects[dst_idx];
                                                            let x_pos = if dw.tab_drag.insert_before {
                                                                indicator_rect.min.x
                                                            } else {
                                                                indicator_rect.max.x
                                                            };
                                                            // Animated pulse effect using time
                                                            let time = ctx.input(|i| i.time);
                                                            let pulse = ((time * 3.0).sin() * 0.5 + 0.5) as f32; // 0.0 to 1.0
                                                            let alpha = ((time * 2.0).sin() * 0.3 + 0.7) as u8; // pulsing alpha

                                                            let line_width = 4.0 + pulse * 2.0; // 4.0 to 6.0
                                                            let expand = pulse * 3.0; // 0.0 to 3.0

                                                            let line_rect = egui::Rect::from_min_max(
                                                                egui::pos2(x_pos - line_width / 2.0 - expand, indicator_rect.min.y - 4.0 - expand),
                                                                egui::pos2(x_pos + line_width / 2.0 + expand, indicator_rect.max.y + 4.0 + expand),
                                                            );

                                                            // Draw outer glow with pulsing alpha
                                                            painter.rect_filled(
                                                                line_rect.expand(3.0),
                                                                egui::Rounding::same(4.0),
                                                                egui::Color32::from_rgba_unmultiplied(self.theme.accent.r(), self.theme.accent.g(), self.theme.accent.b(), (alpha as f32 / 255.0 * 80.0) as u8)
                                                            );
                                                            // Draw middle glow
                                                            painter.rect_filled(
                                                                line_rect.expand(1.0),
                                                                egui::Rounding::same(3.0),
                                                                egui::Color32::from_rgba_unmultiplied(self.theme.accent.r(), self.theme.accent.g(), self.theme.accent.b(), (alpha as f32 / 255.0 * 150.0) as u8)
                                                            );
                                                            // Draw main indicator line with full opacity
                                                            painter.rect_filled(
                                                                line_rect,
                                                                egui::Rounding::same(2.0),
                                                                egui::Color32::from_rgba_unmultiplied(self.theme.accent.r(), self.theme.accent.g(), self.theme.accent.b(), alpha)
                                                            );
                                                        }
                                                    }
                                                }

                                                if ctx.input(|i| i.pointer.any_released()) {
                                                    if let Some(dst) = dw.tab_drag.target_index {
                                                        if src != dst && src < dw.tabs.len() && dst < dw.tabs.len() {
                                                            if dw.tab_drag.is_merge {
                                                                // Merge mode: combine the two tabs
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
                                                            } else {
                                                                // Reorder mode: move the tab
                                                                let src_tab = dw.tabs.remove(src);
                                                                let insert_idx = if dw.tab_drag.insert_before {
                                                                    if src < dst {
                                                                        dst.saturating_sub(1)
                                                                    } else {
                                                                        dst
                                                                    }
                                                                } else {
                                                                    if src < dst {
                                                                        dst
                                                                    } else {
                                                                        (dst + 1).min(dw.tabs.len())
                                                                    }
                                                                };
                                                                dw.tabs.insert(insert_idx, src_tab);

                                                                // After reorder, set the dragged tab as active
                                                                dw.active_tab = insert_idx;
                                                            }
                                                        }
                                                    } else {
                                                        // Dropped outside tab area → detach to new window
                                                        if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                                                            if !full_tab_bar_rect.contains(pos) && src < dw.tabs.len() {
                                                                tab_to_detach = Some(src);
                                                            }
                                                        }
                                                    }
                                                    dw.tab_drag.source_index = None;
                                                    dw.tab_drag.target_index = None;
                                                    dw.tab_drag.animation_start_time = None;
                                                    dw.tab_drag.animation_completed_target = None;
                                                }
                                            }

                                            // Update last_tab_rects for next frame
                                            dw.last_tab_rects = tab_rects.clone();

                                            if let Some(ti) = tab_to_activate { dw.active_tab = ti; }
                                            if let Some(ti) = tab_to_detach {
                                                if dw.tabs.len() > 1 {
                                                    pending_detach.push((i, ti));
                                                }
                                            }
                                            if let Some(ti) = tab_to_close {
                                                if dw.tabs.len() > 1 {
                                                    dw.tabs.remove(ti);
                                                    if dw.active_tab >= dw.tabs.len() {
                                                        dw.active_tab = dw.tabs.len() - 1;
                                                    } else if dw.active_tab > ti {
                                                        dw.active_tab -= 1;
                                                    }
                                                }
                                            }

                                            // New tab button (+) - right after last tab, inside scroll area
                                            ui.add_space(4.0);
                                            if ui.add(
                                                egui::Button::new(egui::RichText::new("+").color(self.theme.fg_dim).size(16.0))
                                                    .frame(false)
                                            ).clicked() {
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
                                        });
                                    });

                                // ── More menu (⋯) at far right of detached tab bar ──
                                let current_tab_broadcast_on = dw.tabs[dw.active_tab].broadcast_enabled;
                                let dw_toggle_id = egui::Id::new("dw_broadcast_toggle").with(i);
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let more_menu_id = egui::Id::new("dw_tab_bar_more_menu").with(i);
                                    let show_menu = ctx.data_mut(|d| *d.get_temp_mut_or_default::<bool>(more_menu_id));
                                    let btn_color = if show_menu { self.theme.accent } else { self.theme.fg_dim };
                                    let more_resp = ui.add(
                                        egui::Button::new(egui::RichText::new("⋯").color(btn_color).size(16.0))
                                            .frame(false)
                                    );
                                    if more_resp.clicked() {
                                        ctx.data_mut(|d| d.insert_temp(more_menu_id, !show_menu));
                                    }

                                    if show_menu {
                                        let popup_pos = egui::pos2(more_resp.rect.min.x, more_resp.rect.max.y + 2.0);
                                        let area_resp = egui::Area::new(more_menu_id.with("popup"))
                                            .order(egui::Order::Foreground)
                                            .fixed_pos(popup_pos)
                                            .show(ctx, |ui| {
                                                egui::Frame {
                                                    fill: self.theme.bg_elevated,
                                                    rounding: egui::Rounding::same(6.0),
                                                    inner_margin: egui::Margin::same(4.0),
                                                    stroke: egui::Stroke::new(1.0, self.theme.border),
                                                    ..Default::default()
                                                }
                                                .show(ui, |ui| {
                                                    ui.set_min_width(200.0);
                                                    let broadcast_label = if current_tab_broadcast_on {
                                                        format!("◉ {}  ⌘⇧I", self.language.t("broadcast_off"))
                                                    } else {
                                                        format!("○ {}  ⌘⇧I", self.language.t("broadcast_on"))
                                                    };
                                                    let btn = ui.add(
                                                        egui::Button::new(
                                                            egui::RichText::new(&broadcast_label)
                                                                .color(if current_tab_broadcast_on { self.theme.accent } else { self.theme.fg_primary })
                                                                .size(13.0)
                                                        )
                                                        .frame(false)
                                                    );
                                                    if btn.clicked() {
                                                        ctx.data_mut(|d| d.insert_temp(dw_toggle_id, true));
                                                        ctx.data_mut(|d| d.insert_temp(more_menu_id, false));
                                                    }
                                                });
                                            });

                                        if ctx.input(|i| i.pointer.any_pressed()) {
                                            if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                                                if !area_resp.response.rect.contains(pos) && !more_resp.rect.contains(pos) {
                                                    ctx.data_mut(|d| d.insert_temp(more_menu_id, false));
                                                }
                                            }
                                        }
                                    }
                                });

                                // Apply deferred broadcast toggle
                                let should_toggle: bool = ctx.data_mut(|d| {
                                    let v = *d.get_temp_mut_or_default::<bool>(dw_toggle_id);
                                    if v { d.insert_temp(dw_toggle_id, false); }
                                    v
                                });
                                if should_toggle {
                                    dw.tabs[dw.active_tab].broadcast_enabled = !dw.tabs[dw.active_tab].broadcast_enabled;
                                }

                            });
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
                        .exact_height(24.0)
                        .frame(egui::Frame {
                            fill: self.theme.bg_secondary,
                            inner_margin: egui::Margin::symmetric(12.0, 0.0),
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
                if dw_view == AppView::Hosts {
                    self.show_add_host_drawer(ctx);
                }

                // ── Central Panel ──
                let dw_view = self.detached_windows[i].current_view;
                egui::CentralPanel::default()
                    .frame(egui::Frame {
                        fill: self.theme.bg_primary,
                        inner_margin: egui::Margin::same(0.0),
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
                                                TerminalSession::new_ssh(host, auth, &self.runtime)
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
                                                TerminalSession::new_ssh(host, auth, &self.runtime)
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
                                                dw.tabs[active].sessions[idx].reconnect_ssh(&self.runtime);
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
                            AppView::Batch => {
                                self.check_batch_execution_updates();
                                self.show_batch_page(ctx);
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
                        last_tab_rects: Vec::new(),
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
        }

        // ── Nav panel (narrow, always shown first to get full height) ──
        self.show_nav_panel(ctx);

        // ── Tab Bar (only in terminal view) ──────────────────────────────────────
        if self.current_view == AppView::Terminal {
        egui::TopBottomPanel::top("tab_bar")
            .frame(egui::Frame {
                fill: self.theme.bg_secondary,
                inner_margin: egui::Margin::symmetric(8.0, 4.0),
                stroke: egui::Stroke::NONE,
                ..Default::default()
            })
            .show(ctx, |ui| {
                // Reserve space for + and ⋯ buttons on the right
                let buttons_width = 60.0;
                let available_width = ui.available_width();

                ui.horizontal(|ui| {
                    ui.add_space(4.0);

                    // Get the actual tab bar area after horizontal layout
                    let full_tab_bar_rect = ui.max_rect(); // Full tab bar rect for drag-out detection

                    // Scrollable tab area - max_width ensures buttons won't be overlapped
                    let tab_area_width = (available_width - buttons_width).max(100.0);

                    egui::ScrollArea::horizontal()
                        .id_salt("tab_scroll")
                        .auto_shrink([false, false])
                        .max_width(tab_area_width)
                        .scroll_bar_visibility(egui::containers::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                // Tab buttons — one per workspace (with drag-to-reorder)
                                let mut tab_to_activate: Option<usize> = None;
                                let mut tab_to_close: Option<usize> = None;
                                let mut tab_to_reconnect: Option<usize> = None;
                                let mut tab_to_detach: Option<usize> = None;
                                let mut tab_rects: Vec<egui::Rect> = Vec::with_capacity(self.tabs.len());

                                // Calculate tab displacement offset with smooth easing
                                let displacement_offset = if let (Some(target), Some(start_time)) = (self.tab_drag.target_index, self.tab_drag.animation_start_time) {
                                    if !self.tab_drag.is_merge && self.tab_drag.source_index.is_some() {
                                        let current_time = ctx.input(|i| i.time);
                                        let elapsed = (current_time - start_time) as f32;
                                        let animation_duration = 0.6; // 600ms for smooth animation
                                        let progress = (elapsed / animation_duration).min(1.0);

                                        // Enhanced ease-in-out (slow-fast-slow) for smooth tab displacement
                                        // Using cubic easing for more pronounced slow-start and slow-end
                                        let t = progress;
                                        let eased = if t < 0.5 {
                                            4.0 * t * t * t  // Ease in: slow → fast
                                        } else {
                                            1.0 - (-2.0 * t + 2.0).powi(3) / 2.0  // Ease out: fast → slow
                                        };

                                        // Use actual tab width
                                        let tab_width = if self.tab_drag.ghost_size.x > 0.0 {
                                            self.tab_drag.ghost_size.x
                                        } else {
                                            150.0
                                        };

                                        // Mark animation as completed for this target
                                        if progress >= 1.0 {
                                            self.tab_drag.animation_completed_target = Some(target);
                                        }

                                        Some(eased * tab_width)
                                    } else {
                                        None
                                    }
                                } else if let (Some(target), Some(completed_target)) = (self.tab_drag.target_index, self.tab_drag.animation_completed_target) {
                                    // Animation completed, keep the final displacement
                                    if !self.tab_drag.is_merge && self.tab_drag.source_index.is_some() && target == completed_target {
                                        let tab_width = if self.tab_drag.ghost_size.x > 0.0 {
                                            self.tab_drag.ghost_size.x
                                        } else {
                                            150.0
                                        };
                                        Some(tab_width)
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                };

                                for (i, tab) in self.tabs.iter().enumerate() {
                                    let is_active = i == self.active_tab;
                                    let is_drag_target = self.tab_drag.source_index.is_some() && self.tab_drag.target_index == Some(i);
                                    let is_broadcasting = tab.broadcast_enabled;

                                    // Apply smooth displacement animation to tabs that should move
                                    if let (Some(target), Some(offset)) = (self.tab_drag.target_index, displacement_offset) {
                                        // If this tab should be displaced, add space before it
                                        let should_displace = if self.tab_drag.insert_before {
                                            // Displace target and all tabs after it
                                            i >= target
                                        } else {
                                            // Displace only tabs after the target
                                            i > target
                                        };
                                        if should_displace && self.tab_drag.source_index != Some(i) {
                                            ui.add_space(offset);
                                        }
                                    }

                                    let tab_fill = if is_active {
                                        self.theme.bg_elevated
                                    } else if is_broadcasting {
                                        // Broadcast mode: highlight with a distinct color
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

                                            // Status dot based on focused session in this workspace
                                            let dot_color = tab.sessions
                                                .get(tab.focused_session)
                                                .map(|s| match &s.session {
                                                    Some(sb) if sb.is_connected() => self.theme.green,
                                                    Some(SessionBackend::Ssh(ssh)) => match ssh.connection_state() {
                                                        SshConnectionState::Connecting | SshConnectionState::Authenticating => self.theme.accent,
                                                        _ => self.theme.red,
                                                    },
                                                    _ => self.theme.red,
                                                })
                                                .unwrap_or(self.theme.fg_dim);
                                            ui.label(egui::RichText::new("●").color(dot_color).size(8.0));

                                            // Broadcast indicator
                                            if is_broadcasting {
                                                ui.label(egui::RichText::new("◉").color(self.theme.accent).size(11.0));
                                            }

                                            // Tab title - show focused pane's cwd folder name
                                            let title_color = if is_active { self.theme.fg_primary } else { self.theme.fg_dim };
                                            let display_title = tab.sessions
                                                .get(tab.focused_session)
                                                .and_then(|s| s.cwd.as_ref())
                                                .and_then(|cwd| {
                                                    // Extract just the folder name from the path
                                                    std::path::Path::new(cwd)
                                                        .file_name()
                                                        .map(|n| n.to_string_lossy().to_string())
                                                })
                                                .unwrap_or_else(|| tab.title.clone());
                                            ui.label(egui::RichText::new(&display_title).color(title_color).size(13.0));

                                            // Close button (only when more than one tab)
                                            if self.tabs.len() > 1 {
                                                let close_resp = ui.add(
                                                    egui::Button::new(
                                                        egui::RichText::new("×").color(self.theme.fg_dim).size(14.0)
                                                    )
                                                    .frame(false)
                                                );
                                                close_btn_rect = Some(close_resp.rect);
                                            }
                                        });
                                    });

                                    let tab_rect = tab_resp.response.rect;
                                    tab_rects.push(tab_rect);

                                    // Draw merge indicator (highlight target tab)
                                    if is_drag_target {
                                        ui.painter().rect_stroke(
                                            tab_rect,
                                            8.0,
                                            egui::Stroke::new(2.0, self.theme.accent),
                                        );
                                    }

                                    // Interact for click and drag
                                    let sense_resp = ui.interact(tab_rect, egui::Id::new(("tab_drag", i)), egui::Sense::click_and_drag());
                                    if sense_resp.clicked() {
                                        let click_pos = ui.ctx().input(|inp| inp.pointer.interact_pos());
                                        let on_close = close_btn_rect.map_or(false, |r| click_pos.map_or(false, |p| r.contains(p)));
                                        if on_close {
                                            tab_to_close = Some(i);
                                        } else {
                                            tab_to_activate = Some(i);
                                            if tab.sessions
                                                .get(tab.focused_session)
                                                .map(|s| s.needs_reconnect())
                                                .unwrap_or(false)
                                            {
                                                tab_to_reconnect = Some(i);
                                            }
                                        }
                                    }
                                    if sense_resp.drag_started() {
                                        self.tab_drag.source_index = Some(i);
                                        self.tab_drag.ghost_title = tab.title.clone();
                                        self.tab_drag.ghost_size = tab_rect.size();
                                    }
                                    // Tab context menu (close tab only)
                                    let tab_count = self.tabs.len();
                                    let tab_idx = i;
                                    sense_resp.context_menu(|ui| {
                                        if ui.add_enabled(tab_count > 1, egui::Button::new(self.language.t("close_tab"))).clicked() {
                                            tab_to_close = Some(tab_idx);
                                            ui.close_menu();
                                        }
                                    });
                                }

                                // Draw drag ghost and handle reorder
                                if let Some(src) = self.tab_drag.source_index {
                                    if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                                        // Save previous target for animation reset logic
                                        let prev_target = self.tab_drag.target_index;

                                        self.tab_drag.target_index = None;
                                        self.tab_drag.insert_before = true;
                                        self.tab_drag.is_merge = false;

                                        let mut new_target = None;

                                        for (i, rect) in tab_rects.iter().enumerate() {
                                            if Some(i) == self.tab_drag.source_index {
                                                continue;
                                            }
                                            if rect.contains(pos) {
                                                self.tab_drag.target_index = Some(i);
                                                new_target = Some(i);
                                                // Check if in center region (merge) or edge region (reorder)
                                                // Center 50% = merge, edges 25% each = reorder
                                                let center_x = rect.center().x;
                                                let edge_threshold = rect.width() * 0.25;
                                                if pos.x >= rect.min.x + edge_threshold && pos.x <= rect.max.x - edge_threshold {
                                                    // In center region → merge mode
                                                    self.tab_drag.is_merge = true;
                                                } else {
                                                    // In edge region → reorder mode
                                                    self.tab_drag.insert_before = pos.x < center_x;
                                                }
                                                break;
                                            }
                                        }

                                        // If not hovering over any tab but still within tab bar area,
                                        // find the closest insertion point (reorder only)
                                        if self.tab_drag.target_index.is_none() && full_tab_bar_rect.contains(pos) {
                                            // Find which tab is closest horizontally
                                            let mut closest_idx = None;
                                            let mut closest_dist = f32::MAX;
                                            for (i, rect) in tab_rects.iter().enumerate() {
                                                if Some(i) == self.tab_drag.source_index {
                                                    continue;
                                                }
                                                // Calculate distance to tab center
                                                let center_x = rect.center().x;
                                                let dist = (pos.x - center_x).abs();
                                                if dist < closest_dist {
                                                    closest_dist = dist;
                                                    closest_idx = Some(i);
                                                }
                                            }
                                            if let Some(i) = closest_idx {
                                                self.tab_drag.target_index = Some(i);
                                                new_target = Some(i);
                                                // Determine insert_before based on which side of center we're on
                                                let center_x = tab_rects[i].center().x;
                                                self.tab_drag.insert_before = pos.x < center_x;
                                            }
                                        }

                                        // Reset animation ONLY when target changes from Some(a) to Some(b) where a != b
                                        // Don't reset when going None -> Some or staying on same target (prevents looping)
                                        let should_reset = match (prev_target, new_target) {
                                            (Some(a), Some(b)) if a != b => true,
                                            _ => false,
                                        };
                                        if should_reset {
                                            self.tab_drag.animation_start_time = Some(ctx.input(|i| i.time));
                                            self.tab_drag.animation_completed_target = None;
                                        }

                                        // Draw ghost tab at cursor position
                                        let ghost_rect = egui::Rect::from_center_size(
                                            pos,
                                            self.tab_drag.ghost_size
                                        );
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
                                        let text_pos = egui::pos2(
                                            ghost_rect.min.x + 12.0,
                                            ghost_rect.center().y - 7.0
                                        );
                                        painter.text(
                                            text_pos,
                                            egui::Align2::LEFT_CENTER,
                                            &self.tab_drag.ghost_title,
                                            egui::FontId::new(13.0, egui::FontFamily::Monospace),
                                            egui::Color32::from_rgba_unmultiplied(220, 228, 255, 180)
                                        );

                                        // Draw insertion indicator (vertical line) when in reorder mode
                                        if let Some(dst_idx) = self.tab_drag.target_index {
                                            if !self.tab_drag.is_merge && dst_idx < tab_rects.len() {
                                                let indicator_rect = tab_rects[dst_idx];
                                                let x_pos = if self.tab_drag.insert_before {
                                                    indicator_rect.min.x
                                                } else {
                                                    indicator_rect.max.x
                                                };

                                                // Animated pulse effect using time
                                                let time = ctx.input(|i| i.time);
                                                let pulse = ((time * 3.0).sin() * 0.5 + 0.5) as f32; // 0.0 to 1.0
                                                let alpha = ((time * 2.0).sin() * 0.3 + 0.7) as u8; // pulsing alpha

                                                let line_width = 4.0 + pulse * 2.0; // 4.0 to 6.0
                                                let expand = pulse * 3.0; // 0.0 to 3.0

                                                let line_rect = egui::Rect::from_min_max(
                                                    egui::pos2(x_pos - line_width / 2.0 - expand, indicator_rect.min.y - 4.0 - expand),
                                                    egui::pos2(x_pos + line_width / 2.0 + expand, indicator_rect.max.y + 4.0 + expand),
                                                );

                                                // Draw outer glow with pulsing alpha
                                                painter.rect_filled(
                                                    line_rect.expand(3.0),
                                                    egui::Rounding::same(4.0),
                                                    egui::Color32::from_rgba_unmultiplied(self.theme.accent.r(), self.theme.accent.g(), self.theme.accent.b(), (alpha as f32 / 255.0 * 80.0) as u8)
                                                );
                                                // Draw middle glow
                                                painter.rect_filled(
                                                    line_rect.expand(1.0),
                                                    egui::Rounding::same(3.0),
                                                    egui::Color32::from_rgba_unmultiplied(self.theme.accent.r(), self.theme.accent.g(), self.theme.accent.b(), (alpha as f32 / 255.0 * 150.0) as u8)
                                                );
                                                // Draw main indicator line with full opacity
                                                painter.rect_filled(
                                                    line_rect,
                                                    egui::Rounding::same(2.0),
                                                    egui::Color32::from_rgba_unmultiplied(self.theme.accent.r(), self.theme.accent.g(), self.theme.accent.b(), alpha)
                                                );
                                            }
                                        }
                                    }

                                    // Handle drop → merge, reorder, or detach to new window if outside
                                    if ctx.input(|i| i.pointer.any_released()) {
                                        if let Some(dst) = self.tab_drag.target_index {
                                            if src != dst && src < self.tabs.len() && dst < self.tabs.len() {
                                                if self.tab_drag.is_merge {
                                                    // Merge mode: combine the two tabs into one with split panes
                                                    let mut src_tab = self.tabs.remove(src);
                                                    // Adjust dst index after removal
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
                                                    // Update active_tab
                                                    if self.active_tab == src {
                                                        self.active_tab = dst;
                                                    } else if self.active_tab > src && self.active_tab > 0 {
                                                        self.active_tab -= 1;
                                                    }
                                                    if self.active_tab >= self.tabs.len() {
                                                        self.active_tab = self.tabs.len().saturating_sub(1);
                                                    }
                                                } else {
                                                    // Reorder mode: move the tab to a new position
                                                    let src_tab = self.tabs.remove(src);

                                                    // Calculate insertion index
                                                    let insert_idx = if self.tab_drag.insert_before {
                                                        if src < dst {
                                                            dst.saturating_sub(1)
                                                        } else {
                                                            dst
                                                        }
                                                    } else {
                                                        if src < dst {
                                                            dst
                                                        } else {
                                                            (dst + 1).min(self.tabs.len())
                                                        }
                                                    };

                                                    self.tabs.insert(insert_idx, src_tab);

                                                    // After reorder, set the dragged tab as active
                                                    self.active_tab = insert_idx;
                                                }
                                            }
                                        } else {
                                            // Dropped outside tab area → detach to new window
                                            if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                                                if !full_tab_bar_rect.contains(pos) && src < self.tabs.len() {
                                                    tab_to_detach = Some(src);
                                                }
                                            }
                                        }
                                        self.tab_drag.source_index = None;
                                        self.tab_drag.target_index = None;
                                        self.tab_drag.animation_start_time = None;
                                        self.tab_drag.animation_completed_target = None;
                                    }
                                }

                                // Update last_tab_rects for next frame
                                self.last_tab_rects = tab_rects.clone();

                                // Apply deferred tab actions
                                if let Some(i) = tab_to_activate {
                                    self.active_tab = i;
                                }
                                if let Some(i) = tab_to_reconnect {
                                    let si = self.tabs[i].focused_session;
                                    self.tabs[i].sessions[si].reconnect_ssh(&self.runtime);
                                }
                                if let Some(i) = tab_to_detach {
                                    self.detach_tab(i);
                                }
                                if let Some(i) = tab_to_close {
                                    if self.tabs.len() > 1 {
                                        self.tabs.remove(i);
                                        if self.active_tab >= self.tabs.len() {
                                            self.active_tab = self.tabs.len() - 1;
                                        } else if self.active_tab > i {
                                            self.active_tab -= 1;
                                        }
                                    }
                                }

                                // New tab button (+) - right after last tab, inside scroll area
                                ui.add_space(4.0);
                                if ui.add(
                                    egui::Button::new(egui::RichText::new("+").color(self.theme.fg_dim).size(16.0))
                                        .frame(false)
                                ).clicked() {
                                    self.add_tab_local();
                                }
                            });
                        });

                    // ── More menu (⋯) at far right ──
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let more_menu_id = egui::Id::new("tab_bar_more_menu");
                        let show_menu = ctx.data_mut(|d| *d.get_temp_mut_or_default::<bool>(more_menu_id));
                        let btn_color = if show_menu { self.theme.accent } else { self.theme.fg_dim };
                        let more_resp = ui.add(
                            egui::Button::new(egui::RichText::new("⋯").color(btn_color).size(16.0))
                                .frame(false)
                        );
                        if more_resp.clicked() {
                            ctx.data_mut(|d| d.insert_temp(more_menu_id, !show_menu));
                        }

                        if show_menu {
                            let popup_pos = egui::pos2(more_resp.rect.min.x, more_resp.rect.max.y + 2.0);
                            let area_resp = egui::Area::new(more_menu_id.with("popup"))
                                .order(egui::Order::Foreground)
                                .fixed_pos(popup_pos)
                                .show(ctx, |ui| {
                                    egui::Frame {
                                        fill: self.theme.bg_elevated,
                                        rounding: egui::Rounding::same(6.0),
                                        inner_margin: egui::Margin::same(8.0),
                                        stroke: egui::Stroke::new(1.0, self.theme.border),
                                        ..Default::default()
                                    }
                                    .show(ui, |ui| {
                                        ui.set_min_width(200.0);
                                        ui.separator();
                                        ui.add_space(4.0);
                                        let current_tab_broadcast = if let Some(tab) = self.tabs.get(self.active_tab) {
                                            tab.broadcast_enabled
                                        } else {
                                            false
                                        };
                                        let broadcast_label = if current_tab_broadcast {
                                            format!("◉ {}  ⌘⇧I", self.language.t("broadcast_off"))
                                        } else {
                                            format!("○ {}  ⌘⇧I", self.language.t("broadcast_on"))
                                        };
                                        if ui.add(
                                            egui::Button::new(
                                                egui::RichText::new(&broadcast_label)
                                                    .color(if current_tab_broadcast { self.theme.accent } else { self.theme.fg_primary })
                                                    .size(13.0)
                                            )
                                            .frame(false)
                                        ).clicked() {
                                            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                                                tab.broadcast_enabled = !tab.broadcast_enabled;
                                            }
                                            ctx.data_mut(|d| d.insert_temp(more_menu_id, false));
                                        }
                                    });
                                });

                            if ctx.input(|i| i.pointer.any_pressed()) {
                                if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                                    if !area_resp.response.rect.contains(pos) && !more_resp.rect.contains(pos) {
                                        ctx.data_mut(|d| d.insert_temp(more_menu_id, false));
                                    }
                                }
                            }
                        }
                    });
                });
            });
        } // end if Terminal tab bar

        // ── Add/Edit Host Drawer (right panel, Hosts view only, before CentralPanel) ──
        if self.current_view == AppView::Hosts {
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

        // ── Status Bar (bottom, terminal view only) ────────────────────────
        if self.current_view == AppView::Terminal {
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

            let is_local_session = conn_type == "Local";
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

            let sep_color = self.theme.border;
            let conn_color = if conn_type == "Local" { self.theme.green } else { self.theme.accent };

            egui::TopBottomPanel::bottom("status_bar")
                .exact_height(24.0)
                .frame(egui::Frame {
                    fill: self.theme.bg_secondary,
                    inner_margin: egui::Margin::symmetric(12.0, 0.0),
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

                        // Connection type
                        status_btn(ui, &conn_type, conn_color);

                        // Broadcast indicator
                        let is_broadcasting = self.tabs.get(self.active_tab)
                            .map(|t| t.broadcast_enabled)
                            .unwrap_or(false);
                        if is_broadcasting {
                            ui.add_space(12.0);
                            ui.label(egui::RichText::new("|").color(sep_color).size(12.0));
                            ui.add_space(12.0);
                            ui.label(egui::RichText::new(self.language.t("broadcast")).color(self.theme.accent).size(12.0));
                        }

                        // Shell
                        ui.add_space(12.0);
                        ui.label(egui::RichText::new("|").color(sep_color).size(12.0));
                        ui.add_space(12.0);
                        // Shell display (non-interactive)
                        ui.label(egui::RichText::new(&shell_label)
                            .color(if is_local_session { self.theme.fg_primary } else { self.theme.fg_dim })
                            .size(12.0));

                        // Encoding
                        ui.add_space(12.0);
                        ui.label(egui::RichText::new("|").color(sep_color).size(12.0));
                        ui.add_space(12.0);
                        // Encoding display (non-interactive)
                        ui.label(egui::RichText::new(&encoding_label)
                            .color(self.theme.fg_dim)
                            .size(12.0));

                        // Uptime
                        if !uptime_label.is_empty() {
                            ui.add_space(12.0);
                            ui.label(egui::RichText::new("|").color(sep_color).size(12.0));
                            ui.add_space(12.0);
                            ui.label(egui::RichText::new(&uptime_label).color(self.theme.fg_dim).size(12.0));
                        }

                        // Detached window count
                        let n = self.detached_windows.len();
                        if n > 0 {
                            ui.add_space(12.0);
                            ui.label(egui::RichText::new("|").color(sep_color).size(12.0));
                            ui.add_space(12.0);
                            ui.label(egui::RichText::new(format!("Detached: {}", n))
                                .color(self.theme.green).size(12.0));
                        }
                });
                });
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

        // ── Central Panel: Hosts page or Terminal ──────────────────────────
        egui::CentralPanel::default()
            .frame(egui::Frame {
                fill: self.theme.bg_primary,
                inner_margin: egui::Margin::same(0.0),
                ..Default::default()
            })
            .show(ctx, |ui| {
                match self.current_view {
                    AppView::Hosts => {
                        self.show_hosts_page(ctx, ui);
                    }

                    AppView::Terminal => {
                        // ── Terminal pane tree ──────────────────────────
                        ui.style_mut().spacing.item_spacing = egui::vec2(0.0, 0.0);
                        // Get available rect and subtract status bar height (24px)
                        // Note: egui processes TopBottomPanel::bottom AFTER CentralPanel,
                        // so available_rect_before_wrap() includes the status bar area
                        let mut available = ui.available_rect_before_wrap();
                        available.max.y -= 24.0;
                        let active = self.active_tab;
                        let focused = self.tabs[active].focused_session;
                        let can_close = self.tabs.len() > 1 || self.tabs[active].sessions.len() > 1;
                        let pane_result = {
                            let tab = &mut self.tabs[active];
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
                                &mut self.ime_composing,
                                &mut self.ime_preedit,
                                can_close,
                                &self.theme,
                                self.font_size,
                                &self.language,
                            )
                        };
                        if let Some((idx, action, input_bytes)) = pane_result {
                            self.tabs[active].focused_session = idx;
                            // Broadcast input to all sessions in current tab if broadcast enabled
                            if self.tabs[active].broadcast_enabled && !input_bytes.is_empty() {
                                for (sess_idx, session) in self.tabs[active].sessions.iter_mut().enumerate() {
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
                                PaneAction::SplitHorizontal => self.split_focused_pane(SplitDirection::Horizontal),
                                PaneAction::SplitVertical   => self.split_focused_pane(SplitDirection::Vertical),
                                PaneAction::ClosePane       => self.close_pane(idx),
                                PaneAction::ToggleBroadcast => {
                                    self.tabs[active].broadcast_enabled = !self.tabs[active].broadcast_enabled;
                                }
                                PaneAction::RemoveHostKey => {
                                    // Remove old SSH host key and reconnect
                                    if let Some(host) = self.tabs[active].sessions.get(idx).and_then(|s| s.ssh_host.clone()) {
                                        let _ = crate::ssh::remove_known_hosts_key(&host.host, host.port);
                                        // Reconnect the SSH session
                                        self.tabs[active].sessions[idx].reconnect_ssh(&self.runtime);
                                    }
                                }
                            }
                        }
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

                    AppView::Batch => {
                        self.check_batch_execution_updates();
                        self.show_batch_page(ctx);
                    }
                }
            });
    }
}

fn load_app_icon() -> Option<egui::IconData> {
    let png_bytes = include_bytes!("../assets/portal-icon-1024.png");
    let decoder = eframe::icon_data::from_png_bytes(png_bytes);
    decoder.ok()
}

fn main() -> eframe::Result<()> {
    env_logger::init();

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
