//! AI provider layer. DOE talks to language/vision models through a registry
//! of *named provider instances*, each declaring which **capabilities** it can
//! serve (`chat`, `ocr`, `image`, `embed`, …). Callers — palette commands or
//! plugins — pick a provider either by name (`"openai"`) or by capability, in
//! which case the user's configured default for that capability is resolved.
//! The same provider *type* can appear under several instance names (a local
//! Ollama plus a remote one, two OpenAI keys, …).
//!
//! Capabilities are **open strings**, not a fixed enum: a plugin can request a
//! capability DOE's core has never heard of, as long as some configured
//! provider claims to serve it.
//!
//! Model calls are network I/O measured in seconds and must never touch the
//! render loop, so a request is *submitted* to a [`Dispatcher`] that runs it on
//! a background thread and streams [`Chunk`]s back over a channel, tagged with a
//! request id. The editor drains that channel each tick — the same shape the
//! WASM host uses to hand results to a plugin (`request_id` + chunk).
//!
//! This module is the provider-agnostic core (traits, registry, dispatcher).
//! Concrete providers (Anthropic, OpenAI-compatible, Mistral) live alongside it
//! and implement [`Provider`]. Several items are part of the caller-facing
//! contract and are not all consumed yet.
#![allow(dead_code)]

pub mod anthropic;
pub mod config;
pub mod http;
pub mod openai;
pub mod presets;
pub mod sse;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;

/// A capability name. Deliberately an open string so plugins can introduce new
/// capabilities without a core code change. The constants in [`cap`] name the
/// ones DOE ships with.
pub type Capability = String;

/// Capability names DOE knows about out of the box. Providers and callers are
/// free to use others.
pub mod cap {
    pub const CHAT: &str = "chat";
    pub const OCR: &str = "ocr";
    pub const IMAGE: &str = "image";
    pub const EMBED: &str = "embed";
}

/// One turn in a chat request.
#[derive(Debug, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

/// The actual work a request carries. Chat is the phase-1 focus; the others
/// are part of the contract so providers and the dispatcher don't need
/// reshaping when image/OCR/embedding providers land.
#[derive(Debug, Clone)]
pub enum Payload {
    Chat { system: Option<String>, messages: Vec<Message>, stream: bool },
    Ocr { data: Vec<u8> },
    Image { prompt: String },
    Embed { text: String },
}

impl Payload {
    /// The capability this payload implies, used when the caller doesn't set
    /// one explicitly.
    pub fn implied_capability(&self) -> &'static str {
        match self {
            Payload::Chat { .. } => cap::CHAT,
            Payload::Ocr { .. } => cap::OCR,
            Payload::Image { .. } => cap::IMAGE,
            Payload::Embed { .. } => cap::EMBED,
        }
    }
}

/// Where the caller wants the streamed output to land. The provider is
/// oblivious to this — it just streams [`Chunk`]s; the editor routes them by
/// the sink recorded on the request. Both targets are supported so a plugin can
/// choose per request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sink {
    /// Splice output straight into the active document at the cursor.
    Buffer,
    /// Stream into a preview/output card (like computed-output cards) so the
    /// document isn't touched until the user accepts.
    Card,
}

/// A request to run one AI operation.
#[derive(Debug, Clone)]
pub struct AiRequest {
    /// Explicit provider instance name. When `None`, the capability's
    /// configured default provider is used.
    pub provider: Option<String>,
    /// Capability to serve. When empty, [`Payload::implied_capability`] is used.
    pub capability: Capability,
    /// Model override. When `None`, the provider instance's default model runs.
    pub model: Option<String>,
    /// Where streamed output should go.
    pub sink: Sink,
    pub payload: Payload,
}

impl AiRequest {
    /// The capability to resolve on: the explicit one, or the payload's implied
    /// capability when the caller left it empty.
    pub fn effective_capability(&self) -> &str {
        if self.capability.is_empty() {
            self.payload.implied_capability()
        } else {
            &self.capability
        }
    }
}

/// A streamed piece of a response. A well-behaved provider emits zero or more
/// content chunks and then exactly one terminal [`Chunk::Done`] or
/// [`Chunk::Error`].
#[derive(Debug, Clone)]
pub enum Chunk {
    /// Incremental text (a token or a span of tokens).
    Text(String),
    /// A generated image, as encoded bytes (PNG/JPEG).
    Image(Vec<u8>),
    /// A structured JSON payload (e.g. OCR result, embedding vector).
    Json(String),
    /// Terminal success marker.
    Done,
    /// Terminal failure with a human-readable reason.
    Error(String),
}

/// A tagged output channel handed to a provider. The provider streams by
/// calling [`ChunkSink::send`]; the id that reaches the editor is filled in
/// here, so the provider never sees it.
#[derive(Clone)]
pub struct ChunkSink {
    id: u64,
    tx: Sender<(u64, Chunk)>,
}

impl ChunkSink {
    /// Deliver one chunk. Errors (a dropped receiver — editor shutting down)
    /// are swallowed: there's nothing useful a provider can do about them.
    pub fn send(&self, chunk: Chunk) {
        let _ = self.tx.send((self.id, chunk));
    }

