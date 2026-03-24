//! # View Modules
//!
//! This module contains all view-specific UI implementations.
//! Each view is responsible for rendering a specific page/feature of the application.
//!
//! The view modules provide `PortalApp` impl methods for rendering:
//! - **nav_panel**: Left navigation panel (shared across all views)
//! - **tab_view**: Tab bar component (shared across Terminal and detached windows)
//! - **sftp_view**: SFTP file browser
//! - **sftp**: SFTP view types and helpers
//! - **hosts_view**: SSH host management
//! - **settings_view**: Application settings
//! - **keychain_view**: System keychain management
//! - **snippet_view**: Command snippets
//! - **tunnel_view**: SSH port forwarding (tunnels)

pub mod nav_panel;
pub mod tab_view;
pub mod sftp;
pub mod sftp_view;
pub mod hosts_view;
pub mod settings_view;
pub mod keychain_view;
pub mod snippet_view;
pub mod tunnel_view;
