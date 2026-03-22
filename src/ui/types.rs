//! # UI Type Definitions
//!
//! This module defines shared data structures used across the UI layer.
//!
//! ## Core Concepts
//!
//! ### Terminal Sessions
//!
//! `TerminalSession` represents a single terminal emulator instance with:
//! - **Terminal Grid**: Character cells, cursor, scrollback buffer
//! - **PTY Backend**: Unix PTY or SSH session
//! - **Selection**: Mouse selection state (start, end, active)
//! - **Scroll Offset**: Current scrollback position
//!
//! ### Session Backend
//!
//! Sessions can be backed by different implementations:
//! - **Local**: Unix PTY for local shell (default)
//! - **Ssh**: Remote SSH connection with authentication
//!
//! ### Pane Tree
//!
//! Terminal panes are organized in a recursive tree structure:
//! ```text
//! PaneNode::Split {
//!     direction: Horizontal,
//!     first: Leaf(session_id),
//!     second: Split {
//!         direction: Vertical,
//!         first: Leaf(session_id),
//!         second: Leaf(session_id),
//!         ratio: 0.5,
//!     },
//!     ratio: 0.7,
//! }
//! ```
//!
//! ### Broadcast Mode
//!
//! When broadcast is enabled, keyboard input is sent to all panes in the current tab,
//! enabling simultaneous command execution across multiple terminals.
//!
//! ## Dialog Types
//!
//! Various dialog types for user interactions:
//! - **AddHostDialog**: Add/edit SSH host configuration
//! - **SftpContextMenu**: Right-click menu for SFTP entries
//! - **SftpRenameDialog**: Rename files/directories
//! - **SftpNewFolderDialog / SftpNewFileDialog**: Create new folders/files
//! - **SftpConfirmDelete**: Confirm deletion with progress tracking
//! - **SftpEditorDialog**: Built-in file editor
//! - **SftpErrorDialog**: Display SFTP operation errors
//!
//! ## Key Structures
//!
//! - **TerminalSession**: Main terminal session type
//! - **SessionBackend**: Backend implementation (Local/Ssh)
//! - **PaneNode**: Pane tree node (Split or Leaf)
//! - **BroadcastState**: Broadcast mode state
//! - **Selection**: Text selection state (start, end, active)

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::config::{HostEntry, ResolvedAuth};
use crate::ssh::{SshSession, SshConnectionState, JumpHostInfo};
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

    pub fn get_shell_name(&self) -> Option<String> {
        match self {
            SessionBackend::Local(s) => s.get_shell_name(),
            SessionBackend::Ssh(_) => None,
        }
    }
}

/// Credential mode for the host dialog
#[derive(Default, PartialEq, Clone, Copy)]
pub enum CredentialMode {
    #[default]
    None,
    Existing,
    Inline,
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
    // Credential selection
    pub credential_mode: CredentialMode,
    pub selected_credential_id: Option<String>,
    // Inline credential fields (used when credential_mode == Inline)
    pub auth_method: AuthMethodChoice,
    pub password: String,
    pub key_path: String,
    pub key_content: String,
    pub key_source: KeySourceChoice,
    pub key_passphrase: String,
    pub key_in_keychain: bool,
    pub startup_commands: String,
    pub agent_forwarding: bool,
    pub test_conn_state: TestConnState,
    pub test_conn_result: Option<Arc<Mutex<Option<Result<String, String>>>>>,
    /// Show "Remove old key" button when host key verification fails
    pub show_remove_key_button: bool,
    /// Message to display after removing a key
    pub remove_key_message: String,
    pub jump_host: Option<String>,
    /// Port forward rules configured for this host
    pub port_forwards: Vec<crate::config::PortForwardConfig>,
    /// Editing state for a new port forward being added
    pub new_forward_kind: crate::config::ForwardKind,
    pub new_forward_local_host: String,
    pub new_forward_local_port: String,
    pub new_forward_remote_host: String,
    pub new_forward_remote_port: String,
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
            credential_mode: CredentialMode::None,
            selected_credential_id: None,
            auth_method: AuthMethodChoice::Password,
            password: String::new(),
            key_path: String::new(),
            key_content: String::new(),
            key_source: KeySourceChoice::LocalFile,
            key_passphrase: String::new(),
            key_in_keychain: false,
            startup_commands: String::new(),
            agent_forwarding: false,
            test_conn_state: TestConnState::Idle,
            test_conn_result: None,
            show_remove_key_button: false,
            remove_key_message: String::new(),
            jump_host: None,
            port_forwards: Vec::new(),
            new_forward_kind: crate::config::ForwardKind::Local,
            new_forward_local_host: "127.0.0.1".to_owned(),
            new_forward_local_port: String::new(),
            new_forward_remote_host: "127.0.0.1".to_owned(),
            new_forward_remote_port: String::new(),
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
        self.startup_commands = host.startup_commands.join("\n");
        self.agent_forwarding = host.agent_forwarding;
        self.jump_host = host.jump_host.clone();
        self.port_forwards = host.port_forwards.clone();
        self.error = String::new();

