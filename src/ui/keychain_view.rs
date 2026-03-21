use eframe::egui;

use crate::app::PortalApp;
use crate::config::{self, Credential, CredentialType};
use crate::ui::types::{KeychainDeleteRequest, CredentialTypeChoice, KeySourceChoice};
use crate::ui::theme::brighter;

impl PortalApp {
    pub fn show_keychain_view(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let theme = self.theme.clone();
        let lang = self.language;

        // Count how many hosts reference each credential
        let binding_counts: std::collections::HashMap<String, Vec<String>> = {
            let mut map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
            for host in &self.hosts {
                if let Some(ref cid) = host.credential_id {
                    map.entry(cid.clone()).or_default().push(host.name.clone());
                }
            }
            map
        };

        let mut delete_request: Option<KeychainDeleteRequest> = None;
        let mut edit_credential_id: Option<String> = None;

        egui::ScrollArea::vertical()
            .id_salt("keychain_page_scroll")
            .show(ui, |ui| {
            ui.add_space(20.0);

            // ── Page header ──
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                ui.label(
                    egui::RichText::new(lang.t("keychain"))
                        .color(theme.fg_dim)
                        .size(12.0)
                        .strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(24.0);
                    // New credential button
                    if ui.add(
                        egui::Button::new(
                            egui::RichText::new(lang.t("new_credential"))
                                .color(theme.accent)
                                .size(12.0),
                        )
                        .frame(false)
                    ).clicked() {
                        self.credential_dialog.open_new();
                    }
                    if !self.credentials.is_empty() {
                        ui.add_space(12.0);
                        if ui.add(
                            egui::Button::new(
                                egui::RichText::new(lang.t("delete_all"))
                                    .color(theme.red)
                                    .size(12.0),
                            )
                            .frame(false)
                        ).clicked() {
                            delete_request = Some(KeychainDeleteRequest::All);
                        }
                    }
                });
            });
            ui.add_space(16.0);

