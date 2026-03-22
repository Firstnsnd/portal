//! Tab management functionality for PortalApp

use crate::config::{HostEntry, resolve_auth, ResolvedAuth};
use crate::ssh::JumpHostInfo;
use crate::ui::pane::{SplitDirection, PaneNode, Tab, DetachedWindow, TabDragState};
use crate::ui::types::{TerminalSession, AppView, BroadcastState};
use super::PortalApp;

impl PortalApp {
    /// Add a new local terminal tab
    pub fn add_tab_local(&mut self) {
        let id = self.next_id;
        self.next_id += 1;
        let tab = Tab {
            title: format!("Terminal {}", id),
            sessions: vec![TerminalSession::new_local(id, &self.selected_shell)],
            layout: PaneNode::Terminal(0),
            focused_session: 0,
            broadcast_enabled: false,
        };
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        self.current_view = AppView::Terminal;
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

    /// Add a new SSH terminal tab connected to the specified host
    pub fn add_tab_ssh(&mut self, host: &HostEntry) {
        let auth = resolve_auth(host, &self.credentials);
        let jump = self.resolve_jump_host(host);
        let session = TerminalSession::new_ssh(host, auth, &self.runtime, jump);
        let tab = Tab {
            title: host.name.clone(),
            sessions: vec![session],
            layout: PaneNode::Terminal(0),
            focused_session: 0,
            broadcast_enabled: false,
        };
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        self.current_view = AppView::Terminal;
        self.connection_history = crate::config::load_history();
    }

    /// Split the currently focused pane in the specified direction
    pub fn split_focused_pane(&mut self, direction: SplitDirection) {
        let new_id = self.next_id;
        self.next_id += 1;
        let tab = &self.tabs[self.active_tab];
        let old_idx = tab.focused_session;
        // Clone connection info from the focused session
        let ssh_host = tab.sessions.get(old_idx).and_then(|s| s.ssh_host.clone());
        let resolved_auth = tab.sessions.get(old_idx).and_then(|s| s.resolved_auth.clone());
        let new_session = if let Some(host) = &ssh_host {
            let auth = resolved_auth.unwrap_or(resolve_auth(host, &self.credentials));
            let jump = self.resolve_jump_host(host);
            TerminalSession::new_ssh(host, auth, &self.runtime, jump)
        } else {
            let shell = self.selected_shell.clone();
            TerminalSession::new_local(new_id, &shell)
        };
        let tab = &mut self.tabs[self.active_tab];
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

    /// Close a pane/session
    pub fn close_pane(&mut self, session_idx: usize) {
        let active = self.active_tab;
        let tab = &mut self.tabs[active];

        if tab.sessions.len() <= 1 {
            // Only one pane → close the entire tab
            let _ = tab;
            if self.tabs.len() > 1 {
                self.tabs.remove(active);
                self.active_tab = active.saturating_sub(1);
            }
            return;
        }

        // Remove from layout tree; collapse the parent Split
        let old_layout = tab.layout.clone();
        if let Some(new_layout) = old_layout.remove(session_idx) {
            tab.layout = new_layout;
        }
        // Decrement indices of sessions that came after the removed one
        tab.layout.decrement_indices_above(session_idx);
        // Remove the session itself
        tab.sessions.remove(session_idx);
        // Fix focused_session
        if tab.focused_session >= tab.sessions.len() {
            tab.focused_session = tab.sessions.len().saturating_sub(1);
        } else if tab.focused_session == session_idx && session_idx > 0 {
            tab.focused_session = session_idx - 1;
        }
    }

    /// Detach a tab into a new window
    pub fn detach_tab(&mut self, tab_index: usize) {
        if self.tabs.len() <= 1 {
            return; // don't detach the only tab
        }
        let tab = self.tabs.remove(tab_index);
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len().saturating_sub(1);
        }

        let id_val = self.next_viewport_id;
        self.next_viewport_id += 1;
        let viewport_id = egui::ViewportId::from_hash_of(format!("detached_{}", id_val));

        let next_id = self.next_id;
        self.next_id += 100; // avoid ID conflicts with main window

        self.detached_windows.push(DetachedWindow {
            viewport_id,
            title: tab.title.clone(),
            tabs: vec![tab],
            active_tab: 0,
            current_view: AppView::Terminal,
            close_requested: false,
            ime_composing: false,
            ime_preedit: String::new(),
            next_id,
            tab_drag: TabDragState::default(),
            broadcast_state: BroadcastState::default(),
        });
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
