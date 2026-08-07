//! # Terminal Session Types
//!
//! This module contains types related to terminal sessions and backends.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::config::{HostEntry, PortForwardConfig, ResolvedAuth};
use crate::ssh::port_forward::ForwardState;
use crate::ssh::{SshSession, SshConnectionState, JumpHostInfo, AppNotification};
use crate::terminal::{TerminalGrid, RealPtySession};

/// Session kind, distinguished at the type level.
///
/// `Ssh` carries a connection state machine (Connecting / Authenticating /
/// Connected / Disconnected / Error) and can be reconnected. `Local` is a
/// spawned PTY child process — it has NO "connection" concept at all: the
/// process is either running or has exited, and that is a one-way, one-time
/// fact reported by [`RealPtySession::has_exited`]. There is deliberately no
/// `is_connected` / `needs_reconnect` path for `Local`, so a local terminal can
/// never enter a "disconnected" rendering branch.
#[allow(clippy::large_enum_variant)] // SshSession is heavy; stored in Vecs, not copied
pub enum SessionKind {
    /// Local PTY. The `String` is the shell path used at spawn time, kept as a
    /// display fallback when live process-name detection fails.
    Local(RealPtySession, String),
    /// SSH session plus the host config + resolved auth needed to reconnect.
    Ssh(SshSession, HostEntry, ResolvedAuth),
}

