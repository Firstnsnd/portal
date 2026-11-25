//! # Connection History View
//!
//! Dedicated page for the connection history (moved off the hosts page —
//! the record list can grow long and doesn't belong next to the host
//! entries). Shows every stored record (append_history caps at 200) with a
//! search box and day grouping. Rows are click-to-connect via ViewActions;
//! the app layer routes protected hosts through the owner gate.

use eframe::egui;

use crate::config::HostEntry;
use crate::ui::pane::AppWindow;
use crate::ui::pane_view::{ViewActions, WindowContext};
use crate::ui::tokens::*;
use crate::ui::widgets;

/// Local datetime for a history record's unix timestamp.
fn record_local(ts: u64) -> Option<chrono::DateTime<chrono::Local>> {
    chrono::DateTime::from_timestamp(ts as i64, 0).map(chrono::DateTime::<chrono::Local>::from)
}

/// Render the connection history page. Draws its own top bar + central
/// panel from `ctx` (like the hosts page), so no outer `ui` is needed.
pub fn render_history_view(
    window: &mut AppWindow,
    ctx: &egui::Context,
    cx: &mut WindowContext,
) -> ViewActions {
    let mut actions = ViewActions::default();
    let mut connect_target: Option<HostEntry> = None;

    // Top navigation bar
    egui::TopBottomPanel::top("history_nav_bar")
        .frame(egui::Frame {
            fill: cx.theme.bg_secondary,
            inner_margin: egui::Margin::symmetric(8.0, 8.0),
            stroke: egui::Stroke::NONE,
            ..Default::default()
        })
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(cx.language.t("connection_history"))
                        .color(cx.theme.fg_dim)
                        .size(FONT_BASE)
                        .strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(widgets::text_button(
                            cx.language.t("clear_history"),
                            cx.theme.fg_dim,
                        ))
                        .clicked()
                    {
                        actions.clear_history = true;
                    }
                });
            });
            ui.add_space(4.0);
        });

    egui::CentralPanel::default()
        .frame(egui::Frame {
            fill: cx.theme.bg_primary,
            ..Default::default()
        })
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("history_page_scroll")
                .show(ui, |ui| {
                    ui.add_space(SPACE_SM);

                    // Search box + record count
                    ui.horizontal(|ui| {
                        ui.add_space(SPACE_XL);
                        let search_w = (ui.available_width() - 160.0).max(120.0);
                        widgets::fixed_input(
                            ui,
                            &mut window.history_view_state.search,
                            cx.language.t("search_placeholder"),
                            search_w,
                            cx.theme,
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(SPACE_XL);
                            ui.label(
                                egui::RichText::new(
                                    cx.language.tf(
                                        "history_count",
                                        &cx.connection_history.len().to_string(),
                                    ),
                                )
                                .color(cx.theme.fg_dim)
                                .size(11.0),
                            );
                        });
                    });
                    ui.add_space(SPACE_SM);

                    let query = window.history_view_state.search.trim().to_lowercase();
                    let records: Vec<&crate::config::ConnectionRecord> = cx
                        .connection_history
                        .iter()
                        .rev()
                        .filter(|r| {
                            query.is_empty()
                                || r.host_name.to_lowercase().contains(&query)
                                || r.host.to_lowercase().contains(&query)
                                || r.username.to_lowercase().contains(&query)
                        })
                        .collect();

                    if records.is_empty() {
                        ui.add_space(SPACE_XL);
                        ui.horizontal(|ui| {
                            ui.add_space(SPACE_XL);
                            let msg = if query.is_empty() {
                                cx.language.t("history_empty")
                            } else {
                                cx.language.t("no_matches")
                            };
                            ui.label(egui::RichText::new(msg).color(cx.theme.fg_dim).size(12.0));
                        });
                    } else {
                        let today = chrono::Local::now().date_naive();
                        let yesterday = today - chrono::Duration::days(1);
                        let mut prev_date: Option<chrono::NaiveDate> = None;
                        for record in &records {
                            // Day header whenever the group changes (records
                            // are iterated newest-first).
                            let date = record_local(record.timestamp)
                                .map(|dt| dt.date_naive())
                                .unwrap_or(today);
                            if prev_date != Some(date) {
                                let label = if date == today {
                                    cx.language.t("history_today").to_string()
                                } else if date == yesterday {
                                    cx.language.t("history_yesterday").to_string()
                                } else {
                                    date.format("%Y-%m-%d").to_string()
                                };
                                ui.add_space(SPACE_MD);
                                ui.horizontal(|ui| {
                                    ui.add_space(SPACE_XL);
                                    ui.label(widgets::section_header(&label, cx.theme));
                                });
                                ui.add_space(SPACE_XS);
                                prev_date = Some(date);
                            }

                            let row_h = 44.0;
                            let width = ui.available_width();
                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(width, row_h),
                                egui::Sense::click(),
                            );
                            if resp.hovered() {
                                ui.painter().rect_filled(
                                    egui::Rect::from_min_max(
                                        egui::pos2(rect.min.x, rect.max.y - 1.0),
                                        rect.max,
                                    ),
                                    0.0,
                                    cx.theme.hover_shadow,
                                );
                                ui.painter().rect_filled(rect, 0.0, cx.theme.hover_bg);
                            }
                            // Success / failure dot
                            ui.painter().circle_filled(
                                egui::pos2(rect.min.x + 24.0, rect.min.y + 22.0),
                                3.0,
                                if record.success {
                                    cx.theme.green
                                } else {
                                    cx.theme.red
                                },
                            );
                            let display_name = if record.host_name.is_empty() {
                                format!("{}:{}", record.host, record.port)
                            } else {
                                record.host_name.clone()
                            };
                            ui.painter().text(
                                egui::pos2(rect.min.x + 46.0, rect.min.y + 14.0),
                                egui::Align2::LEFT_CENTER,
                                &display_name,
                                egui::FontId::proportional(13.0),
                                cx.theme.fg_primary,
                            );
                            let detail =
                                format!("{}@{}:{}", record.username, record.host, record.port);
                            ui.painter().text(
                                egui::pos2(rect.min.x + 46.0, rect.min.y + 30.0),
                                egui::Align2::LEFT_CENTER,
                                &detail,
                                egui::FontId::proportional(10.0),
                                cx.theme.fg_dim,
                            );
                            let time_text = record_local(record.timestamp)
                                .map(|dt| dt.format("%H:%M").to_string())
                                .unwrap_or_default();
                            let time_x = ui.clip_rect().max.x - 24.0;
                            ui.painter().text(
                                egui::pos2(time_x, rect.min.y + 22.0),
                                egui::Align2::RIGHT_CENTER,
                                &time_text,
                                egui::FontId::proportional(10.0),
                                cx.theme.fg_dim,
                            );

                            if resp.clicked() {
                                // Prefer the saved host entry (keeps its
                                // credential/protected settings); fall back
                                // to an ad-hoc SSH target for records whose
                                // host was deleted or was never saved.
                                connect_target = Some(
                                    cx.hosts
                                        .iter()
                                        .find(|h| {
                                            !h.is_local
                                                && h.host == record.host
                                                && h.port == record.port
                                                && h.username == record.username
                                        })
                                        .cloned()
                                        .unwrap_or_else(|| {
                                            HostEntry::new_ssh(
                                                record.host_name.clone(),
                                                record.host.clone(),
                                                record.port,
                                                record.username.clone(),
                                                String::new(),
                                                None,
                                                Vec::new(),
                                            )
                                        }),
                                );
                            }
                        }
                    }

                    ui.add_space(SPACE_XL);
                });
        });

    if let Some(host) = connect_target {
        actions.connect_ssh = Some(host);
    }

    actions
}
