//! # Terminal Session Types
//!
//! This module contains types related to terminal sessions and backends.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::config::{HostEntry, PortForwardConfig, ResolvedAuth};
use crate::ssh::port_forward::ForwardState;
use crate::ssh::{SshSession, SshConnectionState, JumpHostInfo, AppNotification};
use crate::terminal::{TerminalGrid, RealPtySession};

/// Unified session backend: local PTY or SSH
pub enum SessionBackend {
    Local(RealPtySession),
    Ssh(SshSession),
}

impl SessionBackend {
    pub fn write(&mut self, data: &[u8]) -> std::io::Result<()> {
        match self {
            SessionBackend::Local(s) => s.write(data),
            SessionBackend::Ssh(s) => s.write(data),
        }
    }

    pub fn get_grid(&self) -> Arc<Mutex<TerminalGrid>> {
        match self {
            SessionBackend::Local(s) => s.get_grid(),
            SessionBackend::Ssh(s) => s.get_grid(),
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> std::io::Result<()> {
        match self {
            SessionBackend::Local(s) => s.resize(cols, rows),
            SessionBackend::Ssh(s) => s.resize(cols, rows),
        }
    }

    pub fn is_connected(&self) -> bool {
        match self {
            SessionBackend::Local(s) => s.is_connected(),
            SessionBackend::Ssh(s) => matches!(s.connection_state(), SshConnectionState::Connected),
        }
    }

    pub fn get_shell_name(&self) -> Option<String> {
        match self {
            SessionBackend::Local(s) => s.get_shell_name(),
            SessionBackend::Ssh(_) => None,
        }
    }

    /// Drain pending notifications from the session backend
    pub fn drain_notifications(&self) -> Vec<AppNotification> {
        match self {
            SessionBackend::Local(_) => Vec::new(),
            SessionBackend::Ssh(ssh) => ssh.drain_notifications(),
        }
    }
}

/// Text selection state in terminal
#[derive(Default, Clone)]
pub struct Selection {
    /// Whether a drag is in progress
    pub active: bool,
    /// Start position (row, col) — where mouse was pressed
    pub start: (usize, usize),
    /// End position (row, col) — where mouse currently is
    pub end: (usize, usize),
}

impl Selection {
    pub fn has_selection(&self) -> bool {
        self.start != self.end
    }

    /// Returns (start, end) in normalized order (start <= end)
    pub fn ordered(&self) -> ((usize, usize), (usize, usize)) {
        if self.start.0 < self.end.0
            || (self.start.0 == self.end.0 && self.start.1 <= self.end.1)
        {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        }
    }

    pub fn clear(&mut self) {
        self.active = false;
        self.start = (0, 0);
        self.end = (0, 0);
    }
}

/// Search match position in terminal content
#[derive(Clone, Debug)]
pub struct SearchMatch {
    /// Global row index (scrollback + grid unified)
    pub row: usize,
    /// Start column (inclusive)
    pub col_start: usize,
    /// End column (exclusive)
    pub col_end: usize,
}

/// Search state for terminal content
#[derive(Clone)]
pub struct SearchState {
    /// Current search query
    pub query: String,
    /// All matches found
    pub matches: Vec<SearchMatch>,
    /// Index of the currently highlighted match
    pub current_index: usize,
    /// Whether search is case-sensitive
    pub case_sensitive: bool,
}

/// Terminal session
pub struct TerminalSession {
    pub session: Option<SessionBackend>,
    pub grid: Arc<Mutex<TerminalGrid>>,
    pub last_cols: usize,
    pub last_rows: usize,
    /// Scroll offset: 0 = bottom (latest), >0 = scrolled up by N lines
    pub scroll_offset: usize,
    /// Saved host config for SSH reconnection
    pub ssh_host: Option<HostEntry>,
    /// Saved resolved auth for SSH reconnection (avoids needing credentials vec)
    pub resolved_auth: Option<ResolvedAuth>,
    /// Per-session text selection
    pub selection: Selection,
    /// Shell path used for local sessions (e.g. "/bin/zsh")
    pub local_shell: String,
    /// When this session was created
    pub created_at: Instant,
    /// Pending PTY resize (cols, rows) — debounced for column changes
    pub pending_pty_size: Option<(u16, u16)>,
    /// Deadline for sending debounced PTY resize
    pub pty_resize_deadline: Instant,
    /// Tracks if we just sent non-ASCII text (for IME punctuation handling)
    pub last_non_ascii_input: bool,
    /// Current working directory (updated via OSC 7 or initial cwd)
    pub cwd: Option<String>,
    /// Search state (active when Some)
    pub search_state: Option<SearchState>,
}

impl TerminalSession {
    pub fn new_local(id: usize, shell: &str) -> Self {
        // Load settings to get scrollback limit
        let settings = crate::config::load_settings();
        let scrollback_bytes = (settings.scrollback_limit_mb as usize) * 1024 * 1024;

        let session = RealPtySession::with_scrollback_limit(id, 80, 24, shell, scrollback_bytes)
            .ok().map(SessionBackend::Local);
        let grid = session.as_ref().map(|s| s.get_grid()).unwrap_or_else(|| {
            Arc::new(Mutex::new(TerminalGrid::with_scrollback_limit(80, 24, scrollback_bytes)))
        });

        // Get initial cwd
        let cwd = std::env::current_dir().ok().map(|p| p.to_string_lossy().to_string());

        Self {
            session,
            grid,
            last_cols: 80,
            last_rows: 24,
            scroll_offset: 0,
            ssh_host: None,
            resolved_auth: None,
            selection: Selection::default(),
            local_shell: shell.to_string(),
            created_at: Instant::now(),
            pending_pty_size: None,
            pty_resize_deadline: Instant::now(),
            last_non_ascii_input: false,
            cwd,
            search_state: None,
        }
    }

    /// Helper to get effective username (current user if empty)
    pub fn get_effective_username(username: &str) -> String {
        if username.is_empty() {
            std::env::var("USER").unwrap_or_else(|_| {
                // Fallback to whoami command if USER env var not set
                std::process::Command::new("whoami")
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .unwrap_or_else(|_| "root".to_string())
            })
        } else {
            username.to_string()
        }
    }

    pub fn new_ssh(host: &HostEntry, auth: ResolvedAuth, runtime: &tokio::runtime::Runtime, jump_host: Option<JumpHostInfo>) -> Self {
        // Use current system user if username is empty
        let username = Self::get_effective_username(&host.username);

        crate::config::append_history(crate::config::ConnectionRecord {
            host_name: host.name.clone(),
            host: host.host.clone(),
            port: host.port,
            username: username.clone(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            success: true,
        });

        // Load settings to get scrollback limit
        let settings = crate::config::load_settings();
        let scrollback_bytes = (settings.scrollback_limit_mb as usize) * 1024 * 1024;

        let ssh = SshSession::with_scrollback_limit(
            runtime,
            host.host.clone(),
            host.port,
            username,
            auth.clone(),
            80,
            24,
            host.startup_commands.clone(),
            scrollback_bytes,
            settings.ssh_keepalive_interval,
            host.agent_forwarding,
            host.port_forwards.clone(),
            jump_host,
        );
        let grid = ssh.get_grid();
        Self {
            session: Some(SessionBackend::Ssh(ssh)),
            grid,
            last_cols: 80,
            last_rows: 24,
            scroll_offset: 0,
            ssh_host: Some(host.clone()),
            resolved_auth: Some(auth),
            selection: Selection::default(),
            local_shell: String::new(),
            created_at: Instant::now(),
            pending_pty_size: None,
            pty_resize_deadline: Instant::now(),
            last_non_ascii_input: false,
            cwd: None, // SSH sessions start without known cwd
            search_state: None,
        }
    }

    /// Shell display name for this session
    pub fn shell_name(&self) -> String {
        match &self.session {
            Some(SessionBackend::Local(_)) => {
                // Try process-tree detection first (handles nested shells,
                // e.g. user ran `bash` from inside zsh).
                // Falls back to the shell path stored at session creation.
                if let Some(name) = self.session.as_ref()
                    .and_then(|s| s.get_shell_name())
                {
                    return name;
                }
                if !self.local_shell.is_empty() {
                    self.local_shell.rsplit('/').next().unwrap_or("shell").to_string()
                } else {
                    "shell".to_string()
                }
            }
            Some(SessionBackend::Ssh(ssh)) => {
                ssh.get_shell_hint()
                    .as_deref()
                    .and_then(|p| p.rsplit('/').next().map(|s| s.to_string()))
                    .unwrap_or_else(|| "…".to_string())
            }
            None => "—".to_string(),
        }
    }

    /// Reconnect a disconnected SSH session
    pub fn reconnect_ssh(&mut self, runtime: &tokio::runtime::Runtime, jump_host: Option<JumpHostInfo>) {
        if let (Some(ref host), Some(ref auth)) = (&self.ssh_host, &self.resolved_auth) {
            let settings = crate::config::load_settings();
            let ssh = SshSession::connect(
                runtime,
                host.host.clone(),
                host.port,
                host.username.clone(),
                auth.clone(),
                self.last_cols as u16,
                self.last_rows as u16,
                host.startup_commands.clone(),
                settings.ssh_keepalive_interval,
                host.agent_forwarding,
                jump_host,
            );
            self.grid = ssh.get_grid();
            self.session = Some(SessionBackend::Ssh(ssh));
            self.scroll_offset = 0;
        }
    }

    /// Check if this is a disconnected SSH session that can reconnect
    pub fn needs_reconnect(&self) -> bool {
        if self.ssh_host.is_none() {
            return false;
        }
        match &self.session {
            Some(SessionBackend::Ssh(ssh)) => matches!(
                ssh.connection_state(),
                SshConnectionState::Disconnected(_) | SshConnectionState::Error(_)
            ),
            None => true,
            _ => false,
        }
    }

    pub fn write(&mut self, data: &str) {
        if let Some(ref mut session) = self.session {
            let _ = session.write(data.as_bytes());
        }
    }

    pub fn write_bytes(&mut self, data: &[u8]) {
        if let Some(ref mut session) = self.session {
            let _ = session.write(data);
        }
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        // Apply any settled resize to the grid AND the PTY together — but only
        // when the app is NOT in the middle of a synchronized redraw batch
        // (\e[?2026h…?2026l). A full-screen app clears its OLD frame using its
        // assumed (old) height, then redraws at the new height. If we resize
        // mid-batch, the clear only covers rows up to the old height and the
        // rows below it keep stale content (e.g. a duplicated status line when
        // the window grows). Deferring until the batch ends lets the batch
        // complete coherently at the old size, then the app's next (new-size)
        // redraw clears everything. resize() is called every frame, so a
        // deferred pending resize simply applies on a later frame.
        if let Some((pc, pr)) = self.pending_pty_size {
            if Instant::now() >= self.pty_resize_deadline {
                let in_sync = self.grid.lock().map(|g| g.in_sync_update).unwrap_or(false);
                if !in_sync {
                    if let Ok(mut grid) = self.grid.lock() {
                        grid.resize(pc as usize, pr as usize);
                    }
                    if let Some(ref mut session) = self.session {
                        let _ = session.resize(pc, pr);
                    }
                    self.pending_pty_size = None;
                }
                // else: keep pending; a later frame (after ?2026l) applies it
            }
        }

        if cols == self.last_cols && rows == self.last_rows {
            // Even if dimensions haven't changed, clamp cursor position
            // This handles the case where cursor was set to an invalid position
            // before the status bar was properly accounted for
            if let Ok(mut grid) = self.grid.lock() {
                if grid.cursor_row >= grid.rows {
                    grid.cursor_row = grid.rows.saturating_sub(1);
                }
            }
            return;
        }

        self.last_cols = cols;
        self.last_rows = rows;

        // Schedule a debounced resize applied to grid + PTY ATOMICALLY.
        //
        // The grid must NEVER resize ahead of the PTY. The app redraws on its
        // own timer at the PTY width it reads; if the grid jumps to a new width
        // while the PTY still reports the old one, the app's absolute-column
        // layout (Claude Code's CHA) lands on the wrong cells → frames overlap
        // and truncate. Keeping both at the old width during the settle window
        // means the app keeps drawing correctly; when the debounce fires they
        // jump together and the app redraws cleanly once.
        //
        // (Short window: while the size keeps changing the deadline keeps
        // resetting, so a drag coalesces; yet the app redraws ~150ms after you
        // stop — a narrowed frame recovers promptly.)
        self.pending_pty_size = Some((cols as u16, rows as u16));
        self.pty_resize_deadline = Instant::now() + Duration::from_millis(150);
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        // Explicitly drop the session to ensure PTY is cleaned up
        // This prevents PTY resource leaks when the app exits
        if let Some(session) = self.session.take() {
            drop(session);
        }
    }
}

/// Look up the live runtime state of a configured port forward by scanning
/// SSH sessions for one bound to `host` whose `PortForwardConfig` matches.
/// Returns `None` when no matching active session exists. Used by the host
/// list to show how many forwards are running for a host.
pub fn forward_state<'a>(
    sessions: impl Iterator<Item = &'a TerminalSession>,
    host: &HostEntry,
    cfg: &PortForwardConfig,
) -> Option<ForwardState> {
    for s in sessions {
        if let Some(SessionBackend::Ssh(ssh)) = &s.session {
            let bound = s.ssh_host.as_ref()
                .map(|h| h.host == host.host && h.port == host.port)
                .unwrap_or(false);
            if !bound {
                continue;
            }
            if let Ok(pfs) = ssh.port_forwards.lock() {
                for pf in pfs.iter() {
                    if &pf.config == cfg {
                        return pf.state.lock().ok().map(|g| g.clone());
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid_only_session(cols: usize, rows: usize) -> TerminalSession {
        let grid = Arc::new(Mutex::new(
            crate::terminal::TerminalGrid::with_scrollback_limit(cols, rows, 1024 * 1024),
        ));
        TerminalSession {
            session: None, // no PTY — we inspect the debounce scheduling logic
            grid,
            last_cols: cols,
            last_rows: rows,
            scroll_offset: 0,
            ssh_host: None,
            resolved_auth: None,
            selection: Selection::default(),
            local_shell: String::new(),
            created_at: Instant::now(),
            pending_pty_size: None,
            pty_resize_deadline: Instant::now(),
            last_non_ascii_input: false,
            cwd: None,
            search_state: None,
        }
    }

    /// ROOT-CAUSE regression: the grid and the PTY must NEVER diverge. If the
    /// grid resizes immediately while the PTY is debounced, the app (which
    /// redraws on its own timer at the PTY width it reads) draws at the stale
    /// width into the new grid → its absolute-column layout wraps/scrambles/
    /// overlaps. So on a width change the grid must stay put (pending), exactly
    /// like the PTY — they jump together when the debounce fires.
    #[test]
    fn width_change_keeps_grid_and_pty_in_lockstep() {
        let mut s = grid_only_session(148, 49);
        s.resize(100, 49); // width change
        assert_eq!(s.pending_pty_size, Some((100, 49)),
            "must schedule a debounced resize");
        // FIX: the grid must NOT have jumped to 100 — the app would be drawing
        // at the PTY's still-old width (148) into a 100-wide grid → overlap.
        assert_eq!(s.grid.lock().unwrap().cols, 148,
            "grid must not resize ahead of the PTY (this is the overlap root cause)");
    }

    #[test]
    fn width_and_height_change_keep_grid_in_lockstep() {
        let mut s = grid_only_session(148, 49);
        s.resize(100, 40);
        assert_eq!(s.pending_pty_size, Some((100, 40)));
        assert_eq!(s.grid.lock().unwrap().cols, 148, "grid must not jump early");
        assert_eq!(s.grid.lock().unwrap().rows, 49, "grid rows must not jump early");
    }

    #[test]
    fn settled_resize_applies_grid_and_pty_together() {
        let mut s = grid_only_session(148, 49);
        s.resize(100, 40);
        // deadline passes → the pending resize is applied to BOTH together
        s.pty_resize_deadline = Instant::now() - Duration::from_millis(1);
        s.resize(100, 40);
        assert!(s.pending_pty_size.is_none(), "pending resize must fire once settled");
        assert_eq!(s.grid.lock().unwrap().cols, 100, "grid must follow the PTY");
        assert_eq!(s.grid.lock().unwrap().rows, 40, "grid rows must follow the PTY");
    }
}



#[cfg(test)]
mod sync_defer {
    use super::*;
    use crate::terminal::{CellAttrs, TerminalGrid, VteHandler};
    use vte::Parser;

    fn grid_only_session(cols: usize, rows: usize) -> TerminalSession {
        let grid = Arc::new(Mutex::new(TerminalGrid::with_scrollback_limit(cols, rows, 1024 * 1024)));
        TerminalSession {
            session: None, grid, last_cols: cols, last_rows: rows, scroll_offset: 0,
            ssh_host: None, resolved_auth: None, selection: Selection::default(),
            local_shell: String::new(), created_at: Instant::now(),
            pending_pty_size: None, pty_resize_deadline: Instant::now(),
            last_non_ascii_input: false, cwd: None, search_state: None,
        }
    }

    fn feed(grid: &mut crate::terminal::TerminalGrid, p: &mut Parser, a: &mut CellAttrs, s: &str) {
        for &b in s.as_bytes() {
            let mut h = VteHandler { grid, attrs: a };
            p.advance(&mut h, b);
        }
    }

    /// Regression for the duplicated-status-line bug: a resize fired mid-\e[?2026
    /// batch must be DEFERRED (not split claude's clear-then-redraw), so the old
    /// status line is cleared by the next new-size redraw instead of surviving.
    #[test]
    fn resize_deferred_during_sync_batch() {
        let mut s = grid_only_session(100, 30);
        {
            let mut g = s.grid.lock().unwrap();
            let mut p = Parser::new();
            let mut a = CellAttrs::default();
            g.program_uses_positioning = true;
            // status at the bottom, then start a redraw batch (clear old frame)
            feed(&mut g, &mut p, &mut a, "\x1b[30;1HSTATUS_OLD\x1b[?2026h\x1b[H\x1b[2J");
            assert!(g.in_sync_update, "?2026h must set sync flag");
        }
        // render loop wants to grow; debounce fires
        s.resize(100, 40);
        s.pty_resize_deadline = Instant::now() - Duration::from_millis(1);
        s.resize(100, 40);
        // while still in sync, the resize must NOT be applied
        {
            let g = s.grid.lock().unwrap();
            assert_eq!(g.rows, 30, "resize must be deferred during a sync batch");
            assert!(s.pending_pty_size.is_some(), "pending resize kept while in sync");
        }
        // batch ends (?2026l), then a later frame applies the resize
        {
            let mut g = s.grid.lock().unwrap();
            let mut p = Parser::new();
            let mut a = CellAttrs::default();
            feed(&mut g, &mut p, &mut a, "\x1b[?2026l");
        }
        s.pty_resize_deadline = Instant::now() - Duration::from_millis(1);
        s.resize(100, 40);
        {
            let g = s.grid.lock().unwrap();
            assert_eq!(g.rows, 40, "deferred resize applied once sync batch ended");
            assert!(s.pending_pty_size.is_none(), "pending cleared after apply");
        }
    }
}

#[cfg(test)]
mod sync_defer_e2e {
    use super::*;
    use crate::terminal::{CellAttrs, TerminalGrid, VteHandler};
    use vte::Parser;

    fn grid_only_session(cols: usize, rows: usize) -> TerminalSession {
        let grid = Arc::new(Mutex::new(TerminalGrid::with_scrollback_limit(cols, rows, 1024 * 1024)));
        TerminalSession {
            session: None, grid, last_cols: cols, last_rows: rows, scroll_offset: 0,
            ssh_host: None, resolved_auth: None, selection: Selection::default(),
            local_shell: String::new(), created_at: Instant::now(),
            pending_pty_size: None, pty_resize_deadline: Instant::now(),
            last_non_ascii_input: false, cwd: None, search_state: None,
        }
    }
    fn status_count(grid: &TerminalGrid) -> usize {
        (0..grid.rows).filter(|&r| {
            let row: String = grid.cells[r].iter().map(|c| c.c).collect();
            row.contains("STATUS")
        }).count()
    }

    /// End-to-end: claude clears its OLD frame (a sync batch), the render loop
    /// wants to grow the window mid-batch, the resize is DEFERRED until the
    /// batch ends, then the grid grows and claude redraws at the new height —
    /// exactly one status line survives (the duplicated one is gone).
    #[test]
    fn full_race_no_duplicate_status() {
        let mut s = grid_only_session(100, 30);
        {
            let mut g = s.grid.lock().unwrap();
            let mut p = Parser::new();
            let mut a = CellAttrs::default();
            g.program_uses_positioning = true;
            // old status at bottom, then claude's redraw batch: clear all + redraw
            // status at old bottom, still inside ?2026h
            for &b in b"\x1b[30;1HSTATUS\x1b[?2026h\x1b[H\x1b[2J\x1b[30;1HSTATUS".iter() {
                let mut h = VteHandler { grid: &mut *g, attrs: &mut a };
                p.advance(&mut h, b);
            }
            assert!(g.in_sync_update);
        }
        // render loop grows; debounce fires but is DEFERRED (mid-sync)
        s.resize(100, 40);
        s.pty_resize_deadline = Instant::now() - Duration::from_millis(1);
        s.resize(100, 40);
        {
            let g = s.grid.lock().unwrap();
            assert_eq!(g.rows, 30, "must not grow mid-sync-batch");
        }
        // batch ends, resize applies, claude redraws status at NEW bottom
        {
            let mut g = s.grid.lock().unwrap();
            let mut p = Parser::new();
            let mut a = CellAttrs::default();
            for &b in b"\x1b[?2026l".iter() {
                let mut h = VteHandler { grid: &mut *g, attrs: &mut a };
                p.advance(&mut h, b);
            }
        }
        s.pty_resize_deadline = Instant::now() - Duration::from_millis(1);
        s.resize(100, 40);
        {
            let mut g = s.grid.lock().unwrap();
            assert_eq!(g.rows, 40);
            let mut p = Parser::new();
            let mut a = CellAttrs::default();
            for &b in b"\x1b[H\x1b[2J\x1b[40;1HSTATUS".iter() {
                let mut h = VteHandler { grid: &mut *g, attrs: &mut a };
                p.advance(&mut h, b);
            }
        }
        let n = status_count(&s.grid.lock().unwrap());
        println!("end-to-end race: status_lines={n}");
        assert!(n <= 1, "status line duplicated after grow+redraw: {n}");
    }
}
