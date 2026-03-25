//! # SFTP Async Task
//!
//! Async SFTP task running on tokio, handling all remote operations.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::config::ResolvedAuth;
use crate::ssh::connect_and_authenticate;
use crate::sftp::types::{SftpEntry, SftpEntryKind, SftpCommand, SftpResponse, TransferProgress};

/// Variant of sftp_task that navigates to a specific path after connecting (used for auto-reconnect).
pub async fn sftp_task_with_initial_path(
    host: String,
    port: u16,
    username: String,
    auth: ResolvedAuth,
    cmd_rx: mpsc::UnboundedReceiver<SftpCommand>,
    resp_tx: mpsc::UnboundedSender<SftpResponse>,
    cancel_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    initial_path: Option<String>,
) {
    sftp_task_inner(host, port, username, auth, cmd_rx, resp_tx, cancel_flag, initial_path).await
}

/// The async SFTP task running on tokio.
pub async fn sftp_task(
    host: String,
    port: u16,
    username: String,
    auth: ResolvedAuth,
    cmd_rx: mpsc::UnboundedReceiver<SftpCommand>,
    resp_tx: mpsc::UnboundedSender<SftpResponse>,
    cancel_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    sftp_task_inner(host, port, username, auth, cmd_rx, resp_tx, cancel_flag, None).await
}

