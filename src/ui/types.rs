use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::config::HostEntry;
use crate::ssh::{SshSession, SshConnectionState};
use crate::terminal::{TerminalGrid, RealPtySession};

/// Broadcast state - for Tab-internal continuous interactive batch operations
/// When enabled, input is automatically synced to all panes in current tab
#[derive(Default, Clone)]
pub struct BroadcastState {
    pub enabled: bool,
}

impl BroadcastState {
    #[allow(dead_code)]
    pub fn toggle(&mut self) {
        self.enabled = !self.enabled;
    }

    #[allow(dead_code)]
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    #[allow(dead_code)]
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    pub fn is_active(&self) -> bool {
        self.enabled
    }
}

/// Batch execution state for cross-tab one-time command distribution
#[derive(Default)]
pub struct BatchExecutionState {
    /// Show batch execution panel (legacy, kept for compatibility)
    pub show_panel: bool,
    /// Show hosts drawer (for selecting execution targets)
    pub show_hosts_drawer: bool,
    /// Target sessions (global session IDs)
    pub targets: Vec<BatchTarget>,
    /// Command to execute
    pub command: String,
    /// Command history
    pub command_history: Vec<String>,
    /// Execution results
    pub results: Vec<BatchResult>,
    /// Currently executing
    pub executing: bool,
    /// Show command history dropdown
    pub show_history: bool,
    /// Expanded result indices (for showing detailed output)
    pub expanded_results: Vec<usize>,
    /// Result receiver for async execution updates (not serialized)
    pub result_rx: Option<std::sync::mpsc::Receiver<BatchUpdate>>,
}

/// Update message from async batch execution
pub enum BatchUpdate {
    StatusChanged { index: usize, status: BatchStatus },
    Output { index: usize, output: String },
}

impl Clone for BatchExecutionState {
    fn clone(&self) -> Self {
        Self {
            show_panel: self.show_panel,
            show_hosts_drawer: self.show_hosts_drawer,
            targets: self.targets.clone(),
            command: self.command.clone(),
            command_history: self.command_history.clone(),
            results: self.results.clone(),
            executing: self.executing,
            show_history: self.show_history,
            expanded_results: self.expanded_results.clone(),
            result_rx: None, // Cannot clone receiver
        }
    }
}

/// A target session for batch execution
#[derive(Clone)]
pub struct BatchTarget {
    pub tab_idx: usize,
    pub session_idx: usize,
    #[allow(dead_code)]
    pub global_id: usize,
    pub name: String,
}

/// Execution result for a single target
#[derive(Clone)]
pub struct BatchResult {
    pub target: BatchTarget,
    pub status: BatchStatus,
    pub output: String,
    pub timestamp: Instant,
}

#[derive(Clone, PartialEq)]
pub enum BatchStatus {
    Pending,
    Running,
    Success,
    Failed(String),
}

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
    pub tags: String,
    pub error: String,
    pub auth_method: AuthMethodChoice,
    pub password: String,
    pub key_path: String,
    pub key_content: String,
    pub key_source: KeySourceChoice,
    pub key_passphrase: String,
    pub key_in_keychain: bool,
    pub test_conn_state: TestConnState,
    pub test_conn_result: Option<Arc<Mutex<Option<Result<String, String>>>>>,
}

/// Key source choice for SSH key authentication
#[derive(Default, PartialEq, Clone, Copy)]
pub enum KeySourceChoice {
    #[default]
    LocalFile,
    ImportContent,
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
            tags: String::new(),
            error: String::new(),
            auth_method: AuthMethodChoice::Password,
            password: String::new(),
            key_path: String::new(),
            key_content: String::new(),
            key_source: KeySourceChoice::LocalFile,
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
        self.tags = host.tags.join(", ");
        self.error = String::new();
        match &host.auth {
            crate::config::AuthMethod::Password { password } => {
                self.auth_method = AuthMethodChoice::Password;
                self.password = password.clone();
            }
            crate::config::AuthMethod::Key { key_path, key_content, passphrase, key_in_keychain } => {
                self.auth_method = AuthMethodChoice::Key;
                self.key_path = key_path.clone();
                self.key_content = key_content.clone();
                self.key_passphrase = passphrase.clone();
                self.key_in_keychain = *key_in_keychain;
                // Determine key source based on whether key_content is present
                self.key_source = if !key_content.is_empty() {
                    KeySourceChoice::ImportContent
                } else {
                    KeySourceChoice::LocalFile
                };
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
    /// Pending PTY resize (cols, rows) — debounced for column changes
    pub pending_pty_size: Option<(u16, u16)>,
    /// Deadline for sending debounced PTY resize
    pub pty_resize_deadline: Instant,
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
            pending_pty_size: None,
            pty_resize_deadline: Instant::now(),
        }
    }

