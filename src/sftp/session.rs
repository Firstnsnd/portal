//! SFTP session management: async backend + browser state

use std::collections::BTreeSet;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::config::AuthMethod;
use crate::ssh::connect_and_authenticate;

// ─── Multi-selection state ────────────────────────────────────────────────

/// Tracks multi-selection state for file browser panels.
#[derive(Clone, Debug, Default)]
pub struct FileSelection {
    pub selected: BTreeSet<usize>,
    pub anchor: Option<usize>,
    pub focus: Option<usize>,
}

impl FileSelection {
    pub fn clear(&mut self) {
        self.selected.clear();
        self.anchor = None;
        self.focus = None;
    }

    /// Select exactly one item (plain click).
    pub fn select_one(&mut self, i: usize) {
        self.selected.clear();
        self.selected.insert(i);
        self.anchor = Some(i);
        self.focus = Some(i);
    }

    /// Toggle a single item (Cmd/Ctrl+Click).
    pub fn toggle(&mut self, i: usize) {
        if self.selected.contains(&i) {
            self.selected.remove(&i);
        } else {
            self.selected.insert(i);
        }
        self.anchor = Some(i);
        self.focus = Some(i);
    }

    /// Select range from anchor to i (Shift+Click).
    pub fn select_range(&mut self, i: usize) {
        let anchor = self.anchor.unwrap_or(0);
        let (lo, hi) = if anchor <= i { (anchor, i) } else { (i, anchor) };
        self.selected.clear();
        for idx in lo..=hi {
            self.selected.insert(idx);
        }
        self.focus = Some(i);
    }

    /// Extend selection from anchor to i (Shift+Arrow key), keeping anchor.
    pub fn extend_to(&mut self, i: usize) {
        let anchor = self.anchor.unwrap_or(0);
        let (lo, hi) = if anchor <= i { (anchor, i) } else { (i, anchor) };
        self.selected.clear();
        for idx in lo..=hi {
            self.selected.insert(idx);
        }
        self.focus = Some(i);
    }

    /// Select all items in range 0..count.
    pub fn select_all(&mut self, count: usize) {
        self.selected.clear();
        for i in 0..count {
            self.selected.insert(i);
        }
        if count > 0 {
            if self.anchor.is_none() {
                self.anchor = Some(0);
            }
            if self.focus.is_none() {
                self.focus = Some(0);
            }
        }
    }

    pub fn is_selected(&self, i: usize) -> bool {
        self.selected.contains(&i)
    }

    pub fn count(&self) -> usize {
        self.selected.len()
    }

    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }
}

// ─── Local file browser ───────────────────────────────────────────────────

/// Synchronous local filesystem browser (left panel).
pub struct LocalBrowser {
    pub current_path: String,
    pub entries: Vec<SftpEntry>,
    pub selection: FileSelection,
    pub path_input: String,
}