/// Inner implementation shared by sftp_task and sftp_task_with_initial_path.
async fn sftp_task_inner(
    host: String,
    port: u16,
    username: String,
    auth: ResolvedAuth,
    mut cmd_rx: mpsc::UnboundedReceiver<SftpCommand>,
    resp_tx: mpsc::UnboundedSender<SftpResponse>,
    cancel_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    initial_path: Option<String>,
) {
    const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

    // Helper: check if the user cancelled
    let cancelled = || cancel_flag.load(std::sync::atomic::Ordering::Relaxed);

    // Helper: check if error indicates connection loss
    let is_disconnect_error = |e: &str| {
        let err_lower = e.to_lowercase();
        err_lower.contains("connection")
            || err_lower.contains("closed")
            || err_lower.contains("eof")
            || err_lower.contains("broken pipe")
            || err_lower.contains("reset by peer")
            || err_lower.contains("timed out")
            || err_lower.contains("no route")
    };

    // 1. Connect + authenticate (with timeout)
    let handle = match tokio::time::timeout(
        CONNECT_TIMEOUT,
        connect_and_authenticate(&host, port, &username, &auth, 0, false),
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

    // 4. Resolve initial path (reconnect path or home directory)
    let start_path = if let Some(ref path) = initial_path {
        // Reconnecting: try the previous path, fall back to home
        match sftp.canonicalize(path).await {
            Ok(p) => p,
            Err(_) => sftp.canonicalize(".").await.unwrap_or_else(|_| "/".into()),
        }
    } else {
        sftp.canonicalize(".").await.unwrap_or_else(|_| "/".into())
    };

    // List the initial directory
    if let Err(e) = list_dir(&sftp, &start_path, &resp_tx).await {
        if !cancelled() { let _ = resp_tx.send(SftpResponse::Error(e)); }
        return;
    }

    // 5. Command loop
    loop {
        match cmd_rx.recv().await {
            None => {
                // Command channel closed - connection dropped
                let _ = resp_tx.send(SftpResponse::Disconnected);
                break;
            }
            Some(SftpCommand::ListDir(path)) => {
                const OP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
                let canonical = match tokio::time::timeout(OP_TIMEOUT, sftp.canonicalize(&path)).await {
                    Ok(Ok(p)) => p,
                    Ok(Err(_)) => path.clone(),
                    Err(_) => {
                        // Timeout — connection is likely dead
                        let _ = resp_tx.send(SftpResponse::Disconnected);
                        break;
                    }
                };
                match tokio::time::timeout(OP_TIMEOUT, list_dir(&sftp, &canonical, &resp_tx)).await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        if is_disconnect_error(&e) {
                            let _ = resp_tx.send(SftpResponse::Disconnected);
                            break;
                        }
                        let _ = resp_tx.send(SftpResponse::Error(e));
                    }
                    Err(_) => {
                        let _ = resp_tx.send(SftpResponse::Disconnected);
                        break;
                    }
                }
            }
            Some(SftpCommand::Download { remote, local }) => {
                cancel_flag.store(false, std::sync::atomic::Ordering::Relaxed);
                if let Err(e) = download_file(&sftp, &remote, &local, &resp_tx, &cancel_flag).await {
                    if !cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
                        if is_disconnect_error(&e) {
                            let _ = resp_tx.send(SftpResponse::Disconnected);
                            break;
                        }
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
                        if is_disconnect_error(&e) {
                            let _ = resp_tx.send(SftpResponse::Disconnected);
                            break;
                        }
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
                        if is_disconnect_error(&e) {
                            let _ = resp_tx.send(SftpResponse::Disconnected);
                            break;
                        }
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
                        if is_disconnect_error(&e) {
                            let _ = resp_tx.send(SftpResponse::Disconnected);
                            break;
                        }
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
                    Err(e) => {
                        let err_msg = format!("Rename failed: {}", e);
                        if is_disconnect_error(&err_msg) {
                            let _ = resp_tx.send(SftpResponse::Disconnected);
                            break;
                        }
                        let _ = resp_tx.send(SftpResponse::Error(err_msg));
                    }
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
                            Err(e) => {
                                if is_disconnect_error(&e) {
                                    let _ = resp_tx.send(SftpResponse::Disconnected);
                                    break;
                                }
                                let _ = resp_tx.send(SftpResponse::Error(e));
                            }
                        }
                    }
                    Err(e) => {
                        let err_msg = format!("Cannot stat: {}", e);
                        if is_disconnect_error(&err_msg) {
                            let _ = resp_tx.send(SftpResponse::Disconnected);
                            break;
                        }
                        let _ = resp_tx.send(SftpResponse::Error(err_msg));
                    }
                }
            }
            Some(SftpCommand::CreateDir(path)) => {
                match sftp.create_dir(&path).await {
                    Ok(_) => { let _ = resp_tx.send(SftpResponse::OperationComplete); }
                    Err(e) => {
                        let err_msg = format!("Create dir failed: {}", e);
                        if is_disconnect_error(&err_msg) {
                            let _ = resp_tx.send(SftpResponse::Disconnected);
                            break;
                        }
                        let _ = resp_tx.send(SftpResponse::Error(err_msg));
                    }
                }
            }
            Some(SftpCommand::ReadFile { path }) => {
                match read_file_content(&sftp, &path).await {
                    Ok(data) => { let _ = resp_tx.send(SftpResponse::FileContent { path, data }); }
                    Err(e) if e.starts_with("TOO_LARGE:") => {
                        let size: u64 = e.trim_start_matches("TOO_LARGE:").parse().unwrap_or(0);
                        let _ = resp_tx.send(SftpResponse::FileTooLarge { path, size });
                    }
                    Err(e) => {
                        if is_disconnect_error(&e) {
                            let _ = resp_tx.send(SftpResponse::Disconnected);
                            break;
                        }
                        let _ = resp_tx.send(SftpResponse::Error(e));
                    }
                }
            }
            Some(SftpCommand::WriteFile { path, data }) => {
                match write_file_content(&sftp, &path, &data).await {
                    Ok(_) => { let _ = resp_tx.send(SftpResponse::OperationComplete); }
                    Err(e) => {
                        if is_disconnect_error(&e) {
                            let _ = resp_tx.send(SftpResponse::Disconnected);
                            break;
                        }
                        let _ = resp_tx.send(SftpResponse::Error(e));
                    }
                }
            }
            Some(SftpCommand::Disconnect) => {
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

// NOTE: The remaining helper functions (download_file, upload_file, upload_dir, download_dir,
// read_file_content, write_file_content, local_dir_total_size, remote_dir_total_size,
// upload_dir_inner, upload_file_for_dir, download_dir_inner, download_file_for_dir)
// are kept in the original session.rs file for now. They can be extracted in a future refactoring.

/// Download a remote file to local disk with progress reporting.
/// Supports resume: if local file exists, continues from where it left off.
pub async fn download_file(
    sftp: &russh_sftp::client::SftpSession,
    remote_path: &str,
    local_path: &str,
    resp_tx: &mpsc::UnboundedSender<SftpResponse>,
    cancel: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

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

    let mut local_file = match tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .read(true)
        .open(&local_path)
        .await
    {
        Ok(f) => f,
        Err(e) => return Err(format!("Cannot open local file: {}", e)),
    };

    let start_offset = local_file.metadata().await.map(|m| m.len()).unwrap_or(0);

    if start_offset > 0 && start_offset < total_bytes {
        // Resume: seek both files to resume position
        local_file.seek(std::io::SeekFrom::Start(start_offset)).await
            .map_err(|e| format!("Cannot seek local file: {}", e))?;
        remote_file.seek(std::io::SeekFrom::Start(start_offset)).await
            .map_err(|e| format!("Cannot seek remote file: {}", e))?;
    }

    let mut progress = TransferProgress {
        filename: filename.clone(),
        bytes_transferred: start_offset,
        total_bytes,
        is_upload: false,
        started_at: std::time::Instant::now(),
    };

    let mut buffer = vec![0u8; 65536];
    let mut last_progress_time = std::time::Instant::now();

    loop {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(());
        }

        let n = remote_file
            .read(&mut buffer)
            .await
            .map_err(|e| format!("Read error: {}", e))?;

        if n == 0 {
            break;
        }

        local_file.write_all(&buffer[..n]).await
            .map_err(|e| format!("Write error: {}", e))?;

        progress.bytes_transferred += n as u64;

        let now = std::time::Instant::now();
        if now.duration_since(last_progress_time).as_millis() >= 100 {
            let _ = resp_tx.send(SftpResponse::Progress(progress.clone()));
            last_progress_time = now;
        }
    }

    let _ = resp_tx.send(SftpResponse::Progress(progress));
    Ok(())
}

