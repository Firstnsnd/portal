//! # Terminal Recording Replay View
//!
//! Plays back an asciicast v2 (`.cast`) recording by re-driving a private
//! `TerminalGrid` + VTE parser through the recorded output events at their
//! original pace. This is a text-stream replay — not video — so the picture
//! stays crisp at any window size and the file stays tiny.
//!
//! State lives on `AppWindow.replay` so switching away and back keeps the
//! playback position.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::terminal::cast::{self, CastHeader, CastRecording, EventKind};
use crate::terminal::{CellAttrs, TerminalGrid, VteHandler, DEFAULT_BG};
use crate::ui::i18n::Language;
use crate::ui::theme::ThemeColors;
use crate::ui::tokens::*;
use crate::ui::widgets;

/// Playback state for the replay view.
pub struct ReplayState {
    pub path: PathBuf,
    pub recording: CastRecording,
    /// Off-screen grid the recording is replayed into.
    grid: TerminalGrid,
    parser: vte::Parser,
    attrs: CellAttrs,
    /// Next event index to apply.
    cursor: usize,
    playing: bool,
    speed: f32,
    /// Virtual playback clock (seconds into the recording timeline).
    virtual_t: f64,
    /// Re-anchoring: (timeline position, wall-clock instant) captured at the
    /// last tick, so `virtual_t = anchor_vt + elapsed * speed` without drift.
    anchor_vt: f64,
    anchor_instant: Instant,
    /// Parse failure (or other fatal problem) shown as a banner.
    pub error: Option<String>,
}

/// Approximate repaint cadence while playing (~30fps).
const TICK_MS: u64 = 33;
/// Speed presets offered in the control bar.
const SPEEDS: [f32; 4] = [0.5, 1.0, 2.0, 4.0];

impl ReplayState {
    /// Open a recording. Never fails: a parse error is stored in `error` and
    /// shown as a banner, keeping the view stable.
    pub fn load(path: &std::path::Path) -> Self {
        match cast::parse_cast(path) {
            Ok(recording) => Self {
                path: path.to_path_buf(),
                grid: TerminalGrid::new(
                    (recording.header.width as usize).max(2),
                    (recording.header.height as usize).max(2),
                ),
                parser: vte::Parser::new(),
                attrs: CellAttrs::default(),
                cursor: 0,
                playing: true,
                speed: 1.0,
                virtual_t: 0.0,
                anchor_vt: 0.0,
                anchor_instant: Instant::now(),
                error: None,
                recording,
            },
            Err(e) => Self {
                path: path.to_path_buf(),
                recording: CastRecording {
                    header: CastHeader { version: 2, width: 80, height: 24, timestamp: 0, title: None, portal_host: None },
                    events: Vec::new(),
                    duration: 0.0,
                },
                grid: TerminalGrid::new(80, 24),
                parser: vte::Parser::new(),
                attrs: CellAttrs::default(),
                cursor: 0,
                playing: false,
                speed: 1.0,
                virtual_t: 0.0,
                anchor_vt: 0.0,
                anchor_instant: Instant::now(),
                error: Some(e),
            },
        }
    }

    /// Apply every event at or before timeline position `t`. Idempotent in
    /// `t`: advancing to an earlier position is a no-op (no scrubbing in v1).
    fn apply_until(&mut self, t: f64) {
        let Self { grid, parser, attrs, cursor, recording, .. } = self;
        while *cursor < recording.events.len() && recording.events[*cursor].time <= t {
            let event = &recording.events[*cursor];
            match event.kind {
                EventKind::Output => {
                    let mut handler = VteHandler { grid, attrs };
                    for &b in event.data.as_bytes() {
                        parser.advance(&mut handler, b);
                    }
                }
                EventKind::Resize => {
                    if let Some((cols, rows)) = parse_size(&event.data) {
                        grid.resize(cols, rows);
                    }
                }
            }
            *cursor += 1;
        }
        self.virtual_t = self.virtual_t.max(t);
    }