impl LocalBrowser {
    pub fn new() -> Self {
        let home = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("/"))
            .to_string_lossy()
            .to_string();
        let mut browser = Self {
            current_path: home.clone(),
            entries: Vec::new(),
            selection: FileSelection::default(),
            path_input: home,
        };
        browser.refresh();
        browser
    }

    /// Re-read the current directory.
    pub fn refresh(&mut self) {
        let mut entries = Vec::new();
        if let Ok(read_dir) = std::fs::read_dir(&self.current_path) {
            for dir_entry in read_dir.flatten() {
                let name = dir_entry.file_name().to_string_lossy().to_string();
                let meta = dir_entry.metadata().ok();
                let kind = match meta.as_ref().map(|m| m.file_type()) {
                    Some(ft) if ft.is_dir() => SftpEntryKind::Directory,
                    Some(ft) if ft.is_symlink() => SftpEntryKind::Symlink,
                    Some(ft) if ft.is_file() => SftpEntryKind::File,
                    _ => SftpEntryKind::Other,
                };
                let size = meta.as_ref().map(|m| m.len());
                #[cfg(unix)]
                let permissions = {
                    use std::os::unix::fs::PermissionsExt;
                    meta.as_ref().map(|m| m.permissions().mode())
                };
                #[cfg(not(unix))]
                let permissions = None;
                entries.push(SftpEntry {
                    name,
                    kind,
                    size,
                    permissions,
                });
            }
        }
        // Sort: directories first, then alphabetical
        entries.sort_by(|a, b| {
            let a_dir = a.kind == SftpEntryKind::Directory;
            let b_dir = b.kind == SftpEntryKind::Directory;
            b_dir
                .cmp(&a_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        self.entries = entries;
        self.selection.clear();
    }

    /// Navigate to an absolute path.
    pub fn navigate(&mut self, path: &str) {
        let p = std::path::Path::new(path);
        if p.is_dir() {
            self.current_path = p.to_string_lossy().to_string();
            self.path_input = self.current_path.clone();
            self.refresh();
        }
    }

    /// Navigate to the parent directory.
    pub fn navigate_up(&mut self) {
        if let Some(parent) = std::path::Path::new(&self.current_path).parent() {
            let p = parent.to_string_lossy().to_string();
            self.navigate(&p);
        }
    }

    /// Rename a local file or directory.
    pub fn rename(&mut self, old_name: &str, new_name: &str) -> Result<(), String> {
        let old_path = format!("{}/{}", self.current_path.trim_end_matches('/'), old_name);
        let new_path = format!("{}/{}", self.current_path.trim_end_matches('/'), new_name);
        std::fs::rename(&old_path, &new_path)
            .map_err(|e| format!("Rename failed: {}", e))?;
        self.refresh();
        Ok(())
    }

    /// Delete a local file or directory.
    pub fn delete(&mut self, name: &str) -> Result<(), String> {
        let path = format!("{}/{}", self.current_path.trim_end_matches('/'), name);
        let p = std::path::Path::new(&path);
        if p.is_dir() {
            std::fs::remove_dir_all(&path)
                .map_err(|e| format!("Delete failed: {}", e))?;
        } else {
            std::fs::remove_file(&path)
                .map_err(|e| format!("Delete failed: {}", e))?;
        }
        self.refresh();
        Ok(())
    }

    /// Create a new directory inside the current path.
    pub fn create_dir(&mut self, name: &str) -> Result<(), String> {
        let path = format!("{}/{}", self.current_path.trim_end_matches('/'), name);
        std::fs::create_dir(&path)
            .map_err(|e| format!("Create dir failed: {}", e))?;
        self.refresh();
        Ok(())
    }

    /// Read a local file for the editor (≤10MB, UTF-8).
    pub fn read_file(&self, name: &str) -> Result<String, String> {
        let path = format!("{}/{}", self.current_path.trim_end_matches('/'), name);
        let meta = std::fs::metadata(&path)
            .map_err(|e| format!("Cannot stat file: {}", e))?;
        if meta.len() > 10 * 1024 * 1024 {
            return Err(format!("File too large: {} bytes (max 10 MB)", meta.len()));
        }
        let data = std::fs::read(&path)
            .map_err(|e| format!("Cannot read file: {}", e))?;
        String::from_utf8(data)
            .map_err(|_| "Not valid UTF-8".to_string())
    }

    /// Write content to a local file from the editor.
    pub fn write_file(&mut self, name: &str, content: &str) -> Result<(), String> {
        let path = format!("{}/{}", self.current_path.trim_end_matches('/'), name);
        std::fs::write(&path, content.as_bytes())
            .map_err(|e| format!("Cannot write file: {}", e))?;
        self.refresh();
        Ok(())
    }
}

/// File entry returned from a directory listing
#[derive(Clone, Debug)]
pub struct SftpEntry {
    pub name: String,
    pub kind: SftpEntryKind,
    pub size: Option<u64>,
    pub permissions: Option<u32>,
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
    /// Bytes per second based on elapsed time.
    pub fn speed_bps(&self) -> f64 {
        let elapsed = self.started_at.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            self.bytes_transferred as f64 / elapsed
        } else {
            0.0
        }
    }
}

/// Commands sent from the GUI thread to the async SFTP task
enum SftpCommand {
    ListDir(String),
    Download { remote: String, local: String },
    Upload { local: String, remote: String },
    UploadDir { local_dir: String, remote_dir: String },
    DownloadDir { remote_dir: String, local_dir: String },
    Rename { from: String, to: String },
    Delete(String),
    CreateDir(String),
    ReadFile { path: String },
    WriteFile { path: String, data: Vec<u8> },
    Disconnect,
}

/// Responses from the async SFTP task to the GUI thread
enum SftpResponse {
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

/// SFTP file browser state, driven from the GUI
pub struct SftpBrowser {
    cmd_tx: mpsc::UnboundedSender<SftpCommand>,
    resp_rx: mpsc::UnboundedReceiver<SftpResponse>,
    pub state: SftpConnectionState,
    pub current_path: String,
    pub entries: Vec<SftpEntry>,
    pub transfer: Option<TransferProgress>,
    pub host_name: String,
    pub selection: FileSelection,
    pub path_input: String,
    cancel_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub pending_file_content: Option<(String, Vec<u8>)>,
    pub pending_file_too_large: Option<(String, u64)>,
}

impl SftpBrowser {
    /// Spawn a new SFTP connection task and return the browser handle.
    pub fn connect(
        runtime: &tokio::runtime::Runtime,
        host: String,
        port: u16,
        username: String,
        auth: AuthMethod,
        host_name: String,
    ) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (resp_tx, resp_rx) = mpsc::unbounded_channel();
        let cancel_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        let cancel_clone = cancel_flag.clone();
        runtime.spawn(sftp_task(host, port, username, auth, host_name.clone(), cmd_rx, resp_tx, cancel_clone));

        Self {
            cmd_tx,
            resp_rx,
            state: SftpConnectionState::Connecting,
            current_path: String::new(),
            entries: Vec::new(),
            transfer: None,
            host_name,
            selection: FileSelection::default(),
            path_input: String::new(),
            cancel_flag,
            pending_file_content: None,
            pending_file_too_large: None,
        }
    }

    /// Poll for responses from the async task. Call once per frame.
    pub fn poll(&mut self) {
        while let Ok(resp) = self.resp_rx.try_recv() {
            match resp {
                SftpResponse::DirListing { path, entries } => {
                    self.current_path = path.clone();
                    self.path_input = path;
                    self.entries = entries;
                    self.selection.clear();
                    self.state = SftpConnectionState::Connected;
                }
                SftpResponse::Progress(p) => {
                    self.transfer = Some(p);
                }
                SftpResponse::TransferComplete { .. } => {
                    self.transfer = None;
                    // Refresh current directory after transfer
                    let _ = self.cmd_tx.send(SftpCommand::ListDir(self.current_path.clone()));
                }
                SftpResponse::OperationComplete => {
                    // Refresh current directory after rename/delete/mkdir
                    let _ = self.cmd_tx.send(SftpCommand::ListDir(self.current_path.clone()));
                }
                SftpResponse::FileContent { path, data } => {
                    self.pending_file_content = Some((path, data));
                }
                SftpResponse::FileTooLarge { path, size } => {
                    self.pending_file_too_large = Some((path, size));
                }
                SftpResponse::Error(e) => {
                    if matches!(self.state, SftpConnectionState::Connecting) {
                        self.state = SftpConnectionState::Error(e);
                    } else {
                        // Non-fatal error during operation: keep connected, clear transfer
                        self.transfer = None;
                        log::error!("SFTP error: {}", e);
                    }
                }
                SftpResponse::Disconnected => {
                    self.state = SftpConnectionState::Disconnected;
                }
            }
        }
    }

    /// Navigate to a directory path.
    pub fn navigate(&self, path: &str) {
        let _ = self.cmd_tx.send(SftpCommand::ListDir(path.to_string()));
    }

    /// Refresh the current directory.
    pub fn refresh(&self) {
        self.navigate(&self.current_path);
    }

    /// Navigate to the parent directory.
    pub fn navigate_up(&self) {
        let parent = if self.current_path == "/" {
            "/".to_string()
        } else {
            let trimmed = self.current_path.trim_end_matches('/');
            match trimmed.rfind('/') {
                Some(0) => "/".to_string(),
                Some(pos) => trimmed[..pos].to_string(),
                None => "/".to_string(),
            }
        };
        self.navigate(&parent);
    }

    /// Download a remote file to a local path.
    pub fn download(&self, remote_path: &str, local_path: &str) {
        let _ = self.cmd_tx.send(SftpCommand::Download {
            remote: remote_path.to_string(),
            local: local_path.to_string(),
        });
    }

    /// Upload a local file to a remote path.
    pub fn upload(&self, local_path: &str, remote_path: &str) {
        let _ = self.cmd_tx.send(SftpCommand::Upload {
            local: local_path.to_string(),
            remote: remote_path.to_string(),
        });
    }

    /// Signal the async task to stop during connection setup.
    pub fn cancel_connect(&self) {
        self.cancel_flag.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Disconnect the SFTP session.
    pub fn disconnect(&self) {
        let _ = self.cmd_tx.send(SftpCommand::Disconnect);
    }

    /// Upload an entire local directory recursively.
    pub fn upload_dir(&self, local_dir: &str, remote_dir: &str) {
        let _ = self.cmd_tx.send(SftpCommand::UploadDir {
            local_dir: local_dir.to_string(),
            remote_dir: remote_dir.to_string(),
        });
    }

    /// Download an entire remote directory recursively.
    pub fn download_dir(&self, remote_dir: &str, local_dir: &str) {
        let _ = self.cmd_tx.send(SftpCommand::DownloadDir {
            remote_dir: remote_dir.to_string(),
            local_dir: local_dir.to_string(),
        });
    }

    /// Rename a remote file or directory.
    pub fn rename(&self, from: &str, to: &str) {
        let _ = self.cmd_tx.send(SftpCommand::Rename {
            from: from.to_string(),
            to: to.to_string(),
        });
    }

    /// Delete a remote file or directory.
    pub fn delete(&self, path: &str) {
        let _ = self.cmd_tx.send(SftpCommand::Delete(path.to_string()));
    }

    /// Create a remote directory.
    pub fn create_dir(&self, path: &str) {
        let _ = self.cmd_tx.send(SftpCommand::CreateDir(path.to_string()));
    }

    /// Request to read a remote file for the editor.
    pub fn read_file(&self, path: &str) {
        let _ = self.cmd_tx.send(SftpCommand::ReadFile { path: path.to_string() });
    }

    /// Write content to a remote file from the editor.
    pub fn write_file(&self, path: &str, data: Vec<u8>) {
        let _ = self.cmd_tx.send(SftpCommand::WriteFile { path: path.to_string(), data });
    }

    /// Cancel the current transfer in progress.
    pub fn cancel_transfer(&mut self) {
        self.cancel_flag.store(true, std::sync::atomic::Ordering::Relaxed);
        self.transfer = None;
    }
}

impl Drop for SftpBrowser {
    fn drop(&mut self) {
        self.disconnect();
    }
}

/// The async SFTP task running on tokio.
async fn sftp_task(
    host: String,
    port: u16,
    username: String,
    auth: AuthMethod,
    display_name: String,
    mut cmd_rx: mpsc::UnboundedReceiver<SftpCommand>,
    resp_tx: mpsc::UnboundedSender<SftpResponse>,
    cancel_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

    // Helper: check if the user cancelled
    let cancelled = || cancel_flag.load(std::sync::atomic::Ordering::Relaxed);

    // 1. Connect + authenticate (with timeout)
    let handle = match tokio::time::timeout(
        CONNECT_TIMEOUT,
        connect_and_authenticate(&host, port, &username, &auth, &display_name),
    ).await {
        Ok(Ok(h)) => h,
        Ok(Err(e)) => {
            if !cancelled() { let _ = resp_tx.send(SftpResponse::Error(e)); }
            return;
        }
        Err(_) => {
            if !cancelled() { let _ = resp_tx.send(SftpResponse::Error("Connection timed out".to_string())); }
            return;
        }
    };
    if cancelled() { return; }

    // 2. Open a session channel and request SFTP subsystem
    let channel = match handle.channel_open_session().await {
        Ok(ch) => ch,
        Err(e) => {
            if !cancelled() { let _ = resp_tx.send(SftpResponse::Error(format!("Channel open failed: {}", e))); }
            return;
        }
    };
    if cancelled() { return; }

    if let Err(e) = channel.request_subsystem(true, "sftp").await {
        if !cancelled() {
            let _ = resp_tx.send(SftpResponse::Error(format!(
                "SFTP subsystem request failed: {}",
                e
            )));
        }
        return;
    }
    if cancelled() { return; }

    // 3. Create SFTP session from channel stream
    let sftp = match russh_sftp::client::SftpSession::new(channel.into_stream()).await {
        Ok(s) => s,
        Err(e) => {
            if !cancelled() {
                let _ = resp_tx.send(SftpResponse::Error(format!(
                    "SFTP session init failed: {}",
                    e
                )));
            }
            return;
        }
    };
    if cancelled() { return; }

    // 4. Resolve home directory as initial path
    let home = sftp.canonicalize(".").await.unwrap_or_else(|_| "/".into());

    // List the initial directory
    if let Err(e) = list_dir(&sftp, &home, &resp_tx).await {
        if !cancelled() { let _ = resp_tx.send(SftpResponse::Error(e)); }
        return;
    }

    // 5. Command loop
    loop {
        match cmd_rx.recv().await {
            Some(SftpCommand::ListDir(path)) => {
                let canonical = sftp.canonicalize(&path).await.unwrap_or(path);
                if let Err(e) = list_dir(&sftp, &canonical, &resp_tx).await {
                    let _ = resp_tx.send(SftpResponse::Error(e));
                }
            }
            Some(SftpCommand::Download { remote, local }) => {
                cancel_flag.store(false, std::sync::atomic::Ordering::Relaxed);
                if let Err(e) = download_file(&sftp, &remote, &local, &resp_tx, &cancel_flag).await {
                    if !cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
                        let _ = resp_tx.send(SftpResponse::Error(e));
                    }
                }
                let _ = resp_tx.send(SftpResponse::TransferComplete {
                    filename: remote.rsplit('/').next().unwrap_or(&remote).to_string(),
                    is_upload: false,
                });
            }
            Some(SftpCommand::Upload { local, remote }) => {
                cancel_flag.store(false, std::sync::atomic::Ordering::Relaxed);
                if let Err(e) = upload_file(&sftp, &local, &remote, &resp_tx, &cancel_flag).await {
                    if !cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
                        let _ = resp_tx.send(SftpResponse::Error(e));
                    }
                }
                let _ = resp_tx.send(SftpResponse::TransferComplete {
                    filename: local.rsplit('/').next().unwrap_or(&local).to_string(),
                    is_upload: true,
                });
            }
            Some(SftpCommand::UploadDir { local_dir, remote_dir }) => {
                cancel_flag.store(false, std::sync::atomic::Ordering::Relaxed);
                if let Err(e) = upload_dir(&sftp, &local_dir, &remote_dir, &resp_tx, &cancel_flag).await {
                    if !cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
                        let _ = resp_tx.send(SftpResponse::Error(e));
                    }
                }
                let _ = resp_tx.send(SftpResponse::TransferComplete {
                    filename: local_dir.rsplit('/').next().unwrap_or(&local_dir).to_string(),
                    is_upload: true,
                });
            }
            Some(SftpCommand::DownloadDir { remote_dir, local_dir }) => {
                cancel_flag.store(false, std::sync::atomic::Ordering::Relaxed);
                if let Err(e) = download_dir(&sftp, &remote_dir, &local_dir, &resp_tx, &cancel_flag).await {
                    if !cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
                        let _ = resp_tx.send(SftpResponse::Error(e));
                    }
                }
                let _ = resp_tx.send(SftpResponse::TransferComplete {
                    filename: remote_dir.rsplit('/').next().unwrap_or(&remote_dir).to_string(),
                    is_upload: false,
                });
            }
            Some(SftpCommand::Rename { from, to }) => {
                match sftp.rename(&from, &to).await {
                    Ok(_) => { let _ = resp_tx.send(SftpResponse::OperationComplete); }
                    Err(e) => { let _ = resp_tx.send(SftpResponse::Error(format!("Rename failed: {}", e))); }
                }
            }
            Some(SftpCommand::Delete(path)) => {
                match sftp.metadata(&path).await {
                    Ok(meta) => {
                        let is_dir = meta.permissions.map_or(false, |p| (p & 0o170000) == 0o040000);
                        let result = if is_dir {
                            remove_dir_recursive(&sftp, &path).await
                        } else {
                            sftp.remove_file(&path).await.map_err(|e| format!("Delete failed: {}", e))
                        };
                        match result {
                            Ok(_) => { let _ = resp_tx.send(SftpResponse::OperationComplete); }
                            Err(e) => { let _ = resp_tx.send(SftpResponse::Error(e)); }
                        }
                    }
                    Err(e) => { let _ = resp_tx.send(SftpResponse::Error(format!("Cannot stat: {}", e))); }
                }
            }
            Some(SftpCommand::CreateDir(path)) => {
                match sftp.create_dir(&path).await {
                    Ok(_) => { let _ = resp_tx.send(SftpResponse::OperationComplete); }
                    Err(e) => { let _ = resp_tx.send(SftpResponse::Error(format!("Create dir failed: {}", e))); }
                }
            }
            Some(SftpCommand::ReadFile { path }) => {
                match read_file_content(&sftp, &path).await {
                    Ok(data) => { let _ = resp_tx.send(SftpResponse::FileContent { path, data }); }
                    Err(e) if e.starts_with("TOO_LARGE:") => {
                        let size: u64 = e.trim_start_matches("TOO_LARGE:").parse().unwrap_or(0);
                        let _ = resp_tx.send(SftpResponse::FileTooLarge { path, size });
                    }
                    Err(e) => { let _ = resp_tx.send(SftpResponse::Error(e)); }
                }
            }
            Some(SftpCommand::WriteFile { path, data }) => {
                match write_file_content(&sftp, &path, &data).await {
                    Ok(_) => { let _ = resp_tx.send(SftpResponse::OperationComplete); }
                    Err(e) => { let _ = resp_tx.send(SftpResponse::Error(e)); }
                }
            }
            Some(SftpCommand::Disconnect) | None => {
                let _ = sftp.close().await;
                let _ = resp_tx.send(SftpResponse::Disconnected);
                break;
            }
        }
    }
}

