//! PTY Session Management
//!
//! This module contains the PTY session implementation that manages
//! the communication between the application and the PTY, including
//! reading output in a background thread and parsing VTE sequences.

use std::io;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Instant;

use sysinfo::System;
use crate::terminal::metrics::{MetricsSnapshot, SystemMetrics};

pub use super::{Pty, PtySize, Result};

#[cfg(unix)]
pub use super::UnixPty;

#[cfg(windows)]
pub use super::WindowsPty;

// PtyWriter is already pub and will be re-exported by parent module

use super::grid::TerminalGrid;
use super::types::CellAttrs;
use super::vte::VteHandler;
use vte::Parser;

/// Safe wrapper for writing to a raw fd
/// The fd is duplicated and must be closed when dropped
#[cfg(unix)]
pub struct PtyWriter {
    pub fd: std::os::unix::io::RawFd,
}

#[cfg(unix)]
impl PtyWriter {
    pub fn write(&self, data: &[u8]) -> io::Result<()> {
        // The master fd is non-blocking (shared O_NONBLOCK across all dups of
        // the open file description), so a naive write_all drops keystrokes
        // with WouldBlock whenever the PTY input queue fills. Route through
        // the poll-retry helper instead.
        super::unix_pty::write_all_pty(self.fd, data)
    }
}

#[cfg(unix)]
impl Drop for PtyWriter {
    fn drop(&mut self) {
        // Close the duplicated file descriptor
        unsafe {
            libc::close(self.fd);
        }
    }
}

/// Real PTY session with background reader thread
pub struct RealPtySession {
    #[cfg(unix)]
    pty: Option<Arc<Mutex<UnixPty>>>,
    #[cfg(unix)]
    writer: Option<PtyWriter>,
    #[cfg(windows)]
    pty: Option<Arc<Mutex<WindowsPty>>>,
    pub grid: Arc<Mutex<TerminalGrid>>,
    alive: Arc<AtomicBool>,
    _reader_thread: Option<thread::JoinHandle<()>>,
    cached_shell_name: Arc<Mutex<Option<String>>>,
    last_shell_check: Arc<Mutex<Instant>>,
    /// Live system metrics collected from the local machine
    pub metrics: Arc<Mutex<MetricsSnapshot>>,
    /// Asciicast recorder slot: `Some` while the session is being recorded.
    /// Cloned into the reader thread at spawn, so the tap sees it.
    pub recorder: Arc<Mutex<Option<crate::terminal::cast::CastRecorder>>>,
}

impl RealPtySession {
    #[cfg(unix)]
    #[allow(dead_code)]
    pub fn new(id: usize, cols: u16, rows: u16, shell: &str) -> Result<Self> {
        Self::with_scrollback_limit(id, cols, rows, shell, TerminalGrid::DEFAULT_MAX_SCROLLBACK_BYTES, None, false)
    }

    #[cfg(unix)]
    pub fn with_scrollback_limit(id: usize, cols: u16, rows: u16, shell: &str, scrollback_limit_bytes: usize, cwd: Option<&str>, record: bool) -> Result<Self> {
        use std::os::unix::io::AsRawFd;
        let _ = id;

        let grid: Arc<Mutex<TerminalGrid>> = Arc::new(Mutex::new(TerminalGrid::with_scrollback_limit(cols as usize, rows as usize, scrollback_limit_bytes)));
        let pty = UnixPty::spawn_in(shell, &["-l"], PtySize::new(rows, cols), cwd)?;
        let pty = Arc::new(Mutex::new(pty));

        let fd = {
            let pty_ref = pty.lock().unwrap();
            pty_ref.master.as_raw_fd()
        };
        let fd_dup = unsafe { libc::dup(fd) };
        if fd_dup < 0 {
            return Err(super::Error::SpawnFailed("Failed to dup fd".to_string()));
        }

        let writer = PtyWriter { fd: fd_dup };
        let alive = Arc::new(AtomicBool::new(true));
        let grid_clone: Arc<Mutex<TerminalGrid>> = Arc::clone(&grid);
        let pty_clone = Arc::clone(&pty);
        let alive_clone = Arc::clone(&alive);
        let recorder = Arc::new(Mutex::new(
            if record {
                // Zero-loss auto-record: create the recorder BEFORE the reader
                // thread spawns so capture starts at the very first byte.
                let title = std::path::Path::new(shell)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| shell.to_string());
                match crate::terminal::cast::new_recording_path(&title)
                    .and_then(|path| {
                        crate::terminal::cast::CastRecorder::create(
                            &path, cols, rows, Some(title.clone()), Some("localhost".into()),
                        )
                    }) {
                    Ok(rec) => Some(rec),
                    Err(e) => {
                        log::warn!("auto-record: failed to open recording file: {e}");
                        None
                    }
                }
            } else {
                None
            },
        ));
        let recorder_clone = Arc::clone(&recorder);

