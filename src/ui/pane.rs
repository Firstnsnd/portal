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
    dialogs::{AppView, BroadcastState, AddHostDialog, CredentialDialog, SnippetViewState, HostFilter, AddTunnelDialog},
    sftp_types::{SftpContextMenu, SftpRenameDialog, SftpNewFolderDialog, SftpNewFileDialog, SftpConfirmDelete, SftpEditorDialog, SftpErrorDialog},
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
    /// Whether the snippet drawer is open for this tab
    pub snippet_drawer_open: bool,
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
    pub next_id: usize,
    pub tab_drag: TabDragState,
    #[allow(dead_code)]
    pub broadcast_state: BroadcastState,

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
    pub sftp_editor_dialog: Option<SftpEditorDialog>,
    pub sftp_error_dialog: Option<SftpErrorDialog>,
    pub sftp_local_left_refresh_start: Option<std::time::Instant>,
    pub sftp_local_right_refresh_start: Option<std::time::Instant>,
    pub sftp_remote_refresh_start: Option<std::time::Instant>,
    pub sftp_left_remote_refresh_start: Option<std::time::Instant>,
    pub sftp_active_panel_is_local: bool,

    // Page-related dialog states (per-window)
    pub add_host_dialog: AddHostDialog,
    pub credential_dialog: CredentialDialog,
    pub snippet_view_state: SnippetViewState,
    pub host_filter: HostFilter,
    pub confirm_delete_host: Option<usize>,
    pub add_tunnel_dialog: AddTunnelDialog,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a minimal Tab for testing
    fn create_test_tab(title: &str) -> Tab {
        Tab {
            title: title.to_string(),
            sessions: vec![],
            layout: PaneNode::Terminal(0),
            focused_session: 0,
            broadcast_enabled: false,
            snippet_drawer_open: false,
        }
    }

    #[test]
    fn test_tab_default_snippet_drawer_closed() {
        let tab = create_test_tab("Test Tab");
        assert!(!tab.snippet_drawer_open, "New tabs should have snippet drawer closed by default");
    }

    #[test]
    fn test_snippet_drawer_state_per_tab() {
        // Create two tabs
        let mut tab_a = create_test_tab("Tab A");
        let mut tab_b = create_test_tab("Tab B");

        // Open snippet drawer on Tab A only
        tab_a.snippet_drawer_open = true;
        assert!(tab_a.snippet_drawer_open, "Tab A should have snippet drawer open");
        assert!(!tab_b.snippet_drawer_open, "Tab B should remain closed");

        // Close Tab A's drawer
        tab_a.snippet_drawer_open = false;
        assert!(!tab_a.snippet_drawer_open, "Tab A should now be closed");

        // Open Tab B's drawer
        tab_b.snippet_drawer_open = true;
        assert!(tab_b.snippet_drawer_open, "Tab B should have snippet drawer open");
        assert!(!tab_a.snippet_drawer_open, "Tab A should remain closed");
    }

    #[test]
    fn test_snippet_drawer_state_preserved_on_tab_switch() {
        // Simulate multiple tabs with independent drawer states
        let mut tabs = vec![
            create_test_tab("Tab 1"),
            create_test_tab("Tab 2"),
            create_test_tab("Tab 3"),
        ];

        // Open drawer on tabs 0 and 2, leave tab 1 closed
        tabs[0].snippet_drawer_open = true;
        tabs[2].snippet_drawer_open = true;

        // Simulate switching tabs and verifying states are preserved
        let active_tab = 0;
        assert!(tabs[active_tab].snippet_drawer_open, "Tab 0 should have drawer open");

        let active_tab = 1;
        assert!(!tabs[active_tab].snippet_drawer_open, "Tab 1 should have drawer closed");

        let active_tab = 2;
        assert!(tabs[active_tab].snippet_drawer_open, "Tab 2 should have drawer open");

        // Switch back to tab 0
        let active_tab = 0;
        assert!(tabs[active_tab].snippet_drawer_open, "Tab 0 drawer should still be open after switching");
    }

    #[test]
    fn test_multiple_tabs_independent_state() {
        // Create a vector of tabs
        let mut tabs: Vec<Tab> = (0..5).map(|i| {
            let mut tab = create_test_tab(&format!("Tab {}", i));
            // Alternate drawer states
            tab.snippet_drawer_open = i % 2 == 0;
            tab
        }).collect();

        // Verify each tab maintains its independent state
        for (i, tab) in tabs.iter().enumerate() {
            let expected_open = i % 2 == 0;
            assert_eq!(
                tab.snippet_drawer_open,
                expected_open,
                "Tab {} should have drawer {}",
                i,
                if expected_open { "open" } else { "closed" }
            );
        }

        // Modify one tab's state
        tabs[2].snippet_drawer_open = false;

        // Verify other tabs are unaffected
        assert!(tabs[0].snippet_drawer_open, "Tab 0 should be unaffected");
        assert!(!tabs[1].snippet_drawer_open, "Tab 1 should be unaffected");
        assert!(!tabs[2].snippet_drawer_open, "Tab 2 should now be closed");
        assert!(!tabs[3].snippet_drawer_open, "Tab 3 should be unaffected (was closed originally)");
        assert!(tabs[4].snippet_drawer_open, "Tab 4 should be unaffected");
    }
}
