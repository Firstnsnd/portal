//! # Metrics Drawer
//!
//! Right-side drawer showing live system metrics (CPU, memory, network,
//! load) for the focused terminal session's target.
//!
//! Sparkline rendering follows Grafana conventions: auto-scaling y-axis
//! (not pinned to zero), translucent area fill, thin line, and a dot
//! on the latest data point.

use std::sync::{Arc, Mutex};

use eframe::egui;
use eframe::egui::epaint::PathStroke;

use crate::terminal::metrics::MetricsSnapshot;
use crate::ui::pane::AppWindow;
use crate::ui::pane_view::WindowContext;
use crate::ui::theme::ThemeColors;
use crate::ui::types::session::{SessionBackend, TerminalSession};
use crate::ssh::port_forward::ForwardState;

// ── Reusable bodies ───────────────────────────────────────────────

/// Draw the 4 metric blocks + sparklines (no panel shell). Used inside the
/// tools drawer's Metrics section.
pub fn render_metrics_body(ui: &mut egui::Ui, session: &TerminalSession, theme: &ThemeColors) {
    let snap_arc = match session_metrics_snapshot(session) {
        Some(a) => a,
        None => return,
    };
    let snap = snap_arc.lock().unwrap();
    if snap.latest.is_none() {
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Waiting for data\u{2026}").color(theme.fg_dim).size(12.0));
        return;
    }
    let m = snap.latest.as_ref().unwrap();
    let gb = 1_073_741_824.0;
    row(ui, theme, "CPU", &format!("{:.0}%", m.cpu_percent), &snap.cpu_history);
    row(ui, theme, "Memory",
        &format!("{:.2} / {:.2} GB", m.mem_used_bytes as f64 / gb, m.mem_total_bytes as f64 / gb),
        &snap.mem_history);
    network_row(ui, theme,
        &snap.net_rx_history, &snap.net_tx_history,
        &format!("\u{2193}{}", rate(m.net_rx_bytes_per_sec)),
        &format!("\u{2191}{}", rate(m.net_tx_bytes_per_sec)));
    row(ui, theme, "Load 1 min", &format!("{:.2}", m.load_1), &snap.load_history);
}

// ── Tools drawer (broadcast + snippets + metrics in one panel) ────

