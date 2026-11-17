//! Windows transport: one byte-mode named pipe per user (`\\.\pipe\portal-{user}`).
//!
//! Same shape as the unix socket: a blocking accept thread serves one
//! connection at a time, reading newline-delimited requests until the `end`
//! marker (a pipe cannot half-close the way a socket can, so the marker —
//! not EOF — is what ends a batch here), replying `ok\n`, then serving the
//! next connection from a fresh pipe instance.

use windows::core::{HRESULT, PCWSTR};
use windows::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, GetLastError, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED,
    HANDLE,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_SHARE_MODE, FILE_FLAGS_AND_ATTRIBUTES,
    FlushFileBuffers, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, WaitNamedPipeW, NAMED_PIPE_MODE,
    PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};

use super::{IpcRequest, RequestReader, RequestSink, SingleInstanceIpc};

pub(crate) struct NamedPipeIpc;

/// Per-user pipe name. Override with `PORTAL_IPC_PIPE` (used by tests).
fn pipe_name() -> String {
    if let Some(p) = std::env::var_os("PORTAL_IPC_PIPE") {
        return p.to_string_lossy().into_owned();
    }
    let user = std::env::var("USERNAME").unwrap_or_else(|_| "default".to_string());
    // Pipe names tolerate most characters, but normalize anything odd so a
    // display name with spaces or a domain suffix can't break the path.
    let safe: String = user
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '-' })
        .collect();
    format!(r"\\.\pipe\portal-{safe}")
}

/// UTF-16 with trailing null, for PCWSTR construction.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

impl SingleInstanceIpc for NamedPipeIpc {
    fn name(&self) -> &'static str {
        "named pipe"
    }

    fn listen(&self, sink: RequestSink) -> Result<(), String> {
        listen_at(&pipe_name(), sink)
    }

    fn forward(&self, payload: &str) -> bool {
        forward_at(&pipe_name(), payload)
    }

    /// The pipe object disappears when its last handle closes — nothing to remove.
    fn cleanup(&self) {}
}

fn listen_at(name: &str, sink: RequestSink) -> Result<(), String> {
    let name_w = wide(name);
    let name_pc = PCWSTR::from_raw(name_w.as_ptr());
    // FILE_FLAG_FIRST_PIPE_INSTANCE fails loudly when another Portal already
    // owns the channel — same role as the unix bind() failure.
    let first = create_instance(name_pc, true)?;
    // HANDLE is a raw pointer (!Send): carry the address across the thread
    // boundary and rebuild inside (same pattern as windows_pty).
    let addr = first.0 as usize;
    std::thread::Builder::new()
        .name("ipc-server".to_string())
        .spawn(move || {
            let name_pc = PCWSTR::from_raw(name_w.as_ptr());
            let mut instance = HANDLE(addr as *mut core::ffi::c_void);
            loop {
                match unsafe { ConnectNamedPipe(instance, None) } {
                    Ok(()) => {}
                    // The client raced in before ConnectNamedPipe — still connected.
                    Err(e) if e.code() == HRESULT::from_win32(ERROR_PIPE_CONNECTED.0) => {}
                    Err(e) => {
                        log::warn!("IPC: ConnectNamedPipe failed: {e}");
                        return;
                    }
                }
                let requests = read_requests(instance);
                let _ = write_all(instance, b"ok\n");
                unsafe {
                    let _ = FlushFileBuffers(instance);
                    let _ = DisconnectNamedPipe(instance);
                    let _ = CloseHandle(instance);
                }
                if !sink(requests) {
                    return; // UI is gone; stop accepting
                }
                // Serve the next connection from a fresh pipe instance.
                instance = match create_instance(name_pc, false) {
                    Ok(h) => h,
                    Err(e) => {
                        log::warn!("IPC: {e}");
                        return;
                    }
                };
            }
        })
        .map_err(|e| format!("spawn ipc-server: {e}"))?;
    Ok(())
}

fn create_instance(name_pc: PCWSTR, first: bool) -> Result<HANDLE, String> {
    let flags = if first {
        PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE
    } else {
        PIPE_ACCESS_DUPLEX
    };
    let handle = unsafe {
        CreateNamedPipeW(
            name_pc,
            flags,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            PIPE_UNLIMITED_INSTANCES,
            4096,
            4096,
            0,
            None,
        )
    };
    if handle.is_invalid() {
        Err(format!(
            "CreateNamedPipeW failed: {}",
            unsafe { GetLastError() }.0
        ))
    } else {
        Ok(handle)
    }
}

