//! Built-in provider presets. A user config picks a `kind` (e.g. `"openai"`)
//! and only supplies the API key — base URL, default model, wire protocol, and
//! served capabilities come from here. `kind = "custom"` (or an unknown kind)
//! carries no defaults: the user fills in `base_url`, `model`, and `protocol`.

use crate::ai::cap;

/// Which wire protocol a provider speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Anthropic,
    OpenAi,
}

/// Baked-in defaults for a known provider kind.
#[derive(Debug, Clone)]
pub struct Preset {
    pub protocol: Protocol,
    pub base_url: &'static str,
    pub model: &'static str,
    pub capabilities: &'static [&'static str],
    /// Whether an API key is required (false for local servers).
    pub needs_key: bool,
    /// Conventional env var to read the key from when the config doesn't name one.
    pub key_env: &'static str,
}

/// Look up a preset by `kind`. Returns `None` for `"custom"`/unknown kinds,
/// which must be fully specified in config.
pub fn preset(kind: &str) -> Option<Preset> {
    let p = |protocol, base_url, model, capabilities, needs_key, key_env| {
        Some(Preset { protocol, base_url, model, capabilities, needs_key, key_env })
    };
    match kind {
        "anthropic" => p(
            Protocol::Anthropic,
            "https://api.anthropic.com",
            "claude-opus-4-8",
            &[cap::CHAT],
            true,
            "ANTHROPIC_API_KEY",
        ),
        "openai" => p(
            Protocol::OpenAi,
            "https://api.openai.com/v1",
            "gpt-4o",
            &[cap::CHAT],
            true,
            "OPENAI_API_KEY",
        ),
        "openrouter" => p(
            Protocol::OpenAi,
            "https://openrouter.ai/api/v1",
            "anthropic/claude-opus-4-8",
            &[cap::CHAT],
            true,
            "OPENROUTER_API_KEY",
        ),
        // xAI — Grok. `grok` accepted as an alias.
        "xai" | "grok" => p(
            Protocol::OpenAi,
            "https://api.x.ai/v1",
            "grok-4",
            &[cap::CHAT],
            true,
            "XAI_API_KEY",
        ),
        "groq" => p(
            Protocol::OpenAi,
            "https://api.groq.com/openai/v1",
            "llama-3.3-70b-versatile",
            &[cap::CHAT],
            true,
            "GROQ_API_KEY",
        ),
        "mistral" => p(
            Protocol::OpenAi,
            "https://api.mistral.ai/v1",
            "mistral-large-latest",
            &[cap::CHAT],
            true,
            "MISTRAL_API_KEY",
        ),
        // Local servers — no key, OpenAI-compatible endpoint.
        "ollama" => p(
            Protocol::OpenAi,
            "http://localhost:11434/v1",
            "llama3.1",
            &[cap::CHAT, cap::EMBED],
            false,
            "",
        ),
        "lmstudio" => p(
            Protocol::OpenAi,
            "http://localhost:1234/v1",
            "local-model",
            &[cap::CHAT],
            false,
            "",
        ),
        _ => None,
    }
}

/// The kinds shipped with DOE, for documentation and error messages.
pub const KNOWN_KINDS: &[&str] =
    &["anthropic", "openai", "openrouter", "xai", "grok", "groq", "mistral", "ollama", "lmstudio", "custom"];
