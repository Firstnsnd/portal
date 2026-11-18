//! Startup hooks — unix: termination-signal cleanup for PTY children.

/// Set up a signal handler for clean PTY cleanup on exit. Prevents PTY
/// resource leaks when the app is terminated (SIGTERM/SIGINT/SIGHUP).
pub fn install_signal_cleanup() {
    use std::sync::atomic::{AtomicBool, Ordering};

    // Flag to track if we've already cleaned up
    static CLEANUP_DONE: AtomicBool = AtomicBool::new(false);

    // Spawn a dedicated cleanup thread that waits for signals
    std::thread::spawn(|| {
        use signal_hook::consts::signal::*;
        use signal_hook::iterator::Signals;

        let signals = Signals::new([SIGTERM, SIGINT, SIGHUP]).ok();

        if let Some(mut sig) = signals {
            if sig.forever().next().is_some() {
                if !CLEANUP_DONE.swap(true, Ordering::SeqCst) {
                    // SIGKILL exactly the PTY children we spawned. The
                    // previous handler replaced the process with
                    // `pkill -9 -f "/bin/zsh -l"`, which matches every
                    // login shell on the machine by command line and took
                    // the user's own Terminal / VS Code shells with it.
                    crate::terminal::kill_registered_children();
                }
                // process::exit skips Drop — unlink the IPC channel here
                // (a stale socket file is also tolerated: the next launch
                // removes it before rebinding).
                crate::ipc::cleanup();
                // Exit after cleanup
                std::process::exit(0);
            }
        }
    });
}
