//! Real russh-backed mock SSH server for integration tests.
//!
//! The pre-existing `mock_ssh_server.rs` is a TCP echo server with no SSH
//! semantics. This module is a genuine `russh::server` implementation that
//! can require keyboard-interactive (2FA) auth, letting us exercise the
//! full client auth state machine against the real protocol.
//!
//! Compiled into `ssh_2fa_tests.rs` via `mod mock_russh_server;` (the
//! `tests/*.rs` sibling convention used by `e2e_tests.rs`).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rand::rngs::OsRng;
use russh::keys::{Algorithm, PrivateKey};
use russh::server::{Auth, Handler as ServerHandler, Response, Server};
use russh::{MethodKind, MethodSet};

/// What the mock server demands from clients.
#[derive(Debug, Clone)]
pub enum MockAuthMode {
    /// Plain password auth (no 2FA).
    PasswordAccept { password: String },
    /// Rejects password (advertising keyboard-interactive), then:
    /// round 1 asks the password, round 2 asks an OTP. Both must match.
    KbdIntTwoRound { password: String, otp: String },
    /// Rejects password, single keyboard-interactive OTP round.
    KbdIntSingleRound { otp: String },
    /// Rejects password, one round with an echoed + a masked prompt.
    KbdIntEchoFlags { otp: String },
    /// One 0-prompt info round, then accept.
    KbdIntZeroPromptInfo,
    /// Rejects everything.
    RejectAll,
    /// Rejects publickey (advertising keyboard-interactive), then a single
    /// OTP round.
    PubkeyThenKbdInt { otp: String },
    /// Malicious: infinite 0-prompt info rounds — clients must cap rounds.
    LoopInfoRequests,
}

/// Every answer round the server received, for assertions.
type SeenAnswers = Arc<Mutex<Vec<Vec<String>>>>;
/// Every (address, port) tcpip-forward / cancel request, for -R assertions.
type SeenForwards = Arc<Mutex<Vec<(String, u32)>>>;

pub struct MockSshServer {
    pub port: u16,
    handle: russh::server::RunningServerHandle,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
    seen: SeenAnswers,
    forwards: SeenForwards,
    cancels: SeenForwards,
}

impl MockSshServer {
    /// Bind an ephemeral localhost port and start serving `mode`.
    ///
    /// Session channels (PTY/shell/exec) are refused — tests that need a
    /// full SshSession connect must use [`MockSshServer::start_with_session_channels`].
    pub async fn start(mode: MockAuthMode) -> std::io::Result<Self> {
        Self::start_with(mode, false).await
    }

    /// Like [`MockSshServer::start`], but session channels are granted so a
    /// full `SshSession` (shell-hint exec, PTY, shell) can connect.
    pub async fn start_with_session_channels(mode: MockAuthMode) -> std::io::Result<Self> {
        Self::start_with(mode, true).await
    }