/// Upload a local file to a remote path with progress reporting.
/// Supports resume: if remote file exists and is smaller than local file,
/// continues from where it left off.
pub async fn upload_file(
    sftp: &russh_sftp::client::SftpSession,
    local_path: &str,
    remote_path: &str,
    resp_tx: &mpsc::UnboundedSender<SftpResponse>,
    cancel: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

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

    let (mut remote_file, start_offset) = match sftp.metadata(remote_path).await {
        Ok(remote_meta) => {
            let remote_size = remote_meta.size.unwrap_or(0);
            if remote_size > 0 && remote_size < total_bytes {
                let remote_file = sftp
                    .open_with_flags(
                        remote_path,
                        russh_sftp::protocol::OpenFlags::WRITE
                            | russh_sftp::protocol::OpenFlags::CREATE
                            | russh_sftp::protocol::OpenFlags::APPEND,
                    )
                    .await
                    .map_err(|e| format!("Cannot open remote file for append: {}", e))?;

                local_file.seek(std::io::SeekFrom::Start(remote_size)).await
                    .map_err(|e| format!("Cannot seek local file: {}", e))?;

                (remote_file, remote_size)
            } else {
                sftp.remove_file(remote_path).await.ok();
                let new_file = sftp
                    .create(remote_path)
                    .await
                    .map_err(|e| format!("Cannot create remote file: {}", e))?;
                (new_file, 0)
            }
        }
        Err(_) => {
            let new_file = sftp
                .create(remote_path)
                .await
                .map_err(|e| format!("Cannot create remote file: {}", e))?;
            (new_file, 0)
        }
    };

    let mut progress = TransferProgress {
        filename: filename.clone(),
        bytes_transferred: start_offset,
        total_bytes,
        is_upload: true,
        started_at: std::time::Instant::now(),
    };

    let mut buffer = vec![0u8; 65536];
    let mut last_progress_time = std::time::Instant::now();

    loop {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(());
        }

        let n = local_file
            .read(&mut buffer)
            .await
            .map_err(|e| format!("Read error: {}", e))?;

        if n == 0 {
            break;
        }

        remote_file.write_all(&buffer[..n]).await
            .map_err(|e| format!("Write error: {}", e))?;

        progress.bytes_transferred += n as u64;

        let now = std::time::Instant::now();
        if now.duration_since(last_progress_time).as_millis() >= 100 {
            let _ = resp_tx.send(SftpResponse::Progress(progress.clone()));
            last_progress_time = now;
        }
    }

    let _ = resp_tx.send(SftpResponse::Progress(progress));
    Ok(())
}

