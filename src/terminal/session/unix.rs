//! Unix PTY session: forkpty spawn + poll()-based reader loop.
//!
//! The platform half of `super::RealPtySession`. Writes go through a dup of
//! the pty master ([`PtyWriter`]) so keystrokes never contend with the
//! reader's lock snapshots, and ZMODEM transfers divert output in the reader
//! loop before it reaches VTE/scrollback.

use std::io;
use std::sync::{Arc, Mutex};
use std::sync::atomic::Ordering;
use std::thread;

use vte::Parser;

use super::RealPtySession;
use super::super::{Pty, PtySize, Result};
use crate::terminal::grid::TerminalGrid;
use crate::terminal::types::CellAttrs;

pub use crate::terminal::unix_pty::PtyWriter;

impl RealPtySession {
    pub fn with_scrollback_limit(id: usize, cols: u16, rows: u16, shell: &str, scrollback_limit_bytes: usize, cwd: Option<&str>, record: bool) -> Result<Self> {
        let _ = id;

        let grid: Arc<Mutex<TerminalGrid>> = Arc::new(Mutex::new(TerminalGrid::with_scrollback_limit(cols as usize, rows as usize, scrollback_limit_bytes)));
        let pty = crate::terminal::UnixPty::spawn_in(shell, &["-l"], PtySize::new(rows, cols), cwd)?;
        let pty: Arc<Mutex<Box<dyn Pty>>> = Arc::new(Mutex::new(Box::new(pty)));

        let fd = {
            let pty_ref = pty.lock().unwrap();
            pty_ref
                .poll_descriptor()
                .expect("unix pty always exposes its master fd")
        };
        let fd_dup = unsafe { libc::dup(fd) };
        if fd_dup < 0 {
            return Err(super::super::Error::SpawnFailed("Failed to dup fd".to_string()));
        }

        let writer = PtyWriter { fd: fd_dup };
        let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let grid_clone: Arc<Mutex<TerminalGrid>> = Arc::clone(&grid);
        let pty_clone = Arc::clone(&pty);
        let alive_clone = Arc::clone(&alive);
        // Zero-loss auto-record: the recorder is created before the reader
        // thread spawns so capture starts at the very first byte.
        let recorder = super::make_recorder_slot(record, shell, cols, rows);
        let recorder_clone = Arc::clone(&recorder);
        let ai_capture: crate::terminal::ai_capture::AiCaptureSlot =
            Arc::new(Mutex::new(None));
        let ai_capture_clone = Arc::clone(&ai_capture);

        // ZMODEM slots — cloned into the reader thread like the recorder.
        let zmodem_tap: Arc<Mutex<Option<crate::terminal::zmodem::ZmodemTap>>> =
            Arc::new(Mutex::new(None));
        let zmodem_bridge: Arc<Mutex<Option<crate::terminal::zmodem::ZmodemBridge>>> =
            Arc::new(Mutex::new(None));
        let zmodem_transfer: Arc<Mutex<Option<crate::terminal::zmodem::ZmodemTransfer>>> =
            Arc::new(Mutex::new(None));
        // Local shells: sz/rz are a synchronous PATH scan away — probe now.
        let zmodem_support = Arc::new(std::sync::atomic::AtomicU8::new(
            crate::terminal::zmodem::local_zmodem_support().to_u8(),
        ));
        let zmodem_tap_clone = Arc::clone(&zmodem_tap);
        let zmodem_bridge_clone = Arc::clone(&zmodem_bridge);

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
                        pty_ref
                            .poll_descriptor()
                            .expect("unix pty always exposes its master fd")
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
                                // ZMODEM diversion runs BEFORE the VTE parse and
                                // the asciicast tap so binary protocol frames
                                // never reach scrollback or the recording.
                                let diverted = crate::terminal::zmodem::route_chunk(
                                    &zmodem_tap_clone,
                                    &zmodem_bridge_clone,
                                    &data,
                                    |bytes| {
                                        super::feed_chunk(
                                            &mut parser,
                                            &mut attrs,
                                            &grid_clone,
                                            &recorder_clone,
                                            &ai_capture_clone,
                                            bytes,
                                        )
                                    },
                                );
                                if diverted == crate::terminal::zmodem::ChunkRoute::Diverted {
                                    // Wake the UI (progress counter moved) and
                                    // skip the normal path entirely.
                                    crate::repaint::notify_repaint();
                                    continue;
                                }
                                // route_chunk's emit closure is the ONLY render
                                // path for this chunk — on diversion it may have
                                // seen just the non-protocol prefix, so callers
                                // must never parse `data` again here (a second
                                // parse paints every chunk twice).
                                // Every write path and resize()/reflow keep
                                // rows exactly `cols`-wide, so no per-chunk
                                // normalization scan is needed.
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
            .map_err(|_| super::super::Error::SpawnFailed("Failed to spawn reader thread".to_string()))?;

        let cached_shell_name = Arc::new(Mutex::new(None));
        let last_shell_check = Arc::new(Mutex::new(std::time::Instant::now()));
        let metrics = Arc::new(Mutex::new(crate::terminal::metrics::MetricsSnapshot::new()));
        let metrics_enabled = Arc::new(std::sync::atomic::AtomicBool::new(false));

        super::spawn_metrics_collector(&alive, &metrics, &metrics_enabled);

        Ok(Self {
            pty: Some(pty),
            writer: Some(writer),
            grid,
            alive,
            _reader_thread: Some(reader_thread),
            cached_shell_name,
            last_shell_check,
            metrics,
            metrics_enabled,
            recorder,
            ai_capture,
            zmodem_tap,
            zmodem_bridge,
            zmodem_transfer,
            zmodem_support,
        })
    }

