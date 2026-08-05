//! # SFTP Panel Rendering
//!
//! This module contains the file panel rendering logic for the SFTP view,
//! including the breadcrumb navigation and file listing panel.

use eframe::egui;
use crate::sftp::{FileSelection, SftpEntry, SftpEntryKind};
use crate::ui::theme::ThemeColors;
use crate::ui::i18n::Language;
use crate::ui::views::sftp::{DragEntry, DragPayload, SelectionAction, MoveToDirRequest};
use crate::ui::views::sftp::format::{format_file_size, format_modified, format_permissions};

/// Maximum characters shown for a single breadcrumb segment before it is
/// middle-truncated. Keeps long segments (e.g. UUIDs) from blowing out the
/// panel width; the full segment is revealed on hover.
const MAX_SEGMENT_CHARS: usize = 20;

/// Horizontal space reserved to the right of the breadcrumb bar for the
/// per-panel toolbar controls (refresh, switch-to-remote / disconnect, and the
/// show/hide hidden-files toggle). When the full path would not fit alongside
/// these controls, the breadcrumb collapses its middle ancestors into a
/// clickable "…" rather than pushing the toolbar off-screen.
const RIGHT_CONTROLS_RESERVE: f32 = 210.0;

/// Truncate a path segment with a middle ellipsis, preserving head + tail so
/// the segment stays identifiable. Yields "1f27397b-…-945a55" rather than a
/// left-only "1f27397b…", which would hide the discriminating suffix.
fn truncate_segment(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        return s.to_string();
    }
    let head = (max_chars * 2 / 5).max(1);
    let tail = max_chars.saturating_sub(head + 1).max(1);
    let head_s: String = chars[..head].iter().collect();
    let tail_s: String = chars[chars.len() - tail..].iter().collect();
    format!("{head_s}…{tail_s}")
}

