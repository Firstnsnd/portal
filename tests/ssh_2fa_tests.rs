//! SSH keyboard-interactive (2FA) authentication tests — TDD driver.
//!
//! Phase 0 (T0.1-T0.4): mock infrastructure + fast-path regressions.
//! Phase 1 (T1.1): RED — proves the client cannot do keyboard-interactive.

mod mock_russh_server;

use std::sync::Arc;
use std::time::Duration;

use portal::config::ResolvedAuth;
use portal::ssh::{open_ssh_connection, AuthPromptBridge, HostKeyPolicy};
use russh::client::Handle;

use mock_russh_server::{MockAuthMode, MockSshServer};
use portal::ssh::SshClient;

async fn with_timeout<F: std::future::Future>(fut: F) -> F::Output {
    tokio::time::timeout(Duration::from_secs(5), fut)
        .await
        .expect("test timed out")
}

/// Mirror of the production `connect_and_authenticate`, but against the
/// ephemeral-key mock server (AcceptAll policy so tests never touch the
/// user's real known_hosts).
async fn connect_to_mock(
    port: u16,
    auth: &ResolvedAuth,
    bridge: &AuthPromptBridge,
) -> Result<Handle<SshClient>, String> {
    let mut handle = open_ssh_connection(
        "127.0.0.1",
        port,
        HostKeyPolicy::AcceptAll,
        Some((Duration::from_secs(15), 30)),
    )
    .await?;
    portal::ssh::authenticate(&mut handle, "user", auth, bridge).await?;
    Ok(handle)
}

