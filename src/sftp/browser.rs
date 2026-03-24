//! # SFTP Browser
//!
//! SFTP file browser state management, driven from the GUI.

use tokio::sync::mpsc;

use crate::config::ResolvedAuth;
use crate::sftp::selection::FileSelection;
use crate::sftp::types::{SftpEntry, SftpConnectionState, TransferProgress, SftpCommand, SftpResponse};

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
    pub show_hidden_files: bool,
    cancel_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub pending_file_content: Option<(String, Vec<u8>)>,
    pub pending_file_too_large: Option<(String, u64)>,
    // Connection params stored for auto-reconnect
    runtime: tokio::runtime::Handle,
    conn_host: String,
    conn_port: u16,
    conn_username: String,
    conn_auth: ResolvedAuth,
}

impl SftpBrowser {
    /// Spawn a new SFTP connection task and return the browser handle.
    pub fn connect(
        runtime: &tokio::runtime::Runtime,
        host: String,
        port: u16,
        username: String,
        auth: ResolvedAuth,
        host_name: String,
    ) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (resp_tx, resp_rx) = mpsc::unbounded_channel();
        let cancel_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        let cancel_clone = cancel_flag.clone();
        runtime.spawn(crate::sftp::task::sftp_task(host.clone(), port, username.clone(), auth.clone(), cmd_rx, resp_tx, cancel_clone));

        Self {
            cmd_tx,
            resp_rx,
            state: SftpConnectionState::Connecting,
            current_path: String::new(),
            entries: Vec::new(),
            transfer: None,
            host_name: host_name.clone(),
            selection: FileSelection::default(),
            path_input: String::new(),
            show_hidden_files: false,
            cancel_flag,
            pending_file_content: None,
            pending_file_too_large: None,
            runtime: runtime.handle().clone(),
            conn_host: host,
            conn_port: port,
            conn_username: username,
            conn_auth: auth,
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
                    if matches!(self.state, SftpConnectionState::Connected) {
                        // Connection lost while previously connected — auto-reconnect
                        log::info!("SFTP connection lost, auto-reconnecting to {}", self.host_name);
                        self.auto_reconnect();
                    } else {
                        self.state = SftpConnectionState::Disconnected;
                    }
                }
            }
        }
    }

    /// Re-establish the connection using stored params, then navigate back to the current path.
    fn auto_reconnect(&mut self) {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (resp_tx, resp_rx) = mpsc::unbounded_channel();
        let cancel_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        let reconnect_path = if self.current_path.is_empty() {
            None
        } else {
            Some(self.current_path.clone())
        };

        let cancel_clone = cancel_flag.clone();
        self.runtime.spawn(crate::sftp::task::sftp_task_with_initial_path(
            self.conn_host.clone(),
            self.conn_port,
            self.conn_username.clone(),
            self.conn_auth.clone(),
            cmd_rx,
            resp_tx,
            cancel_clone,
            reconnect_path,
        ));

        self.cmd_tx = cmd_tx;
        self.resp_rx = resp_rx;
        self.cancel_flag = cancel_flag;
        self.transfer = None;
        self.state = SftpConnectionState::Connecting;
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

    /// Toggle visibility of hidden files (files starting with .).
    pub fn toggle_hidden_files(&mut self) {
        self.show_hidden_files = !self.show_hidden_files;
        self.selection.clear();
    }

    /// Get filtered entries (hides dotfiles if show_hidden_files is false).
    pub fn filtered_entries(&self) -> Vec<SftpEntry> {
        if self.show_hidden_files {
            self.entries.clone()
        } else {
            self.entries.iter()
                .filter(|e| !e.name.starts_with('.'))
                .cloned()
                .collect()
        }
    }
}

impl Drop for SftpBrowser {
    fn drop(&mut self) {
        self.disconnect();
    }
}
