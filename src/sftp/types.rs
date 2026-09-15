//! # SFTP Types
//!
//! Common data types for SFTP operations.

use crate::ui::formatting::format_duration_hms_from_secs;

/// File entry returned from a directory listing
#[derive(Clone, Debug)]
pub struct SftpEntry {
    pub name: String,
    pub kind: SftpEntryKind,
    pub size: Option<u64>,
    pub permissions: Option<u32>,
    /// Last-modified time. Local gets it from `std::fs::Metadata::modified()`;
    /// remote from SFTP `mtime`. `None` when unavailable.
    pub modified: Option<std::time::SystemTime>,
}

/// Kind of a remote filesystem entry
#[derive(Clone, Debug, PartialEq)]
pub enum SftpEntryKind {
    File,
    Directory,
    Symlink,
    Other,
}

/// SFTP connection state
#[derive(Debug, Clone)]
pub enum SftpConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

/// File transfer progress
#[derive(Clone, Debug)]
pub struct TransferProgress {
    pub filename: String,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub is_upload: bool,
    pub started_at: std::time::Instant,
}

impl TransferProgress {
    /// An immediate "transfer is starting" placeholder, seeded the moment a
    /// transfer is kicked off so the progress bar renders at once (0% with a
    /// fresh clock) instead of remaining blank until the first real
    /// `Progress` message arrives from the SFTP task.
    pub fn preparing(filename: impl Into<String>, is_upload: bool) -> Self {
        TransferProgress {
            filename: filename.into(),
            bytes_transferred: 0,
            total_bytes: 0,
            is_upload,
            started_at: std::time::Instant::now(),
        }
    }

    /// Bytes per second based on elapsed time.
    pub fn speed_bps(&self) -> f64 {
        let elapsed = self.started_at.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            (self.bytes_transferred as f64) / elapsed
        } else {
            0.0
        }
    }

    /// Estimated time remaining as "HH:MM:SS" or "MM:SS", or None if unknown.
    pub fn eta_string(&self) -> Option<String> {
        let elapsed = self.started_at.elapsed().as_secs_f64();
        if elapsed > 0.0 && self.bytes_transferred > 0 {
            let bps = (self.bytes_transferred as f64) / elapsed;
            if bps > 0.0 {
                let remaining_bytes = self.total_bytes.saturating_sub(self.bytes_transferred);
                let remaining_secs = (remaining_bytes as f64) / bps;
                return Some(format_duration_hms_from_secs(remaining_secs as u64));
            }
        }
        None
    }

    /// Estimated time remaining in seconds, or None if unknown.
    #[allow(dead_code)]
    pub fn eta_secs(&self) -> Option<f64> {
        let elapsed = self.started_at.elapsed().as_secs_f64();
        if elapsed > 0.0 && self.bytes_transferred > 0 {
            let bps = (self.bytes_transferred as f64) / elapsed;
            if bps > 0.0 {
                let remaining_bytes = self.total_bytes.saturating_sub(self.bytes_transferred);
                return Some((remaining_bytes as f64) / bps);
            }
        }
        None
    }
}

/// Commands sent from the GUI thread to the async SFTP task
pub enum SftpCommand {
    ListDir(String),
    Download { remote: String, local: String },
    Upload { local: String, remote: String },
    UploadDir { local_dir: String, remote_dir: String },
    DownloadDir { remote_dir: String, local_dir: String },
    /// Like `Download`, but the task acks the result through `done`. Used by
    /// the macOS file-promise drag-out, whose delegate must block until the
    /// file exists locally.
    DownloadSync {
        remote: String,
        local: String,
        done: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    /// Like `DownloadDir`, with a completion ack through `done`.
    DownloadDirSync {
        remote_dir: String,
        local_dir: String,
        done: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    Rename { from: String, to: String },
    Delete(String),
    CreateDir(String),
    ReadFile { path: String },
    WriteFile { path: String, data: Vec<u8> },
    Disconnect,
}

/// Responses from the async SFTP task to the GUI thread
pub enum SftpResponse {
    DirListing {
        path: String,
        entries: Vec<SftpEntry>,
    },
    Progress(TransferProgress),
    #[allow(dead_code)]
    TransferComplete {
        filename: String,
        is_upload: bool,
    },
    OperationComplete,
    FileContent { path: String, data: Vec<u8> },
    FileTooLarge { path: String, size: u64 },
    Error(String),
    Disconnected,
}
