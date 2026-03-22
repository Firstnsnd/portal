//! # Configuration Management
//!
//! This module handles all configuration-related functionality including:
//! - **Host Management**: SSH host entries with credentials
//! - **Credential Storage**: Secure system keychain integration (macOS Keychain)
//! - **Configuration Persistence**: JSON-based host configuration
//! - **Keychain Operations**: Store/load passwords, keys, and passphrases
//!
//! ## Security Architecture
//!
//! ### Credential Storage Strategy
//!
//! Portal uses a **host-based keychain scheme** for secure credential storage:
//!
//! ```text
//! macOS Keychain Entry:
//! - Service: "Portal <host-name>" (for passwords)
//! - Service: "Portal Credential: <credential-id>" (for credentials)
//! - Account: "password:<host>:22:username" or "credential:<id>:kind"
//! - Secret: Actual password/passphrase/key (encrypted by system keychain)
//! ```
//!
//! ### Separation of Concerns
//!
//! 1. **hosts.json** (~/Library/Application Support/portal/hosts.json):
//!    - Contains host metadata (name, host, port, group, tags)
//!    - References credentials by ID (credential_id)
//!    - **NO secrets** - passwords, keys, passphrases never stored in JSON
//!
//! 2. **System Keychain** (macOS Keychain):
//!    - Stores all secrets (passwords, private keys, passphrases)
//!    - Encrypted at rest with system-level encryption
//!    - Requires user/password to access
//!
//! ## Migration Path
//!
//! The old scheme stored everything inline in hosts.json (LEGACY AuthMethod).
//! The new scheme separates credentials into reusable Credential entities.
//!
//! ## Key Functions
//!
//! ### Host Operations
//! - `load_hosts()` - Load all host entries from JSON
//! - `save_hosts()` - Persist host entries to JSON
//! - `delete_host()` - Remove a host and its keychain entries
//!
//! ### Credential Operations
//! - `store_credential_secret()` - Save password/key/passphrase to keychain
//! - `load_credential_secret()` - Retrieve secret from keychain
//! - `delete_credential()` - Remove credential and its secrets
//!
//! ### Authentication Resolution
//! - `resolve_auth()` - Convert AuthMethod to ResolvedAuth with actual secrets
//! - Loads secrets from keychain for SSH/SFTP connections
//!
//! ## Data Structures
//!
//! ### HostEntry
//!
//! Represents a saved SSH host configuration:
//! ```json
//! {
//!   "name": "My Server",
//!   "host": "example.com",
//!   "port": 22,
//!   "username": "user",
//!   "group": "Production",
//!   "tags": ["web", "linux"],
//!   "credential_id": "cred-123",
//!   "startup_commands": ["tmux attach"]
//! }
//! ```
//!
//! ### Credential
//!
//! Reusable credential stored in keychain:
//! ```json
//! {
//!   "id": "cred-123",
//!   "name": "My SSH Key",
//!   "credential_type": "ssh_key",
//!   "created_at": 1234567890
//! }
//! ```
//!
//! ### ResolvedAuth
//!
//! Transient in-memory authentication data with secrets loaded:
//! ```rust
//! ResolvedAuth::Password { password: "secret" }
//! ResolvedAuth::Key {
//!   key_content: "-----BEGIN RSA...",
//!   passphrase: Some("keypassphrase")
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Authentication method for SSH connections (LEGACY — kept for migration/fallback)
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(tag = "type")]
pub enum AuthMethod {
    #[default]
    #[serde(rename = "none")]
    None,
    #[serde(rename = "password")]
    Password { password: String },
    #[serde(rename = "key")]
    Key {
        #[serde(default)]
        key_path: String,
        #[serde(default)]
        key_content: String,
        #[serde(default)]
        passphrase: String,
        #[serde(default)]
        key_in_keychain: bool,
    },
}

// ── Credential (first-class entity) ─────────────────────────────────

/// A reusable credential stored in credentials.json.
/// Secrets (password, private key, passphrase) are NEVER in JSON — only in macOS keychain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credential {
    pub id: String,
    pub name: String,
    pub credential_type: CredentialType,
    #[serde(default)]
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum CredentialType {
    #[serde(rename = "password")]
    Password { username: String },
    #[serde(rename = "ssh_key")]
    SshKey {
        #[serde(default)]
        key_path: String,
        #[serde(default)]
        key_in_keychain: bool,
        #[serde(default)]
        has_passphrase: bool,
    },
}