/// Upload an entire local directory recursively.
pub async fn upload_dir(
    sftp: &russh_sftp::client::SftpSession,
    local_dir: &str,
    remote_dir: &str,
    resp_tx: &mpsc::UnboundedSender<SftpResponse>,
    cancel: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    sftp.create_dir(remote_dir).await
        .map_err(|e| format!("Failed to create remote dir: {}", e))?;

    Box::pin(upload_dir_inner(sftp, local_dir, remote_dir, resp_tx, cancel)).await
}

async fn upload_dir_inner(
    sftp: &russh_sftp::client::SftpSession,
    local_dir: &str,
    remote_dir: &str,
    resp_tx: &mpsc::UnboundedSender<SftpResponse>,
    cancel: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    let mut entries = tokio::fs::read_dir(local_dir)
        .await
        .map_err(|e| format!("Cannot read local dir: {}", e))?;

    while let Some(entry) = entries.next_entry().await
        .map_err(|e| format!("Cannot read dir entry: {}", e))?
    {
        let name = entry.file_name().to_string_lossy().to_string();
        let local_path = format!("{}/{}", local_dir.trim_end_matches('/'), &name);
        let remote_path = format!("{}/{}", remote_dir.trim_end_matches('/'), &name);

        let ft = entry.file_type().await
            .map_err(|e| format!("Cannot get file type: {}", e))?;

        if ft.is_dir() {
            sftp.create_dir(&remote_path).await
                .map_err(|e| format!("Failed to create remote dir: {}", e))?;
            Box::pin(upload_dir_inner(sftp, &local_path, &remote_path, resp_tx, cancel)).await?;
        } else {
            upload_file_for_dir(sftp, &local_path, &remote_path, resp_tx, cancel).await?;
        }
    }

    Ok(())
}

async fn upload_file_for_dir(
    sftp: &russh_sftp::client::SftpSession,
    local_path: &str,
    remote_path: &str,
    resp_tx: &mpsc::UnboundedSender<SftpResponse>,
    cancel: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    let total = tokio::fs::metadata(local_path)
        .await
        .map_err(|e| format!("Cannot stat: {}", e))?
        .len();

    let mut local_file = tokio::fs::File::open(local_path)
        .await
        .map_err(|e| format!("Cannot open local file: {}", e))?;

    let mut remote_file = sftp
        .create(remote_path)
        .await
        .map_err(|e| format!("Cannot create remote file: {}", e))?;

    let mut buffer = vec![0u8; 65536];
    let mut transferred = 0u64;
    let mut last_progress_time = std::time::Instant::now();

    loop {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(());
        }

        let n = local_file
            .read(&mut buffer)
            .await
            .map_err(|e| format!("Read error: {}", e))?;

        if n == 0 {
            break;
        }

        remote_file.write_all(&buffer[..n]).await
            .map_err(|e| format!("Write error: {}", e))?;

        transferred += n as u64;

        let now = std::time::Instant::now();
        if now.duration_since(last_progress_time).as_millis() >= 100 {
            let progress = TransferProgress {
                filename: std::path::Path::new(local_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(local_path)
                    .to_string(),
                bytes_transferred: transferred,
                total_bytes: total,
                is_upload: true,
                started_at: std::time::Instant::now(),
            };
            let _ = resp_tx.send(SftpResponse::Progress(progress));
            last_progress_time = now;
        }
    }

    Ok(())
}

/// Download an entire remote directory recursively.
pub async fn download_dir(
    sftp: &russh_sftp::client::SftpSession,
    remote_dir: &str,
    local_dir: &str,
    resp_tx: &mpsc::UnboundedSender<SftpResponse>,
    cancel: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    tokio::fs::create_dir_all(local_dir)
        .await
        .map_err(|e| format!("Failed to create local dir: {}", e))?;

    download_dir_inner(sftp, remote_dir, local_dir, resp_tx, cancel).await
}