impl SessionKind {
    pub fn write(&mut self, data: &[u8]) -> std::io::Result<()> {
        match self {
            SessionKind::Local(s, _) => s.write(data),
            SessionKind::Ssh(s, _, _) => s.write(data),
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> std::io::Result<()> {
        match self {
            SessionKind::Local(s, _) => s.resize(cols, rows),
            SessionKind::Ssh(s, _, _) => s.resize(cols, rows),
        }
    }

    /// Drain pending notifications. Local sessions never produce notifications.
    pub fn drain_notifications(&self) -> Vec<AppNotification> {
        match self {
            SessionKind::Local(_, _) => Vec::new(),
            SessionKind::Ssh(ssh, _, _) => ssh.drain_notifications(),
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
    pub kind: SessionKind,
    pub grid: Arc<Mutex<TerminalGrid>>,
    pub last_cols: usize,
    pub last_rows: usize,
    /// Scroll offset: 0 = bottom (latest), >0 = scrolled up by N lines
    pub scroll_offset: usize,
    /// Per-session text selection
    pub selection: Selection,
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
    /// Settle window for a pending resize, in milliseconds. While the size keeps
    /// changing this deadline keeps resetting, so a window drag coalesces into a
    /// single resize event; once the size settles, the grid+PTY jump together
    /// ~150ms later and the app redraws cleanly once. Short enough that a
    /// narrowed frame recovers promptly, long enough to avoid a SIGWINCH storm
    /// while dragging.
    const PTY_RESIZE_SETTLE_MS: u64 = 150;

    pub fn new_local(id: usize, shell: &str) -> Self {
        // Load settings to get scrollback limit
        let settings = crate::config::load_settings();
        let scrollback_bytes = (settings.scrollback_limit_mb as usize) * 1024 * 1024;

        let pty = RealPtySession::with_scrollback_limit(id, 80, 24, shell, scrollback_bytes)
            .expect("failed to spawn local PTY");
        let grid = pty.get_grid();

        // Get initial cwd
        let cwd = std::env::current_dir().ok().map(|p| p.to_string_lossy().to_string());

        Self {
            kind: SessionKind::Local(pty, shell.to_string()),
            grid,
            last_cols: 80,
            last_rows: 24,
            scroll_offset: 0,
            selection: Selection::default(),
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
            kind: SessionKind::Ssh(ssh, host.clone(), auth),
            grid,
            last_cols: 80,
            last_rows: 24,
            scroll_offset: 0,
            selection: Selection::default(),
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
        match &self.kind {
            SessionKind::Local(pty, shell_path) => {
                // Try process-tree detection first (handles nested shells,
                // e.g. user ran `bash` from inside zsh).
                if let Some(name) = pty.get_shell_name() {
                    return name;
                }
                if !shell_path.is_empty() {
                    shell_path.rsplit('/').next().unwrap_or("shell").to_string()
                } else {
                    "shell".to_string()
                }
            }
            SessionKind::Ssh(ssh, _, _) => {
                ssh.get_shell_hint()
                    .as_deref()
                    .and_then(|p| p.rsplit('/').next().map(|s| s.to_string()))
                    .unwrap_or_else(|| "…".to_string())
            }
        }
    }

    /// Reconnect a disconnected SSH session. No-op for Local (Local has no
    /// connection concept and can never be reconnected — a dead local shell
    /// is simply closed by the user).
    pub fn reconnect_ssh(&mut self, runtime: &tokio::runtime::Runtime, jump_host: Option<JumpHostInfo>) {
        if let SessionKind::Ssh(_, host, auth) = &self.kind {
            let host = host.clone();
            let auth = auth.clone();
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
            self.kind = SessionKind::Ssh(ssh, host, auth);
            self.scroll_offset = 0;
        }
    }

    /// Check if this is a disconnected SSH session that can reconnect.
    /// Always false for Local.
    pub fn needs_reconnect(&self) -> bool {
        match &self.kind {
            SessionKind::Ssh(ssh, _, _) => matches!(
                ssh.connection_state(),
                SshConnectionState::Disconnected(_) | SshConnectionState::Error(_)
            ),
            SessionKind::Local(_, _) => false,
        }
    }

    pub fn write(&mut self, data: &str) {
        let _ = self.kind.write(data.as_bytes());
    }

    pub fn write_bytes(&mut self, data: &[u8]) {
        let _ = self.kind.write(data);
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
                    let _ = self.kind.resize(pc, pr);
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
        self.pty_resize_deadline = Instant::now() + Duration::from_millis(Self::PTY_RESIZE_SETTLE_MS);
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        // Explicitly drop the backend to ensure PTY is cleaned up.
        // This prevents PTY resource leaks when the app exits.
        // (`kind` owns the RealPtySession / SshSession, so a plain drop suffices;
        //  this impl exists to make the intent explicit and catch future field
        //  additions that might forget cleanup.)
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
        if let SessionKind::Ssh(ssh, ssh_host, _) = &s.kind {
            let bound = ssh_host.host == host.host && ssh_host.port == host.port;
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
        // These tests exercise the resize debounce / reflow logic on the grid,
        // not any backend behaviour. Spawn a real local PTY (the only honest way
        // to construct a `SessionKind::Local`) and then swap in a grid shaped
        // to the test's requested cols/rows.
        let mut s = TerminalSession::new_local(0, "/bin/sh");
        let grid = Arc::new(Mutex::new(
            crate::terminal::TerminalGrid::with_scrollback_limit(cols, rows, 1024 * 1024),
        ));
        s.grid = grid;
        s.last_cols = cols;
        s.last_rows = rows;
        s
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

    /// A window drag issues a rapid sequence of different sizes before the
    /// settle window passes. The pending resize must coalesce: only the FINAL
    /// size is applied once the drag stops (no intermediate SIGWINCH storm).
    #[test]
    fn rapid_resizes_coalesce_to_final_size() {
        let mut s = grid_only_session(148, 49);
        // drag: several sizes before the deadline fires
        s.resize(140, 49);
        s.resize(120, 49);
        s.resize(100, 44);
        // all scheduled to the SAME pending slot — last write wins
        assert_eq!(s.pending_pty_size, Some((100, 44)),
            "pending must hold the latest drag size, not an intermediate");
        assert_eq!(s.grid.lock().unwrap().cols, 148,
            "grid stays at the ORIGINAL size during the drag");
        // deadline fires → exactly ONE resize to the final size
        s.pty_resize_deadline = Instant::now() - Duration::from_millis(1);
        s.resize(100, 44);
        assert!(s.pending_pty_size.is_none(), "coalesced resize must fire once");
        assert_eq!(s.grid.lock().unwrap().cols, 100);
        assert_eq!(s.grid.lock().unwrap().rows, 44);
    }

    /// A resize deferred during a ?2026 sync batch must STILL be applied once
    /// the batch ends (a later frame), not lost forever.
    #[test]
    fn resize_deferred_in_sync_is_applied_after_batch() {
        let mut s = grid_only_session(148, 49);
        // enter sync batch
        {
            use crate::terminal::{CellAttrs, VteHandler};
            use vte::Parser;
            let mut g = s.grid.lock().unwrap();
            let mut p = Parser::new();
            let mut a = CellAttrs::default();
            for &b in b"\x1b[?2026h".iter() {
                let mut h = VteHandler { grid: &mut g, attrs: &mut a };
                p.advance(&mut h, b);
            }
        }
        // deadline fires but resize is deferred (in sync)
        s.resize(100, 40);
        s.pty_resize_deadline = Instant::now() - Duration::from_millis(1);
        s.resize(100, 40);
        assert!(s.pending_pty_size.is_some(), "pending kept while in sync");
        assert_eq!(s.grid.lock().unwrap().cols, 148, "grid not resized mid-batch");
        // batch ends
        {
            use crate::terminal::{CellAttrs, VteHandler};
            use vte::Parser;
            let mut g = s.grid.lock().unwrap();
            let mut p = Parser::new();
            let mut a = CellAttrs::default();
            for &b in b"\x1b[?2026l".iter() {
                let mut h = VteHandler { grid: &mut g, attrs: &mut a };
                p.advance(&mut h, b);
            }
        }
        // a later frame applies the deferred resize
        s.pty_resize_deadline = Instant::now() - Duration::from_millis(1);
        s.resize(100, 40);
        assert!(s.pending_pty_size.is_none(), "deferred resize applied after batch");
        assert_eq!(s.grid.lock().unwrap().cols, 100);
    }
}



#[cfg(test)]
mod sync_defer {
    use super::*;
    use crate::terminal::{CellAttrs, TerminalGrid, VteHandler};
    use vte::Parser;

    fn grid_only_session(cols: usize, rows: usize) -> TerminalSession {
        let mut s = TerminalSession::new_local(0, "/bin/sh");
        s.grid = Arc::new(Mutex::new(TerminalGrid::with_scrollback_limit(cols, rows, 1024 * 1024)));
        s.last_cols = cols;
        s.last_rows = rows;
        s
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
        let mut s = TerminalSession::new_local(0, "/bin/sh");
        s.grid = Arc::new(Mutex::new(TerminalGrid::with_scrollback_limit(cols, rows, 1024 * 1024)));
        s.last_cols = cols;
        s.last_rows = rows;
        s
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
                let mut h = VteHandler { grid: &mut g, attrs: &mut a };
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
                let mut h = VteHandler { grid: &mut g, attrs: &mut a };
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
                let mut h = VteHandler { grid: &mut g, attrs: &mut a };
                p.advance(&mut h, b);
            }
        }
        let n = status_count(&s.grid.lock().unwrap());
        println!("end-to-end race: status_lines={n}");
        assert!(n <= 1, "status line duplicated after grow+redraw: {n}");
    }
}
