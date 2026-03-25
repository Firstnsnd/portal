//! # Terminal Rendering
//!
//! This module contains terminal rendering functions for displaying
//! terminal session content in the UI.

use eframe::egui;

use crate::config::ShortcutAction;
use crate::ssh::SshConnectionState;
use crate::terminal;
use crate::ui::types::{session::{TerminalSession, SearchMatch, SessionBackend}, dialogs::BroadcastState};
use crate::ui::pane::{PaneNode, PaneAction, SplitDirection, split_rect};
use crate::ui::theme::ThemeColors;
use crate::ui::i18n::Language;
use crate::ui::input::{key_to_ctrl_byte, key_to_char, ShortcutResolver};
use super::selection::find_word_boundaries;


// ---------------------------------------------------------------------------
// Helper: measure true monospace cell width via galley composition
// ---------------------------------------------------------------------------
//
// `glyph_width('M')` returns the advance for a single glyph, which includes
// left bearing but omits the inter-glyph spacing that the shaper adds when
// composing a run. Measuring "MM" and halving gives the per-cell advance that
// the font engine actually uses when laying out a sequence of characters.
// This value is used for ALL grid-to-pixel coordinate conversions so that
// text rendering, cursor, and selection highlights all share the same metric.
fn measure_char_width(fonts: &egui::text::Fonts, font_id: &egui::FontId) -> f32 {
    let galley = fonts.layout_no_wrap(
        "MM".to_string(),
        font_id.clone(),
        egui::Color32::WHITE,
    );
    galley.size().x / 2.0
}

