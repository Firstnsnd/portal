//! Single-instance IPC for "open terminal here" requests.
//!
//! Shell/file-manager integrations (Finder Quick Actions, Nautilus scripts,
//! Dolphin service menus, Explorer context menus) invoke the portal binary
//! with `--open-tab` / `--open-window` plus a directory. If a Portal instance
//! is already running, the new process forwards the request over the platform
//! channel (per-user Unix socket / named pipe) and exits before creating any
//! window; the running instance opens the tab/window and focuses itself. If
//! nobody is listening, the process becomes the instance: it binds the
//! channel and applies the requests once the UI is up.
//!
//! Wire protocol: newline-terminated request lines followed by an `end`
//! marker line. A named pipe cannot half-close the way a socket can, so the
//! marker is what ends a batch there; the socket side also accepts EOF from
//! clients that predate the marker.
//!
//! This module owns the shared parts — request type, CLI parsing, delivery
//! to the UI — and defines [`SingleInstanceIpc`]; each OS ships its own
//! transport in a sibling module (`unix`: socket, `windows`: named pipe).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};

#[cfg(unix)]
pub(crate) mod unix;
#[cfg(windows)]
pub(crate) mod windows;

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

/// Line that terminates a forwarded batch of requests.
const END_OF_REQUESTS: &str = "end";

/// Incremental parser for the request wire protocol. Feed raw bytes as they
/// arrive; [`RequestReader::ended`] flips once the terminator (or, on the
/// socket transport, an EOF) is seen.
pub(crate) struct RequestReader {
    buf: String,
    ended: bool,
    requests: Vec<IpcRequest>,
}

impl RequestReader {
    pub(crate) fn new() -> Self {
        Self {
            buf: String::new(),
            ended: false,
            requests: Vec::new(),
        }
    }

    /// Feed raw bytes; complete lines are decoded into the request buffer.
    pub(crate) fn feed(&mut self, bytes: &[u8]) {
        self.buf.push_str(&String::from_utf8_lossy(bytes));
        while let Some(nl) = self.buf.find('\n') {
            let line: String = self.buf.drain(..=nl).collect();
            let line = line.trim_end();
            if line == END_OF_REQUESTS {
                self.ended = true;
                return;
            }
            if let Some(req) = IpcRequest::decode(line) {
                self.requests.push(req);
            }
        }
    }

    pub(crate) fn ended(&self) -> bool {
        self.ended
    }

    /// Finish the batch. A transport end without the marker (old socket
    /// clients) still yields a trailing newline-less line, matching the
    /// original read-until-EOF behavior.
    pub(crate) fn finish(mut self) -> Vec<IpcRequest> {
        let tail = self.buf.trim_end().to_string();
        if !self.ended && !tail.is_empty() && tail != END_OF_REQUESTS {
            if let Some(req) = IpcRequest::decode(&tail) {
                self.requests.push(req);
            }
        }
        self.requests
    }
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

/// Callback for requests received by the server: returns `false` when the
/// UI is gone and the accept loop should stop.
pub(crate) type RequestSink = Arc<dyn Fn(Vec<IpcRequest>) -> bool + Send + Sync>;

/// Platform channel for handing requests to an already-running instance.
pub(crate) trait SingleInstanceIpc: Send + Sync {
    /// Backend display name for log lines.
    fn name(&self) -> &'static str;
    /// Bind the channel and accept requests until the sink returns `false`.
    /// Spawns its own accept thread.
    fn listen(&self, sink: RequestSink) -> Result<(), String>;
    /// Hand a payload (request lines + `end` marker) to the running
    /// instance. Returns `true` when a listener accepted it.
    fn forward(&self, payload: &str) -> bool;
    /// Best-effort channel removal on shutdown.
    fn cleanup(&self);
}

fn platform_ipc() -> &'static dyn SingleInstanceIpc {
    #[cfg(unix)]
    {
        &unix::SocketIpc
    }
    #[cfg(windows)]
    {
        &windows::NamedPipeIpc
    }
}

static RECEIVER: OnceLock<Mutex<Receiver<IpcRequest>>> = OnceLock::new();
static INITIAL: Mutex<Vec<IpcRequest>> = Mutex::new(Vec::new());

/// Stash requests gathered at startup (no running instance was found); the UI
/// applies them on its first frames via [`drain_requests`].
pub fn set_initial(requests: Vec<IpcRequest>) {
    *INITIAL.lock().unwrap() = requests;
}

/// Bind the platform IPC channel and start the accept loop.
pub fn start_server() {
    let (tx, rx) = mpsc::channel();
    let _ = RECEIVER.set(Mutex::new(rx));
    let sink: RequestSink = Arc::new(move |requests| {
        let mut ui_alive = true;
        for req in requests {
            if tx.send(req).is_err() {
                ui_alive = false;
                break;
            }
        }
        if ui_alive {
            crate::repaint::notify_repaint();
        }
        ui_alive
    });
    if let Err(e) = platform_ipc().listen(sink) {
        log::warn!("IPC: failed to bind {}: {e}", platform_ipc().name());
    }
}

/// Try to hand `requests` to an already-running instance. Returns `true` when
/// a listener accepted the payload — the caller should then exit.
pub fn forward(requests: &[IpcRequest]) -> bool {
    let mut payload = String::new();
    for req in requests {
        payload.push_str(&req.encode());
    }
    payload.push_str(END_OF_REQUESTS);
    payload.push('\n');
    platform_ipc().forward(&payload)
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

/// Remove the channel on app shutdown (best effort; the Unix socket is a
/// file, the Windows pipe evaporates with its handles).
pub fn cleanup() {
    platform_ipc().cleanup();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_flags_and_values() {
        let reqs =
            parse_cli_requests(strs(&["--open-tab", "/tmp", "--open-window", "/etc"]).into_iter());
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
        let reqs =
            parse_cli_requests(strs(&["--open-tab", "/tmp", "/definitely/not/a/dir"]).into_iter());
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

    #[test]
    fn request_reader_collects_batch_and_stops_at_marker() {
        let mut r = RequestReader::new();
        r.feed(b"tab\t/a\n");
        r.feed(b"wind");
        r.feed(b"ow\t/b\nend\ntrailing-noise");
        assert!(r.ended());
        // Everything after the marker is ignored.
        assert_eq!(
            r.finish(),
            vec![
                IpcRequest::OpenTab(PathBuf::from("/a")),
                IpcRequest::OpenWindow(PathBuf::from("/b")),
            ]
        );
    }

    #[test]
    fn request_reader_accepts_markerless_eof_batch() {
        // Old clients (pre-marker) just closed the connection.
        let mut r = RequestReader::new();
        r.feed(b"tab\t/a\nwindow\t/b");
        assert!(!r.ended());
        assert_eq!(
            r.finish(),
            vec![
                IpcRequest::OpenTab(PathBuf::from("/a")),
                IpcRequest::OpenWindow(PathBuf::from("/b")),
            ]
        );
    }
}
