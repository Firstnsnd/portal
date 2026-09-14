//! Pure planning logic for SFTP drag-and-drop transfers.
//!
//! Everything here is side-effect-free decision making (path joining,
//! transfer planning, local copies for local→local drops) so it can be
//! unit-tested without an SSH server or an egui context.

use std::path::{Path, PathBuf};

/// Join a remote directory path with an entry name, normalizing away
/// trailing slashes so the root directory ("/") never yields "//name".
/// An empty directory is treated as the root.
pub(crate) fn join_remote_path(dir: &str, name: &str) -> String {
    let trimmed = dir.trim_end_matches('/');
    if trimmed.is_empty() {
        format!("/{}", name)
    } else {
        format!("{}/{}", trimmed, name)
    }
}

/// Which half of the dual-panel view a transfer targets. Defined here (not in
/// `ui`) so the sftp crate stays UI-agnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DropSide {
    Left,
    Right,
}

/// One planned transfer. Paths are full destination paths (including the file
/// or directory name), matching `SftpBrowser::{upload,download}` semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlannedTransfer {
    /// Local file → remote panel, via the given side's connection.
    UploadFile { local: PathBuf, remote: String, via: DropSide },
    /// Local directory tree → remote panel.
    UploadDir { local: PathBuf, remote: String, via: DropSide },
    /// Remote file → local panel.
    DownloadFile { remote: String, local: PathBuf, via: DropSide },
    /// Remote directory tree → local panel.
    DownloadDir { remote: String, local: PathBuf, via: DropSide },
    /// Local → local copy (OS drop onto a local panel).
    CopyFile { src: PathBuf, dest: PathBuf },
    /// Local → local recursive copy.
    CopyDir { src: PathBuf, dest: PathBuf },
}

/// Where an OS-level drop (from Finder) landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OsDropTarget {
    /// A remote panel: `dir` is its current remote path, `side` routes the
    /// transfer through that side's connection.
    Remote { side: DropSide, dir: String },
    /// A local panel: `dir` is its current local path.
    Local { dir: PathBuf },
}

/// An ordered list of transfers produced by a planner.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct TransferPlan {
    pub(crate) transfers: Vec<PlannedTransfer>,
}

impl TransferPlan {
    pub(crate) fn is_empty(&self) -> bool {
        self.transfers.is_empty()
    }
}

/// Plan transfers for an OS-level drop (Finder → app). `is_dir` is injected so
/// the planner stays pure; entries without a file name (e.g. `/`) are skipped.
pub(crate) fn plan_os_drop<F>(paths: &[PathBuf], target: &OsDropTarget, is_dir: F) -> TransferPlan
where
    F: Fn(&Path) -> bool,
{
    let mut transfers = Vec::with_capacity(paths.len());
    for path in paths {
        let name = match path.file_name() {
            Some(n) => n.to_string_lossy().into_owned(),
            None => continue,
        };
        match target {
            OsDropTarget::Remote { side, dir } => {
                let remote = join_remote_path(dir, &name);
                if is_dir(path) {
                    transfers.push(PlannedTransfer::UploadDir {
                        local: path.clone(),
                        remote,
                        via: *side,
                    });
                } else {
                    transfers.push(PlannedTransfer::UploadFile {
                        local: path.clone(),
                        remote,
                        via: *side,
                    });
                }
            }
            OsDropTarget::Local { dir } => {
                let dest = dir.join(&name);
                if is_dir(path) {
                    transfers.push(PlannedTransfer::CopyDir { src: path.clone(), dest });
                } else {
                    transfers.push(PlannedTransfer::CopyFile { src: path.clone(), dest });
                }
            }
        }
    }
    TransferPlan { transfers }
}

/// Copy one local file, creating the destination's parent directory if needed.
/// Overwrites an existing destination, matching upload semantics.
pub(crate) fn copy_local_file(src: &Path, dest: &Path) -> std::io::Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(src, dest).map(|_| ())
}

