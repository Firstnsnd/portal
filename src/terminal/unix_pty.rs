//! Unix PTY implementation using pty crate

use super::{Error, Pty, PtySize, Result};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Get the process name for a given PID via `ps -p PID -o comm=`.
fn ps_comm(pid: u32) -> Option<String> {
    use std::process::Command;
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .ok()?;
    if output.status.success() {
        let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

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
    fn spawn(command: &str, args: &[&str], size: PtySize) -> Result<Self> {
        // forkpty() opens a PTY pair, forks, and in the CHILD does:
        //   setsid() + acquire controlling terminal (TIOCSCTTY) + dup2 the
        //   slave onto stdin/stdout/stderr. This is the exact setup the system
        //   Terminal uses, so a local shell behaves identically — proper job
        //   control, /dev/tty works, no spurious SIGHUP/SIGTTIN that would
        //   kill the shell. (The old `pty` 0.2 crate skipped TIOCSCTTY, leaving
        //   the shell with no controlling terminal — fragile and a source of
        //   unexplained "disconnects".)
        let mut winsize = libc::winsize {
            ws_row: size.rows,
            ws_col: size.cols,
            ws_xpixel: size.xpixel,
            ws_ypixel: size.ypixel,
        };

        let mut master_fd: libc::c_int = -1;
        let pid = unsafe {
            libc::forkpty(
                &mut master_fd,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut winsize,
            )
        };

        if pid < 0 {
            return Err(Error::SpawnFailed(
                std::io::Error::last_os_error().to_string(),
            ));
        }

        if pid == 0 {
            // ── Child process ─────────────────────────────────────────────
            // forkpty already wired up the controlling terminal + stdio.
            // IMPORTANT: After fork(), only async-signal-safe functions can be
            // called before exec(). Using std::process::exit() would trigger
            // destructors and library cleanup (like CoreSpotlight/PowerLog on
            // macOS) which causes crashes because dispatch queues are broken
            // after fork.

            // Set environment variables using libc (async-signal-safe)
            unsafe {
                libc::setenv(b"TERM\0".as_ptr() as *const i8, b"xterm-256color\0".as_ptr() as *const i8, 1);
                libc::setenv(b"LANG\0".as_ptr() as *const i8, b"en_US.UTF-8\0".as_ptr() as *const i8, 1);
                libc::setenv(b"LC_ALL\0".as_ptr() as *const i8, b"en_US.UTF-8\0".as_ptr() as *const i8, 1);

                // Default to the user's home directory so new terminals open at ~
                // instead of the app's working directory. getenv/chdir are
                // async-signal-safe (allowed between fork and exec).
                let home = libc::getenv(b"HOME\0".as_ptr() as *const i8);
                if !home.is_null() {
                    libc::chdir(home);
                }
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

        // ── Parent process ────────────────────────────────────────────────
        // forkpty handed us the master fd directly. dup it so the File we
        // store owns an independent descriptor (and closing our copy won't
        // tear down the kernel's master while PtyWriter/session hold dupes).
        let fd_dup = unsafe { libc::dup(master_fd) };
        if fd_dup < 0 {
            unsafe { libc::close(master_fd) };
            return Err(Error::SpawnFailed("Failed to dup master fd".to_string()));
        }
        let master_file = unsafe { File::from_raw_fd(fd_dup) };

        Ok(Self {
            master: master_file,
            child_pid: pid,
            alive: Arc::new(AtomicBool::new(true)),
        })
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
            // WouldBlock: no data available right now (non-blocking fd).
            // Interrupted: read() was cut off by a signal (e.g. SIGCHLD when
            // the shell forks a child to run a command). Both are transient —
            // treat them as "no data yet" and retry on the next poll.
            // Surfacing either as an error would let a stray signal or a
            // momentarily-busy PTY permanently kill a perfectly alive shell.
            Err(ref e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::Interrupted =>
            {
                Ok(Vec::new())
            }
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

        // Use waitpid(WNOHANG) instead of kill(0, 0): kill(0) reports
        // a zombie (<defunct>) as alive, which causes the terminal to
        // hang since nobody is reading from the dead PTY child.
        // waitpid(WNOHANG) reaps a zombie immediately and returns the
        // correct status in one syscall.
        unsafe {
            let mut status: i32 = 0;
            let result = libc::waitpid(self.child_pid, &mut status, libc::WNOHANG);
            if result == 0 {
                true // child still running
            } else {
                // result == child_pid  → zombie reaped
                // result == -1 (ECHILD) → already gone
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
                Err(Error::ReadFailed(format!("waitpid failed: {}", errno)))
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
            // Try SIGTERM first for graceful shutdown
            if libc::kill(self.child_pid, libc::SIGTERM) < 0 {
                let err = std::io::Error::last_os_error();
                // ESRCH = no such process, already dead
                if err.raw_os_error() == Some(libc::ESRCH) {
                    self.alive.store(false, Ordering::Relaxed);
                    return Ok(());
                }
                return Err(Error::SpawnFailed(format!("Failed to kill process: {}", err)));
            }

            // Wait up to 50ms for graceful exit
            let mut status: i32 = 0;
            for _ in 0..5 {
                let result = libc::waitpid(self.child_pid, &mut status, libc::WNOHANG);
                if result == self.child_pid {
                    self.alive.store(false, Ordering::Relaxed);
                    return Ok(());
                }
                if result < 0 {
                    // ECHILD = no child to wait for, already dead
                    if std::io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD) {
                        self.alive.store(false, Ordering::Relaxed);
                        return Ok(());
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }

            // Force kill with SIGKILL
            libc::kill(self.child_pid, libc::SIGKILL);
            self.alive.store(false, Ordering::Relaxed);
        }
        Ok(())
    }

    fn get_shell_name(&self) -> Option<String> {
        if !self.alive.load(Ordering::Relaxed) {
            return None;
        }

        // 1. Direct PTY child process name (ps first, then platform fallback).
        let direct_name =
            if let Some(name) = ps_comm(self.child_pid as u32) {
                Some(name)
            } else {
                // macOS: fall back to proc_pidpath if ps failed.
                #[cfg(target_os = "macos")]
                {
                    use std::path::PathBuf;
                    (|| {
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
                    })()
                }
                #[cfg(not(target_os = "macos"))]
                {
                    None
                }
            }?;

        // 2. Scan direct children for a nested known interactive shell.
        //    Handles "zsh → user ran `bash`" — the visible shell is the
        //    child, not the PTY's login shell.
        {
            use std::process::Command;
            const KNOWN: &[&str] = &["bash", "zsh", "fish", "ksh", "csh", "tcsh", "dash"];
            if let Ok(out) = Command::new("pgrep")
                .args(["-P", &self.child_pid.to_string()])
                .output()
            {
                if out.status.success() {
                    let mut best: Option<(u32, String)> = None;
                    for line in String::from_utf8_lossy(&out.stdout).lines() {
                        let cpid: u32 = match line.trim().parse() {
                            Ok(p) => p,
                            Err(_) => continue,
                        };
                        if let Some(comm) = ps_comm(cpid) {
                            if KNOWN.contains(&comm.as_str()) && comm != direct_name
                                && best.as_ref().is_none_or(|(pid, _)| cpid > *pid) {
                                    best = Some((cpid, comm));
                                }
                        }
                    }
                    if let Some((_, name)) = best {
                        return Some(name);
                    }
                }
            }
        }

        Some(direct_name)
    }
}

impl Drop for UnixPty {
    fn drop(&mut self) {
        // Always ensure child process is killed to prevent PTY leaks
        // This is critical because PTY devices are limited system resources
        unsafe {
            // Try graceful shutdown first
            if self.alive.load(Ordering::Relaxed) {
                libc::kill(self.child_pid, libc::SIGTERM);
                // Brief wait for graceful exit
                let mut status: i32 = 0;
                let start = std::time::Instant::now();
                while start.elapsed() < std::time::Duration::from_millis(50) {
                    if libc::waitpid(self.child_pid, &mut status, libc::WNOHANG) == self.child_pid {
                        self.alive.store(false, Ordering::Relaxed);
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }

            // Force kill if still alive
            libc::kill(self.child_pid, libc::SIGKILL);
            self.alive.store(false, Ordering::Relaxed);

            // Reap zombie (non-blocking, may fail if already reaped)
            let mut status: i32 = 0;
            libc::waitpid(self.child_pid, &mut status, libc::WNOHANG);
        }
    }
}

impl AsRawFd for UnixPty {
    fn as_raw_fd(&self) -> RawFd {
        self.master.as_raw_fd()
    }
}
