//! Tab management functionality for PortalApp

use crate::config::{HostEntry, resolve_auth, ResolvedAuth};
use crate::ssh::JumpHostInfo;
use crate::ui::pane::{SplitDirection, PaneNode, Tab};
use crate::ui::types::{session::TerminalSession, dialogs::AppView};
use super::PortalApp;

impl PortalApp {
    /// Add a new local terminal tab to the main window (index 0)
    pub fn add_tab_local(&mut self) {
        self.add_tab_local_to_window(0)
    }

    /// Add a new local terminal tab to a specific window
    pub fn add_tab_local_to_window(&mut self, window_idx: usize) {
        let window = if let Some(w) = self.windows.get_mut(window_idx) {
            w
        } else {
            return;
        };

        let id = window.next_id;
        window.next_id += 1;
        let default_shell = std::env::var("SHELL")
            .unwrap_or_else(|_| "/bin/zsh".to_string());
        let tab = Tab {
            title: format!("Terminal {}", id),
            sessions: vec![TerminalSession::new_local(id, &default_shell)],
            layout: PaneNode::Terminal(0),
            focused_session: 0,
            broadcast_enabled: false,
            snippet_drawer_open: false,
        };
        window.tabs.push(tab);
        window.active_tab = window.tabs.len() - 1;
        window.current_view = AppView::Terminal;
    }

    pub(crate) fn resolve_jump_host(&self, host: &HostEntry) -> Option<JumpHostInfo> {
        let jump_name = host.jump_host.as_ref()?;
        let jump_entry = self.hosts.iter().find(|h| &h.name == jump_name)?;
        let jump_auth = resolve_auth(jump_entry, &self.credentials);
        if matches!(jump_auth, ResolvedAuth::None) {
            return None;
        }
        Some(JumpHostInfo {
            host: jump_entry.host.clone(),
            port: jump_entry.port,
            username: TerminalSession::get_effective_username(&jump_entry.username),
            auth: jump_auth,
        })
    }

    /// Add a new SSH terminal tab to the main window (index 0)
    pub fn add_tab_ssh(&mut self, host: &HostEntry) {
        self.add_tab_ssh_to_window(0, host)
    }

    /// Add a new SSH terminal tab to a specific window
    pub fn add_tab_ssh_to_window(&mut self, window_idx: usize, host: &HostEntry) {
        // First, collect all data needed before borrowing windows
        let auth = resolve_auth(host, &self.credentials);
        let jump = self.resolve_jump_host(host);
        let session = TerminalSession::new_ssh(host, auth, &self.runtime, jump);
        let tab = Tab {
            title: host.name.clone(),
            sessions: vec![session],
            layout: PaneNode::Terminal(0),
            focused_session: 0,
            broadcast_enabled: false,
            snippet_drawer_open: false,
        };

        let window = if let Some(w) = self.windows.get_mut(window_idx) {
            w
        } else {
            return;
        };

        window.tabs.push(tab);
        window.active_tab = window.tabs.len() - 1;
        window.current_view = AppView::Terminal;
        self.connection_history = crate::config::load_history();
    }

    /// Split the currently focused pane in a specific window
    pub fn split_focused_pane_in_window(&mut self, window_idx: usize, direction: SplitDirection) {
        // First, extract data from window without mutation
        let (new_id, active_tab, old_idx, ssh_host, resolved_auth) = {
            let window = if let Some(w) = self.windows.get(window_idx) {
                w
            } else {
                return;
            };
            let new_id = window.next_id;
            let active_tab = window.active_tab;
            let tab = &window.tabs[active_tab];
            let old_idx = tab.focused_session;
            let ssh_host = tab.sessions.get(old_idx).and_then(|s| s.ssh_host.clone());
            let resolved_auth = tab.sessions.get(old_idx).and_then(|s| s.resolved_auth.clone());
            (new_id, active_tab, old_idx, ssh_host, resolved_auth)
        };

        // Create new session (may need self for resolve_jump_host)
        let new_session = if let Some(host) = &ssh_host {
            let auth = resolved_auth.unwrap_or(resolve_auth(host, &self.credentials));
            let jump = self.resolve_jump_host(host);
            TerminalSession::new_ssh(host, auth, &self.runtime, jump)
        } else {
            let default_shell = std::env::var("SHELL")
                .unwrap_or_else(|_| "/bin/zsh".to_string());
            TerminalSession::new_local(new_id, &default_shell)
        };

        // Now mutate window
        let window = if let Some(w) = self.windows.get_mut(window_idx) {
            w
        } else {
            return;
        };
        window.next_id += 1;

        let tab = &mut window.tabs[active_tab];
        tab.sessions.push(new_session);
        let new_idx = tab.sessions.len() - 1;
        tab.layout.replace(old_idx, PaneNode::Split {
            direction,
            ratio: 0.5,
            first: Box::new(PaneNode::Terminal(old_idx)),
            second: Box::new(PaneNode::Terminal(new_idx)),
        });
        tab.focused_session = new_idx;
    }

    /// Save hosts to disk
    pub fn save_hosts(&self) {
        crate::config::save_hosts(&self.hosts_file, &self.hosts);
    }

    /// Save credentials to disk
    pub fn save_credentials(&self) {
        crate::config::save_credentials(&self.credentials_file, &self.credentials);
    }
}
