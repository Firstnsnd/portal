//! Session-state persistence DTOs (session_state.json).
//!
//! Pure serialization — no app types here (the `portal` crate must stay
//! UI-agnostic). The orchestration that maps these DTOs to/from live
//! `AppWindow`s lives in `app/session_restore.rs`.
//!
//! v1 scope: root-window tabs only. Detached windows are snapshotted too,
//! but restore rebuilds only the root window's tabs; SSH sessions reconnect
//! through the normal connect path (a failed connect shows the usual
//! failure overlay).

use serde::{Deserialize, Serialize};

use std::path::PathBuf;

pub const SESSION_STATE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionStateFile {
    pub version: u32,
    pub windows: Vec<WindowState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowState {
    pub tabs: Vec<TabState>,
    pub active_tab: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TabState {
    pub title: String,
    pub broadcast_enabled: bool,
    pub focused_session: usize,
    pub layout: PaneNodeDto,
    pub sessions: Vec<SessionDto>,
}

/// Mirror of `ui::pane::PaneNode` (Terminal indices index `TabState.sessions`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum PaneNodeDto {
    #[serde(rename = "terminal")]
    Terminal { session: usize },
    #[serde(rename = "split")]
    Split {
        direction: SplitDirectionDto,
        ratio: f32,
        first: Box<PaneNodeDto>,
        second: Box<PaneNodeDto>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SplitDirectionDto {
    #[serde(rename = "horizontal")]
    Horizontal,
    #[serde(rename = "vertical")]
    Vertical,
}

/// One restored session. Local shells restart in their recorded cwd; SSH
/// sessions reconnect by exact host-entry name.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind")]
pub enum SessionDto {
    #[serde(rename = "local")]
    Local {
        shell: String,
        #[serde(default)]
        cwd: Option<String>,
    },
    #[serde(rename = "ssh")]
    Ssh {
        host_name: String,
        #[serde(default = "default_cols")]
        cols: u16,
        #[serde(default = "default_rows")]
        rows: u16,
        /// True when the snapshot caught the session disconnected — the
        /// restored session will go straight to the reconnect overlay.
        #[serde(default)]
        transfer_interrupted: bool,
    },
}

fn default_cols() -> u16 {
    80
}

fn default_rows() -> u16 {
    24
}

/// Path of the session-state file in the config dir.
pub fn session_state_file_path() -> PathBuf {
    super::config_dir().join("session_state.json")
}

/// Atomically save the session state. Failures are logged, never fatal —
/// a broken save must not take down exit.
pub fn save_session_state(path: &std::path::Path, state: &SessionStateFile) {
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!("session-state: cannot create config dir: {}", e);
            return;
        }
    }
    let json = match serde_json::to_string_pretty(state) {
        Ok(json) => json,
        Err(e) => {
            log::warn!("session-state: serialize failed: {}", e);
            return;
        }
    };
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, json) {
        log::warn!("session-state: write failed: {}", e);
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        log::warn!("session-state: rename failed: {}", e);
    }
}

/// Load the session state. A missing file → `None`. A corrupt or
/// future-version file is renamed `.bad` and also reported as `None` so a
/// bad snapshot can never wedge startup.
pub fn load_session_state(path: &std::path::Path) -> Option<SessionStateFile> {
    let data = match std::fs::read_to_string(path) {
        Ok(data) => data,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            log::warn!("session-state: read failed: {}", e);
            return None;
        }
    };
    match serde_json::from_str::<SessionStateFile>(&data) {
        Ok(state) if state.version == SESSION_STATE_VERSION => Some(state),
        Ok(state) => {
            log::warn!(
                "session-state: version {} not supported (expected {})",
                state.version, SESSION_STATE_VERSION
            );
            let bad = path.with_extension("bad");
            let _ = std::fs::rename(path, &bad);
            None
        }
        Err(e) => {
            log::warn!("session-state: corrupt file ({}); renamed .bad", e);
            let bad = path.with_extension("bad");
            let _ = std::fs::rename(path, &bad);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_node_dto_roundtrip() {
        let node = PaneNodeDto::Split {
            direction: SplitDirectionDto::Vertical,
            ratio: 0.6,
            first: Box::new(PaneNodeDto::Terminal { session: 0 }),
            second: Box::new(PaneNodeDto::Split {
                direction: SplitDirectionDto::Horizontal,
                ratio: 0.5,
                first: Box::new(PaneNodeDto::Terminal { session: 1 }),
                second: Box::new(PaneNodeDto::Terminal { session: 2 }),
            }),
        };
        let json = serde_json::to_string(&node).unwrap();
        let parsed: PaneNodeDto = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, node);
    }

    #[test]
    fn session_dto_serde_transfer_interrupted_defaults_false() {
        let json = r#"{"kind":"ssh","host_name":"prod-1"}"#;
        let dto: SessionDto = serde_json::from_str(json).unwrap();
        assert!(matches!(dto, SessionDto::Ssh { transfer_interrupted: false, .. }));

        let local = SessionDto::Local {
            shell: "/bin/zsh".to_string(),
            cwd: Some("/tmp".to_string()),
        };
        let back: SessionDto =
            serde_json::from_str(&serde_json::to_string(&local).unwrap()).unwrap();
        assert_eq!(back, local);
    }

    #[test]
    fn corrupt_state_returns_none_and_renames_bad() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session_state.json");
        std::fs::write(&path, "{not json").unwrap();

        let result = load_session_state(&path);
        assert!(result.is_none());
        assert!(!path.exists(), "corrupt file must be renamed");
        assert!(dir.path().join("session_state.bad").exists());
    }

    #[test]
    fn missing_file_is_ok_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nope.json");
        assert!(load_session_state(&path).is_none());
    }

    #[test]
    fn future_version_is_rejected_and_renamed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session_state.json");
        let future = SessionStateFile {
            version: 999,
            windows: Vec::new(),
        };
        std::fs::write(&path, serde_json::to_string(&future).unwrap()).unwrap();

        assert!(load_session_state(&path).is_none());
        assert!(!path.exists());
        assert!(dir.path().join("session_state.bad").exists());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session_state.json");
        let state = SessionStateFile {
            version: SESSION_STATE_VERSION,
            windows: vec![WindowState {
                active_tab: 1,
                tabs: vec![
                    TabState {
                        title: "Terminal 1".to_string(),
                        broadcast_enabled: false,
                        focused_session: 0,
                        layout: PaneNodeDto::Terminal { session: 0 },
                        sessions: vec![SessionDto::Local {
                            shell: "/bin/zsh".to_string(),
                            cwd: None,
                        }],
                    },
                    TabState {
                        title: "prod".to_string(),
                        broadcast_enabled: true,
                        focused_session: 0,
                        layout: PaneNodeDto::Terminal { session: 0 },
                        sessions: vec![SessionDto::Ssh {
                            host_name: "prod-1".to_string(),
                            cols: 120,
                            rows: 40,
                            transfer_interrupted: false,
                        }],
                    },
                ],
            }],
        };
        save_session_state(&path, &state);
        let loaded = load_session_state(&path).unwrap();
        assert_eq!(loaded, state);
    }
}
