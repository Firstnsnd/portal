//! PTY Session Management
//!
//! The PTY session implementation that manages communication between the
//! application and the PTY, including reading output in a background thread
//! and parsing VTE sequences.
//!
//! Interface-first layout (see `secret_store/` for the pattern): `mod.rs`
//! holds the shared session core — struct, recorder/AI/metrics plumbing and
//! the platform-neutral methods — while the platform constructors and their
//! reader loops live in per-OS sibling files behind the `Pty` trait:
//!
//! - [`unix`]:    `UnixPty` (forkpty) + poll()-based reader, ZMODEM routing
//! - [`windows`]: `WindowsPty` (ConPTY) + 25 ms try_read() polling reader

#[cfg(unix)]
pub(crate) mod unix;
#[cfg(windows)]
pub(crate) mod windows;

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use crate::terminal::metrics::{MetricsSnapshot, SystemMetrics};
use sysinfo::System;

pub use super::{Pty, PtySize, Result};

// PtyWriter is already pub and will be re-exported by parent module
#[cfg(unix)]
pub use unix::PtyWriter;

use super::grid::TerminalGrid;
use super::types::CellAttrs;
use super::vte::VteHandler;
use vte::Parser;

// ── Shared constructor building blocks (unix + windows) ─────────────────────
// The two platform constructors must stay behaviorally identical apart from
// the PTY transport, so recorder bootstrap, output feeding, and metrics
// collection are shared helpers instead of copy-pasted closures.

/// Recorder slot bootstrap shared by both platform constructors. Creates the
/// recorder BEFORE the reader thread spawns so capture starts at the very
/// first byte.
fn make_recorder_slot(
    record: bool,
    shell: &str,
    cols: u16,
    rows: u16,
) -> Arc<Mutex<Option<crate::terminal::cast::CastRecorder>>> {
    Arc::new(Mutex::new(if record {
        let title = std::path::Path::new(shell)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| shell.to_string());
        match crate::terminal::cast::new_recording_path(&title).and_then(|path| {
            crate::terminal::cast::CastRecorder::create(
                &path,
                cols,
                rows,
                Some(title.clone()),
                Some("localhost".into()),
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
    }))
}

/// Shared reader-side chunk handling: AI display rewrite → VTE parse into the
/// grid → version bump → asciicast tap → AI capture tap. Both platform
/// constructors call this so the choreography can't drift.
fn feed_chunk(
    parser: &mut Parser,
    attrs: &mut CellAttrs,
    grid: &Arc<Mutex<TerminalGrid>>,
    recorder: &Arc<Mutex<Option<crate::terminal::cast::CastRecorder>>>,
    ai_capture: &crate::terminal::ai_capture::AiCaptureSlot,
    bytes: &[u8],
) {
    // Display path goes through the AI capture's echo rewrite when armed
    // (bare command instead of the sentinel wrapper); the taps below still
    // see the raw bytes.
    let display = crate::terminal::ai_capture::display_bytes(ai_capture, bytes);
    let mut grid = grid.lock().unwrap();
    let mut handler = VteHandler {
        grid: &mut grid,
        attrs,
    };
    for &byte in &display {
        parser.advance(&mut handler, byte);
    }
    // One bump per chunk (not per byte): the renderer's visible-rows cache
    // keys on this version to skip re-snapshotting when nothing changed.
    grid.version = grid.version.wrapping_add(1);
    // Asciicast tap: capture the raw chunk with a timestamp.
    if let Ok(mut slot) = recorder.lock() {
        if let Some(rec) = slot.as_mut() {
            rec.output(bytes);
        }
    }
    // AI run-feedback tap (same tap point; only armed while a capture is
    // active).
    if let Ok(slot) = ai_capture.lock() {
        if let Some(cap) = slot.as_ref() {
            if let Ok(mut cap) = cap.lock() {
                cap.feed(bytes);
            }
        }
    }
}

/// 1-minute load average of the local machine. sysinfo's `load_average` is
/// unix-only; platforms without the syscall report 0.
fn platform_load_1() -> f32 {
    #[cfg(unix)]
    {
        System::load_average().one as f32
    }
    #[cfg(windows)]
    {
        0.0f32
    }
}

/// Background thread for local metrics collection, shared by both platform
/// constructors. Gated by `gate` (raised by the UI only while the Metrics
/// drawer is visible for the focused pane) and bounded by `alive`: the thread
/// exits within a tick of the child dying, so closed sessions don't leak a
/// thread that refreshes sysinfo and forces repaints forever.
fn spawn_metrics_collector(
    alive: &Arc<AtomicBool>,
    metrics: &Arc<Mutex<MetricsSnapshot>>,
    gate: &Arc<AtomicBool>,
) {
    let metrics_clone = Arc::clone(metrics);
    let metrics_gate = Arc::clone(gate);
    let alive_for_metrics = Arc::clone(alive);
    thread::Builder::new()
        .name("metrics-collector".to_string())
        .spawn(move || {
            let mut sys = System::new();
            let mut nets = sysinfo::Networks::new_with_refreshed_list();
            let mut prev_rx: Option<u64> = None;
            let mut prev_tx: Option<u64> = None;
            let mut last_poll: Option<Instant> = None;
            while alive_for_metrics.load(std::sync::atomic::Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_secs(1));
                // 1 s tick keeps enable→first-sample latency low at zero
                // cost while the gate is off; collection itself stays on
                // the 5 s cadence.
                if !metrics_gate.load(std::sync::atomic::Ordering::Relaxed) {
                    continue;
                }
                if last_poll.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(5)) {
                    continue;
                }
                last_poll = Some(Instant::now());
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
                let load_1 = platform_load_1();
                let m = SystemMetrics {
                    cpu_percent: sys.global_cpu_usage(),
                    mem_used_bytes: sys.used_memory(),
                    mem_total_bytes: sys.total_memory(),
                    load_1,
                    net_rx_bytes_per_sec: net_rx_rate,
                    net_tx_bytes_per_sec: net_tx_rate,
                };
                metrics_clone.lock().unwrap().push(m);
                // Metrics snapshots change independently of terminal output;
                // wake the UI so the drawer's sparklines keep animating.
                // Only while the drawer is actually watching this session.
                crate::repaint::notify_repaint();
            }
        })
        .ok();
}

