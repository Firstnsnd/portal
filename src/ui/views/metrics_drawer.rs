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
use crate::ui::i18n::Language;
use crate::ui::theme::ThemeColors;
use crate::ui::types::session::TerminalSession;

// ── Public entry point ─────────────────────────────────────────────

pub fn render_metrics_drawer(
    ctx: &egui::Context,
    session: &TerminalSession,
    _language: &Language,
    theme: &ThemeColors,
    open: &mut bool,
) {
    let snap_arc = match session_metrics_snapshot(session) {
        Some(a) => a,
        None => return,
    };
    let snap = snap_arc.lock().unwrap();

    egui::SidePanel::right("metrics_drawer")
        .default_width(270.0)
        .resizable(false)
        .frame(egui::Frame {
            fill: theme.bg_elevated,
            inner_margin: egui::Margin::ZERO,
            rounding: egui::Rounding::ZERO,
            shadow: egui::epaint::Shadow {
                offset: egui::vec2(-4.0, 0.0),
                blur: 20.0,
                spread: 0.0,
                color: egui::Color32::from_black_alpha(20),
            },
            stroke: egui::Stroke::NONE,
            ..Default::default()
        })
        .show(ctx, |ui| {
            render_header(ui, theme, session, open);
            egui::ScrollArea::vertical()
                .id_salt("metrics_scroll")
                .show(ui, |ui| {
                    egui::Frame::none()
                        .inner_margin(egui::Margin {
                            left: 14.0, right: 14.0, top: 10.0, bottom: 14.0,
                        })
                        .show(ui, |ui| {
                            if snap.latest.is_none() {
                                ui.add_space(20.0);
                                ui.label(
                                    egui::RichText::new("Waiting for data\u{2026}")
                                        .color(theme.fg_dim).size(12.0),
                                );
                                return;
                            }
                            let m = snap.latest.as_ref().unwrap();
                            let gb = 1_073_741_824.0;

                            row(ui, theme, "CPU", &format!("{:.0}%", m.cpu_percent),
                                &snap.cpu_history);
                            row(ui, theme, "Memory",
                                &format!("{:.2} / {:.2} GB",
                                    m.mem_used_bytes as f64 / gb, m.mem_total_bytes as f64 / gb),
                                &snap.mem_history);
                            network_row(ui, theme,
                                &snap.net_rx_history, &snap.net_tx_history,
                                &format!("\u{2193}{}", rate(m.net_rx_bytes_per_sec)),
                                &format!("\u{2191}{}", rate(m.net_tx_bytes_per_sec)),
                            );
                            row(ui, theme, "Load 1 min",
                                &format!("{:.2}", m.load_1),
                                &snap.load_history);
                        });
                });
        });
}

// ── Header ─────────────────────────────────────────────────────────

fn render_header(
    ui: &mut egui::Ui,
    theme: &ThemeColors,
    session: &TerminalSession,
    open: &mut bool,
) {
    egui::TopBottomPanel::top("m_hdr")
        .exact_height(46.0)
        .frame(egui::Frame {
            fill: theme.bg_elevated,
            inner_margin: egui::Margin { left: 14.0, right: 8.0, top: 14.0, bottom: 8.0 },
            ..Default::default()
        })
        .show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                let target = target_label(session);
                ui.label(
                    egui::RichText::new(format!("\u{1F4CA}  {}", target))
                        .color(theme.fg_primary).size(13.0).strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(
                        egui::Button::new(
                            egui::RichText::new("\u{2715}").color(theme.fg_dim).size(14.0),
                        ).frame(false),
                    ).clicked() { *open = false; }
                });
            });
        });
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

fn target_label(session: &TerminalSession) -> String {
    match &session.session {
        Some(crate::ui::types::session::SessionBackend::Ssh(_)) => session
            .ssh_host
            .as_ref()
            .map(|h| format!("{}@{}", h.username, h.host))
            .unwrap_or_else(|| "SSH".to_string()),
        _ => "Local".to_string(),
    }
}

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
