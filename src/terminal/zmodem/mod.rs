//! ZMODEM (rz/sz) transfer bridge.
//!
//! Platform facade: the protocol-level machinery (tap, bridge plumbing,
//! transfer-state types) is shared; the engine itself lives in per-OS
//! modules — [`unix`] runs the real `rzsz` engine, [`windows`] provides
//! inert implementations (the engine is POSIX-bound: `UnixStream` + the
//! rzsz crate). UI code gates on [`platform_supports_zmodem`] rather than
//! cfg attributes.
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
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

// Platform dispatch — the only cfg in the facade. unix:: and windows::
// expose identical signatures for the three platform-varying functions.
#[cfg(unix)]
pub use unix::{local_zmodem_support, platform_supports_zmodem, route_chunk};
#[cfg(windows)]
pub use windows::{local_zmodem_support, platform_supports_zmodem, route_chunk};

/// ZRQINIT hex header prefix: ZPAD ZPAD ZDLE ZHEX type=0x00 (hex "00").
/// The remote `sz` announces itself with these bytes; we divert from here.
const ZRQINIT_TOKEN: &[u8; 6] = b"**\x18B00";

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
    Transferring {
        file: String,
    },
    /// Data phase done, ZFIN handshake in flight.
    Finishing,
    Done {
        files: Vec<String>,
        bytes: u64,
    },
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
///
/// The struct is shared across platforms; `spawn` is implemented per OS
/// (unix: real engine; windows: inert — see the platform modules).
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
            Err(std::sync::mpsc::TrySendError::Full(_)) => {
                // The bounded channel backed up past a link-delay window —
                // the transfer thread is not draining (wedged receiver).
                // Abort rather than let the engine see a corrupted stream.
                self.wedged.store(true, Ordering::Relaxed);
                self.cancel();
                false
            }
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
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

/// A bridge-shaped value with no threads behind it, for degenerate spawns
/// (unix: setup failures; windows: no engine at all).
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

// ─────────────────────────────────────────────────────────────────────────────
// Tests (platform-neutral protocol logic; engine tests live in unix.rs)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(matches!(tap.feed(b"deadbeef\r\n"), TapAction::ActiveDivert));
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

    // ── shell quoting ────────────────────────────────────────────────────

    #[test]
    fn shell_quote_single_escapes_quotes() {
        assert_eq!(shell_quote_single("a b"), "'a b'");
        assert_eq!(shell_quote_single("it's"), r"'it'\''s'");
        assert_eq!(shell_quote_single(""), "''");
    }

    // ── Capability probe ─────────────────────────────────────────────────

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
}
