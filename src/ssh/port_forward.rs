//! SSH port forwarding (Local -L and Remote -R)

use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::watch;

use super::SshClient;

// Re-export config types so consumers can use them via this module
pub use crate::config::{ForwardKind, PortForwardConfig};

/// Runtime state of a single port forward
#[derive(Debug, Clone, PartialEq)]
pub enum ForwardState {
    Starting,
    Active,
    Error(String),
    /// Port is already in use by another session
    Conflict(String),
    Stopped,
}

/// Notification severity level
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum NotificationLevel {
    Info,
    Warning,
    Error,
}

/// A single app notification
#[derive(Debug, Clone)]
pub struct AppNotification {
    pub message: String,
    pub level: NotificationLevel,
    pub created_at: std::time::Instant,
}

impl AppNotification {
    #[allow(dead_code)]
    pub fn info(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            level: NotificationLevel::Info,
            created_at: std::time::Instant::now(),
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            level: NotificationLevel::Warning,
            created_at: std::time::Instant::now(),
        }
    }

    #[allow(dead_code)]
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            level: NotificationLevel::Error,
            created_at: std::time::Instant::now(),
        }
    }

    pub fn is_expired(&self, timeout_secs: u64) -> bool {
        self.created_at.elapsed().as_secs() >= timeout_secs
    }
}

/// A live port forward with cancellation handle
pub struct PortForward {
    pub config: PortForwardConfig,
    pub state: Arc<std::sync::Mutex<ForwardState>>,
    /// Allocated port for remote forwards (returned by tcpip_forward).
    /// Read only by tests to verify the port was stored — production reads the
    /// value via the forward's live state, so this is intentionally
    /// test-observable state, not dead.
    #[allow(dead_code)]
    pub allocated_port: Arc<std::sync::Mutex<Option<u32>>>,
    cancel_tx: watch::Sender<bool>,
}

impl PortForward {
    pub fn new(config: PortForwardConfig) -> (Self, watch::Receiver<bool>) {
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let pf = Self {
            config,
            state: Arc::new(std::sync::Mutex::new(ForwardState::Starting)),
            allocated_port: Arc::new(std::sync::Mutex::new(None)),
            cancel_tx,
        };
        (pf, cancel_rx)
    }

    pub fn stop(&self) {
        let _ = self.cancel_tx.send(true);
        if let Ok(mut s) = self.state.lock() {
            *s = ForwardState::Stopped;
        }
    }

    #[allow(dead_code)]
    pub fn current_state(&self) -> ForwardState {
        self.state
            .lock()
            .map(|s| s.clone())
            .unwrap_or(ForwardState::Error("Lock poisoned".into()))
    }
}

