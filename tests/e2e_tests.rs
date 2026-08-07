//! End-to-end tests for Portal terminal emulator
//!
//! These tests simulate real user workflows without manual interaction.

use std::time::Duration;
use tokio::time::timeout;

mod mock_ssh_server;
mod common;

pub use mock_ssh_server::*;

/// Test helper: run a function with a timeout
pub async fn with_timeout<F, T>(duration: Duration, f: F) -> Result<T, &'static str>
where
    F: std::future::Future<Output = T>,
{
    timeout(duration, f).await.map_err(|_| "Test timed out")
}

#[cfg(test)]
mod e2e_tests {
    use super::*;

    /// Test: Application can start up and create a local terminal
    #[tokio::test]
    async fn test_application_startup() {
        // This is a placeholder - actual eframe app testing would require
        // running in headless mode or with a mock graphics context
        // For now, we test the core components that don't require GUI

        // Create components that would normally be in the app
        let hosts = portal::config::load_hosts(&portal::config::hosts_file_path());
        let _credentials = portal::config::load_credentials(&portal::config::credentials_file_path());

        assert!(!hosts.is_empty());
    }

    /// Test: Terminal session can be created and written to
    #[tokio::test]
    async fn test_terminal_session_lifecycle() {
        // Use an interactive shell so the child STAYS ALIVE (a one-shot command
        // like /bin/echo exits immediately and is_alive() is correctly false).
        let session = portal::terminal::RealPtySession::new(
            1,
            80,
            24,
            "/bin/zsh"
        ).expect("Failed to create PTY session");

        // Check session is alive
        assert!(session.is_alive(), "interactive shell must stay alive");

        // Write to the terminal
        let result = session.write(b"echo hello\n");
        assert!(result.is_ok());

        // Give it a moment to process
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Check session is still alive
        assert!(session.is_alive(), "interactive shell still alive after write");
    }

