//! # File Selection State
//!
//! Multi-selection state management for file browser panels.

use std::collections::BTreeSet;

/// Tracks multi-selection state for file browser panels.
#[derive(Clone, Debug, Default)]
pub struct FileSelection {
    pub selected: BTreeSet<usize>,
    pub anchor: Option<usize>,
    pub focus: Option<usize>,
}

impl FileSelection {
    pub fn clear(&mut self) {
        self.selected.clear();
        self.anchor = None;
        self.focus = None;
    }

    /// Select exactly one item (plain click).
    pub fn select_one(&mut self, i: usize) {
        self.selected.clear();
        self.selected.insert(i);
        self.anchor = Some(i);
        self.focus = Some(i);
    }

    /// Toggle a single item (Cmd/Ctrl+Click).
    pub fn toggle(&mut self, i: usize) {
        if self.selected.contains(&i) {
            self.selected.remove(&i);
        } else {
            self.selected.insert(i);
        }
        self.anchor = Some(i);
        self.focus = Some(i);
    }

    /// Select range from anchor to i (Shift+Click).
    pub fn select_range(&mut self, i: usize) {
        let anchor = self.anchor.unwrap_or(0);
        let (lo, hi) = if anchor <= i { (anchor, i) } else { (i, anchor) };
        self.selected.clear();
        for idx in lo..=hi {
            self.selected.insert(idx);
        }
        self.focus = Some(i);
    }

    /// Extend selection from anchor to i (Shift+Arrow key), keeping anchor.
    pub fn extend_to(&mut self, i: usize) {
        let anchor = self.anchor.unwrap_or(0);
        let (lo, hi) = if anchor <= i { (anchor, i) } else { (i, anchor) };
        self.selected.clear();
        for idx in lo..=hi {
            self.selected.insert(idx);
        }
        self.focus = Some(i);
    }

    /// Select all items in range 0..count.
    pub fn select_all(&mut self, count: usize) {
        self.selected.clear();
        for i in 0..count {
            self.selected.insert(i);
        }
        if count > 0 {
            if self.anchor.is_none() {
                self.anchor = Some(0);
            }
            if self.focus.is_none() {
                self.focus = Some(0);
            }
        }
    }

    pub fn is_selected(&self, i: usize) -> bool {
        self.selected.contains(&i)
    }

    pub fn count(&self) -> usize {
        self.selected.len()
    }

    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }
}
