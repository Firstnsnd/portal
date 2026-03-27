//! # View Modules
//!
//! This module contains all view-specific UI implementations.
//! Each view is responsible for rendering a specific page/feature of the application.
//!
//! The view modules provide:
//! - **nav_panel**: Left navigation panel (shared across all views)
//! - **tab_view**: Tab bar component (shared across Terminal and detached windows)
//! - **sftp**: SFTP view types and helpers
//! - **sftp_view**: SFTP file browser view
//! - **settings_view**: Application settings
//! - **hosts_view**: Hosts management view
//! - **keychain_view**: Credentials/keychain view
//! - **snippets_view**: Command snippets view
//! - **tunnels_view**: SSH tunnel management view

pub mod nav_panel;
pub mod tab_view;
pub mod sftp;
pub mod sftp_view;
pub mod settings_view;
pub mod hosts_view;
pub mod keychain_view;
pub mod snippets_view;
pub mod tunnels_view;
