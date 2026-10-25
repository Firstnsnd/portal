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

/// Like `with_timeout`, but with headroom for a full SSH handshake when
/// the whole suite runs in parallel on loaded CI machines.
async fn with_handshake_timeout<F: std::future::Future>(fut: F) -> F::Output {
    tokio::time::timeout(Duration::from_secs(20), fut)
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
        Arc::new(std::sync::Mutex::new(Vec::new())),
        None,
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
        proxy: None,
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
        Arc::new(std::sync::Mutex::new(Vec::new())),
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
            None,
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

/// A pre-configured remote forward (-R) must actually be activated on
/// connect: the client sends the tcpip-forward global request, stores the
/// server-allocated port, and marks the forward Active. Stopping it must
/// send cancel-tcpip-forward. Before Phase 1 this never happened —
/// tcpip_forward was never called anywhere.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remote_forward_activated_on_connect_and_cancelled_on_stop() {
    // Full SshSession connect → the mock must grant session channels.
    let server = with_timeout(MockSshServer::start_with_session_channels(
        MockAuthMode::PasswordAccept { password: "pw".into() },
    ))
    .await
    .expect("mock server starts");

    // SshSession needs a &Runtime for its task; the #[tokio::test] runtime
    // only hands out Handles, so build a dedicated multi-thread runtime for
    // the session (all SSH client I/O lives there; the mock server keeps
    // running on the test runtime).
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("session runtime");

    let remote_cfg = portal::ssh::port_forward::PortForwardConfig {
        kind: portal::ssh::port_forward::ForwardKind::Remote,
        local_host: "127.0.0.1".into(),
        local_port: 2200,
        remote_host: "127.0.0.1".into(),
        remote_port: 12345,
    };
    let session = portal::ssh::SshSession::with_scrollback_limit(
        &rt,
        "127.0.0.1".into(),
        server.port,
        "user".into(),
        ResolvedAuth::Password { password: "pw".into() },
        80,
        24,
        Vec::new(),
        64 * 1024,
        0,
        false,
        vec![remote_cfg.clone()],
        None,
        None,
        false,
    );

    // The -R activation runs inside ssh_task; poll until it lands.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let forward_active = |session: &portal::ssh::SshSession| {
        session
            .port_forwards
            .lock()
            .unwrap()
            .iter()
            .any(|pf| pf.config == remote_cfg && matches!(*pf.state.lock().unwrap(), portal::ssh::port_forward::ForwardState::Active))
    };
    while !forward_active(&session) {
        assert!(
            std::time::Instant::now() < deadline,
            "remote forward never became Active; states={:?} server_saw={:?}",
            session.get_port_forward_states(),
            server.forwards_seen()
        );
        std::thread::sleep(Duration::from_millis(25));
    }

    // The server heard the global request with the requested port, and the
    // allocated port (fixed, non-zero request → same value) was stored.
    assert_eq!(
        server.forwards_seen(),
        vec![("127.0.0.1".to_string(), 12345)]
    );
    let allocated = *session.port_forwards.lock().unwrap()[0]
        .allocated_port
        .lock()
        .unwrap();
    assert_eq!(allocated, Some(12345));

    // Stopping the forward must tell the server to stop listening.
    session.stop_port_forward(remote_cfg.clone());
    let cancel_deadline = std::time::Instant::now() + Duration::from_secs(5);
    while server.cancels_seen().is_empty() {
        assert!(
            std::time::Instant::now() < cancel_deadline,
            "cancel-tcpip-forward never reached the server"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    assert_eq!(
        server.cancels_seen(),
        vec![("127.0.0.1".to_string(), 12345)]
    );
    let forward_removed = {
        let fwds = session.port_forwards.lock().unwrap();
        fwds.iter().all(|pf| pf.config != remote_cfg)
    };
    assert!(forward_removed, "stopped forward must be removed from the list");

    session.disconnect();
    // shutdown_background moves the blocking join off-thread — a plain drop
    // or shutdown_timeout would panic from inside this async test.
    rt.shutdown_background();
    with_timeout(server.shutdown()).await;
}

/// A dynamic forward (-D) must act as a SOCKS5 proxy: greeting, CONNECT,
/// per-connection direct-tcpip channel through the SSH server, then raw
/// bytes bridged to the destination (here: a local echo server the mock
/// pipes to).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dynamic_forward_serves_socks5_connections() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let server = with_timeout(MockSshServer::start(MockAuthMode::PasswordAccept {
        password: "pw".into(),
    }))
    .await
    .expect("mock server starts");

    // Echo server the tunnel will target through the mock's direct-tcpip.
    let echo = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let echo_port = echo.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = echo.accept().await else { return };
            tokio::spawn(async move {
                let mut buf = [0u8; 256];
                loop {
                    match sock.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => {
                            if sock.write_all(&buf[..n]).await.is_err() {
                                return;
                            }
                        }
                    }
                }
            });
        }
    });

    let bridge = AuthPromptBridge::new();
    let handle = with_timeout(connect_to_mock(
        server.port,
        &ResolvedAuth::Password { password: "pw".into() },
        &bridge,
    ))
    .await
    .expect("connects to mock");

    let config = portal::ssh::port_forward::PortForwardConfig {
        kind: portal::ssh::port_forward::ForwardKind::Dynamic,
        local_host: "127.0.0.1".into(),
        local_port: socks_listener_port(),
        remote_host: String::new(),
        remote_port: 0,
    };
    let (pf, cancel_rx) = portal::ssh::port_forward::PortForward::new(config);
    let state = Arc::clone(&pf.state);
    let notifications = Arc::new(std::sync::Mutex::new(Vec::new()));
    tokio::spawn(portal::ssh::port_forward::start_dynamic_forward(
        Arc::new(handle),
        pf.config.clone(),
        state,
        cancel_rx,
        notifications,
    ));

    // The spawned forward task binds asynchronously — retry until the
    // listener accepts (or the deadline expires).
    let connect_deadline = std::time::Instant::now() + Duration::from_secs(2);
    let mut client = loop {
        match tokio::net::TcpStream::connect(("127.0.0.1", pf.config.local_port)).await {
            Ok(c) => break c,
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                assert!(
                    std::time::Instant::now() < connect_deadline,
                    "SOCKS listener never came up on {}",
                    pf.config.local_port
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(e) => panic!("SOCKS connect failed unexpectedly: {e}"),
        }
    };

    // Greeting: offer no-auth.
    client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut reply = [0u8; 2];
    with_timeout(client.read_exact(&mut reply)).await.unwrap();
    assert_eq!(reply, [0x05, 0x00], "server must pick no-auth");

    // CONNECT to 127.0.0.1:echo_port via a domain-name ATYP.
    let mut req = vec![0x05, 0x01, 0x00, 0x03, 9];
    req.extend_from_slice(b"127.0.0.1");
    req.extend_from_slice(&echo_port.to_be_bytes());
    client.write_all(&req).await.unwrap();

    // Success reply echoes the domain + port.
    let mut header = [0u8; 5];
    with_timeout(client.read_exact(&mut header)).await.unwrap();
    assert_eq!(&header[..3], &[0x05, 0x00, 0x00]);
    assert_eq!(header[3], 0x03);
    let mut domain = vec![0u8; header[4] as usize + 2];
    with_timeout(client.read_exact(&mut domain)).await.unwrap();
    assert_eq!(&domain[..9], b"127.0.0.1");
    assert_eq!(&domain[9..], &echo_port.to_be_bytes());

    // Tunnel is live: bytes round-trip through the echo server.
    client.write_all(b"ping-through-tunnel").await.unwrap();
    let mut echo_back = vec![0u8; b"ping-through-tunnel".len()];
    with_timeout(client.read_exact(&mut echo_back)).await.unwrap();
    assert_eq!(echo_back, b"ping-through-tunnel");

    pf.stop();
}