/// Real PTY session with background reader thread
pub struct RealPtySession {
    /// Platform PTY transport as a trait object — the session core is
    /// transport-agnostic; each platform constructor boxes its concrete PTY.
    pub(crate) pty: Option<Arc<Mutex<Box<dyn Pty>>>>,
    /// Unix-only: a dup of the pty master for keystroke writes, kept apart
    /// from the mutex-guarded transport so writes never contend with the
    /// reader's lock snapshots. Windows writes through the pty mutex instead.
    #[cfg(unix)]
    pub(crate) writer: Option<PtyWriter>,
    pub grid: Arc<Mutex<TerminalGrid>>,
    alive: Arc<AtomicBool>,
    _reader_thread: Option<thread::JoinHandle<()>>,
    cached_shell_name: Arc<Mutex<Option<String>>>,
    last_shell_check: Arc<Mutex<Instant>>,
    /// Live system metrics collected from the local machine
    pub metrics: Arc<Mutex<MetricsSnapshot>>,
    /// Gate for the metrics collector: the UI raises it only while the
    /// Metrics drawer is visible for the focused pane (mirrors SSH sessions).
    /// The collector thread also exits once `alive` latches false, so closed
    /// sessions stop costing a thread + a sysinfo refresh every 5 s.
    metrics_enabled: Arc<AtomicBool>,
    /// Asciicast recorder slot: `Some` while the session is being recorded.
    /// Cloned into the reader thread at spawn, so the tap sees it.
    pub recorder: Arc<Mutex<Option<crate::terminal::cast::CastRecorder>>>,
    /// AI run-feedback capture slot: `Some` while a confirmed AI command's
    /// output is being captured for the chat. Cloned into the reader thread
    /// like the recorder slot.
    pub ai_capture: crate::terminal::ai_capture::AiCaptureSlot,
    /// ZMODEM tap: watches output for a remote `sz` ZRQINIT and, once armed
    /// or triggered, diverts all output into the transfer bridge. Cloned
    /// into the reader thread at spawn, like the recorder slot. (Windows
    /// carries the same inert slots — see `terminal::zmodem`.)
    pub zmodem_tap: Arc<Mutex<Option<crate::terminal::zmodem::ZmodemTap>>>,
    /// Active ZMODEM transfer bridge (engine thread + its input path).
    pub zmodem_bridge: Arc<Mutex<Option<crate::terminal::zmodem::ZmodemBridge>>>,
    /// Chip state for the UI: what is transferring and how far it got.
    pub zmodem_transfer: Arc<Mutex<Option<crate::terminal::zmodem::ZmodemTransfer>>>,
    /// ZMODEM support on this machine (probed at session creation; `Missing`
    /// on platforms without the engine).
    pub zmodem_support: Arc<std::sync::atomic::AtomicU8>,
}