        // Set credential mode based on host state
        if let Some(ref cid) = host.credential_id {
            self.credential_mode = CredentialMode::Existing;
            self.selected_credential_id = Some(cid.clone());
        } else if host.auth != crate::config::AuthMethod::None {
            // Legacy host with embedded auth — show as inline
            self.credential_mode = CredentialMode::Inline;
            self.selected_credential_id = None;
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
                    self.key_source = if !key_content.is_empty() {
                        KeySourceChoice::ImportContent
                    } else {
                        KeySourceChoice::LocalFile
                    };
                }
                crate::config::AuthMethod::None => {}
            }
        } else {
            self.credential_mode = CredentialMode::None;
            self.selected_credential_id = None;
        }
    }
}

/// Search match position in terminal content
#[derive(Clone, Debug)]
pub struct SearchMatch {
    /// Global row index (scrollback + grid unified)
    pub row: usize,
    /// Start column (inclusive)
    pub col_start: usize,
    /// End column (exclusive)
    pub col_end: usize,
}

/// Search state for terminal content
pub struct SearchState {
    /// Current search query
    pub query: String,
    /// All matches found
    pub matches: Vec<SearchMatch>,
    /// Index of the currently highlighted match
    pub current_index: usize,
    /// Whether search is case-sensitive
    pub case_sensitive: bool,
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
    /// Saved resolved auth for SSH reconnection (avoids needing credentials vec)
    pub resolved_auth: Option<ResolvedAuth>,
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
    /// Tracks if we just sent non-ASCII text (for IME punctuation handling)
    pub last_non_ascii_input: bool,
    /// Current working directory (updated via OSC 7 or initial cwd)
    pub cwd: Option<String>,
    /// Search state (active when Some)
    pub search_state: Option<SearchState>,
}