    /// Test: One-shot command exits after completion
    #[tokio::test]
    async fn test_one_shot_command_exits() {
        let session = portal::terminal::RealPtySession::new(
            2, 80, 24, "/bin/echo"
        ).expect("Failed to create PTY session");
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!session.is_alive(),
            "a one-shot command must exit so is_alive() returns false");
    }

    /// Regression: a long-lived interactive shell must NEVER report
    /// `is_connected() == false` while it runs. The old `is_connected()`
    /// (`alive && is_alive()`) called `waitpid` on every render frame; a
    /// transient signal/window during that poll could flip the state mid-frame
    /// and stick the terminal in a false "disconnected" overlay that never
    /// recovered. The new model is monotonic: `is_connected()` reads the
    /// `alive` flag only, and `alive` flips false exactly once when the reader
    /// thread confirms a real child exit.
    #[tokio::test]
    async fn test_local_shell_never_reports_false_disconnect() {
        let session = portal::terminal::RealPtySession::new(
            42, 80, 24, "/bin/zsh"
        ).expect("Failed to create PTY session");

        // Immediately after spawn, before any output: must be connected.
        assert!(session.is_connected(),
            "freshly spawned interactive shell must report connected");

        // Poll repeatedly — simulating render frames over ~1s. None may report
        // disconnected, since the shell has not exited.
        for _ in 0..20 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            assert!(session.is_connected(),
                "interactive shell falsely reported disconnected while running");
        }

        // Sanity: the shell really is alive (so a true result is meaningful).
        assert!(session.is_alive(), "shell must still be alive");
    }

    /// Regression: after a real child exit, `is_connected()` flips false
    /// exactly once and never recovers — distinguishing genuine "session ended"
    /// from the spurious flap above.
    #[tokio::test]
    async fn test_local_session_disconnects_only_after_real_exit() {
        // /bin/true exits immediately with status 0.
        let session = portal::terminal::RealPtySession::new(
            43, 80, 24, "/bin/true"
        ).expect("Failed to create PTY session");

        // Wait long enough for the reader thread to observe EOF + reap the child.
        tokio::time::sleep(Duration::from_millis(500)).await;

        assert!(!session.is_connected(),
            "after /bin/true exits, is_connected() must report false (once, permanently)");
        // Poll again: must not flap back to true.
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!session.is_connected(),
            "is_connected() must not flap back to true after a confirmed exit");
    }

    /// Regression: `curl -s https://vaniot.net` produces ~5K of single-line
    /// HTML (Cloudflare challenge page). The entire output wrapped into ~67
    /// rows at 80 cols. Before the fix, `erase_below` recursively cleared
    /// wrapped continuation rows above the cursor, so zsh's prompt emission
    /// (which includes `\e[J` on macOS) erased all visible content — only the
    /// prompt remained. The fix removed that recursive clear, matching
    /// standard terminal behaviour where `\e[J` only erases downward from the
    /// cursor.
    #[tokio::test]
    async fn test_long_single_line_output_visible_in_grid() {
        let session = portal::terminal::RealPtySession::new(
            100, 80, 24, "/bin/zsh"
        ).expect("Failed to create PTY session");
        tokio::time::sleep(Duration::from_millis(2500)).await;

        session.write(b"cat /tmp/cf_vanio.txt 2>&1\n").expect("write");
        tokio::time::sleep(Duration::from_millis(2000)).await;

        let g = session.get_grid();
        let g = g.lock().unwrap();
        let sb = g.scrollback_len();

        let has_html_in_visible = (0..g.rows).any(|r| {
            let t: String = g.cells[r].iter()
                .filter(|c| !c.wide_continuation).map(|c| c.c).collect();
            t.contains('<') || t.contains('>') || t.contains("DOCTYPE") || t.contains("script")
        });

        assert!(has_html_in_visible,
            "HTML content must be visible after cat+prompt cycle. content_rows={} sb={sb}",
            (0..g.rows).filter(|&r| g.cells[r].iter().any(|c| c.c != ' ' && c.c != '\0')).count());
        assert!(sb > 0, "scrollback must contain wrapped HTML rows");
    }

    /// Test: Terminal grid can handle basic operations
    #[test]
    fn test_terminal_grid_basic_operations() {
        let mut grid = portal::terminal::TerminalGrid::new(80, 24);

        // Write some text
        grid.write_char_with_attrs('H', &Default::default());
        grid.write_char_with_attrs('e', &Default::default());
        grid.write_char_with_attrs('l', &Default::default());
        grid.write_char_with_attrs('l', &Default::default());
        grid.write_char_with_attrs('o', &Default::default());

        // Check cursor position
        assert_eq!(grid.cursor_col, 5);

        // Clear the line
        grid.erase_line_all();

        // Check cells are cleared to default (space)
        assert_eq!(grid.cells[0][0].c, ' ');
    }

    /// Test: Config serialization round-trip
    #[test]
    fn test_config_roundtrip() {
        

        let (_temp_file, path) = create_test_temp_file();

        let original_hosts = vec![
            portal::config::HostEntry {
                name: "Test Host".to_string(),
                host: "example.com".to_string(),
                port: 2222,
                username: "user".to_string(),
                group: "Test Group".to_string(),
                tags: vec!["test".to_string()],
                is_local: false,
                credential_id: None,
                auth: portal::config::AuthMethod::None,
                startup_commands: vec![],
                agent_forwarding: false,
                jump_host: None,
                port_forwards: vec![
                    portal::config::PortForwardConfig {
                        kind: portal::config::ForwardKind::Local,
                        local_host: "127.0.0.1".to_string(),
                        local_port: 8080,
                        remote_host: "localhost".to_string(),
                        remote_port: 3000,
                    }
                ],
            },
        ];

        // Save hosts
        portal::config::save_hosts(&path, &original_hosts);

        // Load hosts back
        let loaded_hosts = portal::config::load_hosts(&path);

        assert_eq!(loaded_hosts.len(), original_hosts.len() + 1); // +1 for localhost

        let test_host = &loaded_hosts.iter().find(|h| !h.is_local).unwrap();
        assert_eq!(test_host.name, "Test Host");
        assert_eq!(test_host.port, 2222);
        assert_eq!(test_host.group, "Test Group");
        assert_eq!(test_host.tags.len(), 1);
        assert_eq!(test_host.tags[0], "test");
        assert_eq!(test_host.port_forwards.len(), 1);
    }

    fn create_test_temp_file() -> (tempfile::NamedTempFile, std::path::PathBuf) {
        let temp_file = tempfile::NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();
        (temp_file, path)
    }

    /// Test: Snippet save and load
    #[test]
    fn test_snippet_save_load() {
        // Save some snippets
        let snippets = vec![
            portal::config::Snippet {
                id: uuid::Uuid::new_v4().to_string(),
                name: "Test Snippet".to_string(),
                command: "echo 'Hello, World!'".to_string(),
                group: "Test".to_string(),
            },
        ];

        portal::config::save_snippets(&snippets);
        let loaded = portal::config::load_snippets();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "Test Snippet");
        assert_eq!(loaded[0].command, "echo 'Hello, World!'");
    }
}