    async fn start_with(mode: MockAuthMode, session_channels: bool) -> std::io::Result<Self> {
        let key = PrivateKey::random(&mut OsRng, Algorithm::Ed25519)
            .map_err(|e| std::io::Error::other(format!("keygen: {}", e)))?;

        let config = russh::server::Config {
            keys: vec![key],
            // Default 1s rejection delay would slow every negative test.
            auth_rejection_time: Duration::from_millis(1),
            auth_rejection_time_initial: Some(Duration::from_millis(1)),
            max_auth_attempts: 20,
            methods: MethodSet::from(&[
                MethodKind::Password,
                MethodKind::PublicKey,
                MethodKind::KeyboardInteractive,
            ][..]),
            ..Default::default()
        };

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();

        let seen: SeenAnswers = Arc::new(Mutex::new(Vec::new()));
        let forwards: SeenForwards = Arc::new(Mutex::new(Vec::new()));
        let cancels: SeenForwards = Arc::new(Mutex::new(Vec::new()));
        // `run_on_socket`'s accept loop borrows the server and listener
        // for `'static` (it outlives `start`). Leak them — a bounded,
        // test-only cost of a few hundred bytes per server.
        let server: &'static mut MockSshd = Box::leak(Box::new(MockSshd {
            mode,
            session_channels,
            seen: Arc::clone(&seen),
            forwards: Arc::clone(&forwards),
            cancels: Arc::clone(&cancels),
        }));
        let listener: &'static tokio::net::TcpListener = Box::leak(Box::new(listener));

        let running = server.run_on_socket(Arc::new(config), listener);
        let handle = running.handle();
        let task = tokio::spawn(running);

        Ok(Self {
            port,
            handle,
            task,
            seen,
            forwards,
            cancels,
        })
    }

    /// All answer rounds the server has received so far.
    pub fn answers_seen(&self) -> Vec<Vec<String>> {
        self.seen.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// All tcpip-forward global requests the server accepted so far.
    pub fn forwards_seen(&self) -> Vec<(String, u32)> {
        self.forwards.lock().map(|f| f.clone()).unwrap_or_default()
    }

    /// All cancel-tcpip-forward global requests the server accepted so far.
    pub fn cancels_seen(&self) -> Vec<(String, u32)> {
        self.cancels.lock().map(|f| f.clone()).unwrap_or_default()
    }

    /// Graceful shutdown; waits for the accept loop to finish.
    pub async fn shutdown(self) {
        self.handle.shutdown("test done".to_string());
        let _ = self.task.await;
    }
}

struct MockSshd {
    mode: MockAuthMode,
    session_channels: bool,
    seen: SeenAnswers,
    forwards: SeenForwards,
    cancels: SeenForwards,
}

impl russh::server::Server for MockSshd {
    type Handler = MockHandler;

    fn new_client(&mut self, _peer_addr: Option<std::net::SocketAddr>) -> Self::Handler {
        MockHandler {
            mode: self.mode.clone(),
            session_channels: self.session_channels,
            round: 0,
            seen: Arc::clone(&self.seen),
            forwards: Arc::clone(&self.forwards),
            cancels: Arc::clone(&self.cancels),
        }
    }
}

fn advertise_kbdint() -> Auth {
    Auth::Reject {
        proceed_with_methods: Some(MethodSet::from(&[MethodKind::KeyboardInteractive][..])),
        partial_success: false,
    }
}