/// List a remote directory and send the result.
async fn list_dir(
    sftp: &russh_sftp::client::SftpSession,
    path: &str,
    resp_tx: &mpsc::UnboundedSender<SftpResponse>,
) -> Result<(), String> {
    let read_dir = sftp
        .read_dir(path)
        .await
        .map_err(|e| format!("Failed to list directory: {}", e))?;

    let mut entries: Vec<SftpEntry> = read_dir
        .map(|entry| {
            let meta = entry.metadata();
            let ft = entry.file_type();
            let kind = match ft {
                russh_sftp::protocol::FileType::Dir => SftpEntryKind::Directory,
                russh_sftp::protocol::FileType::Symlink => SftpEntryKind::Symlink,
                russh_sftp::protocol::FileType::File => SftpEntryKind::File,
                russh_sftp::protocol::FileType::Other => SftpEntryKind::Other,
            };
            SftpEntry {
                name: entry.file_name(),
                kind,
                size: meta.size,
                permissions: meta.permissions,
            }
        })
        .collect();

    // Sort: directories first (alphabetical), then files (alphabetical)
    entries.sort_by(|a, b| {
        let a_dir = a.kind == SftpEntryKind::Directory;
        let b_dir = b.kind == SftpEntryKind::Directory;
        b_dir
            .cmp(&a_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    let _ = resp_tx.send(SftpResponse::DirListing {
        path: path.to_string(),
        entries,
    });
    Ok(())
}

/// Recursively remove a remote directory.
async fn remove_dir_recursive(
    sftp: &russh_sftp::client::SftpSession,
    path: &str,
) -> Result<(), String> {
    let entries = sftp
        .read_dir(path)
        .await
        .map_err(|e| format!("Cannot read dir for delete: {}", e))?;

    for entry in entries {
        let name = entry.file_name();
        if name == "." || name == ".." {
            continue;
        }
        let child = format!("{}/{}", path.trim_end_matches('/'), name);
        let ft = entry.file_type();
        if ft == russh_sftp::protocol::FileType::Dir {
            Box::pin(remove_dir_recursive(sftp, &child)).await?;
        } else {
            sftp.remove_file(&child)
                .await
                .map_err(|e| format!("Delete file failed: {}", e))?;
        }
    }

    sftp.remove_dir(path)
        .await
        .map_err(|e| format!("Remove dir failed: {}", e))?;
    Ok(())
}

/// Download a remote file to local disk with progress reporting.
async fn download_file(
    sftp: &russh_sftp::client::SftpSession,
    remote_path: &str,
    local_path: &str,
    resp_tx: &mpsc::UnboundedSender<SftpResponse>,
    cancel: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    let filename = remote_path
        .rsplit('/')
        .next()
        .unwrap_or(remote_path)
        .to_string();

    let meta = sftp
        .metadata(remote_path)
        .await
        .map_err(|e| format!("Cannot stat file: {}", e))?;
    let total_bytes = meta.size.unwrap_or(0);

    let mut remote_file = sftp
        .open(remote_path)
        .await
        .map_err(|e| format!("Cannot open remote file: {}", e))?;

    let mut local_file = tokio::fs::File::create(local_path)
        .await
        .map_err(|e| format!("Cannot create local file: {}", e))?;

    let started_at = std::time::Instant::now();
    let mut bytes_transferred: u64 = 0;
    let chunk_size = 32768;
    let mut buf = vec![0u8; chunk_size];

    loop {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("Cancelled".to_string());
        }
        let n = remote_file
            .read(&mut buf)
            .await
            .map_err(|e| format!("Read error: {}", e))?;
        if n == 0 {
            break;
        }
        local_file
            .write_all(&buf[..n])
            .await
            .map_err(|e| format!("Write error: {}", e))?;
        bytes_transferred += n as u64;

        let _ = resp_tx.send(SftpResponse::Progress(TransferProgress {
            filename: filename.clone(),
            bytes_transferred,
            total_bytes,
            is_upload: false,
            started_at,
        }));
    }

    Ok(())
}

/// Calculate total size of a local directory recursively.
fn local_dir_total_size(path: &str) -> u64 {
    let mut total: u64 = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(ft) = entry.file_type() {
                if ft.is_file() {
                    total += entry.metadata().map(|m| m.len()).unwrap_or(0);
                } else if ft.is_dir() {
                    total += local_dir_total_size(&entry.path().to_string_lossy());
                }
            }
        }
    }
    total
}

