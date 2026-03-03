use eframe::egui;

use crate::PortalApp;
use crate::config::{self, AuthMethod};
use crate::ui::types::KeychainDeleteRequest;
use crate::ui::theme::brighter;

/// A single credential row to display.
struct CredentialRow {
    host_name: String,
    subtitle: String,
    kind_key: &'static str,
    host_index: usize,
    cred_kind: &'static str,
    /// For Key types, the original file path (stored in JSON, not keychain).
    key_path: Option<String>,
}

impl PortalApp {
    pub fn show_keychain_view(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let theme = self.theme.clone();
        let lang = self.language;

        // Collect credential rows from hosts
        let mut rows: Vec<CredentialRow> = Vec::new();
        for (idx, host) in self.hosts.iter().enumerate() {
            if host.is_local {
                continue;
            }
            let subtitle = format!("{}@{}:{}", host.username, host.host, host.port);
            match &host.auth {
                AuthMethod::Password { password } if !password.is_empty() => {
                    rows.push(CredentialRow {
                        host_name: host.name.clone(),
                        subtitle: subtitle.clone(),
                        kind_key: "credential_password",
                        host_index: idx,
                        cred_kind: "password",
                        key_path: None,
                    });
                }
                AuthMethod::Key { key_in_keychain, passphrase, key_path, .. } => {
                    if *key_in_keychain {
                        rows.push(CredentialRow {
                            host_name: host.name.clone(),
                            subtitle: subtitle.clone(),
                            kind_key: "credential_private_key",
                            host_index: idx,
                            cred_kind: "privatekey",
                            key_path: if key_path.is_empty() { None } else { Some(key_path.clone()) },
                        });
                    }
                    if !passphrase.is_empty() {
                        rows.push(CredentialRow {
                            host_name: host.name.clone(),
                            subtitle: subtitle.clone(),
                            kind_key: "credential_passphrase",
                            host_index: idx,
                            cred_kind: "passphrase",
                            key_path: None,
                        });
                    }
                }
                _ => {}
            }
        }

        let mut delete_request: Option<KeychainDeleteRequest> = None;

        egui::ScrollArea::vertical()
            .id_salt("keychain_page_scroll")
            .show(ui, |ui| {
            ui.add_space(20.0);

            // ── Page header (same pattern as hosts_view) ──
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
                    if !rows.is_empty() {
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

            if rows.is_empty() {
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
                // ── Section header (same as hosts "SSH HOSTS" style) ──
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.label(
                        egui::RichText::new(lang.t("storage_keychain"))
                            .color(theme.fg_dim)
                            .size(10.0)
                            .strong(),
                    );
                });
                ui.add_space(4.0);

                let border = brighter(theme.bg_elevated, 20);

                // ── Credential rows (painter-based, matching hosts_view) ──
                for row in rows.iter() {
                    let row_h = if row.key_path.is_some() { 54.0 } else { 44.0 };
                    let width = ui.available_width();
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(width, row_h),
                        egui::Sense::click(),
                    );
                    let hovered = resp.hovered();

                    // Hover background (same as hosts_view)
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

                    // Icon: 🔑
                    ui.painter().text(
                        egui::pos2(rect.min.x + 24.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        "\u{1f511}",
                        egui::FontId::proportional(12.0),
                        theme.accent,
                    );

                    // Host name (top line)
                    let name_y = if row.key_path.is_some() {
                        rect.center().y - 12.0
                    } else {
                        rect.center().y - 7.0
                    };
                    ui.painter().text(
                        egui::pos2(rect.min.x + 46.0, name_y),
                        egui::Align2::LEFT_CENTER,
                        &row.host_name,
                        egui::FontId::proportional(13.0),
                        theme.fg_primary,
                    );

                    // Subtitle (user@host:port)
                    let sub_y = if row.key_path.is_some() {
                        rect.center().y + 1.0
                    } else {
                        rect.center().y + 8.0
                    };
                    ui.painter().text(
                        egui::pos2(rect.min.x + 46.0, sub_y),
                        egui::Align2::LEFT_CENTER,
                        &row.subtitle,
                        egui::FontId::proportional(10.0),
                        theme.fg_dim,
                    );

                    // Key source path (third line, only for key types)
                    if let Some(ref path) = row.key_path {
                        let path_text = format!("{}: {}", lang.t("key_source_path"), path);
                        ui.painter().text(
                            egui::pos2(rect.min.x + 46.0, rect.center().y + 14.0),
                            egui::Align2::LEFT_CENTER,
                            path_text,
                            egui::FontId::proportional(9.0),
                            theme.fg_dim,
                        );
                    }

                    // ── Right side: badges + delete button ──
                    let right_x = rect.max.x - 24.0;

                    // Delete button (only visible on hover, matching hosts Edit button style)
                    if hovered {
                        let del_rect = egui::Rect::from_center_size(
                            egui::pos2(right_x - 4.0, rect.center().y),
                            egui::vec2(24.0, 22.0),
                        );
                        let pointer_pos = ui.ctx().input(|i| i.pointer.hover_pos());
                        let over_del = pointer_pos.map_or(false, |p| del_rect.contains(p));
                        let del_bg = if over_del {
                            theme.red
                        } else {
                            theme.bg_elevated
                        };
                        let del_fg = if over_del {
                            theme.bg_primary
                        } else {
                            theme.fg_dim
                        };
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
                                delete_request = Some(KeychainDeleteRequest::Single {
                                    host_index: row.host_index,
                                    kind: row.cred_kind.to_string(),
                                });
                            }
                        }
                    }

                    // Type badge (e.g. "Password", "Private Key")
                    let badge_text = lang.t(row.kind_key);
                    let badge_galley = ui.painter().layout_no_wrap(
                        badge_text.to_string(),
                        egui::FontId::proportional(10.0),
                        theme.fg_dim,
                    );
                    let badge_w = badge_galley.size().x + 16.0;
                    let badge_h = 18.0;
                    let badge_x = right_x - 34.0 - badge_w;
                    let badge_rect = egui::Rect::from_min_size(
                        egui::pos2(badge_x, rect.center().y - badge_h / 2.0),
                        egui::vec2(badge_w, badge_h),
                    );
                    ui.painter().rect(
                        badge_rect,
                        4.0,
                        egui::Color32::TRANSPARENT,
                        egui::Stroke::new(1.0, border),
                    );
                    ui.painter().galley(
                        egui::pos2(badge_rect.center().x - badge_galley.size().x / 2.0, badge_rect.center().y - badge_galley.size().y / 2.0),
                        badge_galley,
                        theme.fg_dim,
                    );

                    // Storage badge ("🔐 Keychain")
                    let store_text = lang.t("storage_keychain");
                    let store_galley = ui.painter().layout_no_wrap(
                        format!("\u{1f510} {}", store_text),
                        egui::FontId::proportional(10.0),
                        theme.accent,
                    );
                    let store_w = store_galley.size().x + 12.0;
                    let store_rect = egui::Rect::from_min_size(
                        egui::pos2(badge_x - store_w - 6.0, rect.center().y - badge_h / 2.0),
                        egui::vec2(store_w, badge_h),
                    );
                    ui.painter().rect_filled(store_rect, 4.0, theme.accent_alpha(20));
                    ui.painter().galley(
                        egui::pos2(store_rect.center().x - store_galley.size().x / 2.0, store_rect.center().y - store_galley.size().y / 2.0),
                        store_galley,
                        theme.accent,
                    );
                }
            }

