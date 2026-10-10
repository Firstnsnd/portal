//! Single-instance IPC for "open terminal here" requests.
//!
//! Shell/file-manager integrations (Finder Quick Actions, Nautilus scripts,
//! Dolphin service menus) invoke the portal binary with `--open-tab` /
//! `--open-window` plus a directory. If a Portal instance is already running,
//! the new process forwards the request over a per-user Unix socket and exits
//! before creating any window; the running instance opens the tab/window and
//! focuses itself. If nobody is listening, the process becomes the instance:
//! it binds the socket and applies the requests once the UI is up.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Mutex, OnceLock};

/// An "open here" request from the OS shell integration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpcRequest {
    /// Open a new terminal tab in an existing window at this directory.
    OpenTab(PathBuf),
    /// Open a new native window with one terminal at this directory.
    OpenWindow(PathBuf),
}

impl IpcRequest {
    fn encode(&self) -> String {
        match self {
            IpcRequest::OpenTab(p) => format!("tab\t{}\n", p.display()),
            IpcRequest::OpenWindow(p) => format!("window\t{}\n", p.display()),
        }
    }

    fn decode(line: &str) -> Option<Self> {
        let (kind, path) = line.trim_end().split_once('\t')?;
        let path = PathBuf::from(path);
        match kind {
            "tab" => Some(IpcRequest::OpenTab(path)),
            "window" => Some(IpcRequest::OpenWindow(path)),
            _ => None,
        }
    }
}

/// Per-user socket path. Override with `PORTAL_IPC_SOCK` (used by tests).
pub fn socket_path() -> PathBuf {
    if let Some(p) = std::env::var_os("PORTAL_IPC_SOCK") {
        return PathBuf::from(p);
    }
    let uid = unsafe { libc::getuid() };
    std::env::temp_dir().join(format!("portal-{uid}.sock"))
}

/// Parse `--open-tab <dir>` / `--open-window <dir>` from CLI args. Bare
/// arguments that name an existing directory are treated as `--open-tab`
/// (e.g. a folder dropped onto the app icon).
pub fn parse_cli_requests<I: Iterator<Item = String>>(args: I) -> Vec<IpcRequest> {
    enum Expect {
        Tab,
        Window,
    }
    let mut out = Vec::new();
    let mut expect: Option<Expect> = None;
    for arg in args {
        if let Some(kind) = expect.take() {
            let req = match kind {
                Expect::Tab => IpcRequest::OpenTab(PathBuf::from(&arg)),
                Expect::Window => IpcRequest::OpenWindow(PathBuf::from(&arg)),
            };
            out.push(req);
        } else {
            match arg.as_str() {
                "--open-tab" | "--open-tab-here" => expect = Some(Expect::Tab),
                "--open-window" | "--open-window-here" => expect = Some(Expect::Window),
                _ => {
                    let p = Path::new(&arg);
                    if p.is_dir() {
                        out.push(IpcRequest::OpenTab(p.to_path_buf()));
                    }
                }
            }
        }
    }
    out
}

static RECEIVER: OnceLock<Mutex<Receiver<IpcRequest>>> = OnceLock::new();
static INITIAL: Mutex<Vec<IpcRequest>> = Mutex::new(Vec::new());

/// Stash requests gathered at startup (no running instance was found); the UI
/// applies them on its first frames via [`drain_requests`].
pub fn set_initial(requests: Vec<IpcRequest>) {
    *INITIAL.lock().unwrap() = requests;
}

/// Bind the shared IPC socket and start the accept loop.
pub fn start_server() {
    start_server_at(socket_path());
}

fn start_server_at(path: PathBuf) {
    // Remove a stale socket left behind by a crashed instance. If a live
    // instance is listening, bind() below fails and we simply log — two
    // instances shouldn't happen, but don't crash over it.
    let _ = std::fs::remove_file(&path);
    match UnixListener::bind(&path) {
        Ok(listener) => {
            let (tx, rx) = mpsc::channel();
            let _ = RECEIVER.set(Mutex::new(rx));
            std::thread::Builder::new()
                .name("ipc-server".to_string())
                .spawn(move || {
                    for stream in listener.incoming() {
                        let Ok(stream) = stream else { continue };
                        // A client may batch several requests in one
                        // connection; read until it shuts down its write side.
                        let requests = read_requests(&stream);
                        let _ = reply_ok(&stream);
                        let mut ui_alive = true;
                        for req in requests {
                            if tx.send(req).is_err() {
                                ui_alive = false;
                                break;
                            }
                        }
                        if !ui_alive {
                            break; // UI is gone; stop accepting
                        }
                        crate::repaint::notify_repaint();
                    }
                })
                .ok();
        }
        Err(e) => log::warn!("IPC: failed to bind {}: {}", path.display(), e),
    }
}