/// Calculate total size of a remote directory recursively.
async fn remote_dir_total_size(
    sftp: &russh_sftp::client::SftpSession,
    remote_dir: &str,
) -> u64 {
    let mut total: u64 = 0;
    if let Ok(entries) = sftp.read_dir(remote_dir).await {
        for entry in entries {
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let path = format!("{}/{}", remote_dir.trim_end_matches('/'), name);
            let ft = entry.file_type();
            if ft == russh_sftp::protocol::FileType::File {
                total += entry.metadata().size.unwrap_or(0);
            } else if ft == russh_sftp::protocol::FileType::Dir {
                total += Box::pin(remote_dir_total_size(sftp, &path)).await;
            }
        }
    }
    total
}

/// Recursively upload a local directory to a remote path.
async fn upload_dir(
    sftp: &russh_sftp::client::SftpSession,
    local_dir: &str,
    remote_dir: &str,
    resp_tx: &mpsc::UnboundedSender<SftpResponse>,
    cancel: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    let dir_name = std::path::Path::new(local_dir)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(local_dir)
        .to_string();
    let total_bytes = local_dir_total_size(local_dir);
    let bytes_so_far = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let started_at = std::time::Instant::now();

    let _ = resp_tx.send(SftpResponse::Progress(TransferProgress {
        filename: dir_name.clone(),
        bytes_transferred: 0,
        total_bytes,
        is_upload: true,
        started_at,
    }));

    upload_dir_inner(sftp, local_dir, remote_dir, resp_tx, &dir_name, total_bytes, &bytes_so_far, started_at, cancel).await
}