        let reader_thread = thread::Builder::new()
            .name(format!("pty-reader-{}", id))
            .spawn(move || {
                let mut parser = Parser::new();
                let mut attrs = CellAttrs::default();

                // ── INVARIANT ────────────────────────────────────────────
                // `alive` is set false ONLY via `liveness_check`, and ONLY
                // after `is_alive()` (waitpid WNOHANG) confirmed the child is
                // gone. `is_connected()` reads `alive` directly, so a false
                // "disconnected" report is impossible while the child runs. Do
                // not add any other path that sets `alive=false`.
                let liveness_check = || -> bool {
                    let is_alive = {
                        let pty_ref = pty_clone.lock().unwrap();
                        pty_ref.is_alive()
                    };
                    if !is_alive {
                        alive_clone.store(false, Ordering::Relaxed);
                        false
                    } else {
                        true
                    }
                };

                // Block on the master fd with poll() instead of a 10ms
                // sleep-loop: idle terminals now consume ~0 CPU. The fd is
                // non-blocking, so once poll reports readability we drain with
                // try_read() and re-poll when it would-block.
                while alive_clone.load(Ordering::Relaxed) {
                    // Snapshot the fd out of the lock so poll() never holds the
                    // pty mutex — resize()/write() from the UI thread must stay
                    // unblocked while we wait for output.
                    let master_fd = {
                        let pty_ref = pty_clone.lock().unwrap();
                        pty_ref.master.as_raw_fd()
                    };

                    let mut pfd = libc::pollfd {
                        fd: master_fd,
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    // 250ms timeout: a normal exit makes the master readable
                    // (EOF) so poll returns immediately; the timeout is only a
                    // fallback to run the periodic waitpid liveness check when
                    // a grandchild keeps the slave open after the shell exits.
                    let ready = unsafe { libc::poll(&mut pfd, 1, 250) };

                    if ready < 0 {
                        if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                            // Interrupted by a signal (e.g. SIGCHLD when the
                            // shell forks). Retry immediately.
                            continue;
                        }
                        // Unexpected poll error (EBADF/ENOMEM/…). Back off
                        // rather than busy-spin, after a liveness check.
                        if !liveness_check() {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        continue;
                    }

                    if ready == 0 {
                        // Timeout with no data: run the periodic liveness check
                        // so a shell that exits without closing the master is
                        // still detected promptly.
                        if !liveness_check() {
                            break;
                        }
                        continue;
                    }

                    // ready > 0: drain all currently-buffered output. Each chunk
                    // is applied under ONE grid lock so the renderer can't
                    // interleave mid-batch and paint a half-drawn frame
                    // (full-screen apps wrap renders in \e[?2026h…?2026l and
                    // rely on exactly this boundary coherence).
                    let dead = loop {
                        let data = {
                            let mut pty_ref = pty_clone.lock().unwrap();
                            pty_ref.try_read()
                        };
                        match data {
                            Ok(data) if !data.is_empty() => {
                                {
                                    let mut grid = grid_clone.lock().unwrap();
                                    let mut handler = VteHandler {
                                        grid: &mut grid,
                                        attrs: &mut attrs,
                                    };
                                    for byte in &data {
                                        parser.advance(&mut handler, *byte);
                                    }
                                    // Every write path and resize()/reflow keep
                                    // rows exactly `cols`-wide, so no per-chunk
                                    // normalization scan is needed here.
                                }
                                // Asciicast tap: capture the raw chunk with a
                                // timestamp (outside the grid lock, in the
                                // reader thread).
                                if let Ok(mut slot) = recorder_clone.lock() {
                                    if let Some(rec) = slot.as_mut() {
                                        rec.output(&data);
                                    }
                                }
                                // Wake the UI so the new output is painted
                                // promptly (the app no longer repaints
                                // unconditionally).
                                crate::repaint::notify_repaint();
                            }
                            Ok(_) => {
                                // Empty read: either drained everything
                                // (would-block) or EOF (child closed the slave).
                                // Distinguish via waitpid.
                                break !liveness_check();
                            }
                            Err(_) => {
                                // Non-transient read error (EIO on macOS when
                                // the slave closes, broken master, etc.). Do NOT
                                // blindly declare the session dead — confirm via
                                // waitpid that the child actually terminated.
                                break !liveness_check();
                            }
                        }
                    };
                    if dead {
                        break;
                    }

                    // A hangup/error that poll reported but that didn't surface
                    // as an EOF read (e.g. POLLHUP with no pending data).
                    if pfd.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0
                        && !liveness_check()
                    {
                        break;
                    }
                }
            })
            .map_err(|_| super::Error::SpawnFailed("Failed to spawn reader thread".to_string()))?;

        let cached_shell_name = Arc::new(Mutex::new(None));
        let last_shell_check = Arc::new(Mutex::new(Instant::now()));
        let metrics = Arc::new(Mutex::new(MetricsSnapshot::new()));

        // Spawn a background thread for local metrics collection
        let metrics_clone: Arc<Mutex<MetricsSnapshot>> = Arc::clone(&metrics);
        thread::Builder::new()
            .name("metrics-collector".to_string())
            .spawn(move || {
                let mut sys = System::new();
                let mut nets = sysinfo::Networks::new_with_refreshed_list();
                let mut prev_rx: Option<u64> = None;
                let mut prev_tx: Option<u64> = None;
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    sys.refresh_memory();
                    sys.refresh_cpu_usage();
                    nets.refresh();
                    let rx: u64 = nets.values().map(|n| n.total_received()).sum();
                    let tx: u64 = nets.values().map(|n| n.total_transmitted()).sum();
                    let net_rx_rate = prev_rx
                        .map(|p| ((rx.saturating_sub(p)) as f64 / 5.0) as u64)
                        .unwrap_or(0);
                    let net_tx_rate = prev_tx
                        .map(|p| ((tx.saturating_sub(p)) as f64 / 5.0) as u64)
                        .unwrap_or(0);
                    prev_rx = Some(rx);
                    prev_tx = Some(tx);
                    let m = SystemMetrics {
                        cpu_percent: sys.global_cpu_usage(),
                        mem_used_bytes: sys.used_memory(),
                        mem_total_bytes: sys.total_memory(),
                        load_1: System::load_average().one as f32,
                        net_rx_bytes_per_sec: net_rx_rate,
                        net_tx_bytes_per_sec: net_tx_rate,
                    };
                    metrics_clone.lock().unwrap().push(m);
                    // Metrics snapshots change independently of terminal output;
                    // wake the UI so the drawer's sparklines keep animating.
                    crate::repaint::notify_repaint();
                }
            })
            .ok(); // fire-and-forget; thread outlives the loop, cleaned on drop

