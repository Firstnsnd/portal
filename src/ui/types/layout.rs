//! # Layout Types
//!
//! This module contains types related to window and pane layout management.
//!
//! Most layout types are defined in `crate::ui::pane` and re-exported here
//! for backwards compatibility and logical grouping.

#![allow(dead_code)]
#![allow(unused_imports)]

// Re-export core pane types used throughout the codebase
// These are re-exported for backwards compatibility
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
