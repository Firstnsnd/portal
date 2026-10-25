//! ZMODEM (rz/sz) transfer bridge.
//!
//! Built on the `rzsz` crate (exact-pinned: lrzsz-semantics port, single
//! maintainer — upgrades must be reviewed manually). The crate exposes a
//! blocking std::io engine over a `Read + AsFd` stream; this module adapts
//! it to a terminal session:
//!
//! ```text
//! session output ──ZmodemTap──▶ ZmodemBridge ──sync_channel──▶ pump ──▶ UnixStream
//!   (drain loop)     detector       feed()        (bounded)               │
//!                                                                        ▼
//! transfer thread: rzsz engine ◀── ModemReader ◀─────────────────────────┘
//!        │
//!        └── SessionWriter ──▶ session input (pty master / SSH channel)
//! ```
//!
//! Two entry points:
//! - Upload: arm the tap (divert everything from now on) and spawn the
//!   bridge; the engine's `get_receiver_init` types `rz\r` into the session
//!   itself, then runs the ZRQINIT → ZRINIT handshake.
//! - Download: create a passive tap + bridge and write `sz <path>\r`; the
//!   tap detects the remote sz's ZRQINIT token (`**\x18B00`, cross-chunk
//!   safe) and the FIRST diverted byte starts the engine, so its ZRINIT
//!   burst meets an already-running sz instead of garbage-printing the
//!   idle shell.
//!
//! Progress v1: the bridge counts diverted input (download) / protocol
//! output (upload) into an AtomicU64 — rzsz exposes no per-chunk callback.
//!
//! Ordering invariant (tested): diverted bytes never reach the VTE parser
//! or the asciicast tap — `route_chunk` must be called before both.

use std::io::{self, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use rzsz::serial::reader::ModemReader;

/// ZRQINIT hex header prefix: ZPAD ZPAD ZDLE ZHEX type=0x00 (hex "00").
/// The remote `sz` announces itself with these bytes; we divert from here.
const ZRQINIT_TOKEN: &[u8; 6] = b"**\x18B00";

/// lrzsz abort sequence: CAN×10 + BS×10 (same as the crate's send_cancel).
const CANCEL_SEQ: [u8; 20] = [
    0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, //
    0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08,
];

/// Buffered chunks between the feeding session thread and the pump thread.
/// 64 × typical-chunk size is comfortably more than one link-delay window;
/// exhaustion means the transfer thread is wedged (see `feed`).
const CHANNEL_CHUNKS: usize = 64;

// ─────────────────────────────────────────────────────────────────────────────
// Transfer state visible to the UI
// ─────────────────────────────────────────────────────────────────────────────

/// Direction of an active transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Upload,
    Download,
}

/// Lifecycle of a ZMODEM transfer. Held in the session's transfer slot
/// (mirrors the asciicast recorder slot pattern); the UI renders it as a
/// status-bar chip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZmodemState {
    /// Bridge started, waiting for the peer's first frame.
    Handshaking,
    /// Handshake complete; data flowing (`file` is empty for downloads —
    /// rzsz only names files when `receive_files` returns).
    Transferring { file: String },
    /// Data phase done, ZFIN handshake in flight.
    Finishing,
    Done { files: Vec<String>, bytes: u64 },
    Failed(String),
    Cancelled,
}

/// The slot payload: what is transferring and where it got to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZmodemTransfer {
    pub direction: Direction,
    pub state: ZmodemState,
}

