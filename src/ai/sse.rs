//! Minimal Server-Sent Events reader plus the per-provider delta parsers.
//!
//! Both the Anthropic Messages stream and the OpenAI-compatible
//! `chat/completions` stream are SSE: newline-framed records whose payload
//! lines start with `data:`. [`read_sse`] handles the framing; the
//! `*_delta` functions map one payload to a [`Chunk`] and are pure, so the
//! streaming logic is unit-testable without a network.

use crate::ai::Chunk;
use std::io::BufRead;

/// Read SSE records from `r`, invoking `on_data` with the text after each
/// `data:` prefix. `on_data` returns `false` to stop early (terminal marker
/// seen). Blank lines and non-`data:` fields (`event:`, `:` comments) are
/// skipped. Runs until EOF or an early stop; I/O errors propagate.
pub fn read_sse<R: BufRead>(mut r: R, mut on_data: impl FnMut(&str) -> bool) -> std::io::Result<()> {
    let mut line = String::new();
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if let Some(rest) = trimmed.strip_prefix("data:") {
            if !on_data(rest.trim_start()) {
                break;
            }
        }
    }
    Ok(())
}

/// The outcome of parsing one SSE payload: keep streaming, stop cleanly, or a
/// chunk to forward. `Ignore` covers control frames (message_start, pings).
pub enum Delta {
    /// A chunk to forward to the sink.
    Emit(Chunk),
    /// A recognised end-of-stream sentinel (OpenAI's `[DONE]`).
    Stop,
    /// A frame we don't care about — keep reading.
    Ignore,
}

/// Parse one Anthropic Messages-API SSE payload. Emits text for
/// `content_block_delta`/`text_delta`, an error for an `error` frame, and
/// ignores the structural frames.
pub fn anthropic_delta(payload: &str) -> Delta {
    let v: serde_json::Value = match serde_json::from_str(payload) {
        Ok(v) => v,
        Err(_) => return Delta::Ignore,
    };
    match v.get("type").and_then(|t| t.as_str()) {
        Some("content_block_delta") => match v.pointer("/delta/text").and_then(|t| t.as_str()) {
            Some(text) if !text.is_empty() => Delta::Emit(Chunk::Text(text.to_string())),
            _ => Delta::Ignore,
        },
        Some("error") => {
            let msg = v
                .pointer("/error/message")
                .and_then(|m| m.as_str())
                .unwrap_or("anthropic stream error");
            Delta::Emit(Chunk::Error(msg.to_string()))
        }
        Some("message_stop") => Delta::Stop,
        _ => Delta::Ignore,
    }
}

/// Parse one OpenAI-compatible `chat/completions` SSE payload. `[DONE]` is the
/// stop sentinel; otherwise emit `choices[0].delta.content`.
pub fn openai_delta(payload: &str) -> Delta {
    if payload == "[DONE]" {
        return Delta::Stop;
    }
    let v: serde_json::Value = match serde_json::from_str(payload) {
        Ok(v) => v,
        Err(_) => return Delta::Ignore,
    };
    // An error object can arrive mid-stream on some gateways (OpenRouter).
    if let Some(msg) = v.pointer("/error/message").and_then(|m| m.as_str()) {
        return Delta::Emit(Chunk::Error(msg.to_string()));
    }
    match v.pointer("/choices/0/delta/content").and_then(|c| c.as_str()) {
        Some(text) if !text.is_empty() => Delta::Emit(Chunk::Text(text.to_string())),
        _ => Delta::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn read_sse_collects_data_lines() {
        let body = "event: start\ndata: one\n\ndata: two\n: comment\ndata: [DONE]\n";
        let mut got = Vec::new();
        read_sse(Cursor::new(body), |d| {
            got.push(d.to_string());
            d != "[DONE]"
        })
        .unwrap();
        assert_eq!(got, vec!["one", "two", "[DONE]"]);
    }

    #[test]
    fn anthropic_delta_extracts_text_and_errors() {
        let p = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}"#;
        assert!(matches!(anthropic_delta(p), Delta::Emit(Chunk::Text(t)) if t == "hi"));
        assert!(matches!(anthropic_delta(r#"{"type":"message_stop"}"#), Delta::Stop));
        assert!(matches!(anthropic_delta(r#"{"type":"message_start"}"#), Delta::Ignore));
        let err = r#"{"type":"error","error":{"type":"overloaded_error","message":"busy"}}"#;
        assert!(matches!(anthropic_delta(err), Delta::Emit(Chunk::Error(m)) if m == "busy"));
    }

    #[test]
    fn openai_delta_extracts_text_and_done() {
        let p = r#"{"choices":[{"delta":{"content":"yo"}}]}"#;
        assert!(matches!(openai_delta(p), Delta::Emit(Chunk::Text(t)) if t == "yo"));
        assert!(matches!(openai_delta("[DONE]"), Delta::Stop));
        // role-only opening delta has no content.
        assert!(matches!(openai_delta(r#"{"choices":[{"delta":{"role":"assistant"}}]}"#), Delta::Ignore));
        let err = r#"{"error":{"message":"bad key"}}"#;
        assert!(matches!(openai_delta(err), Delta::Emit(Chunk::Error(m)) if m == "bad key"));
    }
}
