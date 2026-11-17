//! Windows PTY implementation using ConPTY (Windows 10 1809+)
//!
//! Mirrors the [`super::UnixPty`] contract: a poll-style `try_read()` fed by
//! an internal reader thread, a one-shot `alive` latch that is only flipped
//! after the child's exit is CONFIRMED via `WaitForSingleObject`, and the
//! same `Error` variants. The child runs inside a pseudoconsole owned by
//! conhost.exe, so it emits VT sequences just like a unix PTY — the VTE
//! parser sees the same byte stream.
//!
//! API notes (windows crate 0.62): every `HANDLE`-taking call below was
//! checked against the generated bindings — `CreatePseudoConsole` returns
//! `Result<HPCON>` directly, `ClosePseudoConsole` is fire-and-forget, and
//! `ReadFile`/`WriteFile` take `Option<&[u8]>` slices.

use super::{Error, Pty, PtySize, Result};
use std::os::windows::process::ExitStatusExt;
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows::Win32::System::Console::{
    ClosePseudoConsole, CreatePseudoConsole, ResizePseudoConsole, COORD, HPCON,
};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION,
    PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject, EXTENDED_STARTUPINFO_PRESENT, STARTUPINFOEXW, STARTUPINFOW,
};

/// Null-terminated UTF-16 encoding for `PCWSTR` parameters. The returned
/// buffer must outlive the call that borrows its pointer.
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Windows ConPTY PTY implementation.
pub struct WindowsPty {
    /// The pseudoconsole. `None` once it has been closed by [`Self::teardown`].
    hpc: Option<HPCON>,
    /// The shell process (kept to probe liveness + exit code).
    h_process: HANDLE,
    /// Parent-side input pipe write end (keystrokes in).
    conin: HANDLE,
    /// Parent-side output pipe read end (drained by the reader thread).
    conout: HANDLE,
    /// One-shot liveness latch — same invariant as unix: flipped `false` only
    /// after `WaitForSingleObject` confirms the process handle is signaled.
    pub alive: Arc<AtomicBool>,
    /// The shell we spawned (for `get_shell_name`).
    shell_path: String,
    /// Bytes drained off `conout` by the internal reader thread; `try_read()`
    /// takes them all. Never blocks, mirroring unix's non-blocking master.
    pending: Arc<Mutex<Vec<u8>>>,
}

impl Pty for WindowsPty {
    fn spawn(command: &str, args: &[&str], size: PtySize) -> Result<Self> {
        Self::spawn_in(command, args, size, None)
    }