impl ZmodemTransfer {
    /// Handshake/data phase — the peer is still busy and cancel makes sense.
    pub fn is_active(&self) -> bool {
        matches!(
            self.state,
            ZmodemState::Handshaking | ZmodemState::Transferring { .. } | ZmodemState::Finishing
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ZmodemTap — output-stream detector
// ─────────────────────────────────────────────────────────────────────────────

/// What the caller must do with the chunk it just fed to the tap.
///
/// Variants that leave the chunk "unhandled" let the caller emit it on the
/// normal path (VTE parse + asciicast); `Divert` consumes the chunk fully
/// and carries the protocol bytes for the bridge.
#[derive(Debug)]
pub enum TapAction<'a> {
    /// Emit this prefix slice of the input normally; the suffix was held
    /// back as a possible partial token (decided on the next chunk).
    Hold(&'a [u8]),
    /// Previously held bytes proved to be plain text: emit them joined with
    /// the current chunk (minus the newly held tail). The current chunk rides
    /// inside this buffer — the caller must NOT emit it again.
    Release(Vec<u8>),
    /// ZMODEM detected: emit `vte` (text before the token) normally and
    /// send `divert` to the bridge; the input chunk is fully consumed.
    Divert { vte: Vec<u8>, divert: Vec<u8> },
    /// Tap is armed (upload) or already active: the caller sends ALL input
    /// to the bridge — zero-copy fast path for every chunk of a transfer.
    ActiveDivert,
}

/// Watches session output for a remote `sz` announcing itself, and once
/// triggered marks the stream as protocol so everything diverts.
///
/// Cheap on the hot path: an inactive tap with an empty hold does a plain
/// substring scan and returns `Hold(&chunk)` — no allocation.
pub struct ZmodemTap {
    /// Trailing bytes that could be the start of [`ZRQINIT_TOKEN`] but the
    /// chunk ended first (at most `TOKEN.len() - 1` bytes).
    hold: Vec<u8>,
    active: bool,
}

impl Default for ZmodemTap {
    fn default() -> Self {
        Self::new()
    }
}

impl ZmodemTap {
    pub fn new() -> Self {
        Self {
            hold: Vec::new(),
            active: false,
        }
    }

    /// Upload mode: divert everything from now on. Returns bytes held back
    /// by a previous chunk — the caller must divert them first.
    pub fn arm(&mut self) -> Vec<u8> {
        self.active = true;
        std::mem::take(&mut self.hold)
    }

    /// Test observation only — the divert path reads `active` internally.
    #[cfg(test)]
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Back to plain-terminal mode (cancel / bridge died). The next ZRQINIT
    /// token re-triggers detection.
    pub fn reset(&mut self) {
        self.hold.clear();
        self.active = false;
    }

    pub fn feed<'a>(&mut self, data: &'a [u8]) -> TapAction<'a> {
        if self.active {
            return TapAction::ActiveDivert;
        }
        if data.is_empty() {
            return TapAction::Hold(data);
        }

        if self.hold.is_empty() {
            if let Some(i) = find(data, ZRQINIT_TOKEN) {
                self.active = true;
                return TapAction::Divert {
                    vte: data[..i].to_vec(),
                    divert: data[i..].to_vec(),
                };
            }
            let keep = longest_token_prefix_suffix(data);
            if keep == 0 {
                return TapAction::Hold(data);
            }
            self.hold = data[data.len() - keep..].to_vec();
            return TapAction::Hold(&data[..data.len() - keep]);
        }

        // A previous chunk ended mid-token: scan the join. Only reachable
        // when the last chunk ended with `*`, `**`, … — rare, so the
        // allocation here is fine.
        let mut joined = std::mem::take(&mut self.hold);
        joined.extend_from_slice(data);
        if let Some(i) = find(&joined, ZRQINIT_TOKEN) {
            self.active = true;
            return TapAction::Divert {
                vte: joined[..i].to_vec(),
                divert: joined[i..].to_vec(),
            };
        }
        let keep = longest_token_prefix_suffix(&joined);
        let text = joined[..joined.len() - keep].to_vec();
        self.hold = joined[joined.len() - keep..].to_vec();
        TapAction::Release(text)
    }
}

/// Index of `needle` in `haystack`, if present.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Length of the longest suffix of `data` that is a proper prefix of
/// [`ZRQINIT_TOKEN`] (0..TOKEN.len()-1).
fn longest_token_prefix_suffix(data: &[u8]) -> usize {
    for len in (1..ZRQINIT_TOKEN.len()).rev() {
        if data.len() >= len && data[data.len() - len..] == ZRQINIT_TOKEN[..len] {
            return len;
        }
    }
    0
}

// ─────────────────────────────────────────────────────────────────────────────
// SessionWriter — the engine's output sink (protocol bytes into the session)
// ─────────────────────────────────────────────────────────────────────────────

/// Shared sink closure: bytes out to the session (protocol frames only).
type SinkFn = dyn Fn(&[u8]) -> io::Result<()> + Send + Sync;

/// `impl Write` adapter over a sink closure. Writes land in the TARGET
/// session only (pty master for local shells, the SSH channel stdin for
/// remote ones) — broadcast mode never sees protocol frames.
#[derive(Clone)]
pub struct SessionWriter {
    sink: Arc<SinkFn>,
}

impl SessionWriter {
    pub fn new(sink: impl Fn(&[u8]) -> io::Result<()> + Send + Sync + 'static) -> Self {
        Self {
            sink: Arc::new(sink),
        }
    }
}

impl Write for SessionWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (self.sink)(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Wraps a writer and counts everything that goes through it (upload byte
/// progress: file data dominates the protocol framing).
struct CountingWriter<W: Write> {
    inner: W,
    counter: Arc<AtomicU64>,
}

impl<W: Write> Write for CountingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write_all(buf)?;
        self.counter.fetch_add(buf.len() as u64, Ordering::Relaxed);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ZmodemBridge — transfer thread + its input path
// ─────────────────────────────────────────────────────────────────────────────

/// What the transfer thread should do once it starts.
pub enum TransferRequest {
    /// Send a local file into the session (the remote end runs `rz`).
    Upload { path: PathBuf },
    /// Receive files into `output_dir` (the remote end runs `sz`).
    Download { output_dir: PathBuf },
}

impl TransferRequest {
    fn direction(&self) -> Direction {
        match self {
            TransferRequest::Upload { .. } => Direction::Upload,
            TransferRequest::Download { .. } => Direction::Download,
        }
    }
}

/// Owns the transfer thread for one ZMODEM transfer. `feed` carries diverted
/// session output into the engine; `cancel` aborts (CAN sequence to the peer,
/// EOF to the engine). Dropping the bridge cancels it.
pub struct ZmodemBridge {
    feed_tx: Option<std::sync::mpsc::SyncSender<Vec<u8>>>,
    started_tx: Option<std::sync::mpsc::Sender<()>>,
    counter: Arc<AtomicU64>,
    count_input: bool,
    cancel: Arc<AtomicBool>,
    wedged: Arc<AtomicBool>,
    /// Detached on cancel/drop — joining from the UI thread could block for
    /// a protocol timeout.
    _handle: Option<JoinHandle<()>>,
}

impl ZmodemBridge {
    /// Spawn the transfer thread. `slot` is initialised to `Handshaking`
    /// and updated by the thread as the transfer progresses; the UI reads
    /// it every frame for the status chip.
    pub fn spawn(
        request: TransferRequest,
        sink: SessionWriter,
        slot: Arc<Mutex<Option<ZmodemTransfer>>>,
    ) -> ZmodemBridge {
        let direction = request.direction();
        let (mut stream_tx, stream_rx) = match UnixStream::pair() {
            Ok(pair) => pair,
            Err(e) => {
                set_transfer_state(
                    &slot,
                    ZmodemTransfer {
                        direction,
                        state: ZmodemState::Failed(format!("bridge setup failed: {e}")),
                    },
                );
                return inert_bridge(direction);
            }
        };
        let (feed_tx, feed_rx) = sync_channel::<Vec<u8>>(CHANNEL_CHUNKS);
        let (started_tx, started_rx) = std::sync::mpsc::channel::<()>();
        let counter = Arc::new(AtomicU64::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let wedged = Arc::new(AtomicBool::new(false));

        // Pump: buffered chunks → the modem stream the engine reads from.
        // Ends (EOF for the engine) when the last feed sender is dropped.
        std::thread::Builder::new()
            .name("zmodem-pump".to_string())
            .spawn(move || {
                for chunk in feed_rx {
                    if stream_tx.write_all(&chunk).is_err() {
                        break;
                    }
                }
            })
            .ok();

        set_transfer_state(
            &slot,
            ZmodemTransfer {
                direction,
                state: ZmodemState::Handshaking,
            },
        );
        // Reveal the transfer to the UI immediately.
        crate::repaint::notify_repaint();

        let wait_for_remote = matches!(request, TransferRequest::Download { .. });
        let count_input = wait_for_remote; // download progress = diverted input
        let counter_t = Arc::clone(&counter);
        let cancel_t = Arc::clone(&cancel);
        let wedged_t = Arc::clone(&wedged);
        let slot_t = Arc::clone(&slot);
        let mut sink_t = sink.clone();

        let handle = std::thread::Builder::new()
            .name("zmodem-transfer".to_string())
            .spawn(move || {
                let mut engine = rzsz::zmodem::Session::new();
                let mut reader = ModemReader::new(stream_rx, 8192);

                // Downloads start on the FIRST diverted byte (the remote sz
                // already runs then) so the ZRINIT burst doesn't garbage-print
                // an idle shell. Uploads start immediately — nothing diverts
                // until the engine has typed `rz\r` and the peer answered.
                if wait_for_remote && started_rx.recv().is_err() {
                    set_transfer_state(
                        &slot_t,
                        ZmodemTransfer {
                            direction,
                            state: ZmodemState::Cancelled,
                        },
                    );
                    return;
                }

                let final_state = match request {
                    TransferRequest::Upload { path } => run_upload(
                        &mut engine,
                        &mut reader,
                        &sink_t,
                        &counter_t,
                        &slot_t,
                        path,
                        &cancel_t,
                        &wedged_t,
                    ),
                    TransferRequest::Download { output_dir } => run_download(
                        &mut engine,
                        &mut reader,
                        &sink_t,
                        &counter_t,
                        output_dir,
                        &cancel_t,
                        &wedged_t,
                    ),
                };
                set_transfer_state(
                    &slot_t,
                    ZmodemTransfer {
                        direction,
                        state: final_state,
                    },
                );
                if cancel_t.load(Ordering::Relaxed) {
                    // Tell the peer to abort; best-effort, the session may be
                    // going away.
                    let _ = sink_t.write_all(&CANCEL_SEQ);
                }
                crate::repaint::notify_repaint();
            })
            .ok();

        ZmodemBridge {
            feed_tx: Some(feed_tx),
            started_tx: Some(started_tx),
            counter,
            count_input,
            cancel,
            wedged,
            _handle: handle,
        }
    }

    /// Bytes transferred so far (diverted input for downloads, protocol
    /// output for uploads).
    pub fn bytes(&self) -> u64 {
        self.counter.load(Ordering::Relaxed)
    }

    /// Push diverted session output into the engine. Returns false when the
    /// bridge is dead (thread gone / wedged) — the caller must stop diverting
    /// and hand the stream back to the normal path.
    pub fn feed(&mut self, data: &[u8]) -> bool {
        let Some(tx) = &self.feed_tx else {
            return false;
        };
        if self.count_input && !data.is_empty() {
            self.counter.fetch_add(data.len() as u64, Ordering::Relaxed);
        }
        // First diverted byte starts a waiting download engine.
        if let Some(started) = self.started_tx.take() {
            let _ = started.send(());
        }
        match tx.try_send(data.to_vec()) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                // The bounded channel backed up past a link-delay window —
                // the transfer thread is not draining (wedged receiver).
                // Abort rather than let the engine see a corrupted stream.
                self.wedged.store(true, Ordering::Relaxed);
                self.cancel();
                false
            }
            Err(TrySendError::Disconnected(_)) => {
                self.feed_tx = None;
                false
            }
        }
    }

    /// Abort the transfer: flag the thread (it sends the CAN sequence and
    /// reports `Cancelled`), close the input path (engine sees EOF), detach.
    pub fn cancel(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.started_tx.take(); // a download that never saw sz just dies
        self.feed_tx.take();
        self._handle.take();
    }
}

impl Drop for ZmodemBridge {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// A bridge-shaped value with no threads behind it, for degenerate spawns.
fn inert_bridge(direction: Direction) -> ZmodemBridge {
    ZmodemBridge {
        feed_tx: None,
        started_tx: None,
        counter: Arc::new(AtomicU64::new(0)),
        count_input: matches!(direction, Direction::Download),
        cancel: Arc::new(AtomicBool::new(false)),
        wedged: Arc::new(AtomicBool::new(false)),
        _handle: None,
    }
}

fn set_transfer_state(slot: &Mutex<Option<ZmodemTransfer>>, transfer: ZmodemTransfer) {
    if let Ok(mut s) = slot.lock() {
        *s = Some(transfer);
    }
}

/// Map an engine error to a final chip state, honouring cancel/wedge flags.
fn map_engine_error(
    e: rzsz::zmodem::ZError,
    cancel: &AtomicBool,
    wedged: &AtomicBool,
) -> ZmodemState {
    if wedged.load(Ordering::Relaxed) {
        ZmodemState::Failed("transfer stalled".to_string())
    } else if cancel.load(Ordering::Relaxed) {
        ZmodemState::Cancelled
    } else {
        match e {
            rzsz::zmodem::ZError::Cancelled => ZmodemState::Cancelled,
            other => ZmodemState::Failed(other.to_string()),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_upload(
    engine: &mut rzsz::zmodem::Session,
    reader: &mut ModemReader<UnixStream>,
    sink: &SessionWriter,
    counter: &Arc<AtomicU64>,
    slot: &Mutex<Option<ZmodemTransfer>>,
    path: PathBuf,
    cancel: &AtomicBool,
    wedged: &AtomicBool,
) -> ZmodemState {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    // Types `rz\r` into the session itself, then ZRQINIT → wait ZRINIT.
    if let Err(e) = rzsz::sender::get_receiver_init(engine, reader, &mut sink.clone()) {
        return map_engine_error(e, cancel, wedged);
    }
    set_transfer_state(
        slot,
        ZmodemTransfer {
            direction: Direction::Upload,
            state: ZmodemState::Transferring {
                file: file_name.clone(),
            },
        },
    );

    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let mut counting = CountingWriter {
        inner: sink.clone(),
        counter: Arc::clone(counter),
    };
    let cfg = rzsz::sender::SenderConfig {
        quiet: true,
        ..Default::default()
    };
    match rzsz::sender::send_file(engine, reader, &mut counting, &path, &cfg, 1, size, None) {
        Ok(_) => {
            set_transfer_state(
                slot,
                ZmodemTransfer {
                    direction: Direction::Upload,
                    state: ZmodemState::Finishing,
                },
            );
            let _ = rzsz::sender::finish_session(engine, reader, &mut sink.clone());
            ZmodemState::Done {
                files: vec![file_name],
                bytes: counter.load(Ordering::Relaxed),
            }
        }
        Err(e) => map_engine_error(e, cancel, wedged),
    }
}

fn run_download(
    engine: &mut rzsz::zmodem::Session,
    reader: &mut ModemReader<UnixStream>,
    sink: &SessionWriter,
    counter: &Arc<AtomicU64>,
    output_dir: PathBuf,
    cancel: &AtomicBool,
    wedged: &AtomicBool,
) -> ZmodemState {
    // Diff snapshot: rzsz 0.1.4 ends a clean batch (sender's ZFIN → ackbibi)
    // as `Err(Cancelled)` and the error path drops the received-names list —
    // recover the names by diffing the output directory.
    let dir_before = dir_file_names(&output_dir);
    let cfg = rzsz::receiver::ReceiverConfig {
        output_dir,
        quiet: true,
        ..Default::default()
    };
    match rzsz::receiver::receive_files(engine, reader, &mut sink.clone(), &cfg) {
        Ok(files) => ZmodemState::Done {
            files,
            bytes: counter.load(Ordering::Relaxed),
        },
        Err(rzsz::zmodem::ZError::Cancelled)
            if !cancel.load(Ordering::Relaxed) =>
        {
            let files = dir_file_names(&cfg.output_dir)
                .into_iter()
                .filter(|name| !dir_before.contains(name))
                .collect();
            ZmodemState::Done {
                files,
                bytes: counter.load(Ordering::Relaxed),
            }
        }
        Err(e) => map_engine_error(e, cancel, wedged),
    }
}

/// File names directly inside `dir` (best-effort; unreadable dir → empty).
fn dir_file_names(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

// ─────────────────────────────────────────────────────────────────────────────
// Drain-loop integration
// ─────────────────────────────────────────────────────────────────────────────

/// Outcome of [`route_chunk`] for the caller's drain loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkRoute {
    /// The chunk (or its remaining bytes) belongs on the normal path.
    Normal,
    /// The chunk was consumed by the transfer — skip VTE parse AND the
    /// asciicast tap for it.
    Diverted,
}

/// Feed one output chunk through the tap and, when active, the bridge.
///
/// `emit` receives every byte that must still reach the normal path (VTE +
/// asciicast), in order. Call this BEFORE the VTE parse — binary protocol
/// frames must never reach scrollback or the recording.
pub fn route_chunk(
    tap: &Mutex<Option<ZmodemTap>>,
    bridge: &Mutex<Option<ZmodemBridge>>,
    data: &[u8],
    mut emit: impl FnMut(&[u8]),
) -> ChunkRoute {
    let action = tap
        .lock()
        .ok()
        .and_then(|mut guard| guard.as_mut().map(|t| t.feed(data)));
    match action {
        None => {
            // No tap (or poisoned): plain terminal output.
            emit(data);
            ChunkRoute::Normal
        }
        Some(TapAction::Hold(pass)) => {
            emit(pass);
            ChunkRoute::Normal
        }
        Some(TapAction::Release(lead)) => {
            // `lead` already carries the current chunk (joined minus the
            // newly held tail) — emitting `data` again would duplicate it.
            emit(&lead);
            ChunkRoute::Normal
        }
        Some(TapAction::Divert { vte, divert }) => {
            emit(&vte);
            feed_bridge(tap, bridge, &divert)
        }
        Some(TapAction::ActiveDivert) => feed_bridge(tap, bridge, data),
    }
}

fn feed_bridge(
    tap: &Mutex<Option<ZmodemTap>>,
    bridge: &Mutex<Option<ZmodemBridge>>,
    data: &[u8],
) -> ChunkRoute {
    let mut guard = match bridge.lock() {
        Ok(g) => g,
        Err(_) => return ChunkRoute::Diverted, // poisoned: swallow protocol bytes
    };
    let alive = match guard.as_mut() {
        Some(b) => b.feed(data),
        // Active tap with no bridge (should not happen — bridge is created
        // before the tap can trigger): swallow the protocol bytes.
        None => false,
    };
    if alive {
        return ChunkRoute::Diverted;
    }
    // Bridge is gone — hand the stream back to the terminal. The trailing
    // protocol bytes of this chunk are dropped (they are garbage on screen).
    *guard = None;
    if let Ok(mut t) = tap.lock() {
        if let Some(tap) = t.as_mut() {
            tap.reset();
        }
    }
    ChunkRoute::Diverted
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Single-quote shell escaping for a remote path (`sz '<path>'`).
pub fn shell_quote_single(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

// ─────────────────────────────────────────────────────────────────────────────
// Capability probe — is rz/sz actually available on the other end?
// ─────────────────────────────────────────────────────────────────────────────

/// ZMODEM support on the session's host — reuses the platform-neutral
/// [`crate::ssh::capability::HostCapability`] tri-state. Drives the
/// context-menu gating: only `Missing` disables the send/receive items.
pub type ZmodemSupport = crate::ssh::capability::HostCapability;

/// Local-shell ZMODEM support: a synchronous PATH scan for `sz` and `rz`
/// (a few stat calls — cheap enough to run at session creation).
pub fn local_zmodem_support() -> ZmodemSupport {
    let path = std::env::var("PATH").unwrap_or_default();
    if executable_in_path("sz", &path) && executable_in_path("rz", &path) {
        ZmodemSupport::Available
    } else {
        ZmodemSupport::Missing
    }
}

/// Whether an executable `name` exists in any `:`-separated PATH directory.
fn executable_in_path(name: &str, path_var: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::env::split_paths(path_var).any(|dir| {
        let candidate = dir.join(name);
        candidate
            .metadata()
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    // ── ZmodemTap ────────────────────────────────────────────────────────

    /// The token fed one byte at a time triggers exactly at the last byte,
    /// and everything after it diverts. Intermediate bytes emit nothing —
    /// every prefix of the token is still a viable hold, so nothing is
    /// released until the token completes.
    #[test]
    fn tap_detects_token_byte_by_byte() {
        let mut tap = ZmodemTap::new();
        for (i, b) in ZRQINIT_TOKEN.iter().enumerate() {
            let last = i + 1 == ZRQINIT_TOKEN.len();
            match tap.feed(&[*b]) {
                TapAction::Hold(passed) => {
                    assert!(!last, "byte {i}: the token must complete here");
                    assert!(passed.is_empty(), "byte {i}: nothing to emit");
                }
                TapAction::Release(lead) => {
                    assert!(!last, "byte {i}: the token must complete here");
                    assert!(lead.is_empty(), "byte {i}: nothing to emit");
                }
                TapAction::Divert { vte, divert } => {
                    assert!(last, "byte {i}: must not trigger early");
                    assert!(vte.is_empty());
                    assert_eq!(divert, ZRQINIT_TOKEN);
                }
                other => panic!("byte {i}: unexpected {other:?}"),
            }
        }
        assert!(tap.is_active());
        // Once active, everything diverts.
        assert!(matches!(
            tap.feed(b"deadbeef\r\n"),
            TapAction::ActiveDivert
        ));
    }

    /// Every possible split point between two chunks still detects.
    #[test]
    fn tap_detects_token_across_all_splits() {
        let rest = b"2b012345678\r\n\x11"; // header tail after the token
        for split in 0..ZRQINIT_TOKEN.len() {
            let mut tap = ZmodemTap::new();
            match tap.feed(&ZRQINIT_TOKEN[..split]) {
                TapAction::Hold(_) => {}
                other => panic!("split {split}: unexpected {other:?}"),
            }
            let mut chunk2 = ZRQINIT_TOKEN[split..].to_vec();
            chunk2.extend_from_slice(rest);
            match tap.feed(&chunk2) {
                TapAction::Divert { vte, divert } => {
                    assert!(vte.is_empty(), "split {split}");
                    let mut expected = ZRQINIT_TOKEN[..split].to_vec();
                    expected.extend_from_slice(&chunk2);
                    assert_eq!(divert, expected, "split {split}");
                }
                other => panic!("split {split}: unexpected {other:?}"),
            }
            assert!(tap.is_active());
        }
    }

    /// Token text preceded on the same chunk keeps the prefix on the normal
    /// path and only diverts from the token start.
    #[test]
    fn tap_diverts_from_token_start_only() {
        let mut tap = ZmodemTap::new();
        let chunk = b"user@host:~$ sz report.pdf\r\n**\x18B00deadbeef\r\n";
        match tap.feed(chunk) {
            TapAction::Divert { vte, divert } => {
                assert_eq!(vte, b"user@host:~$ sz report.pdf\r\n");
                assert!(divert.starts_with(ZRQINIT_TOKEN));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    /// A large ASCII corpus never triggers, and the hold never grows beyond
    /// the token length.
    #[test]
    fn tap_no_false_positive_on_ascii_corpus() {
        let mut tap = ZmodemTap::new();
        let mut line = Vec::new();
        for i in 0..2000 {
            line.extend_from_slice(
                format!("ls -la /tmp/dir{i}  # note ** not a token\r\n").as_bytes(),
            );
        }
        let mut fed = 0usize;
        for chunk in line.chunks(512) {
            match tap.feed(chunk) {
                // Release already contains the current chunk (joined with
                // the previously held bytes) — counting chunk.len() too
                // would double-count it.
                TapAction::Hold(pass) => fed += pass.len(),
                TapAction::Release(lead) => fed += lead.len(),
                other => panic!("false positive: {other:?}"),
            }
            assert!(tap.hold.len() < ZRQINIT_TOKEN.len());
        }
        // The `**\x18B00` token cannot appear in the corpus, so everything
        // eventually came back through the normal path.
        assert_eq!(fed, line.len());
    }

    /// Held bytes that turn out to be plain text are released in order.
    #[test]
    fn tap_releases_dead_hold() {
        let mut tap = ZmodemTap::new();
        // Ends with a live token prefix: `**` is held back.
        match tap.feed(b"stars **") {
            TapAction::Hold(pass) => assert_eq!(pass, b"stars "),
            other => panic!("unexpected {other:?}"),
        }
        // The held `**` proves to be plain text: released joined with the
        // new chunk (the current chunk rides along in `Release`).
        match tap.feed(b" and more\r\n") {
            TapAction::Release(lead) => assert_eq!(lead, b"** and more\r\n"),
            other => panic!("unexpected {other:?}"),
        }
    }

    /// Active tap diverts everything; reset returns to detection mode.
    #[test]
    fn tap_active_divert_and_reset_rearms() {
        let mut tap = ZmodemTap::new();
        assert!(tap.arm().is_empty());
        assert!(tap.is_active());
        assert!(matches!(tap.feed(b"anything"), TapAction::ActiveDivert));

        tap.reset();
        assert!(!tap.is_active());
        assert!(matches!(tap.feed(b"plain text"), TapAction::Hold(_)));

        // Re-arms on a fresh token.
        match tap.feed(&ZRQINIT_TOKEN[..]) {
            TapAction::Divert { divert, .. } => assert_eq!(divert, ZRQINIT_TOKEN),
            other => panic!("unexpected {other:?}"),
        }
    }

    // ── SessionWriter ────────────────────────────────────────────────────

    #[test]
    fn session_writer_concatenates_all_writes() {
        let collected: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let sink_ref = Arc::clone(&collected);
        let mut writer = SessionWriter::new(move |data| {
            sink_ref.lock().unwrap().extend_from_slice(data);
            Ok(())
        });
        writer.write_all(b"**\x18").unwrap();
        writer.write_all(b"B00").unwrap();
        writer.flush().unwrap();
        assert_eq!(&*collected.lock().unwrap(), b"**\x18B00");
    }

    // ── route_chunk ──────────────────────────────────────────────────────

    /// Diversion ordering: plain text reaches `emit`, protocol bytes never
    /// do — and once active, everything diverts.
    #[test]
    fn route_chunk_keeps_protocol_bytes_off_the_normal_path() {
        let tap = Mutex::new(Some(ZmodemTap::new()));
        // A bridge-shaped value with a live feed channel but no transfer
        // thread: feed() just buffers, which is all this test needs. The
        // receiver half must stay alive — a sync_channel with no receiver
        // reports Disconnected on the first try_send.
        let (feed_tx, _feed_rx) = sync_channel(16);
        let bridge = Mutex::new(Some(ZmodemBridge {
            feed_tx: Some(feed_tx),
            started_tx: None,
            counter: Arc::new(AtomicU64::new(0)),
            count_input: false,
            cancel: Arc::new(AtomicBool::new(false)),
            wedged: Arc::new(AtomicBool::new(false)),
            _handle: None,
        }));

        let mut normal = Vec::new();

        let route = route_chunk(&tap, &bridge, b"prompt$ ", |b| normal.extend_from_slice(b));
        assert_eq!(route, ChunkRoute::Normal);
        assert_eq!(normal, b"prompt$ ");

        let route = route_chunk(&tap, &bridge, b"**\x18B00rest\r\n", |b| {
            normal.extend_from_slice(b)
        });
        assert_eq!(route, ChunkRoute::Diverted);
        assert_eq!(normal, b"prompt$ ");

        // Steady state: everything diverts, nothing reaches the normal path.
        let route = route_chunk(&tap, &bridge, b"\x2a\x2a\x18\x42more", |b| {
            normal.extend_from_slice(b)
        });
        assert_eq!(route, ChunkRoute::Diverted);
        assert_eq!(normal, b"prompt$ ");
    }

    // ── shell quoting ────────────────────────────────────────────────────

    #[test]
    fn shell_quote_single_escapes_quotes() {
        assert_eq!(shell_quote_single("a b"), "'a b'");
        assert_eq!(shell_quote_single("it's"), r"'it'\''s'");
        assert_eq!(shell_quote_single(""), "''");
    }

    // ── Capability probe ─────────────────────────────────────────────────

    #[test]
    fn executable_in_path_finds_executables_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("sz");
        std::fs::write(&exe, b"#!/bin/sh\n").unwrap();
        let path_var = dir.path().to_str().unwrap();

        // Not executable yet: not found.
        assert!(!executable_in_path("sz", path_var));

        let mut perm = std::fs::metadata(&exe).unwrap().permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&exe, perm).unwrap();
        assert!(executable_in_path("sz", path_var));

        // Several directories in the var — any hit counts; a missing name
        // or a directory entry named like the binary does not.
        let other = tempfile::tempdir().unwrap();
        assert!(executable_in_path(
            "sz",
            &format!("{}:{}", other.path().display(), path_var)
        ));
        assert!(!executable_in_path("rz", path_var));
    }

    #[test]
    fn zmodem_support_u8_roundtrip() {
        for v in [
            ZmodemSupport::Unknown,
            ZmodemSupport::Available,
            ZmodemSupport::Missing,
        ] {
            assert_eq!(ZmodemSupport::from_u8(v.to_u8()), v);
        }
        // Out-of-range values degrade to Unknown, never to a wrong answer.
        assert_eq!(ZmodemSupport::from_u8(99), ZmodemSupport::Unknown);
    }

    // ── Bridge end-to-end (loopback, no lrzsz binary needed) ─────────────

    /// Full upload through a UnixStream pair: our bridge's engine is the
    /// sender; a plain rzsz receiver thread sits on the other end. Validates
    /// spawn/feed/state wiring AND the crate usage in one shot.
    #[test]
    fn bridge_uploads_file_through_loopback() {
        let src_dir = tempfile::tempdir().unwrap();
        let dst_dir = tempfile::tempdir().unwrap();
        let payload: Vec<u8> = (0..256_000u32)
            .map(|i| (i.wrapping_mul(2654435761) >> 24) as u8)
            .collect();
        let src_path = src_dir.path().join("payload.bin");
        std::fs::write(&src_path, &payload).unwrap();

        let (ours, theirs) = UnixStream::pair().unwrap();

        // Receiver side (acts as the remote `rz`).
        let recv_dir = dst_dir.path().to_path_buf();
        let recv_stream = theirs.try_clone().unwrap();
        let receiver = std::thread::spawn(move || {
            let mut engine = rzsz::zmodem::Session::new();
            let mut reader = ModemReader::new(theirs, 8192);
            let mut out = recv_stream;
            let cfg = rzsz::receiver::ReceiverConfig {
                output_dir: recv_dir,
                quiet: true,
                ..Default::default()
            };
            rzsz::receiver::receive_files(&mut engine, &mut reader, &mut out, &cfg)
        });

        // Our side: bridge (engine = sender) whose sink writes into the pair,
        // pumped back in from the other half. The bridge lives behind an Arc
        // so the pump thread can drive feed() while we watch the slot.
        let slot: Arc<Mutex<Option<ZmodemTransfer>>> = Arc::new(Mutex::new(None));
        let write_half = Mutex::new(ours.try_clone().unwrap());
        let sink = SessionWriter::new(move |data| {
            write_half.lock().unwrap().write_all(data).map(|_| ())
        });
        let bridge = Arc::new(Mutex::new(ZmodemBridge::spawn(
            TransferRequest::Upload {
                path: src_path.clone(),
            },
            sink,
            Arc::clone(&slot),
        )));

        // Pump: whatever comes back from the receiver → bridge.
        let mut pump_stream = ours;
        let pump_bridge = Arc::clone(&bridge);
        let pump = std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match pump_stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let mut b = pump_bridge.lock().unwrap();
                        if !b.feed(&buf[..n]) {
                            break;
                        }
                    }
                }
            }
        });

        // Wait for completion.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let files = loop {
            let state = slot.lock().unwrap().as_ref().map(|t| t.state.clone());
            match state {
                Some(ZmodemState::Done { files, .. }) => break files,
                Some(ZmodemState::Failed(e)) => panic!("upload failed: {e}"),
                _ => {}
            }
            assert!(
                std::time::Instant::now() < deadline,
                "upload did not finish in time; state: {state:?}"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        assert_eq!(files, vec!["payload.bin".to_string()]);
        assert!(
            bridge.lock().unwrap().bytes() > 0,
            "upload progress counter must advance"
        );

        let received =
            std::fs::read(dst_dir.path().join("payload.bin")).expect("received file");
        assert_eq!(received, payload, "loopback bytes must be identical");

        pump.join().ok();
        // rzsz 0.1.4 ends a clean batch with Err(Cancelled): the sender's
        // ZFIN makes try_zmodem ackbibi and surface Cancelled past the `?`.
        match receiver.join().expect("receiver thread") {
            Ok(_) | Err(rzsz::zmodem::ZError::Cancelled) => {}
            other => panic!("receiver failed: {other:?}"),
        }
    }

    /// Cancelling mid-handshake (peer never answers) settles the slot at
    /// `Cancelled` promptly instead of hanging on protocol timeouts.
    #[test]
    fn bridge_cancel_reports_cancelled() {
        let (_a, b) = UnixStream::pair().unwrap();
        // The "remote" end stays silent: the sender handshake will block in
        // read timeouts until we cancel.
        let slot: Arc<Mutex<Option<ZmodemTransfer>>> = Arc::new(Mutex::new(None));
        let sink_stream = Mutex::new(b);
        let sink = SessionWriter::new(move |data| {
            sink_stream.lock().unwrap().write_all(data).map(|_| ())
        });
        let src = tempfile::tempdir().unwrap();
        let path = src.path().join("x.bin");
        std::fs::write(&path, b"hello").unwrap();

        let mut bridge = ZmodemBridge::spawn(
            TransferRequest::Upload { path },
            sink,
            Arc::clone(&slot),
        );

        std::thread::sleep(std::time::Duration::from_millis(200));
        bridge.cancel();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let state = slot.lock().unwrap().as_ref().map(|t| t.state.clone());
            match state {
                Some(ZmodemState::Cancelled) => break,
                Some(ZmodemState::Failed(e)) => panic!("cancel surfaced as failure: {e}"),
                _ => {}
            }
            assert!(
                std::time::Instant::now() < deadline,
                "cancel did not settle the slot in time; state: {state:?}"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}