    /// Advance playback one frame. Returns the delay until the next repaint,
    /// or `None` when paused or the recording has finished.
    pub fn tick(&mut self) -> Option<Duration> {
        if !self.playing {
            return None;
        }
        if self.cursor >= self.recording.events.len() {
            // All events consumed (or nothing to play).
            self.playing = false;
            self.virtual_t = self.recording.duration.max(self.virtual_t);
            return None;
        }
        let elapsed = self.anchor_instant.elapsed().as_secs_f64() * self.speed as f64;
        self.apply_until(self.anchor_vt + elapsed);
        if self.cursor >= self.recording.events.len() {
            // This frame consumed the final events.
            self.playing = false;
            self.virtual_t = self.recording.duration.max(self.virtual_t);
            return None;
        }
        // Re-anchor so wall-clock multiplication can't accumulate drift.
        self.anchor_vt = self.virtual_t;
        self.anchor_instant = Instant::now();
        Some(Duration::from_millis(TICK_MS))
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Toggle playback. Pressing play after the recording finished restarts
    /// it from the beginning.
    pub fn toggle_play(&mut self) {
        if self.error.is_some() {
            return;
        }
        if !self.playing && self.cursor >= self.recording.events.len() {
            self.rewind();
            self.playing = true;
            return;
        }
        self.playing = !self.playing;
        if self.playing {
            self.anchor_vt = self.virtual_t;
            self.anchor_instant = Instant::now();
        }
    }

    /// Change playback speed, re-anchoring so the picture doesn't jump.
    pub fn set_speed(&mut self, speed: f32) {
        if self.playing {
            let t = self.anchor_vt + self.anchor_instant.elapsed().as_secs_f64() * self.speed as f64;
            self.virtual_t = self.virtual_t.max(t);
            self.anchor_vt = self.virtual_t;
            self.anchor_instant = Instant::now();
        }
        self.speed = speed;
    }

    /// Reset playback to the beginning (fresh grid, fresh parser).
    fn rewind(&mut self) {
        let (cols, rows) = (self.grid.cols, self.grid.rows);
        self.grid = TerminalGrid::new(cols, rows);
        self.parser = vte::Parser::new();
        self.attrs = CellAttrs::default();
        self.cursor = 0;
        self.virtual_t = 0.0;
        self.anchor_vt = 0.0;
        self.anchor_instant = Instant::now();
    }
}

/// `"COLSxROWS"` → `(cols, rows)`; resize events from a recorder always have
/// this shape, but a hand-edited file might not.
fn parse_size(s: &str) -> Option<(usize, usize)> {
    let (cols, rows) = s.split_once('x')?;
    Some((cols.trim().parse().ok()?, rows.trim().parse().ok()?))
}

/// `f64` seconds → `m:ss`.
fn fmt_mmss(t: f64) -> String {
    let t = t.max(0.0) as u64;
    format!("{}:{:02}", t / 60, t % 60)
}

/// One recording in the app's recordings library.
pub struct RecordingEntry {
    pub path: PathBuf,
    /// What was recorded, from the header `title` ("user@host", "zsh").
    /// `None` for older files recorded without a title.
    title: Option<String>,
    /// Which machine the session ran on (header `portal_host`:
    /// "localhost", "user@host:port"). `None` for files without it.
    host: Option<String>,
    size_bytes: u64,
    modified: std::time::SystemTime,
}

/// Scan the recordings directory (newest first). Missing directory → empty.
pub fn list_recordings() -> Vec<RecordingEntry> {
    let dir = crate::config::config_dir().join("recordings");
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_cast = path.extension().map(|x| x == "cast").unwrap_or(false);
        let Ok(meta) = entry.metadata() else { continue };
        if is_cast && meta.is_file() {
            let header = cast::peek_header(&path);
            out.push(RecordingEntry {
                host: header.as_ref().and_then(|h| h.portal_host.clone()),
                title: header.and_then(|h| h.title),
                size_bytes: meta.len(),
                modified: meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH),
                path,
            });
        }
    }
    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    out
}

/// `u64` bytes → human size ("1.5 KB").
pub(crate) fn fmt_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

