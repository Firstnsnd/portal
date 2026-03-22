//! # Terminal Rendering System
//!
//! This module provides comprehensive terminal emulation rendering with advanced text selection,
//! mouse interaction, and multi-language support (CJK characters).
//!
//! ## Architecture Overview
//!
//! The rendering system is built on top of egui's immediate mode GUI framework and follows
//! a layered architecture:
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │   Pane Tree (split/multiple tabs)      │
//! ├─────────────────────────────────────────┤
//! │   Terminal Panes (detached windows)    │
//! ├─────────────────────────────────────────┤
//! │   Terminal Session (per pane/tab)      │
//! │   - Text rendering                     │
//! │   - Mouse selection                    │
//! │   - Keyboard input                     │
//! │   - IME support                        │
//! └─────────────────────────────────────────┘
//! ```
//!
//! ## Core Features
//!
//! ### 1. Terminal Content Rendering
//! - **VTE Parsing**: Full ANSI escape sequence support (colors, attributes, alternate screen)
//! - **Character Rendering**: Unicode support with CJK double-width character handling
//! - **Scrollback**: Historical content buffer with scroll-to-view functionality
//! - **Text Layout**: egui galley-based layout with proper font metrics
//!
//! ### 2. Text Selection System
//! - **Mouse Selection**: Click, drag, double-click (word), triple-click (line)
//! - **Unified Indexing**: Seamless selection across scrollback and current grid
//! - **CJK Word Boundaries**: Proper word detection for CJK text
//! - **Visual Feedback**: Selection highlight with accurate positioning
//!
//! ### 3. Input Handling
//! - **Keyboard Input**: Direct character input with support for all Unicode
//! - **IME Integration**: Input Method Editor for CJK composition (Chinese/Japanese/Korean)
//! - **Shortcuts**: Cmd+D (split horizontal), Cmd+Shift+D (split vertical), etc.
//! - **Copy/Paste**: Clipboard integration with selected text
//!
//! ### 4. UI Components
//! - **Split Panes**: Horizontal/vertical split with resizable dividers
//! - **Close Button**: Per-pane close functionality
//! - **Status Bar**: Connection type, shell, encoding display
//! - **Scrollback Indicator**: Shows number of lines above current view
//!
//! ## Design Decisions
//!
//! ### Why egui Immediate Mode?
//! - **Simplicity**: No retained state tree to manage
//! - **Performance**: Efficient redraw with egui's clipping and caching
//! - **Flexibility**: Easy to add custom UI elements
//!
//! ### Global Selection Index System
//! The terminal uses a **unified global index system** that combines scrollback and grid rows:
//!
//! ```text
//! scrollback_len = 100, grid.rows = 30
//!
//! Global indices:
//!   0-99:   scrollback rows (history)
//!   100-129: grid rows (current content)
//! ```
//!
//! **Benefits**:
//! - Selection can span scrollback and grid seamlessly
//! - Single coordinate system for all selection operations
//! - Simplifies selection rendering logic
//!
//! ### Text Layout Strategy
//! - **egui Galleys**: Use egui's text layout for accurate metrics
//! - **Per-Row Layout**: Build layout job for each row independently
//! - **Wide Character Handling**: Filter continuation cells for layout
//! - **Color Preserving**: Maintain per-character colors from VTE
//!
//! ## Performance Considerations
//!
//! ### Rendering Optimization
//! - **Clipping**: Only render visible rows (with scrollback offset)
//! - **Layout Caching**: egui caches galley layouts internally
//! - **Minimal Redraw**: egui's dirty rect tracking reduces unnecessary draws
//!
//! ### Memory Management
//! - **Scrollback Buffer**: Limited size (typically 1000-10000 lines)
//! - **Grid Resize**: Dynamic resizing based on pane size
//! - **Selection State**: Minimal state (start/end positions only)
//!
//! ## Coordinate Systems
//!
//! ### Screen Coordinates
//! - **Origin**: Top-left of the pane
//! - **Units**: Pixels (f32)
//! - **Usage**: Mouse events, rendering
//!
//! ### Grid Coordinates
//! - **Origin**: Top-left of terminal content
//! - **Units**: Character cells (row, col)
//! - **Usage**: Selection, cursor positioning
//!
//! ### Global Selection Coordinates
//! - **Scrollback**: [0, scrollback_len)
//! - **Grid**: [scrollback_len, scrollback_len + grid.rows)
//! - **Usage**: Unified selection storage
//!
//! ## Text Selection Index System
//!
//! ### Index Ranges
//! - **Scrollback rows**: `[0, scrollback_len)` - historical content
//! - **Grid rows**: `[scrollback_len, scrollback_len + grid.rows)` - current content
//!
//! ### Example
//!
//! ```text
//! scrollback_len = 100, grid.rows = 30
//!
//! User clicks on screen row 5 (grid area):
//!   → pixel_to_cell returns global index = 100 + 5 = 105
//!   → selection stores (105, col)
//!   → renderer knows: 105 >= 100, so it's grid row 5 (105 - 100)
//! ```
//!
//! ### Coordinate Transformations
//!
//! #### Click Detection (pixel → global index)
//! ```text
//! screen_row = (pos.y - rect.min.y) / line_height
//! if screen_row < offset:
//!     return scrollback_index  // [0, scrollback_len)
//! else:
//!     return scrollback_len + grid_row  // [scrollback_len, ...)
//! ```
//!
//! #### Selection Rendering (global index → screen position)
//! ```text
//! if sel_row < scrollback_len:
//!     render_scrollback(sel_row)
//! else:
//!     render_grid(sel_row - scrollback_len)
//! ```
//!
//! #### Word Boundary Detection (global → local)
//! ```text
//! local_row = global_row - scrollback_len  // for grid rows
//! find_word_boundaries(grid, local_row, col)
//! ```
//!
//! ## Public API
//!
//! ### Main Rendering Functions
//! - `render_terminal_session()`: Core terminal session renderer
//! - `render_terminal_pane()`: Pane wrapper with UI chrome
//! - `render_pane_tree()`: Recursive pane tree renderer
//!
//! ### Utility Functions
//! - `find_word_boundaries()`: Word boundary detection for selection
//! - `build_row_layout()`: Build egui layout job for terminal row
//!
//! ## Usage Example
//!
//! ```rust
//! // Render a single terminal pane
//! let (action, input_bytes) = render_terminal_session(
//!     ui,
//!     ctx,
//!     &mut session,
//!     session_id,
//!     focused_session_id,
//!     &broadcast_state,
//!     &mut ime_composing,
//!     &mut ime_preedit,
//!     pane_rect,
//!     pane_id,
//!     true,  // show_close_btn
//!     true,  // can_close_pane
//!     &theme,
//!     14.0,  // font_size
//!     &language,
//! );
//!
//! // Handle action (close pane, focus, etc.)
//! if let Some(action) = action {
//!     match action {
//!         PaneAction::ClosePane => { /* ... */ }
//!         PaneAction::Focus => { /* ... */ }
//!         PaneAction::SplitHorizontal => { /* ... */ }
//!         PaneAction::SplitVertical => { /* ... */ }
//!     }
//! }
//!
//! // Send input bytes to terminal
//! if !input_bytes.is_empty() {
//!     session.write(&input_bytes);
//! }
//! ```

use eframe::egui;

