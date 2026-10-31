//! # Split Pane Layout System
//!
//! This module implements a recursive pane tree layout system that supports:
//! - **Horizontal splits**: Left/right panes
//! - **Vertical splits**: Top/bottom panes
//! - **Resizable dividers**: Drag to adjust split ratio
//! - **Unlimited nesting**: Recursive split depth
//! - **Dynamic restructuring**: Add/remove panes at runtime
//!
//! ## Data Structures
//!
//! ### PaneNode
//!
//! The core data structure representing the pane tree:
//!
//! ```text
//! PaneNode::Terminal(session_id)  // Leaf node - actual terminal
//! PaneNode::Split {               // Internal node - split pane
//!     direction: Horizontal/Vertical,
//!     ratio: 0.0-1.0,             // Split position (0.5 = center)
//!     first: Box<PaneNode>,      // Left or top child
//!     second: Box<PaneNode>,     // Right or bottom child
//! }
//! ```
//!
//! ### Example Tree
//!
//! ```text
//! Split(Vertical, 0.6)           // Top 60%, Bottom 40%
//! ├── Split(Horizontal, 0.5)     // Top half split 50/50
//! │   ├── Terminal(0)             // Top-left pane
//! │   └── Terminal(1)             // Top-right pane
//! └── Terminal(2)                 // Bottom pane
//! ```
//!
//! ## Operations
//!
//! ### replace
//! Replace a terminal node with another node (used for closing panes).
//!
//! ### remove
//! Remove a terminal node and collapse the tree:
//! - If removing from a split, collapse to the remaining child
//! - If only one child remains, replace the split with that child
//! - If removing the root, return None (drop entire tree)
//!
//! ### decrement_indices_above
//! After removing a session, decrement all higher indices to maintain continuity.
//!
//! ### offset_indices
//! Add offset to all indices (used when merging tabs).
//!
//! ## Coordinate System
//!
//! ### split_rect
//! Split a rectangle into two sub-rectangles based on direction and ratio.
//! Accounts for a 2-pixel divider gap.
//!
//! ```text
//! Horizontal split (ratio = 0.5):
//!   ┌─────────────┬─────────────┐
//!   │   Left       │   Right      │
//!   │   (50%)      │   (50%)      │
//!   └─────────────┴─────────────┘
//!
//! Vertical split (ratio = 0.3):
//!   ┌─────────────┐
//!   │    Top     │ (30%)
//!   ├─────────────┤
//!   │   Bottom   │ (70%)
//!   └─────────────┘
//! ```

use eframe::egui;

use crate::sftp::{LocalBrowser, SftpBrowser};
use crate::ui::types::{
    session::TerminalSession,
    dialogs::{AppView, BroadcastState, AddHostDialog, CredentialDialog, SnippetViewState, HostFilter},
    sftp_types::{SftpContextMenu, SftpRenameDialog, SftpNewFolderDialog, SftpNewFileDialog, SftpConfirmDelete, SftpEmbeddedEditor, SftpErrorDialog, SftpPanel, SftpSide},
};

/// Split direction for pane layout
#[derive(Clone, Copy, PartialEq)]
pub enum SplitDirection {
    Horizontal, // left | right
    Vertical,   // top / bottom
}

/// Pane layout tree node
#[derive(Clone)]
pub enum PaneNode {
    Terminal(usize), // session index
    Split {
        direction: SplitDirection,
        ratio: f32,
        first: Box<PaneNode>,
        second: Box<PaneNode>,
    },
}

impl PaneNode {
    /// Replace a Terminal(target) with a new node
    pub fn replace(&mut self, target: usize, replacement: PaneNode) -> bool {
        match self {
            PaneNode::Terminal(idx) if *idx == target => {
                *self = replacement;
                true
            }
            PaneNode::Split { first, second, .. } => {
                first.replace(target, replacement.clone()) || second.replace(target, replacement)
            }
            _ => false,
        }
    }

    /// Remove Terminal(target) from the tree. Returns the node that should
    /// replace self: None means self was the target and should be dropped by caller.
    pub fn remove(self, target: usize) -> Option<PaneNode> {
        match self {
            PaneNode::Terminal(idx) => {
                if idx == target { None } else { Some(PaneNode::Terminal(idx)) }
            }
            PaneNode::Split { direction, ratio, first, second } => {
                match first.remove(target) {
                    None => Some(*second),  // first was removed → collapse to second
                    Some(new_first) => {
                        match second.remove(target) {
                            None => Some(new_first), // second was removed → collapse to first
                            Some(new_second) => Some(PaneNode::Split {
                                direction, ratio,
                                first: Box::new(new_first),
                                second: Box::new(new_second),
                            }),
                        }
                    }
                }
            }
        }
    }