            if self.credentials.is_empty() {
                // ── Empty state ──
                ui.add_space(60.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("\u{1f511}")
                            .size(32.0)
                            .color(theme.fg_dim),
                    );
                    ui.add_space(12.0);
                    ui.label(
                        egui::RichText::new(lang.t("keychain_empty"))
                            .color(theme.fg_dim)
                            .size(13.0),
                    );
                });
                ui.add_space(60.0);
            } else {
                // ── Section header ──
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.label(
                        egui::RichText::new(lang.t("credentials_section"))
                            .color(theme.fg_dim)
                            .size(10.0)
                            .strong(),
                    );
                });
                ui.add_space(4.0);

                let border = brighter(theme.bg_elevated, 20);

                // ── Credential rows ──
                for cred in &self.credentials {
                    let bound_hosts = binding_counts.get(&cred.id);
                    let binding_count = bound_hosts.map(|v| v.len()).unwrap_or(0);

                    let type_key = match &cred.credential_type {
                        CredentialType::Password { .. } => "credential_password",
                        CredentialType::SshKey { .. } => "credential_private_key",
                    };

                    let subtitle = match &cred.credential_type {
                        CredentialType::Password { username } => {
                            if username.is_empty() {
                                lang.t("credential_password").to_string()
                            } else {
                                format!("{}: {}", lang.t("username"), username)
                            }
                        }
                        CredentialType::SshKey { key_path, .. } => {
                            if key_path.is_empty() {
                                lang.t("ssh_key").to_string()
                            } else {
                                format!("{}: {}", lang.t("key_source_path"), key_path)
                            }
                        }
                    };

                    let row_h = 52.0;
                    let width = ui.available_width();
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(width, row_h),
                        egui::Sense::click(),
                    );
                    let hovered = resp.hovered();

                    if hovered {
                        ui.painter().rect_filled(
                            egui::Rect::from_min_max(
                                egui::pos2(rect.min.x, rect.max.y - 1.0),
                                rect.max,
                            ),
                            0.0,
                            theme.hover_shadow,
                        );
                        ui.painter().rect_filled(rect, 0.0, theme.hover_bg);
                    }

                    // Icon
                    ui.painter().text(
                        egui::pos2(rect.min.x + 24.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        "\u{1f511}",
                        egui::FontId::proportional(12.0),
                        theme.accent,
                    );

                    // Name (top line)
                    ui.painter().text(
                        egui::pos2(rect.min.x + 46.0, rect.center().y - 8.0),
                        egui::Align2::LEFT_CENTER,
                        &cred.name,
                        egui::FontId::proportional(13.0),
                        theme.fg_primary,
                    );

                    // Subtitle (bottom line)
                    ui.painter().text(
                        egui::pos2(rect.min.x + 46.0, rect.center().y + 8.0),
                        egui::Align2::LEFT_CENTER,
                        &subtitle,
                        egui::FontId::proportional(10.0),
                        theme.fg_dim,
                    );

                    // ── Right side: badges + delete button ──
                    let right_x = rect.max.x - 24.0;

                    // Delete button (hover only)
                    if hovered {
                        let del_rect = egui::Rect::from_center_size(
                            egui::pos2(right_x - 4.0, rect.center().y),
                            egui::vec2(24.0, 22.0),
                        );
                        let pointer_pos = ui.ctx().input(|i| i.pointer.hover_pos());
                        let over_del = pointer_pos.map_or(false, |p| del_rect.contains(p));
                        let del_bg = if over_del { theme.red } else { theme.bg_elevated };
                        let del_fg = if over_del { theme.bg_primary } else { theme.fg_dim };
                        ui.painter().rect(del_rect, 4.0, del_bg, egui::Stroke::NONE);
                        ui.painter().text(
                            del_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "\u{2715}",
                            egui::FontId::proportional(10.0),
                            del_fg,
                        );

                        if resp.clicked() {
                            let click_pos = ui.ctx().input(|i| i.pointer.interact_pos());
                            if click_pos.map_or(false, |p| del_rect.contains(p)) {
                                delete_request = Some(KeychainDeleteRequest::ById {
                                    credential_id: cred.id.clone(),
                                    affected_hosts: bound_hosts.cloned().unwrap_or_default(),
                                });
                            } else {
                                // Click on row (not delete) → edit
                                edit_credential_id = Some(cred.id.clone());
                            }
                        }
                    } else if resp.clicked() {
                        edit_credential_id = Some(cred.id.clone());
                    }

                    // Binding count badge
                    if binding_count > 0 {
                        let badge_text = format!("{} {}", binding_count, lang.t("hosts_bound"));
                        let badge_galley = ui.painter().layout_no_wrap(
                            badge_text,
                            egui::FontId::proportional(10.0),
                            theme.accent,
                        );
                        let badge_w = badge_galley.size().x + 12.0;
                        let badge_h = 18.0;
                        let badge_x = right_x - 34.0 - badge_w;
                        let badge_rect = egui::Rect::from_min_size(
                            egui::pos2(badge_x, rect.center().y - badge_h / 2.0),
                            egui::vec2(badge_w, badge_h),
                        );
                        ui.painter().rect_filled(badge_rect, 4.0, theme.accent_alpha(20));
                        ui.painter().galley(
                            egui::pos2(badge_rect.center().x - badge_galley.size().x / 2.0, badge_rect.center().y - badge_galley.size().y / 2.0),
                            badge_galley,
                            theme.accent,
                        );
                    }

                    // Type badge
                    let type_text = lang.t(type_key);
                    let type_galley = ui.painter().layout_no_wrap(
                        type_text.to_string(),
                        egui::FontId::proportional(10.0),
                        theme.fg_dim,
                    );
                    let type_w = type_galley.size().x + 16.0;
                    let type_h = 18.0;
                    let type_x = if binding_count > 0 {
                        right_x - 34.0 - 80.0 - type_w  // after binding badge
                    } else {
                        right_x - 34.0 - type_w
                    };
                    let type_rect = egui::Rect::from_min_size(
                        egui::pos2(type_x, rect.center().y - type_h / 2.0),
                        egui::vec2(type_w, type_h),
                    );
                    ui.painter().rect(
                        type_rect,
                        4.0,
                        egui::Color32::TRANSPARENT,
                        egui::Stroke::new(1.0, border),
                    );
                    ui.painter().galley(
                        egui::pos2(type_rect.center().x - type_galley.size().x / 2.0, type_rect.center().y - type_galley.size().y / 2.0),
                        type_galley,
                        theme.fg_dim,
                    );
                }
            }

            ui.add_space(20.0);
        }); // end ScrollArea

        // Open edit dialog for clicked credential
        if let Some(cid) = edit_credential_id {
            if let Some(cred) = self.credentials.iter().find(|c| c.id == cid) {
                let cred = cred.clone();
                self.credential_dialog.open_edit(&cred);
            }
        }

        // Apply deferred delete request
        if let Some(req) = delete_request {
            self.keychain_confirm_delete = Some(req);
        }

        // ── Credential create/edit drawer (right SidePanel) ──
        if self.credential_dialog.open {
            self.show_credential_drawer(ctx);
        }

        // ── Delete confirmation dialog ──
        if self.keychain_confirm_delete.is_some() {
            self.show_keychain_delete_dialog(ctx);
        }
    }

    fn show_credential_drawer(&mut self, ctx: &egui::Context) {
        let theme = self.theme.clone();
        let lang = self.language;
        let drawer_width = 340.0;

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
                    ui.label(egui::RichText::new(title).color(theme.fg_primary).size(15.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(
                            egui::Button::new(egui::RichText::new("\u{2715}").color(theme.fg_dim).size(14.0))
                                .frame(false)
                        ).clicked() {
                            close_clicked = true;
                        }
                    });
                });
                ui.add_space(16.0);

                // Name field
                ui.label(egui::RichText::new(lang.t("label")).color(theme.fg_dim).size(12.0));
                ui.add_space(4.0);
                ui.add(egui::TextEdit::singleline(&mut self.credential_dialog.name)
                    .desired_width(f32::INFINITY)
                    .font(egui::FontId::proportional(13.0)));
                ui.add_space(12.0);

                // Type selector
                ui.label(egui::RichText::new(lang.t("credential_type")).color(theme.fg_dim).size(12.0));
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
                        ui.label(egui::RichText::new(lang.t("username")).color(theme.fg_dim).size(12.0));
                        ui.add_space(4.0);
                        ui.add(egui::TextEdit::singleline(&mut self.credential_dialog.username)
                            .desired_width(f32::INFINITY)
                            .font(egui::FontId::proportional(13.0)));
                        ui.add_space(12.0);

                        // Password field
                        ui.label(egui::RichText::new(lang.t("password")).color(theme.fg_dim).size(12.0));
                        ui.add_space(4.0);
                        ui.add(egui::TextEdit::singleline(&mut self.credential_dialog.password)
                            .desired_width(f32::INFINITY)
                            .password(true)
                            .font(egui::FontId::proportional(13.0)));
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
                                ui.label(egui::RichText::new(lang.t("key_path")).color(theme.fg_dim).size(12.0));
                                ui.add_space(4.0);
                                ui.add(egui::TextEdit::singleline(&mut self.credential_dialog.key_path)
                                    .desired_width(f32::INFINITY)
                                    .hint_text("~/.ssh/id_rsa")
                                    .font(egui::FontId::proportional(13.0)));
                            }
                            KeySourceChoice::ImportContent => {
                                ui.label(egui::RichText::new(lang.t("key_content")).color(theme.fg_dim).size(12.0));
                                ui.add_space(4.0);
                                ui.add(egui::TextEdit::multiline(&mut self.credential_dialog.key_content)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(4)
                                    .font(egui::FontId::monospace(11.0)));
                            }
                        }
                        ui.add_space(12.0);

                        // Passphrase
                        ui.label(egui::RichText::new(lang.t("key_passphrase")).color(theme.fg_dim).size(12.0));
                        ui.add_space(4.0);
                        ui.add(egui::TextEdit::singleline(&mut self.credential_dialog.key_passphrase)
                            .desired_width(f32::INFINITY)
                            .password(true)
                            .font(egui::FontId::proportional(13.0)));
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
                    if ui.add(
                        egui::Button::new(egui::RichText::new(lang.t("save")).color(egui::Color32::WHITE).size(13.0))
                            .fill(theme.accent)
                            .rounding(6.0)
                            .min_size(egui::vec2(70.0, 32.0))
                    ).clicked() {
                        save_clicked = true;
                    }
                    if ui.add(
                        egui::Button::new(egui::RichText::new(lang.t("cancel")).color(theme.fg_dim).size(13.0))
                            .fill(theme.bg_elevated)
                            .rounding(6.0)
                            .min_size(egui::vec2(70.0, 32.0))
                    ).clicked() {
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
            KeychainDeleteRequest::All => lang.t("delete_all_confirm").to_string(),
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
                        if ui.add(
                            egui::Button::new(egui::RichText::new(lang.t("delete")).color(egui::Color32::WHITE).size(13.0))
                                .fill(self.theme.red)
                                .rounding(6.0)
                                .min_size(egui::vec2(70.0, 32.0))
                        ).clicked() {
                            self.execute_keychain_delete();
                        }
                        if ui.add(
                            egui::Button::new(egui::RichText::new(lang.t("cancel")).color(self.theme.fg_dim).size(13.0))
                                .fill(self.theme.bg_elevated)
                                .rounding(6.0)
                                .min_size(egui::vec2(70.0, 32.0))
                        ).clicked() {
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
            Some(KeychainDeleteRequest::All) => {
                for cred in &self.credentials {
                    config::delete_credential_secrets(&cred.id, &cred.name);
                }
                // Clear credential_id references on hosts
                for host in &mut self.hosts {
                    host.credential_id = None;
                }
                self.credentials.clear();
                self.save_credentials();
                self.save_hosts();
            }
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