/// Render the replay page for `window`. Mirrors the hosts page chrome: a
/// `bg_secondary` top nav bar (page title + usage + actions on one line), a
/// `bg_primary` content area, and painter-drawn 58px two-line list rows with
/// host badges, hover action chips and a batch-select mode.
pub fn render_replay_view(
    window: &mut crate::ui::pane::AppWindow,
    ctx: &egui::Context,
    _ui: &mut egui::Ui,
    theme: &ThemeColors,
    font_size: f32,
    language: Language,
) {
    let mut open_clicked = false;
    let mut close_clicked = false;
    let mut load_path: Option<PathBuf> = None;
    let mut tick_wait: Option<Duration> = None;
    // Library row management (delete uses a two-step ✓/✕ confirm on the row).
    let pending_delete = window.replay_pending_delete.clone();
    let mut reveal_clicked: Option<PathBuf> = None;
    let mut pending_delete_set: Option<PathBuf> = None;
    let mut deleted_path: Option<PathBuf> = None;
    let mut cancel_delete = false;
    // Management bar / batch / rename actions — collected as flags here and
    // applied after the panels end, so UI closures stay read-only on state.
    let select_mode = window.replay_select_mode;
    let selection = window.replay_selection.clone();
    let batch_armed = window.replay_batch_arm;
    let mut open_folder = false;
    let mut enter_select = false;
    let mut exit_select = false;
    let mut arm_batch = false;
    let mut delete_selected = false;
    let mut selection_flips: Vec<PathBuf> = Vec::new();
    let mut rename_set: Option<PathBuf> = None;
    let mut rename_cancel = false;
    let mut rename_commit: Option<(PathBuf, String)> = None;
    let mut host_filter_set: Option<String> = None;
    let mut host_filter_clear = false;

    let nav_bar_id = format!("replay_nav_bar_{}", window.viewport_id.0.value());
    // Scanned once per frame; the nav bar shows usage, the content lists rows.
    let recordings = list_recordings();

    // ── Top nav bar: page title + actions (player controls while playing) ──
    egui::TopBottomPanel::top(nav_bar_id)
        .frame(egui::Frame {
            fill: theme.bg_secondary,
            inner_margin: egui::Margin::symmetric(8.0, 8.0),
            stroke: egui::Stroke::NONE,
            ..Default::default()
        })
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(state) = window.replay.as_mut() {
                    // Title: the header title when present ("zsh", "u@h"),
                    // else the file name; full path on hover.
                    let title = state.recording.header.title.clone()
                        .or_else(|| state.path.file_name()
                            .map(|n| n.to_string_lossy().to_string()))
                        .unwrap_or_else(|| language.t("replay").to_string());
                    ui.add(egui::Label::new(
                        egui::RichText::new(title).color(theme.fg_dim).size(FONT_BASE).strong(),
                    ).truncate()).on_hover_text(state.path.display().to_string());
                    // Which machine the session ran on, next to the title.
                    if let Some(host) = &state.recording.header.portal_host {
                        ui.add_space(SPACE_SM);
                        ui.label(egui::RichText::new(format!("\u{25cf} {host}"))
                            .color(theme.accent).size(FONT_SM));
                    }

                    let play_label = if state.is_playing() {
                        language.t("replay_pause")
                    } else {
                        language.t("replay_play")
                    };
                    ui.add_space(SPACE_MD);
                    if ui.add(widgets::text_button(play_label, theme.accent)).clicked() {
                        state.toggle_play();
                    }

                    // Speed segment — same selected style as the port-forward
                    // kind selector on the hosts page.
                    for &speed in &SPEEDS {
                        let label =
                            if speed < 1.0 { format!("{:.1}x", speed) } else { format!("{}x", speed) };
                        let selected = (state.speed - speed).abs() < f32::EPSILON;
                        let color = if selected { theme.accent } else { theme.fg_dim };
                        let bg =
                            if selected { theme.accent_alpha(15) } else { egui::Color32::TRANSPARENT };
                        let btn = egui::Button::new(
                            egui::RichText::new(label).size(12.0).color(color),
                        )
                        .fill(bg)
                        .rounding(4.0)
                        .stroke(egui::Stroke::new(
                            1.0,
                            if selected { theme.accent } else { egui::Color32::TRANSPARENT },
                        ));
                        if ui.add(btn).clicked() {
                            state.set_speed(speed);
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Back to the library (rightmost).
                        if ui.add(egui::Button::new(
                            egui::RichText::new("\u{2715}").size(12.0).color(theme.fg_dim),
                        ).frame(false)).clicked() {
                            close_clicked = true;
                        }
                        ui.add_space(SPACE_MD);
                        let pos = state.virtual_t.min(state.recording.duration);
                        ui.label(egui::RichText::new(format!(
                            "{} / {}",
                            fmt_mmss(pos),
                            fmt_mmss(state.recording.duration),
                        )).monospace().color(theme.fg_primary).size(12.0));
                    });
                } else {
                    // Library chrome on one bar, like the hosts page: page
                    // title, usage summary, then the management actions.
                    ui.label(widgets::section_header(language.t("replay"), theme));
                    ui.add_space(SPACE_MD);
                    let total: u64 = recordings.iter().map(|r| r.size_bytes).sum();
                    ui.label(egui::RichText::new(format!(
                        "{} · {}",
                        language.tf("recordings_files", &recordings.len().to_string()),
                        fmt_size(total)
                    )).color(theme.fg_dim).size(FONT_SM));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if select_mode {
                            // \u{2713} marks the armed (two-step) delete.
                            if ui.add(widgets::text_button(language.t("cancel"), theme.fg_dim)).clicked() {
                                exit_select = true;
                            }
                            ui.add_space(SPACE_SM);
                            let n = selection.len();
                            let label = if batch_armed {
                                format!("\u{2713} {}", language.t("replay_delete_selected"))
                            } else if n > 0 {
                                format!("{} ({})", language.t("replay_delete_selected"), n)
                            } else {
                                language.t("replay_delete_selected").to_string()
                            };
                            if ui.add(widgets::text_button(
                                &label,
                                if batch_armed { theme.red } else { theme.accent },
                            )).clicked() {
                                if batch_armed && n > 0 {
                                    delete_selected = true;
                                } else if n > 0 {
                                    arm_batch = true;
                                } else {
                                    // Nothing selected: leaving instead of deleting.
                                    exit_select = true;
                                }
                            }
                        } else {
                            open_clicked =
                                ui.add(widgets::text_button(language.t("replay_open"), theme.accent))
                                    .clicked();
                            ui.add_space(SPACE_SM);
                            if ui.add(widgets::text_button(language.t("replay_select"), theme.fg_dim)).clicked() {
                                enter_select = true;
                            }
                            ui.add_space(SPACE_SM);
                        }
                        if ui.add(widgets::text_button(language.t("replay_open_folder"), theme.fg_dim)).clicked() {
                            open_folder = true;
                        }
                    });
                }
            });
        });

    // ── Content ──
    egui::CentralPanel::default()
        .frame(egui::Frame {
            fill: theme.bg_primary,
            ..Default::default()
        })
        .show(ctx, |ui| {
            let Some(state) = window.replay.as_mut() else {
                // Library: auto-discovered recordings (scanned above for the
                // nav bar), newest first. Rows only — management lives up there.
                egui::ScrollArea::vertical()
                    .id_salt("replay_library_scroll")
                    .show(ui, |ui| {
                        if recordings.is_empty() {
                            ui.centered_and_justified(|ui| {
                                ui.vertical_centered(|ui| {
                                    ui.add_space(SPACE_2XL);
                                    ui.label(egui::RichText::new("\u{25b6}")
                                        .size(36.0).color(theme.fg_dim));
                                    ui.add_space(SPACE_SM);
                                    ui.label(egui::RichText::new(
                                        language.t("replay_no_recordings"),
                                    ).color(theme.fg_dim).size(FONT_MD));
                                });
                            });
                            return;
                        }
                        // Host filter chips: "All" + one chip per known
                        // portal_host. Also the landing state for the
                        // context-menu "View Host Recordings…" jump.
                        let host_filter = window.replay_host_filter.clone();
                        let mut hosts: Vec<String> = recordings
                            .iter()
                            .filter_map(|r| r.host.clone())
                            .collect();
                        hosts.sort();
                        hosts.dedup();
                        if !hosts.is_empty() {
                            ui.add_space(SPACE_SM);
                            ui.horizontal(|ui| {
                                ui.add_space(SPACE_XL);
                                if host_chip(ui, language.t("replay_all_hosts"), host_filter.is_none(), theme).clicked() {
                                    host_filter_clear = true;
                                }
                                for h in &hosts {
                                    let selected = host_filter.as_deref() == Some(h.as_str());
                                    if host_chip(ui, h, selected, theme).clicked() {
                                        host_filter_set = Some(h.clone());
                                    }
                                }
                            });
                        }
                        let shown: Vec<&RecordingEntry> = match &host_filter {
                            Some(h) => recordings
                                .iter()
                                .filter(|r| r.host.as_deref() == Some(h.as_str()))
                                .collect(),
                            None => recordings.iter().collect(),
                        };
                        if shown.is_empty() {
                            ui.centered_and_justified(|ui| {
                                ui.vertical_centered(|ui| {
                                    ui.add_space(SPACE_2XL);
                                    ui.label(egui::RichText::new(
                                        language.tf("replay_none_for_host", host_filter.as_deref().unwrap_or("")),
                                    ).color(theme.fg_dim).size(FONT_MD));
                                });
                            });
                            return;
                        }
                        ui.add_space(SPACE_SM);
                        for rec in shown {
                            // Inline rename replaces the normal row while active.
                            if window.replay_rename.as_ref().is_some_and(|(p, _)| *p == rec.path) {
                                ui.add_space(SPACE_XS);
                                ui.horizontal(|ui| {
                                    ui.add_space(SPACE_XL);
                                    if let Some((_, text)) = window.replay_rename.as_mut() {
                                        let resp = ui.add(
                                            egui::TextEdit::singleline(text)
                                                .desired_width((ui.available_width() - 200.0).max(160.0))
                                                .font(egui::FontId::proportional(13.0)),
                                        );
                                        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                            rename_commit = Some((rec.path.clone(), text.clone()));
                                        }
                                        ui.add_space(SPACE_SM);
                                        if ui.add(widgets::text_button(language.t("rename"), theme.accent)).clicked() {
                                            rename_commit = Some((rec.path.clone(), text.clone()));
                                        }
                                        if ui.add(widgets::text_button(language.t("cancel"), theme.fg_dim)).clicked() {
                                            rename_cancel = true;
                                        }
                                    }
                                });
                                ui.add_space(SPACE_XS);
                                continue;
                            }
                            let row_h = 58.0;
                            let width = ui.available_width();
                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(width, row_h),
                                egui::Sense::click(),
                            );
                            let ticked = select_mode && selection.contains(&rec.path);
                            let armed = pending_delete.as_ref() == Some(&rec.path);
                            // Row fills: an armed (pending-delete) row tints
                            // red, a ticked row tints accent, hover adds the
                            // standard hosts-page strip + wash on top.
                            if armed {
                                ui.painter().rect_filled(rect, 0.0, theme.error_dim);
                            }
                            if ticked {
                                ui.painter().rect_filled(rect, 0.0, theme.accent_alpha(15));
                            }
                            if resp.hovered() {
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
                            // Leading slot: the ✓-checkbox takes over the ▶
                            // glyph in batch mode, at the hosts-page offsets.
                            if select_mode {
                                let cb = egui::Rect::from_center_size(
                                    egui::pos2(rect.min.x + 30.0, rect.center().y),
                                    egui::vec2(14.0, 14.0),
                                );
                                ui.painter().rect_stroke(cb, 3.0, egui::Stroke::new(1.0, theme.fg_dim));
                                if ticked {
                                    ui.painter().rect_filled(cb.shrink(2.0), 2.0, theme.accent);
                                    ui.painter().text(
                                        cb.center(),
                                        egui::Align2::CENTER_CENTER,
                                        "\u{2713}",
                                        egui::FontId::proportional(FONT_XS),
                                        theme.bg_primary,
                                    );
                                }
                            } else {
                                ui.painter().text(
                                    egui::pos2(rect.min.x + 24.0, rect.min.y + 18.0),
                                    egui::Align2::LEFT_CENTER,
                                    "\u{25b6}",
                                    egui::FontId::proportional(FONT_MD),
                                    theme.accent,
                                );
                            }
                            let name = rec.title.clone().unwrap_or_else(|| {
                                rec.path.file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_default()
                            });
                            ui.painter().text(
                                egui::pos2(rect.min.x + 46.0, rect.min.y + 18.0),
                                egui::Align2::LEFT_CENTER,
                                &name,
                                egui::FontId::proportional(FONT_BASE),
                                theme.fg_primary,
                            );
                            // Host as an inline accent badge next to the title
                            // (hosts-page badge style) when there IS a title;
                            // when the title IS the filename, the host joins
                            // the meta line so it still says where it ran.
                            if let Some(host) = rec.host.as_deref().filter(|_| rec.title.is_some()) {
                                let name_w = ui.fonts(|f| f.layout_no_wrap(
                                    name.clone(),
                                    egui::FontId::proportional(FONT_BASE),
                                    egui::Color32::PLACEHOLDER,
                                ).rect.width());
                                ui.painter().text(
                                    egui::pos2(rect.min.x + 46.0 + name_w + 10.0, rect.min.y + 18.0),
                                    egui::Align2::LEFT_CENTER,
                                    format!("\u{25cf} {host}"),
                                    egui::FontId::proportional(FONT_XS),
                                    theme.accent,
                                );
                            }
                            let mut meta_parts: Vec<String> = Vec::new();
                            if rec.title.is_some() {
                                if let Some(file) = rec.path.file_name() {
                                    meta_parts.push(file.to_string_lossy().to_string());
                                }
                            }
                            if rec.title.is_none() {
                                if let Some(host) = &rec.host {
                                    meta_parts.push(host.clone());
                                }
                            }
                            meta_parts.push(fmt_date(rec.modified));
                            meta_parts.push(fmt_size(rec.size_bytes));
                            let meta = meta_parts.join("  ·  ");
                            let meta_clip = egui::Rect::from_min_max(
                                egui::pos2(rect.min.x + 46.0, rect.min.y + 26.0),
                                egui::pos2(rect.max.x - 96.0, rect.max.y - 4.0),
                            );
                            ui.painter().with_clip_rect(meta_clip).text(
                                egui::pos2(meta_clip.min.x, rect.min.y + 34.0),
                                egui::Align2::LEFT_CENTER,
                                &meta,
                                egui::FontId::proportional(FONT_XS),
                                theme.fg_dim,
                            );

                            // ── Row actions: reveal / delete ─────────────
                            // Hover shows two bordered chips at the right edge
                            // (hosts-page button style): ↗ reveal, ✕ delete
                            // (two-step — the armed row fills red behind ✓ and
                            // keeps a dim ✕ as the cancel). Clicks inside these
                            // zones never open the player.
                            let reveal_zone = egui::Rect::from_min_max(
                                egui::pos2(rect.max.x - 68.0, rect.min.y),
                                egui::pos2(rect.max.x - 36.0, rect.max.y),
                            );
                            let delete_zone = egui::Rect::from_min_max(
                                egui::pos2(rect.max.x - 36.0, rect.min.y),
                                egui::pos2(rect.max.x - 8.0, rect.max.y),
                            );
                            let chip_rect = |zone: egui::Rect| {
                                egui::Rect::from_center_size(
                                    zone.center(),
                                    egui::vec2(24.0, 20.0),
                                )
                            };
                            if armed {
                                let confirm = chip_rect(reveal_zone);
                                ui.painter().rect(confirm, 4.0, theme.red, egui::Stroke::NONE);
                                ui.painter().text(
                                    confirm.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "\u{2713}",
                                    egui::FontId::proportional(FONT_MD),
                                    theme.bg_primary,
                                );
                                ui.painter().text(
                                    chip_rect(delete_zone).center(),
                                    egui::Align2::CENTER_CENTER,
                                    "\u{2715}",
                                    egui::FontId::proportional(FONT_SM),
                                    theme.fg_dim,
                                );
                                zone_tooltip(ui, reveal_zone, "replay_confirm_delete", language.t("confirm_delete"));
                                zone_tooltip(ui, delete_zone, "replay_cancel_delete", language.t("delete"));
                            } else if resp.hovered() && !select_mode {
                                let pointer = ui.input(|i| i.pointer.hover_pos());
                                let over_rev = pointer.is_some_and(|p| reveal_zone.contains(p));
                                let over_del = pointer.is_some_and(|p| delete_zone.contains(p));
                                let chip = |zone: egui::Rect, glyph: &str, glyph_color: egui::Color32, ring: egui::Color32, fill: egui::Color32| {
                                    let r = chip_rect(zone);
                                    ui.painter().rect(r, 4.0, fill, egui::Stroke::new(1.0, ring));
                                    ui.painter().text(
                                        r.center(),
                                        egui::Align2::CENTER_CENTER,
                                        glyph,
                                        egui::FontId::proportional(FONT_MD),
                                        glyph_color,
                                    );
                                };
                                chip(
                                    reveal_zone,
                                    "\u{2197}",
                                    if over_rev { theme.accent } else { theme.fg_dim },
                                    if over_rev { theme.accent } else { theme.border },
                                    if over_rev { theme.bg_elevated } else { egui::Color32::TRANSPARENT },
                                );
                                chip(
                                    delete_zone,
                                    "\u{2715}",
                                    if over_del { theme.red } else { theme.fg_dim },
                                    if over_del { theme.red } else { theme.border },
                                    if over_del { theme.bg_elevated } else { egui::Color32::TRANSPARENT },
                                );
                                zone_tooltip(ui, reveal_zone, "replay_reveal", language.t("replay_reveal"));
                                zone_tooltip(ui, delete_zone, "replay_delete", language.t("delete"));
                            }
                            if resp.clicked() {
                                let pos = resp.interact_pointer_pos()
                                    .or_else(|| resp.hover_pos())
                                    .unwrap_or_else(|| rect.center());
                                if select_mode {
                                    selection_flips.push(rec.path.clone());
                                } else if armed {
                                    if reveal_zone.contains(pos) {
                                        deleted_path = Some(rec.path.clone()); // ✓ confirms
                                    } else {
                                        // ✕ or anywhere else cancels the arm.
                                        cancel_delete = true;
                                    }
                                } else if reveal_zone.contains(pos) {
                                    reveal_clicked = Some(rec.path.clone());
                                } else if delete_zone.contains(pos) {
                                    pending_delete_set = Some(rec.path.clone());
                                } else {
                                    load_path = Some(rec.path.clone());
                                }
                            }
                            let resp = resp.on_hover_text(rec.path.display().to_string());
                            if !select_mode {
                                resp.context_menu(|ui| {
                                    if ui.button(language.t("rename")).clicked() {
                                        rename_set = Some(rec.path.clone());
                                        ui.close_menu();
                                    }
                                    if ui.button(language.t("replay_reveal")).clicked() {
                                        reveal_clicked = Some(rec.path.clone());
                                        ui.close_menu();
                                    }
                                    if ui.button(language.t("delete")).clicked() {
                                        pending_delete_set = Some(rec.path.clone());
                                        ui.close_menu();
                                    }
                                });
                            }
                        }
                    });
                return;
            };

            // Parse failure banner — no screen, no ticking.
            if let Some(err) = &state.error {
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(SPACE_2XL);
                        ui.label(egui::RichText::new(language.tf("replay_load_failed", err))
                            .color(theme.red).size(FONT_MD));
                    });
                });
                return;
            }

            // Screen: full-bleed terminal area (grid centered on the
            // terminal background) — no card chrome, like a live pane.
            let screen = ui.available_rect_before_wrap();
            paint_grid(ui, screen, &state.grid, theme, font_size);
            ui.allocate_rect(screen, egui::Sense::hover());

            tick_wait = state.tick();
        });

    // Borrows of window.replay are done — act on clicks from either branch.
    if open_clicked {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("asciicast", &["cast"])
            .pick_file()
        {
            load_path = Some(path);
        }
    }
    if let Some(path) = load_path {
        window.replay = Some(ReplayState::load(&path));
    }
    if close_clicked {
        window.replay = None;
    }
    if let Some(path) = reveal_clicked {
        reveal_in_file_manager(&path);
    }
    if let Some(path) = pending_delete_set {
        window.replay_pending_delete = Some(path);
    }
    if cancel_delete {
        window.replay_pending_delete = None;
    }
    if let Some(path) = deleted_path {
        let _ = std::fs::remove_file(&path);
        window.replay_pending_delete = None;
        window.replay_selection.remove(&path);
        let open_matches = window.replay.as_ref().is_some_and(|r| r.path == path);
        if open_matches {
            window.replay = None;
        }
    }
    // ── Management bar / batch / rename ──
    if open_folder {
        let dir = crate::terminal::cast::recordings_dir();
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(&dir).spawn();
        #[cfg(not(target_os = "macos"))]
        let _ = std::process::Command::new("xdg-open").arg(&dir).spawn();
    }
    if enter_select {
        window.replay_select_mode = true;
        window.replay_batch_arm = false;
    }
    if exit_select {
        window.replay_select_mode = false;
        window.replay_selection.clear();
        window.replay_batch_arm = false;
    }
    for path in selection_flips {
        if !window.replay_selection.remove(&path) {
            window.replay_selection.insert(path);
        }
    }
    if arm_batch {
        window.replay_batch_arm = true;
    }
    if delete_selected {
        let mut removed_open = false;
        for path in window.replay_selection.drain() {
            if std::fs::remove_file(&path).is_ok() {
                if window.replay.as_ref().is_some_and(|r| r.path == path) {
                    removed_open = true;
                }
                if window.replay_pending_delete.as_ref() == Some(&path) {
                    window.replay_pending_delete = None;
                }
            }
        }
        if removed_open {
            window.replay = None;
        }
        window.replay_batch_arm = false;
        window.replay_select_mode = false;
    }
    if let Some(path) = rename_set {
        let current = crate::terminal::cast::peek_header(&path)
            .and_then(|h| h.title)
            .unwrap_or_else(|| {
                path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            });
        window.replay_rename = Some((path, current));
    }
    if rename_cancel {
        window.replay_rename = None;
    }
    if host_filter_clear {
        window.replay_host_filter = None;
    }
    if let Some(host) = host_filter_set {
        window.replay_host_filter = Some(host);
    }
    if let Some((path, title)) = rename_commit {
        // Title lives in the file's NDJSON header line; events are untouched.
        if crate::terminal::cast::set_header_title(&path, &title).is_ok() {
            if let Some(state) = window.replay.as_mut() {
                if state.path == path {
                    state.recording.header.title = Some(title);
                }
            }
        }
        window.replay_rename = None;
    }
    if let Some(wait) = tick_wait {
        ctx.request_repaint_after(wait);
    }
}

