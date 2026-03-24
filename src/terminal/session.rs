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

pub use super::{Pty, PtySize, Result};

#[cfg(unix)]
pub use super::UnixPty;

#[cfg(windows)]
pub use super::WindowsPty;

use super::grid::TerminalGrid;
use super::types::CellAttrs;
use super::vte::VteHandler;
use vte::Parser;

/// Safe wrapper for writing to a raw fd without closing it on drop
#[cfg(unix)]
pub struct PtyWriter {
    pub fd: std::os::unix::io::RawFd,
}

#[cfg(unix)]
impl PtyWriter {
    pub fn write(&self, data: &[u8]) -> io::Result<()> {
        use std::mem::ManuallyDrop;
        use std::os::unix::io::FromRawFd;
        let mut file = ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(self.fd) });
        std::io::Write::write_all(&mut *file, data)
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
}

impl RealPtySession {
    #[cfg(unix)]
    #[allow(dead_code)]
    pub fn new(id: usize, cols: u16, rows: u16, shell: &str) -> Result<Self> {
        Self::with_scrollback_limit(id, cols, rows, shell, TerminalGrid::DEFAULT_MAX_SCROLLBACK_BYTES)
    }

    #[cfg(unix)]
    pub fn with_scrollback_limit(id: usize, cols: u16, rows: u16, shell: &str, scrollback_limit_bytes: usize) -> Result<Self> {
        use std::os::unix::io::AsRawFd;
        let _ = id;

        let grid: Arc<Mutex<TerminalGrid>> = Arc::new(Mutex::new(TerminalGrid::with_scrollback_limit(cols as usize, rows as usize, scrollback_limit_bytes)));
        let pty = UnixPty::spawn(shell, &["-l"], PtySize::new(rows, cols))?;
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

        let reader_thread = thread::Builder::new()
            .name(format!("pty-reader-{}", id))
            .spawn(move || {
                let mut parser = Parser::new();
                let mut attrs = CellAttrs::default();

                while alive_clone.load(Ordering::Relaxed) {
                    let data = {
                        let mut pty_ref = pty_clone.lock().unwrap();
                        pty_ref.try_read()
                    };

                    match data {
                        Ok(data) => {
                            if data.is_empty() {
                                // Check if PTY is still alive
                                let is_alive = {
                                    let pty_ref = pty_clone.lock().unwrap();
                                    pty_ref.is_alive()
                                };
                                if !is_alive {
                                    break;
                                }
                                std::thread::sleep(std::time::Duration::from_millis(10));
                                continue;
                            }

                            for byte in &data {
                                let mut grid = grid_clone.lock().unwrap();
                                let mut handler = VteHandler {
                                    grid: &mut *grid,
                                    attrs: &mut attrs,
                                };
                                parser.advance(&mut handler, *byte);
                            }
                        }
                        Err(_) => {
                            break;
                        }
                    }
                }
            })
            .map_err(|_| super::Error::SpawnFailed("Failed to spawn reader thread".to_string()))?;

        let cached_shell_name = Arc::new(Mutex::new(None));
        let last_shell_check = Arc::new(Mutex::new(Instant::now()));

        Ok(Self {
            pty: Some(pty),
            writer: Some(writer),
            grid,
            alive,
            _reader_thread: Some(reader_thread),
            cached_shell_name,
            last_shell_check,
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
            return pty_ref.resize(PtySize::new(rows, cols)).map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()));
        }
        #[cfg(windows)]
        if let Some(ref pty) = self.pty {
            let mut pty_ref = pty.lock().unwrap();
            return pty_ref.resize(PtySize::new(rows, cols)).map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()));
        }
        Err(io::Error::new(io::ErrorKind::NotConnected, "PTY not available"))
    }

    /// Check if the PTY session is still alive
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

    /// Get the shell name (cached for 5 seconds)
    pub fn get_shell_name(&self) -> Option<String> {
        // Check cache first (5 second TTL)
        {
            let mut last_check = self.last_shell_check.lock().unwrap();
            if let Some(ref name) = *self.cached_shell_name.lock().unwrap() {
                if last_check.elapsed() < std::time::Duration::from_secs(5) {
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
