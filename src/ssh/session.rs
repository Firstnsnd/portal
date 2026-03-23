//! SSH session management using russh

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use tokio::sync::mpsc;

use crate::terminal::{CellAttrs, TerminalGrid, VteHandler};
use crate::config::ResolvedAuth;

#[derive(Clone)]
pub struct JumpHostInfo {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: ResolvedAuth,
}
use super::port_forward::{
    PortForwardConfig, PortForward, ForwardKind, ForwardState,
    start_local_forward,
};

/// Commands sent from the synchronous GUI thread to the async SSH task
enum SshCommand {
    Write(Vec<u8>),
    Resize { cols: u32, rows: u32 },
    Disconnect,
    StartPortForward(PortForwardConfig),
    StopPortForward(usize),
}

/// SSH connection state
#[derive(Debug, Clone)]
pub enum SshConnectionState {
    Connecting,
    Authenticating,
    Connected,
    Disconnected(String),
    Error(String),
}

/// russh client handler with known_hosts verification
pub struct SshClient {
    host: String,
    port: u16,
    /// Remote forward configs: maps (remote_host, remote_port) -> (local_host, local_port)
    /// Used by server_channel_open_forwarded_tcpip callback
    remote_forwards: Arc<Mutex<Vec<PortForwardConfig>>>,
}

impl SshClient {
    pub fn new(host: &str, port: u16) -> Self {
        Self {
            host: host.to_string(),
            port,
            remote_forwards: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl russh::client::Handler for SshClient {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        match russh::keys::check_known_hosts(&self.host, self.port, server_public_key) {
            Ok(true) => {
                // Host key matches known_hosts
                Ok(true)
            }
            Ok(false) => {
                // Host not in known_hosts — auto-learn the key
                log::info!(
                    "New host key for {}:{}, adding to known_hosts",
                    self.host, self.port
                );
                if let Err(e) =
                    russh::keys::known_hosts::learn_known_hosts(&self.host, self.port, server_public_key)
                {
                    log::warn!("Failed to save host key: {}", e);
                }
                Ok(true)
            }
            Err(e) => {
                // Key changed (possible MITM) or file error
                if let russh::keys::Error::KeyChanged { line } = &e {
                    log::error!(
                        "HOST KEY CHANGED for {}:{} at known_hosts line {}!",
                        self.host, self.port, line
                    );
                    return Err(russh::Error::Keys(e));
                }
                log::warn!("known_hosts check error: {}", e);
                Ok(true)
            }
        }
    }

    /// Called when a remote-forwarded connection arrives from the server.
    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<russh::client::Msg>,
        connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        // Look up the remote forward config to find the local target
        let local_target = self.remote_forwards.lock().ok().and_then(|fwds| {
            fwds.iter().find(|c| {
                c.kind == ForwardKind::Remote
                    && c.remote_port == connected_port as u16
            }).map(|c| (c.local_host.clone(), c.local_port))
        });

        if let Some((local_host, local_port)) = local_target {
            log::info!(
                "Remote forward connection: {}:{} -> {}:{}",
                connected_address, connected_port, local_host, local_port
            );
            tokio::spawn(async move {
                super::port_forward::handle_remote_forward_connection(
                    channel, &local_host, local_port,
                ).await;
            });
        } else {
            log::warn!(
                "Received forwarded-tcpip for {}:{} but no matching remote forward config",
                connected_address, connected_port
            );
        }
        Ok(())
    }
}

/// Connect and authenticate an SSH session, returning the handle.
/// Shared by SshSession, test_connection, and SFTP.
pub async fn connect_and_authenticate(
    host: &str,
    port: u16,
    username: &str,
    auth: &ResolvedAuth,
    keepalive_interval: u32,
    _agent_forwarding: bool,
) -> Result<russh::client::Handle<SshClient>, String> {
    let mut config = russh::client::Config::default();
    // Enable SSH keepalive: send a keepalive packet every 15 seconds
    // This helps prevent connections from dropping due to inactivity
    config.keepalive_interval = Some(std::time::Duration::from_secs(15));
    // Max number of failed keepalives before disconnecting (30 = 15s * 30 = 7.5 minutes)
    config.keepalive_max = 30;
    let config = Arc::new(config);
    let addr = format!("{}:{}", host, port);

    let remote_forwards = Arc::new(Mutex::new(Vec::new()));
    let client = SshClient {
        host: host.to_string(),
        port,
        remote_forwards,
    };

    let mut handle = russh::client::connect(config, &addr, client)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("KeyChanged") || msg.contains("key changed") {
                format!("Host key verification failed: server key has changed for {}:{}.\nThis could indicate a MITM attack.\nRemove the old key from ~/.ssh/known_hosts to connect.", host, port)
            } else {
                format!("Connect failed: {}", e)
            }
        })?;

    let auth_ok = match auth {
        ResolvedAuth::Password { password } => {
            handle
                .authenticate_password(username, password)
                .await
                .map(|r| r.success())
                .map_err(|e| format!("Auth error: {}", e))?
        }
        ResolvedAuth::Key { key_content, passphrase } => {
            let pw = passphrase.as_deref();

            let key_pair = if !key_content.is_empty() {
                russh::keys::decode_secret_key(key_content, pw)
                    .map_err(|e| format!("Key decode failed: {}", e))?
            } else {
                return Err("No key content available for authentication".to_string());
            };

            let rsa_hash = handle
                .best_supported_rsa_hash()
                .await
                .ok()
                .flatten()
                .flatten();
            let key_with_hash =
                russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key_pair), rsa_hash);

            handle
                .authenticate_publickey(username, key_with_hash)
                .await
                .map(|r| r.success())
                .map_err(|e| format!("Auth error: {}", e))?
        }
        ResolvedAuth::None => return Err("No authentication configured".to_string()),
    };

    if auth_ok {
        Ok(handle)
    } else {
        Err("Authentication failed".to_string())
    }
}

