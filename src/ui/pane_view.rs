//! # Pane View Rendering
//!
//! This module implements view rendering methods for `AppWindow`.
//! Each window renders its own views, receiving shared data via `WindowContext`.

use eframe::egui;
use std::path::PathBuf;

use crate::config::{HostEntry, Credential, ConnectionRecord, Snippet};
use crate::ui::types::dialogs::AppView;
use crate::ui::pane::AppWindow;
use crate::ui::{ThemeColors, Language};

// Import view rendering functions
use crate::ui::views::{hosts_view, keychain_view, snippets_view, tunnels_view, sftp_view};

/// Actions that a view may request to be performed after rendering.
/// These are returned from view methods and executed by the caller.
#[derive(Default)]
pub struct ViewActions {
    /// Request to create a new local terminal tab
    pub add_local_tab: bool,
    /// Request to connect to an SSH host
    pub connect_ssh: Option<HostEntry>,
    /// Request to clear connection history
    pub clear_history: bool,
    /// Request to save hosts to file
    pub save_hosts: bool,
    /// Request to save credentials to file
    pub save_credentials: bool,
    /// Request to save snippets to file
    pub save_snippets: bool,
    /// Request to delete a host (index into hosts)
    pub delete_host: Option<usize>,
    /// Request to delete a credential (id)
    pub delete_credential: Option<String>,
    /// Request to delete a snippet (id)
    pub delete_snippet: Option<String>,
}

/// Shared context passed to window view methods.
/// Contains references to data shared across all windows.
pub struct WindowContext<'a> {
    // Shared data
    pub hosts: &'a mut Vec<HostEntry>,
    pub credentials: &'a mut Vec<Credential>,
    pub snippets: &'a mut Vec<Snippet>,
    pub connection_history: &'a mut Vec<ConnectionRecord>,

    // File paths
    pub hosts_file: &'a PathBuf,
    pub credentials_file: &'a PathBuf,

    // Settings and theme
    pub theme: &'a ThemeColors,
    pub language: Language,

    // Runtime for async operations
    pub runtime: &'a tokio::runtime::Runtime,

    // Font settings
    pub font_size: f32,
}

impl<'a> WindowContext<'a> {
    /// Create a new window context
    pub fn new(
        hosts: &'a mut Vec<HostEntry>,
        credentials: &'a mut Vec<Credential>,
        snippets: &'a mut Vec<Snippet>,
        connection_history: &'a mut Vec<ConnectionRecord>,
        hosts_file: &'a PathBuf,
        credentials_file: &'a PathBuf,
        theme: &'a ThemeColors,
        language: Language,
        runtime: &'a tokio::runtime::Runtime,
        font_size: f32,
    ) -> Self {
        Self {
            hosts,
            credentials,
            snippets,
            connection_history,
            hosts_file,
            credentials_file,
            theme,
            language,
            runtime,
            font_size,
        }
    }
}

impl AppWindow {
    /// Render the central panel content for this window
    pub fn render_central_content(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        cx: &mut WindowContext,
    ) -> ViewActions {
        match self.current_view {
            AppView::Terminal => {
                // Terminal rendering is handled separately
                ViewActions::default()
            }
            AppView::Hosts => {
                hosts_view::render_hosts_view(self, ctx, ui, cx)
            }
            AppView::Sftp => {
                sftp_view::render_sftp_view(self, ui, cx)
            }
            AppView::Keychain => {
                keychain_view::render_keychain_view(self, ctx, ui, cx)
            }
            AppView::Settings => {
                // Settings rendering
                ViewActions::default()
            }
            AppView::Snippets => {
                snippets_view::render_snippets_view(self, ctx, ui, cx)
            }
            AppView::Tunnels => {
                tunnels_view::render_tunnels_view(self, ctx, ui, cx)
            }
        }
    }


    /// Render drawers (right panels) for this window
    pub fn render_drawers(
        &mut self,
        ctx: &egui::Context,
        cx: &mut WindowContext,
    ) {
        // Host add/edit drawer
        if self.current_view == AppView::Hosts && self.add_host_dialog.open {
            hosts_view::render_add_host_drawer(self, ctx, cx);
        }

        // Credential drawer
        if self.current_view == AppView::Keychain && self.credential_dialog.open {
            keychain_view::render_credential_drawer(self, ctx, cx);
        }

        // Snippet drawer (snippets page)
        if self.current_view == AppView::Snippets && self.snippet_view_state.open {
            snippets_view::render_snippet_drawer(self, ctx, cx);
        }

        // Terminal snippet drawer (quick-access snippet list)
        if self.current_view == AppView::Terminal {
            snippets_view::render_terminal_snippet_drawer(self, ctx, cx);
        }

        // Tunnel drawer
        if self.current_view == AppView::Tunnels && self.add_tunnel_dialog.open {
            tunnels_view::render_tunnel_drawer(self, ctx, cx);
        }
    }
}
