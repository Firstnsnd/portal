//! # SFTP Drop Logic
//!
//! Pure decision functions for SFTP drag-and-drop: OS-drop hit-testing and
//! in-app transfer planning. Side-effect free so they can be unit-tested
//! without an egui context or SSH connection; the executor lives in the view.

use std::path::PathBuf;

use eframe::egui;

use crate::sftp::drop_planner::{
    copy_local_dir, copy_local_file, join_remote_path, DropSide, OsDropTarget,
    PlannedTransfer, TransferPlan,
};
use crate::sftp::{SftpBrowser, SftpConnectionState};
use crate::ui::pane::AppWindow;
use crate::ui::types::sftp_types::SftpPanel;

use super::types::{DragEntry, DragPayload};

/// Current directory of each panel as seen by the planner. A remote entry is
/// `Some` only when that side's connection is established.
pub(crate) struct PanelDirs {
    pub left_local: Option<PathBuf>,
    pub right_local: Option<PathBuf>,
    pub left_remote: Option<String>,
    pub right_remote: Option<String>,
}

/// Resolve which panel an OS-level drop position (in view coordinates) lands
/// on. The pointer can be over neither rect (the divider seam, or slightly
/// outside); in that case fall back to the caller's notion of the active side.
pub(crate) fn resolve_os_drop_target(
    pos: egui::Pos2,
    left_rect: egui::Rect,
    right_rect: egui::Rect,
    active: DropSide,
) -> DropSide {
    if left_rect.contains(pos) {
        DropSide::Left
    } else if right_rect.contains(pos) {
        DropSide::Right
    } else {
        active
    }
}

/// Plan transfers for an in-app panel-to-panel drop. `origin`/`target` name
/// concrete panels; routing (which connection a transfer travels through)
/// follows the remote side of the transfer.
///
/// Returns an empty plan for anything unsupported today: same panel,
/// local→local (existing no-op behavior), remote→remote, and a remote target
/// that is not connected.
pub(crate) fn plan_in_app_drop(
    payload: &DragPayload,
    origin: SftpPanel,
    target: SftpPanel,
    dirs: &PanelDirs,
) -> TransferPlan {
    if origin == target {
        return TransferPlan::default();
    }
    let transfers = match (origin, target) {
        // local → local: preserved no-op (existing behavior)
        (_, t) if t.is_local() && origin.is_local() => vec![],
        // remote → remote: would need a cross-connection relay; unsupported
        (_, t) if !t.is_local() && !origin.is_local() => vec![],

        // local → remote upload, through the target side's connection
        (_, SftpPanel::LeftRemote) => match &dirs.left_remote {
            Some(dir) => payload
                .entries
                .iter()
                .map(|e| planned_upload(e, dir, DropSide::Left))
                .collect(),
            None => vec![],
        },
        (_, SftpPanel::RightRemote) => match &dirs.right_remote {
            Some(dir) => payload
                .entries
                .iter()
                .map(|e| planned_upload(e, dir, DropSide::Right))
                .collect(),
            None => vec![],
        },

        // remote → local download, through the origin side's connection
        (SftpPanel::LeftRemote, _) => match &dirs.right_local {
            Some(dir) => payload
                .entries
                .iter()
                .map(|e| planned_download(e, dir, DropSide::Left))
                .collect(),
            None => vec![],
        },
        (SftpPanel::RightRemote, _) => match &dirs.left_local {
            Some(dir) => payload
                .entries
                .iter()
                .map(|e| planned_download(e, dir, DropSide::Right))
                .collect(),
            None => vec![],
        },

        // Remaining combinations (local→local with missing dirs) are no-ops
        _ => vec![],
    };
    TransferPlan { transfers }
}

fn planned_upload(entry: &DragEntry, remote_dir: &str, via: DropSide) -> PlannedTransfer {
    let remote = join_remote_path(remote_dir, &entry.entry_name);
    let local = PathBuf::from(&entry.full_path);
    if entry.is_dir {
        PlannedTransfer::UploadDir { local, remote, via }
    } else {
        PlannedTransfer::UploadFile { local, remote, via }
    }
}

