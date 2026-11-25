//! ZMODEM engine — Windows implementation: inert.
//!
//! The transfer engine is POSIX-bound (`UnixStream` + the rzsz crate), so
//! Windows gets the facade's types with no transfer behind them: `spawn`
//! reports `Failed`, `route_chunk` passes every chunk straight through to
//! the normal render path, and the capability probe reports `Missing`.
//! The UI hides the feature via [`platform_supports_zmodem`] instead of
//! carrying cfg gates; a Windows port of the engine (named-pipe transport)
//! would only need to change this file.

use std::sync::{Arc, Mutex};

use super::{
    inert_bridge, set_transfer_state, SessionWriter, TransferRequest, ZmodemBridge, ZmodemState,
    ZmodemSupport, ZmodemTap, ZmodemTransfer,
};

impl ZmodemBridge {
    /// No engine on this platform: record the request as failed so a stray
    /// caller sees why, and return a bridge with no threads behind it.
    pub fn spawn(
        request: TransferRequest,
        _sink: SessionWriter,
        slot: Arc<Mutex<Option<ZmodemTransfer>>>,
    ) -> ZmodemBridge {
        let direction = request.direction();
        set_transfer_state(
            &slot,
            ZmodemTransfer {
                direction,
                state: ZmodemState::Failed("ZMODEM is not available on this platform".into()),
            },
        );
        inert_bridge(direction)
    }
}

/// Passthrough: without an engine every chunk belongs on the normal render
/// path (VTE + asciicast), exactly as if no tap were installed.
pub fn route_chunk(
    tap: &Mutex<Option<ZmodemTap>>,
    bridge: &Mutex<Option<ZmodemBridge>>,
    data: &[u8],
    mut emit: impl FnMut(&[u8]),
) -> super::ChunkRoute {
    let _ = (tap, bridge);
    emit(data);
    super::ChunkRoute::Normal
}

/// `sz`/`rz` are POSIX tools; they do not exist on Windows.
pub fn local_zmodem_support() -> ZmodemSupport {
    ZmodemSupport::Missing
}

pub fn platform_supports_zmodem() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The facade stays honest on Windows: the platform answers "no" and
    /// spawn yields an inert bridge that reports failure.
    #[test]
    fn windows_reports_no_zmodem_support() {
        assert!(!platform_supports_zmodem());
        assert_eq!(local_zmodem_support(), ZmodemSupport::Missing);

        let slot: Arc<Mutex<Option<ZmodemTransfer>>> = Arc::new(Mutex::new(None));
        let mut bridge = ZmodemBridge::spawn(
            TransferRequest::Upload {
                path: std::path::PathBuf::from("unused"),
            },
            SessionWriter::new(|_| Ok(())),
            Arc::clone(&slot),
        );
        let transfer = slot.lock().unwrap().clone().expect("failed state set");
        assert!(!transfer.is_active());
        assert!(matches!(transfer.state, ZmodemState::Failed(_)));
        // Inert: nothing to feed, nothing counted.
        assert!(!bridge.feed(b"**\x18B00"));
        assert_eq!(bridge.bytes(), 0);
        bridge.cancel();
    }
}