pub fn render_tools_drawer(window: &mut AppWindow, ctx: &egui::Context, cx: &mut WindowContext) {
    let theme = cx.theme;
    let language = cx.language;
    egui::SidePanel::right("tools_drawer")
        .default_width(300.0)
        .resizable(false)
        .frame(egui::Frame {
            fill: theme.bg_elevated,
            inner_margin: egui::Margin::ZERO,
            rounding: egui::Rounding::ZERO,
            shadow: egui::epaint::Shadow {
                offset: egui::vec2(-4.0, 0.0), blur: 20.0, spread: 0.0,
                color: egui::Color32::from_black_alpha(20),
            },
            stroke: egui::Stroke::NONE,
            ..Default::default()
        })
        .show(ctx, |ui| {
            let active = window.active_tab;

            // Header: title + close
            egui::Frame::none()
                .inner_margin(egui::Margin { left: 14.0, right: 8.0, top: 12.0, bottom: 8.0 })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(language.t("tools")).color(theme.fg_primary).size(13.0).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(egui::Button::new(
                                egui::RichText::new("\u{2715}").color(theme.fg_dim).size(14.0)).frame(false)).clicked() {
                                if let Some(t) = window.tabs.get_mut(active) { t.tools_drawer_open = false; }
                            }
                        });
                    });
                });

            egui::Frame::none().inner_margin(egui::Margin::symmetric(14.0, 4.0))
                .show(ui, |ui| {
                    // Broadcast toggle
                    let broadcasting = window.tabs.get(active).map(|t| t.broadcast_enabled).unwrap_or(false);
                    let b_glyph = if broadcasting { "\u{25C9}" } else { "\u{25CB}" };
                    let b_color = if broadcasting { theme.accent } else { theme.fg_dim };
                    let b_label = format!("{}  {}  \u{2318}\u{21E7}I", b_glyph, language.t("broadcast"));
                    if ui.add(egui::Button::new(
                        egui::RichText::new(&b_label).color(b_color).size(13.0)).frame(false)).clicked() {
                        if let Some(t) = window.tabs.get_mut(active) { t.broadcast_enabled = !t.broadcast_enabled; }
                    }
                    ui.add_space(6.0);

                    // Tab buttons: [Metrics] [Snippets] [Tunnels]
                    let tab = window.tabs.get(active).map(|t| t.tools_tab).unwrap_or(0);
                    ui.horizontal(|ui| {
                        let tabs: [(u8, &str); 3] = [
                            (0, language.t("metrics")),
                            (1, language.t("snippets")),
                            (2, language.t("tunnels")),
                        ];
                        for (tid, label) in &tabs {
                            let is_selected = tab == *tid;
                            let color = if is_selected { theme.accent } else { theme.fg_dim };
                            let text = if is_selected {
                                egui::RichText::new(*label).color(color).size(13.0).strong()
                            } else {
                                egui::RichText::new(*label).color(color).size(13.0)
                            };
                            if ui.add(egui::Button::new(text).frame(false)).clicked() {
                                if let Some(t) = window.tabs.get_mut(active) { t.tools_tab = *tid; }
                            }
                            ui.add_space(8.0);
                        }
                    });
                    ui.separator();
                    ui.add_space(4.0);

                    // Content
                    match tab {
                        0 => {
                            let focused = window.tabs.get(active).map(|t| t.focused_session).unwrap_or(0);
                            if let Some(session) = window.tabs.get(active).and_then(|t| t.sessions.get(focused)) {
                                render_metrics_body(ui, session, theme);
                            } else {
                                ui.label(egui::RichText::new("No active session").color(theme.fg_dim).size(12.0));
                            }
                        }
                        1 => {
                            let mut selected: Option<String> = None;
                            crate::ui::views::snippets_view::render_snippet_list(
                                ui, &cx.snippets, theme, &language,
                                |cmd| { selected = Some(cmd.to_string()); });
                            if let Some(cmd) = selected {
                                if let Some(t) = window.tabs.get_mut(active) { t.pending_snippet = Some(cmd); }
                            }
                        }
                        2 => {
                            render_tunnels_tab(ui, window, active, cx);
                        }
                        _ => {}
                    }
                });
        });
}

// ── Tunnels tab ───────────────────────────────────────────────────