async fn download_dir_inner(
    sftp: &russh_sftp::client::SftpSession,
    remote_dir: &str,
    local_dir: &str,
    resp_tx: &mpsc::UnboundedSender<SftpResponse>,
    cancel: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    let entries = sftp
        .read_dir(remote_dir)
        .await
        .map_err(|e| format!("Cannot read remote dir: {}", e))?;

    for entry in entries {
        let name = entry.file_name();
        if name == "." || name == ".." {
            continue;
        }

        let remote_path = format!("{}/{}", remote_dir.trim_end_matches('/'), &name);
        let local_path = format!("{}/{}", local_dir.trim_end_matches('/'), &name);

        let ft = entry.file_type();
        if ft == russh_sftp::protocol::FileType::Dir {
            tokio::fs::create_dir_all(&local_path)
                .await
                .map_err(|e| format!("Failed to create local dir: {}", e))?;
            Box::pin(download_dir_inner(sftp, &remote_path, &local_path, resp_tx, cancel)).await?;
        } else {
            download_file_for_dir(sftp, &remote_path, &local_path, resp_tx, cancel).await?;
        }
    }

    Ok(())
}

async fn download_file_for_dir(
    sftp: &russh_sftp::client::SftpSession,
    remote_path: &str,
    local_path: &str,
    resp_tx: &mpsc::UnboundedSender<SftpResponse>,
    cancel: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    let meta = sftp
        .metadata(remote_path)
        .await
        .map_err(|e| format!("Cannot stat remote file: {}", e))?;
    let total = meta.size.unwrap_or(0);

    let mut remote_file = sftp
        .open(remote_path)
        .await
        .map_err(|e| format!("Cannot open remote file: {}", e))?;

    let mut local_file = tokio::fs::File::create(local_path)
        .await
        .map_err(|e| format!("Cannot create local file: {}", e))?;

    let mut buffer = vec![0u8; 65536];
    let mut transferred = 0u64;
    let mut last_progress_time = std::time::Instant::now();

    loop {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(());
        }

        let n = remote_file
            .read(&mut buffer)
            .await
            .map_err(|e| format!("Read error: {}", e))?;

        if n == 0 {
            break;
        }

        local_file.write_all(&buffer[..n]).await
            .map_err(|e| format!("Write error: {}", e))?;

        transferred += n as u64;

        let now = std::time::Instant::now();
        if now.duration_since(last_progress_time).as_millis() >= 100 {
            let progress = TransferProgress {
                filename: remote_path
                    .rsplit('/')
                    .next()
                    .unwrap_or(remote_path)
                    .to_string(),
                bytes_transferred: transferred,
                total_bytes: total,
                is_upload: false,
                started_at: std::time::Instant::now(),
            };
            let _ = resp_tx.send(SftpResponse::Progress(progress));
            last_progress_time = now;
        }
    }

    Ok(())
}

/// Read a remote file's content for the editor (≤10MB).
async fn read_file_content(
    sftp: &russh_sftp::client::SftpSession,
    path: &str,
) -> Result<Vec<u8>, String> {
    let meta = sftp
        .metadata(path)
        .await
        .map_err(|e| format!("Cannot stat file: {}", e))?;
    let size = meta.size.unwrap_or(0);

    if size > 10 * 1024 * 1024 {
        return Err(format!("TOO_LARGE:{}", size));
    }

    let mut file = sftp
        .open(path)
        .await
        .map_err(|e| format!("Cannot open file: {}", e))?;

    let mut buffer = Vec::with_capacity(size as usize);
    file.read_to_end(&mut buffer).await
        .map_err(|e| format!("Read error: {}", e))?;

    Ok(buffer)
}

/// Write content to a remote file from the editor.
async fn write_file_content(
    sftp: &russh_sftp::client::SftpSession,
    path: &str,
    data: &[u8],
) -> Result<(), String> {
    let mut file = sftp
        .create(path)
        .await
        .map_err(|e| format!("Cannot create file: {}", e))?;

    file.write_all(data).await
        .map_err(|e| format!("Write error: {}", e))?;

    Ok(())
}
