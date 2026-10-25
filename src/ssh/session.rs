//! SSH session management using russh

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use tokio::sync::mpsc;

use crate::terminal::{CellAttrs, TerminalGrid, VteHandler};
use crate::config::{ResolvedAuth, ResolvedProxy};
use crate::terminal::metrics::MetricsSnapshot;

#[derive(Clone)]
pub struct JumpHostInfo {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: ResolvedAuth,
    /// Proxy for reaching the jump host itself (target leg rides the
    /// jump's direct-tcpip tunnel, so it needs no proxy of its own).
    pub proxy: Option<ResolvedProxy>,
}
use super::port_forward::{
    PortForwardConfig, PortForward, ForwardKind, ForwardState,
    start_local_forward, start_dynamic_forward, AppNotification,
};
use super::auth::connect_and_authenticate;

/// Commands sent from the synchronous GUI thread to the async SSH task
enum SshCommand {
    Write(Vec<u8>),
    Resize { cols: u32, rows: u32 },
    Disconnect,
    #[allow(dead_code)]
    StartPortForward(PortForwardConfig),
    #[allow(dead_code)]
    StopPortForward(PortForwardConfig),
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
    /// Host-key treatment: `Learn` = verify + auto-learn (production);
    /// `AcceptAll` = skip known_hosts entirely (tests only).
    host_key_policy: super::auth::HostKeyPolicy,
    /// Remote forward configs: maps (remote_host, remote_port) -> (local_host, local_port)
    /// Used by server_channel_open_forwarded_tcpip callback
    remote_forwards: Arc<Mutex<Vec<PortForwardConfig>>>,
}

impl SshClient {
    pub fn with_policy(
        host: &str,
        port: u16,
        policy: super::auth::HostKeyPolicy,
        remote_forwards: Arc<Mutex<Vec<PortForwardConfig>>>,
    ) -> Self {
        Self {
            host: host.to_string(),
            port,
            host_key_policy: policy,
            remote_forwards,
        }
    }
}

impl russh::client::Handler for SshClient {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        // Tests connect to ephemeral-key mock servers; never touch the
        // user's real known_hosts from there.
        if self.host_key_policy == super::auth::HostKeyPolicy::AcceptAll {
            return Ok(true);
        }
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

/// Connect to a target host via a jump host using direct-tcpip forwarding.
/// 1. Connects and authenticates to the jump host
/// 2. Opens a direct-tcpip channel to the target
/// 3. Runs a second SSH handshake through the forwarded channel
///
/// `host_key_policy` is `Learn` in production; tests pass `AcceptAll`
/// against ephemeral-key mocks. Both hops share `bridge` — either leg may
/// need keyboard-interactive 2FA.
#[allow(clippy::too_many_arguments)] // jump + target + auth + bridge + policy
pub async fn connect_via_jump(
    jump: &JumpHostInfo,
    target_host: &str,
    target_port: u16,
    target_username: &str,
    target_auth: &ResolvedAuth,
    keepalive_interval: u32,
    _agent_forwarding: bool,
    bridge: &super::auth_prompt::AuthPromptBridge,
    host_key_policy: super::auth::HostKeyPolicy,
    remote_forwards: Arc<Mutex<Vec<PortForwardConfig>>>,
) -> Result<russh::client::Handle<SshClient>, String> {
    let mut jump_handle = super::auth::open_ssh_connection(
        &jump.host,
        jump.port,
        host_key_policy,
        Some((std::time::Duration::from_secs(15), 30)),
        // The jump leg never carries remote forwards of the target session.
        Arc::new(Mutex::new(Vec::new())),
        jump.proxy.as_ref(),
    )
    .await
    .map_err(|e| format!("Jump host connection failed: {}", e))?;

    super::auth::authenticate(&mut jump_handle, &jump.username, &jump.auth, bridge)
        .await
        .map_err(|e| format!("Jump host connection failed: {}", e))?;

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
        config, stream, SshClient::with_policy(
            target_host, target_port, host_key_policy,
            remote_forwards,
        ),
    ).await.map_err(|e| format!("SSH handshake through jump host failed: {}", e))?;

    super::auth::authenticate(&mut handle, target_username, target_auth, bridge)
        .await
        .map_err(|e| format!("Target authentication failed: {}", e))?;

    Ok(handle)
}

/// Execute a one-off command over an open SSH handle and return stdout.
async fn exec_command(
    handle: &Arc<russh::client::Handle<SshClient>>,
    cmd: &str,
) -> Result<String, String> {
    let mut channel = handle
        .channel_open_session()
        .await
        .map_err(|e| format!("open session: {}", e))?;
    channel
        .exec(false, cmd)
        .await
        .map_err(|e| format!("exec: {}", e))?;
    let mut output = Vec::new();
    loop {
        match channel.wait().await {
            Some(russh::ChannelMsg::Data { data }) => output.extend_from_slice(&data),
            Some(russh::ChannelMsg::Eof)
            | Some(russh::ChannelMsg::Close)
            | None => break,
            _ => {}
        }
    }
    Ok(String::from_utf8_lossy(&output).into_owned())
}

