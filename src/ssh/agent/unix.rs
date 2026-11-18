//! ssh-agent transport — unix: `SSH_AUTH_SOCK` unix-domain socket.

use super::PlatformAgentClient;

/// Connect to the user's ssh-agent via `SSH_AUTH_SOCK`, transport-erased.
/// Concrete stream type on this platform (no dyn — dyn through async fns
/// trips rustc's incomplete Send check).
pub type ConcreteStream = tokio::net::UnixStream;

/// Connect to the user's ssh-agent via `SSH_AUTH_SOCK`.
pub async fn connect_agent() -> Result<PlatformAgentClient, String> {
    russh::keys::agent::client::AgentClient::connect_env()
        .await
        .map_err(|e| format!("ssh-agent unavailable: {}", e))
}

/// Actionable hint for the "agent reachable but empty" log line.
pub fn no_keys_hint() -> &'static str {
    "run ssh-add, or check SSH_AUTH_SOCK if you expected identities"
}
