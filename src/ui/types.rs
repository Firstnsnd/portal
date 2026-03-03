use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::config::HostEntry;
use crate::ssh::{SshSession, SshConnectionState};
use crate::terminal::{TerminalGrid, RealPtySession};

/// Unified session backend: local PTY or SSH
pub enum SessionBackend {
    Local(RealPtySession),
    Ssh(SshSession),
}

impl SessionBackend {
    pub fn write(&mut self, data: &[u8]) -> std::io::Result<()> {
        match self {
            SessionBackend::Local(s) => s.write(data),
            SessionBackend::Ssh(s) => s.write(data),
        }
    }

    pub fn get_grid(&self) -> Arc<Mutex<TerminalGrid>> {
        match self {
            SessionBackend::Local(s) => s.get_grid(),
            SessionBackend::Ssh(s) => s.get_grid(),
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> std::io::Result<()> {
        match self {
            SessionBackend::Local(s) => s.resize(cols, rows),
            SessionBackend::Ssh(s) => s.resize(cols, rows),
        }
    }

    pub fn is_connected(&self) -> bool {
        match self {
            SessionBackend::Local(_) => true,
            SessionBackend::Ssh(s) => matches!(s.connection_state(), SshConnectionState::Connected),
        }
    }
}

/// Add Host dialog state
pub struct AddHostDialog {
    pub open: bool,
    pub edit_index: Option<usize>,
    pub name: String,
    pub host: String,
    pub port: String,
    pub username: String,
    pub group: String,
    pub error: String,
    pub auth_method: AuthMethodChoice,
    pub password: String,
    pub key_path: String,
    pub key_passphrase: String,
    pub key_in_keychain: bool,
    pub test_conn_state: TestConnState,
    pub test_conn_result: Option<Arc<Mutex<Option<Result<String, String>>>>>,
}

impl Default for AddHostDialog {
    fn default() -> Self {
        Self {
            open: false,
            edit_index: None,
            name: String::new(),
            host: String::new(),
            port: "22".to_owned(),
            username: String::new(),
            group: String::new(),
            error: String::new(),
            auth_method: AuthMethodChoice::Password,
            password: String::new(),
            key_path: String::new(),
            key_passphrase: String::new(),
            key_in_keychain: false,
            test_conn_state: TestConnState::Idle,
            test_conn_result: None,
        }
    }
}

impl AddHostDialog {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn open_new(&mut self) {
        self.reset();
        self.open = true;
    }

    pub fn open_edit(&mut self, index: usize, host: &HostEntry) {
        self.open = true;
        self.edit_index = Some(index);
        self.name = host.name.clone();
        self.host = host.host.clone();
        self.port = host.port.to_string();
        self.username = host.username.clone();
        self.group = host.group.clone();
        self.error = String::new();
        match &host.auth {
            crate::config::AuthMethod::Password { password } => {
                self.auth_method = AuthMethodChoice::Password;
                self.password = password.clone();
            }
            crate::config::AuthMethod::Key { key_path, passphrase, key_in_keychain } => {
                self.auth_method = AuthMethodChoice::Key;
                self.key_path = key_path.clone();
                self.key_passphrase = passphrase.clone();
                self.key_in_keychain = *key_in_keychain;
            }
            crate::config::AuthMethod::None => {
                self.auth_method = AuthMethodChoice::Password;
            }
        }
    }
}

/// Text selection state in terminal
#[derive(Default, Clone)]
pub struct Selection {
    /// Whether a drag is in progress
    pub active: bool,
    /// Start position (row, col) — where mouse was pressed
    pub start: (usize, usize),
    /// End position (row, col) — where mouse currently is
    pub end: (usize, usize),
}

impl Selection {
    pub fn has_selection(&self) -> bool {
        self.start != self.end
    }

    /// Returns (start, end) in normalized order (start <= end)
    pub fn ordered(&self) -> ((usize, usize), (usize, usize)) {
        if self.start.0 < self.end.0
            || (self.start.0 == self.end.0 && self.start.1 <= self.end.1)
        {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        }
    }

    pub fn clear(&mut self) {
        self.active = false;
        self.start = (0, 0);
        self.end = (0, 0);
    }
}

/// Terminal session
pub struct TerminalSession {
    pub session: Option<SessionBackend>,
    pub grid: Arc<Mutex<TerminalGrid>>,
    pub last_cols: usize,
    pub last_rows: usize,
    /// Scroll offset: 0 = bottom (latest), >0 = scrolled up by N lines
    pub scroll_offset: usize,
    /// Saved host config for SSH reconnection
    pub ssh_host: Option<HostEntry>,
    /// Per-session text selection
    pub selection: Selection,
    /// Shell path used for local sessions (e.g. "/bin/zsh")
    pub local_shell: String,
    /// When this session was created
    pub created_at: Instant,
}

impl TerminalSession {
    pub fn new_local(id: usize, shell: &str) -> Self {
        let session = RealPtySession::new(id, 80, 24, shell).ok().map(SessionBackend::Local);
        let grid = session.as_ref().map(|s| s.get_grid()).unwrap_or_else(|| {
            Arc::new(Mutex::new(TerminalGrid::new(80, 24)))
        });

        Self {
            session,
            grid,
            last_cols: 80,
            last_rows: 24,
            scroll_offset: 0,
            ssh_host: None,
            selection: Selection::default(),
            local_shell: shell.to_string(),
            created_at: Instant::now(),
        }
    }

    pub fn new_ssh(host: &HostEntry, runtime: &tokio::runtime::Runtime) -> Self {
        let ssh = SshSession::connect(
            runtime,
            host.host.clone(),
            host.port,
            host.username.clone(),
            host.auth.clone(),
            host.name.clone(),
            80,
            24,
        );
        let grid = ssh.get_grid();
        Self {
            session: Some(SessionBackend::Ssh(ssh)),
            grid,
            last_cols: 80,
            last_rows: 24,
            scroll_offset: 0,
            ssh_host: Some(host.clone()),
            selection: Selection::default(),
            local_shell: String::new(),
            created_at: Instant::now(),
        }
    }

    /// Shell display name for this session
    pub fn shell_name(&self) -> String {
        match &self.session {
            Some(SessionBackend::Local(_)) => {
                self.local_shell.rsplit('/').next().unwrap_or("shell").to_string()
            }
            Some(SessionBackend::Ssh(ssh)) => {
                ssh.get_shell_hint()
                    .as_deref()
                    .and_then(|p| p.rsplit('/').next().map(|s| s.to_string()))
                    .unwrap_or_else(|| "…".to_string())
            }
            None => "—".to_string(),
        }
    }

    /// Reconnect a disconnected SSH session
    pub fn reconnect_ssh(&mut self, runtime: &tokio::runtime::Runtime) {
        if let Some(ref host) = self.ssh_host {
            let ssh = SshSession::connect(
                runtime,
                host.host.clone(),
                host.port,
                host.username.clone(),
                host.auth.clone(),
                host.name.clone(),
                self.last_cols as u16,
                self.last_rows as u16,
            );
            self.grid = ssh.get_grid();
            self.session = Some(SessionBackend::Ssh(ssh));
            self.scroll_offset = 0;
        }
    }

    /// Check if this is a disconnected SSH session that can reconnect
    pub fn needs_reconnect(&self) -> bool {
        if self.ssh_host.is_none() {
            return false;
        }
        match &self.session {
            Some(SessionBackend::Ssh(ssh)) => matches!(
                ssh.connection_state(),
                SshConnectionState::Disconnected(_) | SshConnectionState::Error(_)
            ),
            None => true,
            _ => false,
        }
    }

    pub fn write(&mut self, data: &str) {
        if let Some(ref mut session) = self.session {
            let _ = session.write(data.as_bytes());
        }
    }

    pub fn write_bytes(&mut self, data: &[u8]) {
        if let Some(ref mut session) = self.session {
            let _ = session.write(data);
        }
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        if cols != self.last_cols || rows != self.last_rows {
            self.last_cols = cols;
            self.last_rows = rows;
            if let Some(ref mut session) = self.session {
                let _ = session.resize(cols as u16, rows as u16);
            }
            // Safety net: directly resize the grid in case the backend resize
            // failed (e.g. PTY ioctl error causing early return before grid resize)
            if let Ok(mut grid) = self.grid.lock() {
                if grid.cols != cols || grid.rows != rows {
                    grid.resize(cols, rows);
                }
            }
        }
    }
}

/// SFTP right-click context menu state (supports multi-selection)
pub struct SftpContextMenu {
    pub pos: egui::Pos2,
    pub is_local: bool,
    pub entry_indices: Vec<usize>,
    pub entry_names: Vec<String>,
    pub all_dirs: bool,
    pub any_dirs: bool,
}

/// SFTP rename dialog state
pub struct SftpRenameDialog {
    pub is_local: bool,
    pub old_name: String,
    pub new_name: String,
    pub error: String,
}

/// SFTP new folder dialog state
pub struct SftpNewFolderDialog {
    pub is_local: bool,
    pub name: String,
    pub error: String,
}

/// SFTP new file dialog state
pub struct SftpNewFileDialog {
    pub is_local: bool,
    pub name: String,
    pub error: String,
}

/// SFTP delete confirmation dialog state (supports multi-file delete)
pub struct SftpConfirmDelete {
    pub is_local: bool,
    pub names: Vec<String>,
}

/// SFTP editor dialog state
pub struct SftpEditorDialog {
    pub is_local: bool,
    pub file_path: String,
    pub file_name: String,
    pub directory: String,
    pub content: String,
    pub original_content: String,
    pub loading: bool,
    pub is_new_file: bool,
    pub error: String,
    pub save_as_name: String,
}

/// Auth method choice for the dialog
#[derive(Default, PartialEq, Clone)]
pub enum AuthMethodChoice {
    #[default]
    Password,
    Key,
}

/// Test connection state shown in the Add Host dialog
#[derive(Default)]
pub enum TestConnState {
    #[default]
    Idle,
    Testing,
    Success(String),
    Failed(String),
}

/// Current view shown in the main area
#[derive(PartialEq, Clone, Copy)]
pub enum AppView {
    Hosts,
    Terminal,
    Sftp,
    Keychain,
    Settings,
}

/// Pending keychain credential deletion (awaiting user confirmation).
pub enum KeychainDeleteRequest {
    /// Delete a single credential: (host_index, kind: "password" | "privatekey" | "passphrase")
    Single { host_index: usize, kind: String },
    /// Delete all credentials
    All,
}

/// Load shells from /etc/shells (Unix), filtering to executables.
pub fn load_available_shells() -> Vec<String> {
    let mut shells = vec![];
    if let Ok(content) = std::fs::read_to_string("/etc/shells") {
        for line in content.lines() {
            let line = line.trim();
            if !line.is_empty() && !line.starts_with('#') && std::path::Path::new(line).exists() {
                shells.push(line.to_string());
            }
        }
    }
    if shells.is_empty() {
        shells.push("/bin/bash".to_string());
    }
    shells
}