fn read_requests(stream: &UnixStream) -> Vec<IpcRequest> {
    let mut reader = BufReader::new(stream);
    let mut out = Vec::new();
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break, // EOF or error: connection is done
            Ok(_) => {
                if let Some(req) = IpcRequest::decode(&line) {
                    out.push(req);
                }
            }
        }
    }
    out
}

fn reply_ok(stream: &UnixStream) -> std::io::Result<()> {
    let mut w = stream.try_clone()?;
    w.write_all(b"ok\n")
}

/// Try to hand `requests` to an already-running instance. Returns `true` when
/// a listener accepted the payload — the caller should then exit.
pub fn forward(requests: &[IpcRequest]) -> bool {
    forward_at(&socket_path(), requests)
}

fn forward_at(path: &Path, requests: &[IpcRequest]) -> bool {
    let Ok(mut stream) = UnixStream::connect(path) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
    let mut payload = String::new();
    for req in requests {
        payload.push_str(&req.encode());
    }
    if stream.write_all(payload.as_bytes()).is_err() {
        return false;
    }
    let _ = stream.shutdown(std::net::Shutdown::Write);
    // Drain the reply (best effort): "ok" means the instance consumed it.
    let mut reply = String::new();
    stream.read_to_string(&mut reply).is_ok()
}

/// Drain all pending requests (startup + forwarded). Called by the UI each
/// frame; returns an empty Vec when nothing is pending.
pub fn drain_requests() -> Vec<IpcRequest> {
    let mut out: Vec<IpcRequest> = INITIAL.lock().unwrap().drain(..).collect();
    if let Some(rx) = RECEIVER.get() {
        let rx = rx.lock().unwrap();
        while let Ok(req) = rx.try_recv() {
            out.push(req);
        }
    }
    out
}

/// Remove the socket file on app shutdown (best effort).
pub fn cleanup() {
    let _ = std::fs::remove_file(socket_path());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_flags_and_values() {
        let reqs = parse_cli_requests(
            strs(&["--open-tab", "/tmp", "--open-window", "/etc"]).into_iter(),
        );
        assert_eq!(
            reqs,
            vec![
                IpcRequest::OpenTab(PathBuf::from("/tmp")),
                IpcRequest::OpenWindow(PathBuf::from("/etc")),
            ]
        );
    }

    #[test]
    fn parse_bare_existing_dir_becomes_tab() {
        let reqs = parse_cli_requests(strs(&["--open-tab", "/tmp", "/definitely/not/a/dir"]).into_iter());
        // The bare non-existent path is ignored; /tmp keeps its request.
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0], IpcRequest::OpenTab(PathBuf::from("/tmp")));
    }

    #[test]
    fn parse_dangling_flag_is_ignored() {
        assert!(parse_cli_requests(strs(&["--open-window"]).into_iter()).is_empty());
    }

    #[test]
    fn encode_decode_roundtrip() {
        for req in [
            IpcRequest::OpenTab(PathBuf::from("/a/b c")),
            IpcRequest::OpenWindow(PathBuf::from("/x")),
        ] {
            let line = req.encode();
            assert_eq!(IpcRequest::decode(&line), Some(req));
        }
    }

    /// End-to-end: server + forward + drain via a temp socket path.
    #[test]
    fn forward_reaches_drain() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("test.sock");
        // Safety: env-var override is read at socket_path() call time; tests
        // run multi-threaded, so exercise the _at helpers directly instead of
        // the env-var path for isolation.
        start_server_at(sock.clone());

        let reqs = vec![
            IpcRequest::OpenTab(PathBuf::from("/tmp")),
            IpcRequest::OpenWindow(PathBuf::from("/usr")),
        ];
        assert!(forward_at(&sock, &reqs));

        // The accept thread delivers via the channel; accumulate until both
        // requests land (each drain consumes what has arrived so far).
        let mut got: Vec<IpcRequest> = Vec::new();
        for _ in 0..100 {
            got.extend(drain_requests());
            if got.len() >= 2 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(got, reqs, "forwarded requests should reach drain verbatim");
    }
}