fn render_tunnels_tab(ui: &mut egui::Ui, window: &mut AppWindow, active: usize, cx: &WindowContext) {
    let sess_idx = match window.tabs.get(active).map(|t| t.focused_session) {
        Some(i) => i,
        None => return,
    };
    let session = match window.tabs.get(active).and_then(|t| t.sessions.get(sess_idx)) {
        Some(s) => s,
        None => return,
    };
    let session_host = match session.ssh_host.as_ref() {
        Some(h) => h.clone(),
        None => {
            ui.label(egui::RichText::new("No SSH session").color(egui::Color32::GRAY).size(12.0));
            return;
        }
    };
    let ssh = match session.session.as_ref() {
        Some(SessionBackend::Ssh(s)) => s,
        _ => {
            ui.label(egui::RichText::new("Not an SSH session").color(egui::Color32::GRAY).size(12.0));
            return;
        }
    };

    // Use the latest config from `cx.hosts` (not the session snapshot) so
    // rules added after the connection was established are visible.
    let configured = cx.hosts.iter()
        .find(|h| h.host == session_host.host && h.port == session_host.port)
        .map(|h| &h.port_forwards)
        .unwrap_or(&session_host.port_forwards);
    if configured.is_empty() {
        ui.label(egui::RichText::new("No tunnels configured for this host.\nAdd port forward rules in Host settings.").color(egui::Color32::GRAY).size(12.0));
        return;
    }

    // Snapshot runtime state so we can precompute the state for each config rule.
    let runtime = ssh.port_forwards.lock().unwrap();
    let find_state = |cfg: &crate::config::PortForwardConfig| -> ForwardState {
        runtime.iter().find(|pf| &pf.config == cfg)
            .and_then(|pf| pf.state.lock().ok().map(|g| g.clone()))
            .unwrap_or(ForwardState::Stopped) // configured but not started
    };

    for cfg in configured {
        let state = find_state(cfg);
        let kind = match cfg.kind {
            crate::config::ForwardKind::Local => "L",
            crate::config::ForwardKind::Remote => "R",
        };
        let detail = match cfg.kind {
            crate::config::ForwardKind::Local => format!("{}:{} → {}:{}", cfg.local_host, cfg.local_port, cfg.remote_host, cfg.remote_port),
            crate::config::ForwardKind::Remote => format!("{}:{} → {}:{}", cfg.remote_host, cfg.remote_port, cfg.local_host, cfg.local_port),
        };
        let (status, status_color) = match &state {
            ForwardState::Active => ("Active", egui::Color32::GREEN),
            ForwardState::Starting => ("Starting…", egui::Color32::from_rgb(100, 149, 237)),
            ForwardState::Stopped => ("Inactive", egui::Color32::GRAY),
            ForwardState::Error(_) => ("Error", egui::Color32::RED),
            ForwardState::Conflict(_) => ("Conflict", egui::Color32::RED),
        };

        // Row 1: type + detail (mono, left-aligned)
        ui.label(egui::RichText::new(format!("{}  {}", kind, detail)).size(12.0).monospace());
        // Row 2: status dot + label + button (right-aligned group)
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("\u{25CF}  {}", status)).size(11.0).color(status_color));
            ui.add_space(8.0);
            match &state {
                ForwardState::Active => {
                    if ui.add(egui::Button::new(egui::RichText::new("Stop").size(12.0)).small()).clicked() {
                        ssh.stop_port_forward(cfg.clone());
                    }
                }
                ForwardState::Stopped | ForwardState::Error(_) | ForwardState::Conflict(_) => {
                    if ui.add(egui::Button::new(egui::RichText::new("Start").size(12.0)).small()).clicked() {
                        ssh.start_port_forward(cfg.clone());
                    }
                }
                ForwardState::Starting => {}
            }
        });
        ui.add_space(4.0);
    }
}

// ── Metric row ─────────────────────────────────────────────────────

fn row(
    ui: &mut egui::Ui,
    theme: &ThemeColors,
    label: &str,
    value: &str,
    history: &std::collections::VecDeque<f32>,
) {
    // Label   ··········   value
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).color(theme.fg_dim).size(11.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(value)
                    .color(theme.fg_primary).size(14.0).strong()
                    .family(egui::FontFamily::Monospace),
            );
        });
    });
    ui.add_space(6.0);

    // Sparkline rect
    let rect = egui::Rect::from_min_size(
        ui.cursor().min,
        egui::vec2(ui.available_width(), 46.0),
    );
    let (_, resp) = ui.allocate_at_least(rect.size(), egui::Sense::hover());
    sparkline(ui, rect, history, theme.accent, &resp);
    ui.add_space(10.0);
}

// ── Network row (dual series: rx ↓ + tx ↑) ────────────────────

fn network_row(
    ui: &mut egui::Ui,
    theme: &ThemeColors,
    rx: &std::collections::VecDeque<f32>,
    tx: &std::collections::VecDeque<f32>,
    rx_label: &str,
    tx_label: &str,
) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Network").color(theme.fg_dim).size(11.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(rx_label).color(theme.accent).size(13.0)
                    .strong().family(egui::FontFamily::Monospace),
            );
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(tx_label).color(theme.green).size(13.0)
                    .strong().family(egui::FontFamily::Monospace),
            );
        });
    });
    ui.add_space(6.0);
    let rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), 46.0));
    let (_, resp) = ui.allocate_at_least(rect.size(), egui::Sense::hover());
    dual_sparkline(ui, rect, rx, tx, theme.accent, theme.green, &resp);
    ui.add_space(10.0);
}

