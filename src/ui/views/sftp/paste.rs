//! # SFTP Clipboard (copy/paste)
//!
//! Snapshot the active selection of a panel into the app clipboard
//! (`SftpClipboard`) and route a paste onto whatever panel receives it:
//!
//! - local → local: `PlannedTransfer::CopyFile/CopyDir` (sync `fs::copy`)
//! - local → remote / remote → local: upload/download through the involved
//!   side's connection, exactly like a drag-and-drop drop
//! - remote → remote, same connection: streamed `CopyRemoteFile/CopyRemoteDir`
//! - remote → remote, cross-connection (two servers): a relay thread stages
//!   each entry through a temp dir (`DownloadSync` → `UploadSync`) so the two
//!   serial command queues never interleave
//!
//! Errors are reported to the caller for the standard error dialog; the relay
//! thread itself only logs (nothing on the UI thread to report to).

use std::path::PathBuf;

use crate::sftp::drop_planner::{
    join_remote_path, DropSide, PlannedTransfer, TransferPlan,
};
use crate::sftp::FileSelection;
use crate::sftp::{SftpCommand, SftpConnectionState, SftpEntry, SftpEntryKind};
use crate::ui::pane::AppWindow;
use crate::ui::types::sftp_types::SftpPanel;

use super::drop_logic::execute_transfer_plan;
use super::types::{ClipboardRequest, DragEntry, SftpClipboard};

/// Apply a keyboard-driven clipboard request captured by `render_file_panel`.
/// Returns an error detail for the caller to surface, if any.
pub(crate) fn handle_clipboard_request(
    window: &mut AppWindow,
    duplicate_suffix: &str,
    panel: SftpPanel,
    request: ClipboardRequest,
) -> Option<String> {
    match request {
        ClipboardRequest::Copy => {
            window.sftp_clipboard = capture_clipboard(window, panel);
            None
        }
        ClipboardRequest::Paste => execute_paste(window, panel, duplicate_suffix),
    }
}

/// Snapshot the current selection of `panel` into a clipboard. `None` when
/// nothing is selected — callers treat that as a no-op rather than storing
/// an empty clipboard.
pub(crate) fn capture_clipboard(window: &AppWindow, panel: SftpPanel) -> Option<SftpClipboard> {
    let (path, selection, filtered) = match panel {
        SftpPanel::LeftLocal => (
            &window.local_browser_left.current_path,
            &window.local_browser_left.selection,
            window.local_browser_left.filtered_entries(),
        ),
        SftpPanel::RightLocal => (
            &window.local_browser_right.current_path,
            &window.local_browser_right.selection,
            window.local_browser_right.filtered_entries(),
        ),
        SftpPanel::LeftRemote => {
            let b = window.sftp_browser_left.as_ref()?;
            (
                &b.current_path,
                &b.selection,
                b.filtered_entries(),
            )
        }
        SftpPanel::RightRemote => {
            let b = window.sftp_browser.as_ref()?;
            (
                &b.current_path,
                &b.selection,
                b.filtered_entries(),
            )
        }
    };
    let entries = snapshot_entries(path, selection, &filtered);
    if entries.is_empty() {
        None
    } else {
        Some(SftpClipboard { origin: panel, entries })
    }
}

/// Selection indices index into the *filtered* listing (what the panel
/// rendered), so the snapshot must use the same filtered view.
fn snapshot_entries(
    current_path: &str,
    selection: &FileSelection,
    filtered: &[SftpEntry],
) -> Vec<DragEntry> {
    selection
        .selected
        .iter()
        .filter_map(|&i| filtered.get(i))
        .map(|e| DragEntry {
            full_path: join_remote_path(current_path, &e.name),
            entry_name: e.name.clone(),
            is_dir: e.kind == SftpEntryKind::Directory,
        })
        .collect()
}

/// Pick the destination name for a pasted entry. When `rename` (pasting back
/// into the directory the entries came from) a colliding name auto-renames
/// Finder-style; otherwise the original name is kept (silent overwrite,
/// matching drop semantics). `roster` grows with every assigned name so a
/// multi-entry paste never collides with itself.
fn paste_dest_name(roster: &mut Vec<String>, name: &str, rename: bool, suffix: &str) -> String {
    if !rename {
        return name.to_string();
    }
    let chosen = dedupe_name(roster, name, suffix);
    roster.push(chosen.clone());
    chosen
}