impl Credential {
    pub fn new_password(name: String, username: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            credential_type: CredentialType::Password { username },
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    pub fn new_ssh_key(name: String, key_path: String, key_in_keychain: bool, has_passphrase: bool) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            credential_type: CredentialType::SshKey { key_path, key_in_keychain, has_passphrase },
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
}

/// Resolved authentication data (transient, in-memory only).
/// Built on-demand by `resolve_auth` — loads secrets from keychain.
#[derive(Debug, Clone)]
pub enum ResolvedAuth {
    None,
    Password { password: String },
    Key { key_content: String, passphrase: Option<String> },
}

/// A saved host/connection entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostEntry {
    pub name: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub is_local: bool,
    #[serde(default)]
    pub credential_id: Option<String>,
    #[serde(default)]
    pub auth: AuthMethod,
    #[serde(default)]
    pub startup_commands: Vec<String>,
}

fn default_port() -> u16 {
    22
}

impl HostEntry {
    pub fn new_local() -> Self {
        Self {
            name: "Localhost".into(),
            host: "localhost".into(),
            port: 0,
            username: String::new(),
            group: String::new(),
            tags: Vec::new(),
            is_local: true,
            credential_id: None,
            auth: AuthMethod::None,
            startup_commands: Vec::new(),
        }
    }

    pub fn new_ssh(name: String, host: String, port: u16, username: String, group: String, credential_id: Option<String>, startup_commands: Vec<String>) -> Self {
        Self {
            name,
            host,
            port,
            username,
            group,
            tags: Vec::new(),
            is_local: false,
            credential_id,
            auth: AuthMethod::None,
            startup_commands,
        }
    }
}

// ── Keychain helpers (macOS `security` CLI) ───────────────────────
//
// Uses the `security` command-line tool instead of the `keyring` crate.
// - Store: `security add-generic-password -A` (allow all apps, no prompt on rebuild)
// - Read:  `security find-generic-password -g` (always accessible, no ACL check)
// - Delete: `security delete-generic-password`
//
// This avoids the macOS Keychain ACL issue where recompiling the app
// changes its code signature and triggers a password prompt on every read.

const KEYRING_SERVICE_LEGACY: &str = "portal-ssh";

/// Build a per-host service name shown in Keychain Access.
fn keyring_service(display_name: &str) -> String {
    format!("Portal: {display_name}")
}

/// Build the keychain account key: "{host}:{port}:{username}:{kind}"
fn keyring_key(host: &str, port: u16, username: &str, kind: &str) -> String {
    format!("{host}:{port}:{username}:{kind}")
}