        Ok(Self {
            pty: Some(pty),
            writer: Some(writer),
            grid,
            alive,
            _reader_thread: Some(reader_thread),
            cached_shell_name,
            last_shell_check,
            metrics,
            recorder,
        })
    }

    #[cfg(windows)]
    pub fn new(id: usize, cols: u16, rows: u16, shell: &str) -> Result<Self> {
        let _ = (id, cols, rows, shell);
        Err(super::Error::SpawnFailed("Windows PTY not implemented".to_string()))
    }

    #[cfg(windows)]
    pub fn with_scrollback_limit(id: usize, cols: u16, rows: u16, shell: &str, _scrollback_limit_bytes: usize) -> Result<Self> {
        let _ = (id, cols, rows, shell);
        Err(super::Error::SpawnFailed("Windows PTY not implemented".to_string()))
    }

    /// Write data to the PTY
    pub fn write(&self, data: &[u8]) -> io::Result<()> {
        #[cfg(unix)]
        if let Some(ref writer) = self.writer {
            return writer.write(data);
        }
        #[cfg(windows)]
        if let Some(ref pty) = self.pty {
            let pty_ref = pty.lock().unwrap();
            return pty_ref.write(data).map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()));
        }
        Err(io::Error::new(io::ErrorKind::NotConnected, "PTY not available"))
    }

    /// Resize the PTY
    pub fn resize(&mut self, cols: u16, rows: u16) -> io::Result<()> {
        #[cfg(unix)]
        if let Some(ref pty) = self.pty {
            let mut pty_ref = pty.lock().unwrap();
            return pty_ref.resize(PtySize::new(rows, cols)).map_err(|e| io::Error::other(e.to_string()));
        }
        #[cfg(windows)]
        if let Some(ref pty) = self.pty {
            let mut pty_ref = pty.lock().unwrap();
            return pty_ref.resize(PtySize::new(rows, cols)).map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()));
        }
        Err(io::Error::new(io::ErrorKind::NotConnected, "PTY not available"))
    }

    /// Check if the PTY session is still alive
    #[allow(dead_code)]
    pub fn is_alive(&self) -> bool {
        #[cfg(unix)]
        if let Some(ref pty) = self.pty {
            let pty_ref = pty.lock().unwrap();
            return pty_ref.is_alive();
        }
        #[cfg(windows)]
        if let Some(ref pty) = self.pty {
            let pty_ref = pty.lock().unwrap();
            return pty_ref.is_alive();
        }
        false
    }

    /// Whether the session is still usable.
    #[allow(dead_code)] // used in tests; production reads has_exited()
    ///
    /// For a local PTY there is no "connection" that can flap — the shell
    /// process is either running or it has exited. `alive` is a one-shot latch
    /// flipped to `false` ONLY by the reader thread after it confirms a real
    /// child exit (`is_alive()` returned false). Reading the flag here, without
    /// re-poking `waitpid`, is intentional: the previous form
    /// (`alive && is_alive()`) called `waitpid` on every render frame, and any
    // transient signal/window during that poll could flip the state mid-frame
    // and stick the terminal in a false "disconnected" overlay that never
    // recovered. Now `is_connected()` is monotonic: true → false exactly once,
    // at the moment the reader thread observes a genuine exit.
    pub fn is_connected(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    /// True once the child process has been confirmed exited by the reader
    /// thread. This is the Local-session equivalent of SSH's "disconnected",
    /// but framed as a one-time process-exit fact rather than a connection
    /// state — there is no flapping, no recovery, no "reconnect". `false` for
    /// the entire lifetime of a running shell; flips to `true` exactly once.
    pub fn has_exited(&self) -> bool {
        !self.alive.load(Ordering::Relaxed)
    }

    /// Get the shell name (cached for 1 second)
    pub fn get_shell_name(&self) -> Option<String> {
        // Check cache first (5 second TTL)
        {
            let mut last_check = self.last_shell_check.lock().unwrap();
            if let Some(ref name) = *self.cached_shell_name.lock().unwrap() {
                if last_check.elapsed() < std::time::Duration::from_secs(1) {
                    return Some(name.clone());
                }
            }
            *last_check = Instant::now();
        }

        // Cache miss or expired, fetch fresh
        #[cfg(unix)]
        if let Some(ref pty) = self.pty {
            let pty_ref = pty.lock().unwrap();
            let name = pty_ref.get_shell_name();
            if let Some(ref name) = name {
                *self.cached_shell_name.lock().unwrap() = Some(name.clone());
                return Some(name.clone());
            }
        }
        #[cfg(windows)]
        if let Some(ref pty) = self.pty {
            let pty_ref = pty.lock().unwrap();
            let name = pty_ref.get_shell_name();
            if let Some(ref name) = name {
                *self.cached_shell_name.lock().unwrap() = Some(name.clone());
                return Some(name.clone());
            }
        }

        None
    }

    /// Get the terminal grid
    pub fn get_grid(&self) -> Arc<Mutex<TerminalGrid>> {
        Arc::clone(&self.grid)
    }

    /// Record a resize event into the active asciicast, if any.
    pub fn record_resize(&self, cols: u16, rows: u16) {
        if let Ok(mut slot) = self.recorder.lock() {
            if let Some(rec) = slot.as_mut() {
                rec.resize(cols, rows);
            }
        }
    }
}