async fn upload_dir_inner(
    sftp: &russh_sftp::client::SftpSession,
    local_dir: &str,
    remote_dir: &str,
    resp_tx: &mpsc::UnboundedSender<SftpResponse>,
    dir_name: &str,
    total_bytes: u64,
    bytes_so_far: &std::sync::Arc<std::sync::atomic::AtomicU64>,
    started_at: std::time::Instant,
    cancel: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("Cancelled".to_string());
    }
    let _ = sftp.create_dir(remote_dir).await;

    let read_dir = std::fs::read_dir(local_dir)
        .map_err(|e| format!("Cannot read local dir {}: {}", local_dir, e))?;

    for entry in read_dir.flatten() {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("Cancelled".to_string());
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let local_path = format!("{}/{}", local_dir.trim_end_matches('/'), name);
        let remote_path = format!("{}/{}", remote_dir.trim_end_matches('/'), name);

        let ft = entry
            .file_type()
            .map_err(|e| format!("Cannot get file type: {}", e))?;

        if ft.is_dir() {
            Box::pin(upload_dir_inner(sftp, &local_path, &remote_path, resp_tx, dir_name, total_bytes, bytes_so_far, started_at, cancel)).await?;
        } else if ft.is_file() {
            upload_file_for_dir(sftp, &local_path, &remote_path, resp_tx, dir_name, total_bytes, bytes_so_far, started_at, cancel).await?;
        }
    }
    Ok(())
}