/// SSH session that mirrors RealPtySession's interface
pub struct SshSession {
    pub grid: Arc<Mutex<TerminalGrid>>,
    cmd_tx: mpsc::UnboundedSender<SshCommand>,
    pub state: Arc<Mutex<SshConnectionState>>,
    /// Remote shell path detected via exec channel (e.g. "/bin/zsh")
    pub shell_hint: Arc<Mutex<Option<String>>>,
    /// Remote OS detected via `uname -s` (e.g. "Linux", "Darwin"); `None` if
    /// the probe failed or hasn't completed yet.
    pub os_hint: Arc<Mutex<Option<String>>>,
    /// Remote architecture detected via `uname -m` (e.g. "x86_64", "aarch64").
    pub arch_hint: Arc<Mutex<Option<String>>>,
    /// Active port forwards managed by this session
    #[allow(dead_code)]
    pub port_forwards: Arc<Mutex<Vec<PortForward>>>,
    /// Notifications queue (drained by UI layer)
    pub notifications: Arc<Mutex<Vec<AppNotification>>>,
    /// Live system metrics collected from the remote host
    pub metrics: Arc<Mutex<MetricsSnapshot>>,
    /// Gate for remote metrics polling — true only while the Metrics tab
    /// of the tools drawer is visible (synced by the UI every frame).
    metrics_enabled: Arc<AtomicBool>,
    /// Keyboard-interactive (2FA) prompt mailbox shared with the UI.
    pub auth_prompt: Arc<super::auth_prompt::AuthPromptBridge>,
    /// Asciicast recorder slot: `Some` while the session is being recorded.
    /// Cloned into ssh_task at spawn, so the data taps see it.
    pub recorder: Arc<Mutex<Option<crate::terminal::cast::CastRecorder>>>,
    /// ZMODEM tap: watches output for a remote `sz` ZRQINIT and, once armed
    /// or triggered, diverts all output into the transfer bridge. Cloned
    /// into ssh_task at spawn, like the recorder slot.
    #[cfg(unix)]
    pub zmodem_tap: Arc<Mutex<Option<crate::terminal::zmodem::ZmodemTap>>>,
    /// Active ZMODEM transfer bridge (engine thread + its input path).
    #[cfg(unix)]
    pub zmodem_bridge: Arc<Mutex<Option<crate::terminal::zmodem::ZmodemBridge>>>,
    /// Chip state for the UI: what is transferring and how far it got.
    #[cfg(unix)]
    pub zmodem_transfer: Arc<Mutex<Option<crate::terminal::zmodem::ZmodemTransfer>>>,
    /// ZMODEM support probe result for the remote host
    /// ([`ZmodemSupport::to_u8`]; 0 = unknown until the probe lands).
    #[cfg(unix)]
    pub zmodem_support: Arc<std::sync::atomic::AtomicU8>,
    /// SFTP subsystem support for the remote host
    /// ([`HostCapability::to_u8`]) — drives the hosts-list badge.
    pub sftp_support: Arc<std::sync::atomic::AtomicU8>,
}

/// ZMODEM slot types — real on unix, unit placeholders elsewhere (the rzsz
/// engine is unix-only; SshSession itself compiles on every platform).
#[cfg(unix)]
type ZmodemTapSlot = Arc<Mutex<Option<crate::terminal::zmodem::ZmodemTap>>>;
#[cfg(not(unix))]
type ZmodemTapSlot = ();
#[cfg(unix)]
type ZmodemBridgeSlot = Arc<Mutex<Option<crate::terminal::zmodem::ZmodemBridge>>>;
#[cfg(not(unix))]
type ZmodemBridgeSlot = ();
#[cfg(unix)]
type ZmodemSupportSlot = Arc<std::sync::atomic::AtomicU8>;
#[cfg(not(unix))]
type ZmodemSupportSlot = ();