fn planned_download(entry: &DragEntry, local_dir: &std::path::Path, via: DropSide) -> PlannedTransfer {
    let local = local_dir.join(&entry.entry_name);
    let remote = entry.full_path.clone();
    if entry.is_dir {
        PlannedTransfer::DownloadDir { remote, local, via }
    } else {
        PlannedTransfer::DownloadFile { remote, local, via }
    }
}

/// True when `pos` lies at least `margin` points outside `rect` on any side.
/// Used to decide that the pointer has left the app's windows even while the
/// OS drag keeps reporting stale hover positions.
pub(crate) fn pointer_beyond_margin(pos: egui::Pos2, rect: egui::Rect, margin: f32) -> bool {
    pos.x < rect.min.x - margin
        || pos.x > rect.max.x + margin
        || pos.y < rect.min.y - margin
        || pos.y > rect.max.y + margin
}

/// Outcome of resolving an OS drop position against the current panel layout.
pub(crate) enum OsDropResolution {
    /// A droppable panel: local dir or connected remote dir.
    Target(OsDropTarget),
    /// The drop aimed at a remote panel whose connection is not established.
    DisconnectedRemote,
}

/// Describe the OS drop target for a resolved side based on the window's
/// current panel layout (which side is local, which remote side is connected).
pub(crate) fn os_drop_target_for(window: &AppWindow, side: DropSide) -> OsDropResolution {
    fn connected(b: Option<&SftpBrowser>) -> Option<&SftpBrowser> {
        b.filter(|b| matches!(b.state, SftpConnectionState::Connected))
    }
    match side {
        DropSide::Left if window.left_panel_is_local => OsDropResolution::Target(
            OsDropTarget::Local { dir: PathBuf::from(&window.local_browser_left.current_path) },
        ),
        DropSide::Left => match connected(window.sftp_browser_left.as_ref()) {
            Some(b) => OsDropResolution::Target(OsDropTarget::Remote {
                side,
                dir: b.current_path.clone(),
            }),
            None => OsDropResolution::DisconnectedRemote,
        },
        DropSide::Right if window.right_panel_is_local => OsDropResolution::Target(
            OsDropTarget::Local { dir: PathBuf::from(&window.local_browser_right.current_path) },
        ),
        DropSide::Right => match connected(window.sftp_browser.as_ref()) {
            Some(b) => OsDropResolution::Target(OsDropTarget::Remote {
                side,
                dir: b.current_path.clone(),
            }),
            None => OsDropResolution::DisconnectedRemote,
        },
    }
}

fn remote_browser_mut(window: &mut AppWindow, side: DropSide) -> Option<&mut SftpBrowser> {
    match side {
        DropSide::Left => window.sftp_browser_left.as_mut(),
        DropSide::Right => window.sftp_browser.as_mut(),
    }
}

/// Snapshot of every panel's current directory, with remote entries `Some`
/// only while that side is connected — the input shape `plan_in_app_drop`
/// expects.
pub(crate) fn panel_dirs_for(window: &AppWindow) -> PanelDirs {
    fn connected_dir(b: Option<&SftpBrowser>) -> Option<String> {
        b.filter(|b| matches!(b.state, SftpConnectionState::Connected))
            .map(|b| b.current_path.clone())
    }
    PanelDirs {
        left_local: Some(PathBuf::from(&window.local_browser_left.current_path)),
        right_local: Some(PathBuf::from(&window.local_browser_right.current_path)),
        left_remote: connected_dir(window.sftp_browser_left.as_ref()),
        right_remote: connected_dir(window.sftp_browser.as_ref()),
    }
}

