//! SSH connection support

mod session;
pub mod port_forward;

#[allow(unused_imports)]
pub use session::{SshSession, SshConnectionState, SshClient, JumpHostInfo, test_connection, connect_and_authenticate, remove_known_hosts_key};
pub use port_forward::{AppNotification, NotificationLevel};

#[cfg(test)]
mod tests {
    include!("tests.rs");
}
