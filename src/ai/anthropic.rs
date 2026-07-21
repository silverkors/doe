//! Anthropic Messages-API provider (`POST /v1/messages`, `x-api-key`, SSE).
//! Serves the `chat` capability. Default model is Claude Opus 4.8; any model
//! (e.g. `claude-fable-5` for hard code tasks) is selectable per request.

use crate::ai::{http, sse, AiRequest, ChunkSink, Payload, Provider};
use reqwest::blocking::Client;
use serde_json::json;

pub struct AnthropicProvider {
    id: String,
    base_url: String,
    api_key: String,
    model: String,
    caps: Vec<String>,
    client: Client,
}

impl AnthropicProvider {
    pub fn new(id: String, base_url: String, api_key: String, model: String, caps: Vec<String>) -> AnthropicProvider {
        AnthropicProvider { id, base_url, api_key, model, caps, client: Client::new() }
    }
}

impl Provider for AnthropicProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn supports(&self, capability: &str) -> bool {
        self.caps.iter().any(|c| c == capability)
    }

    fn dispatch(&self, req: AiRequest, sink: ChunkSink) {
        let Payload::Chat { system, messages, .. } = req.payload else {
            sink.error("anthropic provider only serves chat requests");
            return;
        };
        let model = req.model.unwrap_or_else(|| self.model.clone());
        let msgs: Vec<_> =
            messages.iter().map(|m| json!({ "role": m.role, "content": m.content })).collect();
        let mut body = json!({
            "model": model,
            "max_tokens": 4096,
            "stream": true,
            "messages": msgs,
        });
        if let Some(sys) = system {
            body["system"] = json!(sys);
        }

        let request = self
            .client
            .post(format!("{}/v1/messages", self.base_url.trim_end_matches('/')))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body);
        http::stream(request, &sink, sse::anthropic_delta);
    }
}