impl RealPtySession {
    #[allow(dead_code)] // integration tests (tests/e2e_tests.rs); bin tree spawns via with_scrollback_limit
    pub fn new(id: usize, cols: u16, rows: u16, shell: &str) -> Result<Self> {
        Self::with_scrollback_limit(
            id,
            cols,
            rows,
            shell,
            TerminalGrid::DEFAULT_MAX_SCROLLBACK_BYTES,
            None,
            false,
        )
    }

    /// A session-shaped placeholder with no PTY, used when spawning fails so
    /// the UI can show its standard "session ended" state instead of the whole
    /// app panicking mid-frame. Writes fail, `is_connected()` is false.
    pub fn dead_placeholder(cols: usize, rows: usize) -> Self {
        Self {
            pty: None,
            #[cfg(unix)]
            writer: None,
            grid: Arc::new(Mutex::new(TerminalGrid::with_scrollback_limit(
                cols,
                rows,
                TerminalGrid::DEFAULT_MAX_SCROLLBACK_BYTES,
            ))),
            alive: Arc::new(AtomicBool::new(false)),
            _reader_thread: None,
            cached_shell_name: Arc::new(Mutex::new(None)),
            last_shell_check: Arc::new(Mutex::new(Instant::now())),
            metrics: Arc::new(Mutex::new(MetricsSnapshot::new())),
            metrics_enabled: Arc::new(AtomicBool::new(false)),
            recorder: Arc::new(Mutex::new(None)),
            ai_capture: Arc::new(Mutex::new(None)),
            zmodem_tap: Arc::new(Mutex::new(None)),
            zmodem_bridge: Arc::new(Mutex::new(None)),
            zmodem_transfer: Arc::new(Mutex::new(None)),
            zmodem_support: Arc::new(std::sync::atomic::AtomicU8::new(0)),
        }
    }

    // ── ZMODEM (slot readers — shared; engine entry points are per-OS) ───

    /// ZMODEM support on this machine (probed at session creation).
    pub fn zmodem_support(&self) -> crate::terminal::zmodem::ZmodemSupport {
        crate::terminal::zmodem::ZmodemSupport::from_u8(
            self.zmodem_support
                .load(std::sync::atomic::Ordering::Relaxed),
        )
    }

    /// Abort any ZMODEM transfer on this session (no-op when idle).
    pub fn cancel_zmodem(&self) {
        if let Ok(mut b) = self.zmodem_bridge.lock() {
            if let Some(bridge) = b.as_mut() {
                bridge.cancel();
            }
            *b = None;
        }
        if let Ok(mut t) = self.zmodem_tap.lock() {
            if let Some(tap) = t.as_mut() {
                tap.reset();
            }
        }
        // The transfer slot keeps its final state for the status chip until
        // the next transfer replaces it.
    }

    /// Bytes transferred so far by the active bridge (0 when idle).
    pub fn zmodem_bytes(&self) -> u64 {
        self.zmodem_bridge
            .lock()
            .ok()
            .and_then(|b| b.as_ref().map(|bridge| bridge.bytes()))
            .unwrap_or(0)
    }

    /// Resize the PTY
    pub fn resize(&mut self, cols: u16, rows: u16) -> io::Result<()> {
        if let Some(ref pty) = self.pty {
            let mut pty_ref = pty.lock().unwrap();
            return pty_ref
                .resize(PtySize::new(rows, cols))
                .map_err(|e| io::Error::other(e.to_string()));
        }
        Err(io::Error::new(
            io::ErrorKind::NotConnected,
            "PTY not available",
        ))
    }

    /// Check if the PTY session is still alive
    #[allow(dead_code)] // integration tests (tests/e2e_tests.rs); production reads has_exited()
    pub fn is_alive(&self) -> bool {
        if let Some(ref pty) = self.pty {
            let pty_ref = pty.lock().unwrap();
            return pty_ref.is_alive();
        }
        false
    }

