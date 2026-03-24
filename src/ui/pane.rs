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

use crate::ui::types::{session::TerminalSession, dialogs::{AppView, BroadcastState}};

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
}

/// A workspace tab: one or more split panes sharing a single tab entry.
pub struct Tab {
    pub title: String,
    pub sessions: Vec<TerminalSession>,
    pub layout: PaneNode,
    pub focused_session: usize,
    pub broadcast_enabled: bool,
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

/// A tab that has been detached into its own OS window (full UI)
pub struct DetachedWindow {
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
}
