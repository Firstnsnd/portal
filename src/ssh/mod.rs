//! SSH connection support

pub mod agent;
mod auth;
mod auth_prompt;
pub mod capability;
pub mod port_forward;
pub mod proxy;
mod session;
pub mod socks5;

#[allow(unused_imports)]
pub use auth::{authenticate, connect_and_authenticate, open_ssh_connection, HostKeyPolicy};
#[allow(unused_imports)]
pub use auth_prompt::{AuthPrompt, AuthPromptBridge, AuthPromptField, PromptMode};
pub use port_forward::{AppNotification, NotificationLevel};
#[allow(unused_imports)]
pub use session::{
    connect_via_jump, remove_known_hosts_key, test_connection, JumpHostInfo, SshClient,
    SshConnectionState, SshSession,
};

#[cfg(test)]
mod tests {
    include!("tests.rs");
}
