//! Portal - Modern Terminal Emulator with egui
//! Termius-inspired UI with native terminal input

mod config;
mod sftp;
mod ssh;
mod terminal;
mod ui;

use eframe::egui;
use std::path::PathBuf;
use std::time::Duration;

use config::HostEntry;
use sftp::{LocalBrowser, SftpBrowser, SftpConnectionState};
use ssh::SshConnectionState;

use ui::*;
use ui::types::BatchExecutionState;

struct PortalApp {
    tabs: Vec<Tab>,
    active_tab: usize,
    current_view: AppView,
    hosts: Vec<HostEntry>,
    hosts_file: PathBuf,
    next_id: usize,
    add_host_dialog: AddHostDialog,
    host_to_delete: Option<usize>,
    confirm_delete_host: Option<usize>,
    ime_composing: bool,
    ime_preedit: String,
    runtime: tokio::runtime::Runtime,
    // SFTP browser
    sftp_browser_left: Option<SftpBrowser>,  // Left panel SFTP connection
    sftp_browser: Option<SftpBrowser>,       // Right panel SFTP connection
    local_browser: LocalBrowser,
    left_panel_is_local: bool,               // true = local, false = remote
    sftp_context_menu: Option<SftpContextMenu>,
    sftp_rename_dialog: Option<SftpRenameDialog>,
    sftp_new_folder_dialog: Option<SftpNewFolderDialog>,
    sftp_new_file_dialog: Option<SftpNewFileDialog>,
    sftp_confirm_delete: Option<SftpConfirmDelete>,
    sftp_editor_dialog: Option<SftpEditorDialog>,
    sftp_error_dialog: Option<SftpErrorDialog>,
    sftp_local_refresh_start: Option<std::time::Instant>,
    sftp_remote_refresh_start: Option<std::time::Instant>,
    sftp_active_panel_is_local: bool,
    // Tab drag state
    tab_drag: TabDragState,
    // Status bar pickers
    available_shells: Vec<String>,
    selected_shell: String,
    selected_encoding: String,
    show_shell_picker: bool,
    show_encoding_picker: bool,
    // Detached tab windows
    detached_windows: Vec<DetachedWindow>,
    next_viewport_id: u32,
    // Main window hidden (still running for detached windows)
    main_window_hidden: bool,
    // Broadcast state
    broadcast_state: BroadcastState,
    // Batch execution state
    batch_execution: BatchExecutionState,
    // Keychain delete confirmation
    keychain_confirm_delete: Option<KeychainDeleteRequest>,
    // Settings
    theme: ThemeColors,
    theme_preset: ThemePreset,
    language: Language,
    font_size: f32,
    custom_font_path: String,
    fonts_dirty: bool,
    visuals_dirty: bool,
}