/// Core terminal session rendering function.
///
/// This is the main rendering function that handles all terminal UI including:
/// - Text content rendering (grid + scrollback)
/// - Mouse selection (click, drag, double-click, triple-click)
/// - Keyboard input processing
/// - IME (Input Method Editor) for CJK languages
/// - Close button interaction
/// - Status bar and scrollback indicator
///
/// # Arguments
///
/// * `ui` - egui UI context for rendering
/// * `ctx` - egui context for input/output
/// * `session` - Terminal session state (grid, scrollback, selection, etc.)
/// * `session_id` - Unique identifier for this session
/// * `focused_session_id` - Currently focused session ID
/// * `broadcast_state` - Broadcast mode state (typing to multiple terminals)
/// * `ime_composing` - IME composition state flag
/// * `ime_preedit` - IME preedit text buffer
/// * `pane_rect` - Rectangle for this pane's content area
/// * `pane_id` - egui ID for this pane
/// * `show_close_btn` - Whether to show close button
/// * `can_close_pane` - Whether this pane can be closed
/// * `theme` - Color theme for rendering
/// * `font_size` - Monospace font size in points
/// * `language` - Language for UI text (EN/ZH/JA/KO)
///
/// # Returns
///
/// * `(Option<PaneAction>, Vec<u8>)` - Action to perform (close/focus/split) and input bytes to send
#[allow(clippy::too_many_arguments)]
pub fn render_terminal_session(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    session: &mut TerminalSession,
    session_id: usize,
    focused_session_id: usize,
    broadcast_state: &BroadcastState,
    ime_composing: &mut bool,
    ime_preedit: &mut String,
    pane_rect: egui::Rect,
    pane_id: egui::Id,
    show_close_btn: bool,
    can_close_pane: bool,
    theme: &ThemeColors,
    font_size: f32,
    language: &Language,
    shortcut_resolver: &ShortcutResolver,
) -> (Option<PaneAction>, Vec<u8>) {
    let font_id = egui::FontId::monospace(font_size);
    let pad_x = 8.0_f32;
    let pad_y_top = 6.0_f32;
    let pad_y_bottom = 0.0_f32;
    let mut input_bytes: Vec<u8> = Vec::new();

    // ── FIX 1: measure char_width via galley composition, not glyph_width ──
    //
    // glyph_width('M') returns the advance for a single isolated glyph.
    // When egui composes a run of characters it uses the shaper's per-cell
    // advance (which accounts for inter-glyph spacing). Measuring "MM" and
    // halving gives the value that is consistent with how egui positions
    // subsequent characters in a text run, eliminating a systematic ~0.5 px
    // offset that accumulated into a full character's worth of drift over a
    // typical terminal line.
    let char_width = ui.fonts(|f| measure_char_width(f, &font_id));

    let line_height = ui.fonts(|f| f.row_height(&font_id)).ceil();
    let new_cols = (((pane_rect.width() - pad_x * 2.0) / char_width) as usize).max(10);

    // Reserve one full line-height at the bottom so the cursor/input row is
    // never flush against the pane edge.  We subtract one row from the VTE
    // grid so the shell always sees one fewer usable row; the pixel space that
    // row would have occupied becomes extra bottom padding.
    //
    // Why reduce VTE rows rather than just add visual padding?
    // If we only add visual padding but keep VTE rows the same, the shell
    // (and programs like vim/htop) would write to the last row and have it
    // rendered right at the bottom edge with no breathing room.  Reducing
    // new_rows by 1 tells the PTY "your terminal is one row shorter", so
    // every program — including the shell prompt — naturally stays above the
    // reserved bottom line.
    let available_height = pane_rect.height() - pad_y_top - pad_y_bottom;
    let total_rows = ((available_height / line_height) as usize).max(4);
    let new_rows = total_rows;
    session.resize(new_cols, new_rows);

    // Make rect fill the full available height (pane adjacent to status bar)
    let rect = egui::Rect::from_min_size(
        egui::pos2(pane_rect.min.x + pad_x, pane_rect.min.y + pad_y_top),
        egui::vec2(char_width * new_cols as f32, available_height),
    );

    // Close button rect (used for hit-testing, not as a separate widget)
    let btn_sz = 18.0;
    let close_btn_rect = egui::Rect::from_min_size(
        egui::pos2(pane_rect.max.x - btn_sz - 6.0, pane_rect.min.y + 6.0),
        egui::vec2(btn_sz, btn_sz),
    );

    let response = ui.interact(pane_rect, pane_id, egui::Sense::click_and_drag());
    let mut action: Option<PaneAction> = None;

    let pointer_pos = ctx.input(|i| i.pointer.interact_pos());
    let close_btn_hovered = pointer_pos.map_or(false, |p| close_btn_rect.contains(p)) && response.hovered();
    let close_btn_clicked = show_close_btn && close_btn_hovered && response.clicked();

    if close_btn_clicked {
        action = Some(PaneAction::ClosePane);
    } else if response.clicked() || response.drag_started() {
        action = Some(PaneAction::Focus);
    }
    let is_focused = session_id == focused_session_id;
    if is_focused {
        response.request_focus();
    }

    let search_input_id = egui::Id::new("search_input").with(pane_id);
    let search_has_focus = ui.memory(|mem| mem.has_focus(search_input_id));

    let painter = ui.painter_at(pane_rect);

    // Mouse selection — record pixel positions first, convert to grid coords later
    let mut drag_start_pos: Option<egui::Pos2> = None;
    let mut drag_end_pos: Option<egui::Pos2> = None;
    let mut triple_click_pos: Option<egui::Pos2> = None;
    let mut double_click_pos: Option<egui::Pos2> = None;

    let mouse_state = ctx.input(|i| (
        i.pointer.primary_pressed(),
        i.pointer.primary_down(),
        i.pointer.primary_released(),
        i.pointer.hover_pos(),
        i.pointer.is_moving(),
    ));
    let (primary_pressed, primary_down, primary_released, hover_pos, _is_moving) = mouse_state;

    let _was_dragging = session.selection.active;
    let is_hovering = response.hovered();

    if primary_pressed && is_hovering {
        if let Some(pos) = hover_pos {
            if rect.contains(pos) {
                drag_start_pos = Some(pos);
                drag_end_pos = Some(pos);
                session.selection.active = true;
            }
        }
    }

    if primary_down && session.selection.active {
        if let Some(pos) = hover_pos {
            // Track drag anywhere for proper multi-frame selection
            let clamped_x = pos.x.clamp(rect.min.x, rect.max.x);
            // Don't clamp Y - let pixel_to_cell handle positions outside the rect
            drag_end_pos = Some(egui::pos2(clamped_x, pos.y));

            // Auto-scroll when dragging near or beyond viewport edges
            let edge_margin = line_height * 3.0;
            if pos.y < rect.min.y + edge_margin {
                // Dragging above viewport - scroll into scrollback
                let distance = ((rect.min.y + edge_margin - pos.y) / edge_margin).max(0.0);
                let scroll_speed = (distance * 2.0).ceil() as usize;
                let max_offset = session.grid.lock().map(|g| g.scrollback_len()).unwrap_or(0);
                session.scroll_offset = (session.scroll_offset + scroll_speed).min(max_offset);
            } else if pos.y > rect.max.y - edge_margin {
                // Dragging below viewport - scroll down (if applicable)
                let distance = ((pos.y - (rect.max.y - edge_margin)) / edge_margin).max(0.0);
                let scroll_speed = (distance * 2.0).ceil() as usize;
                session.scroll_offset = session.scroll_offset.saturating_sub(scroll_speed);
            }
        }
    }

    if primary_released {
        session.selection.active = false;
    }

    if response.triple_clicked() {
        triple_click_pos = response.interact_pointer_pos();
    } else if response.double_clicked() {
        double_click_pos = response.interact_pointer_pos();
    } else if response.clicked() {
        if session.selection.has_selection() {
            session.selection.clear();
        }
    }

    // IME cursor position output
    if is_focused && response.has_focus() {
        if let Ok(grid) = session.grid.lock() {
            let cursor_x = rect.min.x + grid.cursor_col as f32 * char_width;
            let cursor_y = rect.min.y + grid.cursor_row as f32 * line_height;
            let cursor_rect = egui::Rect::from_min_size(
                egui::pos2(cursor_x, cursor_y),
                egui::vec2(char_width, line_height),
            );
            ctx.output_mut(|o| {
                o.ime = Some(egui::output::IMEOutput { rect: cursor_rect, cursor_rect });
            });
        }
    }

    // Mouse wheel scrolling
    if response.hovered() {
        let scroll_delta = ctx.input(|i| i.smooth_scroll_delta.y);
        if scroll_delta != 0.0 {
            let scroll_lines = if scroll_delta.abs() > 10.0 {
                (scroll_delta / line_height).round() as i32
            } else if scroll_delta > 0.0 { 3 } else { -3 };
            let max_offset = session.grid.lock().map(|g| g.scrollback_len()).unwrap_or(0);
            if scroll_lines > 0 {
                session.scroll_offset = (session.scroll_offset + scroll_lines as usize).min(max_offset);
            } else {
                session.scroll_offset = session.scroll_offset.saturating_sub((-scroll_lines) as usize);
            }
        }
    }

    // Extract selected text for copy operations
    let selected_text: Option<String> = if session.selection.has_selection() {
        if let Ok(grid) = session.grid.lock() {
            let scrollback_len = grid.scrollback_len();
            let ((sr, sc), (er, ec)) = session.selection.ordered();
            let mut text = String::new();

            for row in sr..=er {
                let cells = if row < scrollback_len {
                    match grid.get_scrollback_row(row) {
                        Some(c) => c,
                        None => break,
                    }
                } else {
                    let grid_row = row.saturating_sub(scrollback_len);
                    if grid_row >= grid.rows { break; }
                    &grid.cells[grid_row]
                };

                let col_start = if row == sr { sc } else { 0 };
                let col_end = (if row == er { ec + 1 } else { grid.cols }).min(grid.cols);

                for col in col_start..col_end {
                    let cell = &cells[col];
                    if !cell.wide_continuation {
                        text.push(cell.c);
                    }
                }

                if row != er {
                    let trimmed = text.trim_end().len();
                    text.truncate(trimmed);
                    text.push('\n');
                }
            }
            Some(text.trim_end().to_owned())
        } else { None }
    } else { None };

    // Right-click context menu
    let mut ctx_copy = false;
    let mut ctx_paste = false;
    let mut ctx_select_all = false;
    let mut ctx_split_h = false;
    let mut ctx_split_v = false;
    let mut ctx_close = false;
    response.context_menu(|ui| {
        if ui.add_enabled(selected_text.is_some(), egui::Button::new("Copy")).clicked() {
            ctx_copy = true;
            ui.close_menu();
        }
        if ui.button("Paste").clicked() {
            ctx_paste = true;
            ui.close_menu();
        }
        ui.separator();
        if ui.button("Select All").clicked() {
            ctx_select_all = true;
            ui.close_menu();
        }
        ui.separator();
        if ui.button("Split Horizontally\t⌘D").clicked() {
            ctx_split_h = true;
            ui.close_menu();
        }
        if ui.button("Split Vertically\t⌘⇧D").clicked() {
            ctx_split_v = true;
            ui.close_menu();
        }
        if ui.add_enabled(can_close_pane, egui::Button::new("Close Pane")).clicked() {
            ctx_close = true;
            ui.close_menu();
        }
    });

    if ctx_copy {
        if let Some(ref text) = selected_text {
            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                let _ = clipboard.set_text(text.clone());
            }
            session.selection.clear();
        }
    }
    if ctx_paste {
        let clip_text = ctx.input(|i| i.events.iter().find_map(|e| {
            if let egui::Event::Paste(t) = e { Some(t.clone()) } else { None }
        }));
        if let Some(text) = clip_text {
            let safe_text: String = text.chars()
                .filter(|c| *c == '\t' || *c == '\n' || *c == '\r' || !c.is_control())
                .collect();
            session.write(&safe_text);
            input_bytes.extend_from_slice(safe_text.as_bytes());
        } else if let Ok(mut clipboard) = arboard::Clipboard::new() {
            if let Ok(text) = clipboard.get_text() {
                session.write(&text);
                input_bytes.extend_from_slice(text.as_bytes());
            }
        }
    }
    if ctx_select_all {
        session.selection.start = (0, 0);
        session.selection.end = (new_rows.saturating_sub(1), new_cols.saturating_sub(1));
    }
    if ctx_split_h { action = Some(PaneAction::SplitHorizontal); }
    if ctx_split_v { action = Some(PaneAction::SplitVertical); }
    if ctx_close   { action = Some(PaneAction::ClosePane); }

    // Keyboard input — only for the focused pane
    let search_is_active = session.search_state.is_some();
    if is_focused && response.has_focus() {
        let events: Vec<egui::Event> = ctx.input(|i| i.events.clone());
        let has_input_events = events.iter().any(|e| matches!(e,
            egui::Event::Key { pressed: true, .. }
            | egui::Event::Ime(_)
            | egui::Event::Paste(_)
        ));
        if has_input_events && !search_is_active {
            session.scroll_offset = 0;
        }

        if search_is_active {
            for event in &events {
                if let egui::Event::Key { key, pressed: true, modifiers, .. } = event {
                    match key {
                        egui::Key::Escape => {
                            session.search_state = None;
                        }
                        egui::Key::Enter if modifiers.shift => {
                            if let Some(ref mut state) = session.search_state {
                                if !state.matches.is_empty() {
                                    if state.current_index == 0 {
                                        state.current_index = state.matches.len() - 1;
                                    } else {
                                        state.current_index -= 1;
                                    }
                                }
                            }
                        }
                        egui::Key::Enter => {
                            if let Some(ref mut state) = session.search_state {
                                if !state.matches.is_empty() {
                                    state.current_index = (state.current_index + 1) % state.matches.len();
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        let mut ime_committed = false;
        let has_ime_events = events.iter().any(|e| matches!(e, egui::Event::Ime(_)));

        let has_non_ascii_text = events.iter().any(|e| {
            if let egui::Event::Text(text) = e {
                !text.chars().all(|c| c.is_ascii())
            } else {
                false
            }
        });

        fn is_chinese_ime_punct(ch: char) -> bool {
            matches!(ch,
                '.' | ',' | ';' | ':' |
                '?' | '!' |
                '(' | ')' |
                '[' | ']' |
                '<' | '>'
            )
        }

        if !search_is_active {

            for event in &events {
                if let egui::Event::Ime(ime_event) = event {
                    match ime_event {
                        egui::ImeEvent::Enabled => {
                            *ime_composing = true;
                        }
                        egui::ImeEvent::Preedit(text) => {
                            *ime_preedit = text.clone();
                        }
                        egui::ImeEvent::Commit(text) => {
                            ime_preedit.clear();
                            session.selection.clear();
                            let safe_text: String = text.chars()
                                .filter(|c| *c == '\t' || *c == '\n' || *c == '\r' || !c.is_control())
                                .collect();
                            session.write(&safe_text);
                            input_bytes.extend_from_slice(safe_text.as_bytes());
                            ime_committed = true;
                            session.last_non_ascii_input = !safe_text.chars().all(|c| c.is_ascii());
                            *ime_composing = false;
                        }
                        egui::ImeEvent::Disabled => {
                            ime_preedit.clear();
                            *ime_composing = false;
                        }
                    }
                }
            }

            for event in &events {
                match event {
                    egui::Event::Text(text) => {
                        // Skip Text events if IME committed text in this frame (prevents duplicates)
                        // On macOS, IME Commit and Text events arrive together for the same input
                        if ime_committed {
                            continue;
                        }
                        // Also skip if there are IME events in the current frame
                        if has_ime_events {
                            continue;
                        }
                        if *ime_composing {
                            continue;
                        }
                        if session.last_non_ascii_input {
                            let is_single_ascii_punct = text.len() == 1 &&
                                text.chars().next().map(|c| is_chinese_ime_punct(c)).unwrap_or(false);
                            if is_single_ascii_punct {
                                continue;
                            }
                        }

                        let is_punct = text.len() == 1 &&
                            text.chars().next().map(|c| is_chinese_ime_punct(c)).unwrap_or(false);

                        if !text.chars().all(|c| c.is_ascii()) {
                            // Non-ASCII text (Chinese, Japanese, Korean, etc.)
                            session.selection.clear();
                            let safe_text: String = text.chars()
                                .filter(|c| *c == '\t' || *c == '\n' || *c == '\r' || !c.is_control())
                                .collect();
                            if !safe_text.is_empty() {
                                session.write(&safe_text);
                                input_bytes.extend_from_slice(safe_text.as_bytes());
                            }
                            session.last_non_ascii_input = true;
                        } else {
                            // ASCII text (letters, numbers, symbols, spaces, punctuation)
                            // Send to terminal and update the last_non_ascii_input flag
                            session.selection.clear();
                            let safe_text: String = text.chars()
                                .filter(|c| *c == '\t' || *c == '\n' || *c == '\r' || !c.is_control())
                                .collect();
                            if !safe_text.is_empty() {
                                session.write(&safe_text);
                                input_bytes.extend_from_slice(safe_text.as_bytes());
                            }
                            // Update flag: regular ASCII text means we're no longer in IME mode
                            if !is_punct {
                                session.last_non_ascii_input = false;
                            }
                        }
                    }
                    egui::Event::Key { key, pressed: true, modifiers, .. } => {
                        if modifiers.command {
                            let cmd_arrow = match key {
                                egui::Key::ArrowLeft  => { session.write("\x01"); input_bytes.push(0x01); true }
                                egui::Key::ArrowRight => { session.write("\x05"); input_bytes.push(0x05); true }
                                _ => false,
                            };
                            if cmd_arrow {
                                session.selection.clear();
                            } else if shortcut_resolver.matches(ShortcutAction::ToggleBroadcast, ctx) {
                                action = Some(PaneAction::ToggleBroadcast);
                            } else if shortcut_resolver.matches(ShortcutAction::Copy, ctx) {
                                if let Some(ref text) = selected_text {
                                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                        let _ = clipboard.set_text(text.clone());
                                    }
                                    session.selection.clear();
                                } else {
                                    session.write_bytes(&[0x03]);
                                    input_bytes.push(0x03);
                                }
                            } else if shortcut_resolver.matches(ShortcutAction::SelectAll, ctx) {
                                session.selection.start = (0, 0);
                                session.selection.end = (new_rows.saturating_sub(1), new_cols.saturating_sub(1));
                            } else if shortcut_resolver.matches(ShortcutAction::Paste, ctx) {
                                // handled by egui::Event::Paste below
                            } else if shortcut_resolver.matches(ShortcutAction::Search, ctx) {
                                if session.search_state.is_some() {
                                    session.search_state = None;
                                } else {
                                    use crate::ui::types::session::SearchState;
                                    session.search_state = Some(SearchState {
                                        query: String::new(),
                                        matches: Vec::new(),
                                        current_index: 0,
                                        case_sensitive: false,
                                    });
                                    let search_input_id = egui::Id::new("search_input").with(pane_id);
                                    ctx.memory_mut(|mem| mem.request_focus(search_input_id));
                                }
                            }
                        } else if modifiers.ctrl {
                            if let Some(byte) = key_to_ctrl_byte(key) {
                                session.selection.clear();
                                session.write_bytes(&[byte]);
                                input_bytes.push(byte);
                            }
                        } else if modifiers.alt {
                            let alt_arrow = match key {
                                egui::Key::ArrowLeft  => { session.write("\x1bb"); input_bytes.extend_from_slice(b"\x1bb"); true }
                                egui::Key::ArrowRight => { session.write("\x1bf"); input_bytes.extend_from_slice(b"\x1bf"); true }
                                egui::Key::ArrowUp    => { session.write("\x1b[1;3A"); input_bytes.extend_from_slice(b"\x1b[1;3A"); true }
                                egui::Key::ArrowDown  => { session.write("\x1b[1;3B"); input_bytes.extend_from_slice(b"\x1b[1;3B"); true }
                                _ => false,
                            };
                            if alt_arrow {
                                session.selection.clear();
                            }
                        } else {
                            let special = match key {
                                egui::Key::Enter     => { session.write("\r"); input_bytes.extend_from_slice(b"\r"); true }
                                egui::Key::Backspace => { session.write("\x7f"); input_bytes.push(0x7f); true }
                                egui::Key::Tab       => { session.write("\t"); input_bytes.push(b'\t'); true }
                                egui::Key::Escape    => { session.write("\x1b"); input_bytes.push(0x1b); true }
                                egui::Key::ArrowUp   => { session.write("\x1b[A"); input_bytes.extend_from_slice(b"\x1b[A"); true }
                                egui::Key::ArrowDown => { session.write("\x1b[B"); input_bytes.extend_from_slice(b"\x1b[B"); true }
                                egui::Key::ArrowRight => { session.write("\x1b[C"); input_bytes.extend_from_slice(b"\x1b[C"); true }
                                egui::Key::ArrowLeft  => { session.write("\x1b[D"); input_bytes.extend_from_slice(b"\x1b[D"); true }
                                egui::Key::Home      => { session.write("\x1b[H"); input_bytes.extend_from_slice(b"\x1b[H"); true }
                                egui::Key::End       => { session.write("\x1b[F"); input_bytes.extend_from_slice(b"\x1b[F"); true }
                                egui::Key::PageUp    => { session.write("\x1b[5~"); input_bytes.extend_from_slice(b"\x1b[5~"); true }
                                egui::Key::PageDown  => { session.write("\x1b[6~"); input_bytes.extend_from_slice(b"\x1b[6~"); true }
                                egui::Key::Delete    => { session.write("\x1b[3~"); input_bytes.extend_from_slice(b"\x1b[3~"); true }
                                egui::Key::Insert    => { session.write("\x1b[2~"); input_bytes.extend_from_slice(b"\x1b[2~"); true }
                                egui::Key::F1  => { session.write("\x1bOP"); input_bytes.extend_from_slice(b"\x1bOP"); true }
                                egui::Key::F2  => { session.write("\x1bOQ"); input_bytes.extend_from_slice(b"\x1bOQ"); true }
                                egui::Key::F3  => { session.write("\x1bOR"); input_bytes.extend_from_slice(b"\x1bOR"); true }
                                egui::Key::F4  => { session.write("\x1bOS"); input_bytes.extend_from_slice(b"\x1bOS"); true }
                                egui::Key::F5  => { session.write("\x1b[15~"); input_bytes.extend_from_slice(b"\x1b[15~"); true }
                                egui::Key::F6  => { session.write("\x1b[17~"); input_bytes.extend_from_slice(b"\x1b[17~"); true }
                                egui::Key::F7  => { session.write("\x1b[18~"); input_bytes.extend_from_slice(b"\x1b[18~"); true }
                                egui::Key::F8  => { session.write("\x1b[19~"); input_bytes.extend_from_slice(b"\x1b[19~"); true }
                                egui::Key::F9  => { session.write("\x1b[20~"); input_bytes.extend_from_slice(b"\x1b[20~"); true }
                                egui::Key::F10 => { session.write("\x1b[21~"); input_bytes.extend_from_slice(b"\x1b[21~"); true }
                                egui::Key::F11 => { session.write("\x1b[23~"); input_bytes.extend_from_slice(b"\x1b[23~"); true }
                                egui::Key::F12 => { session.write("\x1b[24~"); input_bytes.extend_from_slice(b"\x1b[24~"); true }
                                _ => false,
                            };
                            if special {
                                session.selection.clear();
                            } else if !*ime_composing && !ime_committed {
                                if let Some(ch) = key_to_char(key, modifiers.shift) {
                                    let is_ime_punct = is_chinese_ime_punct(ch);
                                    if is_ime_punct && (session.last_non_ascii_input || has_non_ascii_text) {
                                        // suppress duplicate punctuation from IME
                                    } else {
                                        if !is_ime_punct {
                                            session.last_non_ascii_input = false;
                                        }
                                        session.selection.clear();
                                        let s = ch.to_string();
                                        session.write(&s);
                                        input_bytes.extend_from_slice(s.as_bytes());
                                    }
                                }
                            }
                        }
                    }
                    egui::Event::Copy => {
                        if let Some(ref text) = selected_text {
                            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                let _ = clipboard.set_text(text.clone());
                            }
                            session.selection.clear();
                        } else {
                            session.write_bytes(&[0x03]);
                            input_bytes.push(0x03);
                        }
                    }
                    egui::Event::Paste(text) => {
                        session.selection.clear();
                        let safe_text: String = text.chars()
                            .filter(|c| *c == '\t' || *c == '\n' || *c == '\r' || !c.is_control())
                            .collect();
                        session.write(&safe_text);
                        input_bytes.extend_from_slice(safe_text.as_bytes());
                    }
                    _ => {}
                }
            }

        } // end if !search_is_active
    }

    // ── Render terminal content ──────────────────────────────────────────────
    if let Ok(grid) = session.grid.lock() {
        painter.rect_filled(pane_rect, 0.0, theme.bg_primary);
        let scrollback_len = grid.scrollback_len();

        // ── FIX 4: auto-follow PTY output ────────────────────────────────────
        //
        // When the user has scrolled up (scroll_offset > 0) and the PTY
        // produces new output (scrollback_len grows), all mainstream terminals
        // (iTerm2, Terminal.app, kitty) snap the view back to the bottom so
        // the user sees the live output.  Without this, after `cat FILE` the
        // viewport stays wherever the user left it, hiding the prompt and cursor.
        //
        // We detect new output by storing the last-seen scrollback_len in egui
        // Memory (keyed per pane_id) and comparing each frame.  If it grew,
        // the PTY wrote new content — reset scroll_offset to 0.
        //
        // Exception: if the user is actively dragging a selection we leave the
        // viewport alone so the drag isn't interrupted.
        if session.scroll_offset > 0 && !session.selection.active {
            let sb_key = pane_id.with("last_scrollback_len");
            let last_sb: usize = ctx.data(|d| d.get_temp(sb_key).unwrap_or(0));
            if scrollback_len > last_sb {
                // New PTY output arrived — snap back to the live view
                session.scroll_offset = 0;
            }
        }
        // Always update the stored scrollback_len for next frame
        {
            let sb_key = pane_id.with("last_scrollback_len");
            ctx.data_mut(|d| d.insert_temp(sb_key, scrollback_len));
        }

        let offset = session.scroll_offset;

        // Convert pixel position to global grid index.
        // Returns absolute index: 0..scrollback_len-1 for scrollback, scrollback_len.. for active grid
        let pixel_to_cell = |pos: egui::Pos2| -> (usize, usize) {
            let screen_row_float = (pos.y - rect.min.y) / line_height;

            // Position above viewport - select from scrollback
            if screen_row_float < 0.0 {
                let rows_above = (-screen_row_float).ceil() as usize;
                // When above viewport with offset, we're selecting deeper into scrollback
                let sb_idx = scrollback_len.saturating_sub(offset + rows_above);
                return (sb_idx, 0);
            }

            let screen_row = screen_row_float as usize;

            // Calculate absolute grid row index
            let grid_row_idx = if screen_row < new_rows {
                // Within viewport - apply offset to get absolute position
                if offset > 0 && screen_row < offset {
                    // Viewport is showing scrollback content
                    let sb_idx = scrollback_len.saturating_sub(offset) + screen_row;
                    sb_idx
                } else {
                    // Viewport is showing active grid content
                    let grid_row = screen_row.saturating_sub(offset);
                    let grid_row = grid_row.min(grid.rows.saturating_sub(1));
                    scrollback_len + grid_row
                }
            } else {
                // Below viewport - extend to bottom of content
                scrollback_len + grid.rows.saturating_sub(1)
            };

            let x_in_row = (pos.x - rect.min.x).max(0.0);
            let col = (x_in_row / char_width).floor() as usize;
            let col = col.min(grid.cols.saturating_sub(1));

            (grid_row_idx, col)
        };

        // Process mouse events inside grid lock
        if let Some(pos) = drag_start_pos {
            let (grid_row, grid_col) = pixel_to_cell(pos);
            session.selection.start = (grid_row, grid_col);
            session.selection.end = (grid_row, grid_col);
        }
        if session.selection.active {
            if let Some(pos) = drag_end_pos {
                let (grid_row, grid_col) = pixel_to_cell(pos);
                session.selection.end = (grid_row, grid_col);
            }
        }
        if let Some(pos) = triple_click_pos {
            let (grid_row, _) = pixel_to_cell(pos);
            session.selection.start = (grid_row, 0);
            session.selection.end = (grid_row, new_cols.saturating_sub(1));
        }
        if let Some(pos) = double_click_pos {
            let (grid_row, grid_col) = pixel_to_cell(pos);
            let local_row = if grid_row < scrollback_len {
                grid_row
            } else {
                grid_row.saturating_sub(scrollback_len)
            };
            let (word_start, word_end) = find_word_boundaries(&grid, local_row, grid_col);
            session.selection.start = (grid_row, word_start);
            session.selection.end = (grid_row, word_end);
        }

        // ── Text rendering ───────────────────────────────────────────────────
        for screen_row in 0..grid.rows.min(new_rows) {
            let row_y = rect.min.y + screen_row as f32 * line_height;

            let cells: Option<&Vec<terminal::TerminalCell>> = if offset > 0 && screen_row < offset {
                let sb_idx = scrollback_len.saturating_sub(offset) + screen_row;
                grid.get_scrollback_row(sb_idx)
            } else {
                let grid_row = screen_row.saturating_sub(offset);
                if grid_row < grid.rows { Some(&grid.cells[grid_row]) } else { None }
            };

            let Some(cells) = cells else { continue; };
            let row_cols = grid.cols.min(cells.len());

            let mut col = 0;
            while col < row_cols {
                let cell = &cells[col];
                // Skip continuation cells — their owning wide char handles the pixel width
                if cell.wide_continuation { col += 1; continue; }

                let (fg, bg) = if cell.inverse {
                    (cell.bg_color, cell.fg_color)
                } else {
                    (cell.fg_color, cell.bg_color)
                };

                let run_start_col = col;
                let run_fg = fg;
                let run_bg = bg;
                let run_bold = cell.bold;
                let run_italic = cell.italic;
                let run_underline = cell.underline;
                let run_strikethrough = cell.strikethrough;
                let run_dim = cell.dim;
                let run_inverse = cell.inverse;
                let mut run_text = String::new();
                let mut run_end_col = col;

                // Determine if the first cell of this run is a wide (CJK) character.
                // Wide chars always form a solo run — they must start at an exact
                // grid column pixel boundary and must not be mixed with adjacent ASCII.
                let first_is_wide = col + 1 < row_cols && cells[col + 1].wide_continuation;

                // Accumulate characters into a run.
                // Invariant: all chars in the run share the same visual style.
                // CJK (wide) chars always terminate the run immediately after being
                // added — they are never combined with preceding or following chars.
                while run_end_col < row_cols {
                    let c = &cells[run_end_col];
                    // Continuation cells carry no character — skip over them but count
                    // their column so run_end_col advances to the next real cell.
                    if c.wide_continuation { run_end_col += 1; continue; }

                    let is_wide = run_end_col + 1 < row_cols
                        && cells[run_end_col + 1].wide_continuation;

                    // If we have already accumulated at least one ASCII character and
                    // the next character is wide, flush the ASCII run first so the
                    // wide char can start its own run at the correct pixel column.
                    if is_wide && run_end_col > run_start_col {
                        break;
                    }

                    let (c_fg, c_bg) = if c.inverse {
                        (c.bg_color, c.fg_color)
                    } else {
                        (c.fg_color, c.bg_color)
                    };

                    // Style break — start a new run
                    if run_end_col > run_start_col &&
                        (c_fg != run_fg || c_bg != run_bg ||
                            c.bold != run_bold || c.italic != run_italic ||
                            c.underline != run_underline || c.strikethrough != run_strikethrough ||
                            c.dim != run_dim || c.inverse != run_inverse) {
                        break;
                    }

                    run_text.push(c.c);
                    run_end_col += 1;
                    // Advance past any continuation cells so run_end_col points at
                    // the next non-continuation cell (or row_cols if at end).
                    while run_end_col < row_cols && cells[run_end_col].wide_continuation {
                        run_end_col += 1;
                    }

                    // Wide char: isolate it as a solo run and stop here.
                    if is_wide {
                        break;
                    }
                }

                let cell_x = rect.min.x + run_start_col as f32 * char_width;
                let run_cell_width = (run_end_col - run_start_col) as f32 * char_width;

                // Background fill
                if run_bg != terminal::DEFAULT_BG {
                    let bg_color = egui::Color32::from_rgb(run_bg.0, run_bg.1, run_bg.2);
                    let bg_rect = egui::Rect::from_min_size(
                        egui::pos2(cell_x, row_y),
                        egui::vec2(run_cell_width, line_height),
                    );
                    painter.rect_filled(bg_rect, 0.0, bg_color);
                }

                // Text rendering
                let has_visible = run_text.chars().any(|c| c != ' ' && c != '\0');
                if has_visible {
                    let fg_color = if run_dim {
                        egui::Color32::from_rgba_unmultiplied(run_fg.0, run_fg.1, run_fg.2, 128)
                    } else {
                        egui::Color32::from_rgb(run_fg.0, run_fg.1, run_fg.2)
                    };

                    // Clip rect: prevents CJK glyphs whose advance width differs
                    // from 2×char_width from visually overflowing into adjacent cells.
                    // Combined with per-cell origin positioning this is the second
                    // line of defence against glyph drift.
                    let run_clip = painter.clip_rect().intersect(egui::Rect::from_min_size(
                        egui::pos2(cell_x, row_y),
                        egui::vec2(run_cell_width, line_height),
                    ));
                    let clipped_painter = painter.with_clip_rect(run_clip);

                    if first_is_wide {
                        // ── FIX 2: CJK wide-char run ────────────────────────────
                        //
                        // painter.text() with LEFT_CENTER places the glyph at
                        // (x + left_bearing, ...). For CJK fallback fonts this
                        // bearing is non-zero and varies between fonts, causing the
                        // visual position to diverge from cell_x. Switching to
                        // layout_no_wrap + galley() lets us control the *layout*
                        // origin precisely: egui will place the first glyph at the
                        // given pos2, ignoring any bearing offset that would otherwise
                        // shift the character right relative to the grid.
                        //
                        // This fixes the accumulating left-drift seen on selection
                        // highlights and the cursor for lines containing CJK chars.
                        let galley = ui.fonts(|f| {
                            f.layout_no_wrap(
                                run_text.clone(),
                                font_id.clone(),
                                fg_color,
                            )
                        });
                        // Vertically centre the galley within the cell row
                        let gy = row_y + (line_height - galley.size().y) * 0.5;
                        clipped_painter.galley(egui::pos2(cell_x, gy), galley, fg_color);
                    } else {
                        // ASCII / narrow run — painter.text is fine here
                        let text_y = row_y + line_height / 2.0;
                        clipped_painter.text(
                            egui::pos2(cell_x, text_y),
                            egui::Align2::LEFT_CENTER,
                            &run_text,
                            font_id.clone(),
                            fg_color,
                        );
                    }
                }

                // Underline
                if run_underline {
                    let fg_color = if run_dim {
                        egui::Color32::from_rgba_unmultiplied(run_fg.0, run_fg.1, run_fg.2, 128)
                    } else {
                        egui::Color32::from_rgb(run_fg.0, run_fg.1, run_fg.2)
                    };
                    let underline_y = row_y + line_height - 1.0;
                    painter.line_segment(
                        [egui::pos2(cell_x, underline_y), egui::pos2(cell_x + run_cell_width, underline_y)],
                        egui::Stroke::new(1.0, fg_color),
                    );
                }

                // Strikethrough
                if run_strikethrough {
                    let fg_color = if run_dim {
                        egui::Color32::from_rgba_unmultiplied(run_fg.0, run_fg.1, run_fg.2, 128)
                    } else {
                        egui::Color32::from_rgb(run_fg.0, run_fg.1, run_fg.2)
                    };
                    let strike_y = row_y + line_height / 2.0;
                    painter.line_segment(
                        [egui::pos2(cell_x, strike_y), egui::pos2(cell_x + run_cell_width, strike_y)],
                        egui::Stroke::new(1.0, fg_color),
                    );
                }

                col = run_end_col;
            }
        }

        // ── Selection highlight ──────────────────────────────────────────────
        if session.selection.has_selection() {
            let ((sr, sc), (er, ec)) = session.selection.ordered();
            let sel_color = theme.accent_alpha(60);

            for sel_row in sr..=er {
                let (grid_row_idx, is_scrollback) = if offset > 0 && sel_row < scrollback_len {
                    (sel_row, true)
                } else {
                    (sel_row.saturating_sub(scrollback_len), false)
                };

                let screen_row = if is_scrollback {
                    if sel_row >= scrollback_len { continue; }
                    // Calculate scrollback display position
                    // When offset > 0, viewport shows scrollback[scrollback_len-offset..scrollback_len-1]
                    // sel_row is an absolute scrollback index (0 = oldest)
                    // Need to map sel_row to its display position
                    let scrollback_visible_start = scrollback_len.saturating_sub(offset);
                    if sel_row < scrollback_visible_start { continue; } // Not in visible portion
                    sel_row - scrollback_visible_start
                } else {
                    if grid_row_idx >= grid.rows { break; }
                    grid_row_idx + offset
                };

                if screen_row >= new_rows {
                    if is_scrollback { continue; } else { break; }
                }

                let row_cols = if is_scrollback {
                    grid.get_scrollback_row(sel_row).map(|c| c.len().min(grid.cols)).unwrap_or(grid.cols)
                } else {
                    grid.cells[grid_row_idx].len().min(grid.cols)
                };

                let col_start = if sel_row == sr { sc.min(row_cols) } else { 0 };
                let col_end = if sel_row == er { (ec + 1).min(row_cols) } else { row_cols };
                if col_start >= col_end { continue; }

                let start_x = rect.min.x + col_start as f32 * char_width;
                let end_x   = rect.min.x + col_end   as f32 * char_width;
                let row_y   = rect.min.y + screen_row as f32 * line_height;

                let sel_rect = egui::Rect::from_min_max(
                    egui::pos2(start_x, row_y),
                    egui::pos2(end_x, row_y + line_height),
                );
                painter.rect_filled(sel_rect, 0.0, sel_color);
            }
        }

        // ── Search match highlighting ─────────────────────────────────────────
        if let Some(ref search_state) = session.search_state {
            let match_color   = egui::Color32::from_rgba_premultiplied(255, 220,  50,  80);
            let current_color = egui::Color32::from_rgba_premultiplied(255, 140,   0, 120);

            for (match_idx, m) in search_state.matches.iter().enumerate() {
                let is_current = match_idx == search_state.current_index;

                let (grid_row_idx, is_scrollback) = if m.row < scrollback_len {
                    (m.row, true)
                } else {
                    (m.row.saturating_sub(scrollback_len), false)
                };

                let screen_row = if is_scrollback {
                    if m.row >= scrollback_len { continue; }
                    let vis_start = scrollback_len.saturating_sub(offset);
                    if m.row < vis_start || m.row >= vis_start + offset.min(new_rows) { continue; }
                    m.row - vis_start
                } else {
                    if grid_row_idx >= grid.rows { continue; }
                    grid_row_idx + offset
                };

                if screen_row >= new_rows { continue; }

                let col_end_clamped   = m.col_end.min(grid.cols);
                let col_start_clamped = m.col_start.min(col_end_clamped);
                if col_start_clamped >= col_end_clamped { continue; }

                let start_x = rect.min.x + col_start_clamped as f32 * char_width;
                let end_x   = rect.min.x + col_end_clamped   as f32 * char_width;
                let row_y   = rect.min.y + screen_row         as f32 * line_height;

                let highlight_rect = egui::Rect::from_min_max(
                    egui::pos2(start_x, row_y),
                    egui::pos2(end_x, row_y + line_height),
                );
                painter.rect_filled(highlight_rect, 0.0, if is_current { current_color } else { match_color });
            }
        }

        // ── Cursor ───────────────────────────────────────────────────────────
        let ssh_not_connected = matches!(&session.session, Some(SessionBackend::Ssh(ssh))
            if !matches!(ssh.connection_state(), SshConnectionState::Connected));
        if !ssh_not_connected && offset == 0
            && grid.cursor_visible
            && grid.cursor_col < grid.cols
            && grid.cursor_row < grid.rows
        {
            let cursor_x = rect.min.x + (grid.cursor_col as f32) * char_width;
            let cursor_y = rect.min.y + grid.cursor_row as f32 * line_height;
            let cursor_top    = egui::pos2(cursor_x, cursor_y);
            let cursor_bottom = egui::pos2(cursor_x, cursor_y + line_height);

            if is_focused && response.has_focus() && !search_has_focus {
                // FIX 5: request repaint so cursor blink stays animated.
                //
                // egui only repaints on events. Without an explicit repaint
                // request the blink phase freezes on the frame of the last
                // input event — often the dark (off) phase — making the cursor
                // appear missing even when the pane is focused and active.
                // Requesting a repaint after 250 ms (half the 500 ms blink
                // period) keeps the animation alive at minimal GPU cost.
                ctx.request_repaint_after(std::time::Duration::from_millis(250));

                let blink_on = (ctx.input(|i| i.time) * 2.0) as u64 % 2 == 0;
                if blink_on {
                    painter.line_segment(
                        [cursor_top, cursor_bottom],
                        egui::Stroke::new(2.0, theme.cursor_color),
                    );
                }
            } else {
                // FIX 6: unfocused cursor — visible but clearly dimmer than
                // the active cursor. The previous fg_dim color was nearly
                // invisible against most terminal backgrounds.
                let dim_cursor = {
                    let c = theme.cursor_color;
                    egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 140)
                };
                painter.line_segment(
                    [cursor_top, cursor_bottom],
                    egui::Stroke::new(1.5, dim_cursor),
                );
            }
        }

        // ── IME preedit overlay ───────────────────────────────────────────────
        if is_focused && !ime_preedit.is_empty()
            && grid.cursor_col < grid.cols
            && grid.cursor_row < grid.rows
        {
            let safe_preedit: String = ime_preedit.chars()
                .filter(|c| !c.is_control())
                .collect();

            if !safe_preedit.is_empty() {
                let base_x = rect.min.x + grid.cursor_col as f32 * char_width;
                let py = rect.min.y + grid.cursor_row as f32 * line_height;
                let galley = ui.fonts(|f| f.layout_no_wrap(safe_preedit, font_id.clone(), theme.accent));
                let bg_rect = egui::Rect::from_min_size(
                    egui::pos2(base_x, py),
                    galley.size() + egui::vec2(4.0, 0.0),
                );
                painter.rect_filled(bg_rect, 2.0, theme.bg_elevated);
                painter.galley(egui::pos2(base_x + 2.0, py), galley, egui::Color32::TRANSPARENT);
            }
        }

        // ── Scrollback indicator ──────────────────────────────────────────────
        if offset > 0 {
            let indicator = language.tf("lines_above", &offset.to_string());
            let galley = ui.fonts(|f| f.layout_no_wrap(
                indicator,
                egui::FontId::monospace(11.0),
                theme.fg_dim,
            ));
            let ind_x = rect.max.x - galley.rect.width() - 12.0;
            let ind_y = rect.min.y + 4.0;
            painter.rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(ind_x - 4.0, ind_y - 2.0),
                    egui::vec2(galley.rect.width() + 8.0, galley.rect.height() + 4.0),
                ),
                4.0,
                egui::Color32::from_rgba_premultiplied(
                    theme.bg_secondary.r(), theme.bg_secondary.g(), theme.bg_secondary.b(), 200,
                ),
            );
            painter.galley(egui::pos2(ind_x, ind_y), galley, egui::Color32::TRANSPARENT);
        }

        // ── SSH connection state overlay ──────────────────────────────────────
        if let Some(SessionBackend::Ssh(ssh)) = &session.session {
            match ssh.connection_state() {
                SshConnectionState::Connecting | SshConnectionState::Authenticating => {
                    painter.rect_filled(rect, 0.0, egui::Color32::from_rgba_premultiplied(
                        theme.bg_primary.r(), theme.bg_primary.g(), theme.bg_primary.b(), 240,
                    ));
                    let msg = match ssh.connection_state() {
                        SshConnectionState::Connecting    => language.t("connecting"),
                        SshConnectionState::Authenticating => language.t("authenticating"),
                        _ => "",
                    };
                    let galley = ui.fonts(|f| f.layout_no_wrap(
                        msg.to_string(),
                        egui::FontId::monospace(16.0),
                        theme.accent,
                    ));
                    painter.galley(
                        egui::pos2(
                            rect.center().x - galley.rect.width() / 2.0,
                            rect.center().y - galley.rect.height() / 2.0 - 14.0,
                        ),
                        galley,
                        egui::Color32::TRANSPARENT,
                    );

                    let btn_size = egui::vec2(70.0, 28.0);
                    let btn_pos  = egui::pos2(rect.center().x - btn_size.x / 2.0, rect.center().y + 10.0);
                    let btn_rect = egui::Rect::from_min_size(btn_pos, btn_size);
                    let btn_resp = ui.allocate_rect(btn_rect, egui::Sense::click());
                    let btn_bg = if btn_resp.hovered() { theme.bg_elevated } else { theme.bg_secondary };
                    painter.rect(btn_rect, 4.0, btn_bg, egui::Stroke::new(1.0, theme.border));
                    let cancel_galley = ui.fonts(|f| f.layout_no_wrap(
                        language.t("cancel").to_string(),
                        egui::FontId::proportional(12.0),
                        theme.red,
                    ));
                    painter.galley(
                        egui::pos2(
                            btn_rect.center().x - cancel_galley.rect.width() / 2.0,
                            btn_rect.center().y - cancel_galley.rect.height() / 2.0,
                        ),
                        cancel_galley,
                        egui::Color32::TRANSPARENT,
                    );
                    if btn_resp.clicked() {
                        action = Some(PaneAction::ClosePane);
                    }
                }
                SshConnectionState::Error(ref err) => {
                    painter.rect_filled(rect, 0.0, egui::Color32::from_rgba_premultiplied(
                        theme.bg_primary.r(), theme.bg_primary.g(), theme.bg_primary.b(), 220,
                    ));
                    let safe_err: String = err.chars().filter(|c| !c.is_control()).collect();
                    let msg = language.tf("ssh_error", &safe_err);
                    let is_key_error = err.contains("Host key verification failed")
                        || err.contains("MITM attack");
                    let galley = ui.fonts(|f| f.layout(
                        msg,
                        egui::FontId::monospace(13.0),
                        theme.red,
                        rect.width() * 0.85,
                    ));
                    let text_y = rect.center().y - galley.rect.height() / 2.0 - 10.0;
                    painter.galley(
                        egui::pos2(rect.center().x - galley.rect.width() / 2.0, text_y),
                        galley,
                        egui::Color32::TRANSPARENT,
                    );
                    if is_key_error {
                        let btn_size = egui::vec2(120.0, 28.0);
                        let btn_pos  = egui::pos2(
                            rect.center().x - btn_size.x / 2.0,
                            rect.center().y + 10.0,
                        );
                        let btn_rect = egui::Rect::from_min_size(btn_pos, btn_size);
                        let btn_resp = ui.allocate_rect(btn_rect, egui::Sense::click());
                        painter.rect_filled(btn_rect, 4.0, theme.bg_elevated);
                        painter.rect_stroke(btn_rect, 4.0, egui::Stroke::new(1.0, theme.red));
                        let btn_galley = ui.fonts(|f| f.layout_no_wrap(
                            "Remove old key".to_string(),
                            egui::FontId::proportional(12.0),
                            theme.fg_primary,
                        ));
                        painter.galley(
                            egui::pos2(
                                btn_rect.center().x - btn_galley.rect.width() / 2.0,
                                btn_rect.center().y - btn_galley.rect.height() / 2.0,
                            ),
                            btn_galley,
                            egui::Color32::TRANSPARENT,
                        );
                        if btn_resp.clicked() {
                            action = Some(PaneAction::RemoveHostKey);
                        }
                    }
                }
                SshConnectionState::Disconnected(ref reason) => {
                    let safe_reason: String = reason.chars().filter(|c| !c.is_control()).collect();
                    let galley = ui.fonts(|f| f.layout_no_wrap(
                        language.tf("disconnected", &safe_reason),
                        egui::FontId::monospace(11.0),
                        theme.red,
                    ));
                    painter.galley(
                        egui::pos2(rect.min.x + 4.0, rect.max.y - galley.rect.height() - 4.0),
                        galley,
                        egui::Color32::TRANSPARENT,
                    );
                }
                SshConnectionState::Connected => {}
            }
        }
    } // end grid.lock()

    // ── Search bar overlay ────────────────────────────────────────────────────
    if session.search_state.is_some() {
        let font_size_sb  = 13.0;
        let line_height_sb = font_size_sb * 1.5;
        let padding = 6.0;
        let search_bar_height = line_height_sb + padding * 2.0;
        let search_bar_width  = 280.0_f32;
        let search_bar_margin = 8.0_f32;

        let search_bar_rect = egui::Rect::from_min_size(
            egui::pos2(
                pane_rect.max.x - search_bar_width - search_bar_margin,
                pane_rect.min.y + search_bar_margin,
            ),
            egui::vec2(search_bar_width, search_bar_height),
        );

        let bg          = theme.bg_elevated;
        let border      = theme.border;
        let text_color  = theme.fg_primary;
        let muted_color = theme.fg_dim;
        let accent      = theme.accent;

        let btn_size   = line_height_sb - 2.0;
        let spacing    = 3.0;
        let count_width = 70.0;

        let input_width = search_bar_width - padding * 2.0
            - count_width - btn_size * 3.0 - spacing * 3.0;
        let input_rect = egui::Rect::from_min_size(
            egui::pos2(search_bar_rect.min.x + padding, search_bar_rect.min.y + padding),
            egui::vec2(input_width, line_height_sb),
        );

        let mut query = session.search_state.as_ref().map(|s| s.query.clone()).unwrap_or_default();
        let prev_query = query.clone();

        ui.memory_mut(|mem| mem.request_focus(search_input_id));
        let has_focus = search_has_focus;

        let count_x = input_rect.max.x + spacing;
        let btn_x   = count_x + count_width + spacing;
        let btn_y   = input_rect.min.y;

        let prev_rect  = egui::Rect::from_min_size(egui::pos2(btn_x,                    btn_y), egui::vec2(btn_size, line_height_sb));
        let next_rect  = egui::Rect::from_min_size(egui::pos2(btn_x + btn_size + spacing, btn_y), egui::vec2(btn_size, line_height_sb));
        let close_rect = egui::Rect::from_min_size(egui::pos2(btn_x + (btn_size + spacing) * 2.0, btn_y), egui::vec2(btn_size, line_height_sb));

        let prev_response  = ui.allocate_rect(prev_rect,  egui::Sense::click());
        let next_response  = ui.allocate_rect(next_rect,  egui::Sense::click());
        let close_response = ui.allocate_rect(close_rect, egui::Sense::click());

        let painter = ui.painter_at(pane_rect);
        painter.rect_filled(search_bar_rect, 4.0, bg);
        painter.rect_stroke(search_bar_rect, 4.0, egui::Stroke::new(1.0, border));
        if has_focus {
            painter.rect_stroke(search_bar_rect, 4.0, egui::Stroke::new(2.0, accent));
        }

        let text_edit = egui::TextEdit::singleline(&mut query)
            .id(search_input_id)
            .font(egui::FontId::proportional(font_size_sb))
            .desired_width(f32::INFINITY)
            .hint_text(language.t("search_placeholder"))
            .text_color(text_color)
            .frame(false);
        let text_edit_response = ui.put(input_rect, text_edit);
        if text_edit_response.clicked() {
            ui.memory_mut(|mem| mem.request_focus(search_input_id));
        }
        if has_focus && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            session.search_state = None;
        }

        let match_count    = session.search_state.as_ref().map(|s| s.matches.len()).unwrap_or(0);
        let current_index  = session.search_state.as_ref().map(|s| s.current_index).unwrap_or(0);
        let query_str      = session.search_state.as_ref().map(|s| s.query.clone()).unwrap_or_default();

        if !query_str.is_empty() {
            let count_text = if match_count == 0 {
                language.t("no_matches").to_string()
            } else {
                format!("{}/{}", current_index + 1, match_count)
            };
            let count_color = if match_count == 0 { theme.red } else { muted_color };
            let count_galley = ui.fonts(|f| f.layout_no_wrap(
                count_text,
                egui::FontId::proportional(font_size_sb - 1.0),
                count_color,
            ));
            let count_y = input_rect.min.y + (line_height_sb - count_galley.rect.height()) / 2.0;
            painter.galley(egui::pos2(count_x, count_y), count_galley, egui::Color32::TRANSPARENT);
        }

        // Prev button
        if prev_response.hovered() { painter.rect_filled(prev_rect, 3.0, theme.hover_bg); }
        painter.text(prev_rect.center(), egui::Align2::CENTER_CENTER, '▲',
                     egui::FontId::proportional(font_size_sb - 3.0), muted_color);
        if prev_response.clicked() {
            if let Some(ref mut state) = session.search_state {
                if !state.matches.is_empty() {
                    state.current_index = if state.current_index == 0 {
                        state.matches.len() - 1
                    } else {
                        state.current_index - 1
                    };
                }
            }
        }

        // Next button
        if next_response.hovered() { painter.rect_filled(next_rect, 3.0, theme.hover_bg); }
        painter.text(next_rect.center(), egui::Align2::CENTER_CENTER, '▼',
                     egui::FontId::proportional(font_size_sb - 3.0), muted_color);
        if next_response.clicked() {
            if let Some(ref mut state) = session.search_state {
                if !state.matches.is_empty() {
                    state.current_index = (state.current_index + 1) % state.matches.len();
                }
            }
        }

        // Close button
        if close_response.hovered() { painter.rect_filled(close_rect, 3.0, theme.hover_bg); }
        painter.text(close_rect.center(), egui::Align2::CENTER_CENTER, '×',
                     egui::FontId::proportional(font_size_sb + 1.0), muted_color);
        if close_response.clicked() {
            session.search_state = None;
        }

        // Update search on query change
        if query != prev_query {
            if let Some(ref mut state) = session.search_state {
                state.query = query.clone();
                if let Ok(grid) = session.grid.lock() {
                    let raw_matches = grid.search(&state.query, state.case_sensitive);
                    state.matches = raw_matches.into_iter().map(|(row, col_start, col_end)| {
                        SearchMatch { row, col_start, col_end }
                    }).collect();
                    if state.matches.is_empty() {
                        state.current_index = 0;
                    } else {
                        state.current_index = state.current_index.min(state.matches.len() - 1);
                    }
                }
            }
        }

        // Auto-scroll to current match
        if let Some(ref state) = session.search_state {
            if !state.matches.is_empty() {
                let current_match = &state.matches[state.current_index];
                if let Ok(grid) = session.grid.lock() {
                    let scrollback_len = grid.scrollback_len();
                    if current_match.row < scrollback_len {
                        session.scroll_offset = scrollback_len - current_match.row;
                    } else {
                        session.scroll_offset = 0;
                    }
                }
            }
        }
    }

    // ── Broadcast mode border ─────────────────────────────────────────────────
    if broadcast_state.is_active() && !is_focused {
        let painter = ui.painter_at(pane_rect);
        painter.rect_stroke(pane_rect, 0.0, egui::Stroke::new(2.0, theme.accent));
    }

    // ── Close button (×) ──────────────────────────────────────────────────────
    if show_close_btn && response.hovered() {
        let btn_bg = if close_btn_hovered {
            egui::Color32::from_rgb(192, 77, 77)
        } else {
            egui::Color32::from_rgba_premultiplied(60, 62, 80, 220)
        };
        painter.rect_filled(close_btn_rect, 4.0, btn_bg);
        painter.text(
            close_btn_rect.center(),
            egui::Align2::CENTER_CENTER,
            "\u{00d7}",
            egui::FontId::proportional(13.0),
            egui::Color32::WHITE,
        );
    }

    (action, input_bytes)
}

