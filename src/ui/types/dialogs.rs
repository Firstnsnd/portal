//! # Dialog Types
//!
//! This module contains all dialog/state types for user interactions.

use crate::config::HostEntry;

/// Auth method choice for the dialog
#[derive(Default, PartialEq, Clone)]
pub enum AuthMethodChoice {
    #[default]
    Password,
    Key,
}

/// Key source choice for SSH key authentication
#[derive(Default, PartialEq, Clone, Copy)]
pub enum KeySourceChoice {
    #[default]
    LocalFile,
    ImportContent,
}

/// Credential mode for the host dialog
#[derive(Default, PartialEq, Clone, Copy)]
pub enum CredentialMode {
    #[default]
    None,
    Existing,
    Inline,
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

/// Add Host dialog state
pub struct AddHostDialog {
    pub open: bool,
    /// Animation state: 0.0 = fully closed, 1.0 = fully open
    pub anim_state: f32,
    /// Animation start time
    pub anim_start_time: Option<f64>,
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
    pub test_conn_result: Option<std::sync::Arc<Mutex<Option<Result<String, String>>>>>,
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

impl Default for AddHostDialog {
    fn default() -> Self {
        Self {
            open: false,
            anim_state: 0.0,
            anim_start_time: None,
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
        self.open = false;
        self.edit_index = None;
        self.name.clear();
        self.host.clear();
        self.port = "22".to_owned();
        self.username.clear();
        self.group.clear();
        self.tags.clear();
        self.error.clear();
        self.credential_mode = CredentialMode::None;
        self.selected_credential_id = None;
        self.auth_method = AuthMethodChoice::Password;
        self.password.clear();
        self.key_path.clear();
        self.key_content.clear();
        self.key_source = KeySourceChoice::LocalFile;
        self.key_passphrase.clear();
        self.key_in_keychain = false;
        self.startup_commands.clear();
        self.agent_forwarding = false;
        self.test_conn_state = TestConnState::Idle;
        self.test_conn_result = None;
        self.show_remove_key_button = false;
        self.remove_key_message.clear();
        self.jump_host = None;
        self.port_forwards.clear();
        self.new_forward_kind = crate::config::ForwardKind::Local;
        self.new_forward_local_host = "127.0.0.1".to_owned();
        self.new_forward_local_port.clear();
        self.new_forward_remote_host = "127.0.0.1".to_owned();
        self.new_forward_remote_port.clear();
        // Note: don't reset anim_state - let it animate out
    }

    pub fn open_new(&mut self, current_time: f64) {
        self.reset();
        self.open = true;
        self.anim_start_time = Some(current_time);
        self.anim_state = 0.0;
    }

    pub fn open_edit(&mut self, index: usize, host: &HostEntry, current_time: f64) {
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
        self.anim_start_time = Some(current_time);
        self.anim_state = 0.0;
    }

    /// Update animation state, return true if drawer should be visible
    #[allow(dead_code)]
    pub fn update_animation(&mut self, current_time: f64) -> bool {
        const ANIM_DURATION: f64 = 0.3; // 300ms for smooth slide

        if self.open {
            // Opening animation
            if let Some(start) = self.anim_start_time {
                let elapsed = current_time - start;
                let progress = (elapsed / ANIM_DURATION).min(1.0);

                // Ease-out cubic: fast start, smooth end
                let t = progress as f32;
                let eased = 1.0 - (1.0 - t).powi(3);

                self.anim_state = eased;

                if progress >= 1.0 {
                    self.anim_start_time = None; // Animation complete
                }
            } else {
                self.anim_state = 1.0;
            }
            true
        } else {
            // Closing animation
            if let Some(start) = self.anim_start_time {
                let elapsed = current_time - start;
                let progress = (elapsed / ANIM_DURATION).min(1.0);

                // Ease-in cubic: smooth start, fast end
                let eased = (progress as f32).powi(3);

                self.anim_state = 1.0 - eased;

                if progress >= 1.0 {
                    self.anim_start_time = None;
                    return false; // Fully closed, stop rendering
                }
            } else {
                return false; // Not animating, fully closed
            }
            true
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

/// State for the "Add Tunnel" dialog
pub struct AddTunnelDialog {
    pub open: bool,
    /// Index of the selected host (in app.hosts) to add tunnel to
    pub selected_host_idx: Option<usize>,
    pub forward_kind: crate::config::ForwardKind,
    pub local_host: String,
    pub local_port: String,
    pub remote_host: String,
    pub remote_port: String,
    pub error: String,
    /// Pending delete confirmation: (host_idx, tunnel_idx)
    pub confirm_delete: Option<(usize, usize)>,
}

impl Default for AddTunnelDialog {
    fn default() -> Self {
        Self {
            open: false,
            selected_host_idx: None,
            forward_kind: crate::config::ForwardKind::Local,
            local_host: "127.0.0.1".to_owned(),
            local_port: String::new(),
            remote_host: "127.0.0.1".to_owned(),
            remote_port: String::new(),
            error: String::new(),
            confirm_delete: None,
        }
    }
}

impl AddTunnelDialog {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Open the drawer
    pub fn open_drawer(&mut self) {
        self.open = true;
    }

    /// Close the drawer
    pub fn close_drawer(&mut self) {
        self.open = false;
    }
}

/// Pending keychain credential deletion (awaiting user confirmation).
pub enum KeychainDeleteRequest {
    /// Delete a credential by id, with list of affected host names
    ById { credential_id: String, affected_hosts: Vec<String> },
    /// Delete all credentials
    All,
}

/// Snippet view state for Command Snippets feature
pub struct SnippetViewState {
    #[allow(dead_code)]
    pub search_query: String,
    pub group_filter: String,  // Filter by group (empty = show all)
    pub editing: Option<String>,  // id of snippet being edited
    #[allow(dead_code)]
    pub edit_name: String,
    #[allow(dead_code)]
    pub edit_command: String,
    #[allow(dead_code)]
    pub edit_group: String,
    // Drawer state for add/edit snippet
    pub open: bool,
    pub new_name: String,
    pub new_command: String,
    pub new_group: String,
    pub confirm_delete: Option<String>,  // id of snippet pending delete confirmation
    // Quick selector state for running snippets from terminal view
    pub quick_selector_open: bool,  // Whether quick snippet selector is open (from terminal)
    pub selected_snippet_index: Option<usize>,  // Currently selected snippet index in quick selector
    // Session selector state for running snippets (deprecated, kept for compatibility)
    #[allow(dead_code)]
    pub pending_run_command: Option<String>,  // Command waiting to be executed
    #[allow(dead_code)]
    pub selector_open: bool,  // Whether session selector dialog is open
    #[allow(dead_code)]
    pub selected_tab: Option<usize>,  // Currently selected tab index in selector
    #[allow(dead_code)]
    pub selected_session: Option<usize>,  // Currently selected session index in selector
}

impl SnippetViewState {
    /// Open the drawer for creating a new snippet
    pub fn open_new(&mut self, default_group: &str) {
        self.open = true;
        self.editing = None;
        self.new_name.clear();
        self.new_command.clear();
        self.new_group = default_group.to_string();
    }

    /// Open the drawer for editing an existing snippet
    pub fn open_edit(&mut self, id: String, name: &str, command: &str, group: &str) {
        self.open = true;
        self.editing = Some(id.clone());
        self.new_name = name.to_string();
        self.new_command = command.to_string();
        self.new_group = group.to_string();
    }

    /// Close the drawer
    #[allow(dead_code)]
    pub fn close(&mut self) {
        self.open = false;
    }
}

impl Default for SnippetViewState {
    fn default() -> Self {
        Self {
            search_query: String::new(),
            group_filter: String::new(),
            editing: None,
            edit_name: String::new(),
            edit_command: String::new(),
            edit_group: String::new(),
            open: false,
            new_name: String::new(),
            new_command: String::new(),
            new_group: String::new(),
            confirm_delete: None,
            quick_selector_open: false,
            selected_snippet_index: None,
            pending_run_command: None,
            selector_open: false,
            selected_tab: None,
            selected_session: None,
        }
    }
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

/// Current view shown in the main area
#[derive(PartialEq, Clone, Copy)]
pub enum AppView {
    Hosts,
    Terminal,
    Sftp,
    Keychain,
    Snippets,
    Tunnels,
    Settings,
}

/// Broadcast state - for Tab-internal continuous interactive operations
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

use std::sync::Mutex;
