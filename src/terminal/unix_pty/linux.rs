//! Child-process name fallback — Linux.
//!
//! `ps -o comm=` (shared code) already reads `/proc/<pid>/comm` on this
//! platform, so there is no additional fallback to offer: `ps` failing
//! means the pid is gone.

/// No-op fallback slot: keep `None` ([`super::ps_comm`] covers Linux).
pub fn pid_executable_name(_pid: i32) -> Option<String> {
    None
}