fn dual_sparkline(
    ui: &egui::Ui,
    rect: egui::Rect,
    rx: &std::collections::VecDeque<f32>,
    tx: &std::collections::VecDeque<f32>,
    rx_color: egui::Color32,
    tx_color: egui::Color32,
    resp: &egui::Response,
) {
    let nr = rx.len();
    let nt = tx.len();
    if nr < 2 || nt < 2 { return; }

    let (y_min, y_max) = {
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for &v in rx.iter().chain(tx.iter()) { lo = lo.min(v); hi = hi.max(v); }
        (lo, hi)
    };
    let yr = y_max - y_min;
    let (y0, y1) = if yr < f32::EPSILON { (y_min - 1.0, y_max + 1.0) }
                   else { let p = yr * 0.12; (y_min - p, y_max + p) };
    let dy = (y1 - y0).max(0.001);
    let w = rect.width(); let h = rect.height();

    let pts = |d: &std::collections::VecDeque<f32>| -> Vec<egui::Pos2> {
        let n = d.len();
        let xs = if n > 1 { w / (n - 1) as f32 } else { 0.0 };
        d.iter().enumerate().map(|(i, &v)| {
            egui::pos2(rect.min.x + i as f32 * xs, rect.max.y - h * (v - y0) / dy)
        }).collect()
    };
    let rp = pts(rx);
    let tp = pts(tx);

    let rf = egui::Color32::from_rgba_premultiplied(rx_color.r(), rx_color.g(), rx_color.b(), 30);
    let tf = egui::Color32::from_rgba_premultiplied(tx_color.r(), tx_color.g(), tx_color.b(), 30);

    // fills (tx bottom layer, rx on top)
    for (p, c) in [(&tp, tf), (&rp, rf)] {
        for i in 0..p.len()-1 {
            let (a, b) = (p[i], p[i+1]);
            ui.painter().add(egui::Shape::convex_polygon(
                vec![a, b, egui::pos2(b.x, rect.max.y), egui::pos2(a.x, rect.max.y)],
                c, egui::Stroke::NONE));
        }
    }
    // lines
    ui.painter().add(egui::Shape::line(tp.clone(), PathStroke::new(1.8, tx_color)));
    ui.painter().add(egui::Shape::line(rp.clone(), PathStroke::new(1.8, rx_color)));
    // latest dots
    ui.painter().circle_filled(tp[nt-1], 3.5, tx_color);
    ui.painter().circle_filled(rp[nr-1], 3.5, rx_color);

    // hover crosshair
    if let Some(hp) = resp.hover_pos() {
        let idx = ((hp.x - rect.min.x) / w).clamp(0.0, 1.0).mul_add(nr as f32 - 1.0, 0.0).round() as usize;
        let idx = idx.min(nr - 1);
        let cx = rp[idx].x;
        let dim = egui::Color32::from_rgba_premultiplied(255, 255, 255, 80);
        ui.painter().line_segment([egui::pos2(cx, rect.min.y), egui::pos2(cx, rect.max.y)], PathStroke::new(1.0, dim));
        let label = format!("\u{2193}{}  \u{2191}{}", rate(rx[idx].round() as u64), rate(tx[idx].round() as u64));
        let txt_color = egui::Color32::WHITE;
        let g = ui.fonts(|f| f.layout_no_wrap(label.clone(), egui::FontId::monospace(11.0), txt_color));
        let pad = egui::vec2(6.0, 3.0);
        let mut lbl = egui::Rect::from_center_size(egui::pos2(cx, rect.min.y + g.rect.height()/2.0 + 6.0), g.rect.size()+pad*2.0);
        if lbl.right() > rect.right() { lbl = lbl.translate(egui::vec2(rect.right()-lbl.right()-2.0, 0.0)); }
        if lbl.left()  < rect.left()  { lbl = lbl.translate(egui::vec2(rect.left()-lbl.left()+2.0, 0.0)); }
        ui.painter().rect_filled(lbl, 4.0, egui::Color32::from_black_alpha(200));
        ui.painter().text(lbl.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::monospace(11.0), txt_color);
    }
}

// ── Grafana-style sparkline (hand-drawn) ───────────────────────────

