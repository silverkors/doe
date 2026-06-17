//! Internal plugin API. This is the MVP surface: plugins are compiled-in Rust
//! types implementing [`Plugin`]. The shapes here (events, a read view of the
//! buffer, command registration, status contributions) are deliberately the
//! same ones an external/WASM plugin host would expose later, so moving to
//! out-of-process sandboxed plugins is an implementation swap, not a redesign.
//!
//! Several fields/methods here are part of the plugin-facing contract and are
//! not all consumed by the built-in plugins yet.
#![allow(dead_code)]

use ropey::Rope;
use std::path::{Path, PathBuf};

/// Editor lifecycle events plugins can react to.
#[derive(Debug, Clone)]
pub enum Event {
    OpenFile(PathBuf),
    SaveFile(PathBuf),
    BufferChange,
    CursorMove,
    Command(String),
    Exit,
}

/// A read-only view of editor state passed to plugins. Holds borrows rather
/// than copies so plugins stay cheap even on large files; anything O(n) is the
/// plugin's responsibility to bound.
pub struct PluginView<'a> {
    pub rope: &'a Rope,
    pub cursor_line: usize,
    pub cursor_col: usize,
    pub selection: Option<(usize, usize)>,
    pub language: &'a str,
    pub path: Option<&'a Path>,
}

/// A compiled-in plugin. Future external plugins will implement the same
/// contract across an FFI/WASM boundary.
pub trait Plugin {
    fn name(&self) -> &str;

    /// React to an editor event. Plugins keep their own state.
    fn on_event(&mut self, _event: &Event) {}

    /// Optional status-bar segment (right-aligned group).
    fn status_segment(&self, _view: &PluginView) -> Option<String> {
        None
    }

    /// Command aliases this plugin contributes: `(alias, command_string)`.
    /// The alias becomes available on the command line and for keybindings.
    fn commands(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    /// Install the current document so the plugin can read it (e.g. WASM
    /// `doe_read`) while handling the next event. No-op for plugins that don't.
    fn set_context(&mut self, _rope: &Rope) {}

    /// Take any status message the plugin asked to show during the last event.
    fn take_status(&mut self) -> Option<String> {
        None
    }
}
