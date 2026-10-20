//! # Anthropic Messages backend
//!
//! Builds `POST /v1/messages` request bodies and parses the streamed
//! `content_block_delta` / `content_block_start` events. Pure functions.

use super::types::{ChatMessage, ChatRole};
use serde_json::{json, Value};

/// Assemble an Anthropic Messages request body with `stream: true`.
///
/// `System` messages are lifted into the top-level `system` prompt string;
/// only `User` / `Assistant` messages go into the `messages` array.
pub fn build_body(model: &str, messages: &[ChatMessage], max_tokens: u32) -> Value {
    let mut system: Vec<&str> = Vec::new();
    let msgs: Vec<Value> = messages
        .iter()
        .filter_map(|m| match m.role {
            ChatRole::System => {
                system.push(&m.content);
                None
            }
            _ => Some(json!({ "role": m.role.as_str(), "content": m.content })),
        })
        .collect();

    let mut body = json!({
        "model": model,
        "messages": msgs,
        "max_tokens": max_tokens,
        "stream": true,
    });
    if !system.is_empty() {
        body["system"] = Value::String(system.join("\n"));
    }
    body
}

/// Parse streamed Anthropic SSE `data:` events and return text deltas.
///
/// Text arrives in `content_block_delta` events under `delta.text` (the
/// `content_block_start` event carries no text; `message_delta` / `message_stop`
/// carry none either). The `event:` type line is ignored — we key purely off the
/// delta payload, matching the codebase's single-provider dispatch approach.
pub fn parse_sse(data: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for line in data.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("data:") else { continue };
        let rest = rest.trim();
        let Ok(v) = serde_json::from_str::<Value>(rest) else { continue };
        // Anthropic text streaming: type "content_block_delta" → delta.text.
        if let Some(text) = v.pointer("/delta/text").and_then(Value::as_str) {
            tokens.push(text.to_string());
        }
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_body_lifts_system_and_sets_stream() {
        let msgs = vec![
            ChatMessage::user("hello"),
            ChatMessage::assistant("hi"),
        ];
        let body = build_body("claude-sonnet", &msgs, 4096);
        assert_eq!(body["model"], "claude-sonnet");
        assert_eq!(body["max_tokens"], 4096);
        assert_eq!(body["stream"], true);
        let arr = body["messages"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        // no system key when none present
        assert!(body.get("system").is_none());
    }

    #[test]
    fn build_body_moves_system_out_of_messages() {
        let msgs = vec![
            ChatMessage::system("be terse"),
            ChatMessage::user("ls"),
        ];
        let body = build_body("claude", &msgs, 1024);
        assert_eq!(body["system"], "be terse");
        let arr = body["messages"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["role"], "user");
    }

    #[test]
    fn parse_sse_extracts_delta_text() {
        let data = "event: content_block_delta\n\
                    data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\
                    event: message_stop\n\
                    data: {\"type\":\"message_stop\"}\n";
        assert_eq!(parse_sse(data), vec!["hi"]);
    }

    #[test]
    fn parse_sse_ignores_non_delta_events() {
        let data = "data: {\"type\":\"content_block_start\",\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\
                    data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n";
        assert!(parse_sse(data).is_empty());
    }
}
