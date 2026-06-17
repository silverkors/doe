//! Plugin system. The MVP ships an internal Rust plugin API plus a registry;
//! the same event/view/command contract is intended to back sandboxed external
//! (e.g. WASM) plugins in a later version without reworking the editor core.

pub mod api;
pub mod builtins;
pub mod registry;
pub mod wasm;

pub use api::{Event, PluginView};
pub use registry::PluginRegistry;
