//! SSH connection support

mod auth;
mod auth_prompt;
mod session;
pub mod port_forward;

#[allow(unused_imports)]
pub use session::{SshSession, SshConnectionState, SshClient, JumpHostInfo, connect_via_jump, test_connection, remove_known_hosts_key};
#[allow(unused_imports)]
pub use auth::{HostKeyPolicy, open_ssh_connection, authenticate, connect_and_authenticate};
#[allow(unused_imports)]
pub use auth_prompt::{AuthPrompt, AuthPromptBridge, AuthPromptField, PromptMode, PromptError};
pub use port_forward::{AppNotification, NotificationLevel};

#[cfg(test)]
mod tests {
    include!("tests.rs");
}