    fn spawn_in(command: &str, args: &[&str], size: PtySize, cwd: Option<&str>) -> Result<Self> {
        // ── 1. Pipe pairs for the pseudoconsole's input/output ────────────
        let mut in_read = HANDLE::default();
        let mut in_write = HANDLE::default();
        let mut out_read = HANDLE::default();
        let mut out_write = HANDLE::default();
        unsafe {
            CreatePipe(&mut in_read, &mut in_write, None, 0).map_err(|e| {
                Error::SpawnFailed(format!("CreatePipe (input): {e}"))
            })?;
            CreatePipe(&mut out_read, &mut out_write, None, 0).map_err(|e| {
                // Don't leak the first pair if the second fails.
                let _ = CloseHandle(in_read);
                let _ = CloseHandle(in_write);
                Error::SpawnFailed(format!("CreatePipe (output): {e}"))
            })?;
        }

        // ── 2. The pseudoconsole ──────────────────────────────────────────
        let hpc = unsafe {
            CreatePseudoConsole(
                COORD { X: size.cols as i16, Y: size.rows as i16 },
                in_read,
                out_write,
                0,
            )
        }
        .map_err(|e| {
            unsafe {
                let _ = CloseHandle(in_read);
                let _ = CloseHandle(in_write);
                let _ = CloseHandle(out_read);
                let _ = CloseHandle(out_write);
            }
            Error::SpawnFailed(format!("CreatePseudoConsole: {e}"))
        })?;

        // The pseudoconsole owns the child ends now; the parent must never
        // touch them (leaking these breaks the reader thread's EOF).
        unsafe {
            let _ = CloseHandle(in_read);
            let _ = CloseHandle(out_write);
        }

        // ── 3. Proc-thread attribute list carrying the pseudoconsole ─────
        // Two-step init: the first call fails BY DESIGN with
        // ERROR_INSUFFICIENT_BUFFER and writes back the required size.
        let mut attr_size: usize = 0;
        let _ = unsafe { InitializeProcThreadAttributeList(None, 1, None, &mut attr_size) };
        if attr_size == 0 {
            unsafe { ClosePseudoConsole(hpc) };
            unsafe {
                let _ = CloseHandle(out_read);
            }
            return Err(Error::SpawnFailed(
                "InitializeProcThreadAttributeList: could not size attribute list".to_string(),
            ));
        }
        // u64 buffer keeps the pointer-alignment the list needs.
        let mut attr_buf = vec![0u64; attr_size.div_ceil(8)];
        let attr_list = LPPROC_THREAD_ATTRIBUTE_LIST(attr_buf.as_mut_ptr().cast());

        // ── 4. CreateProcessW inside the pseudoconsole ────────────────────
        // Fully-quoted command line (paths with spaces are the norm on
        // Windows); buffer stays alive through the call — CreateProcessW may
        // write into it.
        let cmdline_str = if args.is_empty() {
            format!("\"{command}\"")
        } else {
            let quoted_args: Vec<String> =
                args.iter().map(|a| format!("\"{}\"", a.replace('"', "\\\""))).collect();
            format!("\"{command}\" {}", quoted_args.join(" "))
        };
        let mut cmdline = to_wide(&cmdline_str);
        let cwd_wide = cwd.map(to_wide);

        let spawn = || -> windows::core::Result<PROCESS_INFORMATION> {
            unsafe {
                InitializeProcThreadAttributeList(Some(attr_list), 1, None, &mut attr_size)?;
                UpdateProcThreadAttribute(
                    attr_list,
                    0,
                    PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                    Some(&hpc as *const HPCON as *const core::ffi::c_void),
                    std::mem::size_of::<HPCON>(),
                    None,
                    None,
                )?;
                let mut siex = STARTUPINFOEXW {
                    StartupInfo: STARTUPINFOW {
                        cb: std::mem::size_of::<STARTUPINFOEXW>() as u32,
                        ..Default::default()
                    },
                    lpAttributeList: attr_list,
                };
                let mut pi = PROCESS_INFORMATION::default();
                let cwd_pcwstr = cwd_wide
                    .as_ref()
                    .map(|w| PCWSTR(w.as_ptr()))
                    .unwrap_or_else(PCWSTR::null);
                CreateProcessW(
                    PCWSTR::null(),
                    Some(PWSTR(cmdline.as_mut_ptr())),
                    None,
                    None,
                    false,
                    EXTENDED_STARTUPINFO_PRESENT,
                    None,
                    cwd_pcwstr,
                    &siex.StartupInfo,
                    &mut pi,
                )?;
                Ok(pi)
            }
        };

        let spawned = spawn();

        // The attribute list must be deleted and its buffer freed regardless
        // of the outcome (the list is consumed by CreateProcessW).
        unsafe { DeleteProcThreadAttributeList(attr_list) };
        drop(attr_buf);

        let pi = spawned.map_err(|e| {
            unsafe { ClosePseudoConsole(hpc) };
            unsafe {
                let _ = CloseHandle(out_read);
            }
            Error::SpawnFailed(format!("CreateProcessW({command}): {e}"))
        })?;
        // We never wait on the primary thread directly; drop its handle.
        unsafe {
            let _ = CloseHandle(pi.hThread);
        }

        // ── 5. Internal reader thread ─────────────────────────────────────
        // Blocking ReadFile on the parent's output end; chunks land in
        // `pending` for try_read() to drain (the session reader loop polls it
        // exactly like the unix non-blocking master). Broken pipe / any error
        // after the conhost dies = EOF.
        let pending: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let pending_clone = Arc::clone(&pending);
        std::thread::Builder::new()
            .name("conpty-reader".to_string())
            .spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    let mut n: u32 = 0;
                    let res = unsafe {
                        ReadFile(out_read, Some(&mut buf), Some(&mut n as *mut u32), None)
                    };
                    match res {
                        Ok(()) if n > 0 => {
                            if let Ok(mut p) = pending_clone.lock() {
                                p.extend_from_slice(&buf[..n as usize]);
                            }
                        }
                        Ok(()) => break, // 0 bytes = EOF
                        Err(e) => {
                            log::debug!("[conpty] reader EOF: {e}");
                            break;
                        }
                    }
                }
            })
            .map_err(|e| {
                Error::SpawnFailed(format!("Failed to spawn ConPTY reader thread: {e}"))
            })?;

        Ok(Self {
            hpc: Some(hpc),
            h_process: pi.hProcess,
            conin: in_write,
            conout: out_read,
            alive: Arc::new(AtomicBool::new(true)),
            shell_path: command.to_string(),
            pending,
        })
    }

    fn write(&mut self, data: &[u8]) -> Result<()> {
        if !self.alive.load(Ordering::Relaxed) {
            return Err(Error::AlreadyClosed);
        }
        // Blocking write loop. Unlike unix there is no O_NONBLOCK here — a
        // full input pipe blocks this call until the shell reads again.
        // Accepted trade-off for v1: writes are keystroke-sized and the pipe
        // buffer is multiple KiB, so a healthy shell never wedges this path
        // (and a wedged shell is dead anyway; the UI closes the session).
        let mut total = 0usize;
        while total < data.len() {
            let mut n: u32 = 0;
            let res = unsafe {
                WriteFile(self.conin, Some(&data[total..]), Some(&mut n as *mut u32), None)
            };
            if let Err(e) = res {
                return Err(Error::WriteFailed(format!("WriteFile: {e}")));
            }
            if n == 0 {
                return Err(Error::WriteFailed(
                    "WriteFile wrote 0 bytes (pipe wedged)".to_string(),
                ));
            }
            total += n as usize;
        }
        Ok(())
    }

    fn try_read(&mut self) -> Result<Vec<u8>> {
        // Drain everything the reader thread buffered; an empty Vec means
        // "nothing yet" — the same semantics as unix's WouldBlock path.
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| Error::ReadFailed("pending buffer lock poisoned".to_string()))?;
        Ok(std::mem::take(&mut *pending))
    }

    fn resize(&mut self, size: PtySize) -> Result<()> {
        if !self.alive.load(Ordering::Relaxed) {
            return Err(Error::AlreadyClosed);
        }
        let Some(hpc) = self.hpc else {
            return Err(Error::AlreadyClosed);
        };
        unsafe {
            ResizePseudoConsole(hpc, COORD { X: size.cols as i16, Y: size.rows as i16 })
        }
        .map_err(|e| Error::ResizeFailed(format!("ResizePseudoConsole: {e}")))
    }

    fn is_alive(&self) -> bool {
        if !self.alive.load(Ordering::Relaxed) {
            return false;
        }
        // WAIT_OBJECT_0 = the process handle is signaled = the child exited.
        // WAIT_TIMEOUT = still running. This is the exact analogue of unix's
        // waitpid(WNOHANG): it reaps (observes) the exit exactly once and
        // latches `alive` so `is_connected()` stays monotonic.
        let state = unsafe { WaitForSingleObject(self.h_process, 0) };
        if state == WAIT_OBJECT_0 {
            let mut code = 0u32;
            unsafe {
                let _ = GetExitCodeProcess(self.h_process, &mut code);
            }
            log::warn!(
                "[pty] child terminated (exit code {code}) — marking local session dead"
            );
            self.alive.store(false, Ordering::Relaxed);
            false
        } else {
            true
        }
    }

    fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        if !self.alive.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let state = unsafe { WaitForSingleObject(self.h_process, 0) };
        if state != WAIT_OBJECT_0 {
            return Ok(None);
        }
        let mut code = 0u32;
        unsafe { GetExitCodeProcess(self.h_process, &mut code) }
            .map_err(|e| Error::ReadFailed(format!("GetExitCodeProcess: {e}")))?;
        self.alive.store(false, Ordering::Relaxed);
        Ok(Some(ExitStatus::from_raw(code)))
    }

    fn kill(&mut self) -> Result<()> {
        self.teardown();
        Ok(())
    }

    fn get_shell_name(&self) -> Option<String> {
        std::path::Path::new(&self.shell_path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
    }
}

