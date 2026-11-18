//! ssh-agent transport — Windows: OpenSSH agent pipe, then Pageant.
//!
//! Two agents are common on Windows: OpenSSH for Windows exposes
//! `\\.\pipe\openssh-ssh-agent` when its "OpenSSH Authentication Agent"
//! service runs, and PuTTY's Pageant keeps a resident key store of its
//! own. We try the pipe first (it is the configured-by-default agent) and
//! fall back to Pageant.
//!
//! The two transports hand back different stream types, so [`ConcreteStream`]
//! is an enum delegating `AsyncRead`/`AsyncWrite` to whichever one won.
//! (russh's own `connect_named_pipe`/`connect_pageant` are unusable here:
//! they return `AgentClient`s whose stream fields are private, and a
//! `Box<dyn>` stream type trips rustc's incomplete "Send is not general
//! enough" check through the generic async fns.)

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};

use super::PlatformAgentClient;

/// Named pipe the OpenSSH for Windows agent service listens on.
const OPENSSH_AGENT_PIPE: &str = r"\\.\pipe\openssh-ssh-agent";

/// ERROR_PIPE_BUSY: the pipe instance exists but all instances are busy —
/// retry shortly (the agent will spin up a new instance). Bounded like the
/// pageant crate's own connect loop (40 × 50 ms).
const ERROR_PIPE_BUSY: i32 = 231;
const PIPE_RETRY_ROUNDS: u32 = 40;

/// Concrete per-OS stream for [`PlatformAgentClient`] (no `dyn` — see the
/// module comment).
pub enum ConcreteStream {
    OpenSshAgent(NamedPipeClient),
    Pageant(pageant::PageantStream),
}

impl AsyncRead for ConcreteStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        // Both variants are Unpin (named-pipe clients, no self-references).
        match self.get_mut() {
            ConcreteStream::OpenSshAgent(s) => Pin::new(s).poll_read(cx, buf),
            ConcreteStream::Pageant(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for ConcreteStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            ConcreteStream::OpenSshAgent(s) => Pin::new(s).poll_write(cx, buf),
            ConcreteStream::Pageant(s) => Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            ConcreteStream::OpenSshAgent(s) => Pin::new(s).poll_flush(cx),
            ConcreteStream::Pageant(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            ConcreteStream::OpenSshAgent(s) => Pin::new(s).poll_shutdown(cx),
            ConcreteStream::Pageant(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}

/// Connect to a running Windows SSH agent (OpenSSH pipe, else Pageant).
pub async fn connect_agent() -> Result<PlatformAgentClient, String> {
    match open_openssh_pipe().await {
        Ok(pipe) => Ok(russh::keys::agent::client::AgentClient::connect(
            ConcreteStream::OpenSshAgent(pipe),
        )),
        Err(pipe_err) => match pageant::PageantStream::new().await {
            Ok(stream) => Ok(russh::keys::agent::client::AgentClient::connect(
                ConcreteStream::Pageant(stream),
            )),
            Err(pageant_err) => Err(format!(
                "no Windows SSH agent reachable (OpenSSH pipe: {}; Pageant: {})",
                pipe_err, pageant_err
            )),
        },
    }
}

/// Open the OpenSSH agent pipe, retrying while instances are busy.
async fn open_openssh_pipe() -> Result<NamedPipeClient, io::Error> {
    for _ in 0..PIPE_RETRY_ROUNDS {
        match ClientOptions::new().open(OPENSSH_AGENT_PIPE) {
            Ok(client) => return Ok(client),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) => {}
            Err(e) => return Err(e),
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(io::Error::other("OpenSSH agent pipe stayed busy"))
}

/// Actionable hint for the "agent reachable but empty" log line.
pub fn no_keys_hint() -> &'static str {
    "check that the OpenSSH agent service holds your keys, or add them \
     to a running Pageant"
}