impl PortalApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Load settings
        let settings = config::load_settings();
        let theme_preset = ThemePreset::from_id(&settings.theme_preset);
        let language = Language::from_id(&settings.language);
        let font_size = settings.font_size;
        let custom_font_path = settings.custom_font_path.clone().unwrap_or_default();
        let theme = theme_preset.colors();

        let mut fonts = egui::FontDefinitions::default();

        // Load custom font if specified
        if !custom_font_path.is_empty() {
            if let Ok(font_data) = std::fs::read(&custom_font_path) {
                fonts.font_data.insert(
                    "CustomFont".to_owned(),
                    egui::FontData::from_owned(font_data),
                );
                fonts.families
                    .entry(egui::FontFamily::Monospace)
                    .or_insert_with(Vec::new)
                    .insert(0, "CustomFont".to_owned());
            }
        }

        #[cfg(target_os = "macos")]
        {
            if let Ok(font_data) = std::fs::read("/System/Library/Fonts/Monaco.dfont") {
                fonts.font_data.insert(
                    "Monaco".to_owned(),
                    egui::FontData::from_owned(font_data),
                );
                fonts.families
                    .entry(egui::FontFamily::Monospace)
                    .or_insert_with(Vec::new)
                    .push("Monaco".to_owned());
            }

            // CJK fallback font for Chinese/Japanese/Korean characters
            let cjk_paths = [
                "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
                "/System/Library/Fonts/STHeiti Medium.ttc",
                "/System/Library/Fonts/Hiragino Sans GB.ttc",
            ];
            for path in &cjk_paths {
                if let Ok(font_data) = std::fs::read(path) {
                    fonts.font_data.insert(
                        "CJK".to_owned(),
                        egui::FontData::from_owned(font_data),
                    );
                    fonts.families
                        .entry(egui::FontFamily::Monospace)
                        .or_insert_with(Vec::new)
                        .push("CJK".to_owned());
                    fonts.families
                        .entry(egui::FontFamily::Proportional)
                        .or_insert_with(Vec::new)
                        .push("CJK".to_owned());
                    break;
                }
            }
        }

        cc.egui_ctx.set_fonts(fonts);

        // Visuals will be applied on the first frame via visuals_dirty flag,
        // because eframe may override visuals set during new().

        let runtime = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        let available_shells = load_available_shells();
        let selected_shell = std::env::var("SHELL")
            .unwrap_or_else(|_| available_shells.first().cloned().unwrap_or_else(|| "/bin/bash".to_string()));
        let first_tab = Tab {
            title: "Terminal 1".to_owned(),
            sessions: vec![TerminalSession::new_local(0, &selected_shell)],
            layout: PaneNode::Terminal(0),
            focused_session: 0,
            broadcast_enabled: false,
        };

        let hosts_file = config::hosts_file_path();
        let hosts = config::load_hosts(&hosts_file);

        Self {
            tabs: vec![first_tab],
            active_tab: 0,
            current_view: AppView::Terminal,
            hosts,
            hosts_file,
            next_id: 1,
            add_host_dialog: AddHostDialog::default(),
            host_to_delete: None,
            confirm_delete_host: None,
            ime_composing: false,
            ime_preedit: String::new(),
            runtime,
            sftp_browser_left: None,
            sftp_browser: None,
            local_browser: LocalBrowser::new(),
            left_panel_is_local: true,
            sftp_context_menu: None,
            sftp_rename_dialog: None,
            sftp_new_folder_dialog: None,
            sftp_new_file_dialog: None,
            sftp_confirm_delete: None,
            sftp_editor_dialog: None,
            sftp_error_dialog: None,
            sftp_local_refresh_start: None,
            sftp_remote_refresh_start: None,
            sftp_active_panel_is_local: true,  // Track which panel has focus
            tab_drag: TabDragState::default(),
            available_shells,
            selected_shell,
            selected_encoding: "UTF-8".to_string(),
            show_shell_picker: false,
            show_encoding_picker: false,
            detached_windows: Vec::new(),
            next_viewport_id: 0,
            main_window_hidden: false,
            broadcast_state: BroadcastState::default(),
            batch_execution: BatchExecutionState::default(),
            keychain_confirm_delete: None,
            theme,
            theme_preset,
            language,
            font_size,
            custom_font_path,
            fonts_dirty: false,
            visuals_dirty: true,
        }
    }

    fn add_tab_local(&mut self) {
        let id = self.next_id;
        self.next_id += 1;
        let tab = Tab {
            title: format!("Terminal {}", id),
            sessions: vec![TerminalSession::new_local(id, &self.selected_shell)],
            layout: PaneNode::Terminal(0),
            focused_session: 0,
            broadcast_enabled: false,
        };
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        self.current_view = AppView::Terminal;
    }

    fn add_tab_ssh(&mut self, host: &HostEntry) {
        let session = TerminalSession::new_ssh(host, &self.runtime);
        let tab = Tab {
            title: host.name.clone(),
            sessions: vec![session],
            layout: PaneNode::Terminal(0),
            focused_session: 0,
            broadcast_enabled: false,
        };
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        self.current_view = AppView::Terminal;
    }

    fn split_focused_pane(&mut self, direction: SplitDirection) {
        let new_id = self.next_id;
        self.next_id += 1;
        let tab = &self.tabs[self.active_tab];
        let old_idx = tab.focused_session;
        // Clone connection info from the focused session
        let ssh_host = tab.sessions.get(old_idx).and_then(|s| s.ssh_host.clone());
        let new_session = if let Some(host) = &ssh_host {
            TerminalSession::new_ssh(host, &self.runtime)
        } else {
            let shell = self.selected_shell.clone();
            TerminalSession::new_local(new_id, &shell)
        };
        let tab = &mut self.tabs[self.active_tab];
        tab.sessions.push(new_session);
        let new_idx = tab.sessions.len() - 1;
        tab.layout.replace(old_idx, PaneNode::Split {
            direction,
            ratio: 0.5,
            first: Box::new(PaneNode::Terminal(old_idx)),
            second: Box::new(PaneNode::Terminal(new_idx)),
        });
        tab.focused_session = new_idx;
    }

    fn close_pane(&mut self, session_idx: usize) {
        let active = self.active_tab;
        let tab = &mut self.tabs[active];

        if tab.sessions.len() <= 1 {
            // Only one pane → close the entire tab
            let _ = tab;
            if self.tabs.len() > 1 {
                self.tabs.remove(active);
                self.active_tab = active.saturating_sub(1);
            }
            return;
        }

        // Remove from layout tree; collapse the parent Split
        let old_layout = tab.layout.clone();
        if let Some(new_layout) = old_layout.remove(session_idx) {
            tab.layout = new_layout;
        }
        // Decrement indices of sessions that came after the removed one
        tab.layout.decrement_indices_above(session_idx);
        // Remove the session itself
        tab.sessions.remove(session_idx);
        // Fix focused_session
        if tab.focused_session >= tab.sessions.len() {
            tab.focused_session = tab.sessions.len().saturating_sub(1);
        } else if tab.focused_session == session_idx && session_idx > 0 {
            tab.focused_session = session_idx - 1;
        }
    }

    fn detach_tab(&mut self, tab_index: usize) {
        if self.tabs.len() <= 1 {
            return; // don't detach the only tab
        }
        let tab = self.tabs.remove(tab_index);
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len().saturating_sub(1);
        }

        let id_val = self.next_viewport_id;
        self.next_viewport_id += 1;
        let viewport_id = egui::ViewportId::from_hash_of(format!("detached_{}", id_val));

        let next_id = self.next_id;
        self.next_id += 100; // avoid ID conflicts with main window

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
            show_shell_picker: false,
            show_encoding_picker: false,
            broadcast_state: BroadcastState::default(),
        });
    }

    fn save_hosts(&self) {
        config::save_hosts(&self.hosts_file, &self.hosts);
    }
}

