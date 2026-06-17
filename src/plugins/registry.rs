//! Plugin registry: owns the loaded plugins, fans editor events out to them,
//! collects their status-bar contributions and resolves command aliases.

use super::api::{Event, Plugin, PluginView};
use std::collections::HashMap;

pub struct PluginRegistry {
    plugins: Vec<Box<dyn Plugin>>,
    command_aliases: HashMap<String, String>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        PluginRegistry { plugins: Vec::new(), command_aliases: HashMap::new() }
    }

    /// Register the built-in plugins shipped with DOE.
    pub fn with_builtins() -> Self {
        let mut reg = PluginRegistry::new();
        reg.register(Box::new(super::builtins::WordCountPlugin::default()));
        reg
    }

    pub fn register(&mut self, plugin: Box<dyn Plugin>) {
        for (alias, cmd) in plugin.commands() {
            self.command_aliases.insert(alias, cmd);
        }
        self.plugins.push(plugin);
    }

    /// Load every `*.wasm` in `dir` as a sandboxed plugin. Missing dir is fine;
    /// a module that fails to load is skipped (its error returned for logging)
    /// rather than aborting the others. Returns `(loaded, errors)`.
    pub fn load_wasm_dir(&mut self, dir: &std::path::Path) -> (usize, Vec<String>) {
        let mut loaded = 0;
        let mut errors = Vec::new();
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return (0, errors),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("wasm") {
                continue;
            }
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("plugin").to_string();
            match std::fs::read(&path) {
                Ok(bytes) => match super::wasm::WasmPlugin::load(&file_name, &bytes) {
                    Ok(p) => {
                        self.register(Box::new(p));
                        loaded += 1;
                    }
                    Err(e) => errors.push(format!("{file_name}: {e}")),
                },
                Err(e) => errors.push(format!("{file_name}: {e}")),
            }
        }
        (loaded, errors)
    }

    pub fn dispatch(&mut self, event: &Event) {
        for p in &mut self.plugins {
            p.on_event(event);
        }
    }

    pub fn status_segments(&self, view: &PluginView) -> Vec<String> {
        self.plugins
            .iter()
            .filter_map(|p| p.status_segment(view))
            .collect()
    }

    /// Resolve a plugin-provided command alias to its underlying command string.
    pub fn resolve_alias(&self, name: &str) -> Option<&str> {
        self.command_aliases.get(name).map(|s| s.as_str())
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}