async fn upload_file_for_dir(
    sftp: &russh_sftp::client::SftpSession,
    local_path: &str,
    remote_path: &str,
    resp_tx: &mpsc::UnboundedSender<SftpResponse>,
    dir_name: &str,
    total_bytes: u64,
    bytes_so_far: &std::sync::Arc<std::sync::atomic::AtomicU64>,
    started_at: std::time::Instant,
    cancel: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    let mut local_file = tokio::fs::File::open(local_path)
        .await
        .map_err(|e| format!("Cannot open local file: {}", e))?;

    let mut remote_file = sftp
        .create(remote_path)
        .await
        .map_err(|e| format!("Cannot create remote file: {}", e))?;

    let chunk_size = 32768;
    let mut buf = vec![0u8; chunk_size];

    loop {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("Cancelled".to_string());
        }
        let n = local_file
            .read(&mut buf)
            .await
            .map_err(|e| format!("Read error: {}", e))?;
        if n == 0 {
            break;
        }
        remote_file
            .write_all(&buf[..n])
            .await
            .map_err(|e| format!("Write error: {}", e))?;

        let transferred = bytes_so_far.fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed) + n as u64;
        let _ = resp_tx.send(SftpResponse::Progress(TransferProgress {
            filename: dir_name.to_string(),
            bytes_transferred: transferred,
            total_bytes,
            is_upload: true,
            started_at,
        }));
    }

    remote_file
        .shutdown()
        .await
        .map_err(|e| format!("Remote file close error: {}", e))?;
    Ok(())
}

