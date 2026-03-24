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
    /// Master file descriptor for the PTY
    pub master: File,
    /// Child process ID
    pub child_pid: i32,
    /// Atomic flag indicating if the PTY is still alive
    pub alive: Arc<AtomicBool>,
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
                // IMPORTANT: After fork(), only async-signal-safe functions can be called
                // before exec(). Using std::process::exit() would trigger destructors
                // and library cleanup (like CoreSpotlight/PowerLog on macOS) which
                // causes crashes because dispatch queues are broken after fork.

                // Set environment variables using libc (async-signal-safe)
                unsafe {
                    libc::setenv(b"TERM\0".as_ptr() as *const i8, b"xterm-256color\0".as_ptr() as *const i8, 1);
                    libc::setenv(b"LANG\0".as_ptr() as *const i8, b"en_US.UTF-8\0".as_ptr() as *const i8, 1);
                    libc::setenv(b"LC_ALL\0".as_ptr() as *const i8, b"en_US.UTF-8\0".as_ptr() as *const i8, 1);
                }

                // Build args for execvp (command + args + null terminator)
                let mut exec_args: Vec<*const i8> = Vec::with_capacity(args.len() + 2);
                let command_cstring = std::ffi::CString::new(command).unwrap_or_default();
                exec_args.push(command_cstring.as_ptr());

                let arg_cstrings: Vec<std::ffi::CString> = args
                    .iter()
                    .filter_map(|a| std::ffi::CString::new(*a).ok())
                    .collect();
                for arg in &arg_cstrings {
                    exec_args.push(arg.as_ptr());
                }
                exec_args.push(std::ptr::null());

                // Execute the command - this replaces the current process
                unsafe {
                    libc::execvp(command_cstring.as_ptr(), exec_args.as_ptr());
                    // If execvp returns, it failed - use _exit() which is async-signal-safe
                    libc::_exit(1);
                }
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

    fn get_shell_name(&self) -> Option<String> {
        if !self.alive.load(Ordering::Relaxed) {
            return None;
        }

        use std::process::Command;

        // Use `ps -p PID -o comm=` to get process name more reliably
        let output = Command::new("ps")
            .args(["-p", &self.child_pid.to_string(), "-o", "comm="])
            .output()
            .ok()?;

        if output.status.success() {
            let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }

        // Fallback to proc_pidpath
        use std::path::PathBuf;
        unsafe {
            let mut path: Vec<u8> = vec![0; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
            if libc::proc_pidpath(
                self.child_pid,
                path.as_mut_ptr() as *mut libc::c_void,
                path.len() as u32,
            ) > 0 {
                let null_pos = path.iter().position(|&b| b == 0).unwrap_or(path.len());
                let path_str = std::str::from_utf8(&path[..null_pos]).ok()?;
                
                PathBuf::from(path_str)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(|s| s.to_string())
            } else {
                None
            }
        }
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
