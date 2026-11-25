//! # Dialog Types
//!
//! This module contains all dialog/state types for user interactions.

use crate::config::HostEntry;

/// Type alias for the async test-connection result
pub type TestConnResult = Option<std::sync::Arc<Mutex<Option<Result<String, String>>>>>;

/// Auth method choice for the dialog
#[derive(Default, PartialEq, Clone)]
pub enum AuthMethodChoice {
    #[default]
    Password,
    Key,
    Agent,
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
    /// Record this host's sessions from their first byte.
    pub auto_record: bool,
    /// Connecting/editing/viewing this host requires local owner
    /// verification (see `app::owner_gate`).
    pub protected: bool,
    /// Set by the drawer's Duplicate button when the host is protected —
    /// duplication must go through the owner gate, which the drawer (no
    /// ViewActions channel) cannot do itself. Consumed by the app layer.
    pub duplicate_gate_request: Option<usize>,
    /// Set by the drawer's Save when it would write keychain secrets
    /// (inline credential / typed proxy password): keychain operations must
    /// clear the owner gate first. Consumed by the app layer each frame.
    pub save_gate_request: bool,
    /// Owner verification passed for a parked save — the drawer re-runs its
    /// save body next frame and clears this. Cleared by every drawer open
    /// so a stale grant can never bypass the gate on a later save.
    pub save_gate_granted: bool,
    pub test_conn_state: TestConnState,
    pub test_conn_result: TestConnResult,
    /// Show "Remove old key" button when host key verification fails
    pub show_remove_key_button: bool,
    /// Message to display after removing a key
    pub remove_key_message: String,
    pub jump_host: Option<String>,
    /// Proxy selection: None = connect directly, Some(kind) = tunnel
    /// through an HTTP CONNECT or SOCKS5 proxy.
    pub proxy_kind: Option<crate::config::ProxyKind>,
    pub proxy_host: String,
    pub proxy_port: String,
    pub proxy_username: String,
    /// Typed only; persisted to the keychain on save, never to hosts.json.
    pub proxy_password: String,
    /// True when the saved host already has a proxy password in the
    /// keychain — an empty password field on save then means "keep it".
    pub proxy_password_in_keychain: bool,
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
            auto_record: false,
            protected: false,
            duplicate_gate_request: None,
            save_gate_request: false,
            save_gate_granted: false,
            test_conn_state: TestConnState::Idle,
            test_conn_result: None,
            show_remove_key_button: false,
            remove_key_message: String::new(),
            jump_host: None,
            proxy_kind: None,
            proxy_host: String::new(),
            proxy_port: String::new(),
            proxy_username: String::new(),
            proxy_password: String::new(),
            proxy_password_in_keychain: false,
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
    /// Proxy resolved from the current form fields (test-connection and
    /// save paths). The keychain-held password cannot participate here —
    /// only what's typed in the form is used; the save path re-stores the
    /// keychain entry separately.
    pub fn resolved_proxy(&self) -> Option<crate::config::ResolvedProxy> {
        let kind = self.proxy_kind?;
        let host = self.proxy_host.trim();
        let port: u16 = self.proxy_port.trim().parse().unwrap_or(0);
        if host.is_empty() || port == 0 {
            return None;
        }
        let username =
            (!self.proxy_username.trim().is_empty()).then(|| self.proxy_username.trim().to_owned());
        let password = (!self.proxy_password.is_empty()).then(|| self.proxy_password.clone());
        Some(crate::config::ResolvedProxy {
            kind,
            host: host.to_owned(),
            port,
            username,
            password,
        })
    }

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
        self.auto_record = false;
        self.protected = false;
        self.duplicate_gate_request = None;
        self.save_gate_request = false;
        self.save_gate_granted = false;
        self.test_conn_state = TestConnState::Idle;
        self.test_conn_result = None;
        self.show_remove_key_button = false;
        self.remove_key_message.clear();
        self.jump_host = None;
        self.proxy_kind = None;
        self.proxy_host.clear();
        self.proxy_port.clear();
        self.proxy_username.clear();
        self.proxy_password.clear();
        self.proxy_password_in_keychain = false;
        self.port_forwards.clear();
        self.new_forward_kind = crate::config::ForwardKind::Local;
        self.new_forward_local_host = "127.0.0.1".to_owned();
        self.new_forward_local_port.clear();
        self.new_forward_remote_host = "127.0.0.1".to_owned();
        self.new_forward_remote_port.clear();
    }

    pub fn open_new(&mut self) {
        self.reset();
        self.open = true;
    }

    pub fn open_edit(&mut self, index: usize, host: &HostEntry) {
        self.open = true;
        self.edit_index = Some(index);
        self.fill_from_host(host);
    }

    /// Prefill from an existing host for "Duplicate host": every value is
    /// copied, but the drawer stays in add mode (edit_index None) so saving
    /// creates a NEW entry. The name gets the duplicate suffix.
    pub fn open_duplicate(&mut self, host: &HostEntry, name_suffix: &str) {
        self.open = true;
        self.fill_from_host(host);
        self.edit_index = None;
        if !name_suffix.is_empty() {
            self.name = format!("{} {}", self.name, name_suffix);
        }
    }

    /// Shared prefill used by open_edit and open_duplicate.
    fn fill_from_host(&mut self, host: &HostEntry) {
        // Unlike open_new (full reset), the edit/duplicate prefill keeps
        // most form state — but a parked/stale gate grant from an earlier
        // save must never carry over and bypass the keychain gate.
        self.save_gate_request = false;
        self.save_gate_granted = false;
        self.name = host.name.clone();
        self.host = host.host.clone();
        self.port = host.port.to_string();
        self.username = host.username.clone();
        self.group = host.group.clone();
        self.tags = host.tags.join(", ");
        self.startup_commands = host.startup_commands.join("\n");
        self.agent_forwarding = host.agent_forwarding;
        self.auto_record = host.auto_record;
        self.protected = host.protected;
        self.jump_host = host.jump_host.clone();
        // Proxy config: host/port/username live in hosts.json; the password
        // stays in the keychain and is not echoed back into the form.
        self.proxy_kind = host.proxy.as_ref().map(|p| p.kind);
        self.proxy_host = host
            .proxy
            .as_ref()
            .map(|p| p.host.clone())
            .unwrap_or_default();
        self.proxy_port = host
            .proxy
            .as_ref()
            .map(|p| p.port.to_string())
            .unwrap_or_default();
        self.proxy_username = host
            .proxy
            .as_ref()
            .map(|p| p.username.clone())
            .unwrap_or_default();
        self.proxy_password.clear();
        self.proxy_password_in_keychain = host
            .proxy
            .as_ref()
            .map(|p| p.password_in_keychain)
            .unwrap_or(false);
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
                crate::config::AuthMethod::Key {
                    key_path,
                    key_content,
                    passphrase,
                    key_in_keychain,
                } => {
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
                crate::config::AuthMethod::Agent => {
                    self.auth_method = AuthMethodChoice::Agent;
                }
                crate::config::AuthMethod::None => {}
            }
        } else {
            self.credential_mode = CredentialMode::None;
            self.selected_credential_id = None;
        }
    }
}

