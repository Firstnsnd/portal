//! Unix transport: per-user Unix-domain socket.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use super::{IpcRequest, RequestReader, RequestSink, SingleInstanceIpc};

pub(crate) struct SocketIpc;

/// Per-user socket path. Override with `PORTAL_IPC_SOCK` (used by tests).
pub(crate) fn socket_path() -> PathBuf {
    if let Some(p) = std::env::var_os("PORTAL_IPC_SOCK") {
        return PathBuf::from(p);
    }
    let uid = unsafe { libc::getuid() };
    std::env::temp_dir().join(format!("portal-{uid}.sock"))
}

impl SingleInstanceIpc for SocketIpc {
    fn name(&self) -> &'static str {
        "unix socket"
    }

    fn listen(&self, sink: RequestSink) -> Result<(), String> {
        listen_at(socket_path(), sink)
    }

    fn forward(&self, payload: &str) -> bool {
        forward_at(&socket_path(), payload)
    }

    fn cleanup(&self) {
        let _ = std::fs::remove_file(socket_path());
    }
}

fn listen_at(path: PathBuf, sink: RequestSink) -> Result<(), String> {
    // Remove a stale socket left behind by a crashed instance. If a live
    // instance is listening, bind() below fails and we simply report it —
    // two instances shouldn't happen, but don't crash over it.
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).map_err(|e| format!("{}: {}", path.display(), e))?;
    // Owner-only socket: on macOS $TMPDIR is already per-user, but on
    // Linux a shared /tmp would otherwise let any local user connect.
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    std::thread::Builder::new()
        .name("ipc-server".to_string())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                // A client may batch several requests in one connection;
                // read until the end marker or the client's EOF.
                let requests = read_requests(&stream);
                let _ = reply_ok(&stream);
                if !sink(requests) {
                    break; // UI is gone; stop accepting
                }
            }
        })
        .map_err(|e| format!("spawn ipc-server: {e}"))?;
    Ok(())
}

fn read_requests(mut stream: &UnixStream) -> Vec<IpcRequest> {
    let mut reader = RequestReader::new();
    let mut buf = [0u8; 4096];
    while !reader.ended() {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break, // EOF or error: connection is done
            Ok(n) => reader.feed(&buf[..n]),
        }
    }
    reader.finish()
}

fn reply_ok(stream: &UnixStream) -> std::io::Result<()> {
    let mut w = stream.try_clone()?;
    w.write_all(b"ok\n")
}

fn forward_at(path: &Path, payload: &str) -> bool {
    let Ok(mut stream) = UnixStream::connect(path) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
    if stream.write_all(payload.as_bytes()).is_err() {
        return false;
    }
    let _ = stream.shutdown(std::net::Shutdown::Write);
    // Drain the reply (best effort): "ok" means the instance consumed it.
    let mut reply = String::new();
    (&stream).read_to_string(&mut reply).is_ok()
}

#[cfg(test)]
mod tests {
    use super::super::END_OF_REQUESTS;
    use super::*;
    use std::sync::{Arc, Mutex};

    /// End-to-end: server + forward via a temp socket path.
    #[test]
    fn forward_reaches_sink() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("test.sock");

        let got: Arc<Mutex<Vec<IpcRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let sink: RequestSink = {
            let got = Arc::clone(&got);
            Arc::new(move |reqs| {
                got.lock().unwrap().extend(reqs);
                true
            })
        };
        listen_at(sock.clone(), sink).unwrap();

        let reqs = vec![
            IpcRequest::OpenTab(PathBuf::from("/tmp")),
            IpcRequest::OpenWindow(PathBuf::from("/usr")),
        ];
        let mut payload = String::new();
        for req in &reqs {
            payload.push_str(&req.encode());
        }
        payload.push_str(END_OF_REQUESTS);
        payload.push('\n');
        assert!(forward_at(&sock, &payload));

        // The accept thread delivers via the sink; accumulate until both
        // requests land (the batch may arrive in more than one read).
        for _ in 0..100 {
            if got.lock().unwrap().len() >= 2 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(&*got.lock().unwrap(), &reqs, "forwarded requests should reach the sink verbatim");
    }
}