/// Finder-style rename: `name` if free, else `name {suffix}`, `name {suffix} 2`, …
fn dedupe_name(existing: &[String], name: &str, suffix: &str) -> String {
    if !existing.iter().any(|r| r == name) {
        return name.to_string();
    }
    let mut candidate = format!("{} {}", name, suffix);
    let mut n = 2;
    while existing.iter().any(|r| r == &candidate) {
        candidate = format!("{} {} {}", name, suffix, n);
        n += 1;
    }
    candidate
}

/// Parent directory of an absolute path (POSIX-style, remote or local).
fn parent_dir(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) => "/",
        Some(i) => &trimmed[..i],
        None => "",
    }
}

/// Directory of a paste target, with the names already present there (remote
/// rosters come from the listing; local rosters from a best-effort `read_dir`).
enum PasteTarget {
    Local { dir: PathBuf, names: Vec<String> },
    Remote { side: DropSide, dir: String, names: Vec<String> },
}

fn connected_remote_dir(b: Option<&crate::sftp::SftpBrowser>) -> Option<(String, Vec<String>)> {
    let b = b.filter(|b| matches!(b.state, SftpConnectionState::Connected))?;
    Some((
        b.current_path.clone(),
        b.entries.iter().map(|e| e.name.clone()).collect(),
    ))
}

/// Host label for paste error messages (matches the plan executor's plain
/// "path: not connected" style).
fn paste_target_label(b: Option<&crate::sftp::SftpBrowser>) -> String {
    b.map(|b| b.host_name.clone())
        .unwrap_or_else(|| "remote".to_string())
}

/// Whether `panel` can currently receive a paste: local panels always, remote
/// panels only while connected (drives context-menu paste availability).
pub(crate) fn paste_target_ready(window: &AppWindow, panel: SftpPanel) -> bool {
    match panel {
        SftpPanel::LeftLocal | SftpPanel::RightLocal => true,
        SftpPanel::LeftRemote => connected_remote_dir(window.sftp_browser_left.as_ref()).is_some(),
        SftpPanel::RightRemote => connected_remote_dir(window.sftp_browser.as_ref()).is_some(),
    }
}