fn read_requests(pipe: HANDLE) -> Vec<IpcRequest> {
    let mut reader = RequestReader::new();
    let mut buf = [0u8; 4096];
    while !reader.ended() {
        let mut n = 0u32;
        // Blocking read; broken pipe (client hung up) or EOF ends the batch.
        let ok = unsafe { ReadFile(pipe, Some(&mut buf), Some(&mut n as *mut u32), None) };
        if ok.is_err() || n == 0 {
            break;
        }
        reader.feed(&buf[..n as usize]);
    }
    reader.finish()
}

/// Write the whole buffer; returns `false` on the first failed/zero write.
fn write_all(pipe: HANDLE, mut data: &[u8]) -> bool {
    while !data.is_empty() {
        let mut n = 0u32;
        let ok = unsafe { WriteFile(pipe, Some(data), Some(&mut n as *mut u32), None) };
        if ok.is_err() || n == 0 {
            return false;
        }
        data = &data[n as usize..];
    }
    true
}

fn forward_at(name: &str, payload: &str) -> bool {
    let name_w = wide(name);
    let name_pc = PCWSTR::from_raw(name_w.as_ptr());
    let Some(handle) = open_client(name_pc) else {
        return false;
    };
    if !write_all(handle, payload.as_bytes()) {
        unsafe {
            let _ = CloseHandle(handle);
        }
        return false;
    }
    // Read the reply until the server disconnects us (best effort): any
    // bytes back mean the instance consumed the payload.
    let mut buf = [0u8; 64];
    let mut n = 0u32;
    let read = unsafe { ReadFile(handle, Some(&mut buf), Some(&mut n as *mut u32), None) };
    unsafe {
        let _ = CloseHandle(handle);
    }
    read.is_ok() && n > 0
}

/// Connect as a client, retrying through `ERROR_PIPE_BUSY` (all instances
/// busy — another client is mid-request). `None` when nobody is listening.
fn open_client(name_pc: PCWSTR) -> Option<HANDLE> {
    for _ in 0..10 {
        let handle = unsafe {
            CreateFileW(
                name_pc,
                (GENERIC_READ | GENERIC_WRITE).0,
                FILE_SHARE_MODE(0),
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )
        };
        match handle {
            Ok(h) => return Some(h),
            Err(e) if e.code() == HRESULT::from_win32(ERROR_PIPE_BUSY.0) => {
                if !unsafe { WaitNamedPipeW(name_pc, 2000) }.as_bool() {
                    return None;
                }
            }
            Err(_) => return None,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::END_OF_REQUESTS;
    use super::*;
    use std::sync::{Arc, Mutex};

    /// End-to-end: server + forward via a unique test pipe.
    #[test]
    fn forward_reaches_sink() {
        let dir = tempfile::tempdir().unwrap();
        let name = format!(
            r"\\.\pipe\portal-test-{}",
            dir.path().file_name().unwrap().to_string_lossy()
        );

        let got: Arc<Mutex<Vec<IpcRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let sink: RequestSink = {
            let got = Arc::clone(&got);
            Arc::new(move |reqs| {
                got.lock().unwrap().extend(reqs);
                true
            })
        };
        listen_at(&name, sink).unwrap();

        let reqs = vec![
            IpcRequest::OpenTab(PathBuf::from("C:\\tmp")),
            IpcRequest::OpenWindow(PathBuf::from("C:\\usr")),
        ];
        let mut payload = String::new();
        for req in &reqs {
            payload.push_str(&req.encode());
        }
        payload.push_str(END_OF_REQUESTS);
        payload.push('\n');
        assert!(forward_at(&name, &payload));

        for _ in 0..100 {
            if got.lock().unwrap().len() >= 2 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(&*got.lock().unwrap(), &reqs, "forwarded requests should reach the sink verbatim");
    }

    /// An unread pipe is not a listener: forward must fail cleanly.
    #[test]
    fn forward_to_missing_pipe_returns_false() {
        let name = r"\\.\pipe\portal-test-definitely-not-listening";
        assert!(!forward_at(name, "tab\tC:\\tmp\nend\n"));
    }
}
