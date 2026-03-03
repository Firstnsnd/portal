use eframe::egui;
use std::sync::{Arc, Mutex};

use crate::PortalApp;
use crate::config::HostEntry;
use crate::ssh::test_connection;
use crate::ui::types::{AuthMethodChoice, TestConnState, AppView};

impl PortalApp {
    /// Navigation strip on the left (always visible)
    pub fn show_nav_panel(&mut self, ctx: &egui::Context) {
        let theme = self.theme.clone();
        let language = self.language;
        let nav_width = (ctx.screen_rect().width() * 0.14).min(200.0).max(150.0);
        egui::SidePanel::left("nav")
            .exact_width(nav_width)
            .resizable(false)
            .frame(egui::Frame {
                fill: theme.bg_secondary,
                inner_margin: egui::Margin::same(0.0),
                stroke: egui::Stroke::NONE,
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.add_space(32.0);

                let nav_btn = |ui: &mut egui::Ui, icon: &str, label: &str, active: bool| -> bool {
                    let width = ui.available_width();
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(width, 36.0),
                        egui::Sense::click(),
                    );
                    let bg = if active {
                        theme.accent_alpha(45)
                    } else if resp.hovered() {
                        theme.hover_bg
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    let shadow_color = if active {
                        theme.accent_alpha(80)
                    } else if resp.hovered() {
                        theme.hover_shadow
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    ui.painter().rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(rect.min.x, rect.max.y - 1.0),
                            rect.max,
                        ),
                        0.0,
                        shadow_color,
                    );
                    ui.painter().rect_filled(rect, 0.0, bg);
                    if active {
                        ui.painter().rect_filled(
                            egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + 3.0, rect.max.y)),
                            egui::Rounding { nw: 0.0, ne: 2.0, sw: 0.0, se: 2.0 },
                            theme.accent,
                        );
                    }
                    let color = if active { theme.fg_primary } else if resp.hovered() { theme.fg_primary } else { theme.fg_dim };
                    ui.painter().text(
                        egui::pos2(rect.min.x + 16.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        format!("{}  {}", icon, label),
                        egui::FontId::proportional(13.0),
                        color,
                    );
                    resp.clicked()
                };

                if nav_btn(ui, "☰", language.t("hosts"), self.current_view == AppView::Hosts) {
                    self.current_view = AppView::Hosts;
                }
                if nav_btn(ui, ">_", language.t("terminal"), self.current_view == AppView::Terminal) {
                    self.current_view = AppView::Terminal;
                }
                if nav_btn(ui, "\u{2195}", language.t("sftp"), self.current_view == AppView::Sftp) {
                    self.current_view = AppView::Sftp;
                }
                if nav_btn(ui, "\u{1f511}", language.t("keychain"), self.current_view == AppView::Keychain) {
                    self.current_view = AppView::Keychain;
                }

                // Settings button at bottom
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.add_space(8.0);
                    if nav_btn(ui, "⚙", language.t("settings"), self.current_view == AppView::Settings) {
                        self.current_view = AppView::Settings;
                    }
                });
            });
    }

    /// Full hosts page content (used by both main and detached windows)
    pub fn show_hosts_page(&mut self, _ctx: &egui::Context, ui: &mut egui::Ui) {
        ui.add_space(0.0);
        let mut new_local_session = false;
        let mut connect_ssh_host: Option<usize> = None;
        let mut edit_host_index: Option<usize> = None;

        egui::ScrollArea::vertical()
            .id_salt("hosts_page_scroll")
            .show(ui, |ui| {
            ui.add_space(20.0);

            // Page header
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                ui.label(egui::RichText::new(self.language.t("hosts")).color(self.theme.fg_dim).size(12.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(24.0);
                    if ui.add(
                        egui::Button::new(egui::RichText::new(self.language.t("new_host")).color(self.theme.accent).size(12.0))
                            .frame(false)
                    ).clicked() {
                        self.add_host_dialog.open_new();
                    }
                });
            });
            ui.add_space(16.0);

            // LOCAL section
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                ui.label(egui::RichText::new(self.language.t("local")).color(self.theme.fg_dim).size(10.0).strong());
            });
            ui.add_space(4.0);

            for (_i, host) in self.hosts.iter().enumerate() {
                if !host.is_local { continue; }
                let width = ui.available_width();
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(width, 36.0),
                    egui::Sense::click(),
                );
                if resp.hovered() {
                    ui.painter().rect_filled(
                        egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.max.y - 1.0), rect.max),
                        0.0, self.theme.hover_shadow,
                    );
                    ui.painter().rect_filled(rect, 0.0, self.theme.hover_bg);
                }
                ui.painter().text(
                    egui::pos2(rect.min.x + 24.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    ">_",
                    egui::FontId::proportional(12.0),
                    self.theme.green,
                );
                ui.painter().text(
                    egui::pos2(rect.min.x + 48.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    &host.name,
                    egui::FontId::proportional(13.0),
                    self.theme.fg_primary,
                );
                if resp.clicked() {
                    new_local_session = true;
                }
            }

            ui.add_space(20.0);

            // SSH HOSTS section
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                ui.label(egui::RichText::new(self.language.t("ssh_hosts")).color(self.theme.fg_dim).size(10.0).strong());
            });
            ui.add_space(4.0);

            // Collect groups
            let mut groups: Vec<String> = Vec::new();
            for host in &self.hosts {
                if !host.is_local && !host.group.is_empty() && !groups.contains(&host.group) {
                    groups.push(host.group.clone());
                }
            }

            // Ungrouped SSH hosts
            for (i, host) in self.hosts.iter().enumerate() {
                if host.is_local || !host.group.is_empty() { continue; }
                let row_h = 44.0;
                let width = ui.available_width();
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(width, row_h),
                    egui::Sense::click(),
                );
                let hovered = resp.hovered();
                if hovered {
                    ui.painter().rect_filled(
                        egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.max.y - 1.0), rect.max),
                        0.0, self.theme.hover_shadow,
                    );
                    ui.painter().rect_filled(rect, 0.0, self.theme.hover_bg);
                }
                // Icon
                ui.painter().text(
                    egui::pos2(rect.min.x + 24.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    "@",
                    egui::FontId::proportional(12.0),
                    self.theme.accent,
                );
                // Host name
                ui.painter().text(
                    egui::pos2(rect.min.x + 46.0, rect.center().y - 7.0),
                    egui::Align2::LEFT_CENTER,
                    &host.name,
                    egui::FontId::proportional(13.0),
                    self.theme.fg_primary,
                );
                // Detail
                let detail = if host.username.is_empty() {
                    format!("{}:{}", host.host, host.port)
                } else {
                    format!("{}@{}:{}", host.username, host.host, host.port)
                };
                ui.painter().text(
                    egui::pos2(rect.min.x + 46.0, rect.center().y + 8.0),
                    egui::Align2::LEFT_CENTER,
                    detail,
                    egui::FontId::proportional(10.0),
                    self.theme.fg_dim,
                );
                // Edit button (only visible on hover)
                let btn_rect = egui::Rect::from_center_size(
                    egui::pos2(rect.max.x - 50.0, rect.center().y),
                    egui::vec2(56.0, 22.0),
                );
                if hovered {
                    let pointer_pos = ui.ctx().input(|i| i.pointer.hover_pos());
                    let over_btn = pointer_pos.map_or(false, |p| btn_rect.contains(p));
                    let btn_bg = if over_btn { self.theme.accent } else { self.theme.bg_elevated };
                    let btn_text_color = if over_btn { self.theme.bg_primary } else { self.theme.accent };
                    ui.painter().rect(btn_rect, 4.0, btn_bg, egui::Stroke::new(1.0, self.theme.accent));
                    ui.painter().text(
                        btn_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        self.language.t("edit_file"),
                        egui::FontId::proportional(11.0),
                        btn_text_color,
                    );
                }
                if resp.double_clicked() {
                    connect_ssh_host = Some(i);
                } else if resp.clicked() {
                    let click_pos = ui.ctx().input(|i| i.pointer.interact_pos());
                    if click_pos.map_or(false, |p| btn_rect.contains(p)) {
                        edit_host_index = Some(i);
                    }
                }
            }

            // Grouped SSH hosts
            for group in &groups {
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.label(egui::RichText::new(group).color(self.theme.fg_dim).size(10.0));
                });
                ui.add_space(2.0);
                for (i, host) in self.hosts.iter().enumerate() {
                    if host.is_local || host.group != *group { continue; }
                    let row_h = 44.0;
                    let width = ui.available_width();
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(width, row_h),
                        egui::Sense::click(),
                    );
                    let hovered = resp.hovered();
                    if hovered {
                        ui.painter().rect_filled(
                            egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.max.y - 1.0), rect.max),
                            0.0, self.theme.hover_shadow,
                        );
                        ui.painter().rect_filled(rect, 0.0, self.theme.hover_bg);
                    }
                    ui.painter().text(
                        egui::pos2(rect.min.x + 24.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        "@",
                        egui::FontId::proportional(12.0),
                        self.theme.accent,
                    );
                    ui.painter().text(
                        egui::pos2(rect.min.x + 46.0, rect.center().y - 7.0),
                        egui::Align2::LEFT_CENTER,
                        &host.name,
                        egui::FontId::proportional(13.0),
                        self.theme.fg_primary,
                    );
                    let detail = if host.username.is_empty() {
                        format!("{}:{}", host.host, host.port)
                    } else {
                        format!("{}@{}:{}", host.username, host.host, host.port)
                    };
                    ui.painter().text(
                        egui::pos2(rect.min.x + 46.0, rect.center().y + 8.0),
                        egui::Align2::LEFT_CENTER,
                        detail,
                        egui::FontId::proportional(10.0),
                        self.theme.fg_dim,
                    );
                    let btn_rect = egui::Rect::from_center_size(
                        egui::pos2(rect.max.x - 50.0, rect.center().y),
                        egui::vec2(56.0, 22.0),
                    );
                    if hovered {
                        let pointer_pos = ui.ctx().input(|i| i.pointer.hover_pos());
                        let over_btn = pointer_pos.map_or(false, |p| btn_rect.contains(p));
                        let btn_bg = if over_btn { self.theme.accent } else { self.theme.bg_elevated };
                        let btn_text_color = if over_btn { self.theme.bg_primary } else { self.theme.accent };
                        ui.painter().rect(btn_rect, 4.0, btn_bg, egui::Stroke::new(1.0, self.theme.accent));
                        ui.painter().text(
                            btn_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            self.language.t("edit_file"),
                            egui::FontId::proportional(11.0),
                            btn_text_color,
                        );
                    }
                    if resp.double_clicked() {
                        connect_ssh_host = Some(i);
                    } else if resp.clicked() {
                        let click_pos = ui.ctx().input(|i| i.pointer.interact_pos());
                        if click_pos.map_or(false, |p| btn_rect.contains(p)) {
                            edit_host_index = Some(i);
                        }
                    }
                }
            }

            ui.add_space(20.0);
        }); // end ScrollArea

        // Deferred actions (after borrows of self.hosts are done)
        if new_local_session {
            self.add_tab_local();
        }
        if let Some(idx) = connect_ssh_host {
            let host = self.hosts[idx].clone();
            self.add_tab_ssh(&host);
        }
        if let Some(idx) = edit_host_index {
            let host = self.hosts[idx].clone();
            self.add_host_dialog.open_edit(idx, &host);
        }
    }

    /// Right-side drawer for adding / editing a host (shown in Hosts view)
    pub fn show_add_host_drawer(&mut self, ctx: &egui::Context) {
        if !self.add_host_dialog.open {
            return;
        }

        // Poll the async test result
        let polled_result: Option<Result<String, String>> = self
            .add_host_dialog
            .test_conn_result
            .as_ref()
            .and_then(|arc| arc.lock().ok()?.take());
        if let Some(result) = polled_result {
            self.add_host_dialog.test_conn_state = match result {
                Ok(msg) => TestConnState::Success(msg),
                Err(msg) => TestConnState::Failed(msg),
            };
            self.add_host_dialog.test_conn_result = None;
        }

        let theme = self.theme.clone();
        let lang = self.language;
        let drawer_title = if self.add_host_dialog.edit_index.is_some() {
            lang.t("edit_host")
        } else {
            lang.t("new_host_title")
        };

        let mut save_clicked = false;
        let mut test_clicked = false;

        egui::SidePanel::right("add_host_drawer")
            .exact_width(340.0)
            .resizable(false)
            .frame(egui::Frame {
                fill: theme.bg_secondary,
                inner_margin: egui::Margin::same(0.0),
                stroke: egui::Stroke::NONE,
                ..Default::default()
            })
            .show(ctx, |ui| {
                egui::Frame {
                    fill: theme.bg_elevated,
                    inner_margin: egui::Margin::symmetric(20.0, 12.0),
                    ..Default::default()
                }
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(drawer_title).color(theme.fg_primary).size(16.0).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(
                                egui::Button::new(egui::RichText::new("✕").color(theme.fg_dim).size(16.0))
                                    .frame(false)
                            ).clicked() {
                                self.add_host_dialog.reset();
                            }
                            if self.add_host_dialog.edit_index.is_some() {
                                if ui.add(
                                    egui::Button::new(egui::RichText::new("\u{1F5D1}").size(14.0))
                                        .frame(false)
                                ).on_hover_text(lang.t("delete"))
                                .clicked() {
                                    self.confirm_delete_host = self.add_host_dialog.edit_index;
                                    self.add_host_dialog.reset();
                                }
                            }
                        });
                    });
                });

                ui.separator();

                egui::ScrollArea::vertical()
                    .id_salt("drawer_scroll")
                    .show(ui, |ui| {
                    ui.add_space(4.0);
                    egui::Frame {
                        inner_margin: egui::Margin::symmetric(20.0, 12.0),
                        ..Default::default()
                    }
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 8.0;

                        ui.label(egui::RichText::new(lang.t("label")).color(theme.fg_dim).size(12.0));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.add_host_dialog.name)
                                .hint_text(egui::RichText::new("My Server").color(theme.hint_color()).italics())
                                .desired_width(f32::INFINITY)
                                .text_color(theme.fg_primary)
                        );

                        ui.add_space(4.0);

                        ui.label(egui::RichText::new(lang.t("host_ip")).color(theme.fg_dim).size(12.0));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.add_host_dialog.host)
                                .hint_text(egui::RichText::new("192.168.1.1").color(theme.hint_color()).italics())
                                .desired_width(f32::INFINITY)
                                .text_color(theme.fg_primary)
                        );

                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(lang.t("port")).color(theme.fg_dim).size(12.0));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.add_host_dialog.port)
                                        .desired_width(70.0)
                                        .text_color(theme.fg_primary)
                                );
                            });
                            ui.add_space(8.0);
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(lang.t("group")).color(theme.fg_dim).size(12.0));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.add_host_dialog.group)
                                        .hint_text(egui::RichText::new("Production").color(theme.hint_color()).italics())
                                        .desired_width(f32::INFINITY)
                                        .text_color(theme.fg_primary)
                                );
                            });
                        });

                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(8.0);

                        ui.label(egui::RichText::new(lang.t("authentication")).color(theme.fg_dim).size(12.0));
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let pw_color = if self.add_host_dialog.auth_method == AuthMethodChoice::Password { theme.accent } else { theme.fg_primary };
                            ui.selectable_value(
                                &mut self.add_host_dialog.auth_method,
                                AuthMethodChoice::Password,
                                egui::RichText::new(lang.t("password")).color(pw_color).size(12.0),
                            );
                            let key_color = if self.add_host_dialog.auth_method == AuthMethodChoice::Key { theme.accent } else { theme.fg_primary };
                            ui.selectable_value(
                                &mut self.add_host_dialog.auth_method,
                                AuthMethodChoice::Key,
                                egui::RichText::new(lang.t("ssh_key")).color(key_color).size(12.0),
                            );
                        });

                        ui.add_space(4.0);

                        match self.add_host_dialog.auth_method {
                            AuthMethodChoice::Password => {
                                ui.label(egui::RichText::new(lang.t("username")).color(theme.fg_dim).size(12.0));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.add_host_dialog.username)
                                        .hint_text(egui::RichText::new("root").color(theme.hint_color()).italics())
                                        .desired_width(f32::INFINITY)
                                        .text_color(theme.fg_primary)
                                );
                                ui.add_space(4.0);
                                ui.label(egui::RichText::new(lang.t("password")).color(theme.fg_dim).size(12.0));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.add_host_dialog.password)
                                        .password(true)
                                        .hint_text(egui::RichText::new("Enter password").color(theme.hint_color()).italics())
                                        .desired_width(f32::INFINITY)
                                        .text_color(theme.fg_primary)
                                );
                            }
                            AuthMethodChoice::Key => {
                                ui.label(egui::RichText::new(lang.t("key_path")).color(theme.fg_dim).size(12.0));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.add_host_dialog.key_path)
                                        .hint_text(egui::RichText::new("~/.ssh/id_rsa").color(theme.hint_color()).italics())
                                        .desired_width(f32::INFINITY)
                                        .text_color(theme.fg_primary)
                                );
                                if self.add_host_dialog.key_in_keychain {
                                    ui.label(egui::RichText::new(lang.t("key_stored_in_keychain")).color(theme.green).size(11.0));
                                }
                                ui.add_space(4.0);
                                ui.label(egui::RichText::new(lang.t("key_passphrase")).color(theme.fg_dim).size(12.0));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.add_host_dialog.key_passphrase)
                                        .password(true)
                                        .hint_text(egui::RichText::new("Leave empty if none").color(theme.hint_color()).italics())
                                        .desired_width(f32::INFINITY)
                                        .text_color(theme.fg_primary)
                                );
                            }
                        }

                        ui.add_space(8.0);

                        if !self.add_host_dialog.error.is_empty() {
                            ui.label(egui::RichText::new(&self.add_host_dialog.error).color(theme.red).size(12.0));
                        }

                        match &self.add_host_dialog.test_conn_state {
                            TestConnState::Idle => {}
                            TestConnState::Testing => {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label(egui::RichText::new(lang.t("testing")).color(theme.fg_dim).size(12.0));
                                });
                            }
                            TestConnState::Success(msg) => {
                                ui.label(egui::RichText::new(format!("✓ {}", msg)).color(theme.green).size(12.0));
                            }
                            TestConnState::Failed(msg) => {
                                ui.label(egui::RichText::new(format!("✗ {}", msg)).color(theme.red).size(12.0));
                            }
                        }

                        ui.add_space(16.0);
                        ui.separator();
                        ui.add_space(12.0);

                        ui.horizontal(|ui| {
                            if ui.add(
                                egui::Button::new(egui::RichText::new(lang.t("save")).color(theme.bg_primary).size(13.0))
                                    .fill(theme.accent)
                                    .rounding(6.0)
                                    .min_size(egui::vec2(80.0, 36.0))
                            ).clicked() {
                                save_clicked = true;
                            }

                            ui.add_space(8.0);

                            let is_testing = matches!(self.add_host_dialog.test_conn_state, TestConnState::Testing);
                            if ui.add_enabled(
                                !is_testing,
                                egui::Button::new(egui::RichText::new(lang.t("test")).color(theme.fg_primary).size(13.0))
                                    .fill(theme.bg_elevated)
                                    .rounding(6.0)
                                    .min_size(egui::vec2(80.0, 36.0))
                            ).clicked() {
                                test_clicked = true;
                            }
                        });
                    });
                });
            });

        if test_clicked {
            let host = self.add_host_dialog.host.trim().to_owned();
            let port: u16 = self.add_host_dialog.port.trim().parse().unwrap_or(22);
            let username = self.add_host_dialog.username.trim().to_owned();

            if host.is_empty() {
                self.add_host_dialog.error = "Host is required for testing.".to_owned();
            } else {
                let auth = match self.add_host_dialog.auth_method {
                    AuthMethodChoice::Password => {
                        let pw = self.add_host_dialog.password.clone();
                        if pw.is_empty() {
                            crate::config::AuthMethod::None
                        } else {
                            crate::config::AuthMethod::Password { password: pw }
                        }
                    }
                    AuthMethodChoice::Key => {
                        let path = self.add_host_dialog.key_path.trim().to_owned();
                        crate::config::AuthMethod::Key {
                            key_path: path,
                            passphrase: self.add_host_dialog.key_passphrase.clone(),
                            key_in_keychain: self.add_host_dialog.key_in_keychain,
                        }
                    }
                };

                let result_arc: Arc<Mutex<Option<Result<String, String>>>> =
                    Arc::new(Mutex::new(None));
                self.add_host_dialog.test_conn_result = Some(Arc::clone(&result_arc));
                self.add_host_dialog.test_conn_state = TestConnState::Testing;
                self.add_host_dialog.error.clear();

                let name = self.add_host_dialog.name.trim().to_owned();
                self.runtime.spawn(async move {
                    let result = test_connection(host, port, username, auth, name).await;
                    if let Ok(mut guard) = result_arc.lock() {
                        *guard = Some(result);
                    }
                });
            }
        }

        if save_clicked {
            // Validate
            let name = self.add_host_dialog.name.trim().to_owned();
            let host = self.add_host_dialog.host.trim().to_owned();
            let port: u16 = self.add_host_dialog.port.trim().parse().unwrap_or(22);

            if name.is_empty() || host.is_empty() {
                self.add_host_dialog.error = "Label and Host are required.".to_owned();
                return;
            }

            let auth = match self.add_host_dialog.auth_method {
                AuthMethodChoice::Password => {
                    let pw = self.add_host_dialog.password.clone();
                    if pw.is_empty() {
                        crate::config::AuthMethod::None
                    } else {
                        crate::config::AuthMethod::Password { password: pw }
                    }
                }
                AuthMethodChoice::Key => {
                    let path = self.add_host_dialog.key_path.trim().to_owned();
                    if path.is_empty() {
                        self.add_host_dialog.error = "Key path is required.".to_owned();
                        return;
                    }
                    crate::config::AuthMethod::Key {
                        key_path: path,
                        passphrase: self.add_host_dialog.key_passphrase.clone(),
                        key_in_keychain: self.add_host_dialog.key_in_keychain,
                    }
                }
            };

            let entry = HostEntry::new_ssh(
                name,
                host,
                port,
                self.add_host_dialog.username.trim().to_owned(),
                self.add_host_dialog.group.trim().to_owned(),
                auth,
            );

            if let Some(idx) = self.add_host_dialog.edit_index {
                if idx < self.hosts.len() {
                    self.hosts[idx] = entry;
                }
            } else {
                self.hosts.push(entry);
            }

            self.save_hosts();
            self.add_host_dialog.reset();
        }
    }
}
