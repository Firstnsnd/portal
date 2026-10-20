//! # LLM domain types
//!
//! Protocol-agnostic types shared by the OpenAI-compatible and Anthropic
//! backends and by the UI (chat drawer). Nothing here does I/O.

use serde::{Deserialize, Serialize};

/// Which LLM backend/protocol to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmBackend {
    /// Any OpenAI-compatible `/chat/completions` endpoint (OpenAI, Groq,
    /// DeepSeek, Ollama, LM Studio, …).
    OpenAiCompatible,
    /// Anthropic Messages API (`/v1/messages`).
    Anthropic,
}

/// Non-secret LLM configuration. The API key itself is never stored here —
/// it lives in the system keychain (see `crate::config` keychain helpers).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    pub backend: LlmBackend,
    /// Base URL. For OpenAI-compatible backends this is required (e.g.
    /// `https://api.openai.com/v1`, `http://localhost:11434/v1`). For Anthropic
    /// it's optional — blank falls back to `https://api.anthropic.com`, but a
    /// custom host (LiteLLM/OpenRouter/Bedrock) may be supplied.
    pub base_url: String,
    pub model: String,
    /// Whether an API key has been stored in the keychain for this backend.
    pub has_api_key: bool,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            backend: LlmBackend::OpenAiCompatible,
            base_url: String::new(),
            model: String::new(),
            has_api_key: false,
        }
    }
}

/// A single chat message in the conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: ChatRole::User, content: content.into() }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: ChatRole::Assistant, content: content.into() }
    }
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: ChatRole::System, content: content.into() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

impl ChatRole {
    /// The wire name used by OpenAI/Anthropic for this role.
    pub fn as_str(self) -> &'static str {
        match self {
            ChatRole::System => "system",
            ChatRole::User => "user",
            ChatRole::Assistant => "assistant",
        }
    }
}

/// A description of the execution environment a proposed command will run in,
/// injected into the system prompt so the assistant generates platform- and
/// shell-appropriate commands. For local sessions this mirrors the host; for
/// SSH sessions it reflects the remote host (host label + probed os/arch/shell).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionEnvironment {
    /// OS name: `macOS`, `Linux`, `Windows`, or a remote `uname -s` result.
    pub os: String,
    /// Architecture: `aarch64`, `x86_64`, or a remote `uname -m` result.
    pub arch: String,
    /// Shell: `zsh`, `bash`, or a remote `$SHELL` basename.
    pub shell: String,
    /// Current working directory (may be empty/`?` when unknown).
    pub cwd: String,
    /// Remote target label `user@host`; `None` for a local session.
    pub host: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_config_default_is_openai_no_key() {
        let c = LlmConfig::default();
        assert_eq!(c.backend, LlmBackend::OpenAiCompatible);
        assert!(c.base_url.is_empty());
        assert!(c.model.is_empty());
        assert!(!c.has_api_key);
    }

    #[test]
    fn backend_serde_roundtrips() {
        for b in [LlmBackend::OpenAiCompatible, LlmBackend::Anthropic] {
            let json = serde_json::to_string(&b).unwrap();
            let back: LlmBackend = serde_json::from_str(&json).unwrap();
            assert_eq!(back, b);
        }
    }

    #[test]
    fn llm_config_serde_ignores_missing_fields() {
        // Old settings.json without llm → default.
        let cfg: LlmConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.backend, LlmBackend::OpenAiCompatible);
        assert!(!cfg.has_api_key);
    }
}
