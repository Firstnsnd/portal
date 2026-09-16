//! UI ↔ async-task mailbox for keyboard-interactive (2FA) auth prompts.
//!
//! The auth flow runs on a tokio task; when the server sends an
//! `InfoRequest` the task must pause, let the egui thread render the
//! prompts, and resume with the user's answers. [`AuthPromptBridge`] is
//! that mailbox: the task parks in [`AuthPromptBridge::ask`] on a oneshot
//! receiver; the UI reads [`AuthPromptBridge::pending`] and delivers via
//! [`AuthPromptBridge::respond`] / aborts via [`AuthPromptBridge::cancel`]
//! (dropping the sender wakes the task immediately).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::oneshot;

/// Hard ceiling on how long an auth task waits for the user to answer a
/// prompt. Generous (OTP tokens can arrive late); cancel/Drop still aborts
/// instantly.
const PROMPT_WAIT_TIMEOUT: Duration = Duration::from_secs(600);

/// One round of server prompts (mapped from russh's Debug-only `Prompt`).
#[derive(Debug, Clone)]
pub struct AuthPrompt {
    /// Monotonic per-bridge id — the UI resets its input buffers when it
    /// changes (a new round must never inherit stale answers).
    pub id: u64,
    pub name: String,
    pub instructions: String,
    pub prompts: Vec<AuthPromptField>,
}

#[derive(Debug, Clone)]
pub struct AuthPromptField {
    pub prompt: String,
    /// true = plain text field; false = password-masked field.
    pub echo: bool,
}

/// Whether this bridge may surface prompts to a user at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptMode {
    /// Ask through the UI mailbox.
    Interactive,
    /// Refuse prompts immediately (connection-test dialog has no UI for
    /// OTP entry — it reports an actionable error instead of hanging).
    Decline,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PromptError {
    /// The user cancelled, the pane was dropped, or the wait timed out.
    Cancelled,
    /// This bridge does not support interactive prompts.
    Declined,
}

struct PendingPrompt {
    prompt: AuthPrompt,
    responder: oneshot::Sender<Vec<String>>,
}

/// Shared mailbox between the async auth task and the egui UI thread.
/// Constructed on the GUI side, cloned (as `Arc`) into the spawned task —
/// the same ownership shape as every other `Arc<Mutex<…>>` on
/// [`super::session::SshSession`].
pub struct AuthPromptBridge {
    inner: Arc<Mutex<Option<PendingPrompt>>>,
    mode: PromptMode,
    next_id: Arc<AtomicU64>,
}