    /// Helper to get effective username (current user if empty)
    pub fn get_effective_username(username: &str) -> String {
        if username.is_empty() {
            std::env::var("USER").unwrap_or_else(|_| {
                // Fallback to whoami command if USER env var not set
                std::process::Command::new("whoami")
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .unwrap_or_else(|_| "root".to_string())
            })
        } else {
            username.to_string()
        }
    }

    pub fn new_ssh(host: &HostEntry, runtime: &tokio::runtime::Runtime) -> Self {
        // Use current system user if username is empty
        let username = Self::get_effective_username(&host.username);

        let ssh = SshSession::connect(
            runtime,
            host.host.clone(),
            host.port,
            username,
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
            pending_pty_size: None,
            pty_resize_deadline: Instant::now(),
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
        // Check pending PTY resize (called every frame from render loop)
        if let Some((pc, pr)) = self.pending_pty_size {
            if Instant::now() >= self.pty_resize_deadline {
                if let Some(ref mut session) = self.session {
                    let _ = session.resize(pc, pr);
                }
                self.pending_pty_size = None;
            }
        }

        if cols == self.last_cols && rows == self.last_rows {
            return;
        }

        let cols_changed = cols != self.last_cols;
        self.last_cols = cols;
        self.last_rows = rows;

        // Always reflow grid immediately (for visual feedback)
        if let Ok(mut grid) = self.grid.lock() {
            grid.resize(cols, rows);
        }

        if cols_changed {
            // Debounce PTY resize to prevent shell SIGWINCH response (erase_below)
            // from destroying reflowed content during drag.
            // Use longer debounce during active resize bursts (rapid drag / direction changes)
            // to ensure SIGWINCH only fires after the user has fully stopped dragging.
            let debounce = if self.pending_pty_size.is_some() {
                Duration::from_millis(800)
            } else {
                Duration::from_millis(150)
            };
            self.pending_pty_size = Some((cols as u16, rows as u16));
            self.pty_resize_deadline = Instant::now() + debounce;
        } else {
            // Row-only change: send PTY resize immediately
            if let Some(ref mut session) = self.session {
                let _ = session.resize(cols as u16, rows as u16);
            }
        }
    }
}

/// Which SFTP panel an operation targets
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SftpPanel {
    LeftLocal,
    RightLocal,
    LeftRemote,
    RightRemote,
}

impl SftpPanel {
    #[allow(dead_code)]
    pub fn is_local(self) -> bool {
        matches!(self, SftpPanel::LeftLocal | SftpPanel::RightLocal)
    }
}

/// SFTP right-click context menu state (supports multi-selection)
pub struct SftpContextMenu {
    pub pos: egui::Pos2,
    pub panel: SftpPanel,
    pub entry_indices: Vec<usize>,
    pub entry_names: Vec<String>,
    pub all_dirs: bool,
    pub any_dirs: bool,
}

/// SFTP rename dialog state
pub struct SftpRenameDialog {
    pub panel: SftpPanel,
    pub old_name: String,
    pub new_name: String,
    pub error: String,
}

/// SFTP new folder dialog state
pub struct SftpNewFolderDialog {
    pub panel: SftpPanel,
    pub name: String,
    pub error: String,
}

/// SFTP new file dialog state
pub struct SftpNewFileDialog {
    pub panel: SftpPanel,
    pub name: String,
    pub error: String,
}

/// SFTP delete confirmation dialog state (supports multi-file delete)
pub struct SftpConfirmDelete {
    pub panel: SftpPanel,
    pub names: Vec<String>,
}

/// SFTP error dialog state
pub struct SftpErrorDialog {
    pub title: String,
    pub message: String,
}

/// SFTP editor dialog state
pub struct SftpEditorDialog {
    pub panel: SftpPanel,
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
    Batch,
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

/// Host filter state for the hosts list view
#[derive(Default, Clone)]
pub struct HostFilter {
    /// Filter by tag (empty = show all)
    pub tag: String,
    /// Filter by group (empty = show all)
    pub group: String,
}

impl HostFilter {
    pub fn is_active(&self) -> bool {
        !self.tag.is_empty() || !self.group.is_empty()
    }

    pub fn clear(&mut self) {
        self.tag.clear();
        self.group.clear();
    }

    /// Check if a host matches the filter criteria
    pub fn matches(&self, host: &HostEntry) -> bool {
        // Skip local hosts for tag/group filters
        if host.is_local {
            return true;
        }

        // Apply tag filter
        if !self.tag.is_empty() {
            if !host.tags.iter().any(|t| t.eq_ignore_ascii_case(&self.tag)) {
                return false;
            }
        }

        // Apply group filter
        if !self.group.is_empty() {
            if host.group != self.group {
                return false;
            }
        }

        true
    }
}