use crate::ssh::SshConnectionState;
use crate::terminal;
use crate::ui::types::{SessionBackend, TerminalSession, BroadcastState, SearchMatch};
use crate::ui::pane::{PaneNode, PaneAction, SplitDirection, split_rect};
use crate::ui::theme::ThemeColors;
use crate::ui::i18n::Language;
use crate::ui::input::{key_to_ctrl_byte, key_to_char};

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
///
/// # Rendering Pipeline
///
/// 1. **Setup**: Calculate dimensions, font metrics, layout rects
/// 2. **Input**: Handle keyboard input, IME composition, shortcuts
/// 3. **Interaction**: Process mouse events (click, drag, scroll)
/// 4. **Render**: Draw background, text, selection, cursor, IME preedit, UI chrome
/// 5. **Return**: Action and input bytes for terminal session
///
/// # Example
///
/// ```rust
/// let (action, input) = render_terminal_session(
///     ui, ctx, &mut session, 0, 0,
///     &broadcast_state, &mut ime_composing, &mut ime_preedit,
///     pane_rect, pane_id, true, true, &theme, 14.0, &language
/// );
/// ```
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
) -> (Option<PaneAction>, Vec<u8>) {
    let font_id = egui::FontId::monospace(font_size);
    let pad_x = 8.0_f32;
    let pad_y_top = 6.0_f32;
    let pad_y_bottom = 6.0_f32;
    let mut input_bytes: Vec<u8> = Vec::new();
    let char_width = ui.fonts(|f| f.glyph_width(&font_id, 'M'));
    let line_height = ui.fonts(|f| f.row_height(&font_id)).ceil();
    let new_cols = (((pane_rect.width() - pad_x * 2.0) / char_width) as usize).max(10);
    let new_rows = (((pane_rect.height() - pad_y_top - pad_y_bottom) / line_height) as usize).max(3);
    session.resize(new_cols, new_rows);

    let rect = egui::Rect::from_min_size(
        egui::pos2(pane_rect.min.x + pad_x, pane_rect.min.y + pad_y_top),
        egui::vec2(char_width * new_cols as f32, line_height * new_rows as f32),
    );

    // Close button rect (used for hit-testing, not as a separate widget)
    let btn_sz = 18.0;
    let close_btn_rect = egui::Rect::from_min_size(
        egui::pos2(pane_rect.max.x - btn_sz - 6.0, pane_rect.min.y + 6.0),
        egui::vec2(btn_sz, btn_sz),
    );

    let response = ui.interact(pane_rect, pane_id, egui::Sense::click_and_drag());
    let mut action: Option<PaneAction> = None;

    // Check if click landed on the close button via pointer position
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

    let painter = ui.painter_at(pane_rect);

    // Mouse selection - record pixel positions first, will convert to grid coords later
    let mut drag_start_pos: Option<egui::Pos2> = None;
    let mut drag_end_pos: Option<egui::Pos2> = None;
    let mut triple_click_pos: Option<egui::Pos2> = None;
    let mut double_click_pos: Option<egui::Pos2> = None;

    // Use ctx.input to get raw mouse state for more reliable drag detection
    let mouse_state = ctx.input(|i| (
        i.pointer.primary_pressed(),
        i.pointer.primary_down(),
        i.pointer.primary_released(),
        i.pointer.hover_pos(),
        i.pointer.is_moving(),
    ));
    let (primary_pressed, primary_down, primary_released, hover_pos, _is_moving) = mouse_state;

    // Track drag state locally
    let _was_dragging = session.selection.active;
    let is_hovering = response.hovered();

    // Start dragging on left button press while hovering
    if primary_pressed && is_hovering {
        if let Some(pos) = hover_pos {
            if rect.contains(pos) {
                drag_start_pos = Some(pos);
                drag_end_pos = Some(pos);
                session.selection.active = true;
            }
        }
    }

    // Continue dragging while button is held
    if primary_down && session.selection.active {
        if let Some(pos) = hover_pos {
            // Only update if we're within or near the terminal rect
            if pos.x >= rect.min.x - 10.0 && pos.x <= rect.max.x + 10.0 &&
               pos.y >= rect.min.y - 50.0 && pos.y <= rect.max.y + 50.0 {
                let clamped_x = pos.x.clamp(rect.min.x, rect.max.x);
                let clamped_y = pos.y.clamp(rect.min.y, rect.max.y);
                drag_end_pos = Some(egui::pos2(clamped_x, clamped_y));

                // Auto-scroll when dragging near edges
                // The closer to the edge, the faster we scroll
                let edge_margin = line_height * 3.0;
                if pos.y < rect.min.y + edge_margin && pos.y >= rect.min.y - 50.0 {
                    // Near top edge - scroll up (towards older scrollback)
                    // Speed: 1-3 lines per frame based on distance
                    let distance = (rect.min.y + edge_margin - pos.y) / edge_margin;
                    let scroll_speed = (distance * 3.0).ceil() as usize;
                    let max_offset = session.grid.lock().map(|g| g.scrollback_len()).unwrap_or(0);
                    session.scroll_offset = (session.scroll_offset + scroll_speed).min(max_offset);
                } else if pos.y > rect.max.y - edge_margin && pos.y <= rect.max.y + 50.0 {
                    // Near bottom edge - scroll down (towards newer content)
                    let distance = (pos.y - (rect.max.y - edge_margin)) / edge_margin;
                    let scroll_speed = (distance * 3.0).ceil() as usize;
                    session.scroll_offset = session.scroll_offset.saturating_sub(scroll_speed);
                }
            }
        }
    }

    // Stop dragging on button release
    if primary_released {
        session.selection.active = false;
    }

    // Handle triple-click for line selection
    if response.triple_clicked() {
        triple_click_pos = response.interact_pointer_pos();
    } else if response.double_clicked() {
        double_click_pos = response.interact_pointer_pos();
    } else if response.clicked() {
        if session.selection.has_selection() {
            session.selection.clear();
        }
    }

    // IME for focused pane
    if is_focused && response.has_focus() {
        if let Ok(grid) = session.grid.lock() {
            // Calculate IME cursor position based on actual rendered text width
            // This ensures the IME composition window appears at the correct position
            let grid_row = grid.cursor_row.min(grid.rows.saturating_sub(1));

            // Build layout for current row up to cursor to get accurate position
            let cursor_row_job = build_row_layout(&grid.cells[grid_row][..grid.cursor_col.min(grid.cols)], &font_id, grid.cursor_col.min(grid.cols));
            let cursor_row_galley = ui.fonts(|f| f.layout_job(cursor_row_job));

            let cursor_x = rect.min.x + cursor_row_galley.rect.width();
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
    //
    // IMPORTANT: selection stores global indices that combine scrollback and grid:
    // - Scrollback: [0, scrollback_len)
    // - Grid: [scrollback_len, scrollback_len + grid.rows)
    //
    // This function must correctly map global indices back to actual cell data.
    let selected_text: Option<String> = if session.selection.has_selection() {
        if let Ok(grid) = session.grid.lock() {
            let scrollback_len = grid.scrollback_len();
            let ((sr, sc), (er, ec)) = session.selection.ordered();
            let mut text = String::new();

            for row in sr..=er {
                // Determine if this is a scrollback or grid row based on global index
                let cells = if row < scrollback_len {
                    // Scrollback row
                    match grid.get_scrollback_row(row) {
                        Some(c) => c,
                        None => break,
                    }
                } else {
                    // Grid row: convert global index to local grid index
                    let grid_row = row.saturating_sub(scrollback_len);
                    if grid_row >= grid.rows {
                        break;
                    }
                    &grid.cells[grid_row]
                };

                let col_start = if row == sr { sc } else { 0 };
                let col_end = (if row == er { ec + 1 } else { grid.cols }).min(grid.cols);

                for col in col_start..col_end {
                    let cell = &cells[col];
                    // Skip wide character continuation cells to avoid duplicate/extra spaces
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

    // Right-click context menu (terminal area)
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
        // Read clipboard text via egui's clipboard integration (avoids direct arboard errors)
        let clip_text = ctx.input(|i| i.events.iter().find_map(|e| {
            if let egui::Event::Paste(t) = e { Some(t.clone()) } else { None }
        }));
        if let Some(text) = clip_text {
            // Filter out control characters (except tab, newline, carriage return)
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

        // When search bar is active, intercept Escape and Enter keys
        if search_is_active {
            for event in &events {
                if let egui::Event::Key { key, pressed: true, modifiers, .. } = event {
                    match key {
                        egui::Key::Escape => {
                            session.search_state = None;
                        }
                        egui::Key::Enter if modifiers.shift => {
                            // Shift+Enter: go to previous match
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
                            // Enter: go to next match
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

        // Check if this frame contains non-ASCII text events
        let has_non_ascii_text = events.iter().any(|e| {
            if let egui::Event::Text(text) = e {
                !text.chars().all(|c| c.is_ascii())
            } else {
                false
            }
        });

        // Chinese IME punctuation mapping - ONLY these actually get converted
        // All other ASCII punctuation should be passed through normally
        fn is_chinese_ime_punct(ch: char) -> bool {
            matches!(ch,
                '.' | ',' | ';' | ':' |  // Basic punctuation -> 。；：
                '?' | '!' |              // Question/exclamation -> ？！
                '(' | ')' |              // Parentheses -> （)
                '[' | ']' |              // Square brackets -> 【】
                '<' | '>'                // Angle brackets -> 《》
            )
        }

        // When search is active, don't forward keyboard input to the terminal
        if !search_is_active {

        // First pass: process all IME events
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
                        // Filter out control characters (except common safe ones like tab, newline)
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

        // Second pass: process other events
        for event in &events {
            match event {
                egui::Event::Text(text) => {
                    // If we had any IME events in this frame, skip ALL Text events
                    // This prevents duplicate characters when IME sends both Commit and Text
                    if has_ime_events {
                        continue;
                    }

                    // When IME is composing, skip ALL Text events since IME handles everything
                    if *ime_composing {
                        continue;
                    }

                    // If last input was non-ASCII (Chinese punctuation), skip single ASCII punctuation
                    // This catches the case where IME sends ASCII punct in a separate frame
                    if session.last_non_ascii_input {
                        let is_single_ascii_punct = text.len() == 1 &&
                            text.chars().next().map(|c| is_chinese_ime_punct(c)).unwrap_or(false);
                        if is_single_ascii_punct {
                            // Don't reset flag - keep it true for next punctuation
                            continue;
                        }
                    }

                    // Reset the flag only if we're processing non-punctuation text
                    let is_punct = text.len() == 1 &&
                        text.chars().next().map(|c| is_chinese_ime_punct(c)).unwrap_or(false);

                    // Process non-ASCII text (e.g., direct Unicode input)
                    if !text.chars().all(|c| c.is_ascii()) {
                        session.selection.clear();
                        // Filter out control characters (except tab, newline, carriage return)
                        let safe_text: String = text.chars()
                            .filter(|c| *c == '\t' || *c == '\n' || *c == '\r' || !c.is_control())
                            .collect();
                        if !safe_text.is_empty() {
                            session.write(&safe_text);
                            input_bytes.extend_from_slice(safe_text.as_bytes());
                        }
                        session.last_non_ascii_input = true;
                    } else if !is_punct {
                        // Non-punctuation ASCII text - reset flag
                        session.last_non_ascii_input = false;
                    }
                }
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    if modifiers.command {
                        // Cmd+Arrow for line navigation (macOS style)
                        let cmd_arrow = match key {
                            egui::Key::ArrowLeft  => { session.write("\x01"); input_bytes.push(0x01); true }  // Ctrl+A = line start
                            egui::Key::ArrowRight => { session.write("\x05"); input_bytes.push(0x05); true }  // Ctrl+E = line end
                            _ => false,
                        };
                        if cmd_arrow {
                            session.selection.clear();
                        } else if modifiers.shift && *key == egui::Key::I {
                            // Cmd+Shift+I: toggle broadcast mode
                            action = Some(PaneAction::ToggleBroadcast);
                        } else if *key == egui::Key::C {
                            if let Some(ref text) = selected_text {
                                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                    let _ = clipboard.set_text(text.clone());
                                }
                                session.selection.clear();
                            } else {
                                session.write_bytes(&[0x03]);
                                input_bytes.push(0x03);
                            }
                        } else if *key == egui::Key::A {
                            session.selection.start = (0, 0);
                            session.selection.end = (new_rows.saturating_sub(1), new_cols.saturating_sub(1));
                        } else if *key == egui::Key::V {
                            // Cmd+V: handled by egui::Event::Paste below — no-op here
                        }
                        // Cmd+D / Cmd+Shift+D are consumed by split shortcuts — not forwarded to PTY
                    } else if modifiers.ctrl {
                        if let Some(byte) = key_to_ctrl_byte(key) {
                            session.selection.clear();
                            session.write_bytes(&[byte]);
                            input_bytes.push(byte);
                        }
                    } else if modifiers.alt {
                        // Alt/Option+Arrow for word navigation (macOS style)
                        let alt_arrow = match key {
                            egui::Key::ArrowLeft  => { session.write("\x1bb"); input_bytes.extend_from_slice(b"\x1bb"); true }  // word backward
                            egui::Key::ArrowRight => { session.write("\x1bf"); input_bytes.extend_from_slice(b"\x1bf"); true }  // word forward
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
                                // Check if this is a Chinese IME punctuation key
                                let is_ime_punct = is_chinese_ime_punct(ch);

                                // Skip Chinese IME punctuation if either:
                                // 1. We just had non-ASCII input (from previous frames), OR
                                // 2. This frame contains non-ASCII text (IME punctuation in same frame)
                                if is_ime_punct && (session.last_non_ascii_input || has_non_ascii_text) {
                                    // Don't reset flag - keep it true for next punctuation
                                } else {
                                    // Reset flag only for non-IME-punctuation keys
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
                    // Cmd+C: copy selection, or send Ctrl-C if no selection
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
                    // Filter out control characters (except tab, newline, carriage return)
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

    // ── Render terminal content ──────────────────────────
    if let Ok(grid) = session.grid.lock() {
        painter.rect_filled(pane_rect, 0.0, theme.bg_primary);
        let scrollback_len = grid.scrollback_len();
        let offset = session.scroll_offset;

        // Helper function to convert pixel position to grid coordinates using actual text layout
        //
        // # Selection Index System
        //
        // Terminal selection uses a **global index system** that combines scrollback and grid rows:
        // - Scrollback rows: indices [0, scrollback_len)
        // - Grid rows: indices [scrollback_len, scrollback_len + grid.rows)
        //
        // This unified indexing allows selection to span seamlessly across scrollback and current grid content.
        //
        // ## Conversion Rules
        //
        // When clicking on a row:
        // - If `screen_row < 0` (above visible area): calculate scrollback index from distance above
        // - If `screen_row < offset` (scrollback visible area): return scrollback global index
        // - If `screen_row >= offset` (grid area): return `scrollback_len + local_grid_row`
        //
        // The returned `grid_row_idx` is a **global index** that can be directly stored in `selection.start/end`.
        let pixel_to_cell = |pos: egui::Pos2| -> (usize, usize) {
            // Calculate screen row from Y position (can be negative for positions above rect)
            let screen_row_float = (pos.y - rect.min.y) / line_height;
            let screen_row = if screen_row_float < 0.0 {
                // Position above the terminal rect - calculate scrollback index
                // The offset increases as we move up, so we need to add to current scrollback offset
                let rows_above = (-screen_row_float).ceil() as usize;
                let sb_idx = scrollback_len.saturating_sub(offset + rows_above);
                return (sb_idx, 0);
            } else {
                screen_row_float as usize
            };
            let screen_row = screen_row.min(new_rows.saturating_sub(1));

            // Get the cells for this row to calculate accurate column position
            // Return: (cells reference, global_row_index)
            let (cells, grid_row_idx) = if offset > 0 && screen_row < offset {
                // Scrollback row: return scrollback global index directly
                let sb_idx = scrollback_len.saturating_sub(offset) + screen_row;
                if let Some(row) = grid.get_scrollback_row(sb_idx) {
                    (row, sb_idx)
                } else {
                    (&grid.cells[0], screen_row)
                }
            } else {
                // Grid row: convert to global index by adding scrollback_len
                let grid_row = screen_row.saturating_sub(offset);
                if grid_row < grid.rows {
                    (&grid.cells[grid_row], scrollback_len + grid_row)
                } else {
                    (&grid.cells[0], scrollback_len)
                }
            };

            // Build layout for accurate character width calculation
            let job = build_row_layout(cells, &font_id, cells.len().min(grid.cols));
            let galley = ui.fonts(|f| f.layout_job(job.clone()));

            // Simple approach: use cell position directly
            // Each cell in the grid (including continuation) takes avg space
            let x_in_row = (pos.x - rect.min.x).max(0.0);
            let galley_width = galley.rect.width();
            let n_cells = cells.len();

            // Calculate average width per cell (not per visible character)
            let avg_cell_width = if n_cells > 0 { galley_width / n_cells as f32 } else { char_width };

            // Find which cell contains the click position
            let col = (x_in_row / avg_cell_width).floor() as usize;
            let col = col.min(cells.len().saturating_sub(1));

            (grid_row_idx, col)
        };

        // Process mouse events using actual text layout
        if let Some(pos) = drag_start_pos {
            let (grid_row, grid_col) = pixel_to_cell(pos);
            session.selection.start = (grid_row, grid_col);
            session.selection.end = (grid_row, grid_col);
        }
        // Update selection end position while dragging
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

            // Convert global index to local index for find_word_boundaries
            //
            // `find_word_boundaries` expects a **local grid index** (0 to grid.rows-1) because
            // it only has access to `grid.cells[]`. However, `pixel_to_cell` returns a global index.
            //
            // Conversion:
            // - If grid_row < scrollback_len: it's already a scrollback index (but scrollback words
            //   can't be selected due to access limitations - see find_word_boundaries)
            // - If grid_row >= scrollback_len: subtract scrollback_len to get local grid index
            let local_row = if grid_row < scrollback_len {
                grid_row
            } else {
                grid_row.saturating_sub(scrollback_len)
            };

            let (word_start, word_end) = find_word_boundaries(&grid, local_row, grid_col);

            // Store selection with global indices
            session.selection.start = (grid_row, word_start);
            session.selection.end = (grid_row, word_end);
        }

        // Calculate vertical offset for centering text in rows
        // Use a sample galley to determine the text height
        let sample_job = build_row_layout(&grid.cells[0], &font_id, 1);
        let sample_galley = ui.fonts(|f| f.layout_job(sample_job));
        let vertical_offset = (line_height - sample_galley.rect.height()) / 2.0;

        for screen_row in 0..grid.rows.min(new_rows) {
            let row_y = rect.min.y + screen_row as f32 * line_height;
            if offset > 0 && screen_row < offset {
                let sb_idx = scrollback_len.saturating_sub(offset) + screen_row;
                if let Some(sb_row) = grid.get_scrollback_row(sb_idx) {
                    let job = build_row_layout(sb_row, &font_id, grid.cols.min(sb_row.len()));
                    let galley = ui.fonts(|f| f.layout_job(job));
                    painter.galley(egui::pos2(rect.min.x, row_y + vertical_offset), galley, egui::Color32::TRANSPARENT);
                }
            } else {
                let grid_row = screen_row - offset;
                if grid_row < grid.rows {
                    let job = build_row_layout(&grid.cells[grid_row], &font_id, grid.cols);
                    let galley = ui.fonts(|f| f.layout_job(job));
                    painter.galley(egui::pos2(rect.min.x, row_y + vertical_offset), galley, egui::Color32::TRANSPARENT);
                }
            }
        }

        // Selection highlight
        if session.selection.has_selection() {
            let ((sr, sc), (er, ec)) = session.selection.ordered();
            let sel_color = theme.accent_alpha(60);

            // Render selection for each row in range
            //
            // Selection stores **global indices**, so we need to determine:
            // 1. Is this a scrollback row or a grid row? (sel_row < scrollback_len check)
            // 2. What's the screen position for this row? (considering offset)
            //
            // The global index system enables seamless selection across scrollback and grid.
            for sel_row in sr..=er {
                // Determine if this is a scrollback or grid row based on global index
                let (grid_row_idx, is_scrollback) = if offset > 0 && sel_row < scrollback_len {
                    // Scrollback row: global index = local scrollback index
                    (sel_row, true)
                } else {
                    // Grid row: convert global index to local grid index
                    (sel_row.saturating_sub(scrollback_len), false)
                };

                // Calculate screen row position
                // Screen row is where this row appears visually (0 = top of visible area)
                let screen_row = if is_scrollback {
                    if sel_row >= scrollback_len {
                        continue;
                    }
                    let sb_display_idx = scrollback_len.saturating_sub(offset) + sel_row;
                    if sb_display_idx >= offset + scrollback_len {
                        continue;
                    }
                    sb_display_idx
                } else {
                    if grid_row_idx >= grid.rows {
                        break;
                    }
                    grid_row_idx + offset
                };

                if screen_row >= new_rows {
                    if is_scrollback {
                        continue;
                    } else {
                        break;
                    }
                }

                // Get the cells for this row
                let cells = if is_scrollback {
                    grid.get_scrollback_row(sel_row)
                } else {
                    Some(&grid.cells[grid_row_idx])
                };

                let cells = match cells {
                    Some(c) => c,
                    None => continue,
                };

                let row_cols = cells.len().min(grid.cols);
                let col_start = if sel_row == sr { sc.min(row_cols) } else { 0 };
                let col_end = if sel_row == er { (ec + 1).min(row_cols) } else { row_cols };

                if col_start >= col_end {
                    continue;
                }

                // Use galley for precise character positions
                // Build layout for the selected range and use its actual width
                let job = build_row_layout(cells, &font_id, col_end);
                let galley = ui.fonts(|f| f.layout_job(job.clone()));

                // Use galley size to get accurate total width
                let galley_width = galley.rect.width();
                let n_cols = col_end;
                let avg_col_width = if n_cols > 0 { galley_width / n_cols as f32 } else { char_width };

                // Calculate positions based on column indices
                let start_x = col_start as f32 * avg_col_width;
                let end_x = col_end as f32 * avg_col_width;

                // Get actual rendered height from galley
                let char_height = sample_galley.rect.height();  // Use actual text height for proper alignment

                let row_y = rect.min.y + screen_row as f32 * line_height;
                let sel_rect = egui::Rect::from_min_max(
                    egui::pos2(rect.min.x + start_x, row_y + vertical_offset),
                    egui::pos2(rect.min.x + end_x, row_y + vertical_offset + char_height),
                );
                painter.rect_filled(sel_rect, 0.0, sel_color);
            }
        }

        // Search match highlighting
        if let Some(ref search_state) = session.search_state {
            let match_color = egui::Color32::from_rgba_premultiplied(255, 220, 50, 80);   // semi-transparent yellow
            let current_color = egui::Color32::from_rgba_premultiplied(255, 140, 0, 120); // orange for current match

            for (match_idx, m) in search_state.matches.iter().enumerate() {
                let is_current = match_idx == search_state.current_index;

                // Determine screen row for this match
                let (grid_row_idx, is_scrollback) = if m.row < scrollback_len {
                    (m.row, true)
                } else {
                    (m.row.saturating_sub(scrollback_len), false)
                };

                let screen_row = if is_scrollback {
                    if m.row >= scrollback_len {
                        continue;
                    }
                    let vis_start = scrollback_len.saturating_sub(offset);
                    if m.row < vis_start || m.row >= vis_start + offset.min(new_rows) {
                        continue;
                    }
                    m.row - vis_start
                } else {
                    if grid_row_idx >= grid.rows {
                        continue;
                    }
                    grid_row_idx + offset
                };

                if screen_row >= new_rows {
                    continue;
                }

                // Get cells for this row to compute accurate positions
                let cells = if is_scrollback {
                    match grid.get_scrollback_row(m.row) {
                        Some(c) => c,
                        None => continue,
                    }
                } else {
                    &grid.cells[grid_row_idx]
                };

                let col_end_clamped = m.col_end.min(cells.len()).min(grid.cols);
                let col_start_clamped = m.col_start.min(col_end_clamped);
                if col_start_clamped >= col_end_clamped {
                    continue;
                }

                // Use galley for precise positions (same approach as selection highlighting)
                let job = build_row_layout(cells, &font_id, col_end_clamped);
                let galley = ui.fonts(|f| f.layout_job(job));
                let galley_width = galley.rect.width();
                let n_cols = col_end_clamped;
                let avg_col_width = if n_cols > 0 { galley_width / n_cols as f32 } else { char_width };

                let start_x = col_start_clamped as f32 * avg_col_width;
                let end_x = col_end_clamped as f32 * avg_col_width;
                let char_height = sample_galley.rect.height();

                let row_y = rect.min.y + screen_row as f32 * line_height;
                let highlight_rect = egui::Rect::from_min_max(
                    egui::pos2(rect.min.x + start_x, row_y + vertical_offset),
                    egui::pos2(rect.min.x + end_x, row_y + vertical_offset + char_height),
                );
                let color = if is_current { current_color } else { match_color };
                painter.rect_filled(highlight_rect, 0.0, color);
            }
        }

        // Cursor (skip when SSH is not yet connected)
        let ssh_not_connected = matches!(&session.session, Some(SessionBackend::Ssh(ssh))
            if !matches!(ssh.connection_state(), SshConnectionState::Connected));
        if !ssh_not_connected && offset == 0 && grid.cursor_visible && grid.cursor_col < grid.cols && grid.cursor_row < grid.rows {
            // IMPORTANT: Calculate cursor position based on actual rendered text width
            // rather than grid cell position, to handle wide characters (CJK) correctly.
            //
            // Problem: Using `cursor_col * char_width` causes misalignment because:
            // - Wide chars occupy 2 grid cells but their actual width ≠ 2 * char_width
            // - Small differences accumulate: 10 chars × 0.25px = 2.5px gap
            //
            // Solution: Build layout for current row up to cursor position and use actual width

            let grid_row = grid.cursor_row.min(grid.rows.saturating_sub(1));

            // Build layout for this row up to cursor column to get accurate cursor position
            let cursor_row_job = build_row_layout(&grid.cells[grid_row][..grid.cursor_col.min(grid.cols)], &font_id, grid.cursor_col.min(grid.cols));
            let cursor_row_galley = ui.fonts(|f| f.layout_job(cursor_row_job));
            let cursor_x = rect.min.x + cursor_row_galley.rect.width();

            let cursor_y = rect.min.y + grid.cursor_row as f32 * line_height + vertical_offset;
            let cursor_height = sample_galley.rect.height();
            let cursor_rect = egui::Rect::from_min_size(
                egui::pos2(cursor_x, cursor_y),
                egui::vec2(char_width, cursor_height),
            );
            if is_focused && response.has_focus() {
                let blink_on = (ctx.input(|i| i.time) * 2.0) as u64 % 2 == 0;
                if blink_on {
                    painter.rect_filled(cursor_rect, 0.0,
                        egui::Color32::from_rgba_premultiplied(theme.cursor_color.r(), theme.cursor_color.g(), theme.cursor_color.b(), 160));
                } else {
                    painter.rect_stroke(cursor_rect, 0.0, egui::Stroke::new(1.0, theme.cursor_color));
                }
            } else {
                painter.rect_stroke(cursor_rect, 0.0, egui::Stroke::new(1.0, theme.fg_dim));
            }
        }

        // IME preedit overlay
        if is_focused && !ime_preedit.is_empty() && grid.cursor_col < grid.cols && grid.cursor_row < grid.rows {
            // Filter out control characters that could cause layout issues
            let safe_preedit: String = ime_preedit.chars()
                .filter(|c| !c.is_control())
                .collect();

            if !safe_preedit.is_empty() {
                // Position IME preedit at the same location as the cursor
                // Use the same calculation method as the cursor for consistency
                let grid_row = grid.cursor_row.min(grid.rows.saturating_sub(1));

                // Build layout for current row up to cursor to get actual position
                let cursor_row_job = build_row_layout(&grid.cells[grid_row][..grid.cursor_col.min(grid.cols)], &font_id, grid.cursor_col.min(grid.cols));
                let cursor_row_galley = ui.fonts(|f| f.layout_job(cursor_row_job));
                let base_x = rect.min.x + cursor_row_galley.rect.width();

                let py = rect.min.y + grid.cursor_row as f32 * line_height + vertical_offset;

                // Build layout for IME preedit text
                let galley = ui.fonts(|f| f.layout_no_wrap(safe_preedit, font_id.clone(), theme.accent));

                // Position IME preedit at the cursor location
                let bg_rect = egui::Rect::from_min_size(egui::pos2(base_x, py), galley.size() + egui::vec2(4.0, 0.0));
                painter.rect_filled(bg_rect, 2.0, theme.bg_elevated);
                painter.galley(egui::pos2(base_x + 2.0, py), galley, egui::Color32::TRANSPARENT);
            }
        }

        // Scrollback indicator
        if offset > 0 {
            let indicator = language.tf("lines_above", &offset.to_string());
            let galley = ui.fonts(|f| f.layout_no_wrap(indicator, egui::FontId::monospace(11.0), theme.fg_dim));
            let ind_x = rect.max.x - galley.rect.width() - 12.0;
            let ind_y = rect.min.y + 4.0;
            painter.rect_filled(
                egui::Rect::from_min_size(egui::pos2(ind_x - 4.0, ind_y - 2.0),
                    egui::vec2(galley.rect.width() + 8.0, galley.rect.height() + 4.0)),
                4.0, egui::Color32::from_rgba_premultiplied(
                    theme.bg_secondary.r(), theme.bg_secondary.g(), theme.bg_secondary.b(), 200),
            );
            painter.galley(egui::pos2(ind_x, ind_y), galley, egui::Color32::TRANSPARENT);
        }

        // SSH connection state overlay
        if let Some(SessionBackend::Ssh(ssh)) = &session.session {
            match ssh.connection_state() {
                SshConnectionState::Connecting | SshConnectionState::Authenticating => {
                    painter.rect_filled(rect, 0.0, egui::Color32::from_rgba_premultiplied(
                        theme.bg_primary.r(), theme.bg_primary.g(), theme.bg_primary.b(), 240));
                    let msg = match ssh.connection_state() {
                        SshConnectionState::Connecting => language.t("connecting"),
                        SshConnectionState::Authenticating => language.t("authenticating"),
                        _ => "",
                    };
                    let galley = ui.fonts(|f| f.layout_no_wrap(msg.to_string(), egui::FontId::monospace(16.0), theme.accent));
                    painter.galley(egui::pos2(rect.center().x - galley.rect.width() / 2.0,
                        rect.center().y - galley.rect.height() / 2.0 - 14.0), galley, egui::Color32::TRANSPARENT);

                    // Cancel button
                    let btn_size = egui::vec2(70.0, 28.0);
                    let btn_pos = egui::pos2(rect.center().x - btn_size.x / 2.0, rect.center().y + 10.0);
                    let btn_rect = egui::Rect::from_min_size(btn_pos, btn_size);
                    let btn_resp = ui.allocate_rect(btn_rect, egui::Sense::click());
                    let btn_bg = if btn_resp.hovered() { theme.bg_elevated } else { theme.bg_secondary };
                    painter.rect(btn_rect, 4.0, btn_bg, egui::Stroke::new(1.0, theme.border));
                    let cancel_galley = ui.fonts(|f| f.layout_no_wrap(language.t("cancel").to_string(), egui::FontId::proportional(12.0), theme.red));
                    painter.galley(egui::pos2(btn_rect.center().x - cancel_galley.rect.width() / 2.0,
                        btn_rect.center().y - cancel_galley.rect.height() / 2.0), cancel_galley, egui::Color32::TRANSPARENT);
                    if btn_resp.clicked() {
                        action = Some(PaneAction::ClosePane);
                    }
                }
                SshConnectionState::Error(ref err) => {
                    painter.rect_filled(rect, 0.0, egui::Color32::from_rgba_premultiplied(
                        theme.bg_primary.r(), theme.bg_primary.g(), theme.bg_primary.b(), 220));
                    // Filter out control characters from error message
                    let safe_err: String = err.chars().filter(|c| !c.is_control()).collect();
                    let msg = language.tf("ssh_error", &safe_err);

                    // Check if this is a host key verification error
                    let is_key_error = err.contains("Host key verification failed") ||
                                      err.contains("MITM attack");

                    // Use wrap layout to support newline characters
                    let galley = ui.fonts(|f| f.layout(msg,
                        egui::FontId::monospace(13.0), theme.red, rect.width() * 0.85));

                    let text_y = rect.center().y - galley.rect.height() / 2.0 - 10.0;
                    painter.galley(egui::pos2(rect.center().x - galley.rect.width() / 2.0,
                        text_y), galley, egui::Color32::TRANSPARENT);

                    // Show "Remove old key" button for host key verification failures
                    if is_key_error {
                        let btn_size = egui::vec2(120.0, 28.0);
                        let btn_pos = egui::pos2(rect.center().x - btn_size.x / 2.0,
                            rect.center().y + 10.0);
                        let btn_rect = egui::Rect::from_min_size(btn_pos, btn_size);
                        let btn_resp = ui.allocate_rect(btn_rect, egui::Sense::click());

                        // Button background (stroke style for alert)
                        painter.rect_filled(btn_rect, 4.0, theme.bg_elevated);
                        painter.rect_stroke(btn_rect, 4.0, egui::Stroke::new(1.0, theme.red));

                        let btn_text = "Remove old key";
                        let btn_galley = ui.fonts(|f| f.layout_no_wrap(
                            btn_text.to_string(),
                            egui::FontId::proportional(12.0),
                            theme.fg_primary,
                        ));
                        painter.galley(
                            egui::pos2(btn_rect.center().x - btn_galley.rect.width() / 2.0,
                                btn_rect.center().y - btn_galley.rect.height() / 2.0),
                            btn_galley,
                            egui::Color32::TRANSPARENT,
                        );

                        if btn_resp.clicked() {
                            action = Some(PaneAction::RemoveHostKey);
                        }
                    }
                }
                SshConnectionState::Disconnected(ref reason) => {
                    // Filter out control characters from reason
                    let safe_reason: String = reason.chars().filter(|c| !c.is_control()).collect();
                    let galley = ui.fonts(|f| f.layout_no_wrap(language.tf("disconnected", &safe_reason),
                        egui::FontId::monospace(11.0), theme.red));
                    painter.galley(egui::pos2(rect.min.x + 4.0, rect.max.y - galley.rect.height() - 4.0),
                        galley, egui::Color32::TRANSPARENT);
                }
                SshConnectionState::Connected => {}
            }
        }
    }

    // ── Search bar overlay ──
    if session.search_state.is_some() {
        let search_bar_width = 320.0_f32;
        let search_bar_height = 32.0_f32;
        let search_bar_margin = 8.0_f32;
        let search_bar_rect = egui::Rect::from_min_size(
            egui::pos2(
                pane_rect.max.x - search_bar_width - search_bar_margin,
                pane_rect.min.y + search_bar_margin,
            ),
            egui::vec2(search_bar_width, search_bar_height),
        );

        // Draw search bar background with border
        let search_painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("search_bar").with(pane_id),
        ));
        search_painter.rect(
            search_bar_rect,
            6.0,
            theme.bg_elevated,
            egui::Stroke::new(1.0, theme.border),
        );

        // Layout: [text input] [match count] [prev] [next] [close]
        let inner_margin = 4.0;
        let btn_width = 22.0;
        let count_width = 50.0;
        let input_width = search_bar_width - inner_margin * 2.0 - btn_width * 3.0 - count_width - 8.0;

        // Extract current state for rendering
        let match_count = session.search_state.as_ref().map(|s| s.matches.len()).unwrap_or(0);
        let current_index = session.search_state.as_ref().map(|s| s.current_index).unwrap_or(0);
        let query_str = session.search_state.as_ref().map(|s| s.query.clone()).unwrap_or_default();

        // Match count label
        let count_text = if query_str.is_empty() {
            String::new()
        } else if match_count == 0 {
            language.t("no_matches").to_string()
        } else {
            format!("{}/{}", current_index + 1, match_count)
        };

        let count_color = if match_count == 0 && !query_str.is_empty() {
            theme.red
        } else {
            theme.fg_dim
        };

        let count_galley = ui.fonts(|f| f.layout_no_wrap(
            count_text,
            egui::FontId::proportional(11.0),
            count_color,
        ));

        let count_x = search_bar_rect.min.x + inner_margin + input_width + 4.0;
        let count_y = search_bar_rect.center().y - count_galley.rect.height() / 2.0;
        search_painter.galley(egui::pos2(count_x, count_y), count_galley, egui::Color32::TRANSPARENT);

        // Previous button (▲)
        let prev_x = count_x + count_width;
        let prev_rect = egui::Rect::from_min_size(
            egui::pos2(prev_x, search_bar_rect.min.y + (search_bar_height - btn_width) / 2.0),
            egui::vec2(btn_width, btn_width),
        );
        let prev_resp = ui.allocate_rect(prev_rect, egui::Sense::click());
        let prev_bg = if prev_resp.hovered() { theme.hover_bg } else { egui::Color32::TRANSPARENT };
        search_painter.rect_filled(prev_rect, 4.0, prev_bg);
        search_painter.text(
            prev_rect.center(),
            egui::Align2::CENTER_CENTER,
            "\u{25b2}",
            egui::FontId::proportional(10.0),
            theme.fg_dim,
        );
        if prev_resp.clicked() {
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

        // Next button (▼)
        let next_x = prev_x + btn_width;
        let next_rect = egui::Rect::from_min_size(
            egui::pos2(next_x, search_bar_rect.min.y + (search_bar_height - btn_width) / 2.0),
            egui::vec2(btn_width, btn_width),
        );
        let next_resp = ui.allocate_rect(next_rect, egui::Sense::click());
        let next_bg = if next_resp.hovered() { theme.hover_bg } else { egui::Color32::TRANSPARENT };
        search_painter.rect_filled(next_rect, 4.0, next_bg);
        search_painter.text(
            next_rect.center(),
            egui::Align2::CENTER_CENTER,
            "\u{25bc}",
            egui::FontId::proportional(10.0),
            theme.fg_dim,
        );
        if next_resp.clicked() {
            if let Some(ref mut state) = session.search_state {
                if !state.matches.is_empty() {
                    state.current_index = (state.current_index + 1) % state.matches.len();
                }
            }
        }

        // Close button (×)
        let close_x = next_x + btn_width;
        let close_rect = egui::Rect::from_min_size(
            egui::pos2(close_x, search_bar_rect.min.y + (search_bar_height - btn_width) / 2.0),
            egui::vec2(btn_width, btn_width),
        );
        let close_resp = ui.allocate_rect(close_rect, egui::Sense::click());
        let close_bg = if close_resp.hovered() {
            egui::Color32::from_rgb(192, 77, 77)
        } else {
            egui::Color32::TRANSPARENT
        };
        search_painter.rect_filled(close_rect, 4.0, close_bg);
        search_painter.text(
            close_rect.center(),
            egui::Align2::CENTER_CENTER,
            "\u{00d7}",
            egui::FontId::proportional(13.0),
            theme.fg_dim,
        );
        if close_resp.clicked() {
            session.search_state = None;
        }

        // Text input field — rendered using egui's TextEdit widget
        if session.search_state.is_some() {
            let input_rect = egui::Rect::from_min_size(
                egui::pos2(search_bar_rect.min.x + inner_margin, search_bar_rect.min.y + inner_margin),
                egui::vec2(input_width, search_bar_height - inner_margin * 2.0),
            );

            let search_input_id = egui::Id::new("search_input").with(pane_id);
            let mut query = session.search_state.as_ref().map(|s| s.query.clone()).unwrap_or_default();
            let prev_query = query.clone();

            let text_edit = egui::TextEdit::singleline(&mut query)
                .id(search_input_id)
                .font(egui::FontId::proportional(13.0))
                .desired_width(input_width)
                .hint_text(language.t("search_placeholder"))
                .text_color(theme.fg_primary)
                .frame(false);

            let inner_resp = ui.put(input_rect, text_edit);

            // Auto-focus the text input when search opens
            if !inner_resp.has_focus() {
                inner_resp.request_focus();
            }

            // Update search state when query changes
            if query != prev_query {
                if let Some(ref mut state) = session.search_state {
                    state.query = query.clone();
                    // Run search
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
        }

        // Auto-scroll to current match
        if let Some(ref state) = session.search_state {
            if !state.matches.is_empty() {
                let current_match = &state.matches[state.current_index];
                if let Ok(grid) = session.grid.lock() {
                    let scrollback_len = grid.scrollback_len();
                    if current_match.row < scrollback_len {
                        // Match is in scrollback — scroll to show it
                        let target_offset = scrollback_len - current_match.row;
                        session.scroll_offset = target_offset;
                    } else {
                        // Match is in current grid — ensure we're scrolled to bottom
                        session.scroll_offset = 0;
                    }
                }
            }
        }
    }

    // ── Broadcast mode border (accent border on non-focused panes) ──
    if broadcast_state.is_active() && !is_focused {
        let painter = ui.painter_at(pane_rect);
        painter.rect_stroke(pane_rect, 0.0, egui::Stroke::new(2.0, theme.accent));
    }

    // ── Close button (×) shown on hover (only when multiple panes exist) ──
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

/// Render a single terminal pane into the given rect (thin wrapper around render_terminal_session).
/// Render a single terminal pane leaf node.
///
/// Wraps `render_terminal_session` with UI chrome including:
/// - Close button in top-right corner
/// - Border (accent color when broadcasting, standard border otherwise)
/// - Proper spacing and padding
///
/// # Arguments
///
/// Same as `render_terminal_session` plus:
/// * `session_idx` - Index into sessions vector
/// * `sessions` - Mutable slice of all terminal sessions
/// * `focused_session` - Index of currently focused session
///
/// # Notes
///
/// This function is called from `render_pane_tree` when it reaches a leaf node.
/// For detached windows, this is the entry point. For tabbed panes, the parent
/// manages the tab UI.
///
/// # See Also
///
/// * `render_terminal_session` - Core rendering logic
/// * `render_pane_tree` - Recursive pane tree renderer
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
    )
}

/// Recursively render a pane layout tree into the given rect.
/// Recursively render a pane tree with split panes and terminal sessions.
///
/// This function handles the pane tree structure which supports:
/// - **Horizontal splits**: Panes stacked vertically (Cmd+D)
/// - **Vertical splits**: Panes side-by-side (Cmd+Shift+D)
/// - **Leaf nodes**: Terminal sessions
/// - **Resizable dividers**: Drag to adjust split ratio
/// - **Close button**: Per-pane close functionality
///
/// # Pane Tree Structure
///
/// ```text
/// PaneNode::Split {
///     direction: Horizontal,
///     first: PaneNode::Leaf(0),      // Top pane
///     second: PaneNode::Leaf(1),     // Bottom pane
///     ratio: 0.5,                    // Split at 50%
/// }
/// ```
///
/// # Arguments
///
/// * `ui` - egui UI context
/// * `ctx` - egui context for I/O
/// * `node` - Pane tree node (mutable to update ratio from drag)
/// * `rect` - Available rectangle for this pane
/// * `sessions` - All terminal sessions
/// * `focused_session` - Currently focused session index
/// * `broadcast_state` - Broadcast mode state
/// * `ime_composing` - IME composition state
/// * `ime_preedit` - IME preedit text
/// * `can_close_pane` - Whether panes can be closed
/// * `theme` - Color theme
/// * `font_size` - Terminal font size
/// * `language` - UI language
///
/// # Returns
///
/// * `(Option<usize>, Option<PaneAction>, Vec<u8>)`
///   - Session index (if any)
///   - Action to perform (close/focus/split)
///   - Input bytes to send to terminal
///
/// # Divider Interaction
///
/// Split dividers are interactive areas between panes:
/// - **Width**: 2 pixels with 10-pixel hit area
/// - **Cursor**: Changes to resize cursor on hover
/// - **Drag**: Adjusts the split ratio
/// - **Bounds**: Ratio clamped to [0.1, 0.9]
///
/// # Example
///
/// ```rust
/// let (session_idx, action, input) = render_pane_tree(
///     ui, ctx, &mut pane_root, available_rect,
///     &mut sessions, focused_session, &broadcast_state,
///     &mut ime_composing, &mut ime_preedit,
///     true, &theme, 14.0, &language
/// );
/// ```
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
) -> Option<(usize, PaneAction, Vec<u8>)> {
    match node {
        PaneNode::Terminal(idx) => {
            let (action, input_bytes) = render_terminal_pane(
                ui, ctx, *idx, sessions, focused_session, broadcast_state,
                ime_composing, ime_preedit, rect,
                sessions.len() > 1,
                can_close_pane,
                theme, font_size, language,
            );
            match action {
                Some(a) => Some((*idx, a, input_bytes)),
                None if !input_bytes.is_empty() => Some((*idx, PaneAction::Focus, input_bytes)),
                None => None,
            }
        }
        PaneNode::Split { direction, ratio, first, second } => {
            let dir = *direction;

            // Update ratio on drag BEFORE calculating rects
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

            // Update ratio on drag
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

            // Calculate rects with current ratio
            let (rect1, rect2) = split_rect(rect, dir, *ratio);

            // Resize cursor
            if divider_resp.hovered() || divider_resp.dragged() {
                let icon = match dir {
                    SplitDirection::Horizontal => egui::CursorIcon::ResizeHorizontal,
                    SplitDirection::Vertical   => egui::CursorIcon::ResizeVertical,
                };
                ctx.set_cursor_icon(icon);
            }

            // Render panes
            let f1 = render_pane_tree(ui, ctx, first,  rect1, sessions, focused_session, broadcast_state, ime_composing, ime_preedit, can_close_pane, theme, font_size, language);
            let f2 = render_pane_tree(ui, ctx, second, rect2, sessions, focused_session, broadcast_state, ime_composing, ime_preedit, can_close_pane, theme, font_size, language);

            // Prefer the pane with a non-Focus action (split) over a plain focus
            let result = match (f1, f2) {
                (Some((_, PaneAction::Focus, _)), Some(other)) => Some(other),
                (Some(a), _) => Some(a),
                (None, b)    => b,
            };

            // Draw divider AFTER panes to ensure it's on top
            let div_color = if divider_resp.hovered() || divider_resp.dragged() {
                theme.accent
            } else {
                theme.border
            };
            // Use layer_painter to draw divider on top of everything
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::new("divider").with(divider_id)));
            painter.rect_filled(divider_rect, 0.0, div_color);

            result
        }
    }
}

/// Find word boundaries around a given cell position for double-click selection.
///
/// This function identifies the start and end column indices of the word containing
/// the specified cell. It handles both ASCII and CJK characters, as well as
/// wide character continuation cells.
///
/// # Word Definition
///
/// A "word" is defined as a contiguous sequence of:
/// - ASCII alphanumeric characters (a-z, A-Z, 0-9)
/// - Underscore (_)
/// - CJK ideographs (Chinese, Japanese, Korean characters)
///
/// Everything else (spaces, punctuation, symbols) is treated as a word separator.
///
/// # Arguments
///
/// * `grid` - Terminal grid containing character cells
/// * `row` - Row index (must be < grid.rows)
/// * `col` - Column index to find word around
///
/// # Returns
///
/// * `(usize, usize)` - Start and end column indices (inclusive range)
///
/// # Special Cases
///
/// ## Wide Characters
///
/// CJK characters occupy two cells (main + continuation). If clicking on a
/// continuation cell, the function moves to the main cell first:
///
/// ```text
/// Grid:  你  好  (ni hao - "hello" in Chinese)
/// Cells: [0] [1] [2] [3]
///        main  cont main  cont
///
/// Click on col 1 (continuation):
///   → Moves to col 0 (main cell)
///   → Returns (0, 3) - selects both characters
/// ```
///
/// ## Boundary Cases
///
/// - Clicking on a separator (space, punctuation): returns (col, col)
/// - Out of bounds row: returns (col, col) - no selection
/// - Empty/separator line: returns (col, col) - single character
///
/// # Algorithm
///
/// 1. **Wide character check**: If on continuation cell, move to main cell
/// 2. **Character classification**: Determine if clicked char is a word char
/// 3. **Expand left**: Walk left while chars are word chars
/// 4. **Expand right**: Walk right while chars are word chars
/// 5. **Return**: (start_col, end_col)
///
/// # Example
///
/// ```rust
/// let grid = /* terminal with "echo hello_world" */;
/// let (start, end) = find_word_boundaries(&grid, 0, 8);
/// // start = 5, end = 15 (selects "hello_world")
///
/// let (start, end) = find_word_boundaries(&grid, 0, 10);
/// // start = 5, end = 15 (clicking in middle of "hello_world")
/// ```
///
/// # Note
///
/// This function only works with grid rows, not scrollback rows. When calling
/// from selection code, convert global index to local grid index first:
///
/// ```rust
/// let local_row = if global_row < scrollback_len {
///     /* Can't select scrollback words - not supported */
///     return;
/// } else {
///     global_row - scrollback_len
/// };
/// let (start, end) = find_word_boundaries(&grid, local_row, col);
/// ```
pub fn find_word_boundaries(grid: &terminal::TerminalGrid, row: usize, col: usize) -> (usize, usize) {
    if row >= grid.rows {
        return (col, col);
    }
    let cells = &grid.cells[row];
    let cell = cells.get(col);

    // Handle wide character continuation - if we're on a continuation cell,
    // move to the start of the wide character
    let actual_col = if let Some(c) = cell {
        if c.wide_continuation && col > 0 {
            // Find the start of this wide character
            let mut search = col - 1;
            while search > 0 && cells[search].wide_continuation {
                search -= 1;
            }
            search
        } else {
            col
        }
    } else {
        col
    };

    let ch = cells.get(actual_col).map(|c| c.c).unwrap_or(' ');

    // Word characters: ASCII alphanumeric, underscore, and CJK ideographs
    let is_word_char = |c: char| -> bool {
        // ASCII word characters
        if c.is_ascii_alphanumeric() || c == '_' {
            return true;
        }
        // CJK Unified Ideographs - treat each as individual word character
        let cp = c as u32;
        (0x4E00..=0x9FFF).contains(&cp) || // Common CJK
        (0x3400..=0x4DBF).contains(&cp) || // CJK Extension A
        (0x20000..=0x2A6DF).contains(&cp) || // CJK Extension B
        (0x2A700..=0x2B73F).contains(&cp) || // CJK Extension C
        (0x2B740..=0x2B81F).contains(&cp) || // CJK Extension D
        (0x2B820..=0x2CEAF).contains(&cp) || // CJK Extension E
        (0xF900..=0xFAFF).contains(&cp) || // CJK Compatibility Ideographs
        (0x2F800..=0x2FA1F).contains(&cp) || // CJK Compatibility Ideographs Supplement
        false
    };

    if !is_word_char(ch) {
        return (col, col);
    }

    // For CJK characters, each character is independent - only select the clicked character
    let cp = ch as u32;
    let is_cjk = (0x4E00..=0x9FFF).contains(&cp) ||
                  (0x3400..=0x4DBF).contains(&cp) ||
                  (0x20000..=0x2A6DF).contains(&cp) ||
                  (0x2A700..=0x2B73F).contains(&cp) ||
                  (0x2B740..=0x2B81F).contains(&cp) ||
                  (0x2B820..=0x2CEAF).contains(&cp) ||
                  (0xF900..=0xFAFF).contains(&cp) ||
                  (0x2F800..=0x2FA1F).contains(&cp);

    if is_cjk {
        // CJK: select only this character (may include its continuation cell)
        let mut end = actual_col;
        if end + 1 < cells.len() && cells[end + 1].wide_continuation {
            end += 1;
        }
        return (actual_col, end);
    }

    // For ASCII words, find the full word boundary
    let mut start = actual_col;
    while start > 0 {
        let prev_idx = start - 1;
        let prev_cell = &cells[prev_idx];

        // Skip wide character continuation cells
        if prev_cell.wide_continuation {
            start = prev_idx;
            continue;
        }

        // Stop at non-word characters or CJK
        let prev_cp = prev_cell.c as u32;
        let prev_is_cjk = (0x4E00..=0x9FFF).contains(&prev_cp) ||
                           (0x3400..=0x4DBF).contains(&prev_cp);

        if !is_word_char(prev_cell.c) || prev_is_cjk {
            break;
        }

        start = prev_idx;
    }

    let mut end = actual_col;
    while end + 1 < cells.len().min(grid.cols) {
        let next_idx = end + 1;
        let next_cell = &cells[next_idx];

        // Skip wide character continuation cells
        if next_cell.wide_continuation {
            end = next_idx;
            continue;
        }

        // Stop at non-word characters or CJK
        let next_cp = next_cell.c as u32;
        let next_is_cjk = (0x4E00..=0x9FFF).contains(&next_cp) ||
                           (0x3400..=0x4DBF).contains(&next_cp);

        if !is_word_char(next_cell.c) || next_is_cjk {
            break;
        }

        end = next_idx;

        // If this is a wide character, include its continuation
        if end + 1 < cells.len() && cells[end + 1].wide_continuation {
            end += 1;
        }
    }

    (start, end)
}

/// Build a colored egui LayoutJob for rendering a terminal row.
///
/// This function converts terminal cell data into an egui text layout job, handling:
/// - Character filtering (removes wide character continuation placeholders)
/// - Color preservation (foreground and background from VTE)
/// - Text attributes (bold, italic, underline)
/// - Run-length optimization (groups cells with same attributes)
///
/// # Arguments
///
/// * `cells` - Terminal cell data for this row
/// * `font_id` - egui font identifier (typically monospace)
/// * `cols` - Number of columns to include (max(calls.len(), grid.cols))
///
/// # Returns
///
/// * `LayoutJob` - egui text layout job ready for rendering
///
/// # Layout Job Structure
///
/// The layout job consists of multiple "runs" - contiguous character sequences
/// with the same formatting. Each run specifies:
/// - Text substring (byte range into the full text)
/// - Font ID
/// - Color (foreground)
///
/// ## Example
///
/// ```text
/// Terminal cells (with VTE colors):
///   [H] [e] [l] [l] [o] [ ] [🌍]
///    red red red red red white blue
///
/// → Text: "Hello 🌍"
/// → Runs:
///   - "Hello" with red color
///   - " " with white color
///   - "🌍" with blue color
/// ```
///
/// # Wide Character Handling
///
/// CJK characters occupy two grid cells (main + continuation). This function
/// automatically filters out continuation cells:
///
/// ```text
/// Grid cells: [A] [你] [好] [B]
///             (main) (main)
/// Cells iter:  0    1    2    3
/// Wide cont:   false false true false
///
/// → Visible chars: [A, 你, B]
/// → Continuation cell at index 3 is filtered out
/// ```
///
/// # Color Format
///
/// Terminal cells store colors as RGB tuples (u8, u8, u8). These are converted
/// to egui's Color32 format (RGBA premultiplied).
///
/// Background colors use a special value (`DEFAULT_BG = (255, 255, 255)`) to
/// indicate transparent (inherits from terminal background).
///
/// # Performance Notes
///
/// ## Run-Length Optimization
///
/// Instead of creating one layout section per character, the function groups
/// consecutive characters with identical attributes into runs:
///
/// ```text
/// Before: 100 chars → 100 layout sections
/// After: 100 chars → 1-5 layout sections (typical)
/// ```
///
/// This significantly reduces egui's internal overhead.
///
/// ## Memory Allocation
///
/// - `visible` vector: Pre-allocated with `cols` capacity
/// - `text` string: Pre-allocated with `visible.len()` capacity
/// - Byte offsets: Calculated on-demand using `char().len_utf8()`
///
/// # See Also
///
/// * `TerminalCell` - Terminal cell data structure
/// * `egui::text::LayoutJob` - egui text layout API
/// * `vte` crate - ANSI escape sequence parsing
pub fn build_row_layout(
    cells: &[terminal::TerminalCell],
    font_id: &egui::FontId,
    cols: usize,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.break_on_newline = false;
    job.wrap = egui::text::TextWrapping {
        max_width: f32::INFINITY,
        ..Default::default()
    };

    if cols == 0 {
        return job;
    }

    // Build filtered list of visible cells (skip wide_continuation placeholders)
    let mut visible: Vec<&terminal::TerminalCell> = Vec::with_capacity(cols);
    for cell in cells.iter().take(cols) {
        if !cell.wide_continuation {
            visible.push(cell);
        }
    }

    if visible.is_empty() {
        return job;
    }

    let mut text = String::with_capacity(visible.len());
    for cell in &visible {
        text.push(cell.c);
    }

    let mut run_start = 0;
    let mut run_fg = visible[0].fg_color;
    let mut run_bg = visible[0].bg_color;
    let mut run_bold = visible[0].bold;
    let mut run_italic = visible[0].italic;
    let mut run_underline = visible[0].underline;

    let vlen = visible.len();
    for vi in 0..=vlen {
        let same = vi < vlen
            && visible[vi].fg_color == run_fg
            && visible[vi].bg_color == run_bg
            && visible[vi].bold == run_bold
            && visible[vi].italic == run_italic
            && visible[vi].underline == run_underline;

        if !same && run_start < vi {
            let byte_start: usize = text.chars().take(run_start).map(|c| c.len_utf8()).sum();
            let byte_end: usize = text.chars().take(vi).map(|c| c.len_utf8()).sum();

            let fg = egui::Color32::from_rgb(run_fg.0, run_fg.1, run_fg.2);
            let bg = if run_bg == terminal::DEFAULT_BG {
                egui::Color32::TRANSPARENT
            } else {
                egui::Color32::from_rgb(run_bg.0, run_bg.1, run_bg.2)
            };

            let format = egui::TextFormat {
                font_id: font_id.clone(),
                color: fg,
                background: bg,
                italics: run_italic,
                underline: if run_underline {
                    egui::Stroke::new(1.0, fg)
                } else {
                    egui::Stroke::NONE
                },
                ..Default::default()
            };

            job.sections.push(egui::text::LayoutSection {
                leading_space: 0.0,
                byte_range: byte_start..byte_end,
                format,
            });

            if vi < vlen {
                run_start = vi;
                run_fg = visible[vi].fg_color;
                run_bg = visible[vi].bg_color;
                run_bold = visible[vi].bold;
                run_italic = visible[vi].italic;
                run_underline = visible[vi].underline;
            }
        } else if !same && vi < vlen {
            run_start = vi;
            run_fg = visible[vi].fg_color;
            run_bg = visible[vi].bg_color;
            run_bold = visible[vi].bold;
            run_italic = visible[vi].italic;
            run_underline = visible[vi].underline;
        }
    }

    job.text = text;
    job
}