/// Recursively copy a local directory tree. Overwrites colliding files.
pub(crate) fn copy_local_dir(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_local_dir(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // join_remote_path
    // ------------------------------------------------------------------

    #[test]
    fn join_root_dir() {
        assert_eq!(join_remote_path("/", "file.txt"), "/file.txt");
    }

    #[test]
    fn join_plain_dir() {
        assert_eq!(join_remote_path("/home/user", "file.txt"), "/home/user/file.txt");
    }

    #[test]
    fn join_trailing_slash_dir() {
        assert_eq!(join_remote_path("/home/user/", "file.txt"), "/home/user/file.txt");
    }

    #[test]
    fn join_never_produces_double_slash() {
        assert_eq!(join_remote_path("//", "file.txt"), "/file.txt");
        assert_eq!(join_remote_path("///", "file.txt"), "/file.txt");
    }

    #[test]
    fn join_empty_dir_treated_as_root() {
        assert_eq!(join_remote_path("", "file.txt"), "/file.txt");
    }

    // ------------------------------------------------------------------
    // plan_os_drop (Finder → panel)
    // ------------------------------------------------------------------

    #[test]
    fn os_drop_file_to_remote_panel_plans_upload() {
        let paths = vec![PathBuf::from("/tmp/report.pdf")];
        let target = OsDropTarget::Remote { side: DropSide::Right, dir: "/srv/data".to_string() };
        let plan = plan_os_drop(&paths, &target, |_| false);
        assert_eq!(plan.transfers, vec![PlannedTransfer::UploadFile {
            local: PathBuf::from("/tmp/report.pdf"),
            remote: "/srv/data/report.pdf".to_string(),
            via: DropSide::Right,
        }]);
    }

    #[test]
    fn os_drop_dir_to_remote_panel_plans_upload_dir_at_root() {
        let paths = vec![PathBuf::from("/tmp/project")];
        let target = OsDropTarget::Remote { side: DropSide::Left, dir: "/".to_string() };
        let plan = plan_os_drop(&paths, &target, |p| p.ends_with("project"));
        assert_eq!(plan.transfers, vec![PlannedTransfer::UploadDir {
            local: PathBuf::from("/tmp/project"),
            remote: "/project".to_string(),
            via: DropSide::Left,
        }]);
    }

    #[test]
    fn os_drop_to_local_panel_plans_copy() {
        let paths = vec![PathBuf::from("/tmp/a.txt"), PathBuf::from("/tmp/subdir")];
        let target = OsDropTarget::Local { dir: PathBuf::from("/Users/x/dest") };
        let plan = plan_os_drop(&paths, &target, |p| p.ends_with("subdir"));
        assert_eq!(plan.transfers, vec![
            PlannedTransfer::CopyFile {
                src: PathBuf::from("/tmp/a.txt"),
                dest: PathBuf::from("/Users/x/dest/a.txt"),
            },
            PlannedTransfer::CopyDir {
                src: PathBuf::from("/tmp/subdir"),
                dest: PathBuf::from("/Users/x/dest/subdir"),
            },
        ]);
    }

    #[test]
    fn os_drop_preserves_drop_order() {
        let paths = vec![
            PathBuf::from("/tmp/z.txt"),
            PathBuf::from("/tmp/a.txt"),
            PathBuf::from("/tmp/m.txt"),
        ];
        let target = OsDropTarget::Remote { side: DropSide::Right, dir: "/srv".to_string() };
        let plan = plan_os_drop(&paths, &target, |_| false);
        let names: Vec<String> = plan.transfers.iter().map(|t| match t {
            PlannedTransfer::UploadFile { remote, .. } => remote.clone(),
            _ => String::new(),
        }).collect();
        assert_eq!(names, vec!["/srv/z.txt", "/srv/a.txt", "/srv/m.txt"]);
    }

    #[test]
    fn os_drop_empty_paths_yield_empty_plan() {
        let target = OsDropTarget::Remote { side: DropSide::Right, dir: "/srv".to_string() };
        let plan = plan_os_drop(&[], &target, |_| false);
        assert!(plan.is_empty());
    }

    // ------------------------------------------------------------------
    // copy_local_file / copy_local_dir (local panel targets)
    // ------------------------------------------------------------------

    #[test]
    fn copy_local_file_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src.txt");
        std::fs::write(&src, b"hello portal").unwrap();
        let dest = tmp.path().join("dest").join("src.txt");
        copy_local_file(&src, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"hello portal");
    }

    #[test]
    fn copy_local_file_overwrites_existing_dest() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src.txt");
        std::fs::write(&src, b"new").unwrap();
        let dest = tmp.path().join("dest").join("src.txt");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(&dest, b"old content").unwrap();
        copy_local_file(&src, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"new");
    }

    #[test]
    fn copy_local_dir_copies_nested_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("tree");
        std::fs::create_dir_all(src.join("a/b")).unwrap();
        std::fs::write(src.join("root.txt"), b"root").unwrap();
        std::fs::write(src.join("a/mid.txt"), b"mid").unwrap();
        std::fs::write(src.join("a/b/leaf.txt"), b"leaf").unwrap();

        let dest = tmp.path().join("dest").join("tree");
        copy_local_dir(&src, &dest).unwrap();

        assert_eq!(std::fs::read(dest.join("root.txt")).unwrap(), b"root");
        assert_eq!(std::fs::read(dest.join("a/mid.txt")).unwrap(), b"mid");
        assert_eq!(std::fs::read(dest.join("a/b/leaf.txt")).unwrap(), b"leaf");
    }

    #[test]
    fn copy_local_dir_overwrites_files_in_existing_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("tree");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("f.txt"), b"fresh").unwrap();

        let dest = tmp.path().join("dest").join("tree");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("f.txt"), b"stale").unwrap();

        copy_local_dir(&src, &dest).unwrap();
        assert_eq!(std::fs::read(dest.join("f.txt")).unwrap(), b"fresh");
    }
}
