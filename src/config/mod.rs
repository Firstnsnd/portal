//! Configuration management

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Authentication method for SSH connections
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
    pub auth: AuthMethod,
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
            auth: AuthMethod::None,
        }
    }

    pub fn new_ssh(name: String, host: String, port: u16, username: String, group: String, auth: AuthMethod) -> Self {
        Self {
            name,
            host,
            port,
            username,
            group,
            tags: Vec::new(),
            is_local: false,
            auth,
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
fn store_credential(host: &str, port: u16, username: &str, kind: &str, secret: &str, display_name: &str) -> bool {
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

/// Load a credential from the system keychain.
/// Tries new per-host service first, then falls back to legacy "portal-ssh".
pub fn load_credential(host: &str, port: u16, username: &str, kind: &str, display_name: &str) -> Option<String> {
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

/// Delete a credential from the system keychain (both new and legacy service).
pub fn delete_credential(host: &str, port: u16, username: &str, kind: &str, display_name: &str) {
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

/// Delete all keychain entries associated with a host entry.
pub fn delete_host_credentials(host: &HostEntry) {
    match &host.auth {
        AuthMethod::Password { .. } => {
            delete_credential(&host.host, host.port, &host.username, "password", &host.name);
        }
        AuthMethod::Key { .. } => {
            delete_credential(&host.host, host.port, &host.username, "passphrase", &host.name);
            delete_credential(&host.host, host.port, &host.username, "privatekey", &host.name);
        }
        AuthMethod::None => {}
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

/// Get the settings file path
pub fn settings_file_path() -> PathBuf {
    config_dir().join("settings.json")
}

// ── Portal Settings ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortalSettings {
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default)]
    pub custom_font_path: Option<String>,
    #[serde(default = "default_language")]
    pub language: String,
}

fn default_font_size() -> f32 { 14.0 }
fn default_language() -> String { "en".to_string() }

impl Default for PortalSettings {
    fn default() -> Self {
        Self {
            font_size: 14.0,
            custom_font_path: None,
            language: "en".to_string(),
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
                    if store_credential(&host.host, host.port, &host.username, "password", password, &host.name) {
                        password.clear();
                        needs_resave = true;
                    }
                } else {
                    // Empty password in JSON — restore from keychain
                    if let Some(secret) = load_credential(&host.host, host.port, &host.username, "password", &host.name) {
                        *password = secret;
                    }
                }
            }
            AuthMethod::Key { passphrase, .. } => {
                if !passphrase.is_empty() {
                    // Plaintext passphrase in JSON — migrate to keychain
                    if store_credential(&host.host, host.port, &host.username, "passphrase", passphrase, &host.name) {
                        passphrase.clear();
                        needs_resave = true;
                    }
                } else {
                    // Empty passphrase in JSON — restore from keychain
                    if let Some(secret) = load_credential(&host.host, host.port, &host.username, "passphrase", &host.name) {
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
                        if store_credential(&h.host, h.port, &h.username, "password", password, &h.name) {
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
                            if store_credential(&h.host, h.port, &h.username, "privatekey", &key_data, &h.name) {
                                *key_in_keychain = true;
                                log::info!("Imported private key into keychain for {}@{}:{}", h.username, h.host, h.port);
                            }
                        }
                    }
                    if !passphrase.is_empty() {
                        if store_credential(&h.host, h.port, &h.username, "passphrase", passphrase, &h.name) {
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