    /// Write data to the PTY
    pub fn write(&self, data: &[u8]) -> io::Result<()> {
        if let Some(ref writer) = self.writer {
            return writer.write(data);
        }
        Err(io::Error::new(io::ErrorKind::NotConnected, "PTY not available"))
    }

    /// Kick off a shell-name refresh on a background thread. The detection
    /// (`ps` + `pgrep`) costs 5–30 ms, so it must run unlocked: grab the pid
    /// under a brief lock, then scan without the PTY mutex held, or the scan
    /// would stall keystroke writes. The fresh name lands in the cache
    /// asynchronously — callers keep seeing the stale value until then.
    pub(super) fn refresh_shell_name(&self) -> Option<String> {
        let Some(ref pty) = self.pty else {
            return None;
        };
        let pty = Arc::clone(pty);
        let cache = Arc::clone(&self.cached_shell_name);
        let _ = thread::Builder::new()
            .name("shell-name-detect".to_string())
            .spawn(move || {
                let pid = pty.lock().unwrap().child_pid();
                if let Some(pid) = pid {
                    if let Some(name) =
                        crate::terminal::unix_pty::detect_shell_name(pid as i32)
                    {
                        *cache.lock().unwrap() = Some(name);
                    }
                }
            });
        None // the fresh name arrives via the cache, not the return value
    }

    // ── ZMODEM ───────────────────────────────────────────────────────────

    /// Sink for the ZMODEM engine: a fresh dup of the pty master so the
    /// transfer thread can write protocol frames without racing the
    /// session's own writer.
    fn make_zmodem_sink(&self) -> io::Result<crate::terminal::zmodem::SessionWriter> {
        let pty = self
            .pty
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "PTY not available"))?;
        let master_fd = pty
            .lock()
            .unwrap()
            .poll_descriptor()
            .expect("unix pty always exposes its master fd");
        let fd_dup = unsafe { libc::dup(master_fd) };
        if fd_dup < 0 {
            return Err(io::Error::last_os_error());
        }
        let writer = PtyWriter { fd: fd_dup };
        Ok(crate::terminal::zmodem::SessionWriter::new(move |data| {
            writer.write(data)
        }))
    }

    /// Start a ZMODEM upload of `path` into this session. Arms the tap (all
    /// output diverts) and spawns the transfer engine, which types `rz\r`
    /// into the shell itself and runs the ZRQINIT → ZRINIT handshake.
    pub fn start_zmodem_upload(&self, path: std::path::PathBuf) -> io::Result<()> {
        use crate::terminal::zmodem::{TransferRequest, ZmodemBridge};
        self.cancel_zmodem();
        let sink = self.make_zmodem_sink()?;
        let mut tap = crate::terminal::zmodem::ZmodemTap::new();
        let held = tap.arm();
        *self
            .zmodem_tap
            .lock()
            .map_err(|_| io::Error::other("zmodem tap lock poisoned"))? = Some(tap);
        let mut bridge = ZmodemBridge::spawn(
            TransferRequest::Upload { path },
            sink,
            Arc::clone(&self.zmodem_transfer),
        );
        if !held.is_empty() {
            bridge.feed(&held);
        }
        *self
            .zmodem_bridge
            .lock()
            .map_err(|_| io::Error::other("zmodem bridge lock poisoned"))? = Some(bridge);
        Ok(())
    }

    /// Start a ZMODEM download of remote `remote_path` into `local_dir`:
    /// arm a passive tap, spawn a waiting bridge, then type `sz '<path>'`
    /// so the remote side announces itself and the tap triggers the engine
    /// on its first frame.
    pub fn start_zmodem_download(
        &self,
        remote_path: &str,
        local_dir: std::path::PathBuf,
    ) -> io::Result<()> {
        use crate::terminal::zmodem::{shell_quote_single, TransferRequest, ZmodemBridge};
        self.cancel_zmodem();
        let sink = self.make_zmodem_sink()?;
        *self
            .zmodem_tap
            .lock()
            .map_err(|_| io::Error::other("zmodem tap lock poisoned"))? =
            Some(crate::terminal::zmodem::ZmodemTap::new());
        let bridge = ZmodemBridge::spawn(
            TransferRequest::Download {
                output_dir: local_dir,
            },
            sink,
            Arc::clone(&self.zmodem_transfer),
        );
        *self
            .zmodem_bridge
            .lock()
            .map_err(|_| io::Error::other("zmodem bridge lock poisoned"))? = Some(bridge);
        self.write(format!("sz {}\r", shell_quote_single(remote_path)).as_bytes())
    }

    /// ZMODEM support on this machine (from the PATH scan at creation).
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
}