fn sparkline(
    ui: &egui::Ui,
    rect: egui::Rect,
    data: &std::collections::VecDeque<f32>,
    color: egui::Color32,
    resp: &egui::Response,
) {
    let n = data.len();
    if n < 2 {
        return;
    }

    let (y_min, y_max) = data.iter().fold(
        (f32::INFINITY, f32::NEG_INFINITY),
        |(lo, hi), &v| (lo.min(v), hi.max(v)),
    );

    let y_range = y_max - y_min;
    let (y0, y1) = if y_range < f32::EPSILON {
        (y_min - 1.0, y_max + 1.0)
    } else {
        let pad = y_range * 0.12;
        (y_min - pad, y_max + pad)
    };
    let dy = (y1 - y0).max(0.001);

    let w = rect.width();
    let h = rect.height();
    let x_step = w / (n - 1) as f32;

    // Map data → screen positions
    let pt = |i: usize, v: f32| -> egui::Pos2 {
        egui::pos2(rect.min.x + i as f32 * x_step, rect.max.y - h * (v - y0) / dy)
    };
    let pts: Vec<egui::Pos2> = data.iter().enumerate().map(|(i, &v)| pt(i, v)).collect();

    // Fill: one trapezoid per adjacent pair (= always convex, no bug)
    let fill = egui::Color32::from_rgba_premultiplied(color.r(), color.g(), color.b(), 35);
    for i in 0..n - 1 {
        let a = pts[i];
        let b = pts[i + 1];
        let c = egui::pos2(b.x, rect.max.y);
        let d = egui::pos2(a.x, rect.max.y);
        ui.painter()
            .add(egui::Shape::convex_polygon(vec![a, b, c, d], fill, egui::Stroke::NONE));
    }

    // Line
    ui.painter()
        .add(egui::Shape::line(pts.clone(), PathStroke::new(2.0, color)));

    // Latest-value dot
    ui.painter()
        .circle_filled(pts[n - 1], 3.5, color);

    // ── Hover crosshair (Grafana-style) ──
    if let Some(hp) = resp.hover_pos() {
        let idx = ((hp.x - rect.min.x) / w)
            .clamp(0.0, 1.0)
            .mul_add(n as f32 - 1.0, 0.0)
            .round() as usize;
        let idx = idx.min(n - 1);
        let cx = pts[idx].x;
        let cy = pts[idx].y;
        let dim = egui::Color32::from_rgba_premultiplied(color.r(), color.g(), color.b(), 90);

        // vertical crosshair line
        ui.painter()
            .line_segment([egui::pos2(cx, rect.min.y), egui::pos2(cx, rect.max.y)], PathStroke::new(1.0, dim));

        // highlight dot on the data point
        ui.painter().circle_filled(egui::pos2(cx, cy), 5.0, color);

        // tooltip label
        let txt = format!("{:.1}", data[idx]);
        let galley = ui.fonts(|f| f.layout_no_wrap(txt.clone(), egui::FontId::monospace(11.0), color));
        let pad = egui::vec2(6.0, 3.0);
        let mut lbl = egui::Rect::from_center_size(
            egui::pos2(cx, cy - galley.rect.height() / 2.0 - 10.0),
            galley.rect.size() + pad * 2.0,
        );
        if lbl.right() > rect.right() { lbl = lbl.translate(egui::vec2(rect.right() - lbl.right() - 2.0, 0.0)); }
        if lbl.left()  < rect.left()  { lbl = lbl.translate(egui::vec2(rect.left()  - lbl.left()  + 2.0, 0.0)); }
        if lbl.top()   < 0.0          { lbl = lbl.translate(egui::vec2(0.0, -lbl.top() + 2.0)); }
        ui.painter().rect_filled(lbl, 4.0, egui::Color32::from_black_alpha(200));
        ui.painter().text(lbl.center(), egui::Align2::CENTER_CENTER, txt, egui::FontId::monospace(11.0), color);
    }
}

// ── Helpers ────────────────────────────────────────────────────────

fn session_metrics_snapshot(s: &TerminalSession) -> Option<Arc<Mutex<MetricsSnapshot>>> {
    match &s.session {
        Some(crate::ui::types::session::SessionBackend::Local(pty)) => Some(Arc::clone(&pty.metrics)),
        Some(crate::ui::types::session::SessionBackend::Ssh(ssh)) => Some(Arc::clone(&ssh.metrics)),
        None => None,
    }
}

fn rate(bps: u64) -> String {
    if bps >= 1_000_000 { format!("{:.1}MB/s", bps as f64 / 1e6) }
    else if bps >= 1_000 { format!("{:.0}KB/s", bps as f64 / 1e3) }
    else { format!("{}B/s", bps) }
}
