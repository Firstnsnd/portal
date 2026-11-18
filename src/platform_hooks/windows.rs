//! Startup hooks — Windows.

/// Windows has no POSIX signal set: there is no SIGTERM to intercept, and
/// the ConPTY child handles are closed by the OS when the process dies, so
/// the unix cleanup hook has nothing to install here.
pub fn install_signal_cleanup() {}
