//! Declarative AI configuration parsed from `config.toml`'s `[ai]` table, and
//! the builder that turns it into a live [`AiRegistry`].
//!
//! ```toml
//! [ai.providers.claude]
//! kind = "anthropic"          # preset — base URL, model, protocol filled in
//! # api_key_env = "ANTHROPIC_API_KEY"   (default for this preset)
//! # model = "claude-fable-5"            (override the preset default)
//!
//! [ai.providers.router]
//! kind = "openrouter"
//! api_key = "sk-or-..."       # inline key instead of an env var
//!
//! [ai.providers.local]
//! kind = "ollama"             # no key needed
//! model = "qwen2.5-coder"
//!
//! [ai.providers.house]
//! kind = "custom"             # fill everything in yourself
//! protocol = "openai"
//! base_url = "https://llm.internal/v1"
//! model = "house-7b"
//! api_key_env = "HOUSE_KEY"
//! capabilities = ["chat"]
//!
//! [ai.defaults]
//! chat = "claude"
//! ```

use crate::ai::presets::{preset, Protocol};
use crate::ai::{anthropic::AnthropicProvider, openai::OpenAiProvider, AiRegistry, Provider};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

/// The `[ai]` table.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AiConfig {
    #[serde(default)]
    pub providers: HashMap<String, ProviderCfg>,
    /// capability name -> provider instance name.
    #[serde(default)]
    pub defaults: HashMap<String, String>,
}

/// One `[ai.providers.<name>]` entry. Only `kind` is required; everything else
/// falls back to the preset for that kind (or must be supplied for `custom`).
#[derive(Debug, Clone, Deserialize)]
pub struct ProviderCfg {
    pub kind: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub capabilities: Option<Vec<String>>,
    /// For `custom`: `"openai"` or `"anthropic"`. Ignored when a preset applies.
    #[serde(default)]
    pub protocol: Option<String>,
}

/// Wrapper so the `[ai]` table can be pulled out of the same `config.toml` the
/// other config structs are parsed from.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AiFile {
    #[serde(default)]
    pub ai: AiConfig,
}

/// Resolve an API key: an inline `api_key`, else `api_key_env`, else the
/// preset's conventional env var. Empty string means "no key".
fn resolve_key(cfg: &ProviderCfg, preset_env: &str) -> Option<String> {
    if let Some(k) = &cfg.api_key {
        return Some(k.clone());
    }
    let env = cfg.api_key_env.as_deref().filter(|s| !s.is_empty()).unwrap_or(preset_env);
    if env.is_empty() {
        return Some(String::new());
    }
    std::env::var(env).ok()
}

/// Build a registry from config. Providers whose key can't be resolved are
/// skipped with a warning rather than aborting the whole config, so a partly
/// configured `[ai]` block still yields a working editor.
pub fn build(cfg: &AiConfig) -> (AiRegistry, Vec<String>) {
    let mut registry = AiRegistry::new();
    let mut warnings = Vec::new();

    for (name, pc) in &cfg.providers {
        let preset = preset(&pc.kind);

        // Protocol: preset wins; else the explicit `protocol`; else assume OpenAI.
        let protocol = match &preset {
            Some(p) => p.protocol,
            None => match pc.protocol.as_deref() {
                Some("anthropic") => Protocol::Anthropic,
                _ => Protocol::OpenAi,
            },
        };

        let base_url = pc
            .base_url
            .clone()
            .or_else(|| preset.as_ref().map(|p| p.base_url.to_string()));
        let Some(base_url) = base_url else {
            warnings.push(format!("ai provider '{name}': kind '{}' needs a base_url", pc.kind));
            continue;
        };

        let model = pc
            .model
            .clone()
            .or_else(|| preset.as_ref().map(|p| p.model.to_string()));
        let Some(model) = model else {
            warnings.push(format!("ai provider '{name}': kind '{}' needs a model", pc.kind));
            continue;
        };

        let caps: Vec<String> = pc
            .capabilities
            .clone()
            .or_else(|| preset.as_ref().map(|p| p.capabilities.iter().map(|c| c.to_string()).collect()))
            .unwrap_or_else(|| vec![crate::ai::cap::CHAT.to_string()]);

        let preset_env = preset.as_ref().map(|p| p.key_env).unwrap_or("");
        let needs_key = preset.as_ref().map(|p| p.needs_key).unwrap_or(true);
        let key = match resolve_key(pc, preset_env) {
            Some(k) => k,
            None => {
                warnings.push(format!(
                    "ai provider '{name}': no API key (set api_key, api_key_env, or ${preset_env})"
                ));
                continue;
            }
        };
        if needs_key && key.is_empty() {
            warnings.push(format!("ai provider '{name}': API key is empty"));
            continue;
        }

        let provider: Arc<dyn Provider> = match protocol {
            Protocol::Anthropic => {
                Arc::new(AnthropicProvider::new(name.clone(), base_url, key, model, caps))
            }
            Protocol::OpenAi => {
                Arc::new(OpenAiProvider::new(name.clone(), base_url, key, model, caps))
            }
        };
        registry.register(provider);
    }

    for (capability, provider) in &cfg.defaults {
        registry.set_default(capability.clone(), provider.clone());
    }

    (registry, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{cap, AiRequest, Payload, Sink};

    fn parse(toml_src: &str) -> AiConfig {
        toml::from_str::<AiFile>(toml_src).unwrap().ai
    }

    fn chat_req(provider: Option<&str>) -> AiRequest {
        AiRequest {
            provider: provider.map(|s| s.to_string()),
            capability: cap::CHAT.to_string(),
            model: None,
            sink: Sink::Card,
            payload: Payload::Chat { system: None, messages: vec![], stream: true },
        }
    }

    #[test]
    fn preset_provider_with_inline_key_builds() {
        let cfg = parse(
            r#"
            [ai.providers.claude]
            kind = "anthropic"
            api_key = "sk-test"

            [ai.defaults]
            chat = "claude"
        "#,
        );
        let (registry, warnings) = build(&cfg);
        assert!(warnings.is_empty(), "{warnings:?}");
        let p = registry.resolve(&chat_req(Some("claude"))).unwrap();
        assert_eq!(p.id(), "claude");
        // Resolved via the chat default too.
        assert_eq!(registry.resolve(&chat_req(None)).unwrap().id(), "claude");
    }

    #[test]
    fn keyless_local_provider_builds() {
        let cfg = parse(
            r#"
            [ai.providers.local]
            kind = "ollama"
            model = "qwen2.5-coder"
        "#,
        );
        let (registry, warnings) = build(&cfg);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(registry.resolve(&chat_req(Some("local"))).is_ok());
    }

    #[test]
    fn missing_key_is_skipped_with_warning() {
        // No inline key and the env var is (almost certainly) unset in tests.
        let cfg = parse(
            r#"
            [ai.providers.oai]
            kind = "openai"
            api_key_env = "DOE_TEST_DEFINITELY_UNSET_KEY_XYZ"
        "#,
        );
        let (registry, warnings) = build(&cfg);
        assert_eq!(warnings.len(), 1);
        assert!(registry.resolve(&chat_req(Some("oai"))).is_err());
    }

    #[test]
    fn custom_provider_requires_base_and_model() {
        let cfg = parse(
            r#"
            [ai.providers.house]
            kind = "custom"
            protocol = "openai"
            api_key = "k"
        "#,
        );
        let (_registry, warnings) = build(&cfg);
        // Missing base_url is reported.
        assert!(warnings.iter().any(|w| w.contains("base_url")));
    }
}