/// Render breadcrumb path navigation. Each path segment is a clickable button.
///
/// When the full path would overflow the bar, intermediate ancestors collapse
/// into a single "…" button whose dropdown lists every hidden segment — each
/// clickable to jump straight to that directory. The leading ancestor(s) and
/// the current (last) segment stay visible so global context and the current
/// location are always readable.
pub fn render_breadcrumbs(
    ui: &mut egui::Ui,
    current_path: &str,
    navigate_to: &mut Option<String>,
    _is_local: bool,
    theme: &ThemeColors,
) {
    let parts: Vec<&str> = current_path.split('/').filter(|s| !s.is_empty()).collect();
    let n = parts.len();

    // Root "/"
    if ui
        .add(egui::Button::new(egui::RichText::new("/").color(theme.fg_dim).size(12.0).family(egui::FontFamily::Monospace)).frame(false))
        .clicked()
    {
        *navigate_to = Some("/".to_string());
    }
    if n == 0 {
        return;
    }

    // Precompute truncated display text and measured widths so we can decide
    // how many leading ancestors fit before overflowing the available width.
    let seg_font = egui::FontId::monospace(12.0);
    let sep_font = egui::FontId::monospace(10.0);
    let gap = ui.spacing().item_spacing.x.max(0.0);

    // Measure rendered text width via a memoized single-line galley.
    let text_w = |fid: &egui::FontId, text: &str| -> f32 {
        ui.fonts(|f| f.layout_no_wrap(text.to_string(), fid.clone(), egui::Color32::PLACEHOLDER).size().x)
    };

    let displays: Vec<String> = parts.iter().map(|p| truncate_segment(p, MAX_SEGMENT_CHARS)).collect();
    let seg_widths: Vec<f32> = (0..n).map(|i| text_w(&seg_font, &displays[i])).collect();
    let sep_w = text_w(&sep_font, "/");
    let root_w = text_w(&seg_font, "/");
    // Pad the ellipsis click target a little; over-estimating its width only
    // makes us collapse slightly earlier, never risks overflow.
    let ellipsis_w = text_w(&seg_font, "\u{2026}") + 10.0;
    let safety = 12.0;

    let last = n - 1;
    // Total width if we keep the first `keep` ancestors (parts[0..keep]),
    // collapse the rest into one "…", then show the last segment.
    let width_for = |keep: usize| -> f32 {
        let mut w = root_w + gap;
        for &sw in seg_widths.iter().take(keep) {
            w += sep_w + gap + sw + gap;
        }
        let hidden = last.saturating_sub(keep);
        if hidden > 0 {
            w += sep_w + gap + ellipsis_w + gap;
        }
        w += sep_w + gap + seg_widths[last] + gap;
        w + safety
    };

    let budget = (ui.available_width() - RIGHT_CONTROLS_RESERVE).max(0.0);
    let keep = if width_for(last) <= budget {
        last // everything fits — render the full path
    } else {
        (0..n).rev().find(|&k| width_for(k) <= budget).unwrap_or(0)
    };
    let hidden_count = last.saturating_sub(keep);

    // Kept leading ancestors (clickable → jump to that path).
    for i in 0..keep {
        ui.add_space(0.0);
        ui.label(egui::RichText::new("/").color(theme.fg_dim).size(10.0));
        ui.add_space(0.0);

        let was_truncated = parts[i].chars().count() > MAX_SEGMENT_CHARS;
        let text = egui::RichText::new(&displays[i])
            .color(theme.accent)
            .size(12.0)
            .family(egui::FontFamily::Monospace);
        let mut btn = ui.add(egui::Button::new(text).frame(false));
        if was_truncated {
            btn = btn.on_hover_text(parts[i]);
        }
        if btn.clicked() {
            *navigate_to = Some(format!("/{}", parts[..=i].join("/")));
        }
    }

    // Collapsed middle → "…" menu listing the hidden ancestors.
    if hidden_count > 0 {
        ui.add_space(0.0);
        ui.label(egui::RichText::new("/").color(theme.fg_dim).size(10.0));
        ui.add_space(0.0);

        let mut ellipsis = ui.add(
            egui::Button::new(
                egui::RichText::new("\u{2026}")
                    .color(theme.fg_dim)
                    .size(13.0)
                    .family(egui::FontFamily::Monospace),
            )
            .frame(false),
        );
        let hidden_path: String = (keep..last).map(|i| parts[i]).collect::<Vec<_>>().join("/");
        ellipsis = ellipsis.on_hover_text(format!("{hidden_path}/"));

        let popup_id = ui.make_persistent_id("breadcrumb_ellipsis");
        if ellipsis.clicked() {
            ui.memory_mut(|m| m.toggle_popup(popup_id));
        }
        egui::popup::popup_below_widget(
            ui,
            popup_id,
            &ellipsis,
            egui::popup::PopupCloseBehavior::CloseOnClick,
            |ui| {
                ui.set_min_width(80.0);
                for i in keep..last {
                    // Full (untruncated) segment name so it stays identifiable.
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(parts[i])
                                    .color(theme.accent)
                                    .size(12.0)
                                    .family(egui::FontFamily::Monospace),
                            )
                            .frame(false),
                        )
                        .clicked()
                    {
                        *navigate_to = Some(format!("/{}", parts[..=i].join("/")));
                        ui.memory_mut(|m| m.close_popup());
                    }
                }
            },
        );
    }

    // Current (last) segment — non-clickable label; full text on hover if truncated.
    ui.add_space(0.0);
    ui.label(egui::RichText::new("/").color(theme.fg_dim).size(10.0));
    ui.add_space(0.0);
    let was_truncated = parts[last].chars().count() > MAX_SEGMENT_CHARS;
    let text = egui::RichText::new(&displays[last])
        .color(theme.fg_primary)
        .size(12.0)
        .family(egui::FontFamily::Monospace);
    let resp = ui.label(text);
    if was_truncated {
        resp.on_hover_text(parts[last]);
    }
}

/// Apply a SelectionAction to a FileSelection.
pub fn apply_selection_action(selection: &mut FileSelection, action: SelectionAction, entry_count: usize) {
    match action {
        SelectionAction::Single(i) => selection.select_one(i),
        SelectionAction::Toggle(i) => selection.toggle(i),
        SelectionAction::Range(i) => selection.select_range(i),
        SelectionAction::SelectAll => selection.select_all(entry_count),
        SelectionAction::FocusMove(i) => selection.select_one(i),
        SelectionAction::FocusExtend(i) => selection.extend_to(i),
        SelectionAction::DeselectAll => selection.clear(),
    }
}

