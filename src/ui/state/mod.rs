//! # UI state modules
//!
//! Shared mutable state that outlives a single frame. Kept separate from
//! `ui::types` (plain data structures) — these types hold live interaction
//! state (e.g. an in-flight chat reply).

pub mod ai_chat;
