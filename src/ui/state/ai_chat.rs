//! # Per-tab AI chat state
//!
//! The chat drawer and draggable orb share this state. The LLM runner thread
//! updates it through the shared [`AiChatHandle`] (an `Arc<Mutex<..>>`) so the
//! UI never holds a mutable borrow across the network trip.

use std::sync::{Arc, Mutex};

use crate::llm::types::{ChatMessage, ChatRole, ExecutionEnvironment};

/// Shared handle the background runner and the UI both use to read/write state.
pub type AiChatHandle = Arc<Mutex<AiChatState>>;

/// State of one tab's AI chat (drawer + orb).
#[derive(Debug, Clone)]
pub struct AiChatState {
    /// Whether the drawer is open.
    pub open: bool,
    /// Complete, non-streaming conversation history (user + finished assistant).
    pub messages: Vec<ChatMessage>,
    /// The in-flight assistant reply being streamed, if any.
    pub streaming: Option<StreamingBuf>,
    /// Draft text in the composer.
    pub input: String,
    /// Last error to surface (e.g. missing key, HTTP error).
    pub error: Option<String>,
    /// A request is in flight.
    pub running: bool,
    /// Drag offset of the orb relative to its default bottom-right anchor.
    pub orb_offset: egui::Vec2,
    /// A command awaiting user confirmation before running (from a code block).
    pub confirm_run: Option<String>,
    /// A command queued to run on the next frame (see `Tab::pending_ai_command`).
    pub pending_run: Option<String>,
}

/// A streaming assistant reply chunk (role is always Assistant while streaming).
#[derive(Debug, Clone)]
pub struct StreamingBuf {
    pub text: String,
}

impl Default for AiChatState {
    fn default() -> Self {
        Self {
            open: false,
            messages: Vec::new(),
            streaming: None,
            input: String::new(),
            error: None,
            running: false,
            orb_offset: egui::Vec2::ZERO,
            confirm_run: None,
            pending_run: None,
        }
    }
}

impl AiChatState {
    /// Push a finished user message into history.
    pub fn push_user(&mut self, text: impl Into<String>) {
        self.messages.push(ChatMessage::user(text));
    }

    /// Begin (or continue) streaming an assistant reply.
    pub fn begin_stream(&mut self) {
        if self.streaming.is_none() {
            self.streaming = Some(StreamingBuf { text: String::new() });
        }
        self.running = true;
        self.error = None;
    }

    /// Append a token to the streaming reply.
    pub fn append_token(&mut self, token: &str) {
        if let Some(buf) = self.streaming.as_mut() {
            buf.text.push_str(token);
        }
    }

    /// Finalize the streaming reply into history.
    pub fn finish_stream(&mut self) {
        if let Some(buf) = self.streaming.take() {
            if !buf.text.is_empty() {
                self.messages.push(ChatMessage::assistant(buf.text));
            }
        }
        self.running = false;
    }

    /// Abort the streaming reply on error.
    pub fn fail(&mut self, err: impl Into<String>) {
        // Keep any partial text as a finished assistant message so the user
        // still sees what was generated before the failure.
        if let Some(buf) = self.streaming.take() {
            if !buf.text.is_empty() {
                self.messages.push(ChatMessage::assistant(buf.text));
            }
        }
        self.error = Some(err.into());
        self.running = false;
    }
}

/// Human-readable OS label for the current platform.
pub fn os_label() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    }
}

/// Human-readable shell name (basename of `$SHELL`), falling back to the OS
/// default when unset.
pub fn shell_label() -> String {
    shell_basename(&std::env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(windows) {
            "PowerShell".to_string()
        } else {
            "/bin/sh".to_string()
        }
    }))
}

/// Return the basename of a shell path (e.g. `/bin/zsh` → `zsh`).
fn shell_basename(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

impl ExecutionEnvironment {
    /// Local-host environment (the portal process itself).
    pub fn local() -> Self {
        ExecutionEnvironment {
            os: os_label().to_string(),
            arch: std::env::consts::ARCH.to_string(),
            shell: shell_label(),
            cwd: std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "?".to_string()),
            host: None,
        }
    }
}

/// Build the default system prompt describing the execution environment so the
/// assistant proposes commands that actually run where they'll be executed.
///
/// This is pure and injectable for testing; see [`tests`].
pub fn build_system_prompt(env: &ExecutionEnvironment) -> String {
    let location = match &env.host {
        Some(host) => format!("connected to remote host {host}"),
        None => "running on the local machine".to_string(),
    };
    let shell = if env.shell.is_empty() { "sh" } else { env.shell.as_str() };
    let cwd = if env.cwd.is_empty() { "?" } else { env.cwd.as_str() };

    format!(
        "You are a terminal assistant {location}. The execution environment is \
         {os} ({arch}), shell {shell}, working directory {cwd}. When you suggest \
         a command to run, wrap it in a ```sh code block so it can be executed, \
         and ensure the command is valid for {os} with shell {shell} and working \
         directory {cwd}.\n\n\
         Every reply must follow this Markdown contract:\n\
         - Never use ATX headings (no `#`, `##`, ... `######` lines). Use a \
         **bold** line for emphasis instead.\n\
         - Use `- ` for bullet lists and `1.` `2.` for ordered lists.\n\
         - Do not use Markdown tables; present tabular data as lists.\n\
         - Write math in LaTeX: inline `$...$`, display `$$...$$`.",
        location = location,
        os = env.os,
        arch = env.arch,
        shell = shell,
        cwd = cwd,
    )
}