fn prompt_set(items: &[(&'static str, bool)]) -> std::borrow::Cow<'static, [(std::borrow::Cow<'static, str>, bool)]> {
    items
        .iter()
        .map(|(text, echo)| ((*text).into(), *echo))
        .collect::<Vec<_>>()
        .into()
}

struct MockHandler {
    mode: MockAuthMode,
    session_channels: bool,
    round: usize,
    seen: SeenAnswers,
    forwards: SeenForwards,
    cancels: SeenForwards,
}

impl MockHandler {
    fn record(&self, answers: Vec<String>) {
        if let Ok(mut seen) = self.seen.lock() {
            seen.push(answers);
        }
    }
}

impl ServerHandler for MockHandler {
    type Error = russh::Error;

    async fn auth_password(&mut self, _user: &str, password: &str) -> Result<Auth, Self::Error> {
        match &self.mode {
            MockAuthMode::PasswordAccept { password: expected } if password == expected => {
                Ok(Auth::Accept)
            }
            MockAuthMode::KbdIntTwoRound { .. }
            | MockAuthMode::KbdIntSingleRound { .. }
            | MockAuthMode::KbdIntEchoFlags { .. }
            | MockAuthMode::KbdIntZeroPromptInfo
            | MockAuthMode::PubkeyThenKbdInt { .. }
            | MockAuthMode::LoopInfoRequests => Ok(advertise_kbdint()),
            _ => Ok(Auth::reject()),
        }
    }

    async fn auth_publickey_offered(
        &mut self,
        _user: &str,
        _public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<Auth, Self::Error> {
        if matches!(self.mode, MockAuthMode::PubkeyThenKbdInt { .. }) {
            Ok(advertise_kbdint())
        } else {
            Ok(Auth::reject())
        }
    }

    async fn auth_publickey(
        &mut self,
        _user: &str,
        _public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<Auth, Self::Error> {
        if matches!(self.mode, MockAuthMode::PubkeyThenKbdInt { .. }) {
            Ok(advertise_kbdint())
        } else {
            Ok(Auth::reject())
        }
    }

    /// Forward direct-tcpip channels to the requested target so jump-host
    /// tests get a real tunnel (the default handler refuses them).
    async fn channel_open_direct_tcpip(
        &mut self,
        channel: russh::Channel<russh::server::Msg>,
        host_to_connect: &str,
        port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        _session: &mut russh::server::Session,
    ) -> Result<bool, Self::Error> {
        let upstream = tokio::net::TcpStream::connect((
            host_to_connect,
            port_to_connect as u16,
        ))
        .await
        .map_err(russh::Error::IO)?;

        let mut tunnel = channel.into_stream();
        let mut upstream = upstream;
        tokio::spawn(async move {
            // Pipe until either side closes (test session lifetime).
            let _ = tokio::io::copy_bidirectional(&mut tunnel, &mut upstream).await;
        });
        Ok(true)
    }

    // ---- Session channels: granted only when the server was started with
    // `start_with_session_channels` — enough for a full SshSession connect
    // (PTY + shell + the `echo $SHELL` shell-hint exec) to complete. ----

    async fn channel_open_session(
        &mut self,
        _channel: russh::Channel<russh::server::Msg>,
        _session: &mut russh::server::Session,
    ) -> Result<bool, Self::Error> {
        Ok(self.session_channels)
    }

    async fn pty_request(
        &mut self,
        channel: russh::ChannelId,
        _term: &str,
        _col_width: u32,
        _row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        Ok(())
    }

    async fn shell_request(
        &mut self,
        channel: russh::ChannelId,
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        Ok(())
    }

    /// Reject every subsystem like OpenSSH does for an unknown one — the
    /// client's SFTP capability probe relies on getting an explicit
    /// channel_failure instead of silence.
    async fn subsystem_request(
        &mut self,
        channel: russh::ChannelId,
        _name: &str,
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        session.channel_failure(channel)?;
        Ok(())
    }

    /// Answer exec requests command-aware: the probes the client runs
    /// (shell hint, uname, ZMODEM capability) each get a plausible answer;
    /// anything else falls back to the fixed shell path + EOF so the
    /// client's shell-hint read loop terminates.
    async fn exec_request(
        &mut self,
        channel: russh::ChannelId,
        data: &[u8],
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        let cmd = String::from_utf8_lossy(data);
        let reply: &[u8] = if cmd.contains("uname -s") {
            b"Linux\n"
        } else if cmd.contains("uname -m") {
            b"x86_64\n"
        } else if cmd.contains("command -v sz") {
            // ZMODEM probe: `... && echo yes` on success.
            b"yes\n"
        } else {
            b"/bin/mocksh\n"
        };
        session.data(channel, russh::CryptoVec::from(reply))?;
        session.eof(channel)?;
        Ok(())
    }

    // ---- Global forward requests: accept and record, so -R tests can
    // assert the client actually asked. ----

    async fn tcpip_forward(
        &mut self,
        address: &str,
        port: &mut u32,
        _session: &mut russh::server::Session,
    ) -> Result<bool, Self::Error> {
        if let Ok(mut f) = self.forwards.lock() {
            f.push((address.to_string(), *port));
        }
        Ok(true)
    }

    async fn cancel_tcpip_forward(
        &mut self,
        address: &str,
        port: u32,
        _session: &mut russh::server::Session,
    ) -> Result<bool, Self::Error> {
        if let Ok(mut f) = self.cancels.lock() {
            f.push((address.to_string(), port));
        }
        Ok(true)
    }

    async fn auth_keyboard_interactive<'a>(
        &'a mut self,
        _user: &str,
        _submethods: &str,
        response: Option<Response<'a>>,
    ) -> Result<Auth, Self::Error> {
        let had_response = response.is_some();
        let answers: Vec<String> = response
            .map(|r| {
                r.map(|b| String::from_utf8_lossy(&b).into_owned())
                    .collect()
            })
            .unwrap_or_default();

        match &self.mode {
            MockAuthMode::KbdIntTwoRound { password, otp } => {
                if !had_response {
                    self.round = 1;
                    Ok(Auth::Partial {
                        name: "Password Authentication".into(),
                        instructions: "Enter your password".into(),
                        prompts: prompt_set(&[("Password:", false)]),
                    })
                } else {
                    self.record(answers.clone());
                    match self.round {
                        1 => {
                            if answers.first().map(String::as_str) == Some(password.as_str()) {
                                self.round = 2;
                                Ok(Auth::Partial {
                                    name: "Two-Factor Authentication".into(),
                                    instructions: "Enter your one-time code".into(),
                                    prompts: prompt_set(&[("Verification code:", false)]),
                                })
                            } else {
                                Ok(Auth::reject())
                            }
                        }
                        _ => {
                            if answers.first().map(String::as_str) == Some(otp.as_str()) {
                                Ok(Auth::Accept)
                            } else {
                                Ok(Auth::reject())
                            }
                        }
                    }
                }
            }
            MockAuthMode::KbdIntSingleRound { otp } => {
                if !had_response {
                    Ok(Auth::Partial {
                        name: "Two-Factor Authentication".into(),
                        instructions: "Enter your one-time code".into(),
                        prompts: prompt_set(&[("Verification code:", false)]),
                    })
                } else {
                    self.record(answers.clone());
                    if answers.first().map(String::as_str) == Some(otp.as_str()) {
                        Ok(Auth::Accept)
                    } else {
                        Ok(Auth::reject())
                    }
                }
            }
            MockAuthMode::KbdIntEchoFlags { otp } => {
                if !had_response {
                    Ok(Auth::Partial {
                        name: "Login".into(),
                        instructions: "echo=true on the login, echo=false on the code".into(),
                        prompts: prompt_set(&[("Login:", true), ("OTP:", false)]),
                    })
                } else {
                    self.record(answers.clone());
                    if answers.len() == 2 && answers[1] == *otp {
                        Ok(Auth::Accept)
                    } else {
                        Ok(Auth::reject())
                    }
                }
            }
            MockAuthMode::KbdIntZeroPromptInfo => {
                if !had_response {
                    Ok(Auth::Partial {
                        name: "Notice".into(),
                        instructions: "Just a banner round".into(),
                        prompts: prompt_set(&[]),
                    })
                } else {
                    self.record(answers.clone());
                    Ok(Auth::Accept)
                }
            }
            MockAuthMode::PubkeyThenKbdInt { otp } => {
                if !had_response {
                    Ok(Auth::Partial {
                        name: "Two-Factor Authentication".into(),
                        instructions: "Key accepted, enter your one-time code".into(),
                        prompts: prompt_set(&[("Verification code:", false)]),
                    })
                } else {
                    self.record(answers.clone());
                    if answers.first().map(String::as_str) == Some(otp.as_str()) {
                        Ok(Auth::Accept)
                    } else {
                        Ok(Auth::reject())
                    }
                }
            }
            MockAuthMode::LoopInfoRequests => {
                if had_response {
                    self.record(answers);
                }
                Ok(Auth::Partial {
                    name: "Loop".into(),
                    instructions: "never accepts".into(),
                    prompts: prompt_set(&[]),
                })
            }
            _ => Ok(Auth::reject()),
        }
    }
}