    /// Convenience: stream some text.
    pub fn text(&self, s: impl Into<String>) {
        self.send(Chunk::Text(s.into()));
    }

    /// Convenience: signal successful completion.
    pub fn done(&self) {
        self.send(Chunk::Done);
    }

    /// Convenience: signal failure and finish.
    pub fn error(&self, msg: impl Into<String>) {
        self.send(Chunk::Error(msg.into()));
    }
}

/// A named model backend. Implementations perform network I/O and therefore run
/// on a background thread — [`Provider::dispatch`] is expected to block that
/// thread while streaming and to be cheap to `Arc`-share (`Send + Sync`).
pub trait Provider: Send + Sync {
    /// The instance name this provider is registered under (`"anthropic"`,
    /// `"ollama"`, …). Unique within a registry.
    fn id(&self) -> &str;

    /// Whether this instance serves the given capability.
    fn supports(&self, capability: &str) -> bool;

    /// Run the request, streaming results to `sink`. Runs on a background
    /// thread; may block. Must finish with `Done` or `Error`.
    fn dispatch(&self, req: AiRequest, sink: ChunkSink);
}

/// Why a request couldn't be started. Dispatch-time failures (network, etc.)
/// arrive as [`Chunk::Error`] instead — this is only for resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiError {
    /// A provider name was given but no instance is registered under it.
    UnknownProvider(String),
    /// The named provider exists but doesn't serve the requested capability.
    Unsupported { provider: String, capability: String },
    /// No provider name given and no default is configured for the capability.
    NoDefault(String),
}

impl std::fmt::Display for AiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AiError::UnknownProvider(p) => write!(f, "no AI provider named '{p}'"),
            AiError::Unsupported { provider, capability } => {
                write!(f, "provider '{provider}' does not support '{capability}'")
            }
            AiError::NoDefault(c) => write!(f, "no default AI provider for '{c}'"),
        }
    }
}

/// The set of configured provider instances plus the per-capability default
/// mapping. Resolution is pure and cheap; it's the [`Dispatcher`] that spawns
/// threads.
#[derive(Default)]
pub struct AiRegistry {
    providers: Vec<Arc<dyn Provider>>,
    /// capability -> default provider instance name.
    defaults: HashMap<String, String>,
}

impl AiRegistry {
    pub fn new() -> AiRegistry {
        AiRegistry::default()
    }

    /// Register a provider instance. Later registrations under the same id
    /// shadow earlier ones (last wins).
    pub fn register(&mut self, provider: Arc<dyn Provider>) {
        self.providers.push(provider);
    }

    /// Set the default provider instance for a capability.
    pub fn set_default(&mut self, capability: impl Into<String>, provider: impl Into<String>) {
        self.defaults.insert(capability.into(), provider.into());
    }

    fn by_id(&self, id: &str) -> Option<&Arc<dyn Provider>> {
        // Last-wins: search from the end so a re-registered id shadows.
        self.providers.iter().rev().find(|p| p.id() == id)
    }

    /// Resolve a request to a concrete provider. Explicit `provider` wins; on a
    /// name it must also serve the capability. Otherwise the capability's
    /// configured default is used.
    pub fn resolve(&self, req: &AiRequest) -> Result<Arc<dyn Provider>, AiError> {
        let capability = req.effective_capability();
        if let Some(name) = &req.provider {
            let p = self.by_id(name).ok_or_else(|| AiError::UnknownProvider(name.clone()))?;
            if !p.supports(capability) {
                return Err(AiError::Unsupported {
                    provider: name.clone(),
                    capability: capability.to_string(),
                });
            }
            return Ok(p.clone());
        }
        let name = self
            .defaults
            .get(capability)
            .ok_or_else(|| AiError::NoDefault(capability.to_string()))?;
        // A default pointing at an unsupported/absent provider is a config
        // error surfaced the same way as an explicit bad name.
        let p = self.by_id(name).ok_or_else(|| AiError::UnknownProvider(name.clone()))?;
        if !p.supports(capability) {
            return Err(AiError::Unsupported {
                provider: name.clone(),
                capability: capability.to_string(),
            });
        }
        Ok(p.clone())
    }
}

/// Submits AI requests off the render loop. Owns the registry and hands every
/// caller a rising request id; results stream back on the single [`Receiver`]
/// returned by [`Dispatcher::new`], tagged with that id.
pub struct Dispatcher {
    registry: Arc<AiRegistry>,
    tx: Sender<(u64, Chunk)>,
    next_id: AtomicU64,
}

impl Dispatcher {
    /// Build a dispatcher and the channel the editor drains each tick.
    pub fn new(registry: AiRegistry) -> (Dispatcher, Receiver<(u64, Chunk)>) {
        let (tx, rx) = mpsc::channel();
        let d = Dispatcher { registry: Arc::new(registry), tx, next_id: AtomicU64::new(1) };
        (d, rx)
    }