fn local_dir_names(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// Paste the clipboard into `target`. Returns an error detail for the
/// caller's error dialog (`None` = started or intentionally a no-op).
pub(crate) fn execute_paste(
    window: &mut AppWindow,
    target: SftpPanel,
    duplicate_suffix: &str,
) -> Option<String> {
    let Some(clip) = window.sftp_clipboard.clone() else {
        return None;
    };
    if clip.entries.is_empty() {
        return None;
    }

    // Resolve the target directory and what already lives there.
    let paste_target = match target {
        SftpPanel::LeftLocal => PasteTarget::Local {
            dir: PathBuf::from(&window.local_browser_left.current_path),
            names: local_dir_names(std::path::Path::new(&window.local_browser_left.current_path)),
        },
        SftpPanel::RightLocal => PasteTarget::Local {
            dir: PathBuf::from(&window.local_browser_right.current_path),
            names: local_dir_names(std::path::Path::new(
                &window.local_browser_right.current_path,
            )),
        },
        SftpPanel::LeftRemote => {
            let Some((dir, names)) = connected_remote_dir(window.sftp_browser_left.as_ref()) else {
                return Some(format!("{}: not connected", paste_target_label(window.sftp_browser_left.as_ref())));
            };
            PasteTarget::Remote { side: DropSide::Left, dir, names }
        }
        SftpPanel::RightRemote => {
            let Some((dir, names)) = connected_remote_dir(window.sftp_browser.as_ref()) else {
                return Some(format!("{}: not connected", paste_target_label(window.sftp_browser.as_ref())));
            };
            PasteTarget::Remote { side: DropSide::Right, dir, names }
        }
    };

    // Auto-rename only applies when pasting back into the directory the
    // entries were copied from (string comparison — meaningful only within
    // one filesystem, so remote→local pastes keep overwrite semantics).
    let target_dir_str = match &paste_target {
        PasteTarget::Local { dir, .. } => dir.to_string_lossy().into_owned(),
        PasteTarget::Remote { dir, .. } => dir.clone(),
    };
    let same_dir = clip
        .entries
        .iter()
        .all(|e| parent_dir(&e.full_path) == target_dir_str.as_str());
    let mut roster: Vec<String> = match &paste_target {
        PasteTarget::Local { names, .. } => names.clone(),
        PasteTarget::Remote { names, .. } => names.clone(),
    };
    let dest_names: Vec<String> = clip
        .entries
        .iter()
        .map(|e| paste_dest_name(&mut roster, &e.entry_name, same_dir, duplicate_suffix))
        .collect();

    let origin_local = clip.origin.is_local();
    let target_local = target.is_local();

    match (origin_local, target_local) {
        // ── local → local: sync fs copies via the shared plan executor ──
        (true, true) => {
            let PasteTarget::Local { dir, .. } = paste_target else { unreachable!() };
            let transfers = clip
                .entries
                .iter()
                .zip(&dest_names)
                .map(|(e, name)| {
                    let dest = dir.join(name);
                    if e.is_dir {
                        PlannedTransfer::CopyDir { src: PathBuf::from(&e.full_path), dest }
                    } else {
                        PlannedTransfer::CopyFile { src: PathBuf::from(&e.full_path), dest }
                    }
                })
                .collect();
            execute_transfer_plan(window, &TransferPlan { transfers })
        }
        // ── local → remote: upload through the target side's connection ──
        (true, false) => {
            let PasteTarget::Remote { side, dir, .. } = paste_target else { unreachable!() };
            let transfers = clip
                .entries
                .iter()
                .zip(&dest_names)
                .map(|(e, name)| {
                    let local = PathBuf::from(&e.full_path);
                    let remote = join_remote_path(&dir, name);
                    if e.is_dir {
                        PlannedTransfer::UploadDir { local, remote, via: side }
                    } else {
                        PlannedTransfer::UploadFile { local, remote, via: side }
                    }
                })
                .collect();
            execute_transfer_plan(window, &TransferPlan { transfers })
        }
        // ── remote → local: download through the origin side's connection ──
        (false, true) => {
            let PasteTarget::Local { dir, .. } = paste_target else { unreachable!() };
            let via = match clip.origin {
                SftpPanel::LeftRemote => DropSide::Left,
                SftpPanel::RightRemote => DropSide::Right,
                _ => unreachable!(),
            };
            let mut transfers = Vec::new();
            for (e, name) in clip.entries.iter().zip(&dest_names) {
                // Names came from our own clipboard snapshot, but downloads
                // join onto a local path — keep the same hostile-server guard
                // as drag-and-drop.
                let Some(safe) = crate::sftp::safe_download_name(name) else {
                    log::warn!("paste: skipping entry with unsafe name {:?}", name);
                    continue;
                };
                let local = dir.join(safe);
                let remote = e.full_path.clone();
                if e.is_dir {
                    transfers.push(PlannedTransfer::DownloadDir { remote, local, via });
                } else {
                    transfers.push(PlannedTransfer::DownloadFile { remote, local, via });
                }
            }
            execute_transfer_plan(window, &TransferPlan { transfers })
        }
        // ── remote → remote ──
        (false, false) => {
            let PasteTarget::Remote { dir, .. } = paste_target else { unreachable!() };
            if clip.origin == target {
                paste_same_connection(window, target, &clip.entries, &dest_names, &dir);
                None
            } else {
                paste_cross_connection(window, clip.origin, target, &clip.entries, &dest_names, &dir)
            }
        }
    }
}

/// Remote → remote on ONE connection: stream each entry through this client
/// (`CopyRemoteFile/CopyRemoteDir`). Commands queue serially on the browser's
/// command channel; each `TransferComplete` refreshes the target listing.
fn paste_same_connection(
    window: &mut AppWindow,
    panel: SftpPanel,
    entries: &[DragEntry],
    dest_names: &[String],
    dir: &str,
) {
    let browser = match panel {
        SftpPanel::LeftRemote => window.sftp_browser_left.as_mut(),
        SftpPanel::RightRemote => window.sftp_browser.as_mut(),
        _ => unreachable!(),
    };
    let Some(browser) = browser else { return };
    for (e, name) in entries.iter().zip(dest_names) {
        let to = join_remote_path(dir, name);
        browser.mark_transfer_preparing(name, true);
        if e.is_dir {
            browser.copy_remote_dir(&e.full_path, &to);
        } else {
            browser.copy_remote(&e.full_path, &to);
        }
    }
}

/// One relayed entry: remote src → temp staging → remote dest.
struct RelayItem {
    src: String,
    staging: PathBuf,
    dest: String,
    is_dir: bool,
}

/// Best-effort staging cleanup between relay items (downloads of directories
/// that were cancelled midway may leave partial trees behind).
fn cleanup_staging(item: &RelayItem) {
    let result = if item.is_dir {
        std::fs::remove_dir_all(&item.staging)
    } else {
        std::fs::remove_file(&item.staging)
    };
    if let Err(e) = result {
        log::warn!("paste relay: failed to clean staging {:?}: {}", item.staging, e);
    }
}

/// Remote → remote ACROSS connections (left ↔ right panels on two servers):
/// stage each entry into a temp dir via the source connection's
/// `DownloadSync`/`DownloadDirSync`, then upload it via the target
/// connection's `UploadSync`/`UploadDirSync`, strictly serially.
///
/// Known limitation: cancelling the download leg leaves a partial staging
/// tree; the upload leg starts anyway and recreates files it needs.
fn paste_cross_connection(
    window: &AppWindow,
    origin: SftpPanel,
    target: SftpPanel,
    entries: &[DragEntry],
    dest_names: &[String],
    dest_dir: &str,
) -> Option<String> {
    let src_sender = match origin {
        SftpPanel::LeftRemote => {
            let Some(b) = window.sftp_browser_left.as_ref() else {
                return Some("source: not connected".to_string());
            };
            b.command_sender()
        }
        SftpPanel::RightRemote => {
            let Some(b) = window.sftp_browser.as_ref() else {
                return Some("source: not connected".to_string());
            };
            b.command_sender()
        }
        _ => return None,
    };
    let dest_sender = match target {
        SftpPanel::LeftRemote => window.sftp_browser_left.as_ref()?.command_sender(),
        SftpPanel::RightRemote => window.sftp_browser.as_ref()?.command_sender(),
        _ => return None,
    };

    let root = std::env::temp_dir()
        .join("portal-sftp-copy")
        .join(format!(
            "{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
    if let Err(e) = std::fs::create_dir_all(&root) {
        return Some(format!("{}: {}", root.display(), e));
    }

    let items: Vec<RelayItem> = entries
        .iter()
        .zip(dest_names)
        .map(|(e, name)| RelayItem {
            src: e.full_path.clone(),
            staging: root.join(name),
            dest: join_remote_path(dest_dir, name),
            is_dir: e.is_dir,
        })
        .collect();
    let dest_dir = dest_dir.to_string();

    let spawned = std::thread::Builder::new()
        .name("sftp-paste-relay".into())
        .spawn(move || {
            for item in items {
                // ── download leg (source connection, into staging) ──
                let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
                let download = if item.is_dir {
                    SftpCommand::DownloadDirSync {
                        remote_dir: item.src.clone(),
                        local_dir: item.staging.to_string_lossy().into_owned(),
                        done: ack_tx,
                    }
                } else {
                    SftpCommand::DownloadSync {
                        remote: item.src.clone(),
                        local: item.staging.to_string_lossy().into_owned(),
                        done: ack_tx,
                    }
                };
                if src_sender.send(download).is_err() {
                    log::error!("paste relay: source connection closed");
                    break;
                }
                match ack_rx.blocking_recv() {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        log::error!("paste relay: download {}: {}", item.src, e);
                        cleanup_staging(&item);
                        continue;
                    }
                    Err(_) => {
                        log::error!("paste relay: download {}: cancelled", item.src);
                        cleanup_staging(&item);
                        continue;
                    }
                }

                // ── upload leg (target connection, from staging) ──
                let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
                let upload = if item.is_dir {
                    SftpCommand::UploadDirSync {
                        local_dir: item.staging.to_string_lossy().into_owned(),
                        remote_dir: item.dest.clone(),
                        done: ack_tx,
                    }
                } else {
                    SftpCommand::UploadSync {
                        local: item.staging.to_string_lossy().into_owned(),
                        remote: item.dest.clone(),
                        done: ack_tx,
                    }
                };
                if dest_sender.send(upload).is_err() {
                    log::error!("paste relay: target connection closed");
                    break;
                }
                match ack_rx.blocking_recv() {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => log::error!("paste relay: upload {}: {}", item.dest, e),
                    Err(_) => log::error!("paste relay: upload {}: cancelled", item.dest),
                }
                cleanup_staging(&item);
            }
            let _ = std::fs::remove_dir_all(&root);
            // Refresh the target listing (TransferComplete refreshes only the
            // browsers' own current dirs after *their* transfers; the relay's
            // uploads land in `dest_dir` which may differ per paste).
            let _ = dest_sender.send(SftpCommand::ListDir(dest_dir));
            crate::repaint::notify_repaint();
        });
    if let Err(e) = spawned {
        return Some(format!("relay thread: {}", e));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // dedupe_name
    // ------------------------------------------------------------------

    #[test]
    fn dedupe_keeps_free_name() {
        let roster: Vec<String> = vec!["a.txt".into(), "b.txt".into()];
        assert_eq!(dedupe_name(&roster, "c.txt", "copy"), "c.txt");
    }

    #[test]
    fn dedupe_appends_suffix_once() {
        let roster: Vec<String> = vec!["a.txt".into()];
        assert_eq!(dedupe_name(&roster, "a.txt", "copy"), "a.txt copy");
    }

    #[test]
    fn dedupe_appends_suffix_then_counter() {
        let roster: Vec<String> = vec!["a.txt".into(), "a.txt copy".into()];
        assert_eq!(dedupe_name(&roster, "a.txt", "copy"), "a.txt copy 2");
        let roster: Vec<String> = vec![
            "a.txt".into(),
            "a.txt copy".into(),
            "a.txt copy 2".into(),
        ];
        assert_eq!(dedupe_name(&roster, "a.txt", "copy"), "a.txt copy 3");
    }

    #[test]
    fn dedupe_uses_localized_suffix() {
        let roster: Vec<String> = vec!["报告".into()];
        assert_eq!(dedupe_name(&roster, "报告", "副本"), "报告 副本");
        let roster: Vec<String> = vec!["报告".into(), "报告 副本".into()];
        assert_eq!(dedupe_name(&roster, "报告", "副本"), "报告 副本 2");
    }

    #[test]
    fn dedupe_dir_names_too() {
        let roster: Vec<String> = vec!["assets".into(), "assets copy".into()];
        assert_eq!(dedupe_name(&roster, "assets", "copy"), "assets copy 2");
    }

    // ------------------------------------------------------------------
    // paste_dest_name
    // ------------------------------------------------------------------

    #[test]
    fn dest_name_renames_only_on_collision() {
        let mut roster = vec!["a".to_string()];
        assert_eq!(paste_dest_name(&mut roster, "a", true, "copy"), "a copy");
        assert_eq!(paste_dest_name(&mut roster, "a", true, "copy"), "a copy 2");
        // Overwrite semantics elsewhere: the name survives untouched.
        assert_eq!(paste_dest_name(&mut roster, "a", false, "copy"), "a");
    }

    #[test]
    fn dest_name_records_assigned_names() {
        // Pasting entries "a" and "a copy" together must not map both to
        // "a copy": the roster grows as names are assigned.
        let mut roster: Vec<String> = vec!["a".into(), "a copy".into()];
        assert_eq!(paste_dest_name(&mut roster, "a", true, "copy"), "a copy 2");
        assert_eq!(paste_dest_name(&mut roster, "a copy", true, "copy"), "a copy copy");
    }

    // ------------------------------------------------------------------
    // parent_dir
    // ------------------------------------------------------------------

    #[test]
    fn parent_dir_of_paths() {
        assert_eq!(parent_dir("/home/user/file.txt"), "/home/user");
        assert_eq!(parent_dir("/file.txt"), "/");
        assert_eq!(parent_dir("/home/user/dir/"), "/home/user");
    }

    #[test]
    fn same_dir_detection_matches_parent_against_target() {
        // The rule execute_paste applies: rename only when the target
        // directory equals the entries' source parent directory.
        let target = "/home/user";
        let entries = ["/home/user/a.txt", "/home/user/sub/"];
        assert!(entries.iter().all(|e| parent_dir(e) == target));
        let other = "/home/other";
        assert!(!entries.iter().all(|e| parent_dir(e) == other));
    }
}
