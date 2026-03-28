//! # Keychain View
//!
//! Rendering for the credential/keychain management page.

use eframe::egui;

// These types are defined in pane_view.rs
use crate::ui::pane_view::{WindowContext, ViewActions};
use crate::ui::pane::AppWindow;
use crate::config::CredentialType;
use crate::ui::tokens::*;
use crate::ui::widgets;

/// Render keychain view for this window
pub fn render_keychain_view(
    window: &mut AppWindow,
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    cx: &mut WindowContext,
) -> ViewActions {
    // Top navigation bar
    egui::TopBottomPanel::top("keychain_nav_bar")
        .frame(egui::Frame {
            fill: cx.theme.bg_secondary,
            inner_margin: egui::Margin::symmetric(8.0, 8.0),
            stroke: egui::Stroke::NONE,
            ..Default::default()
        })
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(cx.language.t("keychain"))
                    .color(cx.theme.fg_dim)
                    .size(FONT_BASE)
                    .strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(widgets::text_button(cx.language.t("new_credential"), cx.theme.accent)).clicked() {
                        window.credential_dialog.open_new();
                    }
                });
            });
        });

    egui::CentralPanel::default()
        .frame(egui::Frame {
            fill: cx.theme.bg_primary,
            ..Default::default()
        })
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("keychain_page_scroll")
                .show(ui, |ui| {
                    ui.add_space(SPACE_MD);

                    if cx.credentials.is_empty() {
                        ui.add_space(SPACE_2XL);
                        ui.vertical_centered(|ui| {
                            ui.label(egui::RichText::new("\u{1f511}").size(SPACE_2XL).color(cx.theme.fg_dim));
                            ui.add_space(SPACE_MD);
                            ui.label(egui::RichText::new(cx.language.t("keychain_empty")).color(cx.theme.fg_dim).size(FONT_BASE));
                        });
                    } else {
                        // Section header
                        ui.horizontal(|ui| {
                            ui.add_space(SPACE_XL);
                            ui.label(egui::RichText::new(cx.language.t("credentials_section"))
                                .color(cx.theme.fg_dim)
                                .size(FONT_XS)
                                .strong());
                        });
                        ui.add_space(SPACE_XS);

                        // Credential rows
                        for cred in cx.credentials.iter() {
                            let bound_hosts: Vec<String> = cx.hosts.iter()
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
                                        cx.language.t("credential_password").to_string()
                                    } else {
                                        format!("{}: {}", cx.language.t("username"), username)
                                    }
                                }
                                CredentialType::SshKey { key_path, .. } => {
                                    if key_path.is_empty() {
                                        cx.language.t("ssh_key").to_string()
                                    } else {
                                        format!("{}: {}", cx.language.t("key_source_path"), key_path)
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

                            if hovered {
                                ui.painter().rect_filled(rect, 0.0, cx.theme.hover_bg);
                            }

                            ui.painter().text(
                                egui::pos2(rect.min.x + 24.0, rect.min.y + 18.0),
                                egui::Align2::LEFT_CENTER,
                                "\u{1f511}",
                                egui::FontId::proportional(12.0),
                                cx.theme.accent,
                            );

                            ui.painter().text(
                                egui::pos2(rect.min.x + 46.0, rect.min.y + 18.0),
                                egui::Align2::LEFT_CENTER,
                                &cred.name,
                                egui::FontId::proportional(13.0),
                                cx.theme.fg_primary,
                            );

                            ui.painter().text(
                                egui::pos2(rect.min.x + 46.0, rect.min.y + 34.0),
                                egui::Align2::LEFT_CENTER,
                                &subtitle,
                                egui::FontId::proportional(10.0),
                                cx.theme.fg_dim,
                            );

                            let visible_right = ui.clip_rect().max.x;
                            let mut badge_x = visible_right;

                            // Type badge
                            let type_text = cx.language.t(type_key).to_string();
                            let type_galley = ui.painter().layout_no_wrap(
                                type_text,
                                egui::FontId::proportional(FONT_XS),
                                cx.theme.fg_dim,
                            );
                            let type_w = type_galley.size().x + SPACE_LG;
                            let type_h = 18.0;
                            badge_x -= type_w + SPACE_SM;
                            let type_rect = egui::Rect::from_center_size(
                                egui::pos2(badge_x + type_w / 2.0, rect.center().y),
                                egui::vec2(type_w, type_h),
                            );
                            ui.painter().rect(
                                type_rect, RADIUS_SM, egui::Color32::TRANSPARENT,
                                egui::Stroke::new(1.0, cx.theme.border),
                            );
                            ui.painter().galley(
                                egui::pos2(type_rect.center().x - type_galley.size().x / 2.0, type_rect.center().y - type_galley.size().y / 2.0),
                                type_galley, cx.theme.fg_dim,
                            );

                            // Binding count badge
                            if binding_count > 0 {
                                let badge_text = format!("{} {}", binding_count, cx.language.t("hosts_bound"));
                                let badge_galley = ui.painter().layout_no_wrap(
                                    badge_text,
                                    egui::FontId::proportional(FONT_XS),
                                    cx.theme.accent,
                                );
                                let badge_w = badge_galley.size().x + SPACE_MD;
                                badge_x -= badge_w + SPACE_SM;
                                let badge_rect = egui::Rect::from_center_size(
                                    egui::pos2(badge_x + badge_w / 2.0, rect.center().y),
                                    egui::vec2(badge_w, type_h),
                                );
                                ui.painter().rect_filled(badge_rect, RADIUS_SM, cx.theme.badge_bg);
                                ui.painter().galley(
                                    egui::pos2(badge_rect.center().x - badge_galley.size().x / 2.0, badge_rect.center().y - badge_galley.size().y / 2.0),
                                    badge_galley, cx.theme.accent,
                                );
                            }

                            if resp.clicked() {
                                window.credential_dialog.open_edit(cred);
                            }
                        }

                        ui.add_space(40.0);
                    }
                });
        });

    // Delete confirmation dialog
    if let Some(delete_id) = &window.credential_dialog.confirm_delete.clone() {
        let cred_name = cx.credentials.iter()
            .find(|c| c.id == *delete_id)
            .map(|c| c.name.clone())
            .unwrap_or_default();
        let mut open = true;
        egui::Window::new("delete_credential")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .fixed_size([340.0, 0.0])
            .title_bar(false)
            .frame(widgets::dialog_frame(cx.theme))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("\u{26A0}").size(18.0).color(cx.theme.red));
                    ui.add_space(SPACE_XS);
                    ui.label(egui::RichText::new(cx.language.t("delete_credential")).size(15.0).color(cx.theme.fg_primary).strong());
                });
                ui.add_space(10.0);
                ui.label(egui::RichText::new(cx.language.tf("delete_confirm", &cred_name)).color(cx.theme.fg_primary).size(FONT_BASE));
                ui.add_space(SPACE_XS);
                ui.label(egui::RichText::new(cx.language.t("confirm_delete")).color(cx.theme.fg_dim).size(FONT_SM));
                ui.add_space(SPACE_LG);

                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(widgets::danger_button(cx.language.t("delete"), cx.theme)).clicked() {
                            cx.credentials.retain(|c| c.id != *delete_id);
                            window.credential_dialog.confirm_delete = None;
                        }
                        if ui.add(widgets::secondary_button(cx.language.t("cancel"), cx.theme)).clicked() {
                            window.credential_dialog.confirm_delete = None;
                        }
                    });
                });
            });

        if !open {
            window.credential_dialog.confirm_delete = None;
        }
    }

    ViewActions::default()
}