    /// Decrement all Terminal indices that are strictly greater than `threshold`.
    /// Call after removing session at index `threshold` from the sessions vec.
    pub fn decrement_indices_above(&mut self, threshold: usize) {
        match self {
            PaneNode::Terminal(idx) => {
                if *idx > threshold { *idx -= 1; }
            }
            PaneNode::Split { first, second, .. } => {
                first.decrement_indices_above(threshold);
                second.decrement_indices_above(threshold);
            }
        }
    }

    /// Add `offset` to all Terminal indices in this subtree.
    /// Used when merging another tab's layout into this one.
    pub fn offset_indices(&mut self, offset: usize) {
        match self {
            PaneNode::Terminal(idx) => { *idx += offset; }
            PaneNode::Split { first, second, .. } => {
                first.offset_indices(offset);
                second.offset_indices(offset);
            }
        }
    }
}

/// Split a rect into two parts based on direction and ratio
pub fn split_rect(rect: egui::Rect, direction: SplitDirection, ratio: f32) -> (egui::Rect, egui::Rect) {
    let gap = 2.0; // divider width
    match direction {
        SplitDirection::Horizontal => {
            let split_x = rect.min.x + rect.width() * ratio - gap / 2.0;
            (
                egui::Rect::from_min_max(rect.min, egui::pos2(split_x, rect.max.y)),
                egui::Rect::from_min_max(egui::pos2(split_x + gap, rect.min.y), rect.max),
            )
        }
        SplitDirection::Vertical => {
            let split_y = rect.min.y + rect.height() * ratio - gap / 2.0;
            (
                egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, split_y)),
                egui::Rect::from_min_max(egui::pos2(rect.min.x, split_y + gap), rect.max),
            )
        }
    }
}

/// Action triggered from within a terminal pane
#[derive(Clone, Copy)]
pub enum PaneAction {
    Focus,
    SplitHorizontal,
    SplitVertical,
    ClosePane,
    ToggleBroadcast,
    /// Start/stop asciicast recording on the pane's session
    ToggleRecording,
    /// Open the replay library filtered to the pane session's host
    ViewHostRecordings,
    /// Pick a local file and ZMODEM-upload it into this pane's session
    ZmodemUpload,
    /// Open the remote-path prompt for a ZMODEM download into this pane
    ZmodemDownload,
    /// Cancel the active ZMODEM transfer / dismiss its status chip
    ZmodemCancel,
    /// Hand the pane's selected text to the AI assistant composer (prefill;
    /// the dispatcher re-extracts the selection with `selected_text_of`)
    SendSelectionToAi,
    /// Remove old SSH host key from known_hosts
    RemoveHostKey,
    /// Reconnect a disconnected SSH session
    Reconnect,
}

/// A workspace tab: one or more split panes sharing a single tab entry.
pub struct Tab {
    pub title: String,
    pub sessions: Vec<TerminalSession>,
    pub layout: PaneNode,
    pub focused_session: usize,
    pub broadcast_enabled: bool,
    /// Whether the unified tools drawer (broadcast + snippets + metrics) is open
    pub tools_drawer_open: bool,
    /// Which content tab is active: false = Metrics (default), true = Snippets
    pub tools_tab: u8,  // 0=Metrics 1=Snippets 2=Tunnels
    /// Pending snippet command to execute on next frame (after drawer closes and PTY resizes)
    pub pending_snippet: Option<String>,
    /// AI chat state + draggable orb for this tab.
    pub ai_chat: std::sync::Arc<std::sync::Mutex<crate::ui::state::ai_chat::AiChatState>>,
    /// Command queued by the AI drawer to run on the next frame.
    pub pending_ai_command: Option<String>,
}

/// Per-tab animation state for smooth reordering
#[derive(Clone, Default)]
pub struct TabAnimation {
    /// Current X offset (animation progress)
    current_offset: f32,
    /// Target X offset (where we want to be)
    target_offset: f32,
    /// Start X offset (where animation began)
    start_offset: f32,
    /// Animation start time
    start_time: Option<f64>,
}

