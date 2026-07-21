//! Shared streaming HTTP helper. Sends a prepared request, checks the status,
//! then feeds the response body through the SSE reader and a provider-specific
//! delta parser, forwarding chunks to the sink. Runs on the caller's
//! (background) thread and blocks until the stream ends.

use crate::ai::sse::{read_sse, Delta};
use crate::ai::{Chunk, ChunkSink};
use std::io::BufReader;

/// Send `req`, stream the SSE body, and forward parsed chunks to `sink`. Always
/// terminates the sink with exactly one `Done` or `Error`.
pub fn stream(req: reqwest::blocking::RequestBuilder, sink: &ChunkSink, parse: fn(&str) -> Delta) {
    let resp = match req.send() {
        Ok(r) => r,
        Err(e) => {
            sink.error(format!("request failed: {e}"));
            return;
        }
    };
    if !resp.status().is_success() {
        let code = resp.status();
        let body = resp.text().unwrap_or_default();
        sink.error(format!("HTTP {code}: {}", truncate(body.trim(), 300)));
        return;
    }

    let mut errored = false;
    let read = read_sse(BufReader::new(resp), |payload| match parse(payload) {
        Delta::Emit(Chunk::Error(m)) => {
            sink.error(m);
            errored = true;
            false
        }
        Delta::Emit(chunk) => {
            sink.send(chunk);
            true
        }
        Delta::Stop => false,
        Delta::Ignore => true,
    });
    if let Err(e) = read {
        if !errored {
            sink.error(format!("stream read error: {e}"));
            errored = true;
        }
    }
    if !errored {
        sink.done();
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}
