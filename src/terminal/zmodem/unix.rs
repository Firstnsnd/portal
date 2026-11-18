//! ZMODEM engine — unix implementation.
//!
//! Runs the `rzsz` crate over a `UnixStream` pair: a pump thread forwards
//! diverted session chunks into the modem stream the blocking engine reads
//! from, and protocol frames go back through the session's [`SessionWriter`].
//! This is the only platform with a working ZMODEM transfer today — the
//! facade's [`route_chunk`]/[`local_zmodem_support`]/[`platform_supports_zmodem`]
//! are re-exported from here.

use std::io::{self, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use rzsz::serial::reader::ModemReader;

use super::{
    inert_bridge, set_transfer_state, Direction, SessionWriter, TransferRequest, ZmodemBridge,
    ZmodemState, ZmodemSupport, ZmodemTap, ZmodemTransfer, TapAction, CHANNEL_CHUNKS,
};

/// lrzsz abort sequence: CAN×10 + BS×10 (same as the crate's send_cancel).
const CANCEL_SEQ: [u8; 20] = [
    0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, //
    0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08,
];

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
        let (feed_tx, feed_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(CHANNEL_CHUNKS);
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
// Drain-loop integration (re-exported by the facade)
// ─────────────────────────────────────────────────────────────────────────────

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
) -> super::ChunkRoute {
    let action = tap
        .lock()
        .ok()
        .and_then(|mut guard| guard.as_mut().map(|t| t.feed(data)));
    match action {
        None => {
            // No tap (or poisoned): plain terminal output.
            emit(data);
            super::ChunkRoute::Normal
        }
        Some(TapAction::Hold(pass)) => {
            emit(pass);
            super::ChunkRoute::Normal
        }
        Some(TapAction::Release(lead)) => {
            // `lead` already carries the current chunk (joined minus the
            // newly held tail) — emitting `data` again would duplicate it.
            emit(&lead);
            super::ChunkRoute::Normal
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
) -> super::ChunkRoute {
    let mut guard = match bridge.lock() {
        Ok(g) => g,
        Err(_) => return super::ChunkRoute::Diverted, // poisoned: swallow protocol bytes
    };
    let alive = match guard.as_mut() {
        Some(b) => b.feed(data),
        // Active tap with no bridge (should not happen — bridge is created
        // before the tap can trigger): swallow the protocol bytes.
        None => false,
    };
    if alive {
        return super::ChunkRoute::Diverted;
    }
    // Bridge is gone — hand the stream back to the terminal. The trailing
    // protocol bytes of this chunk are dropped (they are garbage on screen).
    *guard = None;
    if let Ok(mut t) = tap.lock() {
        if let Some(tap) = t.as_mut() {
            tap.reset();
        }
    }
    super::ChunkRoute::Diverted
}

// ─────────────────────────────────────────────────────────────────────────────
// Capability probe
// ─────────────────────────────────────────────────────────────────────────────

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

pub fn platform_supports_zmodem() -> bool {
    true
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests (engine + integration; protocol-logic tests live in mod.rs)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::ChunkRoute;
    use std::io::Read;

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
        let (feed_tx, _feed_rx) = std::sync::mpsc::sync_channel(16);
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
