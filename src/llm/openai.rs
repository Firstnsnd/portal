//! # OpenAI-compatible backend
//!
//! Builds `POST /chat/completions` request bodies and parses `text/event-stream`
//! (`data: …` lines) responses. Pure functions — no I/O — so they are fully
//! unit-testable.

use super::types::ChatMessage;
use serde_json::{json, Value};

/// Assemble an OpenAI chat-completion request body with `stream: true`.
///
/// `messages` may include `System` / `User` / `Assistant` entries; all are sent
/// verbatim in the `messages` array in order.
pub fn build_body(model: &str, messages: &[ChatMessage]) -> Value {
    let msgs: Vec<Value> = messages
        .iter()
        .map(|m| json!({ "role": m.role.as_str(), "content": m.content }))
        .collect();
    json!({
        "model": model,
        "messages": msgs,
        "stream": true,
    })
}

/// Parse one or more SSE `data:` lines and return the delta text tokens.
///
/// The `[DONE]` sentinel ends a stream. Malformed / non-`content` events yield
/// no tokens (they are skipped, not an error). Empty input yields nothing.
pub fn parse_sse(data: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for line in data.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("data:") else { continue };
        let rest = rest.trim();
        if rest == "[DONE]" {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(rest) else { continue };
        // Standard shape: choices[0].delta.content
        if let Some(content) = v
            .pointer("/choices/0/delta/content")
            .and_then(Value::as_str)
        {
            tokens.push(content.to_string());
        }
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_body_emits_stream_and_roles() {
        let msgs = vec![
            ChatMessage::system("You are a shell assistant."),
            ChatMessage::user("hi"),
        ];
        let body = build_body("gpt-4o", &msgs);
        assert_eq!(body["model"], "gpt-4o");
        assert_eq!(body["stream"], true);
        let arr = body["messages"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["role"], "system");
        assert_eq!(arr[1]["role"], "user");
        assert_eq!(arr[1]["content"], "hi");
    }

    #[test]
    fn parse_sse_extracts_delta_content() {
        let data = "data: {\"choices\":[{\"delta\":{\"content\":\"he\"}}]}\n\
                    data: {\"choices\":[{\"delta\":{\"content\":\"llo\"}}]}\n\
                    data: [DONE]\n\n";
        assert_eq!(parse_sse(data), vec!["he", "llo"]);
    }

    #[test]
    fn parse_sse_skips_malformed_and_non_content() {
        let data = "data: {\"choices\":[{\"delta\":{}}]}\n\
                    data: not-json\n\
                    : keep-alive\n";
        assert!(parse_sse(data).is_empty());
    }

    #[test]
    fn parse_sse_handles_system_role_variants() {
        // A message role line (not delta.content) must not emit a token.
        let data = "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}";
        assert!(parse_sse(data).is_empty());
    }
}