/// Start streaming a chat reply on a background thread, writing tokens into the
/// shared `handle`. The user message is already in `messages`; this injects the
/// default system prompt if absent and appends the assistant reply as it streams.
///
/// `api_key` may be empty — the key is then loaded from the keychain for the
/// configured backend. `env` describes where a proposed command will run and is
/// baked into the system prompt.
pub fn spawn_chat(
    cfg: crate::llm::types::LlmConfig,
    api_key: String,
    messages: Vec<ChatMessage>,
    env: ExecutionEnvironment,
    handle: AiChatHandle,
) {
    let default_system = build_system_prompt(&env);
    std::thread::spawn(move || {
        // Resolve the API key from the keychain when not supplied.
        let key = if api_key.is_empty() {
            let backend = match cfg.backend {
                crate::llm::types::LlmBackend::OpenAiCompatible => "openai",
                crate::llm::types::LlmBackend::Anthropic => "anthropic",
            };
            match crate::config::load_llm_api_key(backend) {
                Some(k) => k,
                None => {
                    if let Ok(mut s) = handle.lock() {
                        s.fail("no_api_key");
                    }
                    crate::repaint::notify_repaint();
                    return;
                }
            }
        } else {
            api_key
        };

        // Inject the default system prompt if the caller didn't provide one.
        let mut msgs = messages;
        if !msgs.iter().any(|m| matches!(m.role, ChatRole::System)) {
            msgs.insert(0, ChatMessage::system(default_system));
        }

        {
            let mut s = handle.lock().unwrap();
            s.begin_stream();
        }
        crate::repaint::notify_repaint();

        let result = crate::llm::stream::stream_chat(&cfg, &key, &msgs, {
            let handle = Arc::clone(&handle);
            move |token| {
                if let Ok(mut s) = handle.lock() {
                    s.append_token(token);
                }
                crate::repaint::notify_repaint();
            }
        });

        let mut s = handle.lock().unwrap();
        match result {
            Ok(_) => s.finish_stream(),
            Err(e) => s.fail(e),
        }
        crate::repaint::notify_repaint();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::ChatRole;

    #[test]
    fn default_state_is_closed_idle() {
        let s = AiChatState::default();
        assert!(!s.open);
        assert!(!s.running);
        assert!(s.messages.is_empty());
        assert!(s.confirm_run.is_none());
    }

    #[test]
    fn system_prompt_embeds_local_environment() {
        let env = ExecutionEnvironment::local();
        let prompt = build_system_prompt(&env);
        // OS, arch, shell, and working directory should all be described.
        assert!(prompt.contains(&env.os));
        assert!(prompt.contains(&env.arch));
        assert!(prompt.contains(&env.shell));
        assert!(prompt.contains("local machine"));
        // The prompt must instruct code-block formatting so Run works.
        assert!(prompt.contains("```sh"));
        // The Markdown contract: no ATX headings, dash/number lists only,
        // no tables, LaTeX math.
        assert!(prompt.contains("Never use ATX headings"));
        assert!(prompt.contains("`- ` for bullet lists"));
        assert!(prompt.contains("Do not use Markdown tables"));
        assert!(prompt.contains("inline `$...$`"));
    }

    #[test]
    fn system_prompt_embeds_remote_environment() {
        let env = ExecutionEnvironment {
            os: "Linux".to_string(),
            arch: "x86_64".to_string(),
            shell: "bash".to_string(),
            cwd: "/home/alice".to_string(),
            host: Some("alice@prod-01".to_string()),
        };
        let prompt = build_system_prompt(&env);
        assert!(prompt.contains("Linux"));
        assert!(prompt.contains("x86_64"));
        assert!(prompt.contains("bash"));
        assert!(prompt.contains("/home/alice"));
        assert!(prompt.contains("alice@prod-01"));
        assert!(prompt.contains("connected to remote host"));
    }

    #[test]
    fn local_environment_has_no_host() {
        let env = ExecutionEnvironment::local();
        assert!(env.host.is_none());
        assert!(!env.os.is_empty());
        assert!(!env.shell.is_empty());
    }

    #[test]
    fn shell_label_is_basename() {
        let s = shell_label();
        assert!(!s.contains('/'));
        assert!(!s.is_empty());
    }

    #[test]
    fn os_label_is_nonempty() {
        assert!(!os_label().is_empty());
    }

    #[test]
    fn stream_lifecycle_accumulates_then_history() {
        let mut s = AiChatState::default();
        s.begin_stream();
        s.append_token("he");
        s.append_token("llo");
        assert_eq!(s.streaming.as_ref().unwrap().text, "hello");
        s.finish_stream();
        assert!(s.streaming.is_none());
        assert_eq!(s.messages.len(), 1);
        assert_eq!(s.messages[0].role, ChatRole::Assistant);
        assert_eq!(s.messages[0].content, "hello");
    }

    #[test]
    fn fail_keeps_partial_and_sets_error() {
        let mut s = AiChatState::default();
        s.begin_stream();
        s.append_token("partial");
        s.fail("boom");
        assert!(s.streaming.is_none());
        assert_eq!(s.error.as_deref(), Some("boom"));
        assert_eq!(s.messages.last().unwrap().content, "partial");
    }
}
