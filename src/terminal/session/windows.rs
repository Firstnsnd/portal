//! Windows PTY session: ConPTY spawn + short-interval try_read() polling.
//!
//! The platform half of `super::RealPtySession`. ConPTY has no pollable
//! non-blocking master descriptor, so the reader polls `try_read()` every
//! 25 ms (idle terminals cost one mutex check per tick), and writes go
//! through the pty mutex instead of a dup'd descriptor.

use std::io;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use super::RealPtySession;
use super::super::{Pty, PtySize, Result};
use crate::terminal::grid::TerminalGrid;
use crate::terminal::windows_pty::WindowsPty;

impl RealPtySession {
    pub fn with_scrollback_limit(id: usize, cols: u16, rows: u16, shell: &str, scrollback_limit_bytes: usize, cwd: Option<&str>, record: bool) -> Result<Self> {
        let _ = id;

        let grid: Arc<Mutex<TerminalGrid>> = Arc::new(Mutex::new(TerminalGrid::with_scrollback_limit(cols as usize, rows as usize, scrollback_limit_bytes)));

        // Shells differ in startup flags; only ones we know get any. (Unix
        // hardcodes `["-l"]` — a login flag that does not exist on Windows
        // shells.)
        let basename = std::path::Path::new(shell)
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        let args: Vec<&str> = if basename == "powershell" || basename == "pwsh" {
            // Skip the banner so the first frame isn't polluted.
            vec!["-NoLogo"]
        } else {
            vec![]
        };

        let pty = WindowsPty::spawn_in(shell, &args, PtySize::new(rows, cols), cwd)?;
        let pty: Arc<Mutex<Box<dyn Pty>>> = Arc::new(Mutex::new(Box::new(pty)));
        let alive = Arc::new(AtomicBool::new(true));
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

        let reader_thread = thread::Builder::new()
            .name(format!("pty-reader-{}", id))
            .spawn(move || {
                let mut parser = vte::Parser::new();
                let mut attrs = crate::terminal::types::CellAttrs::default();

                // ── INVARIANT ────────────────────────────────────────────
                // Same as unix: `alive` is set false ONLY via
                // `liveness_check`, and ONLY after `is_alive()`
                // (WaitForSingleObject) confirmed the child is gone.
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

                // No non-blocking master fd here: the ConPTY reader thread
                // fills `pending`, and we poll it on a short sleep — idle
                // terminals cost one mutex check per 25 ms.
                while alive_clone.load(Ordering::Relaxed) {
                    let data = {
                        let mut pty_ref = pty_clone.lock().unwrap();
                        pty_ref.try_read()
                    };
                    match data {
                        Ok(data) if !data.is_empty() => {
                            super::feed_chunk(
                                &mut parser,
                                &mut attrs,
                                &grid_clone,
                                &recorder_clone,
                                &ai_capture_clone,
                                &data,
                            );
                            // Wake the UI so the new output is painted promptly.
                            crate::repaint::notify_repaint();
                        }
                        _ => {
                            if !liveness_check() {
                                break;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(25));
                        }
                    }
                }
            })
            .map_err(|_| super::super::Error::SpawnFailed("Failed to spawn reader thread".to_string()))?;

        let cached_shell_name = Arc::new(Mutex::new(None));
        let last_shell_check = Arc::new(Mutex::new(std::time::Instant::now()));
        let metrics = Arc::new(Mutex::new(crate::terminal::metrics::MetricsSnapshot::new()));
        let metrics_enabled = Arc::new(AtomicBool::new(false));

        super::spawn_metrics_collector(&alive, &metrics, &metrics_enabled);

        Ok(Self {
            pty: Some(pty),
            grid,
            alive,
            _reader_thread: Some(reader_thread),
            cached_shell_name,
            last_shell_check,
            metrics,
            metrics_enabled,
            recorder,
            ai_capture,
        })
    }

    /// Write data to the PTY
    pub fn write(&self, data: &[u8]) -> io::Result<()> {
        if let Some(ref pty) = self.pty {
            let mut pty_ref = pty.lock().unwrap();
            return pty_ref.write(data).map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()));
        }
        Err(io::Error::new(io::ErrorKind::NotConnected, "PTY not available"))
    }

    /// Refresh the cached shell name synchronously. Windows detection is a
    /// cheap in-process lookup (the file stem of the shell we spawned), so
    /// unlike the unix `ps`/`pgrep` scan it can run under the pty mutex
    /// without any risk of a frame hitch.
    pub(super) fn refresh_shell_name(&self) -> Option<String> {
        let Some(ref pty) = self.pty else {
            return None;
        };
        let name = pty.lock().unwrap().get_shell_name();
        if let Some(name) = &name {
            *self.cached_shell_name.lock().unwrap() = Some(name.clone());
        }
        name
    }
}
