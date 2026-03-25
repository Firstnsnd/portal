//! SSH port forwarding (Local -L and Remote -R)

use std::sync::Arc;
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
    Stopped,
}

/// A live port forward with cancellation handle
pub struct PortForward {
    pub config: PortForwardConfig,
    pub state: Arc<std::sync::Mutex<ForwardState>>,
    /// Allocated port for remote forwards (returned by tcpip_forward)
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
) {
    let bind_addr = format!("{}:{}", config.local_host, config.local_port);
    let listener = match TcpListener::bind(&bind_addr).await {
        Ok(l) => l,
        Err(e) => {
            log::error!("Port forward: failed to bind {}: {}", bind_addr, e);
            if let Ok(mut s) = state.lock() {
                *s = ForwardState::Error(format!("Bind failed: {}", e));
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
    match result {
        Ok(_) => log::info!("Local forward {} completed", bind_addr),
        Err(_) => {} // Already logged above
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
    let channel = handle
        .channel_open_direct_tcpip(remote_host, remote_port, originator, originator_port)
        .await
        .map_err(|e| format!("channel_open_direct_tcpip failed: {}", e))?;

    let mut channel_stream = channel.into_stream();
    let (mut tcp_read, mut tcp_write) = tokio::io::split(tcp_stream);

    // Use manual copy loops with select for clean shutdown
    let (mut ch_read, mut ch_write) = tokio::io::split(&mut channel_stream);

    let c1 = tokio::io::copy(&mut tcp_read, &mut ch_write);
    let c2 = tokio::io::copy(&mut ch_read, &mut tcp_write);

    tokio::select! {
        r = c1 => {
            if let Err(e) = r {
                log::debug!("local fwd tcp->ssh ended: {}", e);
            }
        }
        r = c2 => {
            if let Err(e) = r {
                log::debug!("local fwd ssh->tcp ended: {}", e);
            }
        }
    }

    Ok(())
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

    let mut channel_stream = channel.into_stream();
    let (mut tcp_read, mut tcp_write) = tokio::io::split(tcp_stream);
    let (mut ch_read, mut ch_write) = tokio::io::split(&mut channel_stream);

    let c1 = tokio::io::copy(&mut tcp_read, &mut ch_write);
    let c2 = tokio::io::copy(&mut ch_read, &mut tcp_write);

    tokio::select! {
        r = c1 => {
            if let Err(e) = r {
                log::debug!("remote fwd tcp->ssh ended: {}", e);
            }
        }
        r = c2 => {
            if let Err(e) = r {
                log::debug!("remote fwd ssh->tcp ended: {}", e);
            }
        }
    }
}