/// Integration test: SSH client with mock server
#[cfg(test)]
mod ssh_integration_tests {
    use super::*;

    /// Test: Mock SSH server can be created and bound to a port
    #[tokio::test]
    async fn test_mock_ssh_server_creation() {
        let server = with_timeout(Duration::from_secs(5), MockSshServer::new()).await.unwrap().unwrap();

        assert_eq!(server.host(), "127.0.0.1");
        assert!(server.port() > 0);

        server.shutdown().await;
    }

    /// Test: Mock SSH server shutdown
    #[tokio::test]
    async fn test_mock_ssh_server_shutdown() {
        let server = with_timeout(Duration::from_secs(5), MockSshServer::new()).await.unwrap().unwrap();

        // Should not panic
        server.shutdown().await;
        // Server is consumed, we can't shutdown twice
    }

    /// Test: Password mock server creation
    #[tokio::test]
    async fn test_password_server_creation() {
        let server = with_timeout(Duration::from_secs(5), MockPasswordAuthServer::new("testpass")).await.unwrap().unwrap();

        assert_eq!(server.expected_password(), "testpass");
        assert_eq!(server.host(), "127.0.0.1");
        assert!(server.port() > 0);

        server.server.shutdown().await;
    }

    /// Test: Failing server creation
    #[tokio::test]
    async fn test_failing_server_creation() {
        let server = with_timeout(Duration::from_secs(5), FailingSshServer::new(FailureMode::RejectAllAuth)).await.unwrap().unwrap();

        assert_eq!(server.connection_string(), format!("127.0.0.1:{}", server.port()));
        assert!(matches!(server.failure_mode, FailureMode::RejectAllAuth));

        server.server.shutdown().await;
    }

    /// Test: Port forward creation and lifecycle
    #[test]
    fn test_port_forward_lifecycle() {
        use portal::ssh::port_forward::{PortForward, ForwardState, PortForwardConfig, ForwardKind};

        let config = PortForwardConfig {
            kind: ForwardKind::Local,
            local_host: "127.0.0.1".to_string(),
            local_port: 18080,
            remote_host: "localhost".to_string(),
            remote_port: 8080,
        };

        let (pf, _rx) = PortForward::new(config);

        // Initial state
        assert!(matches!(pf.current_state(), ForwardState::Starting));

        // Stop
        pf.stop();
        assert!(matches!(pf.current_state(), ForwardState::Stopped));
    }

    /// Test: Connection history can be saved and loaded
    #[test]
    fn test_connection_history() {
        use portal::config::{ConnectionRecord, append_history, load_history, history_file_path};

        // Clear any existing history
        let _ = std::fs::remove_file(history_file_path());

        let record = ConnectionRecord {
            host_name: "Test Server".to_string(),
            host: "example.com".to_string(),
            port: 2222,
            username: "testuser".to_string(),
            timestamp: 1234567890,
            success: true,
        };

        append_history(record.clone());

        let loaded = load_history();
        assert!(!loaded.is_empty());

        let loaded_record = loaded.iter().find(|r| r.host_name == "Test Server");
        assert!(loaded_record.is_some());

        let loaded_record = loaded_record.unwrap();
        assert_eq!(loaded_record.host, "example.com");
        assert_eq!(loaded_record.port, 2222);
        assert!(loaded_record.success);
    }
}
