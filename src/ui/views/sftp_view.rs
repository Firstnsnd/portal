//! # SFTP View
//!
//! Rendering for the SFTP (file transfer) page with dual-panel file browser.

use eframe::egui;

// Import SFTP view components from subdirectory
use crate::ui::views::sftp::{DragPayload, SelectionAction, MoveToDirRequest};
use crate::ui::views::sftp::{render_breadcrumbs, render_file_panel, apply_selection_action};
use crate::ui::views::sftp::render_transfer_progress;
use crate::ui::pane_view::{ViewActions, WindowContext};
use crate::ui::pane::AppWindow;
use crate::ui::types::sftp_types::{SftpContextMenu, SftpRenameDialog, SftpNewFolderDialog, SftpNewFileDialog, SftpConfirmDelete, SftpEditorDialog, SftpErrorDialog, SftpPanel};
use crate::ui::types::TerminalSession;
use crate::sftp::{SftpConnectionState, SftpEntryKind};
use crate::config;
use crate::ui::widgets;
use crate::ui::tokens::DIALOG_WIDTH_SM;

/// Render SFTP view for this window
pub fn render_sftp_view(window: &mut AppWindow, ui: &mut egui::Ui, cx: &mut WindowContext) -> ViewActions {
    // Configure text cursor for smoother appearance
    ui.ctx().style_mut(|style| {
        style.visuals.text_cursor.stroke = egui::Stroke::new(2.0, cx.theme.fg_primary);
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
    ui.painter().rect_filled(shadow_rect, 0.0, cx.theme.hover_shadow);

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
    let mut local_left_move_to_dir: Option<MoveToDirRequest> = None;
    let mut local_right_move_to_dir: Option<MoveToDirRequest> = None;
    let mut remote_move_to_dir: Option<MoveToDirRequest> = None;
    let mut left_remote_move_to_dir: Option<MoveToDirRequest> = None;
    let mut remote_refresh_request = false;
    let mut left_cancel_connect_request = false;
    let mut right_cancel_connect_request = false;

    // Detect panel focus from mouse clicks
    if ui.ctx().input(|i| i.pointer.any_pressed()) {
        if let Some(pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
            if left_panel_rect.contains(pos) {
                window.sftp_active_panel_is_local = true;
            } else if right_panel_rect.contains(pos) {
                window.sftp_active_panel_is_local = false;
            }
        }
    }

    // ── LEFT PANEL: Local or Remote ──
    let mut left_connect_host: Option<usize> = None;
    let mut left_disconnect_request = false;
    let mut left_toggle_hidden_files = false;
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(left_panel_rect), |ui| {
        if window.left_panel_is_local {
            // ── LEFT PANEL: Local ──
            // Path bar with breadcrumbs
            egui::Frame {
                fill: cx.theme.bg_secondary,
                inner_margin: egui::Margin::symmetric(8.0, 6.0),
                stroke: egui::Stroke::NONE,
                ..Default::default()
            }
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Back button
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new("\u{2190}").color(cx.theme.accent).size(13.0),
                        ).frame(false))
                        .on_hover_text(cx.language.t("back"))
                        .clicked()
                    {
                        local_left_navigate_to = Some("..".to_string());
                    }
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(cx.language.t("local")).color(cx.theme.fg_dim).size(13.0).strong());
                    ui.add_space(4.0);
                    // Breadcrumb path
                    render_breadcrumbs(ui, &window.local_browser_left.current_path, &mut local_left_navigate_to, true, &cx.theme);
                    // Refresh + switch to remote button (right-aligned)
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Switch to remote button
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("\u{2195}").color(cx.theme.accent).size(13.0),
                            ).frame(false))
                            .on_hover_text(cx.language.t("switch_to_remote"))
                            .clicked()
                        {
                            window.left_panel_is_local = false;
                        }
                        ui.add_space(8.0);
                        // Show/Hide hidden files toggle
                        let toggle_text = if window.local_browser_left.show_hidden_files {
                            cx.language.t("hide_hidden_files")
                        } else {
                            cx.language.t("show_hidden_files")
                        };
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new(if window.local_browser_left.show_hidden_files { "\u{1F440}" } else { "\u{1F441}" })
                                    .color(if window.local_browser_left.show_hidden_files { cx.theme.accent } else { cx.theme.fg_dim })
                                    .size(13.0)
                            ).frame(false))
                            .on_hover_text(toggle_text)
                            .clicked()
                        {
                            local_left_toggle_hidden = true;
                        }
                        ui.add_space(8.0);
                        let is_refreshing = window.sftp_local_left_refresh_start
                            .map_or(false, |t| t.elapsed() < std::time::Duration::from_millis(300));
                        if is_refreshing {
                            ui.spinner();
                            ui.ctx().request_repaint();
                        } else {
                            window.sftp_local_left_refresh_start = None;
                            if ui
                                .add(egui::Button::new(
                                    egui::RichText::new("\u{21BB}").color(cx.theme.accent).size(13.0),
                                ).frame(false))
                                .clicked()
                            {
                                window.sftp_local_left_refresh_start = Some(std::time::Instant::now());
                                window.local_browser_left.refresh();
                            }
                        }
                    });
                });
            });

            let filtered_entries = window.local_browser_left.filtered_entries();
            render_file_panel(
                ui,
                &filtered_entries,
                &window.local_browser_left.selection,
                &mut local_left_navigate_to,
                &mut local_left_selection_action,
                true,
                &window.local_browser_left.current_path,
                &cx.theme,
                &mut local_left_ctx_menu_req,
                &mut local_left_open_file_req,
                &mut local_left_delete_request,
                window.sftp_active_panel_is_local,
                &cx.language,
                "local_left_scroll",
                window.sftp_editor_dialog.is_some(),
                &mut local_left_move_to_dir,
            );
        } else {
            // ── LEFT PANEL: Remote (independent connection) ──
            let sftp_left_remote_refresh_start = window.sftp_left_remote_refresh_start;
            let sftp_active_panel_is_local = window.sftp_active_panel_is_local;
            let sftp_editor_dialog_is_some = window.sftp_editor_dialog.is_some();

            match window.sftp_browser_left.as_mut() {
                None => {
                    // ── No connection: show host list ──
                    egui::Frame {
                        fill: cx.theme.bg_secondary,
                        inner_margin: egui::Margin::symmetric(8.0, 6.0),
                        stroke: egui::Stroke::NONE,
                        ..Default::default()
                    }
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(cx.language.t("remote_select")).color(cx.theme.fg_dim).size(13.0).strong());
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
                            ui.painter().rect_filled(rect, 0.0, cx.theme.hover_bg);
                        }
                        ui.painter().text(
                            egui::pos2(rect.min.x + 16.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            "\u{1F4C2}",
                            egui::FontId::proportional(14.0),
                            cx.theme.accent,
                        );
                        ui.painter().text(
                            egui::pos2(rect.min.x + 38.0, rect.center().y - 7.0),
                            egui::Align2::LEFT_CENTER,
                            cx.language.t("local"),
                            egui::FontId::proportional(13.0),
                            cx.theme.fg_primary,
                        );
                        ui.painter().text(
                            egui::pos2(rect.min.x + 38.0, rect.center().y + 8.0),
                            egui::Align2::LEFT_CENTER,
                            &format!("/{}", window.local_browser_left.current_path.split('/').last().unwrap_or("")),
                            egui::FontId::proportional(10.0),
                            cx.theme.fg_dim,
                        );
                        if resp.clicked() {
                            window.left_panel_is_local = true;
                        }

                        ui.add_space(4.0);

                        // ── SSH hosts ──
                        for (i, host) in cx.hosts.iter().enumerate() {
                            if host.is_local { continue; }
                            let width = ui.available_width();
                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(width, row_h),
                                egui::Sense::click(),
                            );
                            if resp.hovered() {
                                ui.painter().rect_filled(rect, 0.0, cx.theme.hover_bg);
                            }
                            ui.painter().text(
                                egui::pos2(rect.min.x + 16.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                "\u{2195}",
                                egui::FontId::proportional(14.0),
                                cx.theme.accent,
                            );
                            ui.painter().text(
                                egui::pos2(rect.min.x + 38.0, rect.center().y - 7.0),
                                egui::Align2::LEFT_CENTER,
                                &host.name,
                                egui::FontId::proportional(13.0),
                                cx.theme.fg_primary,
                            );
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
                                cx.theme.fg_dim,
                            );
                            if resp.clicked() {
                                left_connect_host = Some(i);
                            }
                        }
                        if cx.hosts.iter().all(|h| h.is_local) {
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
                                cx.theme.fg_dim,
                            );
                        }
                    });
                }
                Some(browser) => {
                    match &browser.state {
                        SftpConnectionState::Connecting => {
                            egui::Frame {
                                fill: cx.theme.bg_secondary,
                                inner_margin: egui::Margin::symmetric(8.0, 6.0),
                                stroke: egui::Stroke::NONE,
                                ..Default::default()
                            }
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(cx.language.t("remote")).color(cx.theme.fg_dim).size(13.0).strong());
                                    ui.allocate_space(egui::vec2(ui.available_width(), 0.0));
                                });
                            });
                            ui.vertical_centered(|ui| {
                                ui.add_space(40.0);
                                ui.label(
                                    egui::RichText::new(cx.language.t("connecting"))
                                        .color(cx.theme.fg_dim)
                                        .size(14.0),
                                );
                                ui.add_space(12.0);
                                if ui.add(
                                    egui::Button::new(egui::RichText::new(cx.language.t("cancel")).color(cx.theme.red).size(13.0))
                                        .frame(false)
                                ).clicked() {
                                    left_cancel_connect_request = true;
                                }
                            });
                        }
                        SftpConnectionState::Error(e) => {
                            let err = e.clone();
                            egui::Frame {
                                fill: cx.theme.bg_secondary,
                                inner_margin: egui::Margin::symmetric(8.0, 6.0),
                                stroke: egui::Stroke::NONE,
                                ..Default::default()
                            }
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(cx.language.t("remote")).color(cx.theme.fg_dim).size(13.0).strong());
                                    ui.allocate_space(egui::vec2(ui.available_width(), 0.0));
                                });
                            });
                            ui.vertical_centered(|ui| {
                                ui.add_space(40.0);
                                ui.label(egui::RichText::new(cx.language.t("connection_failed")).color(cx.theme.red).size(14.0).strong());
                                ui.add_space(6.0);
                                ui.label(egui::RichText::new(&err).color(cx.theme.fg_dim).size(12.0));
                                ui.add_space(12.0);
                                if ui.add(
                                    egui::Button::new(egui::RichText::new(cx.language.t("back")).color(cx.theme.accent).size(13.0))
                                        .frame(false)
                                ).clicked() {
                                    left_cancel_connect_request = true;
                                }
                            });
                        }
                        SftpConnectionState::Connected => {
                            let is_refreshing = sftp_left_remote_refresh_start
                                .map_or(false, |t| t.elapsed() < std::time::Duration::from_millis(300));
                            let mut left_remote_refresh_request = false;

                            egui::Frame {
                                fill: cx.theme.bg_secondary,
                                inner_margin: egui::Margin::symmetric(8.0, 6.0),
                                stroke: egui::Stroke::NONE,
                                ..Default::default()
                            }
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    if ui
                                        .add(egui::Button::new(
                                            egui::RichText::new("\u{2190}").color(cx.theme.accent).size(13.0),
                                        ).frame(false))
                                        .on_hover_text(cx.language.t("back"))
                                        .clicked()
                                    {
                                        left_remote_navigate_to = Some("..".to_string());
                                    }
                                    ui.add_space(4.0);
                                    ui.label(egui::RichText::new(cx.language.tf("remote_host", &browser.host_name)).color(cx.theme.fg_dim).size(13.0).strong());
                                    ui.add_space(4.0);
                                    let current_path_clone = browser.current_path.clone();
                                    render_breadcrumbs(ui, &current_path_clone, &mut left_remote_navigate_to, false, &cx.theme);
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui
                                            .add(egui::Button::new(
                                                egui::RichText::new("\u{2715}").color(cx.theme.red).size(13.0),
                                            ).frame(false))
                                            .on_hover_text(cx.language.t("disconnect"))
                                            .clicked()
                                        {
                                            left_disconnect_request = true;
                                        }
                                        ui.add_space(8.0);
                                        let toggle_text = if browser.show_hidden_files {
                                            cx.language.t("hide_hidden_files")
                                        } else {
                                            cx.language.t("show_hidden_files")
                                        };
                                        if ui
                                            .add(egui::Button::new(
                                                egui::RichText::new(if browser.show_hidden_files { "\u{1F440}" } else { "\u{1F441}" })
                                                    .color(if browser.show_hidden_files { cx.theme.accent } else { cx.theme.fg_dim })
                                                    .size(13.0)
                                            ).frame(false))
                                            .on_hover_text(toggle_text)
                                            .clicked()
                                        {
                                            left_toggle_hidden_files = true;
                                        }
                                        ui.add_space(8.0);
                                        if is_refreshing {
                                            ui.spinner();
                                        } else {
                                            if ui
                                                .add(egui::Button::new(
                                                    egui::RichText::new("\u{21BB}").color(cx.theme.accent).size(13.0),
                                                ).frame(false))
                                                .clicked()
                                            {
                                                left_remote_refresh_request = true;
                                            }
                                        }
                                    });
                                });
                            });

                            let mut new_refresh_start: Option<std::time::Instant> = sftp_left_remote_refresh_start;
                            if left_remote_refresh_request {
                                new_refresh_start = Some(std::time::Instant::now());
                                browser.refresh();
                            } else if !is_refreshing {
                                new_refresh_start = None;
                            }
                            let _refresh_start_to_set = new_refresh_start;

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
                                &cx.theme,
                                &mut left_remote_ctx_menu_req,
                                &mut left_remote_open_file_req,
                                &mut left_remote_delete_request,
                                sftp_active_panel_is_local,
                                &cx.language,
                                "left_remote_scroll",
                                sftp_editor_dialog_is_some,
                                &mut left_remote_move_to_dir,
                            );
                        }
                        SftpConnectionState::Disconnected => {}
                    }
                }
            }
        }
    });

    // ── Handle left panel disconnect request ──
    if left_disconnect_request {
        window.sftp_browser_left = None;
    }
    // ── Handle left panel cancel connect request ──
    if left_cancel_connect_request {
        if let Some(ref b) = window.sftp_browser_left {
            b.cancel_connect();
        }
        window.sftp_browser_left = None;
    }
    if left_toggle_hidden_files {
        if let Some(ref mut b) = window.sftp_browser_left.as_mut() {
            b.toggle_hidden_files();
        }
    }
    if local_left_toggle_hidden {
        window.local_browser_left.toggle_hidden_files();
    }

    // ── RIGHT PANEL: Local or Remote ──
    let is_connected = matches!(
        window.sftp_browser.as_ref().map(|b| &b.state),
        Some(SftpConnectionState::Connected)
    );
    let mut connect_host: Option<usize> = None;
    let mut right_disconnect_request = false;
    let mut right_toggle_hidden_files = false;

    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(right_panel_rect), |ui| {
        if window.right_panel_is_local {
            // ── RIGHT PANEL: Local ──
            egui::Frame {
                fill: cx.theme.bg_secondary,
                inner_margin: egui::Margin::symmetric(8.0, 6.0),
                stroke: egui::Stroke::NONE,
                ..Default::default()
            }
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new("\u{2190}").color(cx.theme.accent).size(13.0),
                        ).frame(false))
                        .on_hover_text(cx.language.t("back"))
                        .clicked()
                    {
                        local_right_navigate_to = Some("..".to_string());
                    }
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(cx.language.t("local")).color(cx.theme.fg_dim).size(13.0).strong());
                    ui.add_space(4.0);
                    render_breadcrumbs(ui, &window.local_browser_right.current_path, &mut local_right_navigate_to, true, &cx.theme);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("\u{2195}").color(cx.theme.accent).size(13.0),
                            ).frame(false))
                            .on_hover_text(cx.language.t("switch_to_remote"))
                            .clicked()
                        {
                            window.right_panel_is_local = false;
                        }
                        ui.add_space(8.0);
                        let toggle_text = if window.local_browser_right.show_hidden_files {
                            cx.language.t("hide_hidden_files")
                        } else {
                            cx.language.t("show_hidden_files")
                        };
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new(if window.local_browser_right.show_hidden_files { "\u{1F440}" } else { "\u{1F441}" })
                                    .color(if window.local_browser_right.show_hidden_files { cx.theme.accent } else { cx.theme.fg_dim })
                                    .size(13.0)
                            ).frame(false))
                            .on_hover_text(toggle_text)
                            .clicked()
                        {
                            local_right_toggle_hidden = true;
                        }
                        ui.add_space(8.0);
                        let is_refreshing = window.sftp_local_right_refresh_start
                            .map_or(false, |t| t.elapsed() < std::time::Duration::from_millis(300));
                        if is_refreshing {
                            ui.spinner();
                            ui.ctx().request_repaint();
                        } else {
                            window.sftp_local_right_refresh_start = None;
                            if ui
                                .add(egui::Button::new(
                                    egui::RichText::new("\u{21BB}").color(cx.theme.accent).size(13.0),
                                ).frame(false))
                                .clicked()
                            {
                                window.sftp_local_right_refresh_start = Some(std::time::Instant::now());
                                window.local_browser_right.refresh();
                            }
                        }
                    });
                });
            });

            let filtered_entries = window.local_browser_right.filtered_entries();
            render_file_panel(
                ui,
                &filtered_entries,
                &window.local_browser_right.selection,
                &mut local_right_navigate_to,
                &mut local_right_selection_action,
                true,
                &window.local_browser_right.current_path,
                &cx.theme,
                &mut local_right_ctx_menu_req,
                &mut local_right_open_file_req,
                &mut local_right_delete_request,
                !window.sftp_active_panel_is_local,
                &cx.language,
                "local_right_scroll",
                window.sftp_editor_dialog.is_some(),
                &mut local_right_move_to_dir,
            );
        } else {
            // ── RIGHT PANEL: Remote (host list or remote browser) ──
            let sftp_remote_refresh_start = window.sftp_remote_refresh_start;
            let sftp_active_panel_is_local = window.sftp_active_panel_is_local;
            let sftp_editor_dialog_is_some = window.sftp_editor_dialog.is_some();

            match window.sftp_browser.as_mut() {
                None => {
                    egui::Frame {
                        fill: cx.theme.bg_secondary,
                        inner_margin: egui::Margin::symmetric(8.0, 6.0),
                        stroke: egui::Stroke::NONE,
                        ..Default::default()
                    }
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(cx.language.t("remote_select")).color(cx.theme.fg_dim).size(13.0).strong());
                            ui.allocate_space(egui::vec2(ui.available_width(), 0.0));
                        });
                    });

                    egui::ScrollArea::vertical()
                        .id_salt("sftp_host_list")
                        .show(ui, |ui| {
                        ui.add_space(8.0);
                        let row_h = 44.0;

                        let width = ui.available_width();
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(width, row_h),
                            egui::Sense::click(),
                        );
                        if resp.hovered() {
                            ui.painter().rect_filled(rect, 0.0, cx.theme.hover_bg);
                        }
                        ui.painter().text(
                            egui::pos2(rect.min.x + 16.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            "\u{1F4C2}",
                            egui::FontId::proportional(14.0),
                            cx.theme.accent,
                        );
                        ui.painter().text(
                            egui::pos2(rect.min.x + 38.0, rect.center().y - 7.0),
                            egui::Align2::LEFT_CENTER,
                            cx.language.t("local"),
                            egui::FontId::proportional(13.0),
                            cx.theme.fg_primary,
                        );
                        ui.painter().text(
                            egui::pos2(rect.min.x + 38.0, rect.center().y + 8.0),
                            egui::Align2::LEFT_CENTER,
                            &format!("/{}", window.local_browser_right.current_path.split('/').last().unwrap_or("")),
                            egui::FontId::proportional(10.0),
                            cx.theme.fg_dim,
                        );
                        if resp.clicked() {
                            window.right_panel_is_local = true;
                        }

                        ui.add_space(4.0);

                        for (i, host) in cx.hosts.iter().enumerate() {
                            if host.is_local { continue; }
                            let width = ui.available_width();
                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(width, row_h),
                                egui::Sense::click(),
                            );
                            if resp.hovered() {
                                ui.painter().rect_filled(rect, 0.0, cx.theme.hover_bg);
                            }
                            ui.painter().text(
                                egui::pos2(rect.min.x + 16.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                "\u{2195}",
                                egui::FontId::proportional(14.0),
                                cx.theme.accent,
                            );
                            ui.painter().text(
                                egui::pos2(rect.min.x + 38.0, rect.center().y - 7.0),
                                egui::Align2::LEFT_CENTER,
                                &host.name,
                                egui::FontId::proportional(13.0),
                                cx.theme.fg_primary,
                            );
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
                                cx.theme.fg_dim,
                            );
                            if resp.clicked() {
                                connect_host = Some(i);
                            }
                        }
                        if cx.hosts.iter().all(|h| h.is_local) {
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
                                cx.theme.fg_dim,
                            );
                        }
                    });
                }
                Some(browser) => {
                    match &browser.state {
                        SftpConnectionState::Connecting => {
                            egui::Frame {
                                fill: cx.theme.bg_secondary,
                                inner_margin: egui::Margin::symmetric(8.0, 6.0),
                                stroke: egui::Stroke::NONE,
                                ..Default::default()
                            }
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(cx.language.t("remote")).color(cx.theme.fg_dim).size(13.0).strong());
                                    ui.allocate_space(egui::vec2(ui.available_width(), 0.0));
                                });
                            });
                            ui.vertical_centered(|ui| {
                                ui.add_space(40.0);
                                ui.label(
                                    egui::RichText::new(cx.language.t("connecting"))
                                        .color(cx.theme.fg_dim)
                                        .size(14.0),
                                );
                                ui.add_space(12.0);
                                if ui.add(
                                    egui::Button::new(egui::RichText::new(cx.language.t("cancel")).color(cx.theme.red).size(13.0))
                                        .frame(false)
                                ).clicked() {
                                    right_cancel_connect_request = true;
                                }
                            });
                        }
                        SftpConnectionState::Error(e) => {
                            let err = e.clone();
                            egui::Frame {
                                fill: cx.theme.bg_secondary,
                                inner_margin: egui::Margin::symmetric(8.0, 6.0),
                                stroke: egui::Stroke::NONE,
                                ..Default::default()
                            }
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(cx.language.t("remote")).color(cx.theme.fg_dim).size(13.0).strong());
                                    ui.allocate_space(egui::vec2(ui.available_width(), 0.0));
                                });
                            });
                            ui.vertical_centered(|ui| {
                                ui.add_space(40.0);
                                ui.label(egui::RichText::new(cx.language.t("connection_failed")).color(cx.theme.red).size(14.0).strong());
                                ui.add_space(6.0);
                                ui.label(egui::RichText::new(&err).color(cx.theme.fg_dim).size(12.0));
                                ui.add_space(12.0);
                                if ui.add(
                                    egui::Button::new(egui::RichText::new(cx.language.t("back")).color(cx.theme.accent).size(13.0))
                                        .frame(false)
                                ).clicked() {
                                    right_cancel_connect_request = true;
                                }
                            });
                        }
                        SftpConnectionState::Connected => {
                            egui::Frame {
                                fill: cx.theme.bg_secondary,
                                inner_margin: egui::Margin::symmetric(8.0, 6.0),
                                stroke: egui::Stroke::NONE,
                                ..Default::default()
                            }
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    if ui
                                        .add(egui::Button::new(
                                            egui::RichText::new("\u{2190}").color(cx.theme.accent).size(13.0),
                                        ).frame(false))
                                        .on_hover_text(cx.language.t("back"))
                                        .clicked()
                                    {
                                        remote_navigate_to = Some("..".to_string());
                                    }
                                    ui.add_space(4.0);
                                    ui.label(egui::RichText::new(format!("REMOTE  {}", browser.host_name)).color(cx.theme.fg_dim).size(13.0).strong());
                                    ui.add_space(4.0);
                                    let current_path_clone = browser.current_path.clone();
                                    render_breadcrumbs(ui, &current_path_clone, &mut remote_navigate_to, false, &cx.theme);
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui
                                            .add(egui::Button::new(
                                                egui::RichText::new("\u{2715}").color(cx.theme.red).size(13.0),
                                            ).frame(false))
                                            .on_hover_text(cx.language.t("disconnect"))
                                            .clicked()
                                        {
                                            right_disconnect_request = true;
                                        }
                                        ui.add_space(8.0);
                                        let toggle_text = if browser.show_hidden_files {
                                            cx.language.t("hide_hidden_files")
                                        } else {
                                            cx.language.t("show_hidden_files")
                                        };
                                        if ui
                                            .add(egui::Button::new(
                                                egui::RichText::new(if browser.show_hidden_files { "\u{1F440}" } else { "\u{1F441}" })
                                                    .color(if browser.show_hidden_files { cx.theme.accent } else { cx.theme.fg_dim })
                                                    .size(13.0)
                                            ).frame(false))
                                            .on_hover_text(toggle_text)
                                            .clicked()
                                        {
                                            right_toggle_hidden_files = true;
                                        }
                                        ui.add_space(8.0);
                                        let is_refreshing = sftp_remote_refresh_start
                                            .map_or(false, |t| t.elapsed() < std::time::Duration::from_millis(300));
                                        if is_refreshing {
                                            ui.spinner();
                                            ui.ctx().request_repaint();
                                        } else {
                                            if ui
                                                .add(egui::Button::new(
                                                    egui::RichText::new("\u{21BB}").color(cx.theme.accent).size(13.0),
                                                ).frame(false))
                                                .clicked()
                                            {
                                                remote_refresh_request = true;
                                            }
                                        }
                                    });
                                });
                            });

                            if remote_refresh_request {
                                let path = browser.current_path.clone();
                                browser.navigate(&path);
                            }

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
                                &cx.theme,
                                &mut remote_ctx_menu_req,
                                &mut remote_open_file_req,
                                &mut remote_delete_request,
                                !sftp_active_panel_is_local,
                                &cx.language,
                                "right_remote_scroll",
                                sftp_editor_dialog_is_some,
                                &mut remote_move_to_dir,
                            );
                        }
                        SftpConnectionState::Disconnected => {}
                    }
                }
            }
        }
    });

    // ── Handle right panel disconnect request ──
    if right_disconnect_request {
        window.sftp_browser = None;
    }
    // ── Handle right panel cancel connect request ──
    if right_cancel_connect_request {
        if let Some(ref b) = window.sftp_browser {
            b.cancel_connect();
        }
        window.sftp_browser = None;
    }
    if right_toggle_hidden_files {
        if let Some(ref mut b) = window.sftp_browser.as_mut() {
            b.toggle_hidden_files();
        }
    }
    if local_right_toggle_hidden {
        window.local_browser_right.toggle_hidden_files();
    }

    ui.allocate_rect(available, egui::Sense::hover());

    // ── Connect to host if selected (left panel) ──
    if let Some(idx) = left_connect_host {
        let host = &cx.hosts[idx];
        let username = TerminalSession::get_effective_username(&host.username);
        let auth = config::resolve_auth(host, &cx.credentials);
        window.sftp_browser_left = Some(crate::sftp::SftpBrowser::connect(
            &cx.runtime,
            host.host.clone(),
            host.port,
            username,
            auth,
            host.name.clone(),
        ));
    }

    // ── Connect to host if selected (right panel) ──
    if let Some(idx) = connect_host {
        let host = &cx.hosts[idx];
        let username = TerminalSession::get_effective_username(&host.username);
        let auth = config::resolve_auth(host, &cx.credentials);
        window.sftp_browser = Some(crate::sftp::SftpBrowser::connect(
            &cx.runtime,
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
            window.local_browser_left.navigate_up();
        } else if name.starts_with('/') {
            window.local_browser_left.navigate(&name);
        } else {
            let path = format!(
                "{}/{}",
                window.local_browser_left.current_path.trim_end_matches('/'),
                name
            );
            window.local_browser_left.navigate(&path);
        }
    }
    if let Some(action) = local_left_selection_action {
        let count = window.local_browser_left.filtered_entries().len();
        apply_selection_action(&mut window.local_browser_left.selection, action, count);
    }

    // ── Apply deferred right panel local actions ──
    if let Some(name) = local_right_navigate_to {
        if name == ".." {
            window.local_browser_right.navigate_up();
        } else if name.starts_with('/') {
            window.local_browser_right.navigate(&name);
        } else {
            let path = format!(
                "{}/{}",
                window.local_browser_right.current_path.trim_end_matches('/'),
                name
            );
            window.local_browser_right.navigate(&path);
        }
    }
    if let Some(action) = local_right_selection_action {
        let count = window.local_browser_right.filtered_entries().len();
        apply_selection_action(&mut window.local_browser_right.selection, action, count);
    }

    let left_is_connected = matches!(
        window.sftp_browser_left.as_ref().map(|b| &b.state),
        Some(SftpConnectionState::Connected)
    );
    if left_is_connected {
        if let Some(name) = left_remote_navigate_to {
            let browser = window.sftp_browser_left.as_ref().unwrap();
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
            let count = window.sftp_browser_left.as_ref().unwrap().entries.len();
            apply_selection_action(&mut window.sftp_browser_left.as_mut().unwrap().selection, action, count);
        }
    }

    // ── Apply deferred remote panel actions (only when connected) ──
    if is_connected {
        if let Some(name) = remote_navigate_to {
            let browser = window.sftp_browser.as_ref().unwrap();
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
            let count = window.sftp_browser.as_ref().unwrap().entries.len();
            apply_selection_action(&mut window.sftp_browser.as_mut().unwrap().selection, action, count);
        }
    }

    // ── Drag-and-drop (only when connected) ──
    if is_connected {
        let ctx = ui.ctx().clone();
        if let Some(payload) = egui::DragAndDrop::payload::<DragPayload>(&ctx) {
            if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                if left_panel_rect.contains(pos) && !payload.is_local {
                    ui.painter().rect_stroke(left_panel_rect, 2.0, egui::Stroke::new(2.0, cx.theme.accent));
                }
                if right_panel_rect.contains(pos) && payload.is_local {
                    ui.painter().rect_stroke(right_panel_rect, 2.0, egui::Stroke::new(2.0, cx.theme.accent));
                }
            }
            if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                let ghost_text = if payload.entries.len() == 1 {
                    let e = &payload.entries[0];
                    if e.is_dir {
                        format!("\u{1F4C1} {}", e.entry_name)
                    } else {
                        format!("\u{1F4C4} {}", e.entry_name)
                    }
                } else {
                    cx.language.tf("n_items", &payload.entries.len().to_string())
                };
                egui::Area::new(egui::Id::new("sftp_drag_ghost"))
                    .fixed_pos(pos + egui::vec2(14.0, -8.0))
                    .order(egui::Order::Tooltip)
                    .show(&ctx, |ui| {
                        egui::Frame {
                            fill: cx.theme.bg_elevated,
                            inner_margin: egui::Margin::symmetric(8.0, 4.0),
                            rounding: egui::Rounding::same(8.0),
                            stroke: egui::Stroke::new(1.0, cx.theme.accent),
                            ..Default::default()
                        }
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(ghost_text).color(cx.theme.fg_primary).size(12.0));
                        });
                    });
            }
        }

        // Handle drop
        if ctx.input(|i| i.pointer.any_released()) {
            if let Some(payload) = egui::DragAndDrop::take_payload::<DragPayload>(&ctx) {
                if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                    if let Some(browser) = window.sftp_browser.as_ref() {
                        for entry in &payload.entries {
                            if left_panel_rect.contains(pos) && !payload.is_local {
                                let local_dest = format!(
                                    "{}/{}",
                                    window.local_browser_left.current_path.trim_end_matches('/'),
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
        if let Some(ref browser) = window.sftp_browser.as_ref() {
            if let Some(ref progress) = browser.transfer {
                should_cancel = render_transfer_progress(
                    ui,
                    progress,
                    available,
                    &cx.theme,
                    &cx.language,
                );
            }
        }
        if should_cancel {
            if let Some(ref mut b) = window.sftp_browser.as_mut() {
                b.cancel_transfer();
            }
        }
    }

    // ── Handle delete key requests ──
    if local_left_delete_request {
        let filtered = window.local_browser_left.filtered_entries();
        let names: Vec<String> = window.local_browser_left.selection.selected.iter()
            .filter_map(|&i| filtered.get(i).map(|e| e.name.clone()))
            .collect();
        if !names.is_empty() {
            window.sftp_confirm_delete = Some(SftpConfirmDelete {
                panel: SftpPanel::LeftLocal,
                names,
            });
        }
    }
    if remote_delete_request {
        if let Some(ref browser) = window.sftp_browser {
            let filtered = browser.filtered_entries();
            let names: Vec<String> = browser.selection.selected.iter()
                .filter_map(|&i| filtered.get(i).map(|e| e.name.clone()))
                .collect();
            if !names.is_empty() {
                window.sftp_confirm_delete = Some(SftpConfirmDelete {
                    panel: SftpPanel::RightRemote,
                    names,
                });
            }
        }
    }
    if left_remote_delete_request {
        if let Some(ref browser) = window.sftp_browser_left {
            let filtered = browser.filtered_entries();
            let names: Vec<String> = browser.selection.selected.iter()
                .filter_map(|&i| filtered.get(i).map(|e| e.name.clone()))
                .collect();
            if !names.is_empty() {
                window.sftp_confirm_delete = Some(SftpConfirmDelete {
                    panel: SftpPanel::LeftRemote,
                    names,
                });
            }
        }
    }

    // ── Handle drag-to-folder move requests ──
    if let Some(req) = local_left_move_to_dir {
        let current_path = window.local_browser_left.current_path.clone();
        let target_path = format!("{}/{}", current_path.trim_end_matches('/'), req.target_dir);
        for entry in &req.source_entries {
            let src = &entry.full_path;
            let dst = format!("{}/{}", target_path.trim_end_matches('/'), entry.entry_name);
            if let Err(e) = std::fs::rename(src, &dst) {
                log::error!("Failed to move {} to {}: {}", src, dst, e);
            }
        }
        window.local_browser_left.refresh();
    }
    if let Some(req) = local_right_move_to_dir {
        let current_path = window.local_browser_right.current_path.clone();
        let target_path = format!("{}/{}", current_path.trim_end_matches('/'), req.target_dir);
        for entry in &req.source_entries {
            let src = &entry.full_path;
            let dst = format!("{}/{}", target_path.trim_end_matches('/'), entry.entry_name);
            if let Err(e) = std::fs::rename(src, &dst) {
                log::error!("Failed to move {} to {}: {}", src, dst, e);
            }
        }
        window.local_browser_right.refresh();
    }
    if let Some(req) = remote_move_to_dir {
        if let Some(ref browser) = window.sftp_browser {
            let current_path = browser.current_path.clone();
            let target_path = format!("{}/{}", current_path.trim_end_matches('/'), req.target_dir);
            for entry in &req.source_entries {
                let dst = format!("{}/{}", target_path.trim_end_matches('/'), entry.entry_name);
                browser.rename(&entry.full_path, &dst);
            }
        }
    }
    if let Some(req) = left_remote_move_to_dir {
        if let Some(ref browser) = window.sftp_browser_left {
            let current_path = browser.current_path.clone();
            let target_path = format!("{}/{}", current_path.trim_end_matches('/'), req.target_dir);
            for entry in &req.source_entries {
                let dst = format!("{}/{}", target_path.trim_end_matches('/'), entry.entry_name);
                browser.rename(&entry.full_path, &dst);
            }
        }
    }

    // ── Handle context menu requests ──
    let menu_just_opened = local_left_ctx_menu_req.is_some() || remote_ctx_menu_req.is_some() || left_remote_ctx_menu_req.is_some();
    if let Some((pos, entry_idx)) = local_left_ctx_menu_req {
        if let Some(idx) = entry_idx {
            if !window.local_browser_left.selection.is_selected(idx) {
                window.local_browser_left.selection.select_one(idx);
            }
        }
        let indices: Vec<usize> = if entry_idx.is_some() {
            window.local_browser_left.selection.selected.iter().copied().collect()
        } else {
            vec![]
        };
        let filtered = window.local_browser_left.filtered_entries();
        let names: Vec<String> = indices.iter()
            .filter_map(|&i| filtered.get(i).map(|e| e.name.clone()))
            .collect();
        let all_dirs = !indices.is_empty() && indices.iter().all(|&i| {
            filtered.get(i).map_or(false, |e| e.kind == SftpEntryKind::Directory)
        });
        let any_dirs = indices.iter().any(|&i| {
            filtered.get(i).map_or(false, |e| e.kind == SftpEntryKind::Directory)
        });
        window.sftp_context_menu = Some(SftpContextMenu {
            pos,
            panel: SftpPanel::LeftLocal,
            entry_indices: indices,
            entry_names: names,
            all_dirs,
            any_dirs,
        });
    }
    if let Some((pos, entry_idx)) = local_right_ctx_menu_req {
        if let Some(idx) = entry_idx {
            if !window.local_browser_right.selection.is_selected(idx) {
                window.local_browser_right.selection.select_one(idx);
            }
        }
        let indices: Vec<usize> = if entry_idx.is_some() {
            window.local_browser_right.selection.selected.iter().copied().collect()
        } else {
            vec![]
        };
        let filtered = window.local_browser_right.filtered_entries();
        let names: Vec<String> = indices.iter()
            .filter_map(|&i| filtered.get(i).map(|e| e.name.clone()))
            .collect();
        let all_dirs = !indices.is_empty() && indices.iter().all(|&i| {
            filtered.get(i).map_or(false, |e| e.kind == SftpEntryKind::Directory)
        });
        let any_dirs = indices.iter().any(|&i| {
            filtered.get(i).map_or(false, |e| e.kind == SftpEntryKind::Directory)
        });
        window.sftp_context_menu = Some(SftpContextMenu {
            pos,
            panel: SftpPanel::RightLocal,
            entry_indices: indices,
            entry_names: names,
            all_dirs,
            any_dirs,
        });
    }
    if let Some((pos, entry_idx)) = remote_ctx_menu_req {
        if let Some(ref mut browser) = window.sftp_browser {
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
            window.sftp_context_menu = Some(SftpContextMenu {
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
        if let Some(ref mut browser) = window.sftp_browser_left {
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
            window.sftp_context_menu = Some(SftpContextMenu {
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
    if let Some(ref menu) = window.sftp_context_menu.as_ref().map(|m| {
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
                    fill: cx.theme.bg_elevated,
                    inner_margin: egui::Margin::symmetric(6.0, 4.0),
                    rounding: egui::Rounding::same(6.0),
                    stroke: egui::Stroke::new(1.0, cx.theme.border),
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
                        if is_single {
                            if ui.add(
                                egui::Button::new(
                                    egui::RichText::new(cx.language.t("rename")).size(12.0).color(cx.theme.fg_primary)
                                ).frame(false)
                            ).clicked() {
                                window.sftp_rename_dialog = Some(SftpRenameDialog {
                                    panel,
                                    old_name: entry_names[0].clone(),
                                    new_name: entry_names[0].clone(),
                                    error: String::new(),
                                });
                                close_menu = true;
                            }
                        }

                        if is_single && !any_dirs {
                            if ui.add(
                                egui::Button::new(
                                    egui::RichText::new(cx.language.t("edit_file")).size(12.0).color(cx.theme.fg_primary)
                                ).frame(false)
                            ).clicked() {
                                open_file_for_editing_with_panel(window, panel, &entry_names[0]);
                                close_menu = true;
                            }
                        }

                        let delete_label = if entry_indices.len() > 1 {
                            format!("{} ({})", cx.language.t("delete_file"), entry_indices.len())
                        } else {
                            cx.language.t("delete_file").to_string()
                        };
                        if ui.add(
                            egui::Button::new(
                                egui::RichText::new(delete_label).size(12.0).color(cx.theme.red)
                            ).frame(false)
                        ).clicked() {
                            window.sftp_confirm_delete = Some(SftpConfirmDelete {
                                panel,
                                names: entry_names.clone(),
                            });
                            close_menu = true;
                        }

                        ui.separator();
                    }

                    if ui.add(
                        egui::Button::new(
                            egui::RichText::new(cx.language.t("new_folder")).size(12.0).color(cx.theme.fg_primary)
                        ).frame(false)
                    ).clicked() {
                        window.sftp_new_folder_dialog = Some(SftpNewFolderDialog {
                            panel,
                            name: String::new(),
                            error: String::new(),
                        });
                        close_menu = true;
                    }

                    if ui.add(
                        egui::Button::new(
                            egui::RichText::new(cx.language.t("new_file")).size(12.0).color(cx.theme.fg_primary)
                        ).frame(false)
                    ).clicked() {
                        window.sftp_new_file_dialog = Some(SftpNewFileDialog {
                            panel,
                            name: String::new(),
                            error: String::new(),
                        });
                        close_menu = true;
                    }
                });
            });

        if close_menu {
            window.sftp_context_menu = None;
        } else if !menu_just_opened && ui.ctx().input(|i| i.pointer.any_click()) {
            let menu_rect = area_resp.response.rect;
            if let Some(click_pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
                if !menu_rect.contains(click_pos) {
                    window.sftp_context_menu = None;
                }
            }
        }
    }

    // ── Render all dialogs (rename, new folder, new file, delete confirm, error) ──
    render_sftp_dialogs(window, ui, cx);

    // ── Handle double-click open file requests ──
    if let Some(idx) = local_left_open_file_req {
        let filtered = window.local_browser_left.filtered_entries();
        if let Some(entry) = filtered.get(idx) {
            if entry.kind != SftpEntryKind::Directory {
                open_file_for_editing(window, true, &entry.name.clone());
            }
        }
    }
    if let Some(idx) = local_right_open_file_req {
        let filtered = window.local_browser_right.filtered_entries();
        if let Some(entry) = filtered.get(idx) {
            if entry.kind != SftpEntryKind::Directory {
                open_file_for_editing(window, true, &entry.name.clone());
            }
        }
    }
    if let Some(idx) = remote_open_file_req {
        if let Some(ref browser) = window.sftp_browser {
            let filtered = browser.filtered_entries();
            if let Some(entry) = filtered.get(idx) {
                if entry.kind != SftpEntryKind::Directory {
                    open_file_for_editing(window, false, &entry.name.clone());
                }
            }
        }
    }
    if let Some(idx) = left_remote_open_file_req {
        if let Some(ref browser) = window.sftp_browser_left {
            let filtered = browser.filtered_entries();
            if let Some(entry) = filtered.get(idx) {
                if entry.kind != SftpEntryKind::Directory {
                    open_file_for_editing(window, false, &entry.name.clone());
                }
            }
        }
    }

    // ── Editor dialog ──
    render_editor_dialog(window, ui, cx);

    ViewActions::default()
}

/// Render SFTP-related dialogs
pub fn render_sftp_dialogs(window: &mut AppWindow, ui: &mut egui::Ui, cx: &mut WindowContext) {
    // Rename dialog
    let mut rename_action: Option<(SftpPanel, String, String)> = None;
    let mut close_rename = false;
    if let Some(ref mut dialog) = window.sftp_rename_dialog {
        let mut open = true;
        egui::Window::new(cx.language.t("rename"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .min_size(egui::vec2(280.0, 0.0))
            .default_size(egui::vec2(DIALOG_WIDTH_SM, 0.0))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .title_bar(false)
            .frame(widgets::dialog_frame(&cx.theme))
            .show(ui.ctx(), |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("\u{270F}").size(18.0).color(cx.theme.accent));
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(cx.language.t("rename")).size(15.0).color(cx.theme.fg_primary).strong());
                });
                ui.add_space(10.0);

                ui.label(egui::RichText::new(cx.language.t("new_name")).size(12.0).color(cx.theme.fg_dim));
                ui.add_space(4.0);
                let te = ui.add(
                    egui::TextEdit::singleline(&mut dialog.new_name)
                        .desired_width(260.0)
                        .hint_text(egui::RichText::new(cx.language.t("new_name")).color(cx.theme.hint_color()).italics())
                        .text_color(cx.theme.fg_primary)
                        .font(egui::FontId::proportional(13.0))
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
                    ui.label(egui::RichText::new(&dialog.error).size(11.0).color(cx.theme.red));
                }

                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(widgets::primary_button(cx.language.t("save"), &cx.theme)).clicked() {
                            if !dialog.new_name.is_empty() && dialog.new_name != dialog.old_name {
                                rename_action = Some((dialog.panel, dialog.old_name.clone(), dialog.new_name.clone()));
                                close_rename = true;
                            }
                        }
                        if ui.add(widgets::secondary_button(cx.language.t("cancel"), &cx.theme)).clicked() {
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
        window.sftp_rename_dialog = None;
    }
    if let Some((panel, old_name, new_name)) = rename_action {
        match panel {
            SftpPanel::LeftLocal => {
                if let Err(e) = window.local_browser_left.rename(&old_name, &new_name) {
                    log::error!("Local rename error: {}", e);
                }
            }
            SftpPanel::LeftRemote => {
                if let Some(ref browser) = window.sftp_browser_left {
                    let from = format!("{}/{}", browser.current_path.trim_end_matches('/'), old_name);
                    let to = format!("{}/{}", browser.current_path.trim_end_matches('/'), new_name);
                    browser.rename(&from, &to);
                }
            }
            SftpPanel::RightRemote => {
                if let Some(ref browser) = window.sftp_browser {
                    let from = format!("{}/{}", browser.current_path.trim_end_matches('/'), old_name);
                    let to = format!("{}/{}", browser.current_path.trim_end_matches('/'), new_name);
                    browser.rename(&from, &to);
                }
            }
            SftpPanel::RightLocal => {
                if let Err(e) = window.local_browser_right.rename(&old_name, &new_name) {
                    log::error!("Local rename error: {}", e);
                }
            }
        }
    }

    // New Folder dialog
    let mut create_dir_action: Option<(SftpPanel, String)> = None;
    let mut close_new_folder = false;
    if let Some(ref mut dialog) = window.sftp_new_folder_dialog {
        let mut open = true;
        egui::Window::new(cx.language.t("new_folder"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .min_size(egui::vec2(280.0, 0.0))
            .default_size(egui::vec2(DIALOG_WIDTH_SM, 0.0))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .title_bar(false)
            .frame(widgets::dialog_frame(&cx.theme))
            .show(ui.ctx(), |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("\u{1F4C1}").size(18.0).color(cx.theme.accent));
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(cx.language.t("new_folder")).size(15.0).color(cx.theme.fg_primary).strong());
                });
                ui.add_space(10.0);

                ui.label(egui::RichText::new(cx.language.t("folder_name")).size(12.0).color(cx.theme.fg_dim));
                ui.add_space(4.0);
                let te = ui.add(
                    egui::TextEdit::singleline(&mut dialog.name)
                        .desired_width(260.0)
                        .hint_text(egui::RichText::new(cx.language.t("folder_name")).color(cx.theme.hint_color()).italics())
                        .text_color(cx.theme.fg_primary)
                        .font(egui::FontId::proportional(13.0))
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
                    ui.label(egui::RichText::new(&dialog.error).size(11.0).color(cx.theme.red));
                }

                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(widgets::primary_button(cx.language.t("save"), &cx.theme)).clicked() {
                            if !dialog.name.is_empty() {
                                create_dir_action = Some((dialog.panel, dialog.name.clone()));
                                close_new_folder = true;
                            }
                        }
                        if ui.add(widgets::secondary_button(cx.language.t("cancel"), &cx.theme)).clicked() {
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
        window.sftp_new_folder_dialog = None;
    }
    if let Some((panel, name)) = create_dir_action {
        match panel {
            SftpPanel::LeftLocal => {
                if let Err(e) = window.local_browser_left.create_dir(&name) {
                    log::error!("Local create dir error: {}", e);
                }
            }
            SftpPanel::LeftRemote => {
                if let Some(ref browser) = window.sftp_browser_left {
                    let path = format!("{}/{}", browser.current_path.trim_end_matches('/'), name);
                    browser.create_dir(&path);
                }
            }
            SftpPanel::RightRemote => {
                if let Some(ref browser) = window.sftp_browser {
                    let path = format!("{}/{}", browser.current_path.trim_end_matches('/'), name);
                    browser.create_dir(&path);
                }
            }
            SftpPanel::RightLocal => {
                if let Err(e) = window.local_browser_right.create_dir(&name) {
                    log::error!("Local create dir error: {}", e);
                }
            }
        }
    }

    // New File dialog
    let mut create_file_action: Option<(SftpPanel, String)> = None;
    let mut close_new_file = false;
    if let Some(ref mut dialog) = window.sftp_new_file_dialog {
        let mut open = true;
        egui::Window::new(cx.language.t("new_file"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .min_size(egui::vec2(280.0, 0.0))
            .default_size(egui::vec2(DIALOG_WIDTH_SM, 0.0))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .title_bar(false)
            .frame(widgets::dialog_frame(&cx.theme))
            .show(ui.ctx(), |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("\u{1F4C4}").size(18.0).color(cx.theme.accent));
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(cx.language.t("new_file")).size(15.0).color(cx.theme.fg_primary).strong());
                });
                ui.add_space(10.0);

                ui.label(egui::RichText::new(cx.language.t("file_name")).size(12.0).color(cx.theme.fg_dim));
                ui.add_space(4.0);
                let te = ui.add(
                    egui::TextEdit::singleline(&mut dialog.name)
                        .desired_width(260.0)
                        .hint_text(egui::RichText::new(cx.language.t("file_name")).color(cx.theme.hint_color()).italics())
                        .text_color(cx.theme.fg_primary)
                        .font(egui::FontId::proportional(13.0))
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
                    ui.label(egui::RichText::new(&dialog.error).size(11.0).color(cx.theme.red));
                }

                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(widgets::primary_button(cx.language.t("save"), &cx.theme)).clicked() {
                            if !dialog.name.is_empty() {
                                create_file_action = Some((dialog.panel, dialog.name.clone()));
                                close_new_file = true;
                            }
                        }
                        if ui.add(widgets::secondary_button(cx.language.t("cancel"), &cx.theme)).clicked() {
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
        window.sftp_new_file_dialog = None;
    }
    if let Some((panel, name)) = create_file_action {
        match panel {
            SftpPanel::LeftLocal => {
                let path = format!("{}/{}", window.local_browser_left.current_path.trim_end_matches('/'), name);
                match std::fs::write(&path, b"") {
                    Ok(_) => window.local_browser_left.refresh(),
                    Err(e) => log::error!("Local create file error: {}", e),
                }
            }
            SftpPanel::LeftRemote => {
                if let Some(ref browser) = window.sftp_browser_left {
                    let path = format!("{}/{}", browser.current_path.trim_end_matches('/'), name);
                    browser.write_file(&path, Vec::new());
                }
            }
            SftpPanel::RightRemote => {
                if let Some(ref browser) = window.sftp_browser {
                    let path = format!("{}/{}", browser.current_path.trim_end_matches('/'), name);
                    browser.write_file(&path, Vec::new());
                }
            }
            SftpPanel::RightLocal => {
                let path = format!("{}/{}", window.local_browser_right.current_path.trim_end_matches('/'), name);
                match std::fs::write(&path, b"") {
                    Ok(_) => window.local_browser_right.refresh(),
                    Err(e) => log::error!("Local create file error: {}", e),
                }
            }
        }
    }

    // Delete confirmation dialog
    let mut delete_action: Option<(SftpPanel, Vec<String>)> = None;
    let mut close_delete = false;
    if let Some(ref dialog) = window.sftp_confirm_delete {
        let names = dialog.names.clone();
        let panel = dialog.panel;
        let mut open = true;
        egui::Window::new(cx.language.t("delete_file"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .min_size(egui::vec2(280.0, 0.0))
            .default_size(egui::vec2(DIALOG_WIDTH_SM, 0.0))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .title_bar(false)
            .frame(widgets::dialog_frame(&cx.theme))
            .show(ui.ctx(), |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("\u{26A0}").size(18.0).color(cx.theme.red));
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(cx.language.t("delete_file")).size(15.0).color(cx.theme.fg_primary).strong());
                });
                ui.add_space(10.0);

                let confirm_msg = if names.len() == 1 {
                    cx.language.tf("delete_file_confirm", &names[0])
                } else {
                    cx.language.tf("delete_items_confirm", &names.len().to_string())
                };
                ui.label(egui::RichText::new(confirm_msg).size(13.0).color(cx.theme.fg_primary));
                ui.add_space(4.0);
                ui.label(egui::RichText::new(cx.language.t("confirm_delete")).size(11.0).color(cx.theme.fg_dim));

                ui.add_space(16.0);

                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(widgets::danger_button(cx.language.t("delete_file"), &cx.theme)).clicked() {
                            delete_action = Some((panel, names.clone()));
                            close_delete = true;
                        }
                        if ui.add(widgets::secondary_button(cx.language.t("cancel"), &cx.theme)).clicked() {
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
        window.sftp_confirm_delete = None;
    }

    // Error dialog
    let mut close_error = false;
    if let Some(ref dialog) = window.sftp_error_dialog {
        let mut open = true;
        egui::Window::new(dialog.title.clone())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .min_size(egui::vec2(280.0, 0.0))
            .default_size(egui::vec2(400.0, 0.0))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .title_bar(false)
            .frame(widgets::dialog_frame(&cx.theme))
            .show(ui.ctx(), |ui| {
                ui.vertical_centered(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("❌").size(24.0));
                        ui.add_space(10.0);
                        ui.label(egui::RichText::new(&dialog.title).size(16.0).strong().color(cx.theme.fg_primary));
                    });
                    ui.add_space(16.0);
                    ui.label(egui::RichText::new(&dialog.message).size(13.0).color(cx.theme.fg_dim));
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        if ui.add(widgets::primary_button("确定", &cx.theme)).clicked() {
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
        window.sftp_error_dialog = None;
    }

    if let Some((panel, names)) = delete_action {
        for name in &names {
            match panel {
                SftpPanel::LeftLocal => {
                    if let Err(e) = window.local_browser_left.delete(name) {
                        let error_msg = format!("删除失败：{}\n\n文件：{}", e, name);
                        window.sftp_error_dialog = Some(SftpErrorDialog {
                            title: "删除文件失败".to_string(),
                            message: error_msg,
                        });
                    }
                }
                SftpPanel::LeftRemote => {
                    if let Some(ref browser) = window.sftp_browser_left {
                        let path = format!("{}/{}", browser.current_path.trim_end_matches('/'), name);
                        browser.delete(&path);
                    }
                }
                SftpPanel::RightRemote => {
                    if let Some(ref browser) = window.sftp_browser {
                        let path = format!("{}/{}", browser.current_path.trim_end_matches('/'), name);
                        browser.delete(&path);
                    }
                }
                SftpPanel::RightLocal => {
                    if let Err(e) = window.local_browser_right.delete(name) {
                        let error_msg = format!("删除失败：{}\n\n文件：{}", e, name);
                        window.sftp_error_dialog = Some(SftpErrorDialog {
                            title: "删除文件失败".to_string(),
                            message: error_msg,
                        });
                    }
                }
            }
        }
    }
}

/// Open a file for editing in the built-in editor
fn open_file_for_editing(window: &mut AppWindow, is_local: bool, file_name: &str) {
    if is_local {
        let dir = window.local_browser_left.current_path.clone();
        match window.local_browser_left.read_file(file_name) {
            Ok(content) => {
                window.sftp_editor_dialog = Some(SftpEditorDialog {
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
                window.sftp_editor_dialog = Some(SftpEditorDialog {
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
        if let Some(ref browser) = window.sftp_browser {
            let dir = browser.current_path.clone();
            let full_path = format!("{}/{}", dir.trim_end_matches('/'), file_name);
            browser.read_file(&full_path);
            window.sftp_editor_dialog = Some(SftpEditorDialog {
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

/// Open a file for editing with panel support
fn open_file_for_editing_with_panel(window: &mut AppWindow, panel: SftpPanel, file_name: &str) {
    match panel {
        SftpPanel::LeftLocal => {
            let dir = window.local_browser_left.current_path.clone();
            match window.local_browser_left.read_file(file_name) {
                Ok(content) => {
                    window.sftp_editor_dialog = Some(SftpEditorDialog {
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
                    window.sftp_editor_dialog = Some(SftpEditorDialog {
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
            if let Some(ref browser) = window.sftp_browser_left {
                let dir = browser.current_path.clone();
                let full_path = format!("{}/{}", dir.trim_end_matches('/'), file_name);
                browser.read_file(&full_path);
                window.sftp_editor_dialog = Some(SftpEditorDialog {
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
            if let Some(ref browser) = window.sftp_browser {
                let dir = browser.current_path.clone();
                let full_path = format!("{}/{}", dir.trim_end_matches('/'), file_name);
                browser.read_file(&full_path);
                window.sftp_editor_dialog = Some(SftpEditorDialog {
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
            let dir = window.local_browser_right.current_path.clone();
            match window.local_browser_right.read_file(file_name) {
                Ok(content) => {
                    window.sftp_editor_dialog = Some(SftpEditorDialog {
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
                    window.sftp_editor_dialog = Some(SftpEditorDialog {
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

/// Render the editor dialog
pub fn render_editor_dialog(window: &mut AppWindow, ui: &mut egui::Ui, cx: &mut WindowContext) {
    let mut close_editor = false;
    let mut save_action: Option<(SftpPanel, String, String)> = None;

    if let Some(ref mut dialog) = window.sftp_editor_dialog {
        let has_unsaved = dialog.content != dialog.original_content;

        let mut open = true;
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
                fill: cx.theme.bg_primary,
                rounding: egui::Rounding::same(10.0),
                inner_margin: egui::Margin::ZERO,
                stroke: egui::Stroke::new(1.0, cx.theme.border),
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
                        fill: cx.theme.bg_secondary,
                        inner_margin: egui::Margin { left: 16.0, right: 16.0, top: 12.0, bottom: 12.0 },
                        rounding: egui::Rounding { nw: 10.0, ne: 10.0, sw: 0.0, se: 0.0 },
                        ..Default::default()
                    };
                    title_frame.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("\u{1F4C4}").size(14.0).color(cx.theme.accent));
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new(&dialog.file_name).size(13.0).color(cx.theme.fg_primary).strong());
                            if has_unsaved {
                                ui.add_space(4.0);
                                ui.label(egui::RichText::new("\u{25CF}").size(10.0).color(cx.theme.accent));
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add(
                                    egui::Button::new(
                                        egui::RichText::new("\u{2715}").size(12.0).color(cx.theme.fg_dim)
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
                                    ui.label(egui::RichText::new(dir_display).size(11.0).color(cx.theme.fg_dim).family(egui::FontFamily::Monospace));
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
                    egui::Stroke::new(1.0, cx.theme.border),
                );

                if dialog.loading {
                    ui.vertical_centered(|ui| {
                        ui.add_space(60.0);
                        ui.spinner();
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(cx.language.t("loading")).color(cx.theme.fg_dim));
                    });
                    return;
                }

                if !dialog.error.is_empty() {
                    let err_frame = egui::Frame {
                        fill: cx.theme.bg_primary,
                        inner_margin: egui::Margin::same(20.0),
                        ..Default::default()
                    };
                    err_frame.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("\u{26A0}").size(16.0).color(cx.theme.red));
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new(&dialog.error).color(cx.theme.red).size(12.0));
                        });
                        ui.add_space(16.0);
                        ui.horizontal(|ui| {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add(
                                    egui::Button::new(
                                        egui::RichText::new(cx.language.t("cancel")).color(cx.theme.fg_dim).size(13.0)
                                    )
                                    .fill(cx.theme.bg_elevated)
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
                        fill: cx.theme.bg_secondary,
                        inner_margin: egui::Margin { left: 16.0, right: 16.0, top: 8.0, bottom: 8.0 },
                        ..Default::default()
                    };
                    name_frame.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(cx.language.t("file_name")).size(12.0).color(cx.theme.fg_dim));
                            ui.add_space(6.0);
                            ui.add(
                                egui::TextEdit::singleline(&mut dialog.save_as_name)
                                    .desired_width(ui.available_width())
                                    .font(egui::FontId::monospace(12.0))
                                    .hint_text(egui::RichText::new(cx.language.t("file_name")).color(cx.theme.hint_color()).italics())
                                    .text_color(cx.theme.fg_primary)
                            );
                        });
                    });
                    let sep_rect2 = ui.available_rect_before_wrap();
                    ui.painter().hline(
                        sep_rect2.min.x..=sep_rect2.max.x,
                        sep_rect2.min.y,
                        egui::Stroke::new(1.0, cx.theme.border),
                    );
                }

                // ── Code editor area ──
                {
                    let editor_frame = egui::Frame {
                        fill: cx.theme.bg_primary,
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
                                        .font(egui::FontId::monospace(cx.font_size))
                                        .code_editor()
                                        .lock_focus(true)
                                        .hint_text(egui::RichText::new(cx.language.t("file_content")).color(cx.theme.hint_color()).italics())
                                        .text_color(cx.theme.fg_primary)
                                );
                                let _ = response;
                            });
                    });
                }

                // ── Bottom bar ──
                {
                    let sep_rect3 = ui.available_rect_before_wrap();
                    ui.painter().hline(
                        sep_rect3.min.x..=sep_rect3.max.x,
                        sep_rect3.min.y,
                        egui::Stroke::new(1.0, cx.theme.border),
                    );
                    let bar_frame = egui::Frame {
                        fill: cx.theme.bg_secondary,
                        inner_margin: egui::Margin { left: 16.0, right: 16.0, top: 8.0, bottom: 8.0 },
                        rounding: egui::Rounding { nw: 0.0, ne: 0.0, sw: 10.0, se: 10.0 },
                        ..Default::default()
                    };
                    bar_frame.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let line_count = dialog.content.lines().count().max(1);
                            let char_count = dialog.content.len();
                            ui.label(
                                egui::RichText::new(cx.language.tf2("lines_and_chars", &line_count.to_string(), &char_count.to_string()))
                                    .size(11.0).color(cx.theme.fg_dim).family(egui::FontFamily::Monospace)
                            );

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add(
                                    egui::Button::new(
                                        egui::RichText::new(cx.language.t("save")).color(egui::Color32::WHITE).size(12.0)
                                    )
                                    .fill(cx.theme.accent)
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
                                if has_unsaved {
                                    ui.add_space(4.0);
                                    ui.label(egui::RichText::new(cx.language.t("unsaved_changes")).color(cx.theme.fg_dim).size(11.0));
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
                match window.local_browser_left.write_file(&file_name, &content) {
                    Ok(_) => {
                        close_editor = true;
                    }
                    Err(e) => {
                        if let Some(ref mut dialog) = window.sftp_editor_dialog {
                            dialog.error = e;
                        }
                    }
                }
            }
            SftpPanel::LeftRemote => {
                if let Some(ref browser) = window.sftp_browser_left {
                    browser.write_file(&path, content.as_bytes().to_vec());
                    let current = browser.current_path.clone();
                    browser.navigate(&current);
                    close_editor = true;
                }
            }
            SftpPanel::RightRemote => {
                if let Some(ref browser) = window.sftp_browser {
                    browser.write_file(&path, content.as_bytes().to_vec());
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
                match window.local_browser_right.write_file(&file_name, &content) {
                    Ok(_) => {
                        close_editor = true;
                    }
                    Err(e) => {
                        if let Some(ref mut dialog) = window.sftp_editor_dialog {
                            dialog.error = e;
                        }
                    }
                }
            }
        }
    }

    if close_editor {
        window.sftp_editor_dialog = None;
    }
}