/// Recursively download a remote directory to a local path.
async fn download_dir(
    sftp: &russh_sftp::client::SftpSession,
    remote_dir: &str,
    local_dir: &str,
    resp_tx: &mpsc::UnboundedSender<SftpResponse>,
    cancel: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    let dir_name = remote_dir
        .rsplit('/')
        .next()
        .unwrap_or(remote_dir)
        .to_string();
    let total_bytes = remote_dir_total_size(sftp, remote_dir).await;
    let bytes_so_far = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let started_at = std::time::Instant::now();

    let _ = resp_tx.send(SftpResponse::Progress(TransferProgress {
        filename: dir_name.clone(),
        bytes_transferred: 0,
        total_bytes,
        is_upload: false,
        started_at,
    }));

    download_dir_inner(sftp, remote_dir, local_dir, resp_tx, &dir_name, total_bytes, &bytes_so_far, started_at, cancel).await
}

async fn download_dir_inner(
    sftp: &russh_sftp::client::SftpSession,
    remote_dir: &str,
    local_dir: &str,
    resp_tx: &mpsc::UnboundedSender<SftpResponse>,
    dir_name: &str,
    total_bytes: u64,
    bytes_so_far: &std::sync::Arc<std::sync::atomic::AtomicU64>,
    started_at: std::time::Instant,
    cancel: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("Cancelled".to_string());
    }
    std::fs::create_dir_all(local_dir)
        .map_err(|e| format!("Cannot create local dir {}: {}", local_dir, e))?;

    let read_dir = sftp
        .read_dir(remote_dir)
        .await
        .map_err(|e| format!("Cannot read remote dir {}: {}", remote_dir, e))?;

    for entry in read_dir {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("Cancelled".to_string());
        }
        let name = entry.file_name();
        if name == "." || name == ".." {
            continue;
        }
        let remote_path = format!("{}/{}", remote_dir.trim_end_matches('/'), name);
        let local_path = format!("{}/{}", local_dir.trim_end_matches('/'), name);

        let ft = entry.file_type();
        if ft == russh_sftp::protocol::FileType::Dir {
            Box::pin(download_dir_inner(sftp, &remote_path, &local_path, resp_tx, dir_name, total_bytes, bytes_so_far, started_at, cancel)).await?;
        } else if ft == russh_sftp::protocol::FileType::File {
            download_file_for_dir(sftp, &remote_path, &local_path, resp_tx, dir_name, total_bytes, bytes_so_far, started_at, cancel).await?;
        }
    }
    Ok(())
}