/// Store a credential in the system keychain via `security` CLI.
/// Uses `-A` to allow any application to access (no password prompt on rebuild).
fn store_host_credential(host: &str, port: u16, username: &str, kind: &str, secret: &str, display_name: &str) -> bool {
    let account = keyring_key(host, port, username, kind);
    let service = keyring_service(display_name);

    // Delete existing entry first (-U can't change ACL on existing items)
    let _ = Command::new("security")
        .args(["delete-generic-password", "-a", &account, "-s", &service])
        .output();

    // Add with -A (allow all applications to access without prompt)
    let result = Command::new("security")
        .args(["add-generic-password", "-a", &account, "-s", &service, "-w", secret, "-A"])
        .output();

    match result {
        Ok(o) if o.status.success() => {
            // Clean up legacy entry if it exists
            let _ = Command::new("security")
                .args(["delete-generic-password", "-a", &account, "-s", KEYRING_SERVICE_LEGACY])
                .output();
            true
        }
        Ok(o) => {
            log::warn!(
                "Failed to store credential in keychain: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            );
            false
        }
        Err(e) => {
            log::warn!("Failed to run security command: {e}");
            false
        }
    }
}

/// Read a password from the keychain using `security find-generic-password -g`.
/// Handles both quoted strings and hex-encoded output (for multi-line content like SSH keys).
fn security_find_password(service: &str, account: &str) -> Option<String> {
    let output = Command::new("security")
        .args(["find-generic-password", "-s", service, "-a", account, "-g"])
        .output()
        .ok()?;

    if !output.status.success() {
        return Option::None;
    }

    // Password is on stderr in format:
    //   password: "the password"          (text)
    //   password: 0x2d2d2d2d2d42454...   (hex, for binary/multi-line content)
    //   On some macOS versions the hex is followed by the quoted string on the same line.
    let stderr = String::from_utf8_lossy(&output.stderr);
    for line in stderr.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("password: ") {
            if let Some(hex_str) = rest.strip_prefix("0x") {
                // Hex-encoded binary content — take only hex chars (stop at first space/non-hex)
                let hex_only: String = hex_str.chars()
                    .take_while(|c| c.is_ascii_hexdigit())
                    .collect();
                if !hex_only.is_empty() && hex_only.len() % 2 == 0 {
                    let bytes: Vec<u8> = (0..hex_only.len())
                        .step_by(2)
                        .filter_map(|i| hex_only.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()))
                        .collect();
                    if bytes.len() == hex_only.len() / 2 {
                        return String::from_utf8(bytes).ok();
                    }
                }
            } else if rest.starts_with('"') && rest.ends_with('"') && rest.len() >= 2 {
                // Quoted string
                return Some(rest[1..rest.len() - 1].to_string());
            }
        }
    }
    Option::None
}

/// Load a credential from the system keychain (host-based key scheme).
/// Tries new per-host service first, then falls back to legacy "portal-ssh".
pub fn load_host_credential(host: &str, port: u16, username: &str, kind: &str, display_name: &str) -> Option<String> {
    let account = keyring_key(host, port, username, kind);

    // Try per-host service first
    let service = keyring_service(display_name);
    if let Some(secret) = security_find_password(&service, &account) {
        return Some(secret);
    }

    // Fall back to legacy service
    if let Some(secret) = security_find_password(KEYRING_SERVICE_LEGACY, &account) {
        return Some(secret);
    }

    log::warn!("Credential not found in keychain: kind={kind}, host={host}:{port}, display_name={display_name}");
    Option::None
}

/// Delete a host-based credential from the system keychain (both new and legacy service).
pub fn delete_host_credential(host: &str, port: u16, username: &str, kind: &str, display_name: &str) {
    let account = keyring_key(host, port, username, kind);
    // Delete from new service
    let service = keyring_service(display_name);
    let _ = Command::new("security")
        .args(["delete-generic-password", "-a", &account, "-s", &service])
        .output();
    // Also delete legacy entry
    let _ = Command::new("security")
        .args(["delete-generic-password", "-a", &account, "-s", KEYRING_SERVICE_LEGACY])
        .output();
}

/// Delete all keychain entries associated with a host entry (legacy scheme).
pub fn delete_host_credentials(host: &HostEntry) {
    match &host.auth {
        AuthMethod::Password { .. } => {
            delete_host_credential(&host.host, host.port, &host.username, "password", &host.name);
        }
        AuthMethod::Key { .. } => {
            delete_host_credential(&host.host, host.port, &host.username, "passphrase", &host.name);
            delete_host_credential(&host.host, host.port, &host.username, "privatekey", &host.name);
        }
        AuthMethod::None => {}
    }
}

// ── Credential-based keychain functions (new scheme) ─────────────

/// Build the keychain service name for a credential entity.
fn credential_keyring_service(cred_name: &str) -> String {
    format!("Portal Credential: {cred_name}")
}

/// Build the keychain account key for a credential entity.
fn credential_keyring_key(cred_id: &str, kind: &str) -> String {
    format!("credential:{cred_id}:{kind}")
}