/// Background loop that answers every prompt the bridge surfaces by
/// matching the prompt text prefix against `lookup`. Stands in for the
/// human entering OTPs.
fn spawn_answerer(
    bridge: Arc<AuthPromptBridge>,
    lookup: Vec<(&'static str, String)>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            if let Some(p) = bridge.pending() {
                let answers: Vec<String> = p
                    .prompts
                    .iter()
                    .map(|f| {
                        lookup
                            .iter()
                            .find(|(prefix, _)| f.prompt.starts_with(prefix))
                            .map(|(_, v)| v.clone())
                            .unwrap_or_default()
                    })
                    .collect();
                if answers.len() == p.prompts.len() {
                    bridge.respond(answers);
                }
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
}

// ── Phase 0: infrastructure + fast-path regressions ─────────────────────

/// T0.1 — the mock server binds an ephemeral port, accepts a raw TCP
/// connection, and shuts down cleanly.
#[tokio::test]
async fn mock_server_starts_and_stops() {
    let server = with_timeout(MockSshServer::start(MockAuthMode::PasswordAccept {
        password: "pw".into(),
    }))
    .await
    .expect("mock server starts");
    let port = server.port;

    // A raw TCP connect proves the listener is live (the SSH banner will
    // be written by the server on connect).
    let stream = with_timeout(tokio::net::TcpStream::connect(("127.0.0.1", port)))
        .await
        .expect("raw TCP connect");
    drop(stream);

    with_timeout(server.shutdown()).await;
}

/// T0.2 — password auth against a real SSH server works end to end.
/// The repo's first genuine SSH-protocol test.
#[tokio::test]
async fn password_auth_succeeds_against_mock() {
    let server = MockSshServer::start(MockAuthMode::PasswordAccept {
        password: "pw123".into(),
    })
    .await
    .expect("mock server starts");

    let bridge = AuthPromptBridge::new();
    let handle = connect_to_mock(
        server.port,
        &ResolvedAuth::Password { password: "pw123".into() },
        &bridge,
    )
    .await;

    with_timeout(server.shutdown()).await;
    handle.expect("password auth must succeed");
}

/// T0.3 — a password-only server must never surface a prompt (the fast
/// path stays fast; regression guard against prompting on plain hosts).
#[tokio::test]
async fn password_only_server_never_surfaces_prompt() {
    let server = MockSshServer::start(MockAuthMode::PasswordAccept {
        password: "pw123".into(),
    })
    .await
    .expect("mock server starts");

    let bridge = AuthPromptBridge::new();
    let result = with_timeout(connect_to_mock(
        server.port,
        &ResolvedAuth::Password { password: "pw123".into() },
        &bridge,
    ))
    .await;

    assert!(bridge.pending().is_none(), "no prompt may surface on a password-only server");
    assert!(result.is_ok());

    with_timeout(server.shutdown()).await;
}

/// T0.4 — tests must never touch the user's real known_hosts file.
#[tokio::test]
async fn known_hosts_file_not_touched_by_tests() {
    let known_hosts = dirs::home_dir().map(|h| h.join(".ssh/known_hosts"));
    let before: Option<Vec<u8>> = known_hosts
        .as_ref()
        .and_then(|p| std::fs::read(p).ok());

    let server = MockSshServer::start(MockAuthMode::PasswordAccept {
        password: "pw".into(),
    })
    .await
    .expect("mock server starts");

    let bridge = AuthPromptBridge::new();
    // Connect twice (learning behavior would append on first connect).
    for _ in 0..2 {
        let handle = with_timeout(connect_to_mock(
            server.port,
            &ResolvedAuth::Password { password: "pw".into() },
            &bridge,
        ))
        .await;
        drop(handle.expect("connect ok"));
    }
    with_timeout(server.shutdown()).await;

    let after: Option<Vec<u8>> = known_hosts
        .as_ref()
        .and_then(|p| std::fs::read(p).ok());
    assert_eq!(
        before, after,
        "known_hosts must be byte-identical after test connections"
    );
}

// ── Phase 3: the auth state machine against the mock ────────────────────

/// T3.1 — password rejected → kbd-int round 1 (password) → round 2 (OTP)
/// → connected.
#[tokio::test]
async fn kbdint_two_round_password_then_otp_succeeds() {
    let server = MockSshServer::start(MockAuthMode::KbdIntTwoRound {
        password: "pw123".into(),
        otp: "123456".into(),
    })
    .await
    .expect("mock server starts");

    let bridge = Arc::new(AuthPromptBridge::new());
    let _answerer = spawn_answerer(
        Arc::clone(&bridge),
        vec![("Password:", "pw123".into()), ("Verification", "123456".into())],
    );

    let result = with_timeout(connect_to_mock(
        server.port,
        &ResolvedAuth::Password { password: "ignored-by-kbdint-server".into() },
        &bridge,
    ))
    .await;

    let seen = server.answers_seen();
    with_timeout(server.shutdown()).await;
    assert!(result.is_ok(), "2FA two-round server must be connectable — got: {:?}", result.err());
    // Both rounds reached the server with the typed answers.
    assert_eq!(seen, vec![vec!["pw123".to_string()], vec!["123456".to_string()]]);
}

/// T3.2 — single-round OTP server.
#[tokio::test]
async fn kbdint_single_round_otp_succeeds() {
    let server = MockSshServer::start(MockAuthMode::KbdIntSingleRound {
        otp: "654321".into(),
    })
    .await
    .expect("mock server starts");

    let bridge = Arc::new(AuthPromptBridge::new());
    let _answerer = spawn_answerer(
        Arc::clone(&bridge),
        vec![("Verification", "654321".into())],
    );

    let result = with_timeout(connect_to_mock(
        server.port,
        &ResolvedAuth::Password { password: "x".into() },
        &bridge,
    ))
    .await;

    with_timeout(server.shutdown()).await;
    assert!(result.is_ok(), "single-round OTP server must be connectable — got: {:?}", result.err());
}

/// T3.3 — a wrong OTP fails with the clear auth error, and the typed
/// (wrong) answer did reach the server.
#[tokio::test]
async fn kbdint_wrong_otp_rejected() {
    let server = MockSshServer::start(MockAuthMode::KbdIntSingleRound {
        otp: "123456".into(),
    })
    .await
    .expect("mock server starts");

    let bridge = Arc::new(AuthPromptBridge::new());
    let _answerer = spawn_answerer(
        Arc::clone(&bridge),
        vec![("Verification", "000000".into())], // wrong
    );

    let result = with_timeout(connect_to_mock(
        server.port,
        &ResolvedAuth::Password { password: "x".into() },
        &bridge,
    ))
    .await;

    let seen = server.answers_seen();
    with_timeout(server.shutdown()).await;
    let err = result.err().unwrap_or_else(|| panic!("wrong OTP must fail"));
    assert_eq!(err, "Authentication failed");
    assert_eq!(seen, vec![vec!["000000".to_string()]], "the wrong answer must reach the server");
}

/// T3.4 — per-prompt echo flags survive to the bridge (UI masks echo=false).
#[tokio::test]
async fn kbdint_echo_flags_reach_the_bridge() {
    let server = MockSshServer::start(MockAuthMode::KbdIntEchoFlags {
        otp: "999999".into(),
    })
    .await
    .expect("mock server starts");

    let bridge = Arc::new(AuthPromptBridge::new());
    let auth = ResolvedAuth::Password { password: "x".into() };
    let task_bridge = Arc::clone(&bridge);
    let port = server.port;
    let connect = tokio::spawn(async move {
        connect_to_mock(port, &auth, &task_bridge).await
    });

    // Drive manually to inspect the fields before answering.
    let prompt = loop {
        if let Some(p) = bridge.pending() {
            break p;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    };
    assert_eq!(prompt.prompts.len(), 2);
    assert_eq!(prompt.prompts[0].prompt, "Login:");
    assert!(prompt.prompts[0].echo, "Login: must be echoed");
    assert_eq!(prompt.prompts[1].prompt, "OTP:");
    assert!(!prompt.prompts[1].echo, "OTP: must be masked");

    assert!(bridge.respond(vec!["user".into(), "999999".into()]));
    let result = with_timeout(connect).await.expect("join");
    with_timeout(server.shutdown()).await;
    assert!(result.is_ok(), "echo-flag server must be connectable — got: {:?}", result.err());
}

/// T3.5 — a 0-prompt info round is auto-answered without ever surfacing
/// to the UI.
#[tokio::test]
async fn kbdint_zero_prompt_round_auto_answered_without_ui() {
    let server = MockSshServer::start(MockAuthMode::KbdIntZeroPromptInfo)
        .await
        .expect("mock server starts");

    let bridge = AuthPromptBridge::new();
    let result = with_timeout(connect_to_mock(
        server.port,
        &ResolvedAuth::Password { password: "x".into() },
        &bridge,
    ))
    .await;

    with_timeout(server.shutdown()).await;
    assert!(result.is_ok(), "zero-prompt info server must be connectable — got: {:?}", result.err());
    assert!(bridge.pending().is_none(), "info-only rounds must never surface a prompt");
}

/// T3.6 — a reject-everything server yields the clear error, no prompt.
#[tokio::test]
async fn reject_all_yields_clear_error() {
    let server = MockSshServer::start(MockAuthMode::RejectAll)
        .await
        .expect("mock server starts");

    let bridge = AuthPromptBridge::new();
    let result = with_timeout(connect_to_mock(
        server.port,
        &ResolvedAuth::Password { password: "x".into() },
        &bridge,
    ))
    .await;

    with_timeout(server.shutdown()).await;
    assert!(bridge.pending().is_none());
    assert_eq!(result.err().unwrap_or_else(|| panic!("reject-all must fail")), "Authentication failed");
}

/// T3.7 — a malicious server looping 0-prompt info rounds is stopped by
/// the round cap.
#[tokio::test]
async fn infinite_info_requests_stopped_by_round_guard() {
    let server = MockSshServer::start(MockAuthMode::LoopInfoRequests)
        .await
        .expect("mock server starts");

    let bridge = AuthPromptBridge::new();
    let result = with_timeout(connect_to_mock(
        server.port,
        &ResolvedAuth::Password { password: "x".into() },
        &bridge,
    ))
    .await;

    with_timeout(server.shutdown()).await;
    assert_eq!(
        result.err().unwrap_or_else(|| panic!("loop server must be cut off")),
        "Too many authentication rounds"
    );
}

/// T3.8 — cancelling mid-prompt aborts the auth task promptly.
#[tokio::test]
async fn user_cancel_mid_prompt_aborts_auth() {
    let server = MockSshServer::start(MockAuthMode::KbdIntSingleRound {
        otp: "123456".into(),
    })
    .await
    .expect("mock server starts");

    let bridge = Arc::new(AuthPromptBridge::new());
    let auth = ResolvedAuth::Password { password: "x".into() };
    let task_bridge = Arc::clone(&bridge);
    let port = server.port;
    let connect = tokio::spawn(async move {
        connect_to_mock(port, &auth, &task_bridge).await
    });

    // Wait for the OTP prompt, then the user cancels.
    loop {
        if bridge.pending().is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    bridge.cancel();

    let result = with_timeout(connect).await.expect("join");
    with_timeout(server.shutdown()).await;
    assert_eq!(
        result.err().unwrap_or_else(|| panic!("cancel must abort auth")),
        "Authentication cancelled"
    );
}

/// T3.9 — publickey rejected (kbd-int advertised) → OTP prompt → in.
/// Key+2FA servers work through the same fallback.
#[tokio::test]
async fn pubkey_then_kbdint_fallback() {
    let server = MockSshServer::start(MockAuthMode::PubkeyThenKbdInt {
        otp: "246810".into(),
    })
    .await
    .expect("mock server starts");

    // Generate a throwaway client key and feed it as inline key content,
    // exactly like a host configured with an SSH key.
    let key = russh::keys::PrivateKey::random(
        &mut rand::rngs::OsRng,
        russh::keys::Algorithm::Ed25519,
    )
    .expect("client keygen");
    let key_content = key
        .to_openssh(russh::keys::ssh_key::LineEnding::LF)
        .expect("serialize key")
        .to_string();

    let bridge = Arc::new(AuthPromptBridge::new());
    let _answerer = spawn_answerer(
        Arc::clone(&bridge),
        vec![("Verification", "246810".into())],
    );

    let result = with_timeout(connect_to_mock(
        server.port,
        &ResolvedAuth::Key { key_content, passphrase: None },
        &bridge,
    ))
    .await;

    let seen = server.answers_seen();
    with_timeout(server.shutdown()).await;
    assert!(result.is_ok(), "pubkey→kbd-int server must be connectable — got: {:?}", result.err());
    assert_eq!(seen, vec![vec!["246810".to_string()]]);
}

/// T3.10 — a Decline bridge (connection-test dialog) gets an actionable
/// error instead of hanging or silently failing.
#[tokio::test]
async fn decline_bridge_gives_actionable_error() {
    let server = MockSshServer::start(MockAuthMode::KbdIntSingleRound {
        otp: "123456".into(),
    })
    .await
    .expect("mock server starts");

    let bridge = AuthPromptBridge::decline();
    let result = with_timeout(connect_to_mock(
        server.port,
        &ResolvedAuth::Password { password: "x".into() },
        &bridge,
    ))
    .await;

    with_timeout(server.shutdown()).await;
    let err = result.err().unwrap_or_else(|| panic!("decline must not connect"));
    assert!(
        err.contains("two-factor") && err.contains("terminal session"),
        "error must tell the user what to do, got: {err}"
    );
    assert!(bridge.pending().is_none(), "decline must never publish a prompt");
}

/// T3.11 — jump host (plain password) → 2FA target: the target's OTP
/// prompt surfaces through the same bridge and the connection lands.
#[tokio::test]
async fn jump_target_auth_uses_same_state_machine() {
    let jump = MockSshServer::start(MockAuthMode::PasswordAccept {
        password: "jumppw".into(),
    })
    .await
    .expect("jump mock starts");
    let target = MockSshServer::start(MockAuthMode::KbdIntSingleRound {
        otp: "135790".into(),
    })
    .await
    .expect("target mock starts");

    let bridge = Arc::new(AuthPromptBridge::new());
    let _answerer = spawn_answerer(
        Arc::clone(&bridge),
        vec![("Verification", "135790".into())],
    );

    let jump_info = portal::ssh::JumpHostInfo {
        host: "127.0.0.1".into(),
        port: jump.port,
        username: "user".into(),
        auth: ResolvedAuth::Password { password: "jumppw".into() },
    };

    let result = with_timeout(portal::ssh::connect_via_jump(
        &jump_info,
        "127.0.0.1",
        target.port,
        "user",
        &ResolvedAuth::Password { password: "ignored".into() },
        0,
        false,
        &bridge,
        HostKeyPolicy::AcceptAll,
    ))
    .await;

    let seen = target.answers_seen();
    with_timeout(jump.shutdown()).await;
    with_timeout(target.shutdown()).await;
    assert!(result.is_ok(), "jump→2FA target must be connectable — got: {:?}", result.err());
    assert_eq!(seen, vec![vec!["135790".to_string()]]);
}

// ── Phase 4: SFTP path ──────────────────────────────────────────────────

/// Drive `sftp_task` to completion and collect the terminal response.
async fn drive_sftp_task(
    port: u16,
    otp_delay: std::time::Duration,
    otp: &str,
) -> SftpTerminal {
    use portal::sftp::{sftp_task, SftpResponse};
    use tokio::sync::mpsc;

    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let (resp_tx, mut resp_rx) = mpsc::unbounded_channel();
    let cancel_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let bridge = std::sync::Arc::new(AuthPromptBridge::new());

    let task_bridge = std::sync::Arc::clone(&bridge);
    let auth = ResolvedAuth::Password { password: "x".into() };
    let task = tokio::spawn(async move {
        sftp_task(
            "127.0.0.1".into(),
            port,
            "user".into(),
            auth,
            cmd_rx,
            resp_tx,
            cancel_flag,
            task_bridge,
            HostKeyPolicy::AcceptAll,
        )
        .await
    });
    // Keep the command side alive until the task finishes.
    drop(cmd_tx);

    // Answer the OTP after the requested delay (simulates a slow human).
    let deadline = std::time::Instant::now() + otp_delay;
    let mut answered = false;
    let mut terminal = SftpTerminal::Running;
    while std::time::Instant::now() < deadline + std::time::Duration::from_secs(4) {
        if !answered && deadline <= std::time::Instant::now() {
            if let Some(p) = bridge.pending() {
                let answers: Vec<String> =
                    p.prompts.iter().map(|_| otp.to_string()).collect();
                assert!(bridge.respond(answers), "answer count must match");
                answered = true;
            }
        }
        match resp_rx.try_recv() {
            Ok(SftpResponse::Error(e)) => {
                terminal = SftpTerminal::Error(e);
                break;
            }
            Ok(SftpResponse::Disconnected) => {
                terminal = SftpTerminal::Disconnected;
                break;
            }
            Ok(_) => {}
            Err(_) => {}
        }
        if task.is_finished() {
            // Drain anything left, then treat as disconnected (channel closed).
            while let Ok(r) = resp_rx.try_recv() {
                if let SftpResponse::Error(e) = r {
                    terminal = SftpTerminal::Error(e);
                }
            }
            if matches!(terminal, SftpTerminal::Running) {
                terminal = SftpTerminal::Disconnected;
            }
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let _ = task.await;
    terminal
}

enum SftpTerminal {
    Running,
    Error(String),
    Disconnected,
}

/// T4.2 — the SFTP task completes 2FA through the bridge: OTP answered,
/// auth passes, and the task proceeds into SFTP setup (failing there on
/// the mock, which has no sftp subsystem — an error that is NOT an auth
/// error proves the auth phase succeeded).
#[tokio::test]
async fn sftp_task_completes_2fa_via_bridge() {
    let server = MockSshServer::start(MockAuthMode::KbdIntSingleRound {
        otp: "112233".into(),
    })
    .await
    .expect("mock server starts");

    let terminal = drive_sftp_task(server.port, Duration::from_millis(100), "112233").await;

    with_timeout(server.shutdown()).await;
    match terminal {
        SftpTerminal::Error(e) => {
            assert!(
                !e.contains("Authentication") && !e.contains("timed out"),
                "auth must have passed, got: {e}"
            );
        }
        other => panic!("expected an error response after auth, got {:?}", matches!(other, SftpTerminal::Running)),
    }
}

/// T4.1 — the 15s CONNECT_TIMEOUT caps the transport only. A user who
/// takes longer than that to type an OTP still connects. (Slow by
/// necessity: it must outlast the real CONNECT_TIMEOUT.)
#[tokio::test]
async fn sftp_auth_phase_survives_slow_otp_beyond_connect_timeout() {
    let server = MockSshServer::start(MockAuthMode::KbdIntSingleRound {
        otp: "445566".into(),
    })
    .await
    .expect("mock server starts");

    // 16s > the 15s transport timeout; with the old
    // timeout(connect_and_authenticate) this would die with
    // "Connection timed out".
    let terminal = drive_sftp_task(server.port, Duration::from_secs(16), "445566").await;

    with_timeout(server.shutdown()).await;
    match terminal {
        SftpTerminal::Error(e) => {
            assert!(
                e != "Connection timed out",
                "auth phase must not be capped by the transport timeout, got: {e}"
            );
        }
        SftpTerminal::Running => panic!("task never finished"),
        SftpTerminal::Disconnected => panic!("task died without a response"),
    }
}
