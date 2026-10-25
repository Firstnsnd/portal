//! Session snapshot + restore orchestration (session_state.json).
//!
//! Snapshot converts live `AppWindow`s into the pure DTOs in
//! `config::session_state`; restore rebuilds root-window tabs from them.
//! Detached windows are snapshotted too, but v1 only restores the root
//! window. SSH sessions reconnect through the normal connect path; a host
//! entry that no longer exists falls back to a local shell + banner line.

use crate::config::session_state::{
    self as ss, PaneNodeDto, SessionDto, SessionStateFile, SplitDirectionDto, TabState,
    WindowState,
};
use crate::config::{resolve_auth, Credential, HostEntry};
use crate::ui::pane::{AppWindow, PaneNode, SplitDirection, Tab};
use crate::ui::types::session::{SessionKind, TerminalSession};

/// Snapshot every window's tabs. Indices and sizes are taken as-is; a
/// well-formed live tab always satisfies them.
pub fn snapshot(windows: &[AppWindow]) -> SessionStateFile {
    SessionStateFile {
        version: ss::SESSION_STATE_VERSION,
        windows: windows.iter().map(snapshot_window).collect(),
    }
}

fn snapshot_window(window: &AppWindow) -> WindowState {
    WindowState {
        active_tab: window.active_tab,
        // A tab whose sessions are all gone (snapshotted between session
        // teardown and tab close) must never reach the file: restore would
        // underflow clamping its focus index and abort at every launch.
        tabs: window
            .tabs
            .iter()
            .filter(|t| !t.sessions.is_empty())
            .map(snapshot_tab)
            .collect(),
    }
}

fn snapshot_tab(tab: &Tab) -> TabState {
    TabState {
        title: tab.title.clone(),
        broadcast_enabled: tab.broadcast_enabled,
        focused_session: tab.focused_session,
        layout: snapshot_node(&tab.layout),
        sessions: tab.sessions.iter().map(snapshot_session).collect(),
    }
}

fn snapshot_node(node: &PaneNode) -> PaneNodeDto {
    match node {
        PaneNode::Terminal(idx) => PaneNodeDto::Terminal { session: *idx },
        PaneNode::Split {
            direction,
            ratio,
            first,
            second,
        } => PaneNodeDto::Split {
            direction: match direction {
                SplitDirection::Horizontal => SplitDirectionDto::Horizontal,
                SplitDirection::Vertical => SplitDirectionDto::Vertical,
            },
            ratio: *ratio,
            first: Box::new(snapshot_node(first)),
            second: Box::new(snapshot_node(second)),
        },
    }
}

fn snapshot_session(session: &TerminalSession) -> SessionDto {
    match &session.kind {
        SessionKind::Local(_, shell) => SessionDto::Local {
            shell: shell.clone(),
            cwd: session.cwd.clone(),
        },
        SessionKind::Ssh(_, host, _) => SessionDto::Ssh {
            host_name: host.name.clone(),
            cols: session.last_cols as u16,
            rows: session.last_rows as u16,
            transfer_interrupted: session.needs_reconnect(),
        },
    }
}

/// Try to rebuild root-window tabs from session_state.json. Returns
/// `(tabs, active_tab, next_id)` when a usable snapshot existed — the
/// caller seeds the window's PTY id counter with the returned `next_id`
/// (restore starts at 100, clear of ids already handed out at startup).
/// Disabled by the `restore_sessions` setting; a missing/corrupt snapshot
/// returns `None`.
pub fn restore_root_tabs(
    hosts: &[HostEntry],
    credentials: &[Credential],
    runtime: &tokio::runtime::Runtime,
) -> Option<(Vec<Tab>, usize, usize)> {
    let settings = crate::config::load_settings();
    if !settings.restore_sessions {
        return None;
    }
    let state = ss::load_session_state(&ss::session_state_file_path())?;
    build_tabs_from_state(&state, hosts, credentials, runtime)
}