/// Render a file listing panel (reused for both local and remote).
/// Each entry is draggable via egui DnD; the caller handles drop detection.
pub fn render_file_panel(
    ui: &mut egui::Ui,
    entries: &[SftpEntry],
    selection: &FileSelection,
    navigate_to: &mut Option<String>,
    selection_action: &mut Option<SelectionAction>,
    is_local: bool,
    current_path: &str,
    theme: &ThemeColors,
    context_menu_request: &mut Option<(egui::Pos2, Option<usize>)>,
    open_file_request: &mut Option<usize>,
    delete_request: &mut bool,
    is_active_panel: bool,
    language: &Language,
    panel_id: &str,
    editor_is_open: bool,
    move_to_dir_request: &mut Option<MoveToDirRequest>,
) {
    let row_height = 26.0;
    let status_bar_height = 24.0;

    // Reserve space for the status bar at the bottom
    let available = ui.available_rect_before_wrap();
    let scroll_rect = egui::Rect::from_min_max(
        available.min,
        egui::pos2(available.max.x, available.max.y - status_bar_height),
    );
    let status_rect = egui::Rect::from_min_max(
        egui::pos2(available.min.x, available.max.y - status_bar_height),
        available.max,
    );

    // Keyboard handling (only for active panel and no other UI wants keyboard input and editor is not open)
    if is_active_panel && !entries.is_empty() && !ui.ctx().wants_keyboard_input() && !editor_is_open {
        let modifiers = ui.ctx().input(|i| i.modifiers);
        let cmd = modifiers.command; // Cmd on macOS, Ctrl on others
        let shift = modifiers.shift;

        // Cmd/Ctrl+A → select all
        if cmd && ui.ctx().input(|i| i.key_pressed(egui::Key::A)) {
            *selection_action = Some(SelectionAction::SelectAll);
        }

        // Arrow keys
        let focus = selection.focus.unwrap_or(0);
        if ui.ctx().input(|i| i.key_pressed(egui::Key::ArrowUp))
            && focus > 0 {
                let new_focus = focus - 1;
                if shift {
                    *selection_action = Some(SelectionAction::FocusExtend(new_focus));
                } else {
                    *selection_action = Some(SelectionAction::FocusMove(new_focus));
                }
            }
        if ui.ctx().input(|i| i.key_pressed(egui::Key::ArrowDown))
            && focus + 1 < entries.len() {
                let new_focus = focus + 1;
                if shift {
                    *selection_action = Some(SelectionAction::FocusExtend(new_focus));
                } else {
                    *selection_action = Some(SelectionAction::FocusMove(new_focus));
                }
            }

        // Enter → open focused entry
        if ui.ctx().input(|i| i.key_pressed(egui::Key::Enter)) {
            if let Some(f) = selection.focus {
                if let Some(entry) = entries.get(f) {
                    if entry.kind == SftpEntryKind::Directory {
                        *navigate_to = Some(entry.name.clone());
                    } else {
                        *open_file_request = Some(f);
                    }
                }
            }
        }

        // Delete / Backspace → request delete
        if ui.ctx().input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
            && !selection.is_empty() {
                *delete_request = true;
            }
    }

    // File listing scroll area
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(scroll_rect), |ui| {
        egui::ScrollArea::vertical()
            .id_salt(panel_id)
            .show(ui, |ui| {
            for (i, entry) in entries.iter().enumerate() {
                let is_selected = selection.is_selected(i);
                let is_focus = selection.focus == Some(i);
                let is_dir = entry.kind == SftpEntryKind::Directory;

                let width = ui.available_width();
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(width, row_height),
                    egui::Sense::click_and_drag(),
                );

                // Check if dragging over this directory entry (for move-to-folder)
                let is_drop_target = is_dir && !is_selected && {
                    let ctx = ui.ctx();
                    if let Some(payload) = egui::DragAndDrop::payload::<DragPayload>(ctx) {
                        if payload.is_local == is_local {
                            if let Some(hover_pos) = ctx.input(|i| i.pointer.hover_pos()) {
                                rect.contains(hover_pos)
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                };

                // Handle drop on directory (move files to this folder)
                if is_drop_target && ui.ctx().input(|i| i.pointer.any_released()) {
                    let ctx = ui.ctx();
                    if let Some(payload) = egui::DragAndDrop::take_payload::<DragPayload>(ctx) {
                        if payload.is_local == is_local && !payload.entries.is_empty() {
                            // Don't move if the only item being dragged is this directory itself
                            let can_move = payload.entries.len() > 1 ||
                                payload.entries[0].entry_name != entry.name;
                            if can_move {
                                *move_to_dir_request = Some(MoveToDirRequest {
                                    source_entries: payload.entries.clone(),
                                    target_dir: entry.name.clone(),
                                });
                            }
                        }
                    }
                }

                // Background: selected, focused, hovered, or drop target
                if is_selected || resp.hovered() || is_drop_target {
                    let bg = if is_drop_target {
                        theme.accent_alpha(50)
                    } else if is_selected {
                        theme.accent_alpha(30)
                    } else {
                        theme.hover_bg
                    };
                    ui.painter().rect_filled(rect, 0.0, bg);
                }

                // Drop target indicator (left border)
                if is_drop_target {
                    let indicator_rect = egui::Rect::from_min_size(
                        rect.min,
                        egui::vec2(3.0, rect.height()),
                    );
                    ui.painter().rect_filled(indicator_rect, 0.0, theme.accent);
                }

                // Focus indicator (subtle left border)
                if is_focus && is_active_panel {
                    let focus_rect = egui::Rect::from_min_size(
                        rect.min,
                        egui::vec2(2.0, rect.height()),
                    );
                    ui.painter().rect_filled(focus_rect, 0.0, theme.accent);
                }

                // Icon
                let icon = match entry.kind {
                    SftpEntryKind::Directory => "\u{1F4C1}",
                    SftpEntryKind::Symlink => "\u{1F517}",
                    _ => "\u{1F4C4}",
                };
                ui.painter().text(
                    egui::pos2(rect.min.x + 8.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    icon,
                    egui::FontId::proportional(11.0),
                    theme.fg_dim,
                );

                // Name (truncated to not overlap with permissions/size)
                let name_color = if is_dir { theme.accent } else { theme.fg_primary };
                let display_name = if is_dir {
                    format!("{}/", entry.name)
                } else {
                    entry.name.clone()
                };
                let name_x = rect.min.x + 28.0;
                // Permissions are right-aligned at rect.max.x - 75.0, text extends left
                // "drwxrwxrwx" is ~60px wide, so permissions end around rect.max.x - 135.0
                // Leave some padding, name should not extend past rect.max.x - 145.0
                let name_max_x = (rect.max.x - 250.0).max(name_x + 40.0);
                let max_name_width = name_max_x - name_x;

                // Create a layout job that doesn't wrap but truncates with ellipsis
                let mut job = egui::text::LayoutJob::simple_singleline(
                    display_name.clone(),
                    egui::FontId::proportional(12.0),
                    name_color,
                );
                job.wrap = egui::text::TextWrapping::truncate_at_width(max_name_width);

                let galley = ui.fonts(|f| f.layout_job(job));

                ui.painter().galley(
                    egui::pos2(name_x, rect.center().y - galley.size().y * 0.5),
                    galley,
                    name_color,
                );

                // Size (right-aligned)
                if !is_dir {
                    if let Some(s) = entry.size {
                        ui.painter().text(
                            egui::pos2(rect.max.x - 8.0, rect.center().y),
                            egui::Align2::RIGHT_CENTER,
                            format_file_size(s),
                            egui::FontId::proportional(11.0),
                            theme.fg_dim,
                        );
                    }
                }

                // Permissions (right-aligned, before size)
                if let Some(mode) = entry.permissions {
                    ui.painter().text(
                        egui::pos2(rect.max.x - 75.0, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        format_permissions(mode),
                        egui::FontId::monospace(10.0),
                        theme.fg_dim,
                    );
                }

                // Modified time (right-aligned, before permissions) — shown on every row.
                ui.painter().text(
                    egui::pos2(rect.max.x - 145.0, rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    format_modified(entry.modified),
                    egui::FontId::monospace(10.0),
                    theme.fg_dim,
                );

                // Drag payload (multi-select aware) — only set when dragging
                if resp.dragged() {
                    if is_selected && selection.count() > 1 {
                        // Dragging from a selected item → pack all selected entries
                        let drag_entries: Vec<DragEntry> = selection.selected.iter()
                            .filter_map(|&idx| entries.get(idx).map(|e| DragEntry {
                                full_path: format!("{}/{}", current_path.trim_end_matches('/'), e.name),
                                entry_name: e.name.clone(),
                                is_dir: e.kind == SftpEntryKind::Directory,
                            }))
                            .collect();
                        resp.dnd_set_drag_payload(DragPayload {
                            is_local,
                            entries: drag_entries,
                        });
                    } else {
                        // Single item drag
                        let full_path = format!(
                            "{}/{}",
                            current_path.trim_end_matches('/'),
                            entry.name
                        );
                        resp.dnd_set_drag_payload(DragPayload {
                            is_local,
                            entries: vec![DragEntry {
                                full_path,
                                entry_name: entry.name.clone(),
                                is_dir,
                            }],
                        });
                    }
                }

                if resp.secondary_clicked() {
                    if let Some(pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
                        *context_menu_request = Some((pos, Some(i)));
                    }
                } else if resp.double_clicked() {
                    if is_dir {
                        *navigate_to = Some(entry.name.clone());
                    } else {
                        *open_file_request = Some(i);
                    }
                } else if resp.clicked() {
                    let modifiers = ui.ctx().input(|i| i.modifiers);
                    if modifiers.command {
                        *selection_action = Some(SelectionAction::Toggle(i));
                    } else if modifiers.shift {
                        *selection_action = Some(SelectionAction::Range(i));
                    } else {
                        *selection_action = Some(SelectionAction::Single(i));
                    }
                }
            }

            // Click on blank area → deselect all
            let remaining = ui.available_rect_before_wrap();
            let blank_resp = ui.allocate_rect(remaining, egui::Sense::click());
            if blank_resp.secondary_clicked() {
                if let Some(pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
                    *context_menu_request = Some((pos, None));
                }
            } else if blank_resp.clicked() {
                *selection_action = Some(SelectionAction::DeselectAll);
            }
        });
    });

    // Status bar at the bottom
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(status_rect), |ui| {
        ui.painter().rect_filled(status_rect, 0.0, theme.bg_secondary);

        let sel_count = selection.count();
        let dir_count = entries.iter().filter(|e| e.kind == SftpEntryKind::Directory).count();
        let file_count = entries.iter().filter(|e| e.kind == SftpEntryKind::File).count();
        let total_size: u64 = entries.iter()
            .filter(|e| e.kind == SftpEntryKind::File)
            .filter_map(|e| e.size)
            .sum();

        let status_text = if sel_count > 0 {
            let sel_size: u64 = selection.selected.iter()
                .filter_map(|&i| entries.get(i))
                .filter(|e| e.kind == SftpEntryKind::File)
                .filter_map(|e| e.size)
                .sum();
            format!(
                "{} \u{2014} {}  |  {} files, {} folders",
                language.tf("n_selected", &sel_count.to_string()),
                format_file_size(sel_size),
                file_count,
                dir_count,
            )
        } else {
            format!(
                "{} files, {} folders  \u{2014}  {}",
                file_count,
                dir_count,
                format_file_size(total_size),
            )
        };

        ui.painter().text(
            egui::pos2(status_rect.min.x + 8.0, status_rect.center().y),
            egui::Align2::LEFT_CENTER,
            &status_text,
            egui::FontId::proportional(11.0),
            theme.fg_dim,
        );
    });
    ui.allocate_rect(available, egui::Sense::hover());
}