/// A free loopback port for the SOCKS listener (ephemeral range).
fn socks_listener_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Fake HTTP CONNECT proxy: verifies the CONNECT head targets
/// `expected`, replies 200, then pipes raw bytes to the target.
/// Returns the proxy's ephemeral port.
async fn spawn_http_connect_proxy(expected: String) -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut client, _) = listener.accept().await.unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 512];
        // Read until the end of the request head.
        loop {
            let n = client.read(&mut chunk).await.unwrap_or(0);
            if n == 0 {
                return;
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        let head = String::from_utf8_lossy(&buf);
        let first = head.lines().next().unwrap_or("");
        assert_eq!(
            first, format!("CONNECT {expected} HTTP/1.1"),
            "client must CONNECT to the SSH server through the proxy"
        );
        client
            .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
            .await
            .unwrap();

        // Tunnel to the SSH server.
        let mut target = tokio::net::TcpStream::connect(&expected).await.unwrap();
        let _ = tokio::io::copy_bidirectional(&mut client, &mut target).await;
    });
    port
}

/// Full transport path: TCP -> HTTP CONNECT proxy tunnel -> SSH handshake
/// + password auth against the mock server, i.e. the `connect_stream`
/// branch of `open_ssh_connection`.
///
/// Uses a generous wrapper: a full SSH handshake can outlive the shared
/// 5s `with_timeout` when the whole suite runs in parallel.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ssh_connects_through_http_proxy() {
    let server = MockSshServer::start(MockAuthMode::PasswordAccept {
        password: "pw".into(),
    })
    .await
    .unwrap();
    let proxy_port = spawn_http_connect_proxy(format!("127.0.0.1:{}", server.port)).await;

    let proxy = portal::config::ResolvedProxy {
        kind: portal::config::ProxyKind::Http,
        host: "127.0.0.1".into(),
        port: proxy_port,
        username: None,
        password: None,
    };
    let bridge = AuthPromptBridge::new();
    let mut handle = with_handshake_timeout(open_ssh_connection(
        "127.0.0.1",
        server.port,
        HostKeyPolicy::AcceptAll,
        Some((Duration::from_secs(15), 30)),
        Arc::new(std::sync::Mutex::new(Vec::new())),
        Some(&proxy),
    ))
    .await
    .expect("SSH transport over HTTP proxy succeeds");
    portal::ssh::authenticate(
        &mut handle,
        "user",
        &ResolvedAuth::Password { password: "pw".into() },
        &bridge,
    )
    .await
    .expect("password auth through proxy succeeds");
}