/// Rebuild root-window tabs from an already-loaded snapshot. Exposed
/// separately so tests can run the full pipeline without touching the
/// user's real session_state.json.
pub fn build_tabs_from_state(
    state: &SessionStateFile,
    hosts: &[HostEntry],
    credentials: &[Credential],
    runtime: &tokio::runtime::Runtime,
) -> Option<(Vec<Tab>, usize, usize)> {
    let root = state.windows.first()?;
    if root.tabs.is_empty() {
        return None;
    }

    log::info!(
        "session-restore: rebuilding {} tab(s) from snapshot",
        root.tabs.len()
    );
    let mut next_id = 100;
    let tabs: Vec<Tab> = root
        .tabs
        .iter()
        .filter_map(|tab_state| build_tab(tab_state, hosts, credentials, runtime, &mut next_id))
        .collect();
    if tabs.is_empty() {
        // Every tab in the snapshot was unusable (e.g. all saved with zero
        // sessions) — nothing restorable, take the default fresh window.
        return None;
    }
    let active_tab = root.active_tab.min(tabs.len() - 1);
    Some((tabs, active_tab, next_id))
}

/// Platform default shell — shared by the fallback session paths.
fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string())
}

/// Build one tab. Every DTO yields exactly one session (missing SSH hosts
/// fall back to a local shell), so layout indices stay valid by
/// construction; a malformed layout downgrades the tab to a single pane.
/// Returns `None` for a tab saved with zero sessions — nothing to show.
fn build_tab(
    state: &TabState,
    hosts: &[HostEntry],
    credentials: &[Credential],
    runtime: &tokio::runtime::Runtime,
    next_id: &mut usize,
) -> Option<Tab> {
    let mut sessions: Vec<TerminalSession> = Vec::new();
    let mut banners: Vec<(usize, String)> = Vec::new();

    for dto in &state.sessions {
        match dto {
            SessionDto::Local { shell, cwd } => {
                sessions.push(TerminalSession::new_local_in(
                    *next_id,
                    shell,
                    cwd.as_deref(),
                ));
                *next_id += 1;
            }
            SessionDto::Ssh { host_name, .. } => {
                match hosts.iter().find(|h| h.name == *host_name) {
                    Some(host) => {
                        let host = host.clone();
                        let auth = resolve_auth(&host, credentials);
                        let jump = resolve_jump(hosts, credentials, &host);
                        sessions.push(TerminalSession::new_ssh(&host, auth, runtime, jump));
                    }
                    None => {
                        // Host entry deleted since the snapshot — degrade to
                        // a local shell with an explanatory banner line.
                        let shell = default_shell();
                        sessions.push(TerminalSession::new_local_in(*next_id, &shell, None));
                        let language = crate::ui::Language::from_id(
                            &crate::config::load_settings().language,
                        );
                        banners.push((
                            sessions.len() - 1,
                            format!(
                                "[portal] {}: '{}'",
                                language.t("restore_host_missing"),
                                host_name
                            ),
                        ));
                        *next_id += 1;
                    }
                }
            }
        }
    }
    if sessions.is_empty() {
        return None;
    }

    let layout = build_node(&state.layout, sessions.len()).unwrap_or(PaneNode::Terminal(0));
    let focused_session = state.focused_session.min(sessions.len() - 1);

    // Banner lines go in before the first paint so they sit above the
    // fresh shell's prompt.
    for (idx, line) in banners {
        print_banner_line(&sessions[idx], &line);
    }

    Some(Tab {
        title: state.title.clone(),
        sessions,
        layout,
        focused_session,
        broadcast_enabled: state.broadcast_enabled,
        tools_drawer_open: false,
        tools_tab: 0,
        pending_snippet: None,
        ai_chat: std::sync::Arc::new(std::sync::Mutex::new(
            crate::ui::state::ai_chat::AiChatState::default(),
        )),
        pending_ai_command: None,
    })
}

/// Free-function twin of `PortalApp::resolve_jump_host` for restore time
/// (before the `PortalApp` exists).
fn resolve_jump(
    hosts: &[HostEntry],
    credentials: &[Credential],
    host: &HostEntry,
) -> Option<crate::ssh::JumpHostInfo> {
    let jump_name = host.jump_host.as_ref()?;
    let jump_entry = hosts.iter().find(|h| &h.name == jump_name)?;
    let jump_auth = resolve_auth(jump_entry, credentials);
    if matches!(jump_auth, crate::config::ResolvedAuth::None) {
        return None;
    }
    Some(crate::ssh::JumpHostInfo {
        host: jump_entry.host.clone(),
        port: jump_entry.port,
        username: TerminalSession::get_effective_username(&jump_entry.username),
        auth: jump_auth,
        proxy: crate::config::resolve_proxy(jump_entry),
    })
}

