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

    /// Fan an event out to every plugin, installing `rope` as the document they
    /// may read, and collect any status messages they asked to show.
    pub fn dispatch(&mut self, event: &Event, rope: &ropey::Rope) -> Vec<String> {
        let mut statuses = Vec::new();
        for p in &mut self.plugins {
            p.set_context(rope);
            p.on_event(event);
            if let Some(s) = p.take_status() {
                statuses.push(s);
            }
        }
        statuses
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