impl Drop for RealPtySession {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
        #[cfg(unix)]
        if let Some(pty) = self.pty.take() {
            let mut pty_ref = pty.lock().unwrap();
            let _ = pty_ref.kill();
        }
        #[cfg(windows)]
        if let Some(pty) = self.pty.take() {
            let mut pty_ref = pty.lock().unwrap();
            let _ = pty_ref.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    /// "Open terminal here": a PTY spawned with an explicit cwd must actually
    /// start there. Spawns /bin/sh in a tempdir, asks `pwd`, and checks the
    /// rendered grid for the path.
    #[cfg(unix)]
    #[test]
    fn pty_spawn_with_cwd_starts_in_cwd() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().to_string_lossy().to_string();
        let session = RealPtySession::with_scrollback_limit(
            1,
            80,
            24,
            "/bin/sh",
            1024 * 1024,
            Some(&cwd),
            false,
        )
        .expect("spawn sh");

        session.write(b"pwd\n").expect("write pwd");

        let grid = session.get_grid();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let found = {
                let g = grid.lock().unwrap();
                g.cells.iter().any(|row| {
                    let line: String = row.iter().map(|c| c.c).collect();
                    line.contains(&cwd)
                })
            };
            if found {
                return; // shell reports the requested directory
            }
            if std::time::Instant::now() > deadline {
                panic!("`pwd` output did not contain {cwd:?} within 5s");
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }

    /// E2E: the PTY output tap lands in a `.cast` file — start recording,
    /// run `echo hi`, stop, and the parsed recording contains an Output event
    /// with the echoed text.
    #[cfg(unix)]
    #[test]
    fn recording_captures_pty_output_as_cast() {
        use crate::terminal::cast::{parse_cast, CastRecorder, EventKind};

        let session = RealPtySession::with_scrollback_limit(
            1,
            80,
            24,
            "/bin/sh",
            1024 * 1024,
            None,
            false,
        )
        .expect("spawn sh");

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("e2e.cast");
        let recorder = CastRecorder::create(&path, 80, 24, Some("sh".into()), None).expect("create recorder");
        // Populate the EXISTING slot Arc — the reader thread holds a clone of
        // it, so replacing the field would strand the tap on the old slot.
        *session.recorder.lock().unwrap() = Some(recorder);

        session.write(b"echo hi\n").expect("write echo");

        // Wait until the grid shows the echo. The tap fires in the same
        // reader-loop iteration as the grid update (just after the grid lock),
        // so give it a beat to land in the recorder's BufWriter before we take
        // the slot away.
        let grid = session.get_grid();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let found = {
                let g = grid.lock().unwrap();
                g.cells.iter().any(|row| {
                    let line: String = row.iter().map(|c| c.c).collect();
                    line.contains("hi")
                })
            };
            if found {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "`echo hi` output not seen within 5s"
            );
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        std::thread::sleep(std::time::Duration::from_millis(200));

        // Stop: take the recorder out of the slot and flush.
        let recorder = session
            .recorder
            .lock()
            .unwrap()
            .take()
            .expect("recorder still in slot");
        recorder.finish().expect("flush cast");

        let cast = parse_cast(&path).expect("parse cast");
        assert_eq!(cast.header.version, 2);
        assert_eq!(cast.header.width, 80);
        assert!(!cast.events.is_empty(), "no events recorded");
        assert!(
            cast.events
                .iter()
                .any(|e| e.kind == EventKind::Output && e.data.contains("hi")),
            "no Output event contains \"hi\""
        );
        // Times must be monotonic non-decreasing.
        for pair in cast.events.windows(2) {
            assert!(pair[0].time <= pair[1].time, "non-monotonic event times");
        }
    }

    /// Auto-record: constructing with `record: true` has the recorder in the
    /// slot the moment the constructor returns (before the reader thread can
    /// lose bytes), and the file header carries the session title.
    #[cfg(unix)]
    #[test]
    fn auto_record_starts_at_spawn_with_title() {
        use crate::terminal::cast::parse_cast;

        let session = RealPtySession::with_scrollback_limit(
            1,
            80,
            24,
            "/bin/sh",
            1024 * 1024,
            None,
            true,
        )
        .expect("spawn sh");

        // The recorder must be in the slot the instant the constructor
        // returns — that's what makes capture lossless from byte zero.
        assert!(
            session.recorder.lock().unwrap().is_some(),
            "recorder auto-started with record=true"
        );

        session.write(b"echo hi\n").expect("write echo");
        let grid = session.get_grid();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let found = {
                let g = grid.lock().unwrap();
                g.cells.iter().any(|row| {
                    let line: String = row.iter().map(|c| c.c).collect();
                    line.contains("hi")
                })
            };
            if found {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "`echo hi` output not seen within 5s"
            );
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        std::thread::sleep(std::time::Duration::from_millis(200));

        let recorder = session
            .recorder
            .lock()
            .unwrap()
            .take()
            .expect("recorder still in slot");
        let path = recorder.finish().expect("flush cast");
        let cast = parse_cast(&path).expect("parse cast");
        assert_eq!(cast.header.title.as_deref(), Some("sh"));
        assert!(!cast.events.is_empty(), "shell output captured from the start");
    }

    /// Verify that is_connected() returns false when the alive flag is false.
    /// This simulates what happens when the reader thread encounters a read error
    /// and sets alive to false before exiting.
    #[test]
    fn test_is_connected_false_when_reader_thread_dies() {
        // Simulate: alive = false (reader thread exited on error), no PTY
        let grid = Arc::new(Mutex::new(
            TerminalGrid::with_scrollback_limit(80, 24, 1024 * 1024)
        ));
        let session = RealPtySession {
            #[cfg(unix)]
            pty: None,
            #[cfg(windows)]
            pty: None,
            #[cfg(unix)]
            writer: None,
            grid,
            alive: Arc::new(AtomicBool::new(false)), // Reader thread died
            _reader_thread: None,
            cached_shell_name: Arc::new(Mutex::new(None)),
            last_shell_check: Arc::new(Mutex::new(Instant::now())),
            metrics: Arc::new(Mutex::new(MetricsSnapshot::new())),
            recorder: Arc::new(Mutex::new(None)),
        };

        assert!(!session.is_connected(), "is_connected() must return false when alive flag is false");
    }

    /// Verify that is_connected() is a pure mirror of the `alive` flag.
    /// A session without a PTY is constructed with `alive=false`, so
    /// `is_connected()` must report false. (Under the new monotonic model,
    /// `alive=true && pty=None` is unreachable — `alive` is owned solely by
    /// the reader thread of a real PTY.)
    #[test]
    fn test_is_connected_false_when_no_pty() {
        let grid = Arc::new(Mutex::new(
            TerminalGrid::with_scrollback_limit(80, 24, 1024 * 1024)
        ));
        let session = RealPtySession {
            #[cfg(unix)]
            pty: None,
            #[cfg(windows)]
            pty: None,
            #[cfg(unix)]
            writer: None,
            grid,
            alive: Arc::new(AtomicBool::new(false)),
            _reader_thread: None,
            cached_shell_name: Arc::new(Mutex::new(None)),
            last_shell_check: Arc::new(Mutex::new(Instant::now())),
            metrics: Arc::new(Mutex::new(MetricsSnapshot::new())),
            recorder: Arc::new(Mutex::new(None)),
        };

        assert!(!session.is_connected(), "is_connected() must return false when alive flag is false");
    }

    /// Verify that is_connected() returns true while `alive` is true.
    /// This pins the monotonic invariant: as long as the reader thread has not
    /// observed a genuine child exit, the session reports connected — no matter
    /// what transient read errors occurred.
    #[test]
    fn test_is_connected_true_while_alive_true() {
        let grid = Arc::new(Mutex::new(
            TerminalGrid::with_scrollback_limit(80, 24, 1024 * 1024)
        ));
        let session = RealPtySession {
            #[cfg(unix)]
            pty: None,
            #[cfg(windows)]
            pty: None,
            #[cfg(unix)]
            writer: None,
            grid,
            alive: Arc::new(AtomicBool::new(true)),
            _reader_thread: None,
            cached_shell_name: Arc::new(Mutex::new(None)),
            last_shell_check: Arc::new(Mutex::new(Instant::now())),
            metrics: Arc::new(Mutex::new(MetricsSnapshot::new())),
            recorder: Arc::new(Mutex::new(None)),
        };

        assert!(session.is_connected(),
            "is_connected() must return true while alive=true, regardless of PTY state");
    }

    /// Verify that Drop sets alive to false, ensuring is_connected() transitions correctly.
    #[test]
    fn test_drop_sets_alive_false() {
        let alive = Arc::new(AtomicBool::new(true));
        let grid = Arc::new(Mutex::new(
            TerminalGrid::with_scrollback_limit(80, 24, 1024 * 1024)
        ));

        {
            let _session = RealPtySession {
                #[cfg(unix)]
                pty: None,
                #[cfg(windows)]
                pty: None,
                #[cfg(unix)]
                writer: None,
                grid: Arc::clone(&grid),
                alive: Arc::clone(&alive),
                _reader_thread: None,
                cached_shell_name: Arc::new(Mutex::new(None)),
                last_shell_check: Arc::new(Mutex::new(Instant::now())),
                metrics: Arc::new(Mutex::new(MetricsSnapshot::new())),
                recorder: Arc::new(Mutex::new(None)),
            };
            // session is alive while in scope
            assert!(alive.load(Ordering::Relaxed));
        }
        // After drop, alive must be false
        assert!(!alive.load(Ordering::Relaxed), "Drop must set alive to false");
    }
}
