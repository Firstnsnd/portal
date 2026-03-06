//! Terminal PTY abstraction layer

mod session;

#[cfg(unix)]
mod unix_pty;
#[cfg(windows)]
mod windows_pty;

pub use session::{TerminalCell, TerminalGrid, CellAttrs, VteHandler, RealPtySession, DEFAULT_BG};

// PTY abstraction types
use std::process::ExitStatus;

/// PTY size in rows and columns
#[derive(Debug, Clone, Copy)]
pub struct PtySize {
    pub rows: u16,
    pub cols: u16,
    pub xpixel: u16,
    pub ypixel: u16,
}

impl PtySize {
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            rows,
            cols,
            xpixel: 0,
            ypixel: 0,
        }
    }
}

/// PTY error types
#[derive(Debug)]
#[allow(dead_code)]
pub enum Error {
    SpawnFailed(String),
    WriteFailed(String),
    ReadFailed(String),
    ResizeFailed(String),
    AlreadyClosed,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::SpawnFailed(s) => write!(f, "Failed to spawn PTY: {}", s),
            Error::WriteFailed(s) => write!(f, "Failed to write to PTY: {}", s),
            Error::ReadFailed(s) => write!(f, "Failed to read from PTY: {}", s),
            Error::ResizeFailed(s) => write!(f, "Failed to resize PTY: {}", s),
            Error::AlreadyClosed => write!(f, "PTY is already closed"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// PTY trait for platform-specific implementations
#[allow(dead_code)]
pub trait Pty {
    /// Spawn a new PTY with the given command and arguments
    fn spawn(command: &str, args: &[&str], size: PtySize) -> Result<Self>
    where
        Self: Sized;

    /// Write data to the PTY
    fn write(&mut self, data: &[u8]) -> Result<()>;

    /// Try to read data from the PTY (non-blocking)
    fn try_read(&mut self) -> Result<Vec<u8>>;

    /// Resize the PTY
    fn resize(&mut self, size: PtySize) -> Result<()>;

    /// Check if the PTY is still alive
    fn is_alive(&self) -> bool;

    /// Try to wait for the child process to exit
    fn try_wait(&mut self) -> Result<Option<ExitStatus>>;

    /// Kill the child process
    fn kill(&mut self) -> Result<()>;

    /// Get the current shell process name (e.g., "bash", "zsh", "fish")
    /// Returns None if unable to determine the shell name
    fn get_shell_name(&self) -> Option<String> {
        None
    }
}

// Export platform-specific PTY implementation
#[cfg(unix)]
pub use unix_pty::UnixPty;

#[cfg(windows)]
pub use windows_pty::WindowsPty;