/// Rebuild a pane tree. `None` when any leaf index is out of range
/// (caller downgrades the whole tab to a single pane).
fn build_node(dto: &PaneNodeDto, session_count: usize) -> Option<PaneNode> {
    match dto {
        PaneNodeDto::Terminal { session } => {
            (*session < session_count).then_some(PaneNode::Terminal(*session))
        }
        PaneNodeDto::Split {
            direction,
            ratio,
            first,
            second,
        } => {
            let first = build_node(first, session_count)?;
            let second = build_node(second, session_count)?;
            Some(PaneNode::Split {
                direction: match direction {
                    SplitDirectionDto::Horizontal => SplitDirection::Horizontal,
                    SplitDirectionDto::Vertical => SplitDirection::Vertical,
                },
                ratio: ratio.clamp(0.05, 0.95),
                first: Box::new(first),
                second: Box::new(second),
            })
        }
    }
}

/// Write one line into a session's grid via the VTE parser, so it renders
/// exactly like terminal output.
fn print_banner_line(session: &TerminalSession, text: &str) {
    let mut parser = vte::Parser::new();
    let mut attrs = crate::terminal::CellAttrs::default();
    if let Ok(mut grid) = session.grid.lock() {
        let mut handler = crate::terminal::VteHandler {
            grid: &mut grid,
            attrs: &mut attrs,
        };
        for byte in format!("{text}\r\n").as_bytes() {
            parser.advance(&mut handler, *byte);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Snapshot of hand-built local-only windows round-trips through the
    /// DTO layer (no SSH — new_ssh would open real connections).
    #[test]
    fn snapshot_local_tabs_produces_valid_state() {
        // AppWindow can't be built without egui state in a unit test; the
        // snapshot functions are covered end-to-end via cleanup_sessions at
        // runtime. Here we verify the DTO mapping helpers directly.
        let tab_state = TabState {
            title: "Terminal 1".to_string(),
            broadcast_enabled: false,
            focused_session: 0,
            layout: PaneNodeDto::Terminal { session: 0 },
            sessions: vec![SessionDto::Local {
                shell: "/bin/zsh".to_string(),
                cwd: Some("/tmp".to_string()),
            }],
        };
        let json = serde_json::to_string(&tab_state).unwrap();
        let parsed: TabState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, tab_state);
        assert_eq!(ss::SESSION_STATE_VERSION, 1);
    }

    /// Full pipeline over real local PTYs: snapshot hand-built windows →
    /// rebuild tabs from the state → titles, cwd, layout, and clamps hold.
    /// Local-only, so nothing touches the network or the user's config.
    #[test]
    fn snapshot_then_restore_local_windows_roundtrips() {
        let tmp = tempfile::tempdir().unwrap();
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());

        // Two tabs: one shell in the tempdir, one plain. Built like
        // add_local_tab_to_window_in does.
        let make_tab = |title: &str, cwd: Option<String>, id: usize| {
            let session = TerminalSession::new_local_in(id, &shell, cwd.as_deref());
            Tab {
                title: title.to_string(),
                sessions: vec![session],
                layout: PaneNode::Terminal(0),
                focused_session: 0,
                broadcast_enabled: false,
                tools_drawer_open: false,
                tools_tab: 0,
                pending_snippet: None,
                ai_chat: std::sync::Arc::new(std::sync::Mutex::new(
                    crate::ui::state::ai_chat::AiChatState::default(),
                )),
                pending_ai_command: None,
            }
        };
        let mut window = AppWindow::new_window(
            egui::ViewportId(egui::Id::new("test-window")),
            "Portal",
            make_tab("in-tmp", Some(tmp.path().to_string_lossy().to_string()), 0),
            1,
        );
        window.tabs.push(make_tab("plain", None, 1));
        window.active_tab = 1;

        let state = snapshot(&[window]);
        assert_eq!(state.windows.len(), 1);
        assert_eq!(state.windows[0].active_tab, 1);
        assert_eq!(state.windows[0].tabs.len(), 2);

        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let (tabs, active_tab, next_id) =
            build_tabs_from_state(&state, &[], &[], &rt).expect("restorable");

        assert_eq!(tabs.len(), 2);
        assert_eq!(active_tab, 1);
        assert!(next_id >= 101, "restore consumes ids from 100 up, got {next_id}");
        assert_eq!(tabs[0].title, "in-tmp");
        assert_eq!(tabs[1].title, "plain");
        // Restored sessions are Local with the recorded cwd.
        for tab in &tabs {
            assert!(matches!(tab.sessions[0].kind, SessionKind::Local(_, _)));
        }
        assert_eq!(
            tabs[0].sessions[0].cwd.as_deref(),
            Some(tmp.path().to_string_lossy().as_ref())
        );
        assert!(matches!(tabs[0].layout, PaneNode::Terminal(0)));

        drop(tabs);
        rt.shutdown_background();
    }

    /// An SSH session whose host entry no longer exists degrades to a
    /// local shell (layout indices stay valid), keeping the tab alive.
    #[test]
    fn restore_missing_host_falls_back_to_local() {
        let state = SessionStateFile {
            version: ss::SESSION_STATE_VERSION,
            windows: vec![WindowState {
                active_tab: 0,
                tabs: vec![TabState {
                    title: "gone".to_string(),
                    broadcast_enabled: false,
                    focused_session: 0,
                    layout: PaneNodeDto::Terminal { session: 0 },
                    sessions: vec![SessionDto::Ssh {
                        host_name: "deleted-host".to_string(),
                        cols: 80,
                        rows: 24,
                        transfer_interrupted: false,
                    }],
                }],
            }],
        };

        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let (tabs, _, _) = build_tabs_from_state(&state, &[], &[], &rt).expect("restorable");
        assert_eq!(tabs.len(), 1);
        // Fallback session, not an SSH attempt against a nonexistent host.
        assert!(matches!(tabs[0].sessions[0].kind, SessionKind::Local(_, _)));
        // The banner line is in the grid.
        let grid = tabs[0].sessions[0].grid.lock().unwrap();
        let row_text: String = grid.cells[0]
            .iter()
            .map(|c| c.c)
            .collect();
        drop(grid);
        assert!(
            row_text.contains("deleted-host"),
            "banner must name the missing host, got: {row_text:?}"
        );

        drop(tabs);
        rt.shutdown_background();
    }

    /// Regression: a snapshot whose tabs all have `"sessions": []` (saved
    /// between session teardown and tab close) used to underflow the
    /// focus-index clamp in debug builds, aborting at every launch. It must
    /// restore to nothing (fresh default window), not crash.
    #[test]
    fn restore_all_empty_tabs_falls_back_to_fresh_window() {
        let state = SessionStateFile {
            version: ss::SESSION_STATE_VERSION,
            windows: vec![WindowState {
                active_tab: 1,
                tabs: vec![
                    TabState {
                        title: "Terminal 1".to_string(),
                        broadcast_enabled: false,
                        focused_session: 0,
                        layout: PaneNodeDto::Terminal { session: 0 },
                        sessions: vec![],
                    },
                    TabState {
                        title: "blog".to_string(),
                        broadcast_enabled: false,
                        focused_session: 0,
                        layout: PaneNodeDto::Terminal { session: 0 },
                        sessions: vec![],
                    },
                ],
            }],
        };

        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        assert!(build_tabs_from_state(&state, &[], &[], &rt).is_none());
        rt.shutdown_background();
    }

    /// Empty tabs are skipped alongside usable ones, and the surviving
    /// tabs' indices stay valid.
    #[test]
    fn restore_skips_empty_tabs_keeps_usable_ones() {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        let state = SessionStateFile {
            version: ss::SESSION_STATE_VERSION,
            windows: vec![WindowState {
                active_tab: 1,
                tabs: vec![
                    TabState {
                        title: "empty".to_string(),
                        broadcast_enabled: false,
                        focused_session: 0,
                        layout: PaneNodeDto::Terminal { session: 0 },
                        sessions: vec![],
                    },
                    TabState {
                        title: "alive".to_string(),
                        broadcast_enabled: false,
                        focused_session: 0,
                        layout: PaneNodeDto::Terminal { session: 0 },
                        sessions: vec![SessionDto::Local {
                            shell: shell.clone(),
                            cwd: None,
                        }],
                    },
                ],
            }],
        };

        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let (tabs, active_tab, _) =
            build_tabs_from_state(&state, &[], &[], &rt).expect("one usable tab");
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].title, "alive");
        assert_eq!(active_tab, 0, "clamped to the surviving tab");

        drop(tabs);
        rt.shutdown_background();
    }
}