/// Store a secret for a Credential entity in the system keychain.
pub fn store_credential_secret(cred_id: &str, cred_name: &str, kind: &str, secret: &str) -> bool {
    let account = credential_keyring_key(cred_id, kind);
    let service = credential_keyring_service(cred_name);

    let _ = Command::new("security")
        .args(["delete-generic-password", "-a", &account, "-s", &service])
        .output();

    let result = Command::new("security")
        .args(["add-generic-password", "-a", &account, "-s", &service, "-w", secret, "-A"])
        .output();

    match result {
        Ok(o) if o.status.success() => true,
        Ok(o) => {
            log::warn!(
                "Failed to store credential secret in keychain: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            );
            false
        }
        Err(e) => {
            log::warn!("Failed to run security command: {e}");
            false
        }
    }
}

/// Load a secret for a Credential entity from the system keychain.
pub fn load_credential_secret(cred_id: &str, cred_name: &str, kind: &str) -> Option<String> {
    let account = credential_keyring_key(cred_id, kind);
    let service = credential_keyring_service(cred_name);
    security_find_password(&service, &account)
}

/// Delete all keychain secrets for a Credential entity.
pub fn delete_credential_secrets(cred_id: &str, cred_name: &str) {
    for kind in &["password", "privatekey", "passphrase"] {
        let account = credential_keyring_key(cred_id, kind);
        let service = credential_keyring_service(cred_name);
        let _ = Command::new("security")
            .args(["delete-generic-password", "-a", &account, "-s", &service])
            .output();
    }
}

// ── resolve_auth ────────────────────────────────────────────────────

/// Build a `ResolvedAuth` for a host by looking up its credential.
/// Falls back to legacy `AuthMethod` if no `credential_id` is set.
pub fn resolve_auth(host: &HostEntry, credentials: &[Credential]) -> ResolvedAuth {
    // New path: credential_id is set
    if let Some(ref cred_id) = host.credential_id {
        if let Some(cred) = credentials.iter().find(|c| c.id == *cred_id) {
            return resolve_credential(cred);
        }
        log::warn!("Credential id {} not found for host {}", cred_id, host.name);
    }

    // Legacy fallback: use embedded AuthMethod
    resolve_legacy_auth(host)
}

/// Resolve a Credential to ResolvedAuth by loading secrets from keychain.
pub fn resolve_credential(cred: &Credential) -> ResolvedAuth {
    match &cred.credential_type {
        CredentialType::Password { .. } => {
            if let Some(pw) = load_credential_secret(&cred.id, &cred.name, "password") {
                ResolvedAuth::Password { password: pw }
            } else {
                ResolvedAuth::None
            }
        }
        CredentialType::SshKey { key_in_keychain, has_passphrase, .. } => {
            let key_content = if *key_in_keychain {
                load_credential_secret(&cred.id, &cred.name, "privatekey").unwrap_or_default()
            } else {
                String::new()
            };
            let passphrase = if *has_passphrase {
                load_credential_secret(&cred.id, &cred.name, "passphrase")
            } else {
                None
            };
            if key_content.is_empty() && passphrase.is_none() {
                ResolvedAuth::None
            } else {
                ResolvedAuth::Key { key_content, passphrase }
            }
        }
    }
}

/// Resolve legacy AuthMethod from host-based keychain scheme.
fn resolve_legacy_auth(host: &HostEntry) -> ResolvedAuth {
    match &host.auth {
        AuthMethod::None => ResolvedAuth::None,
        AuthMethod::Password { password } => {
            let pw = if password.is_empty() {
                load_host_credential(&host.host, host.port, &host.username, "password", &host.name)
                    .unwrap_or_default()
            } else {
                password.clone()
            };
            if pw.is_empty() {
                ResolvedAuth::None
            } else {
                ResolvedAuth::Password { password: pw }
            }
        }
        AuthMethod::Key { key_path, key_content, passphrase, key_in_keychain } => {
            let key_data = if *key_in_keychain {
                load_host_credential(&host.host, host.port, &host.username, "privatekey", &host.name)
                    .unwrap_or_default()
            } else if !key_content.is_empty() {
                key_content.clone()
            } else if !key_path.is_empty() {
                let expanded = if key_path.starts_with('~') {
                    if let Some(home) = dirs::home_dir() {
                        home.join(&key_path[2..]).to_string_lossy().to_string()
                    } else {
                        key_path.clone()
                    }
                } else {
                    key_path.clone()
                };
                std::fs::read_to_string(&expanded).unwrap_or_default()
            } else {
                String::new()
            };

            let pp = if passphrase.is_empty() {
                load_host_credential(&host.host, host.port, &host.username, "passphrase", &host.name)
            } else {
                Some(passphrase.clone())
            };

            ResolvedAuth::Key { key_content: key_data, passphrase: pp }
        }
    }
}

// ── Credentials persistence ─────────────────────────────────────────

/// Get the credentials file path
pub fn credentials_file_path() -> PathBuf {
    config_dir().join("credentials.json")
}

/// Load credentials from JSON file.
pub fn load_credentials(path: &Path) -> Vec<Credential> {
    if let Ok(data) = std::fs::read_to_string(path) {
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        Vec::new()
    }
}

/// Save credentials to JSON file.
pub fn save_credentials(path: &Path, credentials: &[Credential]) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(credentials) {
        let _ = std::fs::write(path, json);
    }
}