impl TerminalSession {
    pub fn new_local(id: usize, shell: &str) -> Self {
        // Load settings to get scrollback limit
        let settings = crate::config::load_settings();
        let scrollback_bytes = (settings.scrollback_limit_mb as usize) * 1024 * 1024;

        let session = RealPtySession::with_scrollback_limit(id, 80, 24, shell, scrollback_bytes)
            .ok().map(SessionBackend::Local);
        let grid = session.as_ref().map(|s| s.get_grid()).unwrap_or_else(|| {
            Arc::new(Mutex::new(TerminalGrid::with_scrollback_limit(80, 24, scrollback_bytes)))
        });

        // Get initial cwd
        let cwd = std::env::current_dir().ok().map(|p| p.to_string_lossy().to_string());

        Self {
            session,
            grid,
            last_cols: 80,
            last_rows: 24,
            scroll_offset: 0,
            ssh_host: None,
            resolved_auth: None,
            selection: Selection::default(),
            local_shell: shell.to_string(),
            created_at: Instant::now(),
            pending_pty_size: None,
            pty_resize_deadline: Instant::now(),
            last_non_ascii_input: false,
            cwd,
            search_state: None,
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

    pub fn new_ssh(host: &HostEntry, auth: ResolvedAuth, runtime: &tokio::runtime::Runtime, jump_host: Option<JumpHostInfo>) -> Self {
        // Use current system user if username is empty
        let username = Self::get_effective_username(&host.username);

        crate::config::append_history(crate::config::ConnectionRecord {
            host_name: host.name.clone(),
            host: host.host.clone(),
            port: host.port,
            username: username.clone(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            success: true,
        });

        // Load settings to get scrollback limit
        let settings = crate::config::load_settings();
        let scrollback_bytes = (settings.scrollback_limit_mb as usize) * 1024 * 1024;

        let ssh = SshSession::with_scrollback_limit(
            runtime,
            host.host.clone(),
            host.port,
            username,
            auth.clone(),
            80,
            24,
            host.startup_commands.clone(),
            scrollback_bytes,
            settings.ssh_keepalive_interval,
            host.agent_forwarding,
            host.port_forwards.clone(),
            jump_host,
        );
        let grid = ssh.get_grid();
        Self {
            session: Some(SessionBackend::Ssh(ssh)),
            grid,
            last_cols: 80,
            last_rows: 24,
            scroll_offset: 0,
            ssh_host: Some(host.clone()),
            resolved_auth: Some(auth),
            selection: Selection::default(),
            local_shell: String::new(),
            created_at: Instant::now(),
            pending_pty_size: None,
            pty_resize_deadline: Instant::now(),
            last_non_ascii_input: false,
            cwd: None, // SSH sessions start without known cwd
            search_state: None,
        }
    }

    /// Shell display name for this session
    pub fn shell_name(&self) -> String {
        match &self.session {
            Some(SessionBackend::Local(_)) => {
                // Use stored shell path directly (more reliable than process detection)
                if !self.local_shell.is_empty() {
                    self.local_shell.rsplit('/').next().unwrap_or("shell").to_string()
                } else {
                    // Fallback: try to get actual running shell name
                    self.session.as_ref()
                        .and_then(|s| s.get_shell_name())
                        .unwrap_or_else(|| "shell".to_string())
                }
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
    pub fn reconnect_ssh(&mut self, runtime: &tokio::runtime::Runtime, jump_host: Option<JumpHostInfo>) {
        if let (Some(ref host), Some(ref auth)) = (&self.ssh_host, &self.resolved_auth) {
            let settings = crate::config::load_settings();
            let ssh = SshSession::connect(
                runtime,
                host.host.clone(),
                host.port,
                host.username.clone(),
                auth.clone(),
                self.last_cols as u16,
                self.last_rows as u16,
                host.startup_commands.clone(),
                settings.ssh_keepalive_interval,
                host.agent_forwarding,
                jump_host,
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
            // Even if dimensions haven't changed, clamp cursor position
            // This handles the case where cursor was set to an invalid position
            // before the status bar was properly accounted for
            if let Ok(mut grid) = self.grid.lock() {
                if grid.cursor_row >= grid.rows {
                    grid.cursor_row = grid.rows.saturating_sub(1);
                }
            }
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
    #[allow(dead_code)]
    Batch,
}

/// Pending keychain credential deletion (awaiting user confirmation).
pub enum KeychainDeleteRequest {
    /// Delete a credential by id, with list of affected host names
    ById { credential_id: String, affected_hosts: Vec<String> },
    /// Delete all credentials
    All,
}

/// Credential create/edit dialog state
pub struct CredentialDialog {
    pub open: bool,
    pub edit_id: Option<String>,
    pub name: String,
    pub cred_type: CredentialTypeChoice,
    pub username: String,
    pub password: String,
    pub key_path: String,
    pub key_content: String,
    pub key_source: KeySourceChoice,
    pub key_passphrase: String,
    pub error: String,
}

impl Default for CredentialDialog {
    fn default() -> Self {
        Self {
            open: false,
            edit_id: None,
            name: String::new(),
            cred_type: CredentialTypeChoice::Password,
            username: String::new(),
            password: String::new(),
            key_path: String::new(),
            key_content: String::new(),
            key_source: KeySourceChoice::LocalFile,
            key_passphrase: String::new(),
            error: String::new(),
        }
    }
}

impl CredentialDialog {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn open_new(&mut self) {
        self.reset();
        self.open = true;
    }

    pub fn open_edit(&mut self, cred: &crate::config::Credential) {
        self.open = true;
        self.edit_id = Some(cred.id.clone());
        self.name = cred.name.clone();
        self.error.clear();
        match &cred.credential_type {
            crate::config::CredentialType::Password { username } => {
                self.cred_type = CredentialTypeChoice::Password;
                self.username = username.clone();
                // Load password from keychain for display
                self.password = crate::config::load_credential_secret(&cred.id, &cred.name, "password")
                    .unwrap_or_default();
            }
            crate::config::CredentialType::SshKey { key_path, has_passphrase, .. } => {
                self.cred_type = CredentialTypeChoice::SshKey;
                self.key_path = key_path.clone();
                self.key_source = if key_path.is_empty() {
                    KeySourceChoice::ImportContent
                } else {
                    KeySourceChoice::LocalFile
                };
                self.key_passphrase = if *has_passphrase {
                    crate::config::load_credential_secret(&cred.id, &cred.name, "passphrase")
                        .unwrap_or_default()
                } else {
                    String::new()
                };
            }
        }
    }
}

/// Credential type choice for the dialog
#[derive(Default, PartialEq, Clone, Copy)]
pub enum CredentialTypeChoice {
    #[default]
    Password,
    SshKey,
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