/// Start a local forward (-L): bind a local TCP listener, for each incoming
/// connection open a direct-tcpip channel through the SSH handle and bridge
/// the two streams bidirectionally.
pub async fn start_local_forward(
    handle: Arc<russh::client::Handle<SshClient>>,
    config: PortForwardConfig,
    state: Arc<std::sync::Mutex<ForwardState>>,
    mut cancel_rx: watch::Receiver<bool>,
    notifications: Arc<std::sync::Mutex<Vec<AppNotification>>>,
) {
    let bind_addr = format!("{}:{}", config.local_host, config.local_port);
    let listener = match TcpListener::bind(&bind_addr).await {
        Ok(l) => l,
        Err(e) => {
            let error_msg = e.to_string();
            let is_addr_in_use = error_msg.contains("Address already in use")
                || error_msg.contains("address in use")
                || error_msg.contains("EADDRINUSE");

            if is_addr_in_use {
                log::warn!(
                    "Port forward conflict: {}:{} already in use, skipping",
                    config.local_host, config.local_port
                );
                if let Ok(mut s) = state.lock() {
                    *s = ForwardState::Conflict(format!(
                        "Port {}:{} already in use",
                        config.local_host, config.local_port
                    ));
                }
                if let Ok(mut ns) = notifications.lock() {
                    ns.push(AppNotification::warning(format!(
                        "Port {}:{} is already in use by another session, skipping forward",
                        config.local_host, config.local_port
                    )));
                }
            } else {
                log::error!("Port forward: failed to bind {}: {}", bind_addr, e);
                if let Ok(mut s) = state.lock() {
                    *s = ForwardState::Error(format!("Bind failed: {}", e));
                }
            }
            return;
        }
    };

    log::info!(
        "Local forward active: {} -> {}:{}",
        bind_addr, config.remote_host, config.remote_port
    );
    if let Ok(mut s) = state.lock() {
        *s = ForwardState::Active;
    }

    let remote_host = config.remote_host.clone();
    let remote_port = config.remote_port as u32;

    // Main accept loop with explicit return type
    let result: Result<(), ()> = loop {
        tokio::select! {
            _ = cancel_rx.changed() => {
                if *cancel_rx.borrow() {
                    log::info!("Local forward {} stopped by cancellation", bind_addr);
                    break Ok(());
                }
            }
            result = listener.accept() => {
                match result {
                    Ok((tcp_stream, peer)) => {
                        let handle = Arc::clone(&handle);
                        let rh = remote_host.clone();
                        let rp = remote_port;
                        let originator = peer.ip().to_string();
                        let originator_port = peer.port() as u32;

                        tokio::spawn(async move {
                            if let Err(e) = bridge_local_connection(
                                handle, tcp_stream, &rh, rp, &originator, originator_port,
                            ).await {
                                log::warn!("Local forward connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        log::warn!("Local forward accept error: {}", e);
                        // Continue accepting on error
                    }
                }
            }
        }
    };

    // Listener is dropped here, which closes the TCP socket
    drop(listener);

    if let Ok(mut s) = state.lock() {
        if *s == ForwardState::Active {
            *s = ForwardState::Stopped;
        }
    }

    // Log completion
    if result.is_ok() {
        log::info!("Local forward {} completed", bind_addr);
    }
}

/// How long to wait for the SSH server to confirm a direct-tcpip channel open.
/// A healthy open completes in ~1 RTT; without a cap a wedged server leaves the
/// local TCP connection hanging until the *client's* own connect timeout fires,
/// with nothing in Portal's logs to explain it.
const CHANNEL_OPEN_TIMEOUT: Duration = Duration::from_secs(10);

/// Start a dynamic forward (-D): bind a local TCP listener acting as a SOCKS5
/// proxy. Each client connection performs the RFC 1928 handshake (no-auth,
/// CONNECT only) and gets its own direct-tcpip channel to the destination it
/// asked for — the SSH server resolves domain names.
pub async fn start_dynamic_forward(
    handle: Arc<russh::client::Handle<SshClient>>,
    config: PortForwardConfig,
    state: Arc<std::sync::Mutex<ForwardState>>,
    mut cancel_rx: watch::Receiver<bool>,
    notifications: Arc<std::sync::Mutex<Vec<AppNotification>>>,
) {
    let bind_addr = format!("{}:{}", config.local_host, config.local_port);
    let listener = match TcpListener::bind(&bind_addr).await {
        Ok(l) => l,
        Err(e) => {
            let error_msg = e.to_string();
            let is_addr_in_use = error_msg.contains("Address already in use")
                || error_msg.contains("address in use")
                || error_msg.contains("EADDRINUSE");

            if is_addr_in_use {
                log::warn!(
                    "Dynamic forward conflict: {}:{} already in use, skipping",
                    config.local_host, config.local_port
                );
                if let Ok(mut s) = state.lock() {
                    *s = ForwardState::Conflict(format!(
                        "Port {}:{} already in use",
                        config.local_host, config.local_port
                    ));
                }
                if let Ok(mut ns) = notifications.lock() {
                    ns.push(AppNotification::warning(format!(
                        "Port {}:{} is already in use by another session, skipping forward",
                        config.local_host, config.local_port
                    )));
                }
            } else {
                log::error!("Dynamic forward: failed to bind {}: {}", bind_addr, e);
                if let Ok(mut s) = state.lock() {
                    *s = ForwardState::Error(format!("Bind failed: {}", e));
                }
            }
            return;
        }
    };

    log::info!("Dynamic forward (SOCKS5) active on {}", bind_addr);
    if let Ok(mut s) = state.lock() {
        *s = ForwardState::Active;
    }

    let result: Result<(), ()> = loop {
        tokio::select! {
            _ = cancel_rx.changed() => {
                if *cancel_rx.borrow() {
                    log::info!("Dynamic forward {} stopped by cancellation", bind_addr);
                    break Ok(());
                }
            }
            result = listener.accept() => {
                match result {
                    Ok((tcp_stream, _peer)) => {
                        let handle = Arc::clone(&handle);
                        tokio::spawn(async move {
                            if let Err(e) = serve_socks_connection(handle, tcp_stream).await {
                                log::debug!("SOCKS5 connection ended: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        log::warn!("Dynamic forward accept error: {}", e);
                    }
                }
            }
        }
    };

    drop(listener);

    if let Ok(mut s) = state.lock() {
        if *s == ForwardState::Active {
            *s = ForwardState::Stopped;
        }
    }
    if result.is_ok() {
        log::info!("Dynamic forward {} completed", bind_addr);
    }
}

/// How long a client may take to finish the SOCKS5 greeting + CONNECT request.
const SOCKS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
/// Cap on buffered handshake bytes — greetings and CONNECT requests are tiny.
const SOCKS_MAX_MESSAGE: usize = 1024;

/// Serve one SOCKS5 client connection end-to-end: handshake, channel open,
/// success reply, then bridge the streams until both directions close.
async fn serve_socks_connection(
    handle: Arc<russh::client::Handle<SshClient>>,
    mut stream: tokio::net::TcpStream,
) -> Result<(), String> {
    let request = tokio::time::timeout(
        SOCKS_HANDSHAKE_TIMEOUT,
        socks5_server_handshake(&mut stream),
    )
    .await
    .map_err(|_| "SOCKS5 handshake timed out".to_string())??;

    let channel = match tokio::time::timeout(
        CHANNEL_OPEN_TIMEOUT,
        handle.channel_open_direct_tcpip(
            request.host.to_string(),
            request.port as u32,
            "127.0.0.1",
            0,
        ),
    )
    .await
    {
        Ok(Ok(ch)) => ch,
        Ok(Err(e)) => {
            // Tell the SOCKS client why it failed instead of just closing.
            let _ = stream
                .write_all(&super::socks5::failure_reply(
                    super::socks5::REP_NETWORK_UNREACHABLE,
                ))
                .await;
            return Err(format!("channel open to {} failed: {}", request.host, e));
        }
        Err(_) => {
            return Err(format!("channel open to {} timed out", request.host));
        }
    };

    use tokio::io::AsyncWriteExt;
    stream
        .write_all(&super::socks5::success_reply(&request.host, request.port))
        .await
        .map_err(|e| format!("failed to send SOCKS5 success reply: {}", e))?;

    bridge_half_close(stream, channel.into_stream()).await;
    Ok(())
}

/// Drive the SOCKS5 greeting + CONNECT exchange on a single stream.
///
/// Handles clients that pipeline the greeting and request in one TCP segment:
/// the request parser re-scans the same buffer after the greeting reply is
/// sent, consuming only the bytes it needs.
async fn socks5_server_handshake<S>(stream: &mut S) -> Result<super::socks5::SocksRequest, String>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    use super::socks5::{self, SocksError};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut buf: Vec<u8> = Vec::with_capacity(64);
    let mut tmp = [0u8; 512];

    // 1. Greeting — only no-auth is supported by the -D server.
    let consumed = loop {
        match socks5::parse_greeting(&buf, &[socks5::METHOD_NO_AUTH]) {
            Ok((consumed, _method)) => break consumed,
            Err(SocksError::Incomplete) => {}
            Err(SocksError::UnsupportedMethod) => {
                let _ = stream.write_all(&socks5::greeting_reject_reply()).await;
                return Err("client offered no supported SOCKS5 auth method".into());
            }
            Err(e) => return Err(e.to_string()),
        }
        let n = stream
            .read(&mut tmp)
            .await
            .map_err(|e| format!("SOCKS5 greeting read failed: {}", e))?;
        if n == 0 {
            return Err("client closed before SOCKS5 greeting".into());
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > SOCKS_MAX_MESSAGE {
            return Err("SOCKS5 greeting too large".into());
        }
    };
    stream
        .write_all(&socks5::greeting_reply())
        .await
        .map_err(|e| format!("failed to send SOCKS5 greeting reply: {}", e))?;
    buf.drain(..consumed);

    // 2. CONNECT request (possibly already buffered from the pipelined read).
    loop {
        match socks5::parse_request(&buf) {
            Ok((_consumed, request)) => return Ok(request),
            Err(SocksError::Incomplete) => {}
            Err(e @ SocksError::UnsupportedCommand(_)) => {
                let _ = stream.write_all(&socks5::failure_reply(socks5::REP_CMD_NOT_SUPPORTED)).await;
                return Err(e.to_string());
            }
            Err(e) => {
                let _ = stream.write_all(&socks5::failure_reply(socks5::REP_GENERAL_FAILURE)).await;
                return Err(e.to_string());
            }
        }
        let n = stream
            .read(&mut tmp)
            .await
            .map_err(|e| format!("SOCKS5 request read failed: {}", e))?;
        if n == 0 {
            return Err("client closed before SOCKS5 request".into());
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > SOCKS_MAX_MESSAGE {
            return Err("SOCKS5 request too large".into());
        }
    }
}


/// Bridge a single accepted TCP connection through an SSH direct-tcpip channel.
async fn bridge_local_connection(
    handle: Arc<russh::client::Handle<SshClient>>,
    tcp_stream: tokio::net::TcpStream,
    remote_host: &str,
    remote_port: u32,
    originator: &str,
    originator_port: u32,
) -> Result<(), String> {
    let channel = tokio::time::timeout(
        CHANNEL_OPEN_TIMEOUT,
        handle.channel_open_direct_tcpip(remote_host, remote_port, originator, originator_port),
    )
    .await
    .map_err(|_| {
        format!(
            "channel open to {}:{} timed out after {}s",
            remote_host,
            remote_port,
            CHANNEL_OPEN_TIMEOUT.as_secs()
        )
    })?
    .map_err(|e| {
        format!(
            "channel open to {}:{} failed: {}",
            remote_host, remote_port, e
        )
    })?;

    bridge_half_close(tcp_stream, channel.into_stream()).await;
    Ok(())
}

/// Bridge two bidirectional streams with graceful half-close.
///
/// Each direction runs independently to completion: when one sees EOF it shuts
/// down its write half (TCP FIN / SSH CHANNEL_EOF) so the peer learns no more
/// data follows, while the opposite direction keeps draining until its own EOF.
/// The bridge only ends when both directions finish, so a final response
/// flushed after our EOF (e.g. MySQL's reply to COM_QUIT) is never cut off,
/// and the server sees a clean disconnect instead of an aborted one.
pub(super) async fn bridge_half_close<L, R>(local: L, remote: R)
where
    L: AsyncRead + AsyncWrite,
    R: AsyncRead + AsyncWrite,
{
    let (mut local_r, mut local_w) = tokio::io::split(local);
    let (mut remote_r, mut remote_w) = tokio::io::split(remote);

    let up = async move {
        let res = tokio::io::copy(&mut local_r, &mut remote_w).await;
        let _ = remote_w.shutdown().await;
        res
    };
    let down = async move {
        let res = tokio::io::copy(&mut remote_r, &mut local_w).await;
        let _ = local_w.shutdown().await;
        res
    };

    let (up_res, down_res) = tokio::join!(up, down);
    if let Err(e) = &up_res {
        log::debug!("forward local->remote ended: {}", e);
    }
    if let Err(e) = &down_res {
        log::debug!("forward remote->local ended: {}", e);
    }
}

// Note: Remote forward setup (tcpip_forward) is handled directly in ssh_task
// because it requires &mut Handle. The callback for incoming connections is
// handled via SshClient::server_channel_open_forwarded_tcpip in session.rs.

/// Handle an incoming remote-forwarded connection by connecting to the local target.
/// Called from the SshClient Handler when `server_channel_open_forwarded_tcpip` fires.
pub async fn handle_remote_forward_connection(
    channel: russh::Channel<russh::client::Msg>,
    local_host: &str,
    local_port: u16,
) {
    let addr = format!("{}:{}", local_host, local_port);
    let tcp_stream = match tokio::net::TcpStream::connect(&addr).await {
        Ok(s) => s,
        Err(e) => {
            log::warn!("Remote forward: failed to connect to local {}: {}", addr, e);
            return;
        }
    };

    bridge_half_close(channel.into_stream(), tcp_stream).await;
}
