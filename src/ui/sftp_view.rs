use eframe::egui;

use crate::PortalApp;
use crate::sftp::{FileSelection, SftpConnectionState, SftpEntry, SftpEntryKind};
use crate::ui::theme::ThemeColors;
use crate::ui::i18n::Language;
use crate::ui::types::{SftpContextMenu, SftpRenameDialog, SftpNewFolderDialog, SftpNewFileDialog, SftpConfirmDelete, SftpEditorDialog, SftpErrorDialog, SftpPanel};

/// A single entry in a drag payload.
#[derive(Clone)]
pub struct DragEntry {
    pub full_path: String,
    pub entry_name: String,
    pub is_dir: bool,
}

/// Drag-and-drop payload for SFTP panel file transfers (supports multi-select).
#[derive(Clone)]
pub struct DragPayload {
    pub is_local: bool,
    pub entries: Vec<DragEntry>,
}

/// Actions produced by render_file_panel for the caller to apply to FileSelection.
pub enum SelectionAction {
    Single(usize),
    Toggle(usize),
    Range(usize),
    SelectAll,
    FocusMove(usize),
    FocusExtend(usize),
    DeselectAll,
}

impl PortalApp {
    /// Render the SFTP view: left = local browser (always), right = host list or remote browser.
    pub fn show_sftp_view(&mut self, ui: &mut egui::Ui) {
        // Configure text cursor for smoother appearance
        ui.ctx().style_mut(|style| {
            style.visuals.text_cursor.stroke = egui::Stroke::new(2.0, self.theme.fg_primary);
            style.visuals.text_cursor.blink = true;
            style.visuals.text_cursor.on_duration = 0.48;
            style.visuals.text_cursor.off_duration = 0.32;
        });

        // ── Split the full area into left / divider / right ──
        let available = ui.available_rect_before_wrap();
        let divider_x = available.min.x + available.width() / 2.0;
        let panel_w = (available.width() - 1.0) / 2.0;

        let left_panel_rect = egui::Rect::from_min_size(
            available.min,
            egui::vec2(panel_w, available.height()),
        );
        let right_panel_rect = egui::Rect::from_min_size(
            egui::pos2(divider_x + 1.0, available.min.y),
            egui::vec2(panel_w, available.height()),
        );

        // Draw vertical divider as subtle shadow
        let shadow_rect = egui::Rect::from_min_size(
            egui::pos2(divider_x, available.min.y),
            egui::vec2(2.0, available.height()),
        );
        ui.painter().rect_filled(shadow_rect, 0.0, self.theme.hover_shadow);

        let mut local_left_navigate_to: Option<String> = None;
        let mut local_left_selection_action: Option<SelectionAction> = None;
        let mut local_right_navigate_to: Option<String> = None;
        let mut local_right_selection_action: Option<SelectionAction> = None;
        let mut remote_navigate_to: Option<String> = None;
        let mut remote_selection_action: Option<SelectionAction> = None;
        let mut left_remote_navigate_to: Option<String> = None;
        let mut left_remote_selection_action: Option<SelectionAction> = None;
        let mut local_left_toggle_hidden = false;
        let mut local_right_toggle_hidden = false;
        let mut local_left_ctx_menu_req: Option<(egui::Pos2, Option<usize>)> = None;
        let mut local_right_ctx_menu_req: Option<(egui::Pos2, Option<usize>)> = None;
        let mut remote_ctx_menu_req: Option<(egui::Pos2, Option<usize>)> = None;
        let mut left_remote_ctx_menu_req: Option<(egui::Pos2, Option<usize>)> = None;
        let mut local_left_open_file_req: Option<usize> = None;
        let mut local_right_open_file_req: Option<usize> = None;
        let mut remote_open_file_req: Option<usize> = None;
        let mut left_remote_open_file_req: Option<usize> = None;
        let mut local_left_delete_request = false;
        let mut local_right_delete_request = false;
        let mut remote_delete_request = false;
        let mut left_remote_delete_request = false;

        // Detect panel focus from mouse clicks
        if ui.ctx().input(|i| i.pointer.any_pressed()) {
            if let Some(pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
                if left_panel_rect.contains(pos) {
                    self.sftp_active_panel_is_local = true;
                } else if right_panel_rect.contains(pos) {
                    self.sftp_active_panel_is_local = false;
                }
            }
        }

        // ── LEFT PANEL: Local or Remote ──
        let mut left_connect_host: Option<usize> = None;
        let mut left_disconnect_request = false;
        let mut left_toggle_hidden_files = false;
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(left_panel_rect), |ui| {
            if self.left_panel_is_local {
                // ── LEFT PANEL: Local ──
                // Path bar with breadcrumbs
                egui::Frame {
                    fill: self.theme.bg_secondary,
                    inner_margin: egui::Margin::symmetric(8.0, 6.0),
                    stroke: egui::Stroke::NONE,
                    ..Default::default()
                }
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Back button
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("\u{2190}").color(self.theme.accent).size(13.0),
                            ).frame(false))
                            .on_hover_text(self.language.t("back"))
                            .clicked()
                        {
                            local_left_navigate_to = Some("..".to_string());
                        }
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(self.language.t("local")).color(self.theme.fg_dim).size(13.0).strong());
                        ui.add_space(4.0);
                        // Breadcrumb path
                        render_breadcrumbs(ui, &self.local_browser_left.current_path, &mut local_left_navigate_to, true, &self.theme);
                        // Refresh + switch to remote button (right-aligned)
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Switch to remote button
                            if ui
                                .add(egui::Button::new(
                                    egui::RichText::new("\u{2195}").color(self.theme.accent).size(13.0),
                                ).frame(false))
                                .on_hover_text(self.language.t("switch_to_remote"))
                                .clicked()
                            {
                                self.left_panel_is_local = false;
                            }
                            ui.add_space(8.0);
                            // Show/Hide hidden files toggle
                            let toggle_text = if self.local_browser_left.show_hidden_files {
                                self.language.t("hide_hidden_files")
                            } else {
                                self.language.t("show_hidden_files")
                            };
                            if ui
                                .add(egui::Button::new(
                                    egui::RichText::new(if self.local_browser_left.show_hidden_files { "\u{1F440}" } else { "\u{1F441}" })
                                        .color(if self.local_browser_left.show_hidden_files { self.theme.accent } else { self.theme.fg_dim })
                                        .size(13.0)
                                ).frame(false))
                                .on_hover_text(toggle_text)
                                .clicked()
                            {
                                local_left_toggle_hidden = true;
                            }
                            ui.add_space(8.0);
                            let is_refreshing = self.sftp_local_left_refresh_start
                                .map_or(false, |t| t.elapsed() < std::time::Duration::from_millis(300));
                            if is_refreshing {
                                ui.spinner();
                                ui.ctx().request_repaint();
                            } else {
                                self.sftp_local_left_refresh_start = None;
                                if ui
                                    .add(egui::Button::new(
                                        egui::RichText::new("\u{21BB}").color(self.theme.accent).size(13.0),
                                    ).frame(false))
                                    .clicked()
                                {
                                    self.sftp_local_left_refresh_start = Some(std::time::Instant::now());
                                    self.local_browser_left.refresh();
                                }
                            }
                        });
                    });
                });

                let filtered_entries = self.local_browser_left.filtered_entries();
                render_file_panel(
                    ui,
                    &filtered_entries,
                    &self.local_browser_left.selection,
                    &mut local_left_navigate_to,
                    &mut local_left_selection_action,
                    true,
                    &self.local_browser_left.current_path,
                    &self.theme,
                    &mut local_left_ctx_menu_req,
                    &mut local_left_open_file_req,
                    &mut local_left_delete_request,
                    self.sftp_active_panel_is_local,
                    &self.language,
                    "local_left_scroll",
                    self.sftp_editor_dialog.is_some(),
                );
            } else {
                // ── LEFT PANEL: Remote (independent connection) ──
                match self.sftp_browser_left.as_mut() {
                    None => {
                        // ── No connection: show host list ──
                        egui::Frame {
                            fill: self.theme.bg_secondary,
                            inner_margin: egui::Margin::symmetric(8.0, 6.0),
                            stroke: egui::Stroke::NONE,
                            ..Default::default()
                        }
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(self.language.t("remote_select")).color(self.theme.fg_dim).size(13.0).strong());
                                // Fill remaining width to match right panel header height
                                ui.allocate_space(egui::vec2(ui.available_width(), 0.0));
                            });
                        });

                        egui::ScrollArea::vertical()
                            .id_salt("sftp_left_host_list")
                            .show(ui, |ui| {
                            ui.add_space(8.0);
                            let row_h = 44.0;

                            // ── Local entry (fixed at top) ──
                            let width = ui.available_width();
                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(width, row_h),
                                egui::Sense::click(),
                            );
                            if resp.hovered() {
                                ui.painter().rect_filled(rect, 0.0, self.theme.hover_bg);
                            }
                            // Icon
                            ui.painter().text(
                                egui::pos2(rect.min.x + 16.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                "\u{1F4C2}",
                                egui::FontId::proportional(14.0),
                                self.theme.accent,
                            );
                            // Host name
                            ui.painter().text(
                                egui::pos2(rect.min.x + 38.0, rect.center().y - 7.0),
                                egui::Align2::LEFT_CENTER,
                                self.language.t("local"),
                                egui::FontId::proportional(13.0),
                                self.theme.fg_primary,
                            );
                            // Detail line
                            ui.painter().text(
                                egui::pos2(rect.min.x + 38.0, rect.center().y + 8.0),
                                egui::Align2::LEFT_CENTER,
                                &format!("/{}", self.local_browser_left.current_path.split('/').last().unwrap_or("")),
                                egui::FontId::proportional(10.0),
                                self.theme.fg_dim,
                            );
                            if resp.clicked() {
                                self.left_panel_is_local = true;
                            }

                            ui.add_space(4.0);

                            // ── SSH hosts ──
                            for (i, host) in self.hosts.iter().enumerate() {
                                if host.is_local { continue; }
                                let width = ui.available_width();
                                let (rect, resp) = ui.allocate_exact_size(
                                    egui::vec2(width, row_h),
                                    egui::Sense::click(),
                                );
                                if resp.hovered() {
                                    ui.painter().rect_filled(rect, 0.0, self.theme.hover_bg);
                                }
                                // Icon
                                ui.painter().text(
                                    egui::pos2(rect.min.x + 16.0, rect.center().y),
                                    egui::Align2::LEFT_CENTER,
                                    "\u{2195}",
                                    egui::FontId::proportional(14.0),
                                    self.theme.accent,
                                );
                                // Host name
                                ui.painter().text(
                                    egui::pos2(rect.min.x + 38.0, rect.center().y - 7.0),
                                    egui::Align2::LEFT_CENTER,
                                    &host.name,
                                    egui::FontId::proportional(13.0),
                                    self.theme.fg_primary,
                                );
                                // Detail line
                                let detail = if host.username.is_empty() {
                                    format!("{}:{}", host.host, host.port)
                                } else {
                                    format!("{}@{}:{}", host.username, host.host, host.port)
                                };
                                ui.painter().text(
                                    egui::pos2(rect.min.x + 38.0, rect.center().y + 8.0),
                                    egui::Align2::LEFT_CENTER,
                                    detail,
                                    egui::FontId::proportional(10.0),
                                    self.theme.fg_dim,
                                );
                                if resp.clicked() {
                                    left_connect_host = Some(i);
                                }
                            }
                            if self.hosts.iter().all(|h| h.is_local) {
                                ui.add_space(20.0);
                                let width = ui.available_width();
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(width, 40.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().text(
                                    egui::pos2(rect.min.x + 16.0, rect.center().y),
                                    egui::Align2::LEFT_CENTER,
                                    "No SSH hosts configured.\nAdd one from the Hosts page.",
                                    egui::FontId::proportional(12.0),
                                    self.theme.fg_dim,
                                );
                            }
                        });
                    }
                    Some(browser) => {
                        match &browser.state {
                            SftpConnectionState::Connecting => {
                                egui::Frame {
                                    fill: self.theme.bg_secondary,
                                    inner_margin: egui::Margin::symmetric(8.0, 6.0),
                                    stroke: egui::Stroke::NONE,
                                    ..Default::default()
                                }
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(self.language.t("remote")).color(self.theme.fg_dim).size(13.0).strong());
                                        ui.allocate_space(egui::vec2(ui.available_width(), 0.0));
                                    });
                                });
                                ui.vertical_centered(|ui| {
                                    ui.add_space(40.0);
                                    ui.label(
                                        egui::RichText::new(self.language.t("connecting"))
                                            .color(self.theme.fg_dim)
                                            .size(14.0),
                                    );
                                    ui.add_space(12.0);
                                    if ui.add(
                                        egui::Button::new(egui::RichText::new(self.language.t("cancel")).color(self.theme.red).size(13.0))
                                            .frame(false)
                                    ).clicked() {
                                        if let Some(ref b) = self.sftp_browser_left {
                                            b.cancel_connect();
                                        }
                                        self.sftp_browser_left = None;
                                    }
                                });
                            }
                            SftpConnectionState::Error(e) => {
                                let err = e.clone();
                                egui::Frame {
                                    fill: self.theme.bg_secondary,
                                    inner_margin: egui::Margin::symmetric(8.0, 6.0),
                                    stroke: egui::Stroke::NONE,
                                    ..Default::default()
                                }
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(self.language.t("remote")).color(self.theme.fg_dim).size(13.0).strong());
                                        ui.allocate_space(egui::vec2(ui.available_width(), 0.0));
                                    });
                                });
                                ui.vertical_centered(|ui| {
                                    ui.add_space(40.0);
                                    ui.label(egui::RichText::new(self.language.t("connection_failed")).color(self.theme.red).size(14.0).strong());
                                    ui.add_space(6.0);
                                    ui.label(egui::RichText::new(&err).color(self.theme.fg_dim).size(12.0));
                                    ui.add_space(12.0);
                                    if ui.add(
                                        egui::Button::new(egui::RichText::new(self.language.t("back")).color(self.theme.accent).size(13.0))
                                            .frame(false)
                                    ).clicked() {
                                        self.sftp_browser_left = None;
                                    }
                                });
                            }
                            SftpConnectionState::Connected => {
                                // Path bar with breadcrumbs + disconnect
                                egui::Frame {
                                    fill: self.theme.bg_secondary,
                                    inner_margin: egui::Margin::symmetric(8.0, 6.0),
                                    stroke: egui::Stroke::NONE,
                                    ..Default::default()
                                }
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        // Back button
                                        if ui
                                            .add(egui::Button::new(
                                                egui::RichText::new("\u{2190}").color(self.theme.accent).size(13.0),
                                            ).frame(false))
                                            .on_hover_text(self.language.t("back"))
                                            .clicked()
                                        {
                                            left_remote_navigate_to = Some("..".to_string());
                                        }
                                        ui.add_space(4.0);
                                        ui.label(egui::RichText::new(self.language.tf("remote_host", &browser.host_name)).color(self.theme.fg_dim).size(13.0).strong());
                                        ui.add_space(4.0);
                                        // Breadcrumb path
                                        let current_path_clone = browser.current_path.clone();
                                        render_breadcrumbs(ui, &current_path_clone, &mut left_remote_navigate_to, false, &self.theme);
                                        // Refresh + disconnect (right-aligned)
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if ui
                                                .add(egui::Button::new(
                                                    egui::RichText::new("\u{2715}").color(self.theme.red).size(13.0),
                                                ).frame(false))
                                                .on_hover_text(self.language.t("disconnect"))
                                                .clicked()
                                            {
                                                left_disconnect_request = true;
                                            }
                                            ui.add_space(8.0);
                                            // Show/Hide hidden files toggle
                                            let toggle_text = if browser.show_hidden_files {
                                                self.language.t("hide_hidden_files")
                                            } else {
                                                self.language.t("show_hidden_files")
                                            };
                                            if ui
                                                .add(egui::Button::new(
                                                    egui::RichText::new(if browser.show_hidden_files { "\u{1F440}" } else { "\u{1F441}" })
                                                        .color(if browser.show_hidden_files { self.theme.accent } else { self.theme.fg_dim })
                                                        .size(13.0)
                                                ).frame(false))
                                                .on_hover_text(toggle_text)
                                                .clicked()
                                            {
                                                left_toggle_hidden_files = true;
                                            }
                                            ui.add_space(8.0);
                                            let is_refreshing = self.sftp_left_remote_refresh_start
                                                .map_or(false, |t| t.elapsed() < std::time::Duration::from_millis(300));
                                            if is_refreshing {
                                                ui.spinner();
                                            } else {
                                                self.sftp_left_remote_refresh_start = None;
                                                if ui
                                                    .add(egui::Button::new(
                                                        egui::RichText::new("\u{21BB}").color(self.theme.accent).size(13.0),
                                                    ).frame(false))
                                                    .clicked()
                                                {
                                                    self.sftp_left_remote_refresh_start = Some(std::time::Instant::now());
                                                    browser.refresh();
                                                }
                                            }
                                        });
                                    });
                                });

                                let selection = browser.selection.clone();
                                let current_path = browser.current_path.clone();
                                let filtered_entries = browser.filtered_entries();
                                render_file_panel(
                                    ui,
                                    &filtered_entries,
                                    &selection,
                                    &mut left_remote_navigate_to,
                                    &mut left_remote_selection_action,
                                    false,
                                    &current_path,
                                    &self.theme,
                                    &mut left_remote_ctx_menu_req,
                                    &mut left_remote_open_file_req,
                                    &mut left_remote_delete_request,
                                    self.sftp_active_panel_is_local,
                                    &self.language,
                                    "left_remote_scroll",
                                    self.sftp_editor_dialog.is_some(),
                                );
                            }
                            SftpConnectionState::Disconnected => {
                                // Immediately clean up disconnected browser and return to host list
                                self.sftp_browser_left = None;
                            }
                        }
                    }
                }
            }
        });

        // ── Handle left panel disconnect request ──
        if left_disconnect_request {
            self.sftp_browser_left = None;
        }
        // ── Handle left panel toggle hidden files request ──
        if left_toggle_hidden_files {
            if let Some(ref mut b) = self.sftp_browser_left.as_mut() {
                b.toggle_hidden_files();
            }
        }
        // ── Handle left local panel toggle hidden files request ──
        if local_left_toggle_hidden {
            self.local_browser_left.toggle_hidden_files();
        }

        // ── RIGHT PANEL: Local or Remote ──
        let is_connected = matches!(
            self.sftp_browser.as_ref().map(|b| &b.state),
            Some(SftpConnectionState::Connected)
        );
        let mut connect_host: Option<usize> = None;
        let mut right_disconnect_request = false;
        let mut right_toggle_hidden_files = false;

        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(right_panel_rect), |ui| {
            if self.right_panel_is_local {
                // ── RIGHT PANEL: Local ──
                // Path bar with breadcrumbs
                egui::Frame {
                    fill: self.theme.bg_secondary,
                    inner_margin: egui::Margin::symmetric(8.0, 6.0),
                    stroke: egui::Stroke::NONE,
                    ..Default::default()
                }
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Back button
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("\u{2190}").color(self.theme.accent).size(13.0),
                            ).frame(false))
                            .on_hover_text(self.language.t("back"))
                            .clicked()
                        {
                            local_right_navigate_to = Some("..".to_string());
                        }
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(self.language.t("local")).color(self.theme.fg_dim).size(13.0).strong());
                        ui.add_space(4.0);
                        // Breadcrumb path
                        render_breadcrumbs(ui, &self.local_browser_right.current_path, &mut local_right_navigate_to, true, &self.theme);
                        // Refresh + switch to remote button (right-aligned)
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Switch to remote button
                            if ui
                                .add(egui::Button::new(
                                    egui::RichText::new("\u{2195}").color(self.theme.accent).size(13.0),
                                ).frame(false))
                                .on_hover_text(self.language.t("switch_to_remote"))
                                .clicked()
                            {
                                self.right_panel_is_local = false;
                            }
                            ui.add_space(8.0);
                            // Show/Hide hidden files toggle
                            let toggle_text = if self.local_browser_right.show_hidden_files {
                                self.language.t("hide_hidden_files")
                            } else {
                                self.language.t("show_hidden_files")
                            };
                            if ui
                                .add(egui::Button::new(
                                    egui::RichText::new(if self.local_browser_right.show_hidden_files { "\u{1F440}" } else { "\u{1F441}" })
                                        .color(if self.local_browser_right.show_hidden_files { self.theme.accent } else { self.theme.fg_dim })
                                        .size(13.0)
                                ).frame(false))
                                .on_hover_text(toggle_text)
                                .clicked()
                            {
                                local_right_toggle_hidden = true;
                            }
                            ui.add_space(8.0);
                            let is_refreshing = self.sftp_local_right_refresh_start
                                .map_or(false, |t| t.elapsed() < std::time::Duration::from_millis(300));
                            if is_refreshing {
                                ui.spinner();
                                ui.ctx().request_repaint();
                            } else {
                                self.sftp_local_right_refresh_start = None;
                                if ui
                                    .add(egui::Button::new(
                                        egui::RichText::new("\u{21BB}").color(self.theme.accent).size(13.0),
                                    ).frame(false))
                                    .clicked()
                                {
                                    self.sftp_local_right_refresh_start = Some(std::time::Instant::now());
                                    self.local_browser_right.refresh();
                                }
                            }
                        });
                    });
                });

                let filtered_entries = self.local_browser_right.filtered_entries();
                render_file_panel(
                    ui,
                    &filtered_entries,
                    &self.local_browser_right.selection,
                    &mut local_right_navigate_to,
                    &mut local_right_selection_action,
                    true,
                    &self.local_browser_right.current_path,
                    &self.theme,
                    &mut local_right_ctx_menu_req,
                    &mut local_right_open_file_req,
                    &mut local_right_delete_request,
                    !self.sftp_active_panel_is_local,
                    &self.language,
                    "local_right_scroll",
                    self.sftp_editor_dialog.is_some(),
                );
            } else {
                // ── RIGHT PANEL: Remote (host list or remote browser) ──
                match self.sftp_browser.as_mut() {
                    None => {
                        // ── No connection: show host list ──
                        egui::Frame {
                            fill: self.theme.bg_secondary,
                            inner_margin: egui::Margin::symmetric(8.0, 6.0),
                            stroke: egui::Stroke::NONE,
                            ..Default::default()
                        }
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(self.language.t("remote_select")).color(self.theme.fg_dim).size(13.0).strong());
                                // Fill remaining width to match left panel header height
                                ui.allocate_space(egui::vec2(ui.available_width(), 0.0));
                            });
                        });

                    egui::ScrollArea::vertical()
                        .id_salt("sftp_host_list")
                        .show(ui, |ui| {
                        ui.add_space(8.0);
                        let row_h = 44.0;

                        // ── Local entry (fixed at top) ──
                        let width = ui.available_width();
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(width, row_h),
                            egui::Sense::click(),
                        );
                        if resp.hovered() {
                            ui.painter().rect_filled(rect, 0.0, self.theme.hover_bg);
                        }
                        // Icon
                        ui.painter().text(
                            egui::pos2(rect.min.x + 16.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            "\u{1F4C2}",
                            egui::FontId::proportional(14.0),
                            self.theme.accent,
                        );
                        // Host name
                        ui.painter().text(
                            egui::pos2(rect.min.x + 38.0, rect.center().y - 7.0),
                            egui::Align2::LEFT_CENTER,
                            self.language.t("local"),
                            egui::FontId::proportional(13.0),
                            self.theme.fg_primary,
                        );
                        // Detail line
                        ui.painter().text(
                            egui::pos2(rect.min.x + 38.0, rect.center().y + 8.0),
                            egui::Align2::LEFT_CENTER,
                            &format!("/{}", self.local_browser_right.current_path.split('/').last().unwrap_or("")),
                            egui::FontId::proportional(10.0),
                            self.theme.fg_dim,
                        );
                        if resp.clicked() {
                            self.right_panel_is_local = true;
                        }

                        ui.add_space(4.0);

                        // ── SSH hosts ──
                        for (i, host) in self.hosts.iter().enumerate() {
                            if host.is_local { continue; }
                            let width = ui.available_width();
                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(width, row_h),
                                egui::Sense::click(),
                            );
                            if resp.hovered() {
                                ui.painter().rect_filled(rect, 0.0, self.theme.hover_bg);
                            }
                            // Icon
                            ui.painter().text(
                                egui::pos2(rect.min.x + 16.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                "\u{2195}",
                                egui::FontId::proportional(14.0),
                                self.theme.accent,
                            );
                            // Host name
                            ui.painter().text(
                                egui::pos2(rect.min.x + 38.0, rect.center().y - 7.0),
                                egui::Align2::LEFT_CENTER,
                                &host.name,
                                egui::FontId::proportional(13.0),
                                self.theme.fg_primary,
                            );
                            // Detail line
                            let detail = if host.username.is_empty() {
                                format!("{}:{}", host.host, host.port)
                            } else {
                                format!("{}@{}:{}", host.username, host.host, host.port)
                            };
                            ui.painter().text(
                                egui::pos2(rect.min.x + 38.0, rect.center().y + 8.0),
                                egui::Align2::LEFT_CENTER,
                                detail,
                                egui::FontId::proportional(10.0),
                                self.theme.fg_dim,
                            );
                            if resp.clicked() {
                                connect_host = Some(i);
                            }
                        }
                        if self.hosts.iter().all(|h| h.is_local) {
                            ui.add_space(20.0);
                            let width = ui.available_width();
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(width, 40.0),
                                egui::Sense::hover(),
                            );
                            ui.painter().text(
                                egui::pos2(rect.min.x + 16.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                "No SSH hosts configured.\nAdd one from the Hosts page.",
                                egui::FontId::proportional(12.0),
                                self.theme.fg_dim,
                            );
                        }
                    });
                }
                Some(browser) => {
                    match &browser.state {
                        SftpConnectionState::Connecting => {
                            egui::Frame {
                                fill: self.theme.bg_secondary,
                                inner_margin: egui::Margin::symmetric(8.0, 6.0),
                                stroke: egui::Stroke::NONE,
                                ..Default::default()
                            }
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(self.language.t("remote")).color(self.theme.fg_dim).size(13.0).strong());
                                    ui.allocate_space(egui::vec2(ui.available_width(), 0.0));
                                });
                            });
                            ui.vertical_centered(|ui| {
                                ui.add_space(40.0);
                                ui.label(
                                    egui::RichText::new(self.language.t("connecting"))
                                        .color(self.theme.fg_dim)
                                        .size(14.0),
                                );
                                ui.add_space(12.0);
                                if ui.add(
                                    egui::Button::new(egui::RichText::new(self.language.t("cancel")).color(self.theme.red).size(13.0))
                                        .frame(false)
                                ).clicked() {
                                    if let Some(ref b) = self.sftp_browser {
                                        b.cancel_connect();
                                    }
                                    self.sftp_browser = None;
                                }
                            });
                        }
                        SftpConnectionState::Error(e) => {
                            let err = e.clone();
                            egui::Frame {
                                fill: self.theme.bg_secondary,
                                inner_margin: egui::Margin::symmetric(8.0, 6.0),
                                stroke: egui::Stroke::NONE,
                                ..Default::default()
                            }
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(self.language.t("remote")).color(self.theme.fg_dim).size(13.0).strong());
                                    ui.allocate_space(egui::vec2(ui.available_width(), 0.0));
                                });
                            });
                            ui.vertical_centered(|ui| {
                                ui.add_space(40.0);
                                ui.label(egui::RichText::new(self.language.t("connection_failed")).color(self.theme.red).size(14.0).strong());
                                ui.add_space(6.0);
                                ui.label(egui::RichText::new(&err).color(self.theme.fg_dim).size(12.0));
                                ui.add_space(12.0);
                                if ui.add(
                                    egui::Button::new(egui::RichText::new(self.language.t("back")).color(self.theme.accent).size(13.0))
                                        .frame(false)
                                ).clicked() {
                                    self.sftp_browser = None;
                                }
                            });
                        }
                        SftpConnectionState::Connected => {
                            // Path bar with breadcrumbs + disconnect
                            egui::Frame {
                                fill: self.theme.bg_secondary,
                                inner_margin: egui::Margin::symmetric(8.0, 6.0),
                                stroke: egui::Stroke::NONE,
                                ..Default::default()
                            }
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // Back button
                                    if ui
                                        .add(egui::Button::new(
                                            egui::RichText::new("\u{2190}").color(self.theme.accent).size(13.0),
                                        ).frame(false))
                                        .on_hover_text(self.language.t("back"))
                                        .clicked()
                                    {
                                        remote_navigate_to = Some("..".to_string());
                                    }
                                    ui.add_space(4.0);
                                    ui.label(egui::RichText::new(format!("REMOTE  {}", browser.host_name)).color(self.theme.fg_dim).size(13.0).strong());
                                    ui.add_space(4.0);
                                    // Breadcrumb path
                                    let current_path_clone = browser.current_path.clone();
                                    render_breadcrumbs(ui, &current_path_clone, &mut remote_navigate_to, false, &self.theme);
                                    // Refresh + disconnect (right-aligned)
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui
                                            .add(egui::Button::new(
                                                egui::RichText::new("\u{2715}").color(self.theme.red).size(13.0),
                                            ).frame(false))
                                            .on_hover_text(self.language.t("disconnect"))
                                            .clicked()
                                        {
                                            right_disconnect_request = true;
                                        }
                                        ui.add_space(8.0);
                                        // Show/Hide hidden files toggle
                                        let toggle_text = if browser.show_hidden_files {
                                            self.language.t("hide_hidden_files")
                                        } else {
                                            self.language.t("show_hidden_files")
                                        };
                                        if ui
                                            .add(egui::Button::new(
                                                egui::RichText::new(if browser.show_hidden_files { "\u{1F440}" } else { "\u{1F441}" })
                                                    .color(if browser.show_hidden_files { self.theme.accent } else { self.theme.fg_dim })
                                                    .size(13.0)
                                            ).frame(false))
                                            .on_hover_text(toggle_text)
                                            .clicked()
                                        {
                                            right_toggle_hidden_files = true;
                                        }
                                        ui.add_space(8.0);
                                        let is_refreshing = self.sftp_remote_refresh_start
                                            .map_or(false, |t| t.elapsed() < std::time::Duration::from_millis(300));
                                        if is_refreshing {
                                            ui.spinner();
                                            ui.ctx().request_repaint();
                                        } else {
                                            self.sftp_remote_refresh_start = None;
                                            if ui
                                                .add(egui::Button::new(
                                                    egui::RichText::new("\u{21BB}").color(self.theme.accent).size(13.0),
                                                ).frame(false))
                                                .clicked()
                                            {
                                                self.sftp_remote_refresh_start = Some(std::time::Instant::now());
                                                let path = browser.current_path.clone();
                                                browser.navigate(&path);
                                            }
                                        }
                                    });
                                });
                            });

                            let selection = browser.selection.clone();
                            let current_path = browser.current_path.clone();
                            let filtered_entries = browser.filtered_entries();
                            render_file_panel(
                                ui,
                                &filtered_entries,
                                &selection,
                                &mut remote_navigate_to,
                                &mut remote_selection_action,
                                false,
                                &current_path,
                                &self.theme,
                                &mut remote_ctx_menu_req,
                                &mut remote_open_file_req,
                                &mut remote_delete_request,
                                !self.sftp_active_panel_is_local,
                                &self.language,
                                "right_remote_scroll",
                                self.sftp_editor_dialog.is_some(),
                            );
                        }
                        SftpConnectionState::Disconnected => {
                            // Will be cleaned up by poll()
                        }
                    }
                }
                }
            }
        });

        // ── Handle right panel disconnect request ──
        if right_disconnect_request {
            self.sftp_browser = None;
        }
        // ── Handle right panel toggle hidden files request ──
        if right_toggle_hidden_files {
            if let Some(ref mut b) = self.sftp_browser.as_mut() {
                b.toggle_hidden_files();
            }
        }
        // ── Handle right local panel toggle hidden files request ──
        if local_right_toggle_hidden {
            self.local_browser_right.toggle_hidden_files();
        }

        // Reserve the full area
        ui.allocate_rect(available, egui::Sense::hover());

        // ── Connect to host if selected (left panel) ──
        if let Some(idx) = left_connect_host {
            let host = &self.hosts[idx];
            let username = crate::ui::types::TerminalSession::get_effective_username(&host.username);
            let auth = crate::config::resolve_auth(host, &self.credentials);
            self.sftp_browser_left = Some(crate::sftp::SftpBrowser::connect(
                &self.runtime,
                host.host.clone(),
                host.port,
                username,
                auth,
                host.name.clone(),
            ));
        }

        // ── Connect to host if selected (right panel) ──
        if let Some(idx) = connect_host {
            let host = &self.hosts[idx];
            let username = crate::ui::types::TerminalSession::get_effective_username(&host.username);
            let auth = crate::config::resolve_auth(host, &self.credentials);
            self.sftp_browser = Some(crate::sftp::SftpBrowser::connect(
                &self.runtime,
                host.host.clone(),
                host.port,
                username,
                auth,
                host.name.clone(),
            ));
        }

        // ── Apply deferred local panel actions ──
        if let Some(name) = local_left_navigate_to {
            if name == ".." {
                self.local_browser_left.navigate_up();
            } else if name.starts_with('/') {
                self.local_browser_left.navigate(&name);
            } else {
                let path = format!(
                    "{}/{}",
                    self.local_browser_left.current_path.trim_end_matches('/'),
                    name
                );
                self.local_browser_left.navigate(&path);
            }
        }
        if let Some(action) = local_left_selection_action {
            let count = self.local_browser_left.filtered_entries().len();
            apply_selection_action(&mut self.local_browser_left.selection, action, count);
        }

        // ── Apply deferred left panel remote actions (only when connected) ──

        // ── Apply deferred right panel local actions ──
        if let Some(name) = local_right_navigate_to {
            if name == ".." {
                self.local_browser_right.navigate_up();
            } else if name.starts_with('/') {
                self.local_browser_right.navigate(&name);
            } else {
                let path = format!(
                    "{}/{}",
                    self.local_browser_right.current_path.trim_end_matches('/'),
                    name
                );
                self.local_browser_right.navigate(&path);
            }
        }
        if let Some(action) = local_right_selection_action {
            let count = self.local_browser_right.filtered_entries().len();
            apply_selection_action(&mut self.local_browser_right.selection, action, count);
        }
        let left_is_connected = matches!(
            self.sftp_browser_left.as_ref().map(|b| &b.state),
            Some(SftpConnectionState::Connected)
        );
        if left_is_connected {
            if let Some(name) = left_remote_navigate_to {
                let browser = self.sftp_browser_left.as_ref().unwrap();
                if name == ".." {
                    browser.navigate_up();
                } else if name.starts_with('/') {
                    browser.navigate(&name);
                } else {
                    let path = format!(
                        "{}/{}",
                        browser.current_path.trim_end_matches('/'),
                        name
                    );
                    browser.navigate(&path);
                }
            }
            if let Some(action) = left_remote_selection_action {
                let count = self.sftp_browser_left.as_ref().unwrap().entries.len();
                apply_selection_action(&mut self.sftp_browser_left.as_mut().unwrap().selection, action, count);
            }
        }

        // ── Apply deferred remote panel actions (only when connected) ──
        if is_connected {
            if let Some(name) = remote_navigate_to {
                let browser = self.sftp_browser.as_ref().unwrap();
                if name == ".." {
                    browser.navigate_up();
                } else if name.starts_with('/') {
                    browser.navigate(&name);
                } else {
                    let path = format!(
                        "{}/{}",
                        browser.current_path.trim_end_matches('/'),
                        name
                    );
                    browser.navigate(&path);
                }
            }
            if let Some(action) = remote_selection_action {
                let count = self.sftp_browser.as_ref().unwrap().entries.len();
                apply_selection_action(&mut self.sftp_browser.as_mut().unwrap().selection, action, count);
            }
        }

        // ── Drag-and-drop (only when connected) ──
        if is_connected {
            let ctx = ui.ctx().clone();
            if let Some(payload) = egui::DragAndDrop::payload::<DragPayload>(&ctx) {
                if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                    if left_panel_rect.contains(pos) && !payload.is_local {
                        ui.painter().rect_stroke(left_panel_rect, 2.0, egui::Stroke::new(2.0, self.theme.accent));
                    }
                    if right_panel_rect.contains(pos) && payload.is_local {
                        ui.painter().rect_stroke(right_panel_rect, 2.0, egui::Stroke::new(2.0, self.theme.accent));
                    }
                }
                // Drag ghost
                if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                    let ghost_text = if payload.entries.len() == 1 {
                        let e = &payload.entries[0];
                        if e.is_dir {
                            format!("\u{1F4C1} {}", e.entry_name)
                        } else {
                            format!("\u{1F4C4} {}", e.entry_name)
                        }
                    } else {
                        self.language.tf("n_items", &payload.entries.len().to_string())
                    };
                    egui::Area::new(egui::Id::new("sftp_drag_ghost"))
                        .fixed_pos(pos + egui::vec2(14.0, -8.0))
                        .order(egui::Order::Tooltip)
                        .show(&ctx, |ui| {
                            egui::Frame {
                                fill: self.theme.bg_elevated,
                                inner_margin: egui::Margin::symmetric(8.0, 4.0),
                                rounding: egui::Rounding::same(8.0),
                                stroke: egui::Stroke::new(1.0, self.theme.accent),
                                ..Default::default()
                            }
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new(ghost_text).color(self.theme.fg_primary).size(12.0));
                            });
                        });
                }
            }

            // Handle drop
            if ctx.input(|i| i.pointer.any_released()) {
                if let Some(payload) = egui::DragAndDrop::take_payload::<DragPayload>(&ctx) {
                    if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                        // Only handle drops to remote browser when connected
                        if let Some(browser) = self.sftp_browser.as_ref() {
                            for entry in &payload.entries {
                                if left_panel_rect.contains(pos) && !payload.is_local {
                                    let local_dest = format!(
                                        "{}/{}",
                                        self.local_browser_left.current_path.trim_end_matches('/'),
                                        entry.entry_name,
                                    );
                                    if entry.is_dir {
                                        browser.download_dir(&entry.full_path, &local_dest);
                                    } else {
                                        browser.download(&entry.full_path, &local_dest);
                                    }
                                } else if right_panel_rect.contains(pos) && payload.is_local {
                                    let remote_dest = format!(
                                        "{}/{}",
                                        browser.current_path.trim_end_matches('/'),
                                        entry.entry_name,
                                    );
                                    if entry.is_dir {
                                        browser.upload_dir(&entry.full_path, &remote_dest);
                                    } else {
                                        browser.upload(&entry.full_path, &remote_dest);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Transfer progress bar
            let mut should_cancel = false;
            if let Some(ref browser) = self.sftp_browser.as_ref() {
                if let Some(ref progress) = browser.transfer {
                let label = if progress.is_upload {
                    self.language.tf("uploading", &progress.filename)
                } else {
                    self.language.tf("downloading", &progress.filename)
                };
                let pct = if progress.total_bytes > 0 {
                    progress.bytes_transferred as f32 / progress.total_bytes as f32
                } else {
                    0.0
                };
                // Format speed
                let speed = progress.speed_bps();
                let speed_str = format_transfer_speed(speed);
                // Format transferred / total
                let size_str = format!(
                    "{} / {}",
                    format_file_size(progress.bytes_transferred),
                    format_file_size(progress.total_bytes),
                );

                let bar_h = 40.0;
                let bar_area = egui::Rect::from_min_size(
                    egui::pos2(available.min.x, available.max.y - bar_h),
                    egui::vec2(available.width(), bar_h),
                );
                ui.painter().rect_filled(bar_area, 0.0, self.theme.bg_elevated);

                let pad_x = 12.0;
                let stop_size = 20.0;
                let stop_right_pad = 10.0;
                let content_right = bar_area.max.x - stop_size - stop_right_pad * 2.0;

                // ── Row 1: label + info text + stop button ──
                let row1_cy = bar_area.min.y + bar_h * 0.30;

                // Label: "Uploading filename"
                let label_galley = ui.painter().layout_no_wrap(
                    label,
                    egui::FontId::proportional(12.0),
                    self.theme.fg_primary,
                );
                let label_w = label_galley.size().x;
                ui.painter().galley(
                    egui::pos2(bar_area.min.x + pad_x, row1_cy - label_galley.size().y / 2.0),
                    label_galley,
                    self.theme.fg_primary,
                );

                // Info text (right-aligned, left of stop button)
                let info_text = format!("{:.0}%   {}   {}", pct * 100.0, speed_str, size_str);
                let info_galley = ui.painter().layout_no_wrap(
                    info_text,
                    egui::FontId::proportional(11.0),
                    self.theme.fg_dim,
                );
                let info_x = (content_right - info_galley.size().x).max(bar_area.min.x + pad_x + label_w + 12.0);
                ui.painter().galley(
                    egui::pos2(info_x, row1_cy - info_galley.size().y / 2.0),
                    info_galley,
                    self.theme.fg_dim,
                );

                // Stop button (vertically centered in bar)
                let stop_rect = egui::Rect::from_min_size(
                    egui::pos2(bar_area.max.x - stop_size - stop_right_pad, bar_area.center().y - stop_size / 2.0),
                    egui::vec2(stop_size, stop_size),
                );
                let stop_resp = ui.allocate_rect(stop_rect, egui::Sense::click());
                let stop_color = if stop_resp.hovered() { self.theme.red } else { self.theme.fg_dim };
                ui.painter().text(
                    stop_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "\u{25A0}",
                    egui::FontId::proportional(14.0),
                    stop_color,
                );
                if stop_resp.on_hover_text(self.language.t("stop_transfer")).clicked() {
                    should_cancel = true;
                }

                // ── Row 2: full-width progress bar ──
                let row2_cy = bar_area.min.y + bar_h * 0.75;
                let pb_h = 6.0;
                let pb_rect = egui::Rect::from_min_size(
                    egui::pos2(bar_area.min.x + pad_x, row2_cy - pb_h / 2.0),
                    egui::vec2(content_right - bar_area.min.x - pad_x, pb_h),
                );
                ui.painter().rect_filled(pb_rect, 3.0, self.theme.border);
                let filled = egui::Rect::from_min_size(
                    pb_rect.min,
                    egui::vec2(pb_rect.width() * pct, pb_rect.height()),
                );
                ui.painter().rect_filled(filled, 3.0, self.theme.accent);
            }
            if should_cancel {
                // Cancel transfer outside the immutable borrow
                let _ = browser;
                if let Some(ref mut b) = self.sftp_browser.as_mut() {
                    b.cancel_transfer();
                }
            }
        }
        }

        // ── Handle delete key requests ──
        if local_left_delete_request {
            let filtered = self.local_browser_left.filtered_entries();
            let names: Vec<String> = self.local_browser_left.selection.selected.iter()
                .filter_map(|&i| filtered.get(i).map(|e| e.name.clone()))
                .collect();
            if !names.is_empty() {
                self.sftp_confirm_delete = Some(SftpConfirmDelete {
                    panel: SftpPanel::LeftLocal,
                    names,
                });
            }
        }
        if remote_delete_request {
            if let Some(ref browser) = self.sftp_browser {
                let filtered = browser.filtered_entries();
                let names: Vec<String> = browser.selection.selected.iter()
                    .filter_map(|&i| filtered.get(i).map(|e| e.name.clone()))
                    .collect();
                if !names.is_empty() {
                    self.sftp_confirm_delete = Some(SftpConfirmDelete {
                        panel: SftpPanel::RightRemote,
                        names,
                    });
                }
            }
        }
        if left_remote_delete_request {
            if let Some(ref browser) = self.sftp_browser_left {
                let filtered = browser.filtered_entries();
                let names: Vec<String> = browser.selection.selected.iter()
                    .filter_map(|&i| filtered.get(i).map(|e| e.name.clone()))
                    .collect();
                if !names.is_empty() {
                    self.sftp_confirm_delete = Some(SftpConfirmDelete {
                        panel: SftpPanel::LeftRemote,
                        names,
                    });
                }
            }
        }

        // ── Handle context menu requests ──
        let menu_just_opened = local_left_ctx_menu_req.is_some() || remote_ctx_menu_req.is_some() || left_remote_ctx_menu_req.is_some();
        if let Some((pos, entry_idx)) = local_left_ctx_menu_req {
            // If right-clicked on an entry not in selection, select it first
            if let Some(idx) = entry_idx {
                if !self.local_browser_left.selection.is_selected(idx) {
                    self.local_browser_left.selection.select_one(idx);
                }
            }
            let indices: Vec<usize> = if entry_idx.is_some() {
                self.local_browser_left.selection.selected.iter().copied().collect()
            } else {
                vec![]
            };
            let filtered = self.local_browser_left.filtered_entries();
            let names: Vec<String> = indices.iter()
                .filter_map(|&i| filtered.get(i).map(|e| e.name.clone()))
                .collect();
            let all_dirs = !indices.is_empty() && indices.iter().all(|&i| {
                filtered.get(i).map_or(false, |e| e.kind == SftpEntryKind::Directory)
            });
            let any_dirs = indices.iter().any(|&i| {
                filtered.get(i).map_or(false, |e| e.kind == SftpEntryKind::Directory)
            });
            self.sftp_context_menu = Some(SftpContextMenu {
                pos,
                panel: SftpPanel::LeftLocal,
                entry_indices: indices,
                entry_names: names,
                all_dirs,
                any_dirs,
            });
        }
        if let Some((pos, entry_idx)) = local_right_ctx_menu_req {
            // If right-clicked on an entry not in selection, select it first
            if let Some(idx) = entry_idx {
                if !self.local_browser_right.selection.is_selected(idx) {
                    self.local_browser_right.selection.select_one(idx);
                }
            }
            let indices: Vec<usize> = if entry_idx.is_some() {
                self.local_browser_right.selection.selected.iter().copied().collect()
            } else {
                vec![]
            };
            let filtered = self.local_browser_right.filtered_entries();
            let names: Vec<String> = indices.iter()
                .filter_map(|&i| filtered.get(i).map(|e| e.name.clone()))
                .collect();
            let all_dirs = !indices.is_empty() && indices.iter().all(|&i| {
                filtered.get(i).map_or(false, |e| e.kind == SftpEntryKind::Directory)
            });
            let any_dirs = indices.iter().any(|&i| {
                filtered.get(i).map_or(false, |e| e.kind == SftpEntryKind::Directory)
            });
            self.sftp_context_menu = Some(SftpContextMenu {
                pos,
                panel: SftpPanel::RightLocal,
                entry_indices: indices,
                entry_names: names,
                all_dirs,
                any_dirs,
            });
        }
        if let Some((pos, entry_idx)) = remote_ctx_menu_req {
            if let Some(ref mut browser) = self.sftp_browser {
                if let Some(idx) = entry_idx {
                    if !browser.selection.is_selected(idx) {
                        browser.selection.select_one(idx);
                    }
                }
                let indices: Vec<usize> = if entry_idx.is_some() {
                    browser.selection.selected.iter().copied().collect()
                } else {
                    vec![]
                };
                let filtered = browser.filtered_entries();
                let names: Vec<String> = indices.iter()
                    .filter_map(|&i| filtered.get(i).map(|e| e.name.clone()))
                    .collect();
                let all_dirs = !indices.is_empty() && indices.iter().all(|&i| {
                    filtered.get(i).map_or(false, |e| e.kind == SftpEntryKind::Directory)
                });
                let any_dirs = indices.iter().any(|&i| {
                    filtered.get(i).map_or(false, |e| e.kind == SftpEntryKind::Directory)
                });
                self.sftp_context_menu = Some(SftpContextMenu {
                    pos,
                    panel: SftpPanel::RightRemote,
                    entry_indices: indices,
                    entry_names: names,
                    all_dirs,
                    any_dirs,
                });
            }
        }
        if let Some((pos, entry_idx)) = left_remote_ctx_menu_req {
            if let Some(ref mut browser) = self.sftp_browser_left {
                if let Some(idx) = entry_idx {
                    if !browser.selection.is_selected(idx) {
                        browser.selection.select_one(idx);
                    }
                }
                let indices: Vec<usize> = if entry_idx.is_some() {
                    browser.selection.selected.iter().copied().collect()
                } else {
                    vec![]
                };
                let filtered = browser.filtered_entries();
                let names: Vec<String> = indices.iter()
                    .filter_map(|&i| filtered.get(i).map(|e| e.name.clone()))
                    .collect();
                let all_dirs = !indices.is_empty() && indices.iter().all(|&i| {
                    filtered.get(i).map_or(false, |e| e.kind == SftpEntryKind::Directory)
                });
                let any_dirs = indices.iter().any(|&i| {
                    filtered.get(i).map_or(false, |e| e.kind == SftpEntryKind::Directory)
                });
                self.sftp_context_menu = Some(SftpContextMenu {
                    pos,
                    panel: SftpPanel::LeftRemote,
                    entry_indices: indices,
                    entry_names: names,
                    all_dirs,
                    any_dirs,
                });
            }
        }

        // ── Render context menu ──
        if let Some(ref menu) = self.sftp_context_menu.as_ref().map(|m| {
            (m.pos, m.panel, m.entry_indices.clone(), m.entry_names.clone(), m.all_dirs, m.any_dirs)
        }) {
            let (pos, panel, entry_indices, entry_names, _all_dirs, any_dirs) = menu.clone();
            let has_entries = !entry_indices.is_empty();
            let is_single = entry_indices.len() == 1;
            let mut close_menu = false;
            let area_resp = egui::Area::new(egui::Id::new("sftp_context_menu"))
                .fixed_pos(pos)
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    egui::Frame {
                        fill: self.theme.bg_elevated,
                        inner_margin: egui::Margin::symmetric(6.0, 4.0),
                        rounding: egui::Rounding::same(6.0),
                        stroke: egui::Stroke::new(1.0, self.theme.border),
                        shadow: egui::epaint::Shadow {
                            offset: egui::vec2(2.0, 2.0),
                            blur: 8.0,
                            spread: 0.0,
                            color: egui::Color32::from_black_alpha(40),
                        },
                        ..Default::default()
                    }
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);

                        if has_entries {
                            // Rename (single selection only)
                            if is_single {
                                if ui.add(
                                    egui::Button::new(
                                        egui::RichText::new(self.language.t("rename")).size(12.0).color(self.theme.fg_primary)
                                    ).frame(false)
                                ).clicked() {
                                    self.sftp_rename_dialog = Some(SftpRenameDialog {
                                        panel,
                                        old_name: entry_names[0].clone(),
                                        new_name: entry_names[0].clone(),
                                        error: String::new(),
                                    });
                                    close_menu = true;
                                }
                            }

                            // Edit (single file only, not directories)
                            if is_single && !any_dirs {
                                if ui.add(
                                    egui::Button::new(
                                        egui::RichText::new(self.language.t("edit_file")).size(12.0).color(self.theme.fg_primary)
                                    ).frame(false)
                                ).clicked() {
                                    self.open_file_for_editing_with_panel(panel, &entry_names[0]);
                                    close_menu = true;
                                }
                            }

                            // Delete (with count for multi-select)
                            let delete_label = if entry_indices.len() > 1 {
                                format!("{} ({})", self.language.t("delete_file"), entry_indices.len())
                            } else {
                                self.language.t("delete_file").to_string()
                            };
                            if ui.add(
                                egui::Button::new(
                                    egui::RichText::new(delete_label).size(12.0).color(self.theme.red)
                                ).frame(false)
                            ).clicked() {
                                self.sftp_confirm_delete = Some(SftpConfirmDelete {
                                    panel,
                                    names: entry_names.clone(),
                                });
                                close_menu = true;
                            }

                            ui.separator();
                        }

                        // New Folder
                        if ui.add(
                            egui::Button::new(
                                egui::RichText::new(self.language.t("new_folder")).size(12.0).color(self.theme.fg_primary)
                            ).frame(false)
                        ).clicked() {
                            self.sftp_new_folder_dialog = Some(SftpNewFolderDialog {
                                panel,
                                name: String::new(),
                                error: String::new(),
                            });
                            close_menu = true;
                        }

                        // New File
                        if ui.add(
                            egui::Button::new(
                                egui::RichText::new(self.language.t("new_file")).size(12.0).color(self.theme.fg_primary)
                            ).frame(false)
                        ).clicked() {
                            self.sftp_new_file_dialog = Some(SftpNewFileDialog {
                                panel,
                                name: String::new(),
                                error: String::new(),
                            });
                            close_menu = true;
                        }
                    });
                });

            // Close menu if clicked outside (skip on the frame it was just opened
            // to avoid the opening right-click from immediately closing it)
            if close_menu {
                self.sftp_context_menu = None;
            } else if !menu_just_opened && ui.ctx().input(|i| i.pointer.any_click()) {
                let menu_rect = area_resp.response.rect;
                if let Some(click_pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
                    if !menu_rect.contains(click_pos) {
                        self.sftp_context_menu = None;
                    }
                }
            }
        }

        // ── Rename dialog ──
        let mut rename_action: Option<(SftpPanel, String, String)> = None;
        let mut close_rename = false;
        if let Some(ref mut dialog) = self.sftp_rename_dialog {
            let mut open = true;
            egui::Window::new(self.language.t("rename"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .fixed_size(egui::vec2(300.0, 0.0))
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .title_bar(false)
                .frame(egui::Frame {
                    fill: self.theme.bg_secondary,
                    rounding: egui::Rounding::same(10.0),
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
                .show(ui.ctx(), |ui| {
                    // Title
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("\u{270F}").size(18.0).color(self.theme.accent));
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(self.language.t("rename")).size(15.0).color(self.theme.fg_primary).strong());
                    });
                    ui.add_space(10.0);

                    ui.label(egui::RichText::new(self.language.t("new_name")).size(12.0).color(self.theme.fg_dim));
                    ui.add_space(4.0);
                    let te = ui.add(
                        egui::TextEdit::singleline(&mut dialog.new_name)
                            .desired_width(260.0)
                    );
                    if te.lost_focus() && ui.ctx().input(|i| i.key_pressed(egui::Key::Enter)) {
                        if !dialog.new_name.is_empty() && dialog.new_name != dialog.old_name {
                            rename_action = Some((dialog.panel, dialog.old_name.clone(), dialog.new_name.clone()));
                            close_rename = true;
                        }
                    }
                    te.request_focus();

                    if !dialog.error.is_empty() {
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(&dialog.error).size(11.0).color(self.theme.red));
                    }

                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(
                                egui::Button::new(
                                    egui::RichText::new(self.language.t("save")).color(egui::Color32::WHITE).size(13.0)
                                )
                                .fill(self.theme.accent)
                                .rounding(6.0)
                                .min_size(egui::vec2(70.0, 32.0))
                            ).clicked() {
                                if !dialog.new_name.is_empty() && dialog.new_name != dialog.old_name {
                                    rename_action = Some((dialog.panel, dialog.old_name.clone(), dialog.new_name.clone()));
                                    close_rename = true;
                                }
                            }
                            if ui.add(
                                egui::Button::new(
                                    egui::RichText::new(self.language.t("cancel")).color(self.theme.fg_dim).size(13.0)
                                )
                                .fill(self.theme.bg_elevated)
                                .rounding(6.0)
                                .min_size(egui::vec2(70.0, 32.0))
                            ).clicked() {
                                close_rename = true;
                            }
                        });
                    });
                });
            if !open {
                close_rename = true;
            }
        }
        if close_rename {
            self.sftp_rename_dialog = None;
        }
        if let Some((panel, old_name, new_name)) = rename_action {
            match panel {
                SftpPanel::LeftLocal => {
                    if let Err(e) = self.local_browser_left.rename(&old_name, &new_name) {
                        log::error!("Local rename error: {}", e);
                    }
                }
                SftpPanel::LeftRemote => {
                    if let Some(ref browser) = self.sftp_browser_left {
                        let from = format!("{}/{}", browser.current_path.trim_end_matches('/'), old_name);
                        let to = format!("{}/{}", browser.current_path.trim_end_matches('/'), new_name);
                        browser.rename(&from, &to);
                    }
                }
                SftpPanel::RightRemote => {
                    if let Some(ref browser) = self.sftp_browser {
                        let from = format!("{}/{}", browser.current_path.trim_end_matches('/'), old_name);
                        let to = format!("{}/{}", browser.current_path.trim_end_matches('/'), new_name);
                        browser.rename(&from, &to);
                    }
                }
                SftpPanel::RightLocal => {
                    if let Err(e) = self.local_browser_right.rename(&old_name, &new_name) {
                        log::error!("Local rename error: {}", e);
                    }
                }
            }
        }

        // ── New Folder dialog ──
        let mut create_dir_action: Option<(SftpPanel, String)> = None;
        let mut close_new_folder = false;
        if let Some(ref mut dialog) = self.sftp_new_folder_dialog {
            let mut open = true;
            egui::Window::new(self.language.t("new_folder"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .fixed_size(egui::vec2(300.0, 0.0))
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .title_bar(false)
                .frame(egui::Frame {
                    fill: self.theme.bg_secondary,
                    rounding: egui::Rounding::same(10.0),
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
                .show(ui.ctx(), |ui| {
                    // Title
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("\u{1F4C1}").size(18.0).color(self.theme.accent));
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(self.language.t("new_folder")).size(15.0).color(self.theme.fg_primary).strong());
                    });
                    ui.add_space(10.0);

                    ui.label(egui::RichText::new(self.language.t("folder_name")).size(12.0).color(self.theme.fg_dim));
                    ui.add_space(4.0);
                    let te = ui.add(
                        egui::TextEdit::singleline(&mut dialog.name)
                            .desired_width(260.0)
                    );
                    if te.lost_focus() && ui.ctx().input(|i| i.key_pressed(egui::Key::Enter)) {
                        if !dialog.name.is_empty() {
                            create_dir_action = Some((dialog.panel, dialog.name.clone()));
                            close_new_folder = true;
                        }
                    }
                    te.request_focus();

                    if !dialog.error.is_empty() {
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(&dialog.error).size(11.0).color(self.theme.red));
                    }

                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(
                                egui::Button::new(
                                    egui::RichText::new(self.language.t("save")).color(egui::Color32::WHITE).size(13.0)
                                )
                                .fill(self.theme.accent)
                                .rounding(6.0)
                                .min_size(egui::vec2(70.0, 32.0))
                            ).clicked() {
                                if !dialog.name.is_empty() {
                                    create_dir_action = Some((dialog.panel, dialog.name.clone()));
                                    close_new_folder = true;
                                }
                            }
                            if ui.add(
                                egui::Button::new(
                                    egui::RichText::new(self.language.t("cancel")).color(self.theme.fg_dim).size(13.0)
                                )
                                .fill(self.theme.bg_elevated)
                                .rounding(6.0)
                                .min_size(egui::vec2(70.0, 32.0))
                            ).clicked() {
                                close_new_folder = true;
                            }
                        });
                    });
                });
            if !open {
                close_new_folder = true;
            }
        }
        if close_new_folder {
            self.sftp_new_folder_dialog = None;
        }
        if let Some((panel, name)) = create_dir_action {
            match panel {
                SftpPanel::LeftLocal => {
                    if let Err(e) = self.local_browser_left.create_dir(&name) {
                        log::error!("Local create dir error: {}", e);
                    }
                }
                SftpPanel::LeftRemote => {
                    if let Some(ref browser) = self.sftp_browser_left {
                        let path = format!("{}/{}", browser.current_path.trim_end_matches('/'), name);
                        browser.create_dir(&path);
                    }
                }
                SftpPanel::RightRemote => {
                    if let Some(ref browser) = self.sftp_browser {
                        let path = format!("{}/{}", browser.current_path.trim_end_matches('/'), name);
                        browser.create_dir(&path);
                    }
                }
                SftpPanel::RightLocal => {
                    if let Err(e) = self.local_browser_right.create_dir(&name) {
                        log::error!("Local create dir error: {}", e);
                    }
                }
            }
        }

        // ── New File dialog ──
        let mut create_file_action: Option<(SftpPanel, String)> = None;
        let mut close_new_file = false;
        if let Some(ref mut dialog) = self.sftp_new_file_dialog {
            let mut open = true;
            egui::Window::new(self.language.t("new_file"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .fixed_size(egui::vec2(300.0, 0.0))
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .title_bar(false)
                .frame(egui::Frame {
                    fill: self.theme.bg_secondary,
                    rounding: egui::Rounding::same(10.0),
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
                .show(ui.ctx(), |ui| {
                    // Title
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("\u{1F4C4}").size(18.0).color(self.theme.accent));
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(self.language.t("new_file")).size(15.0).color(self.theme.fg_primary).strong());
                    });
                    ui.add_space(10.0);

                    ui.label(egui::RichText::new(self.language.t("file_name")).size(12.0).color(self.theme.fg_dim));
                    ui.add_space(4.0);
                    let te = ui.add(
                        egui::TextEdit::singleline(&mut dialog.name)
                            .desired_width(260.0)
                    );
                    if te.lost_focus() && ui.ctx().input(|i| i.key_pressed(egui::Key::Enter)) {
                        if !dialog.name.is_empty() {
                            create_file_action = Some((dialog.panel, dialog.name.clone()));
                            close_new_file = true;
                        }
                    }
                    te.request_focus();

                    if !dialog.error.is_empty() {
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(&dialog.error).size(11.0).color(self.theme.red));
                    }

                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(
                                egui::Button::new(
                                    egui::RichText::new(self.language.t("save")).color(egui::Color32::WHITE).size(13.0)
                                )
                                .fill(self.theme.accent)
                                .rounding(6.0)
                                .min_size(egui::vec2(70.0, 32.0))
                            ).clicked() {
                                if !dialog.name.is_empty() {
                                    create_file_action = Some((dialog.panel, dialog.name.clone()));
                                    close_new_file = true;
                                }
                            }
                            if ui.add(
                                egui::Button::new(
                                    egui::RichText::new(self.language.t("cancel")).color(self.theme.fg_dim).size(13.0)
                                )
                                .fill(self.theme.bg_elevated)
                                .rounding(6.0)
                                .min_size(egui::vec2(70.0, 32.0))
                            ).clicked() {
                                close_new_file = true;
                            }
                        });
                    });
                });
            if !open {
                close_new_file = true;
            }
        }
        if close_new_file {
            self.sftp_new_file_dialog = None;
        }
        if let Some((panel, name)) = create_file_action {
            match panel {
                SftpPanel::LeftLocal => {
                    let path = format!("{}/{}", self.local_browser_left.current_path.trim_end_matches('/'), name);
                    match std::fs::write(&path, b"") {
                        Ok(_) => self.local_browser_left.refresh(),
                        Err(e) => log::error!("Local create file error: {}", e),
                    }
                }
                SftpPanel::LeftRemote => {
                    if let Some(ref browser) = self.sftp_browser_left {
                        let path = format!("{}/{}", browser.current_path.trim_end_matches('/'), name);
                        browser.write_file(&path, Vec::new());
                    }
                }
                SftpPanel::RightRemote => {
                    if let Some(ref browser) = self.sftp_browser {
                        let path = format!("{}/{}", browser.current_path.trim_end_matches('/'), name);
                        browser.write_file(&path, Vec::new());
                    }
                }
                SftpPanel::RightLocal => {
                    let path = format!("{}/{}", self.local_browser_right.current_path.trim_end_matches('/'), name);
                    match std::fs::write(&path, b"") {
                        Ok(_) => self.local_browser_right.refresh(),
                        Err(e) => log::error!("Local create file error: {}", e),
                    }
                }
            }
        }

        // ── Delete confirmation dialog ──
        let mut delete_action: Option<(SftpPanel, Vec<String>)> = None;
        let mut close_delete = false;
        if let Some(ref dialog) = self.sftp_confirm_delete {
            let names = dialog.names.clone();
            let panel = dialog.panel;
            let mut open = true;
            egui::Window::new(self.language.t("delete_file"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .fixed_size(egui::vec2(300.0, 0.0))
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .title_bar(false)
                .frame(egui::Frame {
                    fill: self.theme.bg_secondary,
                    rounding: egui::Rounding::same(10.0),
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
                .show(ui.ctx(), |ui| {
                    // Warning icon + title
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("\u{26A0}").size(18.0).color(self.theme.red));
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(self.language.t("delete_file")).size(15.0).color(self.theme.fg_primary).strong());
                    });
                    ui.add_space(10.0);

                    // Confirmation message (single vs multi)
                    let confirm_msg = if names.len() == 1 {
                        self.language.tf("delete_file_confirm", &names[0])
                    } else {
                        self.language.tf("delete_items_confirm", &names.len().to_string())
                    };
                    ui.label(egui::RichText::new(confirm_msg).size(13.0).color(self.theme.fg_primary));
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(self.language.t("confirm_delete")).size(11.0).color(self.theme.fg_dim));

                    ui.add_space(16.0);

                    // Right-aligned buttons
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(
                                egui::Button::new(
                                    egui::RichText::new(self.language.t("delete_file")).color(egui::Color32::WHITE).size(13.0)
                                )
                                .fill(self.theme.red)
                                .rounding(6.0)
                                .min_size(egui::vec2(70.0, 32.0))
                            ).clicked() {
                                delete_action = Some((panel, names.clone()));
                                close_delete = true;
                            }
                            if ui.add(
                                egui::Button::new(
                                    egui::RichText::new(self.language.t("cancel")).color(self.theme.fg_dim).size(13.0)
                                )
                                .fill(self.theme.bg_elevated)
                                .rounding(6.0)
                                .min_size(egui::vec2(70.0, 32.0))
                            ).clicked() {
                                close_delete = true;
                            }
                        });
                    });
                });
            if !open {
                close_delete = true;
            }
        }
        if close_delete {
            self.sftp_confirm_delete = None;
        }

        // ── Error dialog ──
        let mut close_error = false;
        if let Some(ref dialog) = self.sftp_error_dialog {
            let mut open = true;
            egui::Window::new(dialog.title.clone())
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .fixed_size(egui::vec2(400.0, 0.0))
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .title_bar(false)
                .frame(egui::Frame {
                    fill: self.theme.bg_secondary,
                    rounding: egui::Rounding::same(10.0),
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
                .show(ui.ctx(), |ui| {
                    ui.vertical_centered(|ui| {
                        // Error icon + title
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("❌").size(24.0));
                            ui.add_space(10.0);
                            ui.label(egui::RichText::new(&dialog.title).size(16.0).strong().color(self.theme.fg_primary));
                        });
                        ui.add_space(16.0);
                        // Error message
                        ui.label(egui::RichText::new(&dialog.message).size(13.0).color(self.theme.fg_dim));
                        ui.add_space(20.0);
                        // Close button
                        ui.vertical_centered(|ui| {
                            if ui.add(
                                egui::Button::new(
                                    egui::RichText::new("确定").color(egui::Color32::WHITE).size(13.0)
                                )
                                .fill(self.theme.accent)
                                .rounding(6.0)
                                .min_size(egui::vec2(100.0, 32.0))
                            ).clicked() {
                                close_error = true;
                            }
                        });
                    });
                });
            if !open {
                close_error = true;
            }
        }
        if close_error {
            self.sftp_error_dialog = None;
        }

        if let Some((panel, names)) = delete_action {
            for name in &names {
                match panel {
                    SftpPanel::LeftLocal => {
                        if let Err(e) = self.local_browser_left.delete(name) {
                            let error_msg = format!("删除失败：{}\n\n文件：{}", e, name);
                            self.sftp_error_dialog = Some(SftpErrorDialog {
                                title: "删除文件失败".to_string(),
                                message: error_msg,
                            });
                        }
                    }
                    SftpPanel::LeftRemote => {
                        if let Some(ref browser) = self.sftp_browser_left {
                            let path = format!("{}/{}", browser.current_path.trim_end_matches('/'), name);
                            browser.delete(&path);
                        }
                    }
                    SftpPanel::RightRemote => {
                        if let Some(ref browser) = self.sftp_browser {
                            let path = format!("{}/{}", browser.current_path.trim_end_matches('/'), name);
                            browser.delete(&path);
                        }
                    }
                    SftpPanel::RightLocal => {
                        if let Err(e) = self.local_browser_right.delete(name) {
                            let error_msg = format!("删除失败：{}\n\n文件：{}", e, name);
                            self.sftp_error_dialog = Some(SftpErrorDialog {
                                title: "删除文件失败".to_string(),
                                message: error_msg,
                            });
                        }
                    }
                }
            }
        }

        // ── Handle double-click open file requests ──
        if let Some(idx) = local_left_open_file_req {
            let filtered = self.local_browser_left.filtered_entries();
            if let Some(entry) = filtered.get(idx) {
                if entry.kind != SftpEntryKind::Directory {
                    self.open_file_for_editing(true, &entry.name.clone());
                }
            }
        }
        if let Some(idx) = local_right_open_file_req {
            let filtered = self.local_browser_right.filtered_entries();
            if let Some(entry) = filtered.get(idx) {
                if entry.kind != SftpEntryKind::Directory {
                    self.open_file_for_editing(true, &entry.name.clone());
                }
            }
        }
        if let Some(idx) = remote_open_file_req {
            if let Some(ref browser) = self.sftp_browser {
                let filtered = browser.filtered_entries();
                if let Some(entry) = filtered.get(idx) {
                    if entry.kind != SftpEntryKind::Directory {
                        self.open_file_for_editing(false, &entry.name.clone());
                    }
                }
            }
        }
        if let Some(idx) = left_remote_open_file_req {
            if let Some(ref browser) = self.sftp_browser_left {
                let filtered = browser.filtered_entries();
                if let Some(entry) = filtered.get(idx) {
                    if entry.kind != SftpEntryKind::Directory {
                        self.open_file_for_editing(false, &entry.name.clone());
                    }
                }
            }
        }

        // ── Editor dialog ──
        self.show_editor_dialog(ui);
    }

    /// Open a file for editing in the built-in editor.
    fn open_file_for_editing(&mut self, is_local: bool, file_name: &str) {
        if is_local {
            let dir = self.local_browser_left.current_path.clone();
            match self.local_browser_left.read_file(file_name) {
                Ok(content) => {
                    self.sftp_editor_dialog = Some(SftpEditorDialog {
                        panel: SftpPanel::LeftLocal,
                        file_path: format!("{}/{}", dir.trim_end_matches('/'), file_name),
                        file_name: file_name.to_string(),
                        directory: dir,
                        content: content.clone(),
                        original_content: content,
                        loading: false,
                        is_new_file: false,
                        error: String::new(),
                        save_as_name: String::new(),
                    });
                }
                Err(e) => {
                    self.sftp_editor_dialog = Some(SftpEditorDialog {
                        panel: SftpPanel::LeftLocal,
                        file_path: format!("{}/{}", dir.trim_end_matches('/'), file_name),
                        file_name: file_name.to_string(),
                        directory: dir,
                        content: String::new(),
                        original_content: String::new(),
                        loading: false,
                        is_new_file: false,
                        error: e,
                        save_as_name: String::new(),
                    });
                }
            }
        } else {
            // Default to right remote for backward compatibility
            if let Some(ref browser) = self.sftp_browser {
                let dir = browser.current_path.clone();
                let full_path = format!("{}/{}", dir.trim_end_matches('/'), file_name);
                browser.read_file(&full_path);
                self.sftp_editor_dialog = Some(SftpEditorDialog {
                    panel: SftpPanel::RightRemote,
                    file_path: full_path,
                    file_name: file_name.to_string(),
                    directory: dir,
                    content: String::new(),
                    original_content: String::new(),
                    loading: true,
                    is_new_file: false,
                    error: String::new(),
                    save_as_name: String::new(),
                });
            }
        }
    }

    /// Open a file for editing in the built-in editor (with panel support).
    fn open_file_for_editing_with_panel(&mut self, panel: SftpPanel, file_name: &str) {
        match panel {
            SftpPanel::LeftLocal => {
                let dir = self.local_browser_left.current_path.clone();
                match self.local_browser_left.read_file(file_name) {
                    Ok(content) => {
                        self.sftp_editor_dialog = Some(SftpEditorDialog {
                            panel: SftpPanel::LeftLocal,
                            file_path: format!("{}/{}", dir.trim_end_matches('/'), file_name),
                            file_name: file_name.to_string(),
                            directory: dir,
                            content: content.clone(),
                            original_content: content,
                            loading: false,
                            is_new_file: false,
                            error: String::new(),
                            save_as_name: String::new(),
                        });
                    }
                    Err(e) => {
                        self.sftp_editor_dialog = Some(SftpEditorDialog {
                            panel: SftpPanel::LeftLocal,
                            file_path: format!("{}/{}", dir.trim_end_matches('/'), file_name),
                            file_name: file_name.to_string(),
                            directory: dir,
                            content: String::new(),
                            original_content: String::new(),
                            loading: false,
                            is_new_file: false,
                            error: e,
                            save_as_name: String::new(),
                        });
                    }
                }
            }
            SftpPanel::LeftRemote => {
                if let Some(ref browser) = self.sftp_browser_left {
                    let dir = browser.current_path.clone();
                    let full_path = format!("{}/{}", dir.trim_end_matches('/'), file_name);
                    browser.read_file(&full_path);
                    self.sftp_editor_dialog = Some(SftpEditorDialog {
                        panel: SftpPanel::LeftRemote,
                        file_path: full_path,
                        file_name: file_name.to_string(),
                        directory: dir,
                        content: String::new(),
                        original_content: String::new(),
                        loading: true,
                        is_new_file: false,
                        error: String::new(),
                        save_as_name: String::new(),
                    });
                }
            }
            SftpPanel::RightRemote => {
                if let Some(ref browser) = self.sftp_browser {
                    let dir = browser.current_path.clone();
                    let full_path = format!("{}/{}", dir.trim_end_matches('/'), file_name);
                    browser.read_file(&full_path);
                    self.sftp_editor_dialog = Some(SftpEditorDialog {
                        panel: SftpPanel::RightRemote,
                        file_path: full_path,
                        file_name: file_name.to_string(),
                        directory: dir,
                        content: String::new(),
                        original_content: String::new(),
                        loading: true,
                        is_new_file: false,
                        error: String::new(),
                        save_as_name: String::new(),
                    });
                }
            }
            SftpPanel::RightLocal => {
                let dir = self.local_browser_right.current_path.clone();
                match self.local_browser_right.read_file(file_name) {
                    Ok(content) => {
                        self.sftp_editor_dialog = Some(SftpEditorDialog {
                            panel: SftpPanel::RightLocal,
                            file_path: format!("{}/{}", dir.trim_end_matches('/'), file_name),
                            file_name: file_name.to_string(),
                            directory: dir,
                            content: content.clone(),
                            original_content: content,
                            loading: false,
                            is_new_file: false,
                            error: String::new(),
                            save_as_name: String::new(),
                        });
                    }
                    Err(e) => {
                        self.sftp_editor_dialog = Some(SftpEditorDialog {
                            panel: SftpPanel::RightLocal,
                            file_path: format!("{}/{}", dir.trim_end_matches('/'), file_name),
                            file_name: file_name.to_string(),
                            directory: dir,
                            content: String::new(),
                            original_content: String::new(),
                            loading: false,
                            is_new_file: false,
                            error: e,
                            save_as_name: String::new(),
                        });
                    }
                }
            }
        }
    }

    /// Render the editor dialog window.
    fn show_editor_dialog(&mut self, ui: &mut egui::Ui) {
        let mut close_editor = false;
        let mut save_action: Option<(SftpPanel, String, String)> = None; // (panel, path, content)

        if let Some(ref mut dialog) = self.sftp_editor_dialog {
            let has_unsaved = dialog.content != dialog.original_content;

            let mut open = true;
            // Use fixed title and ID to prevent window flickering when title changes (e.g. adding "*" for unsaved)
            egui::Window::new("sftp_editor")
                .id(egui::Id::new("sftp_editor_window"))
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .min_size(egui::vec2(400.0, 200.0))
                .default_width(680.0)
                .default_height(500.0)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .pivot(egui::Align2::CENTER_CENTER)
                .title_bar(false)
                .frame(egui::Frame {
                    fill: self.theme.bg_primary,
                    rounding: egui::Rounding::same(10.0),
                    inner_margin: egui::Margin::ZERO,
                    stroke: egui::Stroke::new(1.0, self.theme.border),
                    shadow: egui::epaint::Shadow {
                        offset: egui::vec2(0.0, 6.0),
                        blur: 24.0,
                        spread: 4.0,
                        color: egui::Color32::from_black_alpha(100),
                    },
                    ..Default::default()
                })
                .show(ui.ctx(), |ui| {
                    let is_new_unnamed = dialog.is_new_file && dialog.file_path.is_empty();

                    // ── Title bar ──
                    {
                        let title_frame = egui::Frame {
                            fill: self.theme.bg_secondary,
                            inner_margin: egui::Margin { left: 16.0, right: 16.0, top: 12.0, bottom: 12.0 },
                            rounding: egui::Rounding { nw: 10.0, ne: 10.0, sw: 0.0, se: 0.0 },
                            ..Default::default()
                        };
                        title_frame.show(ui, |ui| {
                            ui.horizontal(|ui| {
                                // File icon
                                ui.label(egui::RichText::new("\u{1F4C4}").size(14.0).color(self.theme.accent));
                                ui.add_space(6.0);
                                // Filename
                                ui.label(egui::RichText::new(&dialog.file_name).size(13.0).color(self.theme.fg_primary).strong());
                                // Unsaved indicator
                                if has_unsaved {
                                    ui.add_space(4.0);
                                    ui.label(egui::RichText::new("\u{25CF}").size(10.0).color(self.theme.accent));
                                }
                                // Path (right-aligned, dim)
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    // Close button
                                    if ui.add(
                                        egui::Button::new(
                                            egui::RichText::new("\u{2715}").size(12.0).color(self.theme.fg_dim)
                                        ).frame(false)
                                    ).clicked() {
                                        close_editor = true;
                                    }
                                    ui.add_space(8.0);
                                    if !dialog.directory.is_empty() {
                                        let dir_display = if dialog.directory.len() > 40 {
                                            format!("...{}", &dialog.directory[dialog.directory.len()-37..])
                                        } else {
                                            dialog.directory.clone()
                                        };
                                        ui.label(egui::RichText::new(dir_display).size(11.0).color(self.theme.fg_dim).family(egui::FontFamily::Monospace));
                                    }
                                });
                            });
                        });
                    }

                    // Separator line
                    let sep_rect = ui.available_rect_before_wrap();
                    ui.painter().hline(
                        sep_rect.min.x..=sep_rect.max.x,
                        sep_rect.min.y,
                        egui::Stroke::new(1.0, self.theme.border),
                    );

                    if dialog.loading {
                        ui.vertical_centered(|ui| {
                            ui.add_space(60.0);
                            ui.spinner();
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new(self.language.t("loading")).color(self.theme.fg_dim));
                        });
                        return;
                    }

                    if !dialog.error.is_empty() {
                        let err_frame = egui::Frame {
                            fill: self.theme.bg_primary,
                            inner_margin: egui::Margin::same(20.0),
                            ..Default::default()
                        };
                        err_frame.show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("\u{26A0}").size(16.0).color(self.theme.red));
                                ui.add_space(6.0);
                                ui.label(egui::RichText::new(&dialog.error).color(self.theme.red).size(12.0));
                            });
                            ui.add_space(16.0);
                            ui.horizontal(|ui| {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.add(
                                        egui::Button::new(
                                            egui::RichText::new(self.language.t("cancel")).color(self.theme.fg_dim).size(13.0)
                                        )
                                        .fill(self.theme.bg_elevated)
                                        .rounding(6.0)
                                        .min_size(egui::vec2(70.0, 32.0))
                                    ).clicked() {
                                        close_editor = true;
                                    }
                                });
                            });
                        });
                        return;
                    }

                    // ── New file name input ──
                    if is_new_unnamed {
                        let name_frame = egui::Frame {
                            fill: self.theme.bg_secondary,
                            inner_margin: egui::Margin { left: 16.0, right: 16.0, top: 8.0, bottom: 8.0 },
                            ..Default::default()
                        };
                        name_frame.show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(self.language.t("file_name")).size(12.0).color(self.theme.fg_dim));
                                ui.add_space(6.0);
                                ui.add(
                                    egui::TextEdit::singleline(&mut dialog.save_as_name)
                                        .desired_width(ui.available_width())
                                        .font(egui::FontId::monospace(12.0))
                                );
                            });
                        });
                        // Separator
                        let sep_rect2 = ui.available_rect_before_wrap();
                        ui.painter().hline(
                            sep_rect2.min.x..=sep_rect2.max.x,
                            sep_rect2.min.y,
                            egui::Stroke::new(1.0, self.theme.border),
                        );
                    }

                    // ── Code editor area ──
                    {
                        let editor_frame = egui::Frame {
                            fill: self.theme.bg_primary,
                            inner_margin: egui::Margin { left: 4.0, right: 4.0, top: 4.0, bottom: 4.0 },
                            ..Default::default()
                        };
                        let max_editor_h = 500.0_f32;
                        editor_frame.show(ui, |ui| {
                            egui::ScrollArea::both()
                                .max_height(max_editor_h)
                                .show(ui, |ui| {
                                    let response = ui.add(
                                        egui::TextEdit::multiline(&mut dialog.content)
                                            .id(egui::Id::new("sftp_editor_content"))
                                            .desired_width(f32::INFINITY)
                                            .desired_rows(20)
                                            .font(egui::FontId::monospace(self.font_size))
                                            .code_editor()
                                            .lock_focus(true)
                                    );
                                    let _ = response; // response unused, just for focus
                                });
                        });
                    }

                    // ── Bottom bar ──
                    {
                        // Separator
                        let sep_rect3 = ui.available_rect_before_wrap();
                        ui.painter().hline(
                            sep_rect3.min.x..=sep_rect3.max.x,
                            sep_rect3.min.y,
                            egui::Stroke::new(1.0, self.theme.border),
                        );
                        let bar_frame = egui::Frame {
                            fill: self.theme.bg_secondary,
                            inner_margin: egui::Margin { left: 16.0, right: 16.0, top: 8.0, bottom: 8.0 },
                            rounding: egui::Rounding { nw: 0.0, ne: 0.0, sw: 10.0, se: 10.0 },
                            ..Default::default()
                        };
                        bar_frame.show(ui, |ui| {
                            ui.horizontal(|ui| {
                                // Left: line count info
                                let line_count = dialog.content.lines().count().max(1);
                                let char_count = dialog.content.len();
                                ui.label(
                                    egui::RichText::new(self.language.tf2("lines_and_chars", &line_count.to_string(), &char_count.to_string()))
                                        .size(11.0).color(self.theme.fg_dim).family(egui::FontFamily::Monospace)
                                );

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    // Save button
                                    if ui.add(
                                        egui::Button::new(
                                            egui::RichText::new(self.language.t("save")).color(egui::Color32::WHITE).size(12.0)
                                        )
                                        .fill(self.theme.accent)
                                        .rounding(6.0)
                                        .min_size(egui::vec2(64.0, 28.0))
                                    ).clicked() {
                                        if is_new_unnamed {
                                            if !dialog.save_as_name.is_empty() {
                                                let path = format!(
                                                    "{}/{}",
                                                    dialog.directory.trim_end_matches('/'),
                                                    dialog.save_as_name
                                                );
                                                save_action = Some((dialog.panel, path, dialog.content.clone()));
                                            }
                                        } else {
                                            save_action = Some((dialog.panel, dialog.file_path.clone(), dialog.content.clone()));
                                        }
                                    }
                                    // Unsaved indicator
                                    if has_unsaved {
                                        ui.add_space(4.0);
                                        ui.label(egui::RichText::new(self.language.t("unsaved_changes")).color(self.theme.fg_dim).size(11.0));
                                    }
                                });
                            });
                        });
                    }
                });

            if !open {
                close_editor = true;
            }
        }

        // Execute save
        if let Some((panel, path, content)) = save_action {
            match panel {
                SftpPanel::LeftLocal => {
                    let file_name = std::path::Path::new(&path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();
                    match self.local_browser_left.write_file(&file_name, &content) {
                        Ok(_) => {
                            close_editor = true;
                        }
                        Err(e) => {
                            if let Some(ref mut dialog) = self.sftp_editor_dialog {
                                dialog.error = e;
                            }
                        }
                    }
                }
                SftpPanel::LeftRemote => {
                    if let Some(ref browser) = self.sftp_browser_left {
                        browser.write_file(&path, content.as_bytes().to_vec());
                        // Refresh remote dir to show changes
                        let current = browser.current_path.clone();
                        browser.navigate(&current);
                        close_editor = true;
                    }
                }
                SftpPanel::RightRemote => {
                    if let Some(ref browser) = self.sftp_browser {
                        browser.write_file(&path, content.as_bytes().to_vec());
                        // Refresh remote dir to show changes
                        let current = browser.current_path.clone();
                        browser.navigate(&current);
                        close_editor = true;
                    }
                }
                SftpPanel::RightLocal => {
                    let file_name = std::path::Path::new(&path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();
                    match self.local_browser_right.write_file(&file_name, &content) {
                        Ok(_) => {
                            close_editor = true;
                        }
                        Err(e) => {
                            if let Some(ref mut dialog) = self.sftp_editor_dialog {
                                dialog.error = e;
                            }
                        }
                    }
                }
            }
        }

        if close_editor {
            self.sftp_editor_dialog = None;
        }
    }
}

