//! OpenAI-compatible chat provider (`POST {base}/chat/completions`, optional
//! `Authorization: Bearer`, SSE). One type covers OpenAI, OpenRouter, xAI
//! (Grok), Groq, Mistral chat, and local servers (Ollama, LM Studio) — they all
//! speak the same wire format; only `base_url`, key, and model differ.

use crate::ai::{http, sse, AiRequest, ChunkSink, Message, Payload, Provider};
use reqwest::blocking::Client;
use serde_json::json;

pub struct OpenAiProvider {
    id: String,
    base_url: String,
    /// Empty for keyless local servers (Ollama, LM Studio).
    api_key: String,
    model: String,
    caps: Vec<String>,
    client: Client,
}

impl OpenAiProvider {
    pub fn new(id: String, base_url: String, api_key: String, model: String, caps: Vec<String>) -> OpenAiProvider {
        OpenAiProvider { id, base_url, api_key, model, caps, client: Client::new() }
    }
}

impl Provider for OpenAiProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn supports(&self, capability: &str) -> bool {
        self.caps.iter().any(|c| c == capability)
    }

    fn dispatch(&self, req: AiRequest, sink: ChunkSink) {
        let Payload::Chat { system, messages, .. } = req.payload else {
            sink.error("openai-compatible provider only serves chat requests");
            return;
        };
        let model = req.model.unwrap_or_else(|| self.model.clone());
        // OpenAI folds the system prompt in as a leading system message.
        let mut all: Vec<Message> = Vec::with_capacity(messages.len() + 1);
        if let Some(sys) = system {
            all.push(Message { role: "system".into(), content: sys });
        }
        all.extend(messages);
        let msgs: Vec<_> =
            all.iter().map(|m| json!({ "role": m.role, "content": m.content })).collect();
        let body = json!({ "model": model, "stream": true, "messages": msgs });

        let mut request = self
            .client
            .post(format!("{}/chat/completions", self.base_url.trim_end_matches('/')))
            .header("content-type", "application/json")
            .json(&body);
        if !self.api_key.is_empty() {
            request = request.bearer_auth(&self.api_key);
        }
        http::stream(request, &sink, sse::openai_delta);
    }
}
