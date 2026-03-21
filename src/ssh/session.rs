//! SSH session management using russh

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use tokio::sync::mpsc;

use crate::terminal::{CellAttrs, TerminalGrid, VteHandler};
use crate::config::ResolvedAuth;

/// Commands sent from the synchronous GUI thread to the async SSH task
enum SshCommand {
    Write(Vec<u8>),
    Resize { cols: u32, rows: u32 },
    Disconnect,
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
}

impl SshClient {
    pub fn new(host: &str, port: u16) -> Self {
        Self {
            host: host.to_string(),
            port,
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
}

/// Connect and authenticate an SSH session, returning the handle.
/// Shared by SshSession, test_connection, and SFTP.
pub async fn connect_and_authenticate(
    host: &str,
    port: u16,
    username: &str,
    auth: &ResolvedAuth,
) -> Result<russh::client::Handle<SshClient>, String> {
    let config = Arc::new(russh::client::Config::default());
    let addr = format!("{}:{}", host, port);

    let mut handle = russh::client::connect(config, &addr, SshClient::new(host, port))
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

/// SSH session that mirrors RealPtySession's interface
pub struct SshSession {
    pub grid: Arc<Mutex<TerminalGrid>>,
    cmd_tx: mpsc::UnboundedSender<SshCommand>,
    pub state: Arc<Mutex<SshConnectionState>>,
    /// Remote shell path detected via exec channel (e.g. "/bin/zsh")
    pub shell_hint: Arc<Mutex<Option<String>>>,
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

        let grid_clone = Arc::clone(&grid);
        let state_clone = Arc::clone(&state);
        let alive_clone = Arc::clone(&alive);
        let shell_hint_clone = Arc::clone(&shell_hint);

        runtime.spawn(async move {
            Self::ssh_task(
                host, port, username, auth, cols, rows, grid_clone, cmd_rx,
                state_clone, alive_clone, shell_hint_clone, startup_commands,
            )
            .await;
        });

        Self {
            grid,
            cmd_tx,
            state,
            shell_hint,
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
    ) {
        let set_state = |s: SshConnectionState| {
            if let Ok(mut st) = state.lock() {
                *st = s;
            }
        };

        // 1. Connect + Authenticate using shared helper
        set_state(SshConnectionState::Authenticating);

        let handle = match connect_and_authenticate(&host, port, &username, &auth).await {
            Ok(h) => h,
            Err(e) => {
                set_state(SshConnectionState::Error(e));
                alive.store(false, Ordering::Relaxed);
                return;
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

        // 4. Main I/O loop
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
                        Some(SshCommand::Disconnect) | None => {
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
) -> Result<String, String> {
    let _handle = connect_and_authenticate(&host, port, &username, &auth).await?;
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
            let expected = if port == 22 {
                format!("[{}]:{}", host, port)
            } else {
                format!("[{}]:{}", host, port)
            };
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