    /// Raise/lower the metrics collector gate. The UI calls this every frame
    /// while the Metrics drawer is visible for the focused pane — mirroring
    /// `SshSession::set_metrics_enabled` — so a hidden drawer costs nothing.
    pub fn set_metrics_enabled(&self, enabled: bool) {
        self.metrics_enabled
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
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

    /// Get the shell name. Served from a 1 s cache; refreshes happen on a
    /// background thread — the unix detection (`ps` + `pgrep`) costs 5–30 ms
    /// and this runs on the UI thread every frame (status line), so a
    /// synchronous refresh was a recurring frame hitch. Callers see the
    /// stale value (or `None`, which the UI maps to the shell-path fallback)
    /// until the fresh one lands.
    pub fn get_shell_name(&self) -> Option<String> {
        const TTL: std::time::Duration = std::time::Duration::from_secs(1);

        let cached = self.cached_shell_name.lock().unwrap().clone();
        let last = *self.last_shell_check.lock().unwrap();
        if cached.is_some() && last.elapsed() < TTL {
            return cached;
        }

        // Dead session: freeze the last known name, stop spawning refreshes.
        if !self.is_connected() {
            return cached;
        }

        // Expired (or first call). The 1 s `last_shell_check` stamp doubles
        // as the in-flight guard — it is set at refresh START, so while the
        // background detection runs (≪1 s) no extra threads spawn.
        {
            let mut last = self.last_shell_check.lock().unwrap();
            if last.elapsed() < TTL {
                return self.cached_shell_name.lock().unwrap().clone();
            }
            *last = Instant::now();
        }

        self.refresh_shell_name().or(cached)
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

    /// E2E regression (2026-10-30 report): a freshly opened LOCAL terminal
    /// showed "[Session ended]" — the login zsh died right after spawn. This
    /// exercises the exact production spawn path (forkpty + `zsh -l` + HOME
    /// cwd + reader thread) and asserts the shell stays alive, accepts input,
    /// and executes it — i.e. the banner must NOT appear on a fresh terminal.
    #[cfg(unix)]
    #[test]
    fn fresh_local_login_shell_stays_alive_and_accepts_input() {
        let session = RealPtySession::with_scrollback_limit(
            1,
            80,
            24,
            "/bin/zsh",
            1024 * 1024,
            None, // default cwd (HOME), like a plain "new terminal"
            false,
        )
        .expect("spawn login zsh");

        // Login shells load /etc/zprofile + ~/.zprofile + ~/.zshrc (conda,
        // nvm on this machine) — give them time to settle.
        std::thread::sleep(std::time::Duration::from_millis(1500));
        assert!(
            !session.has_exited(),
            "login zsh exited within 1.5s of spawn — the session-ended bug"
        );

        session
            .write(b"echo PORTAL-SURVIVED\n")
            .expect("write probe");
        let grid = session.get_grid();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let found = {
                let g = grid.lock().unwrap();
                g.cells.iter().any(|row| {
                    let line: String = row.iter().map(|c| c.c).collect();
                    line.contains("PORTAL-SURVIVED")
                })
            };
            if found {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "shell did not echo the probe within 5s"
            );
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(
            !session.has_exited(),
            "login zsh exited after executing input"
        );
    }

    /// E2E: the PTY output tap lands in a `.cast` file — start recording,
    /// run `echo hi`, stop, and the parsed recording contains an Output event
    /// with the echoed text.
    #[cfg(unix)]
    #[test]
    fn recording_captures_pty_output_as_cast() {
        use crate::terminal::cast::{parse_cast, CastRecorder, EventKind};

        let session =
            RealPtySession::with_scrollback_limit(1, 80, 24, "/bin/sh", 1024 * 1024, None, false)
                .expect("spawn sh");

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("e2e.cast");
        let recorder =
            CastRecorder::create(&path, 80, 24, Some("sh".into()), None).expect("create recorder");
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

        let session =
            RealPtySession::with_scrollback_limit(1, 80, 24, "/bin/sh", 1024 * 1024, None, true)
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
        assert!(
            !cast.events.is_empty(),
            "shell output captured from the start"
        );
    }

    /// Verify that is_connected() returns false when the alive flag is false.
    /// This simulates what happens when the reader thread encounters a read error
    /// and sets alive to false before exiting.
    #[test]
    fn test_is_connected_false_when_reader_thread_dies() {
        // Simulate: alive = false (reader thread exited on error), no PTY
        let grid = Arc::new(Mutex::new(TerminalGrid::with_scrollback_limit(
            80,
            24,
            1024 * 1024,
        )));
        let session = RealPtySession {
            pty: None,
            #[cfg(unix)]
            writer: None,
            grid,
            alive: Arc::new(AtomicBool::new(false)), // Reader thread died
            _reader_thread: None,
            cached_shell_name: Arc::new(Mutex::new(None)),
            last_shell_check: Arc::new(Mutex::new(Instant::now())),
            metrics: Arc::new(Mutex::new(MetricsSnapshot::new())),
            metrics_enabled: Arc::new(AtomicBool::new(false)),
            recorder: Arc::new(Mutex::new(None)),
            ai_capture: Arc::new(Mutex::new(None)),
            zmodem_tap: Arc::new(Mutex::new(None)),
            zmodem_bridge: Arc::new(Mutex::new(None)),
            zmodem_transfer: Arc::new(Mutex::new(None)),
            zmodem_support: Arc::new(std::sync::atomic::AtomicU8::new(0)),
        };

        assert!(
            !session.is_connected(),
            "is_connected() must return false when alive flag is false"
        );
    }

    /// Verify that is_connected() is a pure mirror of the `alive` flag.
    /// A session without a PTY is constructed with `alive=false`, so
    /// `is_connected()` must report false. (Under the new monotonic model,
    /// `alive=true && pty=None` is unreachable — `alive` is owned solely by
    /// the reader thread of a real PTY.)
    #[test]
    fn test_is_connected_false_when_no_pty() {
        let grid = Arc::new(Mutex::new(TerminalGrid::with_scrollback_limit(
            80,
            24,
            1024 * 1024,
        )));
        let session = RealPtySession {
            pty: None,
            #[cfg(unix)]
            writer: None,
            grid,
            alive: Arc::new(AtomicBool::new(false)),
            _reader_thread: None,
            cached_shell_name: Arc::new(Mutex::new(None)),
            last_shell_check: Arc::new(Mutex::new(Instant::now())),
            metrics: Arc::new(Mutex::new(MetricsSnapshot::new())),
            metrics_enabled: Arc::new(AtomicBool::new(false)),
            recorder: Arc::new(Mutex::new(None)),
            ai_capture: Arc::new(Mutex::new(None)),
            zmodem_tap: Arc::new(Mutex::new(None)),
            zmodem_bridge: Arc::new(Mutex::new(None)),
            zmodem_transfer: Arc::new(Mutex::new(None)),
            zmodem_support: Arc::new(std::sync::atomic::AtomicU8::new(0)),
        };

        assert!(
            !session.is_connected(),
            "is_connected() must return false when alive flag is false"
        );
    }

    /// Verify that is_connected() returns true while `alive` is true.
    /// This pins the monotonic invariant: as long as the reader thread has not
    /// observed a genuine child exit, the session reports connected — no matter
    /// what transient read errors occurred.
    #[test]
    fn test_is_connected_true_while_alive_true() {
        let grid = Arc::new(Mutex::new(TerminalGrid::with_scrollback_limit(
            80,
            24,
            1024 * 1024,
        )));
        let session = RealPtySession {
            pty: None,
            #[cfg(unix)]
            writer: None,
            grid,
            alive: Arc::new(AtomicBool::new(true)),
            _reader_thread: None,
            cached_shell_name: Arc::new(Mutex::new(None)),
            last_shell_check: Arc::new(Mutex::new(Instant::now())),
            metrics: Arc::new(Mutex::new(MetricsSnapshot::new())),
            metrics_enabled: Arc::new(AtomicBool::new(false)),
            recorder: Arc::new(Mutex::new(None)),
            ai_capture: Arc::new(Mutex::new(None)),
            zmodem_tap: Arc::new(Mutex::new(None)),
            zmodem_bridge: Arc::new(Mutex::new(None)),
            zmodem_transfer: Arc::new(Mutex::new(None)),
            zmodem_support: Arc::new(std::sync::atomic::AtomicU8::new(0)),
        };

        assert!(
            session.is_connected(),
            "is_connected() must return true while alive=true, regardless of PTY state"
        );
    }

    /// Verify that Drop sets alive to false, ensuring is_connected() transitions correctly.
    #[test]
    fn test_drop_sets_alive_false() {
        let alive = Arc::new(AtomicBool::new(true));
        let grid = Arc::new(Mutex::new(TerminalGrid::with_scrollback_limit(
            80,
            24,
            1024 * 1024,
        )));

        {
            let _session = RealPtySession {
                pty: None,
                #[cfg(unix)]
                writer: None,
                grid: Arc::clone(&grid),
                alive: Arc::clone(&alive),
                _reader_thread: None,
                cached_shell_name: Arc::new(Mutex::new(None)),
                last_shell_check: Arc::new(Mutex::new(Instant::now())),
                metrics: Arc::new(Mutex::new(MetricsSnapshot::new())),
                metrics_enabled: Arc::new(AtomicBool::new(false)),
                recorder: Arc::new(Mutex::new(None)),
                ai_capture: Arc::new(Mutex::new(None)),
                zmodem_tap: Arc::new(Mutex::new(None)),
                zmodem_bridge: Arc::new(Mutex::new(None)),
                zmodem_transfer: Arc::new(Mutex::new(None)),
                zmodem_support: Arc::new(std::sync::atomic::AtomicU8::new(0)),
            };
            // session is alive while in scope
            assert!(alive.load(Ordering::Relaxed));
        }
        // After drop, alive must be false
        assert!(
            !alive.load(Ordering::Relaxed),
            "Drop must set alive to false"
        );
    }
}