/// Reveal a recording file in the OS file manager (Finder on macOS,
/// xdg-open of the parent directory elsewhere).
fn reveal_in_file_manager(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    }
    #[cfg(not(target_os = "macos"))]
    {
        if let Some(parent) = path.parent() {
            let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
        }
    }
}

/// Filter chip button (speed-selector visual: accent when selected, dim
/// ghost otherwise).
fn host_chip(ui: &mut egui::Ui, label: &str, selected: bool, theme: &ThemeColors) -> egui::Response {
    let color = if selected { theme.accent } else { theme.fg_dim };
    let bg = if selected { theme.accent_alpha(15) } else { egui::Color32::TRANSPARENT };
    ui.add(egui::Button::new(
        egui::RichText::new(label).size(FONT_SM).color(color),
    )
    .fill(bg)
    .rounding(4.0)
    .stroke(egui::Stroke::new(
        1.0,
        if selected { theme.accent } else { egui::Color32::TRANSPARENT },
    )))
}

/// Tooltip anchored to a painted glyph zone (the row's action glyphs are
/// painted, not widgets, so `Response::on_hover_text` can't reach them).
fn zone_tooltip(ui: &egui::Ui, zone: egui::Rect, id: &str, text: &str) {
    if ui.rect_contains_pointer(zone) {
        egui::containers::popup::show_tooltip(
            ui.ctx(),
            ui.layer_id(),
            egui::Id::new(id),
            |tip| {
                tip.label(text);
            },
        );
    }
}

