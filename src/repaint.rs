//! Process-wide repaint notifier.
//!
//! The reader threads (PTY reader, SSH task) live in UI-agnostic modules that
//! are compiled into BOTH the `portal` lib crate and the `portal` bin crate.
//! To let them wake egui without depending on egui, this module holds a global
//! `Fn() + Send + Sync` callback that the GUI layer installs once at startup.

use std::sync::{Arc, OnceLock};

/// Cross-thread "please repaint the UI now" callback.
pub type RepaintNotifier = Arc<dyn Fn() + Send + Sync>;

static GLOBAL_REPAINT: OnceLock<RepaintNotifier> = OnceLock::new();

/// Install the process-wide repaint notifier (called once by the GUI layer).
pub fn set_global_repaint_notifier(f: RepaintNotifier) {
    let _ = GLOBAL_REPAINT.set(f);
}

/// Wake the UI for a repaint, if a notifier is installed (no-op otherwise).
pub fn notify_repaint() {
    if let Some(f) = GLOBAL_REPAINT.get() {
        f();
    }
}
