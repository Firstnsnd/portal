//! OS-level drag-out support (app → Finder).
//!
//! The registry tracks the file promises offered during a native drag
//! session. macOS-specific glue lives in `drag_out/macos.rs`; on other
//! platforms `begin_file_promise_drag` is a no-op stub.

use tokio::sync::mpsc::UnboundedSender;

use super::types::SftpCommand;

/// One file offered to the OS during a drag-out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PromiseSpec {
    pub(crate) file_name: String,
    pub(crate) remote_path: String,
    pub(crate) is_dir: bool,
}

/// A promise registered for delivery, carrying everything needed to fulfill
/// it: what to download and which connection's command channel to use.
pub(crate) struct PendingPromise {
    pub(crate) spec: PromiseSpec,
    pub(crate) cmd_tx: UnboundedSender<SftpCommand>,
}

/// Registry of the promises in the current native drag session. Keys are the
/// native provider object pointers (usize); tests use synthetic keys.
#[derive(Default)]
pub(crate) struct PromiseRegistry {
    promises: std::collections::HashMap<usize, PendingPromise>,
}

impl PromiseRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Number of promises awaiting delivery (session may still be running).
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.promises.len()
    }

    /// Register a promise under a provider key.
    pub(crate) fn insert(&mut self, key: usize, spec: PromiseSpec, cmd_tx: UnboundedSender<SftpCommand>) {
        self.promises.insert(key, PendingPromise { spec, cmd_tx });
    }

    /// File name for a provider (`promiseProvider:fileNameForType:`).
    pub(crate) fn file_name(&self, key: usize) -> Option<&str> {
        self.promises.get(&key).map(|p| p.spec.file_name.as_str())
    }

    /// Take the promise for delivery (`provideFileForPromiseProvider:`).
    /// Removes it so a repeated callback is a no-op.
    pub(crate) fn take_for_delivery(&mut self, key: usize) -> Option<PendingPromise> {
        self.promises.remove(&key)
    }

    /// Drag session ended: drop every promise that was never delivered.
    pub(crate) fn end_session(&mut self) {
        self.promises.clear();
    }
}

/// Begin a native file-promise drag session. Returns true if the OS accepted
/// it. Stubbed out on non-macOS platforms.
#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub(crate) fn begin_file_promise_drag(
    specs: &[PromiseSpec],
    cmd_tx: UnboundedSender<SftpCommand>,
) -> bool {
    macos::begin_file_promise_drag(specs, cmd_tx)
}

#[cfg(not(target_os = "macos"))]
#[allow(unused_variables)]
pub(crate) fn begin_file_promise_drag(
    specs: &[PromiseSpec],
    cmd_tx: UnboundedSender<SftpCommand>,
) -> bool {
    false
}

/// True when the pointer is outside every visible app window. Always false on
/// non-macOS (drag-out escalation never triggers there).
#[cfg(target_os = "macos")]
pub(crate) fn pointer_outside_app_windows() -> bool {
    macos::pointer_outside_app_windows()
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn pointer_outside_app_windows() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tx() -> UnboundedSender<SftpCommand> {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        tx
    }

    fn spec(name: &str, remote: &str, is_dir: bool) -> PromiseSpec {
        PromiseSpec {
            file_name: name.to_string(),
            remote_path: remote.to_string(),
            is_dir,
        }
    }

    #[test]
    fn file_name_lookup_finds_inserted_promises() {
        let mut reg = PromiseRegistry::new();
        reg.insert(1, spec("a.txt", "/srv/a.txt", false), tx());
        reg.insert(2, spec("logs", "/srv/logs", true), tx());
        assert_eq!(reg.file_name(1), Some("a.txt"));
        assert_eq!(reg.file_name(2), Some("logs"));
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn take_for_delivery_returns_and_removes_promise() {
        let mut reg = PromiseRegistry::new();
        reg.insert(7, spec("a.txt", "/srv/a.txt", false), tx());
        let pending = reg.take_for_delivery(7).expect("promise should be there");
        assert_eq!(pending.spec.file_name, "a.txt");
        assert_eq!(reg.len(), 0);
        // A repeated delivery callback must be a no-op.
        assert!(reg.take_for_delivery(7).is_none());
    }

    #[test]
    fn end_session_clears_undelivered_promises() {
        let mut reg = PromiseRegistry::new();
        reg.insert(1, spec("a.txt", "/srv/a.txt", false), tx());
        reg.insert(2, spec("b.txt", "/srv/b.txt", false), tx());
        let _ = reg.take_for_delivery(1); // a.txt delivered mid-session
        reg.end_session(); // user cancelled / released outside Finder
        assert_eq!(reg.len(), 0);
        assert!(reg.file_name(2).is_none());
        assert!(reg.take_for_delivery(2).is_none());
    }

    #[test]
    fn delivered_promise_lookup_is_noop() {
        let mut reg = PromiseRegistry::new();
        reg.insert(3, spec("a.txt", "/srv/a.txt", false), tx());
        let _ = reg.take_for_delivery(3);
        assert_eq!(reg.file_name(3), None);
    }

    #[test]
    fn unknown_key_lookups_return_none() {
        let mut reg = PromiseRegistry::new();
        assert!(reg.file_name(999).is_none());
        assert!(reg.take_for_delivery(999).is_none());
        assert_eq!(reg.len(), 0);
    }
}