/// Render the add/edit credential drawer (shadcn/ui style)
pub fn render_credential_drawer(window: &mut AppWindow, ctx: &egui::Context, cx: &mut WindowContext) {
    if !window.credential_dialog.open {
        return;
    }

    let is_editing = window.credential_dialog.edit_id.is_some();

    egui::SidePanel::right("credential_drawer")
        .default_width(400.0)
        .frame(egui::Frame {
            fill: cx.theme.bg_elevated,
            inner_margin: egui::Margin::ZERO,
            ..Default::default()
        })
        .show(ctx, |ui| {
            // Header
            egui::TopBottomPanel::top("credential_drawer_header")
                .exact_height(56.0)
                .frame(egui::Frame {
                    fill: cx.theme.bg_elevated,
                    inner_margin: egui::Margin { left: 24.0, right: 16.0, top: 16.0, bottom: 16.0 },
                    ..Default::default()
                })
                .show_inside(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(
                            if is_editing { cx.language.t("edit_credential") } else { cx.language.t("new_credential") }
                        ).size(16.0).strong().color(cx.theme.fg_primary));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            // Close button
                            if ui.add(
                                egui::Button::new(egui::RichText::new("×").size(20.0).color(cx.theme.fg_dim))
                                    .frame(false)
                            ).clicked() {
                                window.credential_dialog.open = false;
                                window.credential_dialog.edit_id = None;
                            }
                            // Delete button (edit mode only)
                            if is_editing {
                                if ui.add(
                                    egui::Button::new(egui::RichText::new("\u{1F5D1}").size(FONT_BASE))
                                        .frame(false)
                                ).on_hover_text(cx.language.t("delete"))
                                .clicked() {
                                    window.credential_dialog.confirm_delete = window.credential_dialog.edit_id.clone();
                                    window.credential_dialog.open = false;
                                }
                            }
                        });
                    });
                });

            // Divider
            ui.add_space(16.0);
            ui.add(egui::Separator::default().spacing(0.0));
            ui.add_space(24.0);

            // Content
            egui::ScrollArea::vertical()
                .id_salt("credential_drawer_scroll")
                .show(ui, |ui| {
                    egui::Frame::none()
                        .inner_margin(egui::Margin::symmetric(widgets::FORM_LEFT_MARGIN, 0.0))
                        .show(ui, |ui| {
                            // Credential type selector
                            ui.horizontal(|ui| {
                                widgets::form_label(ui, cx.language.t("credential_type"), true, cx.theme);
                                ui.add_space(8.0);
                                let selected_text = match window.credential_dialog.cred_type {
                                    crate::ui::types::dialogs::CredentialTypeChoice::Password => cx.language.t("credential_password"),
                                    crate::ui::types::dialogs::CredentialTypeChoice::SshKey => cx.language.t("credential_private_key"),
                                };
                                egui::ComboBox::from_id_salt("cred_type")
                                    .selected_text(egui::RichText::new(selected_text).size(widgets::FONT_SIZE_INPUT).color(cx.theme.fg_primary))
                                    .width(ui.available_width())
                                    .show_ui(ui, |ui| {
                                        widgets::style_dropdown(ui, cx.theme);
                                        if ui.selectable_label(
                                            matches!(window.credential_dialog.cred_type, crate::ui::types::dialogs::CredentialTypeChoice::Password),
                                            cx.language.t("credential_password")
                                        ).clicked() {
                                            window.credential_dialog.cred_type = crate::ui::types::dialogs::CredentialTypeChoice::Password;
                                            window.credential_dialog.key_path.clear();
                                            window.credential_dialog.key_content.clear();
                                            window.credential_dialog.key_passphrase.clear();
                                        }
                                        if ui.selectable_label(
                                            matches!(window.credential_dialog.cred_type, crate::ui::types::dialogs::CredentialTypeChoice::SshKey),
                                            cx.language.t("credential_private_key")
                                        ).clicked() {
                                            window.credential_dialog.cred_type = crate::ui::types::dialogs::CredentialTypeChoice::SshKey;
                                            window.credential_dialog.password.clear();
                                        }
                                    });
                            });
                            ui.add_space(widgets::SPACING_FIELD);

                            // Name field
                            widgets::form_field(ui, cx.language.t("name"), true,
                                &mut window.credential_dialog.name,
                                cx.language.t("name"), cx.theme);
                            ui.add_space(widgets::SPACING_FIELD);

                            // Password type fields
                            if window.credential_dialog.cred_type == crate::ui::types::dialogs::CredentialTypeChoice::Password {
                                // Username + Password in same row
                                widgets::form_field_2col_mixed(
                                    ui,
                                    cx.language.t("username"), true,
                                    &mut window.credential_dialog.username,
                                    cx.language.t("username_hint"), 125.0,
                                    cx.language.t("password"), true,
                                    &mut window.credential_dialog.password,
                                    cx.language.t("password_hint"), 125.0,
                                    cx.theme
                                );
                            }

                            // SSH key fields
                            if window.credential_dialog.cred_type == crate::ui::types::dialogs::CredentialTypeChoice::SshKey {
                                // Key source selector
                                ui.horizontal(|ui| {
                                    widgets::form_label(ui, cx.language.t("key_source"), true, cx.theme);
                                    ui.add_space(8.0);
                                    let source_text = match window.credential_dialog.key_source {
                                        crate::ui::types::dialogs::KeySourceChoice::LocalFile => cx.language.t("key_source_path"),
                                        crate::ui::types::dialogs::KeySourceChoice::ImportContent => cx.language.t("import_key"),
                                    };
                                    egui::ComboBox::from_id_salt("key_source")
                                        .selected_text(egui::RichText::new(source_text).size(widgets::FONT_SIZE_INPUT).color(cx.theme.fg_primary))
                                        .width(ui.available_width())
                                        .show_ui(ui, |ui| {
                                            widgets::style_dropdown(ui, cx.theme);
                                            if ui.selectable_label(
                                                matches!(window.credential_dialog.key_source, crate::ui::types::dialogs::KeySourceChoice::LocalFile),
                                                cx.language.t("key_source_path")
                                            ).clicked() {
                                                window.credential_dialog.key_source = crate::ui::types::dialogs::KeySourceChoice::LocalFile;
                                            }
                                            if ui.selectable_label(
                                                matches!(window.credential_dialog.key_source, crate::ui::types::dialogs::KeySourceChoice::ImportContent),
                                                cx.language.t("import_key")
                                            ).clicked() {
                                                window.credential_dialog.key_source = crate::ui::types::dialogs::KeySourceChoice::ImportContent;
                                            }
                                        });
                                });
                                ui.add_space(widgets::SPACING_FIELD);

                                if window.credential_dialog.key_source == crate::ui::types::dialogs::KeySourceChoice::LocalFile {
                                    // Key path
                                    widgets::form_field(ui, cx.language.t("key_path"), true,
                                        &mut window.credential_dialog.key_path,
                                        cx.language.t("key_path_hint"), cx.theme);
                                } else {
                                    // Key content - multiline
                                    widgets::form_field_textarea(ui, cx.language.t("key_content"), true,
                                        &mut window.credential_dialog.key_content,
                                        cx.language.t("key_content_hint"), 80.0, cx.theme);
                                }

                                ui.add_space(widgets::SPACING_FIELD);

                                // Key passphrase (optional)
                                widgets::form_field_password(ui, cx.language.t("key_passphrase"), false,
                                    &mut window.credential_dialog.key_passphrase,
                                    cx.language.t("key_passphrase_hint"), cx.theme);
                            }

                            // Error message
                            if !window.credential_dialog.error.is_empty() {
                                ui.add_space(widgets::SPACING_FIELD);
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("\u{26A0}").size(14.0).color(cx.theme.red));
                                    ui.add_space(4.0);
                                    ui.label(egui::RichText::new(&window.credential_dialog.error).color(cx.theme.red).size(12.0));
                                });
                            }

                        });
                });

            // Footer (fixed at bottom)
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.add_space(widgets::FORM_LEFT_MARGIN);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(16.0);
                    let can_save = !window.credential_dialog.name.trim().is_empty();
                    let has_auth = match window.credential_dialog.cred_type {
                        crate::ui::types::dialogs::CredentialTypeChoice::Password => {
                            !window.credential_dialog.username.trim().is_empty()
                                && !window.credential_dialog.password.is_empty()
                        }
                        crate::ui::types::dialogs::CredentialTypeChoice::SshKey => {
                            if window.credential_dialog.key_source == crate::ui::types::dialogs::KeySourceChoice::LocalFile {
                                !window.credential_dialog.key_path.trim().is_empty()
                            } else {
                                !window.credential_dialog.key_content.trim().is_empty()
                            }
                        }
                    };

                    if ui.add(widgets::primary_button(cx.language.t("save"), cx.theme)).clicked() && can_save && has_auth {
                        use crate::config::{Credential, store_credential_secret};
                        use uuid::Uuid;

                        if let Some(edit_id) = &window.credential_dialog.edit_id {
                            if let Some(cred) = cx.credentials.iter_mut().find(|c| c.id == *edit_id) {
                                cred.name = window.credential_dialog.name.trim().to_string();
                                match window.credential_dialog.cred_type {
                                    crate::ui::types::dialogs::CredentialTypeChoice::Password => {
                                        cred.credential_type = CredentialType::Password {
                                            username: window.credential_dialog.username.trim().to_string(),
                                        };
                                        let _ = store_credential_secret(&cred.id, &cred.name, "password", &window.credential_dialog.password);
                                    }
                                    crate::ui::types::dialogs::CredentialTypeChoice::SshKey => {
                                        let (key_path, key_in_keychain) = if window.credential_dialog.key_source == crate::ui::types::dialogs::KeySourceChoice::LocalFile {
                                            (window.credential_dialog.key_path.trim().to_string(), false)
                                        } else {
                                            let _ = store_credential_secret(&cred.id, &cred.name, "ssh_key", &window.credential_dialog.key_content);
                                            (String::new(), true)
                                        };
                                        cred.credential_type = CredentialType::SshKey {
                                            key_path,
                                            key_in_keychain,
                                            has_passphrase: !window.credential_dialog.key_passphrase.is_empty(),
                                        };
                                        if !window.credential_dialog.key_passphrase.is_empty() {
                                            let _ = store_credential_secret(&cred.id, &cred.name, "passphrase", &window.credential_dialog.key_passphrase);
                                        }
                                    }
                                }
                            }
                        } else {
                            let id = Uuid::new_v4().to_string();
                            let name = window.credential_dialog.name.trim().to_string();
                            match window.credential_dialog.cred_type {
                                crate::ui::types::dialogs::CredentialTypeChoice::Password => {
                                    let credential_type = CredentialType::Password {
                                        username: window.credential_dialog.username.trim().to_string(),
                                    };
                                    let cred = Credential {
                                        id: id.clone(),
                                        name: name.clone(),
                                        credential_type,
                                        created_at: std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs(),
                                    };
                                    cx.credentials.push(cred);
                                    let _ = store_credential_secret(&id, &name, "password", &window.credential_dialog.password);
                                }
                                crate::ui::types::dialogs::CredentialTypeChoice::SshKey => {
                                    let (key_path, key_in_keychain) = if window.credential_dialog.key_source == crate::ui::types::dialogs::KeySourceChoice::LocalFile {
                                        (window.credential_dialog.key_path.trim().to_string(), false)
                                    } else {
                                        let _ = store_credential_secret(&id, &name, "ssh_key", &window.credential_dialog.key_content);
                                        (String::new(), true)
                                    };
                                    let credential_type = CredentialType::SshKey {
                                        key_path,
                                        key_in_keychain,
                                        has_passphrase: !window.credential_dialog.key_passphrase.is_empty(),
                                    };
                                    let cred = Credential {
                                        id: id.clone(),
                                        name: name.clone(),
                                        credential_type,
                                        created_at: std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs(),
                                    };
                                    cx.credentials.push(cred);
                                    if !window.credential_dialog.key_passphrase.is_empty() {
                                        let _ = store_credential_secret(&id, &name, "passphrase", &window.credential_dialog.key_passphrase);
                                    }
                                }
                            }
                        }
                        window.credential_dialog.open = false;
                        window.credential_dialog.edit_id = None;
                    }
                    ui.add_space(8.0);
                    if ui.add(widgets::secondary_button(cx.language.t("cancel"), cx.theme)).clicked() {
                        window.credential_dialog.open = false;
                        window.credential_dialog.edit_id = None;
                    }
                });
            });
        });
}