/// Execute a planned transfer against the window's browsers and panels.
/// Returns the first error encountered (missing connection for a planned
/// transfer, or a local copy failure) for the caller to surface; transfers
/// issued before the failure are already in flight and are not rolled back.
pub(crate) fn execute_transfer_plan(window: &mut AppWindow, plan: &TransferPlan) -> Option<String> {
    if plan.is_empty() {
        return None;
    }
    for t in &plan.transfers {
        match t {
            PlannedTransfer::UploadFile { local, remote, via } => {
                let Some(b) = remote_browser_mut(window, *via) else {
                    return Some(format!("{}: not connected", remote));
                };
                let name = local
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| remote.clone());
                b.mark_transfer_preparing(&name, true);
                b.upload(&local.to_string_lossy(), remote);
            }
            PlannedTransfer::UploadDir { local, remote, via } => {
                let Some(b) = remote_browser_mut(window, *via) else {
                    return Some(format!("{}: not connected", remote));
                };
                let name = local
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| remote.clone());
                b.mark_transfer_preparing(&name, true);
                b.upload_dir(&local.to_string_lossy(), remote);
            }
            PlannedTransfer::DownloadFile { remote, local, via } => {
                let Some(b) = remote_browser_mut(window, *via) else {
                    return Some(format!("{}: not connected", remote));
                };
                let name = remote.rsplit('/').next().unwrap_or(remote).to_string();
                b.mark_transfer_preparing(&name, false);
                b.download(remote, &local.to_string_lossy());
            }
            PlannedTransfer::DownloadDir { remote, local, via } => {
                let Some(b) = remote_browser_mut(window, *via) else {
                    return Some(format!("{}: not connected", remote));
                };
                let name = remote.rsplit('/').next().unwrap_or(remote).to_string();
                b.mark_transfer_preparing(&name, false);
                b.download_dir(remote, &local.to_string_lossy());
            }
            PlannedTransfer::CopyFile { src, dest } => {
                if let Err(e) = copy_local_file(src, dest) {
                    return Some(format!("{}: {}", dest.display(), e));
                }
            }
            PlannedTransfer::CopyDir { src, dest } => {
                if let Err(e) = copy_local_dir(src, dest) {
                    return Some(format!("{}: {}", dest.display(), e));
                }
            }
        }
    }
    // Local copies bypass the transfer pipeline (no TransferComplete response
    // will refresh the panels), so refresh eagerly.
    if plan.transfers.iter().any(|t| matches!(t, PlannedTransfer::CopyFile { .. } | PlannedTransfer::CopyDir { .. })) {
        window.local_browser_left.refresh();
        window.local_browser_right.refresh();
    }
    None
}

