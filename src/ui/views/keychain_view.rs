use eframe::egui;

use crate::app::PortalApp;
use crate::config::{self, Credential, CredentialType};
use crate::ui::types::dialogs::{KeychainDeleteRequest, CredentialTypeChoice, KeySourceChoice};
use crate::ui::tokens::*;
use crate::ui::widgets;

impl PortalApp {
    /// Full keychain page content (used by both main and detached windows)
    pub fn show_keychain_view(&mut self, ctx: &egui::Context, _ui: &mut egui::Ui) {
        // Collect all unique groups and tags
        let mut all_groups: Vec<String> = Vec::new();
        for host in &self.hosts {
            if !host.group.is_empty() && !all_groups.contains(&host.group) {
                all_groups.push(host.group.clone());
            }
        }
        all_groups.sort();

        // Top navigation bar (matching terminal tab bar style)
        egui::TopBottomPanel::top("keychain_nav_bar")
            .frame(egui::Frame {
                fill: self.theme.bg_secondary,
                inner_margin: egui::Margin::symmetric(8.0, 8.0),
                stroke: egui::Stroke::NONE,
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Left side: Keychain title
                    ui.label(egui::RichText::new(self.language.t("keychain"))
                        .color(self.theme.fg_dim)
                        .size(FONT_BASE)
                        .strong());

                    // Right side: New Credential button
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(
                            widgets::text_button(self.language.t("new_credential"), self.theme.accent)
                        ).clicked() {
                            self.credential_dialog.open_new();
                        }
                    });
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame {
                fill: self.theme.bg_primary,
                ..Default::default()
            })
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("keychain_page_scroll")
                    .show(ui, |ui| {
                        ui.add_space(SPACE_MD);

                        // ── Empty state ──
                        if self.credentials.is_empty() {
                            ui.add_space(SPACE_2XL);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    egui::RichText::new("\u{1f511}")
                                        .size(SPACE_2XL)
                                        .color(self.theme.fg_dim),
                                );
                                ui.add_space(SPACE_MD);
                                ui.label(
                                    egui::RichText::new(self.language.t("keychain_empty"))
                                        .color(self.theme.fg_dim)
                                        .size(FONT_BASE),
                                );
                            });
                            return;
                        }

                        // ── Section header ──
                        ui.horizontal(|ui| {
                            ui.add_space(SPACE_XL);
                            ui.label(
                                egui::RichText::new(self.language.t("credentials_section"))
                                    .color(self.theme.fg_dim)
                                    .size(FONT_XS)
                                    .strong(),
                            );
                        });
                        ui.add_space(SPACE_XS);

                        // ── Credential rows ──
                        for cred in &self.credentials {
                            // Count how many hosts reference this credential
                            let bound_hosts: Vec<String> = self.hosts.iter()
                                .filter(|h| h.credential_id.as_ref() == Some(&cred.id))
                                .map(|h| h.name.clone())
                                .collect();
                            let binding_count = bound_hosts.len();

                            let type_key = match &cred.credential_type {
                                CredentialType::Password { .. } => "credential_password",
                                CredentialType::SshKey { .. } => "credential_private_key",
                            };

                            let subtitle = match &cred.credential_type {
                                CredentialType::Password { username } => {
                                    if username.is_empty() {
                                        self.language.t("credential_password").to_string()
                                    } else {
                                        format!("{}: {}", self.language.t("username"), username)
                                    }
                                }
                                CredentialType::SshKey { key_path, .. } => {
                                    if key_path.is_empty() {
                                        self.language.t("ssh_key").to_string()
                                    } else {
                                        format!("{}: {}", self.language.t("key_source_path"), key_path)
                                    }
                                }
                            };

                            let row_h = LIST_ROW_HEIGHT;
                            let width = ui.available_width();
                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(width, row_h),
                                egui::Sense::click(),
                            );
                            let hovered = resp.hovered();

                            // Background hover effect
                            if hovered {
                                ui.painter().rect_filled(
                                    egui::Rect::from_min_max(
                                        egui::pos2(rect.min.x, rect.max.y - 1.0),
                                        rect.max,
                                    ),
                                    0.0,
                                    self.theme.hover_shadow,
                                );
                                ui.painter().rect_filled(rect, 0.0, self.theme.hover_bg);
                            }

                            // Icon (matching hosts_view layout)
                            ui.painter().text(
                                egui::pos2(rect.min.x + 24.0, rect.min.y + 18.0),
                                egui::Align2::LEFT_CENTER,
                                "\u{1f511}",
                                egui::FontId::proportional(12.0),
                                self.theme.accent,
                            );

                            // Name (matching hosts_view layout)
                            ui.painter().text(
                                egui::pos2(rect.min.x + 46.0, rect.min.y + 18.0),
                                egui::Align2::LEFT_CENTER,
                                &cred.name,
                                egui::FontId::proportional(13.0),
                                self.theme.fg_primary,
                            );

                            // Subtitle (matching hosts_view layout)
                            ui.painter().text(
                                egui::pos2(rect.min.x + 46.0, rect.min.y + 34.0),
                                egui::Align2::LEFT_CENTER,
                                &subtitle,
                                egui::FontId::proportional(10.0),
                                self.theme.fg_dim,
                            );

                            // Right-aligned: edit button + badges (hover only)
                            if hovered {
                                let visible_right = ui.clip_rect().max.x;
                                let pointer_pos = ui.ctx().input(|i| i.pointer.hover_pos());

                                // Edit button (matching hosts_view style)
                                let btn_rect = egui::Rect::from_center_size(
                                    egui::pos2(visible_right - 40.0, rect.min.y + 26.0),
                                    egui::vec2(56.0, 22.0),
                                );
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
                                // Handle edit click
                                if over_btn && resp.clicked() {
                                    self.credential_dialog.open_edit(cred);
                                }
                                ui.allocate_exact_size(egui::vec2(56.0, 22.0), egui::Sense::hover());
                            }

                            // Badges (visible when not hovering for cleaner look)
                            if !hovered {
                                let visible_right = ui.clip_rect().max.x;
                                let mut badge_x = visible_right;

                                // Type badge
                                let type_text = self.language.t(type_key).to_string();
                                let type_galley = ui.painter().layout_no_wrap(
                                    type_text,
                                    egui::FontId::proportional(FONT_XS),
                                    self.theme.fg_dim,
                                );
                                let type_w = type_galley.size().x + SPACE_LG;
                                let type_h = 18.0;
                                badge_x -= type_w + SPACE_SM;
                                let type_rect = egui::Rect::from_center_size(
                                    egui::pos2(badge_x + type_w / 2.0, rect.center().y),
                                    egui::vec2(type_w, type_h),
                                );
                                ui.painter().rect(
                                    type_rect,
                                    RADIUS_SM,
                                    egui::Color32::TRANSPARENT,
                                    egui::Stroke::new(1.0, self.theme.border),
                                );
                                ui.painter().galley(
                                    egui::pos2(type_rect.center().x - type_galley.size().x / 2.0, type_rect.center().y - type_galley.size().y / 2.0),
                                    type_galley,
                                    self.theme.fg_dim,
                                );

                                // Binding count badge
                                if binding_count > 0 {
                                    let badge_text = format!("{} {}", binding_count, self.language.t("hosts_bound"));
                                    let badge_galley = ui.painter().layout_no_wrap(
                                        badge_text,
                                        egui::FontId::proportional(FONT_XS),
                                        self.theme.accent,
                                    );
                                    let badge_w = badge_galley.size().x + SPACE_MD;
                                    badge_x -= badge_w + SPACE_SM;
                                    let badge_rect = egui::Rect::from_center_size(
                                        egui::pos2(badge_x + badge_w / 2.0, rect.center().y),
                                        egui::vec2(badge_w, type_h),
                                    );
                                    ui.painter().rect_filled(badge_rect, RADIUS_SM, self.theme.badge_bg);
                                    ui.painter().galley(
                                        egui::pos2(badge_rect.center().x - badge_galley.size().x / 2.0, badge_rect.center().y - badge_galley.size().y / 2.0),
                                        badge_galley,
                                        self.theme.accent,
                                    );
                                }
                            }

                            // Handle click interactions
                            if resp.clicked() && !hovered {
                                // Only open edit on non-hovered row click (prevent double-trigger)
                                self.credential_dialog.open_edit(cred);
                            }
                        }

                        ui.add_space(40.0);
                    });
            });

        // ── Delete confirmation dialog ──
        if self.keychain_confirm_delete.is_some() {
            self.show_keychain_delete_dialog(ctx);
        }
    }

    pub fn show_credential_drawer(&mut self, ctx: &egui::Context) {
        let theme = self.theme.clone();
        let lang = self.language;
        let drawer_width = ctx.screen_rect().width().min(DRAWER_WIDTH).max(280.0);

        let mut save_clicked = false;
        let mut close_clicked = false;

        egui::SidePanel::right("credential_drawer")
            .exact_width(drawer_width)
            .resizable(false)
            .frame(egui::Frame {
                fill: theme.bg_secondary,
                inner_margin: egui::Margin::same(20.0),
                stroke: egui::Stroke::new(1.0, theme.border),
                ..Default::default()
            })
            .show(ctx, |ui| {
                let title = if self.credential_dialog.edit_id.is_some() {
                    lang.t("edit_credential")
                } else {
                    lang.t("new_credential")
                };

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(title).color(theme.fg_primary).size(FONT_BASE).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(
                            egui::Button::new(egui::RichText::new("\u{2715}").color(theme.fg_dim).size(FONT_MD))
                                .frame(false)
                        ).clicked() {
                            close_clicked = true;
                        }
                        if self.credential_dialog.edit_id.is_some() {
                            if ui.add(
                                egui::Button::new(egui::RichText::new("\u{1F5D1}").size(FONT_BASE))
                                    .frame(false)
                            ).on_hover_text(lang.t("delete"))
                            .clicked() {
                                // Set confirm_delete to the editing credential id and close drawer
                                if let Some(editing_id) = &self.credential_dialog.edit_id {
                                    let affected_hosts: Vec<String> = self.hosts.iter()
                                        .filter(|h| h.credential_id.as_ref() == Some(editing_id))
                                        .map(|h| h.name.clone())
                                        .collect();
                                    self.keychain_confirm_delete = Some(
                                        KeychainDeleteRequest::ById {
                                            credential_id: editing_id.clone(),
                                            affected_hosts,
                                        }
                                    );
                                }
                                close_clicked = true;
                            }
                        }
                    });
                });
                ui.add_space(16.0);

                // Name field
                ui.label(widgets::field_label(lang.t("label"), &theme));
                ui.add_space(4.0);
                ui.add(egui::TextEdit::singleline(&mut self.credential_dialog.name)
                    .desired_width(f32::INFINITY)
                    .hint_text(egui::RichText::new("My SSH Key").color(theme.hint_color()).italics())
                    .font(egui::FontId::proportional(13.0))
                    .text_color(theme.fg_primary));
                ui.add_space(12.0);

                // Type selector
                ui.label(widgets::field_label(lang.t("credential_type"), &theme));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.credential_dialog.cred_type,
                        CredentialTypeChoice::Password,
                        egui::RichText::new(lang.t("password")).size(12.0),
                    );
                    ui.selectable_value(
                        &mut self.credential_dialog.cred_type,
                        CredentialTypeChoice::SshKey,
                        egui::RichText::new(lang.t("ssh_key")).size(12.0),
                    );
                });
                ui.add_space(12.0);

                match self.credential_dialog.cred_type {
                    CredentialTypeChoice::Password => {
                        // Username field
                        ui.label(widgets::field_label(lang.t("username"), &theme));
                        ui.add_space(4.0);
                        ui.add(egui::TextEdit::singleline(&mut self.credential_dialog.username)
                            .desired_width(f32::INFINITY)
                            .hint_text(egui::RichText::new("root").color(theme.hint_color()).italics())
                            .font(egui::FontId::proportional(13.0))
                            .text_color(theme.fg_primary));
                        ui.add_space(12.0);

                        // Password field
                        ui.label(widgets::field_label(lang.t("password"), &theme));
                        ui.add_space(4.0);
                        ui.add(egui::TextEdit::singleline(&mut self.credential_dialog.password)
                            .desired_width(f32::INFINITY)
                            .password(true)
                            .hint_text(egui::RichText::new("Enter password").color(theme.hint_color()).italics())
                            .font(egui::FontId::proportional(13.0))
                            .text_color(theme.fg_primary));
                    }
                    CredentialTypeChoice::SshKey => {
                        // Key source selector
                        ui.horizontal(|ui| {
                            ui.selectable_value(
                                &mut self.credential_dialog.key_source,
                                KeySourceChoice::LocalFile,
                                egui::RichText::new(lang.t("key_path")).size(12.0),
                            );
                            ui.selectable_value(
                                &mut self.credential_dialog.key_source,
                                KeySourceChoice::ImportContent,
                                egui::RichText::new(lang.t("import_key")).size(12.0),
                            );
                        });
                        ui.add_space(8.0);

                        match self.credential_dialog.key_source {
                            KeySourceChoice::LocalFile => {
                                ui.label(widgets::field_label(lang.t("key_path"), &theme));
                                ui.add_space(4.0);
                                ui.add(egui::TextEdit::singleline(&mut self.credential_dialog.key_path)
                                    .desired_width(f32::INFINITY)
                                    .hint_text(egui::RichText::new("~/.ssh/id_rsa").color(theme.hint_color()).italics())
                                    .font(egui::FontId::proportional(13.0))
                                    .text_color(theme.fg_primary));
                            }
                            KeySourceChoice::ImportContent => {
                                ui.label(widgets::field_label(lang.t("key_content"), &theme));
                                ui.add_space(4.0);
                                ui.add(egui::TextEdit::multiline(&mut self.credential_dialog.key_content)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(4)
                                    .hint_text(egui::RichText::new("-----BEGIN OPENSSH PRIVATE KEY-----\n...\n-----END OPENSSH PRIVATE KEY-----").color(theme.hint_color()).italics())
                                    .font(egui::FontId::monospace(11.0))
                                    .text_color(theme.fg_primary));
                            }
                        }
                        ui.add_space(12.0);

                        // Passphrase
                        ui.label(widgets::field_label(lang.t("key_passphrase"), &theme));
                        ui.add_space(4.0);
                        ui.add(egui::TextEdit::singleline(&mut self.credential_dialog.key_passphrase)
                            .desired_width(f32::INFINITY)
                            .password(true)
                            .hint_text(egui::RichText::new("Leave empty if none").color(theme.hint_color()).italics())
                            .font(egui::FontId::proportional(13.0))
                            .text_color(theme.fg_primary));
                    }
                }
                ui.add_space(16.0);

                // Error message
                if !self.credential_dialog.error.is_empty() {
                    ui.label(egui::RichText::new(&self.credential_dialog.error).color(theme.red).size(12.0));
                    ui.add_space(8.0);
                }

                // Save / Cancel buttons
                ui.horizontal(|ui| {
                    if ui.add(widgets::primary_button(lang.t("save"), &theme)).clicked() {
                        save_clicked = true;
                    }
                    if ui.add(widgets::secondary_button(lang.t("cancel"), &theme)).clicked() {
                        close_clicked = true;
                    }
                });
            });

        if close_clicked {
            self.credential_dialog.reset();
        }

        if save_clicked {
            self.save_credential_from_dialog();
        }
    }

    fn save_credential_from_dialog(&mut self) {
        let name = self.credential_dialog.name.trim().to_owned();
        if name.is_empty() {
            self.credential_dialog.error = "Name is required.".to_owned();
            return;
        }

        // Extract all dialog values before borrowing self.credentials mutably
        let cred_type = self.credential_dialog.cred_type;
        let dialog_username = self.credential_dialog.username.clone();
        let dialog_password = self.credential_dialog.password.clone();
        let dialog_key_path = self.credential_dialog.key_path.clone();
        let dialog_key_passphrase = self.credential_dialog.key_passphrase.clone();
        let key_content = self.get_key_content_from_dialog();

        if let Some(ref edit_id) = self.credential_dialog.edit_id.clone() {
            // Edit existing
            if let Some(cred) = self.credentials.iter_mut().find(|c| c.id == *edit_id) {
                cred.name = name;
                match cred_type {
                    CredentialTypeChoice::Password => {
                        cred.credential_type = CredentialType::Password {
                            username: dialog_username,
                        };
                        if !dialog_password.is_empty() {
                            config::store_credential_secret(&cred.id, &cred.name, "password", &dialog_password);
                        }
                    }
                    CredentialTypeChoice::SshKey => {
                        let key_in_keychain = if !key_content.is_empty() {
                            config::store_credential_secret(&cred.id, &cred.name, "privatekey", &key_content)
                        } else {
                            match &cred.credential_type {
                                CredentialType::SshKey { key_in_keychain, .. } => *key_in_keychain,
                                _ => false,
                            }
                        };
                        let has_passphrase = !dialog_key_passphrase.is_empty();
                        if has_passphrase {
                            config::store_credential_secret(&cred.id, &cred.name, "passphrase", &dialog_key_passphrase);
                        }
                        cred.credential_type = CredentialType::SshKey {
                            key_path: dialog_key_path,
                            key_in_keychain,
                            has_passphrase,
                        };
                    }
                }
            }
        } else {
            // Create new
            match cred_type {
                CredentialTypeChoice::Password => {
                    let cred = Credential::new_password(name, dialog_username);
                    if !dialog_password.is_empty() {
                        config::store_credential_secret(&cred.id, &cred.name, "password", &dialog_password);
                    }
                    self.credentials.push(cred);
                }
                CredentialTypeChoice::SshKey => {
                    let has_passphrase = !dialog_key_passphrase.is_empty();
                    let mut cred = Credential::new_ssh_key(
                        name,
                        dialog_key_path,
                        false,
                        has_passphrase,
                    );
                    if !key_content.is_empty() {
                        config::store_credential_secret(&cred.id, &cred.name, "privatekey", &key_content);
                        if let CredentialType::SshKey { ref mut key_in_keychain, .. } = cred.credential_type {
                            *key_in_keychain = true;
                        }
                    }
                    if has_passphrase {
                        config::store_credential_secret(&cred.id, &cred.name, "passphrase", &dialog_key_passphrase);
                    }
                    self.credentials.push(cred);
                }
            }
        }

        self.save_credentials();
        self.credential_dialog.reset();
    }

    fn get_key_content_from_dialog(&self) -> String {
        match self.credential_dialog.key_source {
            KeySourceChoice::ImportContent => {
                self.credential_dialog.key_content.trim().to_owned()
            }
            KeySourceChoice::LocalFile => {
                let key_path = self.credential_dialog.key_path.trim();
                if key_path.is_empty() {
                    return String::new();
                }
                let expanded = if key_path.starts_with('~') {
                    if let Some(home) = dirs::home_dir() {
                        home.join(&key_path[2..]).to_string_lossy().to_string()
                    } else {
                        key_path.to_owned()
                    }
                } else {
                    key_path.to_owned()
                };
                std::fs::read_to_string(&expanded).unwrap_or_default()
            }
        }
    }

    fn show_keychain_delete_dialog(&mut self, ctx: &egui::Context) {
        let lang = self.language;

        let confirm_msg = match self.keychain_confirm_delete.as_ref().unwrap() {
            KeychainDeleteRequest::ById { credential_id, affected_hosts } => {
                let cred_name = self.credentials.iter()
                    .find(|c| c.id == *credential_id)
                    .map(|c| c.name.clone())
                    .unwrap_or_default();
                if affected_hosts.is_empty() {
                    lang.tf("delete_confirm", &cred_name)
                } else {
                    format!(
                        "{}\n\n{}: {}",
                        lang.tf("delete_confirm", &cred_name),
                        lang.t("affected_hosts"),
                        affected_hosts.join(", ")
                    )
                }
            }
        };

        let mut open = true;
        egui::Window::new(lang.t("keychain"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .min_size([280.0, 0.0])
            .default_size([DIALOG_WIDTH_MD, 0.0])
            .title_bar(false)
            .frame(widgets::dialog_frame(&self.theme))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("\u{26A0}").size(18.0).color(self.theme.red));
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(lang.t("delete")).size(15.0).color(self.theme.fg_primary).strong());
                });
                ui.add_space(10.0);

                ui.label(
                    egui::RichText::new(&confirm_msg)
                        .color(self.theme.fg_primary).size(13.0)
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(lang.t("confirm_delete"))
                        .color(self.theme.fg_dim).size(11.0)
                );
                ui.add_space(16.0);

                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(widgets::danger_button(lang.t("delete"), &self.theme)).clicked() {
                            self.execute_keychain_delete();
                        }
                        if ui.add(widgets::secondary_button(lang.t("cancel"), &self.theme)).clicked() {
                            self.keychain_confirm_delete = None;
                        }
                    });
                });
            });

        if !open {
            self.keychain_confirm_delete = None;
        }
    }

    /// Execute the confirmed keychain deletion.
    fn execute_keychain_delete(&mut self) {
        let request = self.keychain_confirm_delete.take();
        match request {
            Some(KeychainDeleteRequest::ById { credential_id, .. }) => {
                if let Some(cred) = self.credentials.iter().find(|c| c.id == credential_id) {
                    config::delete_credential_secrets(&cred.id, &cred.name);
                }
                self.credentials.retain(|c| c.id != credential_id);
                // Clear credential_id references on hosts
                for host in &mut self.hosts {
                    if host.credential_id.as_deref() == Some(&credential_id) {
                        host.credential_id = None;
                    }
                }
                self.save_credentials();
                self.save_hosts();
            }
            None => {}
        }
    }
}