/// Render breadcrumb path navigation. Each path segment is a clickable button.
pub fn render_breadcrumbs(
    ui: &mut egui::Ui,
    current_path: &str,
    navigate_to: &mut Option<String>,
    _is_local: bool,
    theme: &ThemeColors,
) {
    let parts: Vec<&str> = current_path.split('/').filter(|s| !s.is_empty()).collect();

    if ui
        .add(egui::Button::new(egui::RichText::new("/").color(theme.fg_dim).size(12.0).family(egui::FontFamily::Monospace)).frame(false))
        .clicked()
    {
        *navigate_to = Some("/".to_string());
    }

    for (i, part) in parts.iter().enumerate() {
        ui.add_space(0.0);
        ui.label(egui::RichText::new("/").color(theme.fg_dim).size(10.0));
        ui.add_space(0.0);

        let is_last = i == parts.len() - 1;
        let text = egui::RichText::new(*part)
            .color(if is_last { theme.fg_primary } else { theme.accent })
            .size(12.0)
            .family(egui::FontFamily::Monospace);

        if is_last {
            ui.label(text);
        } else if ui.add(egui::Button::new(text).frame(false)).clicked() {
            let target: String = format!("/{}", parts[..=i].join("/"));
            *navigate_to = Some(target);
        }
    }
}

