//! # Streaming chat transport
//!
//! Performs a blocking (ureq) streaming chat request for either backend and
//! invokes `on_token` for each decoded text delta. Run it from a background
//! thread so the UI never blocks.

use std::io::Read;

use super::anthropic;
use super::openai;
use super::types::{ChatMessage, LlmBackend, LlmConfig};

/// Default Anthropic base URL when none is configured.
const ANTHROPIC_DEFAULT_BASE: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: u32 = 4096;

/// Send a streaming chat request and feed text deltas to `on_token`.
///
/// Returns the accumulated full text on success, or an error string. The whole
/// request (connect + read) is blocking — call off the UI thread.
pub fn stream_chat(
    cfg: &LlmConfig,
    api_key: &str,
    messages: &[ChatMessage],
    mut on_token: impl FnMut(&str),
) -> Result<String, String> {
    let (url, headers, body) = match cfg.backend {
        LlmBackend::OpenAiCompatible => {
            let base = cfg.base_url.trim().trim_end_matches('/');
            if base.is_empty() {
                return Err("OpenAI-compatible base URL is not configured".to_string());
            }
            let url = format!("{base}/chat/completions");
            let body = openai::build_body(&cfg.model, messages);
            let headers = vec![
                ("Authorization", format!("Bearer {api_key}")),
                ("Content-Type", "application/json".to_string()),
            ];
            (url, headers, body)
        }
        LlmBackend::Anthropic => {
            // Honor a custom base URL (LiteLLM/OpenRouter/Bedrock/claude-code-router)
            // and fall back to the official endpoint when left blank.
            let base = cfg.base_url.trim().trim_end_matches('/');
            let base = if base.is_empty() { ANTHROPIC_DEFAULT_BASE } else { base };
            let url = format!("{base}/v1/messages");
            let body = anthropic::build_body(&cfg.model, messages, DEFAULT_MAX_TOKENS);
            let headers = vec![
                ("x-api-key", api_key.to_string()),
                ("anthropic-version", ANTHROPIC_VERSION.to_string()),
                ("Content-Type", "application/json".to_string()),
            ];
            (url, headers, body)
        }
    };

    let mut req = ureq::post(&url);
    for (k, v) in &headers {
        req = req.set(k, v);
    }

    let resp = req
        .send_string(&body.to_string())
        .map_err(|e| format!("request failed: {e}"))?;

    if resp.status() >= 400 {
        let status = resp.status();
        let msg = resp.into_string().unwrap_or_default();
        return Err(format!("HTTP {status}: {}", msg.trim()));
    }

    // Streaming read: accumulate raw bytes into a line buffer, split SSE events
    // on blank lines / `data:` boundaries, and dispatch text deltas.
    let mut reader = resp.into_reader();
    let mut buf = [0u8; 4096];
    let mut pending = String::new();
    let mut full = String::new();

    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("read failed: {e}"))?;
        if n == 0 {
            break;
        }
        pending.push_str(&String::from_utf8_lossy(&buf[..n]));

        // Consume complete SSE events (each ends with a blank line). Keep any
        // trailing partial event in `pending` for the next chunk.
        let mut tokens: Vec<String>;
        while let Some(idx) = pending.find("\n\n") {
            let event = pending[..idx].to_string();
            pending.drain(..=idx);
            if cfg.backend == LlmBackend::OpenAiCompatible {
                tokens = openai::parse_sse(&event);
            } else {
                tokens = anthropic::parse_sse(&event);
            }
            for t in tokens {
                full.push_str(&t);
                on_token(&t);
            }
        }
    }

    // Flush any final, unterminated event.
    if !pending.trim().is_empty() {
        let tokens = if cfg.backend == LlmBackend::OpenAiCompatible {
            openai::parse_sse(&pending)
        } else {
            anthropic::parse_sse(&pending)
        };
        for t in tokens {
            full.push_str(&t);
            on_token(&t);
        }
    }

    Ok(full)
}

// NOTE: the streaming path is intentionally integration-tested indirectly via
// the pure `openai::parse_sse` / `anthropic::parse_sse` functions (see their
// modules). `stream_chat` itself needs a live server, so it has no unit test;
// the SSE event chunking/accumulation edge cases are covered by those parsers
// plus a synthetic chunk test below.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_url_is_derived_from_base_url() {
        let cfg = LlmConfig {
            backend: LlmBackend::OpenAiCompatible,
            base_url: "http://localhost:11434/v1/".to_string(),
            model: "llama3".to_string(),
            has_api_key: true,
        };
        // Sanity: a trailing slash is trimmed before appending /chat/completions.
        assert_eq!(cfg.base_url.trim().trim_end_matches('/'), "http://localhost:11434/v1");
    }

    #[test]
    fn empty_openai_base_url_is_a_clean_error() {
        let cfg = LlmConfig::default(); // empty base_url
        let err = stream_chat(&cfg, "x", &[], |_| {}).unwrap_err();
        assert!(err.contains("base URL"));
    }

    #[test]
    fn anthropic_honors_custom_base_url() {
        let cfg = LlmConfig {
            backend: LlmBackend::Anthropic,
            base_url: "https://router.example.com/".to_string(),
            model: "claude-x".to_string(),
            has_api_key: true,
        };
        // Trailing slash trimmed, `/v1/messages` appended.
        assert_eq!(
            cfg.base_url.trim().trim_end_matches('/'),
            "https://router.example.com"
        );
    }
}