/// Connect to a target host via a jump host using direct-tcpip forwarding.
/// 1. Connects and authenticates to the jump host
/// 2. Opens a direct-tcpip channel to the target
/// 3. Runs a second SSH handshake through the forwarded channel
pub async fn connect_via_jump(
    jump: &JumpHostInfo,
    target_host: &str,
    target_port: u16,
    target_username: &str,
    target_auth: &ResolvedAuth,
    keepalive_interval: u32,
    _agent_forwarding: bool,
) -> Result<russh::client::Handle<SshClient>, String> {
    let jump_handle = connect_and_authenticate(
        &jump.host, jump.port, &jump.username, &jump.auth,
        keepalive_interval, false,
    ).await.map_err(|e| format!("Jump host connection failed: {}", e))?;

    let tunnel_channel = jump_handle
        .channel_open_direct_tcpip(target_host, target_port as u32, "127.0.0.1", 0)
        .await
        .map_err(|e| format!("Failed to open tunnel through jump host: {}", e))?;

    let stream = tunnel_channel.into_stream();

    let mut config = russh::client::Config::default();
    if keepalive_interval > 0 {
        config.keepalive_interval = Some(std::time::Duration::from_secs(keepalive_interval as u64));
        config.keepalive_max = 3;
    }
    let config = Arc::new(config);

    let mut handle = russh::client::connect_stream(
        config, stream, SshClient::new(target_host, target_port),
    ).await.map_err(|e| format!("SSH handshake through jump host failed: {}", e))?;

    let auth_ok = match target_auth {
        ResolvedAuth::Password { password } => {
            handle
                .authenticate_password(target_username, password)
                .await
                .map(|r| r.success())
                .map_err(|e| format!("Target auth error: {}", e))?
        }
        ResolvedAuth::Key { key_content, passphrase } => {
            let pw = passphrase.as_deref();
            let key_pair = if !key_content.is_empty() {
                russh::keys::decode_secret_key(key_content, pw)
                    .map_err(|e| format!("Target key decode failed: {}", e))?
            } else {
                return Err("No key content available for target authentication".to_string());
            };
            let rsa_hash = handle
                .best_supported_rsa_hash()
                .await
                .ok()
                .flatten()
                .flatten();
            let key_with_hash =
                russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key_pair), rsa_hash);
            handle
                .authenticate_publickey(target_username, key_with_hash)
                .await
                .map(|r| r.success())
                .map_err(|e| format!("Target auth error: {}", e))?
        }
        ResolvedAuth::None => return Err("No authentication configured for target".to_string()),
    };

    if auth_ok {
        Ok(handle)
    } else {
        Err("Target authentication failed".to_string())
    }
}