/// Apply a SelectionAction to a FileSelection.
fn apply_selection_action(selection: &mut FileSelection, action: SelectionAction, entry_count: usize) {
    match action {
        SelectionAction::Single(i) => selection.select_one(i),
        SelectionAction::Toggle(i) => selection.toggle(i),
        SelectionAction::Range(i) => selection.select_range(i),
        SelectionAction::SelectAll => selection.select_all(entry_count),
        SelectionAction::FocusMove(i) => selection.select_one(i),
        SelectionAction::FocusExtend(i) => selection.extend_to(i),
        SelectionAction::DeselectAll => selection.clear(),
    }
}

/// Render a file listing panel (reused for both local and remote).
/// Each entry is draggable via egui DnD; the caller handles drop detection.
pub fn render_file_panel(
    ui: &mut egui::Ui,
    entries: &[SftpEntry],
    selection: &FileSelection,
    navigate_to: &mut Option<String>,
    selection_action: &mut Option<SelectionAction>,
    is_local: bool,
    current_path: &str,
    theme: &ThemeColors,
    context_menu_request: &mut Option<(egui::Pos2, Option<usize>)>,
    open_file_request: &mut Option<usize>,
    delete_request: &mut bool,
    is_active_panel: bool,
    language: &Language,
    panel_id: &str,
    editor_is_open: bool,
) {
    let row_height = 26.0;
    let status_bar_height = 24.0;

    // Reserve space for the status bar at the bottom
    let available = ui.available_rect_before_wrap();
    let scroll_rect = egui::Rect::from_min_max(
        available.min,
        egui::pos2(available.max.x, available.max.y - status_bar_height),
    );
    let status_rect = egui::Rect::from_min_max(
        egui::pos2(available.min.x, available.max.y - status_bar_height),
        available.max,
    );

    // Keyboard handling (only for active panel and no other UI wants keyboard input and editor is not open)
    if is_active_panel && !entries.is_empty() && !ui.ctx().wants_keyboard_input() && !editor_is_open {
        let modifiers = ui.ctx().input(|i| i.modifiers);
        let cmd = modifiers.command; // Cmd on macOS, Ctrl on others
        let shift = modifiers.shift;

        // Cmd/Ctrl+A → select all
        if cmd && ui.ctx().input(|i| i.key_pressed(egui::Key::A)) {
            *selection_action = Some(SelectionAction::SelectAll);
        }

        // Arrow keys
        let focus = selection.focus.unwrap_or(0);
        if ui.ctx().input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            if focus > 0 {
                let new_focus = focus - 1;
                if shift {
                    *selection_action = Some(SelectionAction::FocusExtend(new_focus));
                } else {
                    *selection_action = Some(SelectionAction::FocusMove(new_focus));
                }
            }
        }
        if ui.ctx().input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            if focus + 1 < entries.len() {
                let new_focus = focus + 1;
                if shift {
                    *selection_action = Some(SelectionAction::FocusExtend(new_focus));
                } else {
                    *selection_action = Some(SelectionAction::FocusMove(new_focus));
                }
            }
        }

        // Enter → open focused entry
        if ui.ctx().input(|i| i.key_pressed(egui::Key::Enter)) {
            if let Some(f) = selection.focus {
                if let Some(entry) = entries.get(f) {
                    if entry.kind == SftpEntryKind::Directory {
                        *navigate_to = Some(entry.name.clone());
                    } else {
                        *open_file_request = Some(f);
                    }
                }
            }
        }

        // Delete / Backspace → request delete
        if ui.ctx().input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) {
            if !selection.is_empty() {
                *delete_request = true;
            }
        }
    }

    // File listing scroll area
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(scroll_rect), |ui| {
        egui::ScrollArea::vertical()
            .id_salt(panel_id)
            .show(ui, |ui| {
            for (i, entry) in entries.iter().enumerate() {
                let is_selected = selection.is_selected(i);
                let is_focus = selection.focus == Some(i);
                let is_dir = entry.kind == SftpEntryKind::Directory;

                let width = ui.available_width();
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(width, row_height),
                    egui::Sense::click_and_drag(),
                );

                // Background: selected, focused, or hovered
                if is_selected || resp.hovered() {
                    let bg = if is_selected {
                        theme.accent_alpha(30)
                    } else {
                        theme.hover_bg
                    };
                    ui.painter().rect_filled(rect, 0.0, bg);
                }

                // Focus indicator (subtle left border)
                if is_focus && is_active_panel {
                    let focus_rect = egui::Rect::from_min_size(
                        rect.min,
                        egui::vec2(2.0, rect.height()),
                    );
                    ui.painter().rect_filled(focus_rect, 0.0, theme.accent);
                }

                // Icon
                let icon = match entry.kind {
                    SftpEntryKind::Directory => "\u{1F4C1}",
                    SftpEntryKind::Symlink => "\u{1F517}",
                    _ => "\u{1F4C4}",
                };
                ui.painter().text(
                    egui::pos2(rect.min.x + 8.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    icon,
                    egui::FontId::proportional(11.0),
                    theme.fg_dim,
                );

                // Name (truncated to not overlap with permissions/size)
                let name_color = if is_dir { theme.accent } else { theme.fg_primary };
                let display_name = if is_dir {
                    format!("{}/", entry.name)
                } else {
                    entry.name.clone()
                };
                let name_x = rect.min.x + 28.0;
                // Permissions are right-aligned at rect.max.x - 75.0, text extends left
                // "drwxrwxrwx" is ~60px wide, so permissions end around rect.max.x - 135.0
                // Leave some padding, name should not extend past rect.max.x - 145.0
                let name_max_x = (rect.max.x - 145.0).max(name_x + 20.0);
                let max_name_width = name_max_x - name_x;

                // Create a layout job that doesn't wrap but truncates with ellipsis
                let mut job = egui::text::LayoutJob::simple_singleline(
                    display_name.clone(),
                    egui::FontId::proportional(12.0),
                    name_color,
                );
                job.wrap = egui::text::TextWrapping::truncate_at_width(max_name_width);

                let galley = ui.fonts(|f| f.layout_job(job));

                ui.painter().galley(
                    egui::pos2(name_x, rect.center().y - galley.size().y * 0.5),
                    galley,
                    name_color,
                );

                // Size (right-aligned)
                if !is_dir {
                    if let Some(s) = entry.size {
                        ui.painter().text(
                            egui::pos2(rect.max.x - 8.0, rect.center().y),
                            egui::Align2::RIGHT_CENTER,
                            format_file_size(s),
                            egui::FontId::proportional(11.0),
                            theme.fg_dim,
                        );
                    }
                }

                // Permissions (right-aligned, before size)
                if let Some(mode) = entry.permissions {
                    ui.painter().text(
                        egui::pos2(rect.max.x - 75.0, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        format_permissions(mode),
                        egui::FontId::monospace(10.0),
                        theme.fg_dim,
                    );
                }

                // Drag payload (multi-select aware) — only set when dragging
                if resp.dragged() {
                    if is_selected && selection.count() > 1 {
                        // Dragging from a selected item → pack all selected entries
                        let drag_entries: Vec<DragEntry> = selection.selected.iter()
                            .filter_map(|&idx| entries.get(idx).map(|e| DragEntry {
                                full_path: format!("{}/{}", current_path.trim_end_matches('/'), e.name),
                                entry_name: e.name.clone(),
                                is_dir: e.kind == SftpEntryKind::Directory,
                            }))
                            .collect();
                        resp.dnd_set_drag_payload(DragPayload {
                            is_local,
                            entries: drag_entries,
                        });
                    } else {
                        // Single item drag
                        let full_path = format!(
                            "{}/{}",
                            current_path.trim_end_matches('/'),
                            entry.name
                        );
                        resp.dnd_set_drag_payload(DragPayload {
                            is_local,
                            entries: vec![DragEntry {
                                full_path,
                                entry_name: entry.name.clone(),
                                is_dir,
                            }],
                        });
                    }
                }

                if resp.secondary_clicked() {
                    if let Some(pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
                        *context_menu_request = Some((pos, Some(i)));
                    }
                } else if resp.double_clicked() {
                    if is_dir {
                        *navigate_to = Some(entry.name.clone());
                    } else {
                        *open_file_request = Some(i);
                    }
                } else if resp.clicked() {
                    let modifiers = ui.ctx().input(|i| i.modifiers);
                    if modifiers.command {
                        *selection_action = Some(SelectionAction::Toggle(i));
                    } else if modifiers.shift {
                        *selection_action = Some(SelectionAction::Range(i));
                    } else {
                        *selection_action = Some(SelectionAction::Single(i));
                    }
                }
            }

            // Click on blank area → deselect all
            let remaining = ui.available_rect_before_wrap();
            let blank_resp = ui.allocate_rect(remaining, egui::Sense::click());
            if blank_resp.secondary_clicked() {
                if let Some(pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
                    *context_menu_request = Some((pos, None));
                }
            } else if blank_resp.clicked() {
                *selection_action = Some(SelectionAction::DeselectAll);
            }
        });
    });

    // Status bar at the bottom
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(status_rect), |ui| {
        ui.painter().rect_filled(status_rect, 0.0, theme.bg_secondary);

        let sel_count = selection.count();
        let dir_count = entries.iter().filter(|e| e.kind == SftpEntryKind::Directory).count();
        let file_count = entries.iter().filter(|e| e.kind == SftpEntryKind::File).count();
        let total_size: u64 = entries.iter()
            .filter(|e| e.kind == SftpEntryKind::File)
            .filter_map(|e| e.size)
            .sum();

        let status_text = if sel_count > 0 {
            let sel_size: u64 = selection.selected.iter()
                .filter_map(|&i| entries.get(i))
                .filter(|e| e.kind == SftpEntryKind::File)
                .filter_map(|e| e.size)
                .sum();
            format!(
                "{} \u{2014} {}  |  {} files, {} folders",
                language.tf("n_selected", &sel_count.to_string()),
                format_file_size(sel_size),
                file_count,
                dir_count,
            )
        } else {
            format!(
                "{} files, {} folders  \u{2014}  {}",
                file_count,
                dir_count,
                format_file_size(total_size),
            )
        };

        ui.painter().text(
            egui::pos2(status_rect.min.x + 8.0, status_rect.center().y),
            egui::Align2::LEFT_CENTER,
            &status_text,
            egui::FontId::proportional(11.0),
            theme.fg_dim,
        );
    });
    ui.allocate_rect(available, egui::Sense::hover());
}

/// Format transfer speed in human-readable form (e.g. "1.2 MB/s").
pub fn format_transfer_speed(bps: f64) -> String {
    if bps < 1024.0 {
        format!("{:.0} B/s", bps)
    } else if bps < 1024.0 * 1024.0 {
        format!("{:.1} KB/s", bps / 1024.0)
    } else if bps < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB/s", bps / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB/s", bps / (1024.0 * 1024.0 * 1024.0))
    }
}

pub fn format_file_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Format Unix file permissions to rwxrwxrwx string.
pub fn format_permissions(mode: u32) -> String {
    let file_type = match mode & 0o170000 {
        0o040000 => 'd',
        0o120000 => 'l',
        _ => '-',
    };
    let mut s = String::with_capacity(10);
    s.push(file_type);
    for shift in [6, 3, 0] {
        let bits = (mode >> shift) & 0o7;
        s.push(if bits & 4 != 0 { 'r' } else { '-' });
        s.push(if bits & 2 != 0 { 'w' } else { '-' });
        s.push(if bits & 1 != 0 { 'x' } else { '-' });
    }
    s
}