/// Drag state for tab ghost visualization
#[derive(Clone, Default)]
pub struct TabDragState {
    pub source_index: Option<usize>,
    pub target_index: Option<usize>,
    pub ghost_title: String,
    pub ghost_size: egui::Vec2,
    /// Insert position: true = insert before target, false = insert after target
    pub insert_before: bool,
    /// true = merge tabs, false = reorder tabs
    pub is_merge: bool,
    /// Per-tab animation states (index in this vec = tab index)
    tab_animations: Vec<TabAnimation>,
}

impl TabDragState {
    /// Ensure animations vector has correct size
    pub fn ensure_size(&mut self, tab_count: usize) {
        while self.tab_animations.len() < tab_count {
            self.tab_animations.push(TabAnimation::default());
        }
        while self.tab_animations.len() > tab_count {
            self.tab_animations.pop();
        }
    }

    /// Update animation target for a specific tab
    pub fn set_target_offset(&mut self, tab_index: usize, offset: f32, current_time: f64) {
        if tab_index < self.tab_animations.len() {
            let anim = &mut self.tab_animations[tab_index];
            if anim.target_offset != offset {
                anim.start_offset = anim.current_offset;
                anim.target_offset = offset;
                anim.start_time = Some(current_time);
            }
        }
    }

    /// Get current animated offset for a tab
    pub fn get_offset(&mut self, tab_index: usize, current_time: f64) -> f32 {
        if tab_index >= self.tab_animations.len() {
            return 0.0;
        }
        let anim = &mut self.tab_animations[tab_index];
        if let Some(start_time) = anim.start_time {
            let elapsed = (current_time - start_time) as f32;
            let duration = 0.25; // 250ms for snappy but smooth animation
            let progress = (elapsed / duration).min(1.0);

            // Ease-out cubic: fast start, slow end
            let eased = 1.0 - (1.0 - progress).powi(3);

            anim.current_offset = anim.start_offset + (anim.target_offset - anim.start_offset) * eased;

            if progress >= 1.0 {
                anim.start_time = None; // Animation complete
            }
        } else {
            anim.current_offset = anim.target_offset;
        }
        anim.current_offset
    }

    /// Reset all animations
    pub fn reset(&mut self) {
        self.source_index = None;
        self.target_index = None;
        self.ghost_title.clear();
        self.ghost_size = egui::Vec2::ZERO;
        self.insert_before = true;
        self.is_merge = false;
        for anim in &mut self.tab_animations {
            anim.current_offset = 0.0;
            anim.target_offset = 0.0;
            anim.start_offset = 0.0;
            anim.start_time = None;
        }
    }
}

/// A window containing one or more tabs
/// All windows are equal - there's no distinction between "main" and "detached"
pub struct AppWindow {
    pub viewport_id: egui::ViewportId,
    pub tabs: Vec<Tab>,
    pub active_tab: usize,
    pub current_view: AppView,
    #[allow(dead_code)]
    pub title: String,
    pub close_requested: bool,
    pub ime_composing: bool,
    pub ime_preedit: String,
    /// Printable chars already written by the Key-event fallback whose
    /// matching Text/Ime-Commit event may still arrive in a later frame
    /// (macOS IME in direct-commit mode splits the delivery). Each entry
    /// suppresses one echo of that char; entries expire after ~250 ms.
    pub key_char_suppress: Vec<(char, std::time::Instant)>,
    pub next_id: usize,
    pub tab_drag: TabDragState,
    #[allow(dead_code)]
    pub broadcast_state: BroadcastState,

    // Asciicast recording / replay state
    /// Replay page state; `None` until a recording is opened.
    pub replay: Option<crate::ui::views::replay_view::ReplayState>,
    /// Failed-to-start recording message shown in the status bar for 5s.
    pub recording_error: Option<(String, std::time::Instant)>,
    /// Replay library row armed for deletion (two-step ✓/✕ confirm); `None`
    /// when no row is armed.
    pub replay_pending_delete: Option<std::path::PathBuf>,
    /// Replay library batch mode: rows show checkboxes for multi-delete.
    pub replay_select_mode: bool,
    /// Paths ticked in batch mode.
    pub replay_selection: std::collections::HashSet<std::path::PathBuf>,
    /// Batch delete button armed (first click of the two-step confirm).
    pub replay_batch_arm: bool,
    /// In-row rename state: (file path, edited title). `None` when idle.
    pub replay_rename: Option<(std::path::PathBuf, String)>,
    /// Replay library host filter (`portal_host`); `None` = show all.
    pub replay_host_filter: Option<String>,
    /// Recently stopped recording — the status bar shows a clickable
    /// "saved" notice that opens it in the player.
    pub recording_saved: Option<(std::path::PathBuf, std::time::Instant)>,
    /// ZMODEM download dialog state: `Some(remote_path_buffer)` while the
    /// small "path on the remote host" prompt is open.
    pub zmodem_download_dialog: Option<String>,

