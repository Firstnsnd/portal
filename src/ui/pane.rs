use eframe::egui;

use crate::ui::types::{TerminalSession, AppView, BroadcastState};

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
}

/// A workspace tab: one or more split panes sharing a single tab entry.
pub struct Tab {
    pub title: String,
    pub sessions: Vec<TerminalSession>,
    pub layout: PaneNode,
    pub focused_session: usize,
    pub broadcast_enabled: bool,
}

/// Drag state for tab ghost visualization
#[derive(Default, Clone)]
pub struct TabDragState {
    pub source_index: Option<usize>,
    pub target_index: Option<usize>,
    pub ghost_title: String,
    pub ghost_size: egui::Vec2,
}

/// A tab that has been detached into its own OS window (full UI)
pub struct DetachedWindow {
    pub viewport_id: egui::ViewportId,
    pub tabs: Vec<Tab>,
    pub active_tab: usize,
    pub current_view: AppView,
    pub title: String,
    pub close_requested: bool,
    pub ime_composing: bool,
    pub ime_preedit: String,
    pub next_id: usize,
    pub tab_drag: TabDragState,
    pub show_shell_picker: bool,
    pub show_encoding_picker: bool,
    pub broadcast_state: BroadcastState,
}