async fn download_file_for_dir(
    sftp: &russh_sftp::client::SftpSession,
    remote_path: &str,
    local_path: &str,
    resp_tx: &mpsc::UnboundedSender<SftpResponse>,
    dir_name: &str,
    total_bytes: u64,
    bytes_so_far: &std::sync::Arc<std::sync::atomic::AtomicU64>,
    started_at: std::time::Instant,
    cancel: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    let mut remote_file = sftp
        .open(remote_path)
        .await
        .map_err(|e| format!("Cannot open remote file: {}", e))?;

    let mut local_file = tokio::fs::File::create(local_path)
        .await
        .map_err(|e| format!("Cannot create local file: {}", e))?;

    let chunk_size = 32768;
    let mut buf = vec![0u8; chunk_size];

    loop {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("Cancelled".to_string());
        }
        let n = remote_file
            .read(&mut buf)
            .await
            .map_err(|e| format!("Read error: {}", e))?;
        if n == 0 {
            break;
        }
        local_file
            .write_all(&buf[..n])
            .await
            .map_err(|e| format!("Write error: {}", e))?;

        let transferred = bytes_so_far.fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed) + n as u64;
        let _ = resp_tx.send(SftpResponse::Progress(TransferProgress {
            filename: dir_name.to_string(),
            bytes_transferred: transferred,
            total_bytes,
            is_upload: false,
            started_at,
        }));
    }

    Ok(())
}

const MAX_EDIT_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10 MB

/// Read a remote file's content into memory (≤10MB, for the editor).
async fn read_file_content(
    sftp: &russh_sftp::client::SftpSession,
    path: &str,
) -> Result<Vec<u8>, String> {
    let meta = sftp
        .metadata(path)
        .await
        .map_err(|e| format!("Cannot stat file: {}", e))?;
    let file_size = meta.size.unwrap_or(0);
    if file_size > MAX_EDIT_FILE_SIZE {
        return Err(format!("TOO_LARGE:{}", file_size));
    }

    let mut file = sftp
        .open(path)
        .await
        .map_err(|e| format!("Cannot open file: {}", e))?;

    let mut data = Vec::with_capacity(file_size as usize);
    let mut buf = vec![0u8; 32768];
    loop {
        let n = file.read(&mut buf).await.map_err(|e| format!("Read error: {}", e))?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
    }
    Ok(data)
}

/// Write data to a remote file (for the editor).
async fn write_file_content(
    sftp: &russh_sftp::client::SftpSession,
    path: &str,
    data: &[u8],
) -> Result<(), String> {
    let mut file = sftp
        .create(path)
        .await
        .map_err(|e| format!("Cannot create file: {}", e))?;
    file.write_all(data)
        .await
        .map_err(|e| format!("Write error: {}", e))?;
    file.shutdown()
        .await
        .map_err(|e| format!("Close error: {}", e))?;
    Ok(())
}

/// Upload a local file to a remote path with progress reporting.
async fn upload_file(
    sftp: &russh_sftp::client::SftpSession,
    local_path: &str,
    remote_path: &str,
    resp_tx: &mpsc::UnboundedSender<SftpResponse>,
    cancel: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    let filename = std::path::Path::new(local_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(local_path)
        .to_string();

    let local_meta = tokio::fs::metadata(local_path)
        .await
        .map_err(|e| format!("Cannot stat local file: {}", e))?;
    let total_bytes = local_meta.len();

    let mut local_file = tokio::fs::File::open(local_path)
        .await
        .map_err(|e| format!("Cannot open local file: {}", e))?;

    let mut remote_file = sftp
        .create(remote_path)
        .await
        .map_err(|e| format!("Cannot create remote file: {}", e))?;

    let started_at = std::time::Instant::now();
    let mut bytes_transferred: u64 = 0;
    let chunk_size = 32768;
    let mut buf = vec![0u8; chunk_size];

    loop {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("Cancelled".to_string());
        }
        let n = local_file
            .read(&mut buf)
            .await
            .map_err(|e| format!("Read error: {}", e))?;
        if n == 0 {
            break;
        }
        remote_file
            .write_all(&buf[..n])
            .await
            .map_err(|e| format!("Write error: {}", e))?;
        bytes_transferred += n as u64;

        let _ = resp_tx.send(SftpResponse::Progress(TransferProgress {
            filename: filename.clone(),
            bytes_transferred,
            total_bytes,
            is_upload: true,
            started_at,
        }));
    }

    remote_file
        .shutdown()
        .await
        .map_err(|e| format!("Remote file close error: {}", e))?;

    Ok(())
}