/// Credential type choice for the dialog
#[derive(Default, PartialEq, Clone, Copy)]
pub enum CredentialTypeChoice {
    #[default]
    Password,
    SshKey,
    Agent,
}

/// Credential create/edit dialog state
#[derive(Clone)]
pub struct CredentialDialog {
    pub open: bool,
    pub edit_id: Option<String>,
    pub confirm_delete: Option<String>,
    pub name: String,
    pub cred_type: CredentialTypeChoice,
    pub username: String,
    pub password: String,
    pub key_path: String,
    pub key_content: String,
    pub key_source: KeySourceChoice,
    pub key_passphrase: String,
    pub error: String,
    /// Set by the drawer's Save (after field validation): writing the
    /// credential store is a keychain operation, so the save parks behind
    /// owner verification. Consumed by the app layer each frame.
    pub gate_request: bool,
    /// Owner verification passed for a parked save — the drawer re-runs its
    /// save body next frame and clears this. Cleared on every drawer open.
    pub gate_granted: bool,
}

impl Default for CredentialDialog {
    fn default() -> Self {
        Self {
            open: false,
            edit_id: None,
            confirm_delete: None,
            name: String::new(),
            cred_type: CredentialTypeChoice::Password,
            username: String::new(),
            password: String::new(),
            key_path: String::new(),
            key_content: String::new(),
            key_source: KeySourceChoice::LocalFile,
            key_passphrase: String::new(),
            error: String::new(),
            gate_request: false,
            gate_granted: false,
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
        // A stale grant from an earlier save must never carry over and
        // bypass the keychain gate (open_new resets the whole struct).
        self.gate_request = false;
        self.gate_granted = false;
        self.edit_id = Some(cred.id.clone());
        self.name = cred.name.clone();
        self.error.clear();
        match &cred.credential_type {
            crate::config::CredentialType::Password { username } => {
                self.cred_type = CredentialTypeChoice::Password;
                self.username = username.clone();
                // Load password from keychain for display
                self.password =
                    crate::config::load_credential_secret(&cred.id, &cred.name, "password")
                        .unwrap_or_default();
            }
            crate::config::CredentialType::SshKey {
                key_path,
                has_passphrase,
                ..
            } => {
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
            crate::config::CredentialType::Agent { username } => {
                self.cred_type = CredentialTypeChoice::Agent;
                self.username = username.clone();
            }
        }
    }
}

/// Snippet view state for Command Snippets feature
#[derive(Default)]
pub struct SnippetViewState {
    pub group_filter: String,    // Filter by group (empty = show all)
    pub editing: Option<String>, // id of snippet being edited
    // Drawer state for add/edit snippet
    pub open: bool,
    pub new_name: String,
    pub new_command: String,
    pub new_group: String,
    pub error: String,
    pub confirm_delete: Option<String>, // id of snippet pending delete confirmation
}

impl SnippetViewState {
    /// Open the drawer for creating a new snippet
    pub fn open_new(&mut self, default_group: &str) {
        self.open = true;
        self.editing = None;
        self.new_name.clear();
        self.new_command.clear();
        self.new_group = default_group.to_string();
        self.error.clear();
    }

    /// Open the drawer for editing an existing snippet
    pub fn open_edit(&mut self, id: String, name: &str, command: &str, group: &str) {
        self.open = true;
        self.editing = Some(id.clone());
        self.new_name = name.to_string();
        self.new_command = command.to_string();
        self.new_group = group.to_string();
        self.error.clear();
    }
}

/// Connection-history page view state (dedicated History page): the
/// search box filtering the day-grouped record list.
#[derive(Default)]
pub struct HistoryViewState {
    pub search: String,
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
        if !self.tag.is_empty() && !host.tags.iter().any(|t| t.eq_ignore_ascii_case(&self.tag)) {
            return false;
        }

        // Apply group filter
        if !self.group.is_empty() && host.group != self.group {
            return false;
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
    /// Dedicated connection-history page (moved off the hosts page).
    History,
    Snippets,
    Settings,
    Replay,
}

/// "Open Portal Tab/Window Here" hit a folder Portal cannot enter (macOS TCC
/// / permission denial). Offers to open the authorization settings, retry,
/// or give up; the shell meanwhile starts at ~.
#[derive(Clone)]
pub struct CwdPermissionDialog {
    /// The folder Portal could not enter.
    pub path: String,
    /// Which window receives the retried tab (tab case only).
    pub window_idx: usize,
    /// Retry opens a new WINDOW instead of a tab in `window_idx`.
    pub open_window: bool,
}

use std::sync::Mutex;

#[cfg(test)]
mod duplicate_host_tests {
    use super::*;

    fn sample_host(name: &str) -> HostEntry {
        HostEntry {
            name: name.to_string(),
            host: "10.0.0.5".to_string(),
            port: 2222,
            username: "deploy".to_string(),
            group: "prod".to_string(),
            tags: Vec::new(),
            is_local: false,
            credential_id: None,
            auth: crate::config::AuthMethod::None,
            startup_commands: Vec::new(),
            agent_forwarding: false,
            auto_record: false,
            jump_host: None,
            port_forwards: Vec::new(),
            proxy: None,
            protected: false,
        }
    }

    #[test]
    fn open_duplicate_prefills_in_add_mode_with_suffix() {
        let host = sample_host("web-1");
        let mut d = AddHostDialog::default();
        d.open_duplicate(&host, "copy");
        // Add mode: saving must create a new entry, not overwrite web-1.
        assert_eq!(d.edit_index, None);
        assert!(d.open);
        assert_eq!(d.name, "web-1 copy");
        assert_eq!(d.host, "10.0.0.5");
        assert_eq!(d.port, "2222");
        assert_eq!(d.username, "deploy");
        assert_eq!(d.group, "prod");
    }

    #[test]
    fn open_edit_still_targets_the_existing_entry() {
        let host = sample_host("web-1");
        let mut d = AddHostDialog::default();
        d.open_edit(3, &host);
        assert_eq!(d.edit_index, Some(3));
        assert_eq!(d.name, "web-1");
    }
}
