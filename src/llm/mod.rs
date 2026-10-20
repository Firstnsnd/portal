//! # LLM integration
//!
//! Protocol-agnostic chat backends (OpenAI-compatible + Anthropic) with SSE
//! streaming. All network I/O is blocking (via `ureq`) and runs on a background
//! thread; the pure wire-format builders/parsers are unit-tested without any
//! network access.

pub mod types;

pub mod openai;
pub mod anthropic;
pub mod stream;