impl SshSession {
    #[allow(clippy::too_many_arguments)]
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
        proxy: Option<ResolvedProxy>,
        record: bool,
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
            proxy,
            record,
        )
    }

    #[allow(clippy::too_many_arguments)]
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
        proxy: Option<ResolvedProxy>,
        record: bool,
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
        let os_hint = Arc::new(Mutex::new(None::<String>));
        let arch_hint = Arc::new(Mutex::new(None::<String>));
        let port_forwards = Arc::new(Mutex::new(Vec::<PortForward>::new()));
        let notifications = Arc::new(Mutex::new(Vec::<AppNotification>::new()));
        let metrics = Arc::new(Mutex::new(MetricsSnapshot::new()));
        let metrics_enabled = Arc::new(AtomicBool::new(false));
        #[cfg(unix)]
        let zmodem_support = Arc::new(std::sync::atomic::AtomicU8::new(0));
        #[cfg(not(unix))]
        let zmodem_support = ();
        let sftp_support = Arc::new(std::sync::atomic::AtomicU8::new(0));

        let grid_clone = Arc::clone(&grid);
        let state_clone = Arc::clone(&state);
        let alive_clone = Arc::clone(&alive);
        let shell_hint_clone = Arc::clone(&shell_hint);
        let os_hint_clone = Arc::clone(&os_hint);
        let arch_hint_clone = Arc::clone(&arch_hint);
        let port_forwards_clone = Arc::clone(&port_forwards);
        let notifications_clone = Arc::clone(&notifications);
        let metrics_clone = Arc::clone(&metrics);
        let metrics_enabled_clone = Arc::clone(&metrics_enabled);
        let auth_prompt = Arc::new(super::auth_prompt::AuthPromptBridge::new());
        let auth_prompt_clone = Arc::clone(&auth_prompt);
        let recorder = Arc::new(Mutex::new(
            if record {
                // Zero-loss auto-record: create the recorder BEFORE ssh_task
                // spawns so capture starts at the very first byte of output.
                let title = format!("{username}@{host}");
                let portal_host = if username.is_empty() {
                    format!("{host}:{port}")
                } else {
                    format!("{username}@{host}:{port}")
                };
                match crate::terminal::cast::new_recording_path(&host)
                    .and_then(|path| {
                        crate::terminal::cast::CastRecorder::create(
                            &path, cols, rows, Some(title), Some(portal_host),
                        )
                    }) {
                    Ok(rec) => Some(rec),
                    Err(e) => {
                        log::warn!("auto-record: failed to open recording file: {e}");
                        None
                    }
                }
            } else {
                None
            },
        ));
        let recorder_clone = Arc::clone(&recorder);

        // ZMODEM slots — cloned into ssh_task like the recorder.
        #[cfg(unix)]
        let zmodem_tap = Arc::new(Mutex::new(None));
        #[cfg(unix)]
        let zmodem_bridge = Arc::new(Mutex::new(None));
        #[cfg(unix)]
        let zmodem_transfer = Arc::new(Mutex::new(None));
        #[cfg(unix)]
        let zmodem_tap_task = Arc::clone(&zmodem_tap);
        #[cfg(unix)]
        let zmodem_bridge_task = Arc::clone(&zmodem_bridge);
        #[cfg(not(unix))]
        let zmodem_tap_task = ();
        #[cfg(not(unix))]
        let zmodem_bridge_task = ();
        #[cfg(unix)]
        let zmodem_support_probe = Arc::clone(&zmodem_support);
        #[cfg(not(unix))]
        let zmodem_support_probe = ();
        let sftp_support_probe = Arc::clone(&sftp_support);

        runtime.spawn(async move {
            Self::ssh_task(
                host, port, username, auth, cols, rows, grid_clone, cmd_rx,
                state_clone, alive_clone, shell_hint_clone, os_hint_clone,
                arch_hint_clone, startup_commands,
                keepalive_interval, agent_forwarding, port_forward_configs,
                port_forwards_clone, notifications_clone, metrics_clone,
                metrics_enabled_clone, auth_prompt_clone, recorder_clone,
                zmodem_tap_task, zmodem_bridge_task, zmodem_support_probe,
                sftp_support_probe, jump_host,
                proxy,
            )
            .await;
        });

        Self {
            grid,
            cmd_tx,
            state,
            shell_hint,
            os_hint,
            arch_hint,
            port_forwards,
            notifications,
            metrics,
            metrics_enabled,
            auth_prompt,
            recorder,
            #[cfg(unix)]
            zmodem_tap,
            #[cfg(unix)]
            zmodem_bridge,
            #[cfg(unix)]
            zmodem_transfer,
            #[cfg(unix)]
            zmodem_support,
            sftp_support,
        }
    }

    #[allow(clippy::too_many_arguments)]
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
        os_hint: Arc<Mutex<Option<String>>>,
        arch_hint: Arc<Mutex<Option<String>>>,
        startup_commands: Vec<String>,
        keepalive_interval: u32,
        agent_forwarding: bool,
        port_forward_configs: Vec<PortForwardConfig>,
        port_forwards: Arc<Mutex<Vec<PortForward>>>,
        notifications: Arc<Mutex<Vec<AppNotification>>>,
        metrics: Arc<Mutex<MetricsSnapshot>>,
        metrics_enabled: Arc<AtomicBool>,
        auth_bridge: Arc<super::auth_prompt::AuthPromptBridge>,
        recorder: Arc<Mutex<Option<crate::terminal::cast::CastRecorder>>>,
        zmodem_tap: ZmodemTapSlot,
        zmodem_bridge: ZmodemBridgeSlot,
        zmodem_support: ZmodemSupportSlot,
        sftp_support: Arc<std::sync::atomic::AtomicU8>,
        jump_host: Option<JumpHostInfo>,
        proxy: Option<ResolvedProxy>,
    ) {
        let set_state = |s: SshConnectionState| {
            if let Ok(mut st) = state.lock() {
                *st = s;
            }
            // Connection state changes (Connecting → Connected → Disconnected)
            // happen off the UI thread; wake it so the status/banner updates.
            crate::repaint::notify_repaint();
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
                keepalive_interval, agent_forwarding, &auth_bridge,
                super::auth::HostKeyPolicy::Learn,
                Arc::clone(&remote_fwd_arc),
            ).await {
                Ok(h) => h,
                Err(e) => {
                    set_state(SshConnectionState::Error(e));
                    alive.store(false, Ordering::Relaxed);
                    return;
                }
            }
        } else {
            // Without a jump host the proxy applies to the target leg
            // directly; with one, the target rides the jump's tunnel and
            // only the jump's own proxy (jump.proxy) matters.
            match connect_and_authenticate(
                &host, port, &username, &auth, keepalive_interval, agent_forwarding,
                &auth_bridge, Arc::clone(&remote_fwd_arc), proxy.as_ref(),
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

        // 5. Activate configured remote forwards (-R). tcpip_forward needs
        // &mut Handle, so this must run before the Arc wrap below. Incoming
        // forwarded-tcpip channels are routed via SshClient's remote_forwards
        // table (shared above). Local (-L) and dynamic (-D) forwards are
        // started on demand from the Tunnels tab.
        for cfg in port_forward_configs.iter().filter(|c| c.kind == ForwardKind::Remote) {
            match handle.tcpip_forward(&cfg.remote_host, cfg.remote_port as u32).await {
                Ok(allocated) => {
                    // RFC 4254: a reply to a specific-port request carries no
                    // port — russh surfaces that as 0, meaning "the port you
                    // asked for". A port-0 request gets the assigned port back.
                    let bound_port = if allocated == 0 { cfg.remote_port as u32 } else { allocated };
                    log::info!(
                        "Remote forward active: {} (port {}) -> {}:{}",
                        cfg.remote_host, bound_port, cfg.local_host, cfg.local_port
                    );
                    let (pf, _cancel_rx) = PortForward::new(cfg.clone());
                    if let Ok(mut ap) = pf.allocated_port.lock() {
                        *ap = Some(bound_port);
                    }
                    if let Ok(mut s) = pf.state.lock() {
                        *s = ForwardState::Active;
                    }
                    if let Ok(mut fwds) = port_forwards.lock() {
                        fwds.push(pf);
                    }
                }
                Err(e) => {
                    log::warn!(
                        "Remote forward request for {}:{} failed: {}",
                        cfg.remote_host, cfg.remote_port, e
                    );
                    let (pf, _cancel_rx) = PortForward::new(cfg.clone());
                    if let Ok(mut s) = pf.state.lock() {
                        *s = ForwardState::Error(format!("tcpip_forward failed: {}", e));
                    }
                    if let Ok(mut fwds) = port_forwards.lock() {
                        fwds.push(pf);
                    }
                }
            }
        }

        let handle = Arc::new(handle);

        // Probe remote OS and architecture (best-effort; failure only means the
        // AI system prompt degrades to "unknown", never that connection fails).
        // Done after Arc wrapping so we can reuse `exec_command(&handle, ..)`.
        if let Ok(os) = exec_command(&handle, "uname -s").await {
            let os = os.trim().to_string();
            if !os.is_empty() {
                if let Ok(mut h) = os_hint.lock() { *h = Some(os); }
            }
        }
        if let Ok(arch) = exec_command(&handle, "uname -m").await {
            let arch = arch.trim().to_string();
            if !arch.is_empty() {
                if let Ok(mut h) = arch_hint.lock() { *h = Some(arch); }
            }
        }

        // ZMODEM support: both sz and rz must be on the remote PATH.
        // `command -v` is POSIX; `echo yes` marks success because
        // exec_command only returns stdout. Exec failing (hardened servers
        // may disable it) leaves Unknown — the interactive shell may still
        // run sz, so a probe error must never claim Missing.
        #[cfg(unix)]
        if let Ok(out) = exec_command(
            &handle,
            "command -v sz >/dev/null 2>&1 && command -v rz >/dev/null 2>&1 && echo yes",
        )
        .await
        {
            let support = if out.trim() == "yes" {
                crate::terminal::zmodem::ZmodemSupport::Available
            } else {
                crate::terminal::zmodem::ZmodemSupport::Missing
            };
            zmodem_support.store(support.to_u8(), Ordering::Relaxed);
            // The context menu greys out once this lands — wake the UI.
            crate::repaint::notify_repaint();
        }

        // SFTP subsystem support: open a throwaway session channel and
        // ask for the subsystem — the server's Success/Failure reply is
        // the whole answer. Probe failure (channel never answered) leaves
        // Unknown: never claim Missing on our own transport trouble.
        // Spawned, not awaited: a server that never answers a subsystem
        // request (the 10 s timeout only bounds this task) must not delay
        // ssh_task's entry into its select loop.
        let probe_handle = Arc::clone(&handle);
        let probe_sftp_support = Arc::clone(&sftp_support);
        tokio::spawn(async move {
            let Ok(mut channel) = probe_handle.channel_open_session().await else {
                return;
            };
            let verdict = if channel.request_subsystem(true, "sftp").await.is_err() {
                Some(false)
            } else {
                match tokio::time::timeout(
                    std::time::Duration::from_secs(10),
                    channel.wait(),
                )
                .await
                {
                    Ok(Some(russh::ChannelMsg::Success)) => Some(true),
                    Ok(Some(russh::ChannelMsg::Failure)) => Some(false),
                    _ => None,
                }
            };
            let _ = channel.close().await;
            if let Some(ok) = verdict {
                let cap = if ok {
                    crate::ssh::capability::HostCapability::Available
                } else {
                    crate::ssh::capability::HostCapability::Missing
                };
                probe_sftp_support.store(cap.to_u8(), Ordering::Relaxed);
                // The hosts-list badge appears once this lands.
                crate::repaint::notify_repaint();
            }
        });

        // Local forwards are no longer auto-started on connect — use the
        // Tunnels tab (Start button → start_port_forward) instead.
        // The loop that used to spawn_local_forward here has been removed.

        // 5b. Metrics collection loop (spawned, non-blocking).
        // Gated by `metrics_enabled` — the UI only raises it while the
        // Metrics tab of the tools drawer is visible. A disabled session
        // costs zero remote execs: no channel opens, no traffic competing
        // with interactive I/O (a standing 5 s exec loop head-of-line
        // blocks a slow link and makes the terminal feel stuck). The 1 s
        // tick keeps enable→first-sample latency ~1 s at no network cost.
        let metrics_handle = Arc::clone(&handle);
        let metrics_arc = Arc::clone(&metrics);
        let metrics_gate = Arc::clone(&metrics_enabled);
        tokio::spawn(async move {
            let mut last_poll: Option<std::time::Instant> = None;
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                if !metrics_gate.load(Ordering::Relaxed) {
                    continue;
                }
                if last_poll.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(5)) {
                    continue;
                }
                last_poll = Some(std::time::Instant::now());
                let cmd = "echo =M=; grep -E 'MemTotal|MemAvailable' /proc/meminfo; \
                           echo =N=; cat /proc/net/dev; echo =L=; cat /proc/loadavg; \
                           echo =C=; head -n1 /proc/stat";
                if let Ok(out) = exec_command(&metrics_handle, cmd).await {
                    if let Ok(mut snap) = metrics_arc.lock() {
                        crate::terminal::metrics::parse_remote(&out, &mut snap);
                    }
                    // Metrics snapshots change independently of terminal
                    // output; wake the UI so the drawer keeps animating.
                    crate::repaint::notify_repaint();
                }
            }
        });

        // 6. Main I/O loop
        let mut parser = vte::Parser::new();
        let mut attrs = CellAttrs::default();

        loop {
            tokio::select! {
                // Periodic health check: if no activity for 30s, the keepalive
                // should have caught a dead connection. If we still haven't
                // received anything, the transport is likely stuck.
                _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
                    // Keepalive handles transport liveness. This branch just
                    // ensures the select! doesn't block indefinitely if both
                    // channel.wait() and cmd_rx are idle.
                    if !alive.load(Ordering::Relaxed) {
                        break;
                    }
                }
                msg = channel.wait() => {
                    match msg {
                        Some(russh::ChannelMsg::Data { data }) => {
                            // ZMODEM diversion must run BEFORE the VTE parse
                            // and the asciicast tap so binary protocol frames
                            // never reach scrollback or the recording.
                            // NOTE: the emit closure is the ONLY render path
                            // here — on diversion it may receive just the
                            // non-protocol prefix, so this chunk must never
                            // be parsed again after route_chunk returns.
                            #[cfg(unix)]
                            {
                                let diverted = crate::terminal::zmodem::route_chunk(
                                    &zmodem_tap,
                                    &zmodem_bridge,
                                    &data,
                                    |bytes| {
                                        if let Ok(mut g) = grid.lock() {
                                            let mut handler = VteHandler {
                                                grid: &mut g,
                                                attrs: &mut attrs,
                                            };
                                            for byte in bytes {
                                                parser.advance(&mut handler, *byte);
                                            }
                                        }
                                        // Asciicast tap (outside the grid lock).
                                        if let Ok(mut slot) = recorder.lock() {
                                            if let Some(rec) = slot.as_mut() {
                                                rec.output(bytes);
                                            }
                                        }
                                    },
                                );
                                if diverted
                                    == crate::terminal::zmodem::ChunkRoute::Diverted
                                {
                                    crate::repaint::notify_repaint();
                                    continue;
                                }
                            }
                            #[cfg(not(unix))]
                            {
                                // No ZMODEM routing on this platform.
                                if let Ok(mut g) = grid.lock() {
                                    let mut handler = VteHandler {
                                        grid: &mut g,
                                        attrs: &mut attrs,
                                    };
                                    for byte in &*data {
                                        parser.advance(&mut handler, *byte);
                                    }
                                }
                                if let Ok(mut slot) = recorder.lock() {
                                    if let Some(rec) = slot.as_mut() {
                                        rec.output(&data);
                                    }
                                }
                            }
                            crate::repaint::notify_repaint();
                        }
                        Some(russh::ChannelMsg::ExtendedData { data, .. }) => {
                            // ZMODEM diversion (same ordering and single-render
                            // rule as the Data arm above).
                            #[cfg(unix)]
                            {
                                let diverted = crate::terminal::zmodem::route_chunk(
                                    &zmodem_tap,
                                    &zmodem_bridge,
                                    &data,
                                    |bytes| {
                                        if let Ok(mut g) = grid.lock() {
                                            let mut handler = VteHandler {
                                                grid: &mut g,
                                                attrs: &mut attrs,
                                            };
                                            for byte in bytes {
                                                parser.advance(&mut handler, *byte);
                                            }
                                        }
                                        // Asciicast tap (stderr merged into the
                                        // same stream).
                                        if let Ok(mut slot) = recorder.lock() {
                                            if let Some(rec) = slot.as_mut() {
                                                rec.output(bytes);
                                            }
                                        }
                                    },
                                );
                                if diverted
                                    == crate::terminal::zmodem::ChunkRoute::Diverted
                                {
                                    crate::repaint::notify_repaint();
                                    continue;
                                }
                            }
                            #[cfg(not(unix))]
                            {
                                // No ZMODEM routing on this platform.
                                if let Ok(mut g) = grid.lock() {
                                    let mut handler = VteHandler {
                                        grid: &mut g,
                                        attrs: &mut attrs,
                                    };
                                    for byte in &*data {
                                        parser.advance(&mut handler, *byte);
                                    }
                                }
                                if let Ok(mut slot) = recorder.lock() {
                                    if let Some(rec) = slot.as_mut() {
                                        rec.output(&data);
                                    }
                                }
                            }
                            crate::repaint::notify_repaint();
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
                            if let Err(e) = channel.data(&data[..]).await {
                                log::warn!("SSH write failed, connection lost: {}", e);
                                set_state(SshConnectionState::Disconnected("Write failed".into()));
                                alive.store(false, Ordering::Relaxed);
                                break;
                            }
                        }
                        Some(SshCommand::Resize { cols, rows }) => {
                            if let Err(e) = channel.window_change(cols, rows, 0, 0).await {
                                log::warn!("SSH resize failed, connection lost: {}", e);
                                set_state(SshConnectionState::Disconnected("Resize failed".into()));
                                alive.store(false, Ordering::Relaxed);
                                break;
                            }
                            if let Ok(mut g) = grid.lock() {
                                g.resize(cols as usize, rows as usize);
                            }
                        }
                        Some(SshCommand::StartPortForward(cfg)) => {
                            match cfg.kind {
                                ForwardKind::Local | ForwardKind::Dynamic => {
                                    Self::spawn_forward(
                                        Arc::clone(&handle),
                                        cfg,
                                        &port_forwards,
                                        &notifications,
                                    );
                                }
                                ForwardKind::Remote => {
                                    // tcpip_forward needs &mut Handle, gone after Arc
                                    // wrapping — remote forwards activate at connect time.
                                    log::warn!("Remote forward cannot start after connection (requires &mut Handle)");
                                    let (pf, _cancel_rx) = PortForward::new(cfg);
                                    if let Ok(mut s) = pf.state.lock() {
                                        *s = ForwardState::Error(
                                            "Remote forwards are set up when connecting — reconnect to change them".into(),
                                        );
                                    }
                                    if let Ok(mut fwds) = port_forwards.lock() {
                                        fwds.push(pf);
                                    }
                                }
                            }
                        }
                        Some(SshCommand::StopPortForward(cfg)) => {
                            // Find matching port forward by full config (not just index)
                            let mut stopped_index = None;
                            let mut remote_cancel: Option<(String, u32)> = None;
                            if let Ok(mut fwds) = port_forwards.lock() {
                                for (idx, pf) in fwds.iter().enumerate() {
                                    if pf.config == cfg {
                                        pf.stop();
                                        stopped_index = Some(idx);
                                        if cfg.kind == ForwardKind::Remote {
                                            // cancel_tcpip_forward takes &self, so the
                                            // Arc<Handle> works — tell the server to stop
                                            // listening. Await happens after the lock drops.
                                            if let Ok(ap) = pf.allocated_port.lock() {
                                                if let Some(port) = *ap {
                                                    remote_cancel = Some((cfg.remote_host.clone(), port));
                                                }
                                            }
                                        }
                                        break;
                                    }
                                }

                                // Clean up: remove stopped forwards from the Vec
                                if let Some(idx) = stopped_index {
                                    fwds.remove(idx);
                                }
                            }
                            if let Some((host, port)) = remote_cancel {
                                match handle.cancel_tcpip_forward(&host, port).await {
                                    Ok(_) => log::info!(
                                        "Remote forward cancelled on server: {}:{}",
                                        host, port
                                    ),
                                    Err(e) => log::warn!(
                                        "cancel_tcpip_forward for {}:{} failed: {}",
                                        host, port, e
                                    ),
                                }
                            }
                        }
                        Some(SshCommand::Disconnect) | None => {
                            // Stop all port forwards and clean up the Vec
                            if let Ok(mut fwds) = port_forwards.lock() {
                                for pf in fwds.iter() {
                                    pf.stop();
                                }
                                fwds.clear();
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

        // ── Release bound ports on every exit path ──
        // The main loop `break`s on organic disconnects (remote Eof/Close,
        // write/resize failure, cmd channel closed) as well as the explicit
        // `Disconnect` command. Each local forward holds a bound TcpListener
        // in a spawned task until its cancel signal fires, so leaving this
        // cleanup only in the `Disconnect` arm leaked the bound ports after
        // an abrupt disconnect. Stop and clear them unconditionally here so
        // any session teardown frees the listener.
        if let Ok(mut fwds) = port_forwards.lock() {
            for pf in fwds.iter() {
                pf.stop();
            }
            fwds.clear();
        }
    }

    /// Spawn a local port forward task and track it in port_forwards.
    fn spawn_forward(
        handle: Arc<russh::client::Handle<SshClient>>,
        cfg: PortForwardConfig,
        port_forwards: &Arc<Mutex<Vec<PortForward>>>,
        notifications: &Arc<Mutex<Vec<AppNotification>>>,
    ) {
        let (pf, cancel_rx) = PortForward::new(cfg.clone());
        let fwd_state = Arc::clone(&pf.state);

        if let Ok(mut fwds) = port_forwards.lock() {
            fwds.push(pf);
        }

        let notifications = Arc::clone(notifications);
        tokio::spawn(async move {
            match cfg.kind {
                ForwardKind::Dynamic => {
                    start_dynamic_forward(handle, cfg, fwd_state, cancel_rx, notifications).await;
                }
                _ => {
                    start_local_forward(handle, cfg, fwd_state, cancel_rx, notifications).await;
                }
            }
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

    /// ZMODEM support on the remote host (Unknown until the probe lands).
    #[cfg(unix)]
    pub fn zmodem_support(&self) -> crate::terminal::zmodem::ZmodemSupport {
        crate::terminal::zmodem::ZmodemSupport::from_u8(
            self.zmodem_support
                .load(std::sync::atomic::Ordering::Relaxed),
        )
    }

    /// SFTP subsystem support on the remote host (Unknown until probed).
    pub fn sftp_support(&self) -> crate::ssh::capability::HostCapability {
        crate::ssh::capability::HostCapability::from_u8(
            self.sftp_support
                .load(std::sync::atomic::Ordering::Relaxed),
        )
    }

    /// Enable/disable remote metrics polling. Called by the UI every
    /// frame — enabled only while the Metrics tab of the tools drawer is
    /// the visible view for this session.
    pub fn set_metrics_enabled(&self, enabled: bool) {
        self.metrics_enabled.store(enabled, Ordering::Relaxed);
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
    pub fn stop_port_forward(&self, config: PortForwardConfig) {
        let _ = self.cmd_tx.send(SshCommand::StopPortForward(config));
    }

    /// Get the current list of port forward states
    #[allow(dead_code)]
    pub fn get_port_forward_states(&self) -> Vec<(PortForwardConfig, ForwardState)> {
        self.port_forwards.lock().ok().map(|fwds| {
            fwds.iter().map(|pf| (pf.config.clone(), pf.current_state())).collect()
        }).unwrap_or_default()
    }

    /// Drain pending notifications from the session
    pub fn drain_notifications(&self) -> Vec<AppNotification> {
        self.notifications.lock()
            .map(|mut ns| std::mem::take(&mut *ns))
            .unwrap_or_default()
    }

    /// Record a resize event into the active asciicast, if any.
    pub fn record_resize(&self, cols: u16, rows: u16) {
        if let Ok(mut slot) = self.recorder.lock() {
            if let Some(rec) = slot.as_mut() {
                rec.resize(cols, rows);
            }
        }
    }

    // ── ZMODEM (unix) ────────────────────────────────────────────────────

    /// Sink for the ZMODEM engine: protocol frames go through the command
    /// channel as writes, landing in THIS session's shell channel only
    /// (broadcast mode never sees them).
    #[cfg(unix)]
    fn make_zmodem_sink(&self) -> std::io::Result<crate::terminal::zmodem::SessionWriter> {
        let tx = self.cmd_tx.clone();
        Ok(crate::terminal::zmodem::SessionWriter::new(move |data| {
            tx.send(SshCommand::Write(data.to_vec())).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "ssh command channel closed",
                )
            })
        }))
    }

    /// Start a ZMODEM upload of `path` to the remote host. Arms the tap (all
    /// output diverts) and spawns the transfer engine, which types `rz\r`
    /// into the remote shell itself and runs the ZRQINIT → ZRINIT handshake.
    #[cfg(unix)]
    pub fn start_zmodem_upload(&self, path: std::path::PathBuf) -> std::io::Result<()> {
        use crate::terminal::zmodem::{TransferRequest, ZmodemBridge};
        self.cancel_zmodem();
        let sink = self.make_zmodem_sink()?;
        let mut tap = crate::terminal::zmodem::ZmodemTap::new();
        let held = tap.arm();
        *self
            .zmodem_tap
            .lock()
            .map_err(|_| std::io::Error::other("zmodem tap lock poisoned"))? = Some(tap);
        let mut bridge = ZmodemBridge::spawn(
            TransferRequest::Upload { path },
            sink,
            Arc::clone(&self.zmodem_transfer),
        );
        if !held.is_empty() {
            bridge.feed(&held);
        }
        *self
            .zmodem_bridge
            .lock()
            .map_err(|_| std::io::Error::other("zmodem bridge lock poisoned"))? = Some(bridge);
        Ok(())
    }

    /// Start a ZMODEM download of remote `remote_path` into `local_dir`:
    /// arm a passive tap, spawn a waiting bridge, then type `sz '<path>'`
    /// so the remote side announces itself and the tap triggers the engine
    /// on its first frame.
    #[cfg(unix)]
    pub fn start_zmodem_download(
        &self,
        remote_path: &str,
        local_dir: std::path::PathBuf,
    ) -> std::io::Result<()> {
        use crate::terminal::zmodem::{shell_quote_single, TransferRequest, ZmodemBridge};
        self.cancel_zmodem();
        let sink = self.make_zmodem_sink()?;
        *self
            .zmodem_tap
            .lock()
            .map_err(|_| std::io::Error::other("zmodem tap lock poisoned"))? =
            Some(crate::terminal::zmodem::ZmodemTap::new());
        let bridge = ZmodemBridge::spawn(
            TransferRequest::Download {
                output_dir: local_dir,
            },
            sink,
            Arc::clone(&self.zmodem_transfer),
        );
        *self
            .zmodem_bridge
            .lock()
            .map_err(|_| std::io::Error::other("zmodem bridge lock poisoned"))? = Some(bridge);
        self.write(format!("sz {}\r", shell_quote_single(remote_path)).as_bytes())
    }

    /// Abort any ZMODEM transfer on this session (no-op when idle).
    #[cfg(unix)]
    pub fn cancel_zmodem(&self) {
        if let Ok(mut b) = self.zmodem_bridge.lock() {
            if let Some(bridge) = b.as_mut() {
                bridge.cancel();
            }
            *b = None;
        }
        if let Ok(mut t) = self.zmodem_tap.lock() {
            if let Some(tap) = t.as_mut() {
                tap.reset();
            }
        }
        // The transfer slot keeps its final state for the status chip until
        // the next transfer replaces it.
    }

    /// Bytes transferred so far by the active bridge (0 when idle).
    #[cfg(unix)]
    pub fn zmodem_bytes(&self) -> u64 {
        self.zmodem_bridge
            .lock()
            .ok()
            .and_then(|b| b.as_ref().map(|bridge| bridge.bytes()))
            .unwrap_or(0)
    }
}

impl Drop for SshSession {
    fn drop(&mut self) {
        // Wake a task parked on a 2FA prompt before tearing down the
        // transport — auth-stage tasks don't read the Disconnect command.
        self.auth_prompt.cancel();
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
    proxy: Option<ResolvedProxy>,
) -> Result<String, String> {
    // The test dialog has no UI for OTP entry: refuse prompts so 2FA
    // hosts report an actionable error instead of hanging.
    let _handle = connect_and_authenticate(
        &host, port, &username, &auth, keepalive_interval, agent_forwarding,
        &super::auth_prompt::AuthPromptBridge::decline(),
        // The test dialog never forwards ports.
        Arc::new(Mutex::new(Vec::new())),
        proxy.as_ref(),
    ).await?;
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
            clean_pattern == host || host_pattern == expected
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
