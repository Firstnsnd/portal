//! ssh-agent client facade — per-OS transports behind one uniform type.
//!
//! The authentication flow (list identities, offer each to the server,
//! first accepted key wins) is shared; only the transport differs:
//!
//! - [`unix`]: `SSH_AUTH_SOCK` unix-domain socket (`connect_env`).
//! - [`windows`]: the OpenSSH for Windows agent named pipe first, then
//!   Pageant (PuTTY's resident key store) as fallback.
//!
//! Both hand back a `.dynamic()`-erased [`PlatformAgentClient`], so the
//! shared authenticate loop below is generic-free and the caller (auth.rs)
//! has no platform branches.

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use unix::{connect_agent, no_keys_hint, ConcreteStream};
#[cfg(windows)]
pub use windows::{connect_agent, no_keys_hint, ConcreteStream};

use super::session::SshClient;

/// Cap on how many ssh-agent keys one login may offer. Mirrors OpenSSH's
/// `IdentitiesOnly`-style restraint: each rejected key burns one of the
/// server's MaxAuthTries, so a keychain with dozens of identities must not
/// lock the account out on its own.
pub const MAX_AGENT_KEYS: usize = 6;

/// The agent client type every platform hands back, with its concrete
/// per-OS stream ([`ConcreteStream`]: unix socket / enum'd pipe+Pageant).
/// No `dyn` here — a boxed stream type parameter through russh's generic
/// async fns trips rustc's incomplete "Send is not general enough" check.
pub type PlatformAgentClient =
    russh::keys::agent::client::AgentClient<crate::ssh::agent::ConcreteStream>;

/// ssh-agent authentication (OpenSSH-style): ask the local agent for its
/// identities and offer each one to the server, letting the agent produce
/// the signatures. First accepted key wins.
///
/// Returns `Ok(true)` when a key authenticated, `Ok(false)` when every key
/// was rejected (caller decides on fallback), `Err` when the agent itself
/// is unreachable or errors (no agent running, dead socket/pipe, protocol
/// failure) — those are environment problems, not auth rejections.
pub async fn authenticate(
    handle: &mut russh::client::Handle<SshClient>,
    username: &str,
) -> Result<bool, String> {
    let mut agent = connect_agent().await?;

    let keys = agent
        .request_identities()
        .await
        .map_err(|e| format!("ssh-agent key listing failed: {}", e))?;
    log::info!("ssh-agent: {} key(s) offered for {}", keys.len(), username);
    if keys.is_empty() {
        log::warn!("ssh-agent holds no keys — {}", no_keys_hint());
    }

    // RSA keys sign with the server-negotiated hash (same as the direct path).
    let rsa_hash = handle
        .best_supported_rsa_hash()
        .await
        .ok()
        .flatten()
        .flatten();

    for key in keys.into_iter().take(MAX_AGENT_KEYS) {
        match handle
            .authenticate_publickey_with(username, key, rsa_hash, &mut agent)
            .await
        {
            Ok(russh::client::AuthResult::Success) => return Ok(true),
            Ok(_) => continue, // rejected — try the next identity
            Err(e) => return Err(format!("ssh-agent auth error: {}", e)),
        }
    }
    Ok(false)
}