    // SFTP state (per-window, independent connections)
    pub sftp_browser_left: Option<SftpBrowser>,   // Left panel SFTP connection
    pub sftp_browser: Option<SftpBrowser>,        // Right panel SFTP connection
    pub local_browser_left: LocalBrowser,         // Left panel local browser
    pub local_browser_right: LocalBrowser,         // Right panel local browser
    pub left_panel_is_local: bool,                // true = local, false = remote (left panel)
    pub right_panel_is_local: bool,               // true = local, false = remote (right panel)
    pub sftp_context_menu: Option<SftpContextMenu>,
    pub sftp_rename_dialog: Option<SftpRenameDialog>,
    pub sftp_new_folder_dialog: Option<SftpNewFolderDialog>,
    pub sftp_new_file_dialog: Option<SftpNewFileDialog>,
    pub sftp_confirm_delete: Option<SftpConfirmDelete>,
    pub sftp_editor_left: Option<SftpEmbeddedEditor>,  // editor occupying the left half
    pub sftp_editor_right: Option<SftpEmbeddedEditor>, // editor occupying the right half
    pub sftp_error_dialog: Option<SftpErrorDialog>,
    pub sftp_local_left_refresh_start: Option<std::time::Instant>,
    pub sftp_local_right_refresh_start: Option<std::time::Instant>,
    pub sftp_remote_refresh_start: Option<std::time::Instant>,
    pub sftp_left_remote_refresh_start: Option<std::time::Instant>,
    pub sftp_active_panel_is_local: bool,
    pub sftp_active_side_left: bool,            // last-clicked SFTP panel side (drop fallback hint)
    pub sftp_os_drag_active: bool,              // native file-promise drag session in flight

    // Page-related dialog states (per-window)
    pub add_host_dialog: AddHostDialog,
    pub credential_dialog: CredentialDialog,
    pub snippet_view_state: SnippetViewState,
    pub host_filter: HostFilter,
    pub confirm_delete_host: Option<usize>,
    pub notifications: Vec<crate::ssh::AppNotification>,
}

impl AppWindow {
    /// egui Id of the AI composer's TextEdit, scoped to this window's
    /// viewport so detached windows can't collide. The terminal panes query
    /// it to decide when to stand down from re-claiming keyboard focus.
    pub fn ai_composer_id(&self) -> egui::Id {
        egui::Id::new("ai_composer").with(self.viewport_id)
    }

    /// Deliver an async remote file read to the embedded editor that
    /// requested it. Safe no-op when the editor was closed (or replaced by
    /// another file) mid-flight.
    pub fn sftp_editor_deliver_file_content(&mut self, panel: SftpPanel, path: String, data: Vec<u8>) {
        let slot = match panel.side() {
            SftpSide::Left => &mut self.sftp_editor_left,
            SftpSide::Right => &mut self.sftp_editor_right,
        };
        let Some(editor) = slot.as_mut() else { return };
        if !editor.loading || editor.panel != panel {
            return;
        }
        match String::from_utf8(data) {
            Ok(text) => {
                editor.content = text.clone();
                editor.original_content = text;
                editor.file_path = path;
                editor.loading = false;
            }
            Err(_) => {
                editor.error = "Not valid UTF-8".to_string();
                editor.loading = false;
            }
        }
    }

    /// Deliver a "file exceeds the 10 MB read cap" reply to the requesting
    /// editor (same routing rules as [`Self::sftp_editor_deliver_file_content`]).
    pub fn sftp_editor_deliver_file_too_large(&mut self, panel: SftpPanel, size: u64) {
        let slot = match panel.side() {
            SftpSide::Left => &mut self.sftp_editor_left,
            SftpSide::Right => &mut self.sftp_editor_right,
        };
        let Some(editor) = slot.as_mut() else { return };
        if !editor.loading || editor.panel != panel {
            return;
        }
        editor.loading = false;
        editor.error = format!("File too large ({} bytes)", size);
    }