/// SSH session that mirrors RealPtySession's interface
pub struct SshSession {
    pub grid: Arc<Mutex<TerminalGrid>>,
    cmd_tx: mpsc::UnboundedSender<SshCommand>,
    pub state: Arc<Mutex<SshConnectionState>>,
    /// Remote shell path detected via exec channel (e.g. "/bin/zsh")
    pub shell_hint: Arc<Mutex<Option<String>>>,
    /// Active port forwards managed by this session
    pub port_forwards: Arc<Mutex<Vec<PortForward>>>,
}

impl SshSession {
    pub fn connect(
        runtime: &tokio::runtime::Runtime,
        host: String,
        port: u16,
        username: String,
        auth: ResolvedAuth,
        cols: u16,
        rows: u16,
        startup_commands: Vec<String>,
        keepalive_interval: u32,
        agent_forwarding: bool,
        jump_host: Option<JumpHostInfo>,
    ) -> Self {
        Self::with_scrollback_limit(
            runtime,
            host,
            port,
            username,
            auth,
            cols,
            rows,
            startup_commands,
            crate::terminal::TerminalGrid::DEFAULT_MAX_SCROLLBACK_BYTES,
            keepalive_interval,
            agent_forwarding,
            Vec::new(),
            jump_host,
        )
    }

    pub fn with_scrollback_limit(
        runtime: &tokio::runtime::Runtime,
        host: String,
        port: u16,
        username: String,
        auth: ResolvedAuth,
        cols: u16,
        rows: u16,
        startup_commands: Vec<String>,
        scrollback_limit_bytes: usize,
        keepalive_interval: u32,
        agent_forwarding: bool,
        port_forward_configs: Vec<PortForwardConfig>,
        jump_host: Option<JumpHostInfo>,
    ) -> Self {
        let grid = Arc::new(Mutex::new(TerminalGrid::with_scrollback_limit(
            cols as usize,
            rows as usize,
            scrollback_limit_bytes,
        )));
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let state = Arc::new(Mutex::new(SshConnectionState::Connecting));
        let alive = Arc::new(AtomicBool::new(true));
        let shell_hint = Arc::new(Mutex::new(None::<String>));
        let port_forwards = Arc::new(Mutex::new(Vec::<PortForward>::new()));

        let grid_clone = Arc::clone(&grid);
        let state_clone = Arc::clone(&state);
        let alive_clone = Arc::clone(&alive);
        let shell_hint_clone = Arc::clone(&shell_hint);
        let port_forwards_clone = Arc::clone(&port_forwards);

        runtime.spawn(async move {
            Self::ssh_task(
                host, port, username, auth, cols, rows, grid_clone, cmd_rx,
                state_clone, alive_clone, shell_hint_clone, startup_commands,
                keepalive_interval, agent_forwarding, port_forward_configs,
                port_forwards_clone, jump_host,
            )
            .await;
        });

        Self {
            grid,
            cmd_tx,
            state,
            shell_hint,
            port_forwards,
        }
    }