impl eframe::App for PortalApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(16));

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
            let title = self.detached_windows[i].title.clone();
            let builder = egui::ViewportBuilder::default()
                .with_title(&title)
                .with_inner_size([1200.0, 800.0])
                .with_min_inner_size([600.0, 400.0]);
            ctx.show_viewport_immediate(viewport_id, builder, |ctx, _class| {
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
                        let new_session = if let Some(host) = &ssh_host {
                            TerminalSession::new_ssh(host, &self.runtime)
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
                        let new_session = if let Some(host) = &ssh_host {
                            TerminalSession::new_ssh(host, &self.runtime)
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

                        // Settings button at bottom
                        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                            ui.add_space(8.0);
                            if nav_btn(ui, "\u{2699}", self.language.t("settings"), dw.current_view == AppView::Settings) {
                                dw.current_view = AppView::Settings;
                            }
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
                                ui.add_space(4.0);
                                let mut tab_to_activate: Option<usize> = None;
                                let mut tab_to_close: Option<usize> = None;
                                let mut tab_to_detach: Option<usize> = None;
                                let mut tab_rects: Vec<egui::Rect> = Vec::with_capacity(dw.tabs.len());
                                let tab_bar_rect = ui.max_rect(); // Track tab bar area for drag-out detection

                                for (ti, tab) in dw.tabs.iter().enumerate() {
                                    let is_active = ti == dw.active_tab;
                                    let is_drag_target = dw.tab_drag.source_index.is_some() && dw.tab_drag.target_index == Some(ti);
                                    let tab_fill = if is_active { self.theme.bg_elevated } else { egui::Color32::TRANSPARENT };

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
                                            let title_color = if is_active { self.theme.fg_primary } else { self.theme.fg_dim };
                                            ui.label(egui::RichText::new(&tab.title).color(title_color).size(13.0));
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
                                        dw.tab_drag.target_index = None;
                                        for (ti, rect) in tab_rects.iter().enumerate() {
                                            if rect.contains(pos) && Some(ti) != dw.tab_drag.source_index {
                                                dw.tab_drag.target_index = Some(ti);
                                                break;
                                            }
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
                                    }

                                    if ctx.input(|i| i.pointer.any_released()) {
                                        if let Some(dst) = dw.tab_drag.target_index {
                                            // Dropping on another tab → merge
                                            if src != dst && src < dw.tabs.len() && dst < dw.tabs.len() {
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
                                                // Update active_tab
                                                if dw.active_tab == src {
                                                    dw.active_tab = dst;
                                                } else if dw.active_tab > src && dw.active_tab > 0 {
                                                    dw.active_tab -= 1;
                                                }
                                                if dw.active_tab >= dw.tabs.len() {
                                                    dw.active_tab = dw.tabs.len().saturating_sub(1);
                                                }
                                            }
                                        } else {
                                            // Dropped outside tab area → detach to new window
                                            if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                                                if !tab_bar_rect.contains(pos) && src < dw.tabs.len() {
                                                    tab_to_detach = Some(src);
                                                }
                                            }
                                        }
                                        dw.tab_drag.source_index = None;
                                        dw.tab_drag.target_index = None;
                                    }
                                }

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

                                ui.add_space(4.0);
                                // New tab button
                                if ui.add(
                                    egui::Button::new(egui::RichText::new("+").color(self.theme.fg_dim).size(16.0)).frame(false)
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
                                    SshConnectionState::Error(e) => format!("SSH  {} ({})", host, e),
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

                    let mut shell_btn_rect = egui::Rect::NOTHING;
                    let mut enc_btn_rect = egui::Rect::NOTHING;

                    let bar_result = egui::TopBottomPanel::bottom(egui::Id::new("detached_status_bar").with(i))
                        .exact_height(24.0)
                        .frame(egui::Frame {
                            fill: self.theme.bg_secondary,
                            inner_margin: egui::Margin::symmetric(12.0, 0.0),
                            stroke: egui::Stroke::NONE,
                            ..Default::default()
                        })
                        .show(ctx, |ui| {
                            let mut shell_clicked = false;
                            let mut enc_clicked = false;
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
                                let sr = status_btn(ui, &shell_label, if is_local_session { self.theme.fg_primary } else { self.theme.fg_dim });
                                if is_local_session && sr.clicked() { shell_clicked = true; }
                                shell_btn_rect = sr.rect;
                                ui.add_space(12.0);
                                ui.label(egui::RichText::new("|").color(sep_color).size(12.0));
                                ui.add_space(12.0);
                                let er = status_btn(ui, &encoding_label, self.theme.fg_primary);
                                if er.clicked() { enc_clicked = true; }
                                enc_btn_rect = er.rect;
                                if !uptime_label.is_empty() {
                                    ui.add_space(12.0);
                                    ui.label(egui::RichText::new("|").color(sep_color).size(12.0));
                                    ui.add_space(12.0);
                                    ui.label(egui::RichText::new(&uptime_label).color(self.theme.fg_dim).size(12.0));
                                }
                            });
                            (shell_clicked, enc_clicked)
                        });

                    let (shell_clicked, enc_clicked) = bar_result.inner;
                    let dw = &mut self.detached_windows[i];
                    if shell_clicked { dw.show_shell_picker = !dw.show_shell_picker; dw.show_encoding_picker = false; }
                    if enc_clicked   { dw.show_encoding_picker = !dw.show_encoding_picker; dw.show_shell_picker = false; }

                    let popup_frame = egui::Frame::popup(&ctx.style())
                        .inner_margin(egui::Margin::same(4.0));

                    if dw.show_shell_picker && is_local_session {
                        let shells = self.available_shells.clone();
                        let item_h = 22.0;
                        let h = shells.len() as f32 * item_h + 10.0;
                        let pos = egui::pos2(shell_btn_rect.min.x, shell_btn_rect.min.y - h - 4.0);
                        let area_resp = egui::Area::new(egui::Id::new("detached_shell_picker").with(i))
                            .order(egui::Order::Foreground)
                            .fixed_pos(pos)
                            .show(ctx, |ui| {
                                popup_frame.show(ui, |ui| {
                                    ui.set_min_width(160.0);
                                    for shell_path in &shells {
                                        let name = shell_path.rsplit('/').next().unwrap_or(shell_path.as_str());
                                        let selected = *shell_path == self.selected_shell;
                                        if ui.selectable_label(selected, name).clicked() {
                                            self.selected_shell = shell_path.clone();
                                            self.detached_windows[i].show_shell_picker = false;
                                        }
                                    }
                                });
                            });
                        if ctx.input(|i| i.pointer.any_click()) {
                            let popup_rect = area_resp.response.rect;
                            if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                                if !popup_rect.contains(pos) && !shell_btn_rect.contains(pos) {
                                    self.detached_windows[i].show_shell_picker = false;
                                }
                            }
                        }
                    }

                    let dw = &mut self.detached_windows[i];
                    if dw.show_encoding_picker {
                        let encodings = ["UTF-8", "GBK", "GB2312", "ISO-8859-1", "UTF-16"];
                        let item_h = 22.0;
                        let h = encodings.len() as f32 * item_h + 10.0;
                        let pos = egui::pos2(enc_btn_rect.min.x, enc_btn_rect.min.y - h - 4.0);
                        let area_resp = egui::Area::new(egui::Id::new("detached_encoding_picker").with(i))
                            .order(egui::Order::Foreground)
                            .fixed_pos(pos)
                            .show(ctx, |ui| {
                                popup_frame.show(ui, |ui| {
                                    ui.set_min_width(120.0);
                                    for &enc in &encodings {
                                        if ui.selectable_label(enc == self.selected_encoding, enc).clicked() {
                                            self.selected_encoding = enc.to_string();
                                            self.detached_windows[i].show_encoding_picker = false;
                                        }
                                    }
                                });
                            });
                        if ctx.input(|inp| inp.pointer.any_click()) {
                            let popup_rect = area_resp.response.rect;
                            if let Some(pos) = ctx.input(|inp| inp.pointer.interact_pos()) {
                                if !popup_rect.contains(pos) && !enc_btn_rect.contains(pos) {
                                    self.detached_windows[i].show_encoding_picker = false;
                                }
                            }
                        }
                    }
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
                                            let new_session = if let Some(host) = &ssh_host {
                                                TerminalSession::new_ssh(host, &self.runtime)
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
                                            let new_session = if let Some(host) = &ssh_host {
                                                TerminalSession::new_ssh(host, &self.runtime)
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
                        show_shell_picker: false,
                        show_encoding_picker: false,
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
                ui.horizontal(|ui| {
                    ui.add_space(4.0);

                    // Tab buttons — one per workspace (with drag-to-reorder)
                    let mut tab_to_activate: Option<usize> = None;
                    let mut tab_to_close: Option<usize> = None;
                    let mut tab_to_reconnect: Option<usize> = None;
                    let mut tab_to_detach: Option<usize> = None;
                    let mut tab_rects: Vec<egui::Rect> = Vec::with_capacity(self.tabs.len());
                    let tab_bar_rect = ui.max_rect(); // Track tab bar area for drag-out detection

                    for (i, tab) in self.tabs.iter().enumerate() {
                        let is_active = i == self.active_tab;
                        let is_drag_target = self.tab_drag.source_index.is_some() && self.tab_drag.target_index == Some(i);
                        let is_broadcasting = tab.broadcast_enabled;

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

                                // Tab title
                                let title_color = if is_active { self.theme.fg_primary } else { self.theme.fg_dim };
                                ui.label(egui::RichText::new(&tab.title).color(title_color).size(13.0));

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
                            self.tab_drag.target_index = None;
                            for (i, rect) in tab_rects.iter().enumerate() {
                                if rect.contains(pos) && Some(i) != self.tab_drag.source_index {
                                    self.tab_drag.target_index = Some(i);
                                    break;
                                }
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
                        }

                        // Handle drop → merge source tab into target tab, or detach to new window
                        if ctx.input(|i| i.pointer.any_released()) {
                            if let Some(dst) = self.tab_drag.target_index {
                                // Dropping on another tab → merge
                                if src != dst && src < self.tabs.len() && dst < self.tabs.len() {
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
                                }
                            } else {
                                // Dropped outside tab area → detach to new window
                                if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                                    if !tab_bar_rect.contains(pos) && src < self.tabs.len() {
                                        tab_to_detach = Some(src);
                                    }
                                }
                            }
                            self.tab_drag.source_index = None;
                            self.tab_drag.target_index = None;
                        }
                    }

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

                    ui.add_space(4.0);

                    // New tab button
                    if ui.add(
                        egui::Button::new(
                            egui::RichText::new("+").color(self.theme.fg_dim).size(16.0)
                        )
                        .frame(false)
                    ).clicked() {
                        self.add_tab_local();
                    }

                    // ── More menu (⋯) at far right of tab bar ──
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

                                        // Broadcast toggle
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

                            // Close menu if clicked outside
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
                            SshConnectionState::Error(e) => format!("SSH  {} ({})", host, e),
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

            let mut shell_btn_rect = egui::Rect::NOTHING;
            let mut enc_btn_rect = egui::Rect::NOTHING;

            let bar_result = egui::TopBottomPanel::bottom("status_bar")
                .exact_height(24.0)
                .frame(egui::Frame {
                    fill: self.theme.bg_secondary,
                    inner_margin: egui::Margin::symmetric(12.0, 0.0),
                    stroke: egui::Stroke::NONE,
                    ..Default::default()
                })
                .show(ctx, |ui| {
                    let mut shell_clicked = false;
                    let mut enc_clicked = false;
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
                        let sr = status_btn(ui, &shell_label, if is_local_session { self.theme.fg_primary } else { self.theme.fg_dim });
                        if is_local_session && sr.clicked() { shell_clicked = true; }
                        shell_btn_rect = sr.rect;

                        // Encoding
                        ui.add_space(12.0);
                        ui.label(egui::RichText::new("|").color(sep_color).size(12.0));
                        ui.add_space(12.0);
                        let er = status_btn(ui, &encoding_label, self.theme.fg_primary);
                        if er.clicked() { enc_clicked = true; }
                        enc_btn_rect = er.rect;

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
                    (shell_clicked, enc_clicked)
                });

            let (shell_clicked, enc_clicked) = bar_result.inner;
            if shell_clicked { self.show_shell_picker = !self.show_shell_picker; self.show_encoding_picker = false; }
            if enc_clicked   { self.show_encoding_picker = !self.show_encoding_picker; self.show_shell_picker = false; }

            let popup_frame = egui::Frame::popup(&ctx.style())
                .inner_margin(egui::Margin::same(4.0));

            // Shell picker popup
            if self.show_shell_picker && is_local_session {
                let item_h = 22.0;
                let h = self.available_shells.len() as f32 * item_h + 10.0;
                let pos = egui::pos2(shell_btn_rect.min.x, shell_btn_rect.min.y - h - 4.0);
                let area_resp = egui::Area::new(egui::Id::new("shell_picker_area"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(pos)
                    .show(ctx, |ui| {
                        popup_frame.show(ui, |ui| {
                            ui.set_min_width(160.0);
                            let shells = self.available_shells.clone();
                            for shell_path in &shells {
                                let name = shell_path.rsplit('/').next().unwrap_or(shell_path.as_str());
                                let selected = *shell_path == self.selected_shell;
                                if ui.selectable_label(selected, name).clicked() {
                                    self.selected_shell = shell_path.clone();
                                    self.show_shell_picker = false;
                                }
                            }
                        });
                    });
                // Close if click landed outside the popup and outside the trigger button
                if ctx.input(|i| i.pointer.any_click()) {
                    let popup_rect = area_resp.response.rect;
                    if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                        if !popup_rect.contains(pos) && !shell_btn_rect.contains(pos) {
                            self.show_shell_picker = false;
                        }
                    }
                }
            }

            // Encoding picker popup
            if self.show_encoding_picker {
                let encodings = ["UTF-8", "GBK", "GB2312", "ISO-8859-1", "UTF-16"];
                let item_h = 22.0;
                let h = encodings.len() as f32 * item_h + 10.0;
                let pos = egui::pos2(enc_btn_rect.min.x, enc_btn_rect.min.y - h - 4.0);
                let area_resp = egui::Area::new(egui::Id::new("encoding_picker_area"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(pos)
                    .show(ctx, |ui| {
                        popup_frame.show(ui, |ui| {
                            ui.set_min_width(120.0);
                            for &enc in &encodings {
                                if ui.selectable_label(enc == self.selected_encoding, enc).clicked() {
                                    self.selected_encoding = enc.to_string();
                                    self.show_encoding_picker = false;
                                }
                            }
                        });
                    });
                if ctx.input(|i| i.pointer.any_click()) {
                    let popup_rect = area_resp.response.rect;
                    if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                        if !popup_rect.contains(pos) && !enc_btn_rect.contains(pos) {
                            self.show_encoding_picker = false;
                        }
                    }
                }
            }
        }

        // ── Poll SFTP browser ──────────────────────────────────────────────
        if let Some(ref mut browser) = self.sftp_browser {
            let had_transfer = browser.transfer.is_some();
            let was_download = browser.transfer.as_ref().map_or(false, |t| !t.is_upload);
            browser.poll();
            // Auto-refresh local browser after download completes
            if had_transfer && browser.transfer.is_none() && was_download {
                self.local_browser.refresh();
            }
            // Handle async file read results for the editor
            if let Some((path, data)) = browser.pending_file_content.take() {
                if let Some(ref mut dialog) = self.sftp_editor_dialog {
                    if dialog.loading && !dialog.is_local {
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
            if let Some((path, size)) = browser.pending_file_too_large.take() {
                if let Some(ref mut dialog) = self.sftp_editor_dialog {
                    if dialog.loading && !dialog.is_local {
                        dialog.error = format!(
                            "File too large: {} bytes (max 10 MB)",
                            size
                        );
                        dialog.loading = false;
                    }
                } else {
                    log::warn!("File too large (no editor dialog): {} ({} bytes)", path, size);
                }
            }
            if matches!(browser.state, SftpConnectionState::Disconnected) {
                self.sftp_browser = None;
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
                        let available = ui.available_rect_before_wrap();
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

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1400.0, 900.0])
        .with_min_inner_size([800.0, 600.0])
        .with_title("Portal");

    if let Some(icon) = load_app_icon() {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "Portal",
        options,
        Box::new(|cc| Ok(Box::new(PortalApp::new(cc)))),
    )
}