    /// Resolve and start a request on a background thread. Returns the id that
    /// will tag every chunk of this request's output. Resolution failures are
    /// returned synchronously (fail fast); runtime failures arrive as
    /// [`Chunk::Error`] on the channel.
    pub fn submit(&self, req: AiRequest) -> Result<u64, AiError> {
        let provider = self.registry.resolve(&req)?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let sink = ChunkSink { id, tx: self.tx.clone() };
        thread::spawn(move || provider.dispatch(req, sink));
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A provider that immediately echoes its chat text back, for testing the
    /// registry and dispatcher without touching the network.
    struct Echo {
        id: String,
        caps: Vec<String>,
    }

    impl Echo {
        fn new(id: &str, caps: &[&str]) -> Arc<dyn Provider> {
            Arc::new(Echo {
                id: id.to_string(),
                caps: caps.iter().map(|s| s.to_string()).collect(),
            })
        }
    }

    impl Provider for Echo {
        fn id(&self) -> &str {
            &self.id
        }
        fn supports(&self, capability: &str) -> bool {
            self.caps.iter().any(|c| c == capability)
        }
        fn dispatch(&self, req: AiRequest, sink: ChunkSink) {
            if let Payload::Chat { messages, .. } = &req.payload {
                for m in messages {
                    sink.text(m.content.clone());
                }
            }
            sink.done();
        }
    }

    fn chat(provider: Option<&str>, capability: &str, text: &str) -> AiRequest {
        AiRequest {
            provider: provider.map(|s| s.to_string()),
            capability: capability.to_string(),
            model: None,
            sink: Sink::Card,
            payload: Payload::Chat {
                system: None,
                messages: vec![Message { role: "user".into(), content: text.into() }],
                stream: true,
            },
        }
    }

    fn registry() -> AiRegistry {
        let mut r = AiRegistry::new();
        r.register(Echo::new("anthropic", &[cap::CHAT]));
        r.register(Echo::new("openai", &[cap::CHAT, cap::IMAGE]));
        r.register(Echo::new("mistral-ocr", &[cap::OCR, cap::CHAT]));
        r.set_default(cap::CHAT, "anthropic");
        r.set_default(cap::OCR, "mistral-ocr");
        r
    }

    #[test]
    fn resolves_by_explicit_name() {
        let r = registry();
        let p = r.resolve(&chat(Some("openai"), cap::CHAT, "hi")).unwrap();
        assert_eq!(p.id(), "openai");
    }

    #[test]
    fn resolves_by_capability_default() {
        let r = registry();
        // No provider name → chat default is anthropic.
        let p = r.resolve(&chat(None, cap::CHAT, "hi")).unwrap();
        assert_eq!(p.id(), "anthropic");
    }

    #[test]
    fn empty_capability_falls_back_to_payload() {
        let r = registry();
        // capability left empty → implied from the chat payload → chat default.
        let p = r.resolve(&chat(None, "", "hi")).unwrap();
        assert_eq!(p.id(), "anthropic");
    }

    #[test]
    fn unknown_name_and_unsupported_capability_error() {
        let r = registry();
        assert_eq!(
            r.resolve(&chat(Some("nope"), cap::CHAT, "hi")).err(),
            Some(AiError::UnknownProvider("nope".into()))
        );
        // anthropic exists but doesn't do OCR.
        assert_eq!(
            r.resolve(&chat(Some("anthropic"), cap::OCR, "hi")).err(),
            Some(AiError::Unsupported { provider: "anthropic".into(), capability: "ocr".into() })
        );
        // image has no configured default.
        assert_eq!(
            r.resolve(&chat(None, cap::IMAGE, "hi")).err(),
            Some(AiError::NoDefault("image".into()))
        );
    }

    #[test]
    fn last_registration_shadows_same_id() {
        let mut r = AiRegistry::new();
        r.register(Echo::new("x", &[cap::CHAT]));
        r.register(Echo::new("x", &[cap::CHAT, cap::IMAGE]));
        // The second "x" (which also does image) wins.
        assert!(r.resolve(&chat(Some("x"), cap::IMAGE, "hi")).is_ok());
    }

    #[test]
    fn dispatcher_streams_tagged_chunks() {
        let (d, rx) = Dispatcher::new(registry());
        let id = d.submit(chat(Some("anthropic"), cap::CHAT, "hello")).unwrap();
        // Collect until the terminal marker.
        let mut text = String::new();
        let mut done = false;
        for (rid, chunk) in rx.iter() {
            assert_eq!(rid, id);
            match chunk {
                Chunk::Text(t) => text.push_str(&t),
                Chunk::Done => {
                    done = true;
                    break;
                }
                Chunk::Error(e) => panic!("unexpected error: {e}"),
                _ => {}
            }
        }
        assert!(done);
        assert_eq!(text, "hello");
    }

    #[test]
    fn dispatcher_rejects_unresolvable_before_spawn() {
        let (d, _rx) = Dispatcher::new(registry());
        assert_eq!(d.submit(chat(Some("nope"), cap::CHAT, "hi")), Err(AiError::UnknownProvider("nope".into())));
    }
}