    async fn ssh_task(
        host: String,
        port: u16,
        username: String,
        auth: ResolvedAuth,
        cols: u16,
        rows: u16,
        grid: Arc<Mutex<TerminalGrid>>,
        mut cmd_rx: mpsc::UnboundedReceiver<SshCommand>,
        state: Arc<Mutex<SshConnectionState>>,
        alive: Arc<AtomicBool>,
        shell_hint: Arc<Mutex<Option<String>>>,
        startup_commands: Vec<String>,
        keepalive_interval: u32,
        agent_forwarding: bool,
        port_forward_configs: Vec<PortForwardConfig>,
        port_forwards: Arc<Mutex<Vec<PortForward>>>,
        jump_host: Option<JumpHostInfo>,
    ) {
        let set_state = |s: SshConnectionState| {
            if let Ok(mut st) = state.lock() {
                *st = s;
            }
        };

        // 1. Connect + Authenticate using shared helper
        set_state(SshConnectionState::Authenticating);

        // Pre-populate remote forward configs so the Handler callback can route connections
        let remote_fwd_configs: Vec<PortForwardConfig> = port_forward_configs.iter()
            .filter(|c| c.kind == ForwardKind::Remote)
            .cloned()
            .collect();
        let remote_fwd_arc = Arc::new(Mutex::new(remote_fwd_configs));

        let mut handle = if let Some(ref jump) = jump_host {
            match connect_via_jump(
                jump, &host, port, &username, &auth,
                keepalive_interval, agent_forwarding,
            ).await {
                Ok(h) => h,
                Err(e) => {
                    set_state(SshConnectionState::Error(e));
                    alive.store(false, Ordering::Relaxed);
                    return;
                }
            }
        } else {
            match connect_and_authenticate(
                &host, port, &username, &auth, keepalive_interval, agent_forwarding,
            ).await {
                Ok(h) => h,
                Err(e) => {
                    set_state(SshConnectionState::Error(e));
                    alive.store(false, Ordering::Relaxed);
                    return;
                }
            }
        };

        // 3. Detect remote shell via a short-lived exec channel
        if let Ok(mut exec_ch) = handle.channel_open_session().await {
            if exec_ch.exec(false, "echo $SHELL").await.is_ok() {
                let mut output = Vec::new();
                loop {
                    match exec_ch.wait().await {
                        Some(russh::ChannelMsg::Data { data }) => output.extend_from_slice(&data),
                        Some(russh::ChannelMsg::Eof)
                        | Some(russh::ChannelMsg::Close)
                        | None => break,
                        _ => {}
                    }
                }
                let shell_path = String::from_utf8_lossy(&output).trim().to_string();
                if !shell_path.is_empty() {
                    if let Ok(mut h) = shell_hint.lock() {
                        *h = Some(shell_path);
                    }
                }
            }
        }

        // 4. Open channel + request PTY + shell
        let mut channel = match handle.channel_open_session().await {
            Ok(ch) => ch,
            Err(e) => {
                set_state(SshConnectionState::Error(format!(
                    "Channel open failed: {}",
                    e
                )));
                alive.store(false, Ordering::Relaxed);
                return;
            }
        };

        if let Err(e) = channel
            .request_pty(false, "xterm-256color", cols as u32, rows as u32, 0, 0, &[])
            .await
        {
            set_state(SshConnectionState::Error(format!(
                "PTY request failed: {}",
                e
            )));
            alive.store(false, Ordering::Relaxed);
            return;
        }

        if agent_forwarding {
            if let Err(e) = channel.agent_forward(false).await {
                log::warn!("Agent forwarding request failed: {}", e);
            }
        }

        if let Err(e) = channel.request_shell(false).await {
            set_state(SshConnectionState::Error(format!(
                "Shell request failed: {}",
                e
            )));
            alive.store(false, Ordering::Relaxed);
            return;
        }

        set_state(SshConnectionState::Connected);

        // Send startup commands
        for cmd in &startup_commands {
            let cmd_line = format!("{}\n", cmd.trim());
            if channel.data(cmd_line.as_bytes()).await.is_err() {
                break;
            }
        }

        // 5. Auto-start configured port forwards
        //    Remote forwards need &mut handle, so do them first before wrapping in Arc.
        for cfg in port_forward_configs.iter().filter(|c| c.kind == ForwardKind::Remote) {
            let (pf, cancel_rx) = PortForward::new(cfg.clone());
            let fwd_state = Arc::clone(&pf.state);
            if let Ok(mut fwds) = port_forwards.lock() {
                fwds.push(pf);
            }
            // Register in handler's remote forward table
            if let Ok(mut rfwds) = remote_fwd_arc.lock() {
                if !rfwds.iter().any(|c| c.remote_port == cfg.remote_port) {
                    rfwds.push(cfg.clone());
                }
            }
            match handle.tcpip_forward(&cfg.remote_host, cfg.remote_port as u32).await {
                Ok(port) => {
                    log::info!(
                        "Remote forward active: {}:{} (allocated port {}) -> {}:{}",
                        cfg.remote_host, cfg.remote_port, port,
                        cfg.local_host, cfg.local_port
                    );
                    if let Ok(mut s) = fwd_state.lock() { *s = ForwardState::Active; }
                }
                Err(e) => {
                    log::error!("Remote forward request failed: {}", e);
                    if let Ok(mut s) = fwd_state.lock() {
                        *s = ForwardState::Error(format!("tcpip_forward failed: {}", e));
                    }
                }
            }
            let _keep = cancel_rx; // keep alive for tracking
        }

        //    Local forwards only need &self, so Arc is fine.
        let handle = Arc::new(handle);
        for cfg in port_forward_configs.iter().filter(|c| c.kind == ForwardKind::Local) {
            Self::spawn_local_forward(
                Arc::clone(&handle),
                cfg.clone(),
                &port_forwards,
            );
        }

        // 6. Main I/O loop
        let mut parser = vte::Parser::new();
        let mut attrs = CellAttrs::default();

        loop {
            tokio::select! {
                msg = channel.wait() => {
                    match msg {
                        Some(russh::ChannelMsg::Data { data }) => {
                            if let Ok(mut g) = grid.lock() {
                                let mut handler = VteHandler {
                                    grid: &mut g,
                                    attrs: &mut attrs,
                                };
                                for byte in &*data {
                                    parser.advance(&mut handler, *byte);
                                }
                            }
                        }
                        Some(russh::ChannelMsg::ExtendedData { data, .. }) => {
                            if let Ok(mut g) = grid.lock() {
                                let mut handler = VteHandler {
                                    grid: &mut g,
                                    attrs: &mut attrs,
                                };
                                for byte in &*data {
                                    parser.advance(&mut handler, *byte);
                                }
                            }
                        }
                        Some(russh::ChannelMsg::Eof) | Some(russh::ChannelMsg::Close) | None => {
                            set_state(SshConnectionState::Disconnected("Session ended".into()));
                            alive.store(false, Ordering::Relaxed);
                            break;
                        }
                        _ => {}
                    }
                }
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(SshCommand::Write(data)) => {
                            let _ = channel.data(&data[..]).await;
                        }
                        Some(SshCommand::Resize { cols, rows }) => {
                            let _ = channel.window_change(cols, rows, 0, 0).await;
                            if let Ok(mut g) = grid.lock() {
                                g.resize(cols as usize, rows as usize);
                            }
                        }
                        Some(SshCommand::StartPortForward(cfg)) => {
                            // Only local forwards can be started dynamically via Arc<Handle>
                            // Remote forwards need &mut Handle which we don't have here after Arc wrapping
                            if cfg.kind == ForwardKind::Local {
                                Self::spawn_local_forward(
                                    Arc::clone(&handle),
                                    cfg,
                                    &port_forwards,
                                );
                            } else {
                                log::warn!("Dynamic remote forward not supported after connection (requires &mut Handle)");
                                let (pf, _cancel_rx) = PortForward::new(cfg);
                                if let Ok(mut s) = pf.state.lock() {
                                    *s = ForwardState::Error("Remote forwards must be configured before connect".into());
                                }
                                if let Ok(mut fwds) = port_forwards.lock() {
                                    fwds.push(pf);
                                }
                            }
                        }
                        Some(SshCommand::StopPortForward(index)) => {
                            if let Ok(fwds) = port_forwards.lock() {
                                if let Some(pf) = fwds.get(index) {
                                    pf.stop();
                                }
                            }
                        }
                        Some(SshCommand::Disconnect) | None => {
                            // Stop all port forwards
                            if let Ok(fwds) = port_forwards.lock() {
                                for pf in fwds.iter() {
                                    pf.stop();
                                }
                            }
                            let _ = channel.eof().await;
                            let _ = channel.close().await;
                            set_state(SshConnectionState::Disconnected("User disconnected".into()));
                            alive.store(false, Ordering::Relaxed);
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Spawn a local port forward task and track it in port_forwards.
    fn spawn_local_forward(
        handle: Arc<russh::client::Handle<SshClient>>,
        cfg: PortForwardConfig,
        port_forwards: &Arc<Mutex<Vec<PortForward>>>,
    ) {
        let (pf, cancel_rx) = PortForward::new(cfg.clone());
        let fwd_state = Arc::clone(&pf.state);

        if let Ok(mut fwds) = port_forwards.lock() {
            fwds.push(pf);
        }

        tokio::spawn(async move {
            start_local_forward(handle, cfg, fwd_state, cancel_rx).await;
        });
    }

    pub fn write(&self, data: &[u8]) -> std::io::Result<()> {
        let _ = self.cmd_tx.send(SshCommand::Write(data.to_vec()));
        Ok(())
    }

    pub fn get_grid(&self) -> Arc<Mutex<TerminalGrid>> {
        Arc::clone(&self.grid)
    }

    pub fn resize(&self, cols: u16, rows: u16) -> std::io::Result<()> {
        let _ = self.cmd_tx.send(SshCommand::Resize {
            cols: cols as u32,
            rows: rows as u32,
        });
        Ok(())
    }

    pub fn get_shell_hint(&self) -> Option<String> {
        self.shell_hint.lock().ok().and_then(|h| h.clone())
    }

    pub fn connection_state(&self) -> SshConnectionState {
        self.state
            .lock()
            .map(|s| s.clone())
            .unwrap_or(SshConnectionState::Error("Lock poisoned".into()))
    }

    pub fn disconnect(&self) {
        let _ = self.cmd_tx.send(SshCommand::Disconnect);
    }

    /// Start a port forward on this session (sent as a command to the async task)
    pub fn start_port_forward(&self, config: PortForwardConfig) {
        let _ = self.cmd_tx.send(SshCommand::StartPortForward(config));
    }

    /// Stop a port forward by index
    pub fn stop_port_forward(&self, index: usize) {
        let _ = self.cmd_tx.send(SshCommand::StopPortForward(index));
    }

    /// Get the current list of port forward states
    pub fn get_port_forward_states(&self) -> Vec<(PortForwardConfig, ForwardState)> {
        self.port_forwards.lock().ok().map(|fwds| {
            fwds.iter().map(|pf| (pf.config.clone(), pf.current_state())).collect()
        }).unwrap_or_default()
    }
}

impl Drop for SshSession {
    fn drop(&mut self) {
        self.disconnect();
    }
}

/// Test SSH connectivity: connect + authenticate without opening a shell.
/// Returns Ok(message) on success or Err(message) on failure.
pub async fn test_connection(
    host: String,
    port: u16,
    username: String,
    auth: ResolvedAuth,
    keepalive_interval: u32,
    agent_forwarding: bool,
) -> Result<String, String> {
    let _handle = connect_and_authenticate(&host, port, &username, &auth, keepalive_interval, agent_forwarding).await?;
    Ok("Connection successful! Authentication passed.".to_string())
}

/// Remove a host key from the user's known_hosts file.
/// This is useful when a server's host key has changed.
/// Returns Ok(lines_removed) on success or Err(message) on failure.
pub fn remove_known_hosts_key(host: &str, port: u16) -> Result<usize, String> {
    use std::env;
    use std::fs::File;
    use std::io::{BufRead, BufReader, Write};

    // Get the known_hosts file path
    let home = env::var("HOME").map_err(|_| "Failed to get HOME directory".to_string())?;
    let known_hosts_path = format!("{}/.ssh/known_hosts", home);

    // Read the existing known_hosts file
    let file = File::open(&known_hosts_path);
    if file.is_err() {
        return Err("known_hosts file not found".to_string());
    }

    let reader = BufReader::new(file.unwrap());
    let mut lines = Vec::new();
    let mut removed_count = 0;

    // Parse each line and filter out matching entries
    for line in reader.lines() {
        let line = line.map_err(|e| format!("Failed to read known_hosts: {}", e))?;
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            lines.push(line);
            continue;
        }

        // Parse the host pattern from the line
        // Format: [host]:port keytype keydata OR host keytype keydata
        let host_pattern = if let Some(stripped_hash) = trimmed.strip_prefix("@") {
            // Hashed hostname entry - skip as we can't easily parse it
            // These entries start with "|1|..." or similar
            if stripped_hash.starts_with('|') {
                lines.push(line);
                continue;
            }
            stripped_hash.split_whitespace().next().unwrap_or("")
        } else {
            trimmed.split_whitespace().next().unwrap_or("")
        };

        // Check if this line matches our host:port
        let matches = if host_pattern.contains(':') {
            // Pattern already includes port like "[host]:port"
            let expected = format!("[{}]:{}", host, port);
            host_pattern == expected
        } else {
            // Pattern is just hostname or "[hostname]"
            let clean_pattern = host_pattern.trim_start_matches('[').trim_end_matches(']');
            let expected = if port == 22 {
                host.to_string()
            } else {
                format!("[{}]:{}", host, port)
            };
            clean_pattern == host || host_pattern == &expected
        };

        if matches {
            removed_count += 1;
        } else {
            lines.push(line);
        }
    }

    if removed_count == 0 {
        return Err("No matching host key found in known_hosts".to_string());
    }

    // Write back the filtered lines
    let mut file = File::create(&known_hosts_path)
        .map_err(|e| format!("Failed to open known_hosts for writing: {}", e))?;

    for line in lines {
        writeln!(file, "{}", line)
            .map_err(|e| format!("Failed to write to known_hosts: {}", e))?;
    }

    Ok(removed_count)
}