/// Thin wrapper around `render_terminal_session` that binds a session index and pane ID.
#[allow(clippy::too_many_arguments)]
pub fn render_terminal_pane(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    session_idx: usize,
    sessions: &mut Vec<TerminalSession>,
    focused_session: usize,
    broadcast_state: &BroadcastState,
    ime_composing: &mut bool,
    ime_preedit: &mut String,
    pane_rect: egui::Rect,
    show_close_btn: bool,
    can_close_pane: bool,
    theme: &ThemeColors,
    font_size: f32,
    language: &Language,
    shortcut_resolver: &ShortcutResolver,
) -> (Option<PaneAction>, Vec<u8>) {
    if session_idx >= sessions.len() {
        return (None, Vec::new());
    }
    let pane_id = egui::Id::new("pane").with(session_idx);
    render_terminal_session(
        ui, ctx,
        &mut sessions[session_idx],
        session_idx,
        focused_session,
        broadcast_state,
        ime_composing, ime_preedit,
        pane_rect, pane_id,
        show_close_btn,
        can_close_pane,
        theme, font_size, language,
        shortcut_resolver,
    )
}

/// Recursively render a pane layout tree.
#[allow(clippy::too_many_arguments)]
pub fn render_pane_tree(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    node: &mut PaneNode,
    rect: egui::Rect,
    sessions: &mut Vec<TerminalSession>,
    focused_session: usize,
    broadcast_state: &BroadcastState,
    ime_composing: &mut bool,
    ime_preedit: &mut String,
    can_close_pane: bool,
    theme: &ThemeColors,
    font_size: f32,
    language: &Language,
    shortcut_resolver: &ShortcutResolver,
) -> Option<(usize, PaneAction, Vec<u8>)> {
    match node {
        PaneNode::Terminal(idx) => {
            let (action, input_bytes) = render_terminal_pane(
                ui, ctx, *idx, sessions, focused_session, broadcast_state,
                ime_composing, ime_preedit, rect,
                sessions.len() > 1,
                can_close_pane,
                theme, font_size, language,
                shortcut_resolver,
            );
            match action {
                Some(a) => Some((*idx, a, input_bytes)),
                None if !input_bytes.is_empty() => Some((*idx, PaneAction::Focus, input_bytes)),
                None => None,
            }
        }
        PaneNode::Split { direction, ratio, first, second } => {
            let dir = *direction;
            let gap = 2.0;

            let divider_rect = match dir {
                SplitDirection::Horizontal => egui::Rect::from_min_max(
                    egui::pos2(rect.min.x + rect.width() * *ratio - gap / 2.0, rect.min.y),
                    egui::pos2(rect.min.x + rect.width() * *ratio + gap / 2.0, rect.max.y),
                ),
                SplitDirection::Vertical => egui::Rect::from_min_max(
                    egui::pos2(rect.min.x, rect.min.y + rect.height() * *ratio - gap / 2.0),
                    egui::pos2(rect.max.x, rect.min.y + rect.height() * *ratio + gap / 2.0),
                ),
            };

            let divider_id = egui::Id::new("split_divider")
                .with((rect.min.x as i32, rect.min.y as i32, dir == SplitDirection::Horizontal));
            let divider_resp = ui.interact(divider_rect, divider_id, egui::Sense::drag());

            if divider_resp.dragged() {
                let delta = divider_resp.drag_delta();
                match dir {
                    SplitDirection::Horizontal => {
                        *ratio = (*ratio + delta.x / rect.width()).clamp(0.1, 0.9);
                    }
                    SplitDirection::Vertical => {
                        *ratio = (*ratio + delta.y / rect.height()).clamp(0.1, 0.9);
                    }
                }
            }

            let (rect1, rect2) = split_rect(rect, dir, *ratio);

            if divider_resp.hovered() || divider_resp.dragged() {
                let icon = match dir {
                    SplitDirection::Horizontal => egui::CursorIcon::ResizeHorizontal,
                    SplitDirection::Vertical   => egui::CursorIcon::ResizeVertical,
                };
                ctx.set_cursor_icon(icon);
            }

            let f1 = render_pane_tree(ui, ctx, first,  rect1, sessions, focused_session, broadcast_state, ime_composing, ime_preedit, can_close_pane, theme, font_size, language, shortcut_resolver);
            let f2 = render_pane_tree(ui, ctx, second, rect2, sessions, focused_session, broadcast_state, ime_composing, ime_preedit, can_close_pane, theme, font_size, language, shortcut_resolver);

            let result = match (f1, f2) {
                (Some((_, PaneAction::Focus, _)), Some(other)) => Some(other),
                (Some(a), _) => Some(a),
                (None, b)    => b,
            };

            let div_color = if divider_resp.hovered() || divider_resp.dragged() {
                theme.accent
            } else {
                theme.border
            };
            // Use painter_at to clip to the divider's rect, preventing it from appearing in overlays
            let painter = ui.painter_at(divider_rect);
            painter.rect_filled(divider_rect, 0.0, div_color);

            result
        }
    }
}