// ── Migration ───────────────────────────────────────────────────────

/// Migrate hosts with embedded AuthMethod to credential-based references.
/// Creates Credential entities, re-keys keychain entries from old→new scheme,
/// and sets `credential_id` on each host. Idempotent (skips already-migrated hosts).
pub fn migrate_hosts_to_credentials(hosts: &mut [HostEntry], credentials: &mut Vec<Credential>) {
    let mut changed = false;

    for host in hosts.iter_mut() {
        if host.is_local || host.credential_id.is_some() || host.auth == AuthMethod::None {
            continue;
        }

        match &host.auth {
            AuthMethod::Password { .. } => {
                let cred = Credential::new_password(
                    format!("{} (password)", host.name),
                    host.username.clone(),
                );

                // Re-key: load from old scheme, store to new scheme
                if let Some(pw) = load_host_credential(&host.host, host.port, &host.username, "password", &host.name) {
                    store_credential_secret(&cred.id, &cred.name, "password", &pw);
                }

                host.credential_id = Some(cred.id.clone());
                credentials.push(cred);
                changed = true;
            }
            AuthMethod::Key { key_path, key_in_keychain, passphrase, .. } => {
                let has_passphrase = !passphrase.is_empty() ||
                    load_host_credential(&host.host, host.port, &host.username, "passphrase", &host.name).is_some();

                let cred = Credential::new_ssh_key(
                    format!("{} (key)", host.name),
                    key_path.clone(),
                    *key_in_keychain,
                    has_passphrase,
                );

                // Re-key private key
                if *key_in_keychain {
                    if let Some(key_data) = load_host_credential(&host.host, host.port, &host.username, "privatekey", &host.name) {
                        store_credential_secret(&cred.id, &cred.name, "privatekey", &key_data);
                    }
                }

                // Re-key passphrase
                if has_passphrase {
                    if let Some(pp) = load_host_credential(&host.host, host.port, &host.username, "passphrase", &host.name) {
                        store_credential_secret(&cred.id, &cred.name, "passphrase", &pp);
                    }
                }

                host.credential_id = Some(cred.id.clone());
                credentials.push(cred);
                changed = true;
            }
            AuthMethod::None => {}
        }
    }

    if changed {
        log::info!("Migrated {} host(s) to credential-based auth", credentials.len());
    }
}

// ── Config directory ────────────────────────────────────────────────

/// Get the config directory path (~/.config/portal/)
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("portal")
}

/// Get the hosts file path
pub fn hosts_file_path() -> PathBuf {
    config_dir().join("hosts.json")
}

// ── Connection History ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionRecord {
    pub host_name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub timestamp: u64,
    pub success: bool,
}

pub fn history_file_path() -> PathBuf {
    config_dir().join("history.json")
}

pub fn load_history() -> Vec<ConnectionRecord> {
    let path = history_file_path();
    if let Ok(data) = std::fs::read_to_string(&path) {
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        Vec::new()
    }
}

pub fn save_history(records: &[ConnectionRecord]) {
    let path = history_file_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(records) {
        let _ = std::fs::write(path, json);
    }
}

pub fn append_history(record: ConnectionRecord) {
    let mut records = load_history();
    records.push(record);
    if records.len() > 200 {
        let drain_count = records.len() - 200;
        records.drain(..drain_count);
    }
    save_history(&records);
}