            ui.add_space(20.0);
        }); // end ScrollArea

        // Apply deferred delete request
        if let Some(req) = delete_request {
            self.keychain_confirm_delete = Some(req);
        }

        // ── Confirmation dialog (shadcn modal style) ──
        if self.keychain_confirm_delete.is_some() {
            let confirm_msg = match self.keychain_confirm_delete.as_ref().unwrap() {
                KeychainDeleteRequest::All => lang.t("delete_all_confirm").to_string(),
                KeychainDeleteRequest::Single { host_index, kind } => {
                    let label = self.hosts.get(*host_index)
                        .map(|h| {
                            let kind_label = lang.t(match kind.as_str() {
                                "password" => "credential_password",
                                "privatekey" => "credential_private_key",
                                "passphrase" => "credential_passphrase",
                                _ => "keychain",
                            });
                            format!("{} ({})", h.name, kind_label)
                        })
                        .unwrap_or_default();
                    lang.tf("delete_confirm", &label)
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
                    // Warning icon + title
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("\u{26A0}").size(18.0).color(self.theme.red));
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(lang.t("delete_all")).size(15.0).color(self.theme.fg_primary).strong());
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
    }

    /// Execute the confirmed keychain deletion: remove from keychain, update host auth in memory, save.
    fn execute_keychain_delete(&mut self) {
        let request = self.keychain_confirm_delete.take();
        match request {
            Some(KeychainDeleteRequest::All) => {
                for host in &mut self.hosts {
                    if host.is_local {
                        continue;
                    }
                    config::delete_host_credentials(host);
                    match &mut host.auth {
                        AuthMethod::Password { password } => {
                            password.clear();
                        }
                        AuthMethod::Key { key_in_keychain, passphrase, .. } => {
                            *key_in_keychain = false;
                            passphrase.clear();
                        }
                        AuthMethod::None => {}
                    }
                }
                self.save_hosts();
            }
            Some(KeychainDeleteRequest::Single { host_index, kind }) => {
                if let Some(host) = self.hosts.get(host_index) {
                    config::delete_credential(&host.host, host.port, &host.username, &kind, &host.name);
                }
                if let Some(host) = self.hosts.get_mut(host_index) {
                    match &mut host.auth {
                        AuthMethod::Password { password } if kind == "password" => {
                            password.clear();
                        }
                        AuthMethod::Key { key_in_keychain, passphrase, .. } => {
                            if kind == "privatekey" {
                                *key_in_keychain = false;
                            } else if kind == "passphrase" {
                                passphrase.clear();
                            }
                        }
                        _ => {}
                    }
                }
                self.save_hosts();
            }
            None => {}
        }
    }
}