/// Same path through a SOCKS5 proxy (no-auth), exercising the shared
/// socks5 framing on the client side end to end.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ssh_connects_through_socks5_proxy() {
    use portal::ssh::socks5 as s5;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let server = MockSshServer::start(MockAuthMode::PasswordAccept {
        password: "pw".into(),
    })
    .await
    .unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut client, _) = listener.accept().await.unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 256];
        // Greeting: client must offer no-auth.
        loop {
            match s5::parse_greeting(&buf, &[s5::METHOD_NO_AUTH]) {
                Ok((consumed, method)) => {
                    buf.drain(..consumed);
                    assert_eq!(method, s5::METHOD_NO_AUTH);
                    client.write_all(&[s5::SOCKS_VERSION, method]).await.unwrap();
                    break;
                }
                Err(s5::SocksError::Incomplete) => {}
                Err(_) => return,
            }
            let n = client.read(&mut chunk).await.unwrap_or(0);
            if n == 0 {
                return;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        // CONNECT request (IPv4).
        loop {
            if buf.len() >= 10 && buf[0] == s5::SOCKS_VERSION && buf[1] == s5::CMD_CONNECT {
                break;
            }
            let n = client.read(&mut chunk).await.unwrap_or(0);
            if n == 0 {
                return;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        let target_port = u16::from_be_bytes([buf[8], buf[9]]);
        assert_eq!(target_port, server.port, "SOCKS5 CONNECT must target the SSH server");
        client
            .write_all(&s5::success_reply(&s5::SocksHost::Ipv4(
                std::net::Ipv4Addr::LOCALHOST,
            ), 0))
            .await
            .unwrap();

        let mut target = tokio::net::TcpStream::connect(("127.0.0.1", target_port)).await.unwrap();
        let _ = tokio::io::copy_bidirectional(&mut client, &mut target).await;
    });

    let proxy = portal::config::ResolvedProxy {
        kind: portal::config::ProxyKind::Socks5,
        host: "127.0.0.1".into(),
        port: proxy_port,
        username: None,
        password: None,
    };
    let bridge = AuthPromptBridge::new();
    let mut handle = with_handshake_timeout(open_ssh_connection(
        "127.0.0.1",
        server.port,
        HostKeyPolicy::AcceptAll,
        Some((Duration::from_secs(15), 30)),
        Arc::new(std::sync::Mutex::new(Vec::new())),
        Some(&proxy),
    ))
    .await
    .expect("SSH transport over SOCKS5 proxy succeeds");
    portal::ssh::authenticate(
        &mut handle,
        "user",
        &ResolvedAuth::Password { password: "pw".into() },
        &bridge,
    )
    .await
    .expect("password auth through SOCKS5 proxy succeeds");
}
