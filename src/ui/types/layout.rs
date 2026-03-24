//! # Layout Types
//!
//! This module contains types related to window and pane layout management.
//!
//! Most layout types are defined in `crate::ui::pane` and re-exported here
//! for backwards compatibility and logical grouping.

// Re-export core pane types used throughout the codebase
pub use crate::ui::pane::{
    SplitDirection,
    PaneNode,
    PaneAction,
    Tab,
    TabDragState,
    DetachedWindow,
};

// Note: TabAnimation is used internally by TabDragState but is not
// typically needed directly by consumers of this module.