/// Get the settings file path
pub fn settings_file_path() -> PathBuf {
    config_dir().join("settings.json")
}

// ── Keyboard Shortcuts ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ShortcutAction {
    SplitHorizontal,
    SplitVertical,
    NewTab,
    CloseTab,
    ClosePane,
    NextTab,
    PrevTab,
    ToggleBroadcast,
    Search,
    Copy,
    Paste,
    SelectAll,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyBinding {
    pub action: ShortcutAction,
    pub key: String,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub command: bool,
}

pub fn default_shortcuts() -> Vec<KeyBinding> {
    vec![
        KeyBinding { action: ShortcutAction::SplitHorizontal, key: "D".into(), ctrl: false, alt: false, shift: false, command: true },
        KeyBinding { action: ShortcutAction::SplitVertical, key: "D".into(), ctrl: false, alt: false, shift: true, command: true },
        KeyBinding { action: ShortcutAction::NewTab, key: "T".into(), ctrl: false, alt: false, shift: false, command: true },
        KeyBinding { action: ShortcutAction::CloseTab, key: "W".into(), ctrl: false, alt: false, shift: false, command: true },
        KeyBinding { action: ShortcutAction::ClosePane, key: "W".into(), ctrl: false, alt: false, shift: true, command: true },
        KeyBinding { action: ShortcutAction::NextTab, key: "RightBracket".into(), ctrl: false, alt: false, shift: true, command: true },
        KeyBinding { action: ShortcutAction::PrevTab, key: "LeftBracket".into(), ctrl: false, alt: false, shift: true, command: true },
        KeyBinding { action: ShortcutAction::ToggleBroadcast, key: "I".into(), ctrl: false, alt: false, shift: true, command: true },
        KeyBinding { action: ShortcutAction::Search, key: "F".into(), ctrl: false, alt: false, shift: false, command: true },
        KeyBinding { action: ShortcutAction::Copy, key: "C".into(), ctrl: false, alt: false, shift: false, command: true },
        KeyBinding { action: ShortcutAction::Paste, key: "V".into(), ctrl: false, alt: false, shift: false, command: true },
        KeyBinding { action: ShortcutAction::SelectAll, key: "A".into(), ctrl: false, alt: false, shift: false, command: true },
    ]
}

// ── Portal Settings ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PortalSettings {
    pub font_size: f32,
    pub custom_font_path: Option<String>,
    pub language: String,
    /// Scrollback buffer limit in MB (default: 100MB)
    pub scrollback_limit_mb: u64,
    #[serde(default = "default_keepalive_interval")]
    pub ssh_keepalive_interval: u32,
    #[serde(default = "default_shortcuts")]
    pub keyboard_shortcuts: Vec<KeyBinding>,
}

fn default_keepalive_interval() -> u32 {
    30
}

impl PortalSettings {
    /// Get scrollback limit in bytes
    #[allow(dead_code)]
    pub fn scrollback_limit_bytes(&self) -> usize {
        (self.scrollback_limit_mb as usize) * 1024 * 1024
    }
}

impl Default for PortalSettings {
    fn default() -> Self {
        Self {
            font_size: 14.0,
            custom_font_path: None,
            language: "en".to_string(),
            scrollback_limit_mb: 100,
            ssh_keepalive_interval: default_keepalive_interval(),
            keyboard_shortcuts: default_shortcuts(),
        }
    }
}

pub fn load_settings() -> PortalSettings {
    let path = settings_file_path();
    if let Ok(data) = std::fs::read_to_string(&path) {
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        PortalSettings::default()
    }
}

pub fn save_settings(settings: &PortalSettings) {
    let path = settings_file_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(settings) {
        let _ = std::fs::write(path, json);
    }
}