/// `SystemTime` → "YYYY-MM-DD HH:MM" (local time).
fn fmt_date(t: std::time::SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d %H:%M").to_string()
}

/// Paint the replayed grid into `rect`. A mini terminal painter: the live
/// renderer (~1500 lines) handles selection, scrollback, links and input —
/// none of which a replay needs.
fn paint_grid(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    grid: &TerminalGrid,
    theme: &ThemeColors,
    font_size: f32,
) {
    if grid.cells.is_empty() || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let font_id = egui::FontId::monospace(font_size);
    // "MM"/2 measures the shaped advance (see render.rs measure_char_width).
    let char_w = ui.fonts(|f| {
        let g = f.layout_no_wrap("MM".to_string(), font_id.clone(), egui::Color32::WHITE);
        (g.size().x / 2.0).max(1.0)
    });
    let line_h = ui.fonts(|f| f.row_height(&font_id)).ceil();
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme.bg_primary);

    // Center the recorded screen in the available rect.
    let origin = egui::Pos2::new(
        rect.left() + ((rect.width() - grid.cols as f32 * char_w) / 2.0).max(0.0),
        rect.top() + ((rect.height() - grid.rows as f32 * line_h) / 2.0).max(0.0),
    );

    for (row_idx, row) in grid.cells.iter().enumerate() {
        let y = origin.y + row_idx as f32 * line_h;
        let mut col = 0usize;
        while col < row.len() {
            let cell = &row[col];
            // Blank-on-default cells render nothing; wide-char continuation
            // cells are painted as part of the wide glyph before them.
            if cell.c == '\0'
                || (cell.c == ' ' && cell.bg_color == DEFAULT_BG)
                || cell.wide_continuation
            {
                col += 1;
                continue;
            }
            // Chunk runs of same-styled cells into one galley — a row of
            // uniform text is a single layout instead of one per glyph.
            let key = (
                cell.fg_color, cell.bg_color, cell.bold,
                cell.italic, cell.underline, cell.dim,
            );
            let start = col;
            let mut text = String::new();
            while col < row.len() {
                let c = &row[col];
                if c.c == '\0' {
                    break;
                }
                if c.wide_continuation {
                    col += 1;
                    continue;
                }
                if (
                    c.fg_color, c.bg_color, c.bold,
                    c.italic, c.underline, c.dim,
                ) != key {
                    break;
                }
                text.push(c.c);
                col += 1;
            }
            let pos = egui::Pos2::new(origin.x + start as f32 * char_w, y);
            if key.1 != DEFAULT_BG {
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        pos,
                        egui::vec2((col - start) as f32 * char_w, line_h),
                    ),
                    0.0,
                    egui::Color32::from_rgb(key.1 .0, key.1 .1, key.1 .2),
                );
            }
            let fg = egui::Color32::from_rgb(key.0 .0, key.0 .1, key.0 .2);
            let galley = painter.layout_no_wrap(text, font_id.clone(), fg);
            painter.galley(pos, galley, fg);
        }
    }

    // Soft accent block for the recorded cursor position.
    if grid.cursor_visible {
        if let Some(cell) = grid.cells.get(grid.cursor_row).and_then(|r| r.get(grid.cursor_col)) {
            if !(cell.c == '\0' || cell.c == ' ' && cell.bg_color == DEFAULT_BG) {
                let cpos = egui::Pos2::new(
                    origin.x + grid.cursor_col as f32 * char_w,
                    origin.y + grid.cursor_row as f32 * line_h,
                );
                painter.rect_filled(
                    egui::Rect::from_min_size(cpos, egui::vec2(char_w, line_h)),
                    2.0,
                    theme.accent_alpha(90),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::cast::CastEvent;

    /// `apply_until` drives the recorded byte stream through a real VTE
    /// parser: events land in the grid in order, only up to the given time.
    #[test]
    fn apply_until_replays_events_in_time_order() {
        let mut state = {
            let rec = CastRecording {
                header: CastHeader { version: 2, width: 20, height: 5, timestamp: 0, title: None, portal_host: None },
                events: vec![
                    CastEvent { time: 0.0, kind: EventKind::Output, data: "hel".into() },
                    CastEvent { time: 1.0, kind: EventKind::Output, data: "lo".into() },
                    CastEvent { time: 2.0, kind: EventKind::Output, data: " world".into() },
                ],
                duration: 2.0,
            };
            ReplayState {
                path: PathBuf::from("test.cast"),
                recording: rec,
                grid: TerminalGrid::new(20, 5),
                parser: vte::Parser::new(),
                attrs: CellAttrs::default(),
                cursor: 0,
                playing: false,
                speed: 1.0,
                virtual_t: 0.0,
                anchor_vt: 0.0,
                anchor_instant: Instant::now(),
                error: None,
            }
        };

        // t=0.5: only the first event.
        state.apply_until(0.5);
        assert_eq!(state.cursor, 1);
        let text: String = state.grid.cells[0].iter().map(|c| c.c).collect();
        assert!(text.starts_with("hel"), "got: {text:?}");

        // t=1.5: second event applied.
        state.apply_until(1.5);
        assert_eq!(state.cursor, 2);
        let text: String = state.grid.cells[0].iter().map(|c| c.c).collect();
        assert!(text.starts_with("hello"), "got: {text:?}");

        // Replaying an earlier time is a no-op (no scrubbing in v1).
        state.apply_until(0.1);
        assert_eq!(state.cursor, 2);

        // Past the end: everything applied.
        state.apply_until(99.0);
        assert_eq!(state.cursor, 3);
        let text: String = state.grid.cells[0].iter().map(|c| c.c).collect();
        assert!(text.starts_with("hello world"), "got: {text:?}");
    }

    #[test]
    fn parse_size_reads_cols_x_rows() {
        assert_eq!(parse_size("100x40"), Some((100, 40)));
        assert_eq!(parse_size(" 80 x 24 "), Some((80, 24)));
        assert_eq!(parse_size("bogus"), None);
        assert_eq!(parse_size("10x"), None);
    }

    #[test]
    fn fmt_mmss_formats_minutes_and_seconds() {
        assert_eq!(fmt_mmss(0.0), "0:00");
        assert_eq!(fmt_mmss(65.0), "1:05");
        assert_eq!(fmt_mmss(600.4), "10:00");
    }
}
