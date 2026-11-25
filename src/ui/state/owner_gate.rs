//! # Protected-host owner gate
//!
//! Interaction state for "verify the local computer owner before touching a
//! protected host" (sudo-style, 5-minute process-wide grace window in
//! [`OwnerVerification`]; the prompt/pending pair below is per window).
//! Follows the pending-slot idiom of the AI two-step run approval: a gated
//! action is stashed in [`OwnerGate::pending`], verification runs
//! asynchronously, and the app layer re-dispatches the action once it passes.
//!
//! This module is deliberately egui-free so the state machine stays
//! unit-testable; the dialog rendering lives in `app::owner_gate`.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::config::HostEntry;

/// How long one successful owner verification covers every protected-host
/// action (sudo-style grace window, process-wide).
pub const OWNER_VERIFY_WINDOW: Duration = Duration::from_secs(5 * 60);

/// Process-wide "owner verified at T" cache (lives on `PortalApp`, shared by
/// all windows — sudo semantics, not per-window).
#[derive(Default)]
pub struct OwnerVerification {
    verified_until: Option<Instant>,
}

impl OwnerVerification {
    pub fn is_verified(&self, now: Instant) -> bool {
        self.verified_until.is_some_and(|t| now < t)
    }

    pub fn mark_verified(&mut self, now: Instant) {
        self.verified_until = Some(now + OWNER_VERIFY_WINDOW);
    }
}

/// What the gate is currently showing.
#[derive(Default)]
pub enum GatePrompt {
    /// No prompt active.
    #[default]
    None,
    /// App-drawn password field (Linux/Windows); the user is typing.
    Password,
    /// Verification is running off-frame — the macOS system sheet, or the
    /// background password-check thread. Polled every frame.
    InFlight {
        /// True = waiting on the OS sheet (spinner + cancel), false = waiting
        /// on a password check thread.
        system_prompt: bool,
        result: mpsc::Receiver<Result<bool, String>>,
    },
}

/// A gated action parked until verification passes. The dispatcher re-checks
/// identity before running: `name` guards against the hosts vec shifting
/// during the async wait, and index-only variants are never trusted blindly.
pub enum PendingOwnerAction {
    /// `add_tab_ssh` (lands in window 0, existing behavior).
    ConnectSsh { host: HostEntry },
    /// `add_host_dialog.open_edit`.
    OpenHostEdit { index: usize, name: String },
    /// `add_host_dialog.open_duplicate` (duplicates stay protected).
    DuplicateHost { index: usize, name: String },
    /// The `handle_view_actions` deletion body (credentials + entry + save).
    DeleteHost { index: usize, name: String },
    /// The SFTP panel connect body, into the left (`true`) or right browser.
    ConnectSftp { left: bool, host: HostEntry },
    /// `reconnect_ssh` on a specific tab/session. `remove_known_key` re-runs
    /// `remove_known_hosts_key` first — the RemoveHostKey site must NOT drop
    /// the key until the gate has passed.
    Reconnect {
        tab: usize,
        session: usize,
        remove_known_key: bool,
    },
    /// Open the new-credential drawer (Keychain page "+ New credential").
    /// Every keychain operation clears the owner gate — one credential can
    /// back several hosts, so an unverified edit can silently change other
    /// hosts' logins.
    KeychainNew,
    /// Open the edit drawer for the credential with this id.
    KeychainEdit { id: String },
    /// The delete-credential body: keychain secrets, the stored entry, and
    /// every host binding (dangling credential_ids would break logins).
    DeleteCredential { id: String },
    /// The credential drawer's save body. The drawer state lives on the
    /// window, so dispatch just re-arms `credential_dialog.gate_granted`
    /// and the drawer re-runs its own save next frame.
    SaveCredential,
    /// The host drawer's save body when it writes keychain secrets (inline
    /// credential / typed proxy password). Same handshake, on
    /// `add_host_dialog.save_gate_granted`.
    SaveHostKeychain,
    /// Store the AI API key from the settings draft (OS keychain write).
    SaveLlmKey,
    /// Remove the stored AI API key (OS keychain delete).
    ClearLlmKey,
    /// Switch the sidebar view to the Keychain page — the door to the
    /// credential store clears the owner gate at entry.
    OpenKeychainView,
}

/// Per-window gate state (lives on `AppWindow`): one parked action, the
/// prompt being shown for it, and the password-field scratch space.
#[derive(Default)]
pub struct OwnerGate {
    pub pending: Option<PendingOwnerAction>,
    pub prompt: GatePrompt,
    /// Localized request time — shown in the dialog/sheet.
    pub reason: String,
    pub password: String,
    pub error: String,
}

impl OwnerGate {
    /// Drop the in-flight verification and its parked action (dialog cancel).
    /// The global verification cache is untouched — a completed verification
    /// keeps its grace window even after a cancel.
    pub fn cancel(&mut self) {
        self.pending = None;
        self.prompt = GatePrompt::None;
        self.password.clear();
        self.error.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verification_window_expires() {
        let mut v = OwnerVerification::default();
        let t0 = Instant::now();
        assert!(!v.is_verified(t0));
        v.mark_verified(t0);
        assert!(v.is_verified(t0));
        assert!(v.is_verified(t0 + Duration::from_secs(299)));
        assert!(!v.is_verified(t0 + Duration::from_secs(301)));
    }

    #[test]
    fn cancel_clears_everything_but_is_idempotent() {
        let mut gate = OwnerGate {
            pending: Some(PendingOwnerAction::DeleteHost {
                index: 3,
                name: "prod".into(),
            }),
            prompt: GatePrompt::Password,
            reason: "test".into(),
            password: "hunter2".into(),
            error: "wrong".into(),
        };
        gate.cancel();
        assert!(gate.pending.is_none());
        assert!(gate.password.is_empty());
        assert!(gate.error.is_empty());
        assert!(matches!(gate.prompt, GatePrompt::None));
        gate.cancel(); // cancelling with nothing parked is a no-op
        assert!(matches!(gate.prompt, GatePrompt::None));
    }
}