impl Default for AuthPromptBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl AuthPromptBridge {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
            mode: PromptMode::Interactive,
            next_id: Arc::new(AtomicU64::new(0)),
        }
    }

    /// A bridge that never surfaces prompts — `ask` fails immediately.
    pub fn decline() -> Self {
        Self {
            mode: PromptMode::Decline,
            ..Self::new()
        }
    }

    /// The bridge's mode. Exercised by the inline tests (T2.1/T2.5); no
    /// production code calls it yet.
    #[allow(dead_code)]
    pub fn mode(&self) -> PromptMode {
        self.mode
    }

    // ── UI side (sync, called from the egui thread) ─────────────────────

    /// The prompt currently awaiting an answer, if any.
    pub fn pending(&self) -> Option<AuthPrompt> {
        self.inner.lock().ok().and_then(|g| g.as_ref().map(|p| p.prompt.clone()))
    }

    /// Deliver the user's answers. Returns false (and keeps the prompt
    /// pending) when the answer count doesn't match the prompt count.
    pub fn respond(&self, answers: Vec<String>) -> bool {
        let mut guard = match self.inner.lock() {
            Ok(g) => g,
            Err(_) => return false,
        };
        let expected = match guard.as_ref() {
            Some(p) => p.prompt.prompts.len(),
            None => return false,
        };
        if answers.len() != expected {
            return false;
        }
        if let Some(pending) = guard.take() {
            // A send error means the auth task is already gone; the answers
            // are simply dropped with it.
            let _ = pending.responder.send(answers);
            true
        } else {
            false
        }
    }

    /// Abort the pending prompt (user hit cancel / pane closed). Wakes the
    /// task's `ask` with [`PromptError::Cancelled`].
    pub fn cancel(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            // Taking drops the oneshot sender — the awaiting task wakes.
            *guard = None;
        }
    }

    // ── Task side (async, called from the ssh/sftp task) ────────────────

    /// Publish a prompt round and wait for the UI's answers.
    pub(crate) async fn ask(
        &self,
        name: String,
        instructions: String,
        fields: Vec<AuthPromptField>,
    ) -> Result<Vec<String>, PromptError> {
        if self.mode == PromptMode::Decline {
            return Err(PromptError::Declined);
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, mut rx) = oneshot::channel();
        let pending = PendingPrompt {
            prompt: AuthPrompt {
                id,
                name,
                instructions,
                prompts: fields,
            },
            responder: tx,
        };
        {
            let mut guard = self.inner.lock().map_err(|_| PromptError::Cancelled)?;
            // Only one auth flow uses a bridge at a time; a leftover prompt
            // (previous cancelled round) is overwritten.
            *guard = Some(pending);
        }

        match tokio::time::timeout(PROMPT_WAIT_TIMEOUT, &mut rx).await {
            Ok(Ok(answers)) => Ok(answers),
            // Sender dropped: user cancel / pane drop.
            Ok(Err(_recv_closed)) => Err(PromptError::Cancelled),
            // 10-minute ceiling hit.
            Err(_elapsed) => {
                self.cancel();
                Err(PromptError::Cancelled)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(prompt: &str, echo: bool) -> AuthPromptField {
        AuthPromptField { prompt: prompt.to_string(), echo }
    }

    /// T2.1 — a fresh bridge has nothing pending.
    #[test]
    fn pending_is_none_initially() {
        let bridge = AuthPromptBridge::new();
        assert!(bridge.pending().is_none());
        assert_eq!(bridge.mode(), PromptMode::Interactive);
    }

    /// T2.2 — ask publishes the prompt (echo flags intact); respond
    /// delivers the answers to the awaiting task.
    #[tokio::test]
    async fn ask_publishes_prompt_and_respond_delivers_answers() {
        let bridge = Arc::new(AuthPromptBridge::new());
        let task_bridge = Arc::clone(&bridge);
        let task = tokio::spawn(async move {
            task_bridge
                .ask("2FA".into(), "Enter code".into(), vec![field("OTP:", false)])
                .await
        });

        // Wait for the prompt to surface.
        let prompt = loop {
            if let Some(p) = bridge.pending() {
                break p;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        };
        assert_eq!(prompt.name, "2FA");
        assert_eq!(prompt.instructions, "Enter code");
        assert_eq!(prompt.prompts.len(), 1);
        assert_eq!(prompt.prompts[0].prompt, "OTP:");
        assert!(!prompt.prompts[0].echo, "echo=false must survive the mailbox");

        assert!(bridge.respond(vec!["123456".to_string()]));
        let answers = task.await.unwrap().expect("ask should succeed");
        assert_eq!(answers, vec!["123456".to_string()]);
        // Mailbox drained after delivery.
        assert!(bridge.pending().is_none());
    }

    /// T2.3 — a wrong answer count is rejected and the prompt stays
    /// pending (the UI can retry).
    #[tokio::test]
    async fn respond_rejects_wrong_answer_count() {
        let bridge = Arc::new(AuthPromptBridge::new());
        let task_bridge = Arc::clone(&bridge);
        let task = tokio::spawn(async move {
            task_bridge
                .ask("x".into(), String::new(), vec![field("A:", true), field("B:", false)])
                .await
        });
        loop {
            if bridge.pending().is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        assert!(!bridge.respond(vec!["only-one".to_string()]),
            "one answer for two prompts must be rejected");
        assert!(bridge.pending().is_some(), "prompt must stay pending");

        assert!(bridge.respond(vec!["a".to_string(), "b".to_string()]));
        let answers = task.await.unwrap().expect("ask should succeed");
        assert_eq!(answers.len(), 2);
    }

    /// T2.4 — cancel wakes the awaiting ask with Cancelled and clears the
    /// mailbox.
    #[tokio::test]
    async fn cancel_wakes_ask_with_cancelled() {
        let bridge = Arc::new(AuthPromptBridge::new());
        let task_bridge = Arc::clone(&bridge);
        let task = tokio::spawn(async move {
            task_bridge.ask("x".into(), String::new(), vec![field("OTP:", false)]).await
        });
        loop {
            if bridge.pending().is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        bridge.cancel();
        assert!(bridge.pending().is_none());
        let err = task.await.unwrap().expect_err("cancel must fail ask");
        assert_eq!(err, PromptError::Cancelled);
    }

    /// T2.5 — a Decline bridge fails ask immediately without publishing.
    #[tokio::test]
    async fn decline_mode_ask_returns_declined_without_publishing() {
        let bridge = AuthPromptBridge::decline();
        assert_eq!(bridge.mode(), PromptMode::Decline);
        let res = bridge.ask("x".into(), String::new(), vec![field("OTP:", false)]).await;
        assert_eq!(res.expect_err("decline bridge must refuse"), PromptError::Declined);
        assert!(bridge.pending().is_none());
    }

    /// T2.6 — consecutive rounds get strictly increasing prompt ids (the
    /// UI's reset trigger).
    #[tokio::test]
    async fn prompt_ids_increase_per_round() {
        let bridge = Arc::new(AuthPromptBridge::new());

        let run_round = |bridge: &Arc<AuthPromptBridge>, name: &str| {
            let b = Arc::clone(bridge);
            let n = name.to_string();
            tokio::spawn(async move {
                b.ask(n, String::new(), vec![field("P:", false)]).await
            })
        };

        let round1 = run_round(&bridge, "r1");
        let id1 = loop {
            if let Some(p) = bridge.pending() {
                break p.id;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        };
        bridge.cancel();
        assert!(round1.await.unwrap().is_err());

        let round2 = run_round(&bridge, "r2");
        let id2 = loop {
            if let Some(p) = bridge.pending() {
                break p.id;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        };
        bridge.cancel();
        assert!(round2.await.unwrap().is_err());

        assert!(id2 > id1, "ids must strictly increase (got {id1} then {id2})");
    }
}
