//! SSH transport + authentication driver.
//!
//! Single home for the auth dispatch that was previously duplicated across
//! `connect_and_authenticate`, the jump-host target leg, and the SFTP task.
//! Keyboard-interactive (2FA) fallback lives here too.

use std::sync::Arc;
use std::time::Duration;

use crate::config::ResolvedAuth;

use super::auth_prompt::{AuthPromptField, AuthPromptBridge, PromptError};
use super::session::SshClient;

/// Cap on keyboard-interactive rounds. Real servers use 1-3; a malicious
/// or buggy server sending endless 0-prompt info rounds must not hang the
/// auth task forever.
pub const MAX_KBDINT_ROUNDS: usize = 20;

/// How `check_server_key` treats host keys.
///
/// * `Learn` — production behavior: known keys pass, unknown keys are
///   auto-added to known_hosts, changed keys are a hard MITM error.
/// * `AcceptAll` — accept any key without touching known_hosts.
///   Only reachable through [`open_ssh_connection`] — tests only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKeyPolicy {
    Learn,
    AcceptAll,
}

/// Open the transport (TCP + key exchange) without authenticating.
///
/// `keepalive` mirrors the previous per-caller russh configs: the direct
/// path always used (15s, 30); the jump-target leg only enables it when
/// the configured interval is > 0. The `usize` is russh's `keepalive_max`.
pub async fn open_ssh_connection(
    host: &str,
    port: u16,
    policy: HostKeyPolicy,
    keepalive: Option<(Duration, usize)>,
) -> Result<russh::client::Handle<SshClient>, String> {
    let mut config = russh::client::Config::default();
    if let Some((interval, max)) = keepalive {
        config.keepalive_interval = Some(interval);
        config.keepalive_max = max;
    }
    let config = Arc::new(config);
    let addr = format!("{}:{}", host, port);

    let client = SshClient::with_policy(host, port, policy);

    russh::client::connect(config, &addr, client)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("KeyChanged") || msg.contains("key changed") {
                format!("Host key verification failed: server key has changed for {}:{}.\nThis could indicate a MITM attack.\nRemove the old key from ~/.ssh/known_hosts to connect.", host, port)
            } else {
                format!("Connect failed: {}", e)
            }
        })
}

/// Authenticate an open transport:
/// 1. try the configured method (password / publickey);
/// 2. on failure, if the server advertises keyboard-interactive, fall
///    back to it (RFC 4256) — surfacing each prompt round to the user
///    through the bridge (2FA / OTP servers).
pub async fn authenticate(
    handle: &mut russh::client::Handle<SshClient>,
    username: &str,
    auth: &ResolvedAuth,
    bridge: &AuthPromptBridge,
) -> Result<(), String> {
    let result: russh::client::AuthResult = match auth {
        ResolvedAuth::Password { password } => {
            handle
                .authenticate_password(username, password)
                .await
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
                .map_err(|e| format!("Auth error: {}", e))?
        }
        ResolvedAuth::None => return Err("No authentication configured".to_string()),
    };

    match result {
        russh::client::AuthResult::Success => Ok(()),
        russh::client::AuthResult::Failure { remaining_methods, .. } => {
            // Rejections are Ok(Failure), not Err — Err means transport.
            if remaining_methods.contains(&russh::MethodKind::KeyboardInteractive) {
                keyboard_interactive(handle, username, bridge).await
            } else {
                Err("Authentication failed".to_string())
            }
        }
    }
}

/// RFC 4256 keyboard-interactive loop. Info-only rounds (0 prompts) are
/// auto-answered without pausing for the user; prompt rounds go through
/// the bridge. Capped at [`MAX_KBDINT_ROUNDS`].
async fn keyboard_interactive(
    handle: &mut russh::client::Handle<SshClient>,
    username: &str,
    bridge: &AuthPromptBridge,
) -> Result<(), String> {
    // Note: submethods must be None::<String> — a bare `None` (or `&str`)
    // cannot satisfy russh's `S: Into<Option<String>>` bound.
    let mut resp = handle
        .authenticate_keyboard_interactive_start(username.to_string(), None::<String>)
        .await
        .map_err(|e| format!("Auth error: {}", e))?;

    for _round in 0..MAX_KBDINT_ROUNDS {
        match resp {
            russh::client::KeyboardInteractiveAuthResponse::Success => return Ok(()),
            russh::client::KeyboardInteractiveAuthResponse::Failure {
                remaining_methods,
                partial_success,
            } => {
                log::info!(
                    "keyboard-interactive rejected (partial_success={partial_success}, remaining={remaining_methods:?})"
                );
                return Err("Authentication failed".to_string());
            }
            russh::client::KeyboardInteractiveAuthResponse::InfoRequest {
                name,
                instructions,
                prompts,
            } => {
                let answers = if prompts.is_empty() {
                    // Info-only round (banner): answer empty, no UI pause.
                    Vec::new()
                } else {
                    let fields = prompts
                        .into_iter()
                        .map(|p| AuthPromptField { prompt: p.prompt, echo: p.echo })
                        .collect();
                    match bridge.ask(name, instructions, fields).await {
                        Ok(answers) => answers,
                        Err(PromptError::Cancelled) => {
                            return Err("Authentication cancelled".to_string())
                        }
                        Err(PromptError::Declined) => {
                            return Err(
                                "Server requires interactive two-factor authentication; \
                                 open a terminal session to connect"
                                    .to_string(),
                            )
                        }
                    }
                };
                resp = handle
                    .authenticate_keyboard_interactive_respond(answers)
                    .await
                    .map_err(|e| format!("Auth error: {}", e))?;
            }
        }
    }
    Err("Too many authentication rounds".to_string())
}

/// Connect and authenticate an SSH session, returning the handle.
/// Shared by SshSession, test_connection, and SFTP.
pub async fn connect_and_authenticate(
    host: &str,
    port: u16,
    username: &str,
    auth: &ResolvedAuth,
    _keepalive_interval: u32,
    _agent_forwarding: bool,
    bridge: &AuthPromptBridge,
) -> Result<russh::client::Handle<SshClient>, String> {
    let mut handle = open_ssh_connection(
        host,
        port,
        HostKeyPolicy::Learn,
        Some((Duration::from_secs(15), 30)),
    )
    .await?;

    authenticate(&mut handle, username, auth, bridge).await?;

    Ok(handle)
}