    /// Create a fresh window around a single tab with fresh SFTP/dialog state.
    /// Used for the root window at startup, for detached tabs, and for
    /// "New Portal Window Here".
    pub fn new_window(
        viewport_id: egui::ViewportId,
        title: impl Into<String>,
        tab: Tab,
        next_id: usize,
    ) -> Self {
        Self {
            viewport_id,
            title: title.into(),
            tabs: vec![tab],
            active_tab: 0,
            current_view: AppView::Terminal,
            close_requested: false,
            ime_composing: false,
            ime_preedit: String::new(),
            key_char_suppress: Vec::new(),
            next_id,
            tab_drag: TabDragState::default(),
            broadcast_state: BroadcastState::default(),
            replay: None,
            recording_error: None,
            replay_pending_delete: None,
            replay_select_mode: false,
            replay_selection: std::collections::HashSet::new(),
            replay_batch_arm: false,
            replay_rename: None,
            replay_host_filter: None,
            recording_saved: None,
            zmodem_download_dialog: None,
            sftp_browser_left: None,
            sftp_browser: None,
            local_browser_left: LocalBrowser::new(),
            local_browser_right: LocalBrowser::new(),
            left_panel_is_local: true,
            right_panel_is_local: false,
            sftp_context_menu: None,
            sftp_rename_dialog: None,
            sftp_new_folder_dialog: None,
            sftp_new_file_dialog: None,
            sftp_confirm_delete: None,
            sftp_editor_left: None,
            sftp_editor_right: None,
            sftp_error_dialog: None,
            sftp_local_left_refresh_start: None,
            sftp_local_right_refresh_start: None,
            sftp_remote_refresh_start: None,
            sftp_left_remote_refresh_start: None,
            sftp_active_panel_is_local: true,
            sftp_active_side_left: true,
            sftp_os_drag_active: false,
            add_host_dialog: AddHostDialog::default(),
            credential_dialog: CredentialDialog::default(),
            snippet_view_state: SnippetViewState::default(),
            host_filter: HostFilter::default(),
            confirm_delete_host: None,
            notifications: Vec::new(),
        }
    }
}