impl WindowsPty {
    /// Full teardown: prefer the child's natural exit for a moment, force
    /// terminate if needed, then close the pseudoconsole and pipes.
    ///
    /// ORDER MATTERS: `ClosePseudoConsole` can deadlock if conhost is blocked
    /// writing into a FULL output pipe that nobody drains — so the reader
    /// thread (still running until EOF) is deliberately kept alive through
    /// this call; it unblocks with EOF right after the console/pipe close.
    /// Idempotent: safe to call from both `kill()` and `Drop`.
    fn teardown(&mut self) {
        if self.alive.swap(false, Ordering::Relaxed) {
            unsafe {
                // Grace: 200ms for a natural exit before force-terminating.
                WaitForSingleObject(self.h_process, 200);
                let _ = TerminateProcess(self.h_process, 1);
            }
        }
        if let Some(hpc) = self.hpc.take() {
            unsafe { ClosePseudoConsole(hpc) };
        }
        unsafe {
            if !self.conin.is_invalid() {
                let _ = CloseHandle(self.conin);
                self.conin = HANDLE::default();
            }
            if !self.conout.is_invalid() {
                let _ = CloseHandle(self.conout);
                self.conout = HANDLE::default();
            }
            // Reap: wait briefly so the exit code is settled before dropping
            // the process handle.
            WaitForSingleObject(self.h_process, 1000);
            if !self.h_process.is_invalid() {
                let _ = CloseHandle(self.h_process);
                self.h_process = HANDLE::default();
            }
        }
    }
}

impl Drop for WindowsPty {
    fn drop(&mut self) {
        // Always ensure the child is gone and handles are closed — ConPTY
        // resources (and conhost.exe) are leaked otherwise.
        self.teardown();
    }
}
