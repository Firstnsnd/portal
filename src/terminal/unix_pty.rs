//! Unix PTY implementation using pty crate

use super::{Error, Pty, PtySize, Result};
use pty::fork::Fork;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Unix PTY implementation
pub struct UnixPty {
    master: File,
    child_pid: i32,
    alive: Arc<AtomicBool>,
}

impl Pty for UnixPty {
    fn spawn(command: &str, args: &[&str], _size: PtySize) -> Result<Self> {
        // Use forkpty to create a PTY
        let fork = Fork::from_ptmx().map_err(|e| Error::SpawnFailed(e.to_string()))?;

        match fork {
            Fork::Parent(child_pid, master) => {
                // We're in the parent process
                // child_pid is the PID of the child
                // master is the master file descriptor

                // Duplicate the file descriptor to create a File
                let fd = master.as_raw_fd();
                let fd_dup = unsafe { libc::dup(fd) };
                if fd_dup < 0 {
                    return Err(Error::SpawnFailed("Failed to dup fd".to_string()));
                }

                let master_file = unsafe { File::from_raw_fd(fd_dup) };

                Ok(Self {
                    master: master_file,
                    child_pid,
                    alive: Arc::new(AtomicBool::new(true)),
                })
            }
            Fork::Child(ref _slave) => {
                // Child process - exec the command
                let shell = std::path::PathBuf::from(command);
                let _result = std::process::Command::new(&shell)
                    .args(args)
                    .env("TERM", "xterm-256color")
                    .status();

                // If exec fails, exit
                std::process::exit(1);
            }
        }
    }

    fn write(&mut self, data: &[u8]) -> Result<()> {
        if !self.alive.load(Ordering::Relaxed) {
            return Err(Error::AlreadyClosed);
        }
        self.master
            .write_all(data)
            .map_err(|e| Error::WriteFailed(e.to_string()))?;
        Ok(())
    }

    fn try_read(&mut self) -> Result<Vec<u8>> {
        if !self.alive.load(Ordering::Relaxed) {
            return Ok(Vec::new());
        }

        // Set non-blocking mode using libc
        let fd = self.master.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 {
            return Err(Error::ReadFailed("Failed to get flags".to_string()));
        }
        unsafe {
            if libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
                return Err(Error::ReadFailed("Failed to set non-blocking".to_string()));
            }
        }

        let mut buffer = vec![0u8; 8192];
        match self.master.read(&mut buffer) {
            Ok(n) if n > 0 => {
                buffer.truncate(n);
                Ok(buffer)
            }
            Ok(_) => Ok(Vec::new()),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(Vec::new()),
            Err(e) => Err(Error::ReadFailed(e.to_string())),
        }
    }

    fn resize(&mut self, size: PtySize) -> Result<()> {
        use libc::{winsize, TIOCSWINSZ};

        if !self.alive.load(Ordering::Relaxed) {
            return Err(Error::AlreadyClosed);
        }

        let ws = winsize {
            ws_row: size.rows,
            ws_col: size.cols,
            ws_xpixel: size.xpixel,
            ws_ypixel: size.ypixel,
        };

        unsafe {
            if libc::ioctl(self.master.as_raw_fd(), TIOCSWINSZ as _, &ws) < 0 {
                return Err(Error::ResizeFailed(format!(
                    "ioctl failed: {}",
                    io::Error::last_os_error()
                )));
            }
        }
        Ok(())
    }

    fn is_alive(&self) -> bool {
        if !self.alive.load(Ordering::Relaxed) {
            return false;
        }

        // Check if the child process is still alive
        unsafe {
            let result = libc::kill(self.child_pid, 0);
            if result == 0 {
                true // Process is alive
            } else {
                // errno is ESRCH (No such process)
                self.alive.store(false, Ordering::Relaxed);
                false
            }
        }
    }

    fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        if !self.alive.load(Ordering::Relaxed) {
            return Ok(None);
        }

        unsafe {
            let mut status: i32 = 0;
            let result = libc::waitpid(self.child_pid, &mut status, libc::WNOHANG);

            if result < 0 {
                // Error - check if ECHILD (no child processes)
                let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
                if errno == libc::ECHILD {
                    self.alive.store(false, Ordering::Relaxed);
                    return Ok(None);
                }
                return Err(Error::ReadFailed(format!("waitpid failed: {}", errno)));
            } else if result == 0 {
                // Child still alive
                Ok(None)
            } else {
                // Child exited
                self.alive.store(false, Ordering::Relaxed);
                Ok(Some(ExitStatus::from_raw(status)))
            }
        }
    }

    fn kill(&mut self) -> Result<()> {
        if !self.alive.load(Ordering::Relaxed) {
            return Ok(());
        }

        unsafe {
            if libc::kill(self.child_pid, libc::SIGTERM) < 0 {
                return Err(Error::SpawnFailed(format!(
                    "Failed to kill process: {}",
                    std::io::Error::last_os_error()
                )));
            }
        }
        self.alive.store(false, Ordering::Relaxed);
        Ok(())
    }
}

impl Drop for UnixPty {
    fn drop(&mut self) {
        let _ = self.kill();
    }
}

impl AsRawFd for UnixPty {
    fn as_raw_fd(&self) -> RawFd {
        self.master.as_raw_fd()
    }
}