/// Minimal Tab for tests in other modules (pane-local tests use
/// [`create_test_tab`], which delegates here).
#[cfg(test)]
pub(crate) fn test_tab(title: &str) -> Tab {
    Tab {
        title: title.to_string(),
        sessions: vec![],
        layout: PaneNode::Terminal(0),
        focused_session: 0,
        broadcast_enabled: false,
        tools_drawer_open: false,
        tools_tab: 0,
        pending_snippet: None,
        ai_chat: std::sync::Arc::new(std::sync::Mutex::new(
            crate::ui::state::ai_chat::AiChatState::default(),
        )),
        pending_ai_command: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a minimal Tab for testing
    fn create_test_tab(title: &str) -> Tab {
        test_tab(title)
    }

    #[test]
    fn test_tab_default_tools_drawer_closed() {
        let tab = create_test_tab("Test Tab");
        assert!(!tab.tools_drawer_open, "New tabs should have tools drawer closed by default");
    }

    #[test]
    fn test_tools_drawer_state_per_tab() {
        // Create two tabs
        let mut tab_a = create_test_tab("Tab A");
        let mut tab_b = create_test_tab("Tab B");

        // Open tools drawer on Tab A only
        tab_a.tools_drawer_open = true;
        assert!(tab_a.tools_drawer_open, "Tab A should have tools drawer open");
        assert!(!tab_b.tools_drawer_open, "Tab B should remain closed");

        // Close Tab A's drawer
        tab_a.tools_drawer_open = false;
        assert!(!tab_a.tools_drawer_open, "Tab A should now be closed");

        // Open Tab B's drawer
        tab_b.tools_drawer_open = true;
        assert!(tab_b.tools_drawer_open, "Tab B should have tools drawer open");
        assert!(!tab_a.tools_drawer_open, "Tab A should remain closed");
    }

    #[test]
    fn test_snippet_drawer_state_preserved_on_tab_switch() {
        // Simulate multiple tabs with independent drawer states
        let mut tabs = [create_test_tab("Tab 1"),
            create_test_tab("Tab 2"),
            create_test_tab("Tab 3")];

        // Open drawer on tabs 0 and 2, leave tab 1 closed
        tabs[0].tools_drawer_open = true;
        tabs[2].tools_drawer_open = true;

        // Simulate switching tabs and verifying states are preserved
        let active_tab = 0;
        assert!(tabs[active_tab].tools_drawer_open, "Tab 0 should have drawer open");

        let active_tab = 1;
        assert!(!tabs[active_tab].tools_drawer_open, "Tab 1 should have drawer closed");

        let active_tab = 2;
        assert!(tabs[active_tab].tools_drawer_open, "Tab 2 should have drawer open");

        // Switch back to tab 0
        let active_tab = 0;
        assert!(tabs[active_tab].tools_drawer_open, "Tab 0 drawer should still be open after switching");
    }

    #[test]
    fn test_multiple_tabs_independent_state() {
        // Create a vector of tabs
        let mut tabs: Vec<Tab> = (0..5).map(|i| {
            let mut tab = create_test_tab(&format!("Tab {}", i));
            // Alternate drawer states
            tab.tools_drawer_open = i % 2 == 0;
            tab
        }).collect();

        // Verify each tab maintains its independent state
        for (i, tab) in tabs.iter().enumerate() {
            let expected_open = i % 2 == 0;
            assert_eq!(
                tab.tools_drawer_open,
                expected_open,
                "Tab {} should have drawer {}",
                i,
                if expected_open { "open" } else { "closed" }
            );
        }

        // Modify one tab's state
        tabs[2].tools_drawer_open = false;

        // Verify other tabs are unaffected
        assert!(tabs[0].tools_drawer_open, "Tab 0 should be unaffected");
        assert!(!tabs[1].tools_drawer_open, "Tab 1 should be unaffected");
        assert!(!tabs[2].tools_drawer_open, "Tab 2 should now be closed");
        assert!(!tabs[3].tools_drawer_open, "Tab 3 should be unaffected (was closed originally)");
        assert!(tabs[4].tools_drawer_open, "Tab 4 should be unaffected");
    }

    // ── PaneNode lifecycle tests ──────────────────────────────────────────
    //
    // These tests verify the pane tree operations used when closing sessions
    // and managing split layouts. Regressions here cause zombie panes,
    // index corruption, and crash-on-close bugs.

    /// remove(Terminal(N)) on a single terminal returns None.
    #[test]
    fn test_pane_node_remove_single_terminal() {
        let node = PaneNode::Terminal(0);
        let result = node.remove(0);
        assert!(result.is_none(), "Removing the only terminal must return None");
    }

    /// remove(Terminal(N)) on a different terminal returns Some(Terminal(N)).
    #[test]
    fn test_pane_node_remove_wrong_terminal() {
        let node = PaneNode::Terminal(5);
        let result = node.remove(3);
        assert!(matches!(result, Some(PaneNode::Terminal(5))),
            "Removing a non-matching terminal must return the original node");
    }

    /// remove on a Split collapses to the surviving child.
    #[test]
    fn test_pane_node_remove_from_split_collapses() {
        // Split(Terminal(0), Terminal(1))
        let split = PaneNode::Split {
            direction: SplitDirection::Horizontal,
            ratio: 0.5,
            first: Box::new(PaneNode::Terminal(0)),
            second: Box::new(PaneNode::Terminal(1)),
        };

        // Remove first child → collapses to second
        let result = split.remove(0);
        assert!(matches!(result, Some(PaneNode::Terminal(1))),
            "Removing first child must collapse to second");

        // Remove second child → collapses to first
        let split = PaneNode::Split {
            direction: SplitDirection::Horizontal,
            ratio: 0.5,
            first: Box::new(PaneNode::Terminal(0)),
            second: Box::new(PaneNode::Terminal(1)),
        };
        let result = split.remove(1);
        assert!(matches!(result, Some(PaneNode::Terminal(0))),
            "Removing second child must collapse to first");
    }

    /// remove on a deep tree removes the target and collapses correctly.
    #[test]
    fn test_pane_node_remove_deep_tree() {
        //     Split
        //    /     \
        //  T(0)   Split
        //         /    \
        //       T(1)  T(2)
        let tree = PaneNode::Split {
            direction: SplitDirection::Vertical,
            ratio: 0.5,
            first: Box::new(PaneNode::Terminal(0)),
            second: Box::new(PaneNode::Split {
                direction: SplitDirection::Horizontal,
                ratio: 0.5,
                first: Box::new(PaneNode::Terminal(1)),
                second: Box::new(PaneNode::Terminal(2)),
            }),
        };

        // Remove T(1) → right side collapses to T(2)
        let result = tree.remove(1);
        assert!(result.is_some(), "remove(1) must return Some");
        let tree = result.unwrap();
        // Should be Split(T(0), T(2))
        match &tree {
            PaneNode::Split { first, second, .. } => {
                assert!(matches!(**first, PaneNode::Terminal(0)), "first must be T(0)");
                assert!(matches!(**second, PaneNode::Terminal(2)), "second must be T(2)");
            }
            _ => panic!("Expected Split, got Terminal"),
        }
    }

    /// decrement_indices_above shifts indices > threshold down by 1.
    #[test]
    fn test_pane_node_decrement_indices_above() {
        let mut tree = PaneNode::Split {
            direction: SplitDirection::Horizontal,
            ratio: 0.5,
            first: Box::new(PaneNode::Terminal(0)),
            second: Box::new(PaneNode::Split {
                direction: SplitDirection::Vertical,
                ratio: 0.5,
                first: Box::new(PaneNode::Terminal(2)),
                second: Box::new(PaneNode::Terminal(4)),
            }),
        };

        // After removing session at index 1, decrement indices > 1
        tree.decrement_indices_above(1);

        // Index 0 → unchanged, Index 2 → 1, Index 4 → 3
        match &tree {
            PaneNode::Split { first, second, .. } => {
                assert!(matches!(**first, PaneNode::Terminal(0)), "Index 0 unchanged");
                match &**second {
                    PaneNode::Split { first, second, .. } => {
                        assert!(matches!(**first, PaneNode::Terminal(1)), "Index 2 → 1");
                        assert!(matches!(**second, PaneNode::Terminal(3)), "Index 4 → 3");
                    }
                    _ => panic!("Expected nested Split"),
                }
            }
            _ => panic!("Expected Split"),
        }
    }

    /// decrement_indices_above does not affect indices <= threshold.
    #[test]
    fn test_pane_node_decrement_preserves_lower_indices() {
        let mut tree = PaneNode::Split {
            direction: SplitDirection::Horizontal,
            ratio: 0.5,
            first: Box::new(PaneNode::Terminal(0)),
            second: Box::new(PaneNode::Terminal(1)),
        };

        tree.decrement_indices_above(5);

        // No indices > 5, nothing changes
        assert!(matches!(tree, PaneNode::Split { .. }));
    }

    /// replace(Terminal(N), new_node) correctly substitutes the target.
    #[test]
    fn test_pane_node_replace_target() {
        let mut tree = PaneNode::Split {
            direction: SplitDirection::Horizontal,
            ratio: 0.5,
            first: Box::new(PaneNode::Terminal(0)),
            second: Box::new(PaneNode::Terminal(1)),
        };

        let replaced = tree.replace(1, PaneNode::Terminal(99));
        assert!(replaced, "replace must return true when target is found");

        // Verify the replacement took effect
        match &tree {
            PaneNode::Split { first, second, .. } => {
                assert!(matches!(**first, PaneNode::Terminal(0)));
                assert!(matches!(**second, PaneNode::Terminal(99)));
            }
            _ => panic!("Expected Split"),
        }
    }

    /// replace returns false when target is not in the tree.
    #[test]
    fn test_pane_node_replace_not_found() {
        let mut tree = PaneNode::Terminal(0);
        let replaced = tree.replace(99, PaneNode::Terminal(1));
        assert!(!replaced, "replace must return false when target not found");
    }

    /// offset_indices adds offset to all terminal indices.
    #[test]
    fn test_pane_node_offset_indices() {
        let mut tree = PaneNode::Split {
            direction: SplitDirection::Horizontal,
            ratio: 0.5,
            first: Box::new(PaneNode::Terminal(0)),
            second: Box::new(PaneNode::Terminal(1)),
        };

        tree.offset_indices(5);

        match &tree {
            PaneNode::Split { first, second, .. } => {
                assert!(matches!(**first, PaneNode::Terminal(5)));
                assert!(matches!(**second, PaneNode::Terminal(6)));
            }
            _ => panic!("Expected Split"),
        }
    }

    /// Full lifecycle: split, remove, decrement — simulates closing a pane.
    #[test]
    fn test_pane_lifecycle_close_focused_pane() {
        // Start: Split(T(0), T(1)) — two panes, focused on T(1)
        let mut tree = PaneNode::Split {
            direction: SplitDirection::Horizontal,
            ratio: 0.5,
            first: Box::new(PaneNode::Terminal(0)),
            second: Box::new(PaneNode::Terminal(1)),
        };

        // Step 1: Remove T(1)
        let new_tree = tree.remove(1);
        assert!(matches!(new_tree, Some(PaneNode::Terminal(0))),
            "After closing T(1), tree must collapse to T(0)");
        tree = new_tree.unwrap();

        // Step 2: decrement_indices_above(1) — no-op since tree has only T(0)
        tree.decrement_indices_above(1);
        assert!(matches!(tree, PaneNode::Terminal(0)));
    }

    /// Full lifecycle with 3 sessions: close middle, verify indices adjust.
    #[test]
    fn test_pane_lifecycle_close_middle_session() {
        // Split(T(0), Split(T(1), T(2)))
        let mut tree = PaneNode::Split {
            direction: SplitDirection::Horizontal,
            ratio: 0.5,
            first: Box::new(PaneNode::Terminal(0)),
            second: Box::new(PaneNode::Split {
                direction: SplitDirection::Vertical,
                ratio: 0.5,
                first: Box::new(PaneNode::Terminal(1)),
                second: Box::new(PaneNode::Terminal(2)),
            }),
        };

        // Close session 1 (middle)
        let new_tree = tree.remove(1);
        tree = new_tree.expect("remove must return Some for multi-node tree");
        tree.decrement_indices_above(1);

        // Expected: Split(T(0), T(1)) — T(2) shifted down to T(1)
        match &tree {
            PaneNode::Split { first, second, .. } => {
                assert!(matches!(**first, PaneNode::Terminal(0)));
                assert!(matches!(**second, PaneNode::Terminal(1)),
                    "Index 2 must be decremented to 1 after removing session at index 1");
            }
            _ => panic!("Tree must remain a Split after closing one of 3 panes"),
        }
    }

    fn make_loading_editor(panel: SftpPanel) -> SftpEmbeddedEditor {
        SftpEmbeddedEditor {
            panel,
            file_path: "/srv/a.txt".to_string(),
            file_name: "a.txt".to_string(),
            directory: "/srv".to_string(),
            content: String::new(),
            original_content: String::new(),
            loading: true,
            error: String::new(),
            confirm_close: false,
        }
    }

    #[test]
    fn test_sftp_editor_delivery_routes_by_side() {
        let mut window = AppWindow::new_window(
            egui::ViewportId(egui::Id::new("test-editor-win")),
            "Portal",
            create_test_tab("Tab 1"),
            1,
        );
        window.sftp_editor_left = Some(make_loading_editor(SftpPanel::LeftRemote));

        // The matching payload lands in the left slot.
        window.sftp_editor_deliver_file_content(SftpPanel::LeftRemote, "/srv/a.txt".to_string(), b"hello".to_vec());
        {
            let ed = window.sftp_editor_left.as_ref().unwrap();
            assert_eq!(ed.content, "hello");
            assert_eq!(ed.original_content, "hello");
            assert_eq!(ed.file_path, "/srv/a.txt");
            assert!(!ed.loading);
            assert!(!ed.is_dirty());
        }

        // A payload for the other side's remote panel must not touch it.
        window.sftp_editor_deliver_file_content(SftpPanel::RightRemote, "/x".to_string(), b"nope".to_vec());
        assert_eq!(window.sftp_editor_left.as_ref().unwrap().content, "hello");
        assert!(window.sftp_editor_right.is_none());

        // A non-loading editor ignores further deliveries.
        window.sftp_editor_deliver_file_content(SftpPanel::LeftRemote, "/srv/a.txt".to_string(), b"late".to_vec());
        assert_eq!(window.sftp_editor_left.as_ref().unwrap().content, "hello");

        // Closed editor → delivery is a safe no-op (no panic).
        window.sftp_editor_left = None;
        window.sftp_editor_deliver_file_content(SftpPanel::LeftRemote, "/srv/a.txt".to_string(), b"boom".to_vec());
        assert!(window.sftp_editor_left.is_none());
    }

    #[test]
    fn test_sftp_editor_too_large_delivery() {
        let mut window = AppWindow::new_window(
            egui::ViewportId(egui::Id::new("test-editor-win2")),
            "Portal",
            create_test_tab("Tab 1"),
            1,
        );
        window.sftp_editor_right = Some(make_loading_editor(SftpPanel::RightRemote));

        window.sftp_editor_deliver_file_too_large(SftpPanel::RightRemote, 12_582_912);
        {
            let ed = window.sftp_editor_right.as_ref().unwrap();
            assert!(!ed.loading);
            assert!(ed.error.contains("File too large"), "error: {}", ed.error);
            assert!(ed.error.contains("12582912"), "error: {}", ed.error);
        }

        // Mismatched panel is ignored.
        window.sftp_editor_deliver_file_too_large(SftpPanel::LeftRemote, 1);
        assert!(!window.sftp_editor_right.as_ref().unwrap().error.contains("(1 bytes)"));
    }
}