/// Decide whether an in-app drag of remote entries should escalate into a
/// native macOS file-promise drag session. All of: a remote-source payload
/// must be in flight, the pointer must be outside all app windows, the
/// primary button must still be held, and no session may already be running.
pub(crate) fn should_start_os_drag(
    remote_payload_active: bool,
    pointer_outside_app: bool,
    primary_down: bool,
    session_active: bool,
) -> bool {
    remote_payload_active && pointer_outside_app && primary_down && !session_active
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{pos2, vec2, Rect};

    fn panel_dirs() -> PanelDirs {
        PanelDirs {
            left_local: Some(PathBuf::from("/Users/x")),
            right_local: Some(PathBuf::from("/Users/x/dl")),
            left_remote: Some("/home/left".to_string()),
            right_remote: Some("/srv/upload".to_string()),
        }
    }

    fn file_entry(name: &str, base: &str) -> DragEntry {
        DragEntry {
            full_path: format!("{}/{}", base, name),
            entry_name: name.to_string(),
            is_dir: false,
        }
    }

    fn dir_entry(name: &str, base: &str) -> DragEntry {
        DragEntry {
            full_path: format!("{}/{}", base, name),
            entry_name: name.to_string(),
            is_dir: true,
        }
    }

    // ------------------------------------------------------------------
    // resolve_os_drop_target
    // ------------------------------------------------------------------

    #[test]
    fn os_drop_pos_in_left_rect_targets_left() {
        let left = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 400.0));
        let right = Rect::from_min_size(pos2(201.0, 0.0), vec2(200.0, 400.0));
        let got = resolve_os_drop_target(pos2(100.0, 200.0), left, right, DropSide::Right);
        assert_eq!(got, DropSide::Left);
    }

    #[test]
    fn os_drop_pos_in_right_rect_targets_right() {
        let left = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 400.0));
        let right = Rect::from_min_size(pos2(201.0, 0.0), vec2(200.0, 400.0));
        let got = resolve_os_drop_target(pos2(350.0, 120.0), left, right, DropSide::Left);
        assert_eq!(got, DropSide::Right);
    }

    #[test]
    fn os_drop_pos_in_divider_seam_falls_back_to_active() {
        // 2pt seam between the panels hits neither rect.
        let left = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 400.0));
        let right = Rect::from_min_size(pos2(201.0, 0.0), vec2(200.0, 400.0));
        let got = resolve_os_drop_target(pos2(200.5, 200.0), left, right, DropSide::Right);
        assert_eq!(got, DropSide::Right);
    }

    #[test]
    fn os_drop_pos_outside_both_rects_falls_back_to_active() {
        let left = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 400.0));
        let right = Rect::from_min_size(pos2(201.0, 0.0), vec2(200.0, 400.0));
        let got = resolve_os_drop_target(pos2(-50.0, 700.0), left, right, DropSide::Left);
        assert_eq!(got, DropSide::Left);
    }

    // ------------------------------------------------------------------
    // plan_in_app_drop
    // ------------------------------------------------------------------

    #[test]
    fn in_app_local_left_to_remote_right_plans_upload() {
        let payload = DragPayload {
            is_local: true,
            origin: SftpPanel::LeftLocal,
            entries: vec![file_entry("report.pdf", "/Users/x")],
        };
        let plan = plan_in_app_drop(&payload, SftpPanel::LeftLocal, SftpPanel::RightRemote, &panel_dirs());
        assert_eq!(plan.transfers, vec![PlannedTransfer::UploadFile {
            local: PathBuf::from("/Users/x/report.pdf"),
            remote: "/srv/upload/report.pdf".to_string(),
            via: DropSide::Right,
        }]);
    }

    #[test]
    fn in_app_remote_right_to_local_left_plans_download() {
        let payload = DragPayload {
            is_local: false,
            origin: SftpPanel::RightRemote,
            entries: vec![file_entry("archive.tar.gz", "/srv/upload")],
        };
        let plan = plan_in_app_drop(&payload, SftpPanel::RightRemote, SftpPanel::LeftLocal, &panel_dirs());
        assert_eq!(plan.transfers, vec![PlannedTransfer::DownloadFile {
            remote: "/srv/upload/archive.tar.gz".to_string(),
            local: PathBuf::from("/Users/x/archive.tar.gz"),
            via: DropSide::Right,
        }]);
    }

    /// Regression: a left-side remote panel previously had no drop handler at
    /// all (silent no-op). Dragging from it to a local panel must download via
    /// the LEFT connection.
    #[test]
    fn in_app_left_remote_to_right_local_plans_download_via_left() {
        let payload = DragPayload {
            is_local: false,
            origin: SftpPanel::LeftRemote,
            entries: vec![file_entry("notes.md", "/home/left")],
        };
        let plan = plan_in_app_drop(&payload, SftpPanel::LeftRemote, SftpPanel::RightLocal, &panel_dirs());
        assert_eq!(plan.transfers, vec![PlannedTransfer::DownloadFile {
            remote: "/home/left/notes.md".to_string(),
            local: PathBuf::from("/Users/x/dl/notes.md"),
            via: DropSide::Left,
        }]);
    }

    #[test]
    fn in_app_right_local_to_left_remote_plans_upload_via_left() {
        let payload = DragPayload {
            is_local: true,
            origin: SftpPanel::RightLocal,
            entries: vec![file_entry("pic.png", "/Users/x/dl")],
        };
        let plan = plan_in_app_drop(&payload, SftpPanel::RightLocal, SftpPanel::LeftRemote, &panel_dirs());
        assert_eq!(plan.transfers, vec![PlannedTransfer::UploadFile {
            local: PathBuf::from("/Users/x/dl/pic.png"),
            remote: "/home/left/pic.png".to_string(),
            via: DropSide::Left,
        }]);
    }

    #[test]
    fn in_app_local_to_local_yields_empty_plan() {
        let payload = DragPayload {
            is_local: true,
            origin: SftpPanel::LeftLocal,
            entries: vec![file_entry("a.txt", "/Users/x")],
        };
        let plan = plan_in_app_drop(&payload, SftpPanel::LeftLocal, SftpPanel::RightLocal, &panel_dirs());
        assert!(plan.is_empty());
    }

    #[test]
    fn in_app_remote_to_remote_yields_empty_plan() {
        let payload = DragPayload {
            is_local: false,
            origin: SftpPanel::RightRemote,
            entries: vec![file_entry("a.txt", "/srv/upload")],
        };
        let plan = plan_in_app_drop(&payload, SftpPanel::RightRemote, SftpPanel::LeftRemote, &panel_dirs());
        assert!(plan.is_empty());
    }

    #[test]
    fn in_app_same_panel_yields_empty_plan() {
        let payload = DragPayload {
            is_local: false,
            origin: SftpPanel::RightRemote,
            entries: vec![file_entry("a.txt", "/srv/upload")],
        };
        let plan = plan_in_app_drop(&payload, SftpPanel::RightRemote, SftpPanel::RightRemote, &panel_dirs());
        assert!(plan.is_empty());
    }

    #[test]
    fn in_app_remote_target_not_connected_yields_empty_plan() {
        let payload = DragPayload {
            is_local: true,
            origin: SftpPanel::LeftLocal,
            entries: vec![file_entry("a.txt", "/Users/x")],
        };
        let mut dirs = panel_dirs();
        dirs.right_remote = None;
        let plan = plan_in_app_drop(&payload, SftpPanel::LeftLocal, SftpPanel::RightRemote, &dirs);
        assert!(plan.is_empty());
    }

    #[test]
    fn in_app_multiple_entries_preserve_order_and_dir_kinds() {
        let payload = DragPayload {
            is_local: false,
            origin: SftpPanel::RightRemote,
            entries: vec![
                file_entry("z.txt", "/srv/upload"),
                dir_entry("assets", "/srv/upload"),
                file_entry("a.txt", "/srv/upload"),
            ],
        };
        let plan = plan_in_app_drop(&payload, SftpPanel::RightRemote, SftpPanel::LeftLocal, &panel_dirs());
        assert_eq!(plan.transfers, vec![
            PlannedTransfer::DownloadFile {
                remote: "/srv/upload/z.txt".to_string(),
                local: PathBuf::from("/Users/x/z.txt"),
                via: DropSide::Right,
            },
            PlannedTransfer::DownloadDir {
                remote: "/srv/upload/assets".to_string(),
                local: PathBuf::from("/Users/x/assets"),
                via: DropSide::Right,
            },
            PlannedTransfer::DownloadFile {
                remote: "/srv/upload/a.txt".to_string(),
                local: PathBuf::from("/Users/x/a.txt"),
                via: DropSide::Right,
            },
        ]);
    }

    // ------------------------------------------------------------------
    // pointer_beyond_margin
    // ------------------------------------------------------------------

    #[test]
    fn pointer_inside_rect_is_not_beyond() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 400.0));
        assert!(!pointer_beyond_margin(pos2(100.0, 200.0), rect, 2.0));
    }

    #[test]
    fn pointer_within_margin_of_edge_is_not_beyond() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 400.0));
        assert!(!pointer_beyond_margin(pos2(-1.0, 200.0), rect, 2.0));
        assert!(!pointer_beyond_margin(pos2(201.0, 200.0), rect, 2.0));
    }

    #[test]
    fn pointer_past_margin_is_beyond() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 400.0));
        assert!(pointer_beyond_margin(pos2(-3.0, 200.0), rect, 2.0));
        assert!(pointer_beyond_margin(pos2(100.0, 405.0), rect, 2.0));
    }

    // ------------------------------------------------------------------
    // should_start_os_drag
    // ------------------------------------------------------------------

    #[test]
    fn os_drag_starts_for_remote_payload_outside_window_while_pressed() {
        assert!(should_start_os_drag(true, true, true, false));
    }

    #[test]
    fn os_drag_never_starts_for_local_payload() {
        assert!(!should_start_os_drag(false, true, true, false));
    }

    #[test]
    fn os_drag_never_starts_while_pointer_still_inside_app() {
        assert!(!should_start_os_drag(true, false, true, false));
    }

    #[test]
    fn os_drag_never_starts_after_primary_release() {
        assert!(!should_start_os_drag(true, true, false, false));
    }

    #[test]
    fn os_drag_never_starts_when_session_already_active() {
        assert!(!should_start_os_drag(true, true, true, true));
    }

    #[test]
    fn os_drag_never_starts_without_any_drag_payload() {
        // No payload at all (remote_payload_active=false covers both "no
        // drag" and "dragging local entries").
        assert!(!should_start_os_drag(false, false, true, false));
    }
}