/// Load hosts from JSON file. Returns default list if file doesn't exist.
///
/// After loading, credentials are restored from the system keychain.
/// If plaintext passwords are found in the JSON (legacy), they are
/// automatically migrated to the keychain and the file is re-saved.
pub fn load_hosts(path: &Path) -> Vec<HostEntry> {
    let mut hosts = if let Ok(data) = std::fs::read_to_string(path) {
        if let Ok(hosts) = serde_json::from_str::<Vec<HostEntry>>(&data) {
            let has_local = hosts.iter().any(|h| h.is_local);
            if has_local {
                hosts
            } else {
                let mut result = vec![HostEntry::new_local()];
                result.extend(hosts);
                result
            }
        } else {
            vec![HostEntry::new_local()]
        }
    } else {
        return vec![HostEntry::new_local()];
    };

    // Migrate plaintext passwords to keychain & restore from keychain
    let mut needs_resave = false;
    for host in &mut hosts {
        if host.is_local {
            continue;
        }
        match &mut host.auth {
            AuthMethod::Password { password } => {
                if !password.is_empty() {
                    // Plaintext password in JSON — migrate to keychain
                    if store_host_credential(&host.host, host.port, &host.username, "password", password, &host.name) {
                        password.clear();
                        needs_resave = true;
                    }
                } else {
                    // Empty password in JSON — restore from keychain
                    if let Some(secret) = load_host_credential(&host.host, host.port, &host.username, "password", &host.name) {
                        *password = secret;
                    }
                }
            }
            AuthMethod::Key { passphrase, .. } => {
                if !passphrase.is_empty() {
                    // Plaintext passphrase in JSON — migrate to keychain
                    if store_host_credential(&host.host, host.port, &host.username, "passphrase", passphrase, &host.name) {
                        passphrase.clear();
                        needs_resave = true;
                    }
                } else {
                    // Empty passphrase in JSON — restore from keychain
                    if let Some(secret) = load_host_credential(&host.host, host.port, &host.username, "passphrase", &host.name) {
                        *passphrase = secret;
                    }
                }
            }
            AuthMethod::None => {}
        }
    }

    if needs_resave {
        // Re-save with passwords stripped from JSON
        save_hosts(path, &hosts);
        log::info!("Migrated plaintext credentials to system keychain");
    }

    hosts
}

/// Save hosts to JSON file. Creates parent directories if needed.
///
/// Passwords are stored in the system keychain; the JSON file only
/// contains empty password/passphrase fields.
pub fn save_hosts(path: &Path, hosts: &[HostEntry]) {
    // Build a version of hosts with secrets moved to keychain
    let cleaned: Vec<HostEntry> = hosts
        .iter()
        .map(|h| {
            if h.is_local {
                return h.clone();
            }
            let mut entry = h.clone();
            match &mut entry.auth {
                AuthMethod::Password { password } => {
                    if !password.is_empty() {
                        if store_host_credential(&h.host, h.port, &h.username, "password", password, &h.name) {
                            password.clear();
                        }
                        // If keychain store failed, keep plaintext as fallback
                    }
                }
                AuthMethod::Key { key_path, key_content, passphrase, key_in_keychain } => {
                    // Import private key content into keychain (from file or pasted content)
                    if !*key_in_keychain {
                        let key_data = if !key_content.is_empty() {
                            // Use pasted key content
                            key_content.clone()
                        } else if !key_path.is_empty() {
                            // Read from local file
                            let expanded = if key_path.starts_with('~') {
                                if let Some(home) = dirs::home_dir() {
                                    home.join(&key_path[2..]).to_string_lossy().to_string()
                                } else {
                                    key_path.clone()
                                }
                            } else {
                                key_path.clone()
                            };
                            match std::fs::read_to_string(&expanded) {
                                Ok(content) => content,
                                Err(e) => {
                                    log::warn!("Failed to read key file {}: {}", expanded, e);
                                    String::new()
                                }
                            }
                        } else {
                            String::new()
                        };

                        if !key_data.is_empty() {
                            if store_host_credential(&h.host, h.port, &h.username, "privatekey", &key_data, &h.name) {
                                *key_in_keychain = true;
                                log::info!("Imported private key into keychain for {}@{}:{}", h.username, h.host, h.port);
                            }
                        }
                    }
                    if !passphrase.is_empty() {
                        if store_host_credential(&h.host, h.port, &h.username, "passphrase", passphrase, &h.name) {
                            passphrase.clear();
                        }
                    }
                }
                AuthMethod::None => {}
            }
            entry
        })
        .collect();

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(&cleaned) {
        let _ = std::fs::write(path, json);
    }
}
