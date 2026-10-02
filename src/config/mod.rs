//! Configuration loading. Settings, keybindings and the active theme are read
//! from `<config>/doe/config.toml` (Linux/macOS: `~/.config/doe`, Windows:
//! `%APPDATA%/doe`). Everything has sensible built-in defaults, so DOE runs
//! fine with no config file at all; on first run a documented default config
//! and theme are written for the user to edit.

pub mod callouts;
pub mod obsidian;
pub mod theme;

use callouts::Callouts;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use theme::Theme;

#[derive(Debug, Clone)]
pub struct Config {
    pub settings: Settings,
    pub keybindings: Keybindings,
    pub theme: Theme,
    pub callouts: Callouts,
    /// Base config directory; used for theme loading and recovery/usage stores.
    pub config_dir: PathBuf,
    /// The user's own keybinding overrides (raw, for round-tripping on save).
    user_keybindings: HashMap<String, HashMap<String, String>>,
    /// Problems found while loading `config.toml`, shown once at startup.
    pub load_warning: Option<String>,
    /// True when `config.toml` couldn't be parsed at all; `save` then refuses
    /// to overwrite it with defaults.
    unparseable: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    pub theme: String,
    pub line_numbers: bool,
    pub relative_line_numbers: bool,
    pub tab_width: usize,
    pub insert_spaces: bool,
    pub mouse: bool,
    pub syntax_highlighting: bool,
    pub soft_wrap: bool,
    pub show_whitespace: bool,
    pub trim_trailing_whitespace_on_save: bool,
    /// Render Markdown callouts in a decorated form when the cursor is not on
    /// them (the cursor's own callout shows raw source for editing).
    pub render_callouts: bool,
    /// Drag (one-finger touch) scrolls the document instead of selecting text.
    /// Useful over SSH/tmux on touch clients (e.g. Termius).
    pub touch_scroll: bool,
    /// Show the tab-stop ruler row above the text area (click to set stops,
    /// click a stop to cycle its alignment, right-click to remove).
    pub show_tab_ruler: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: "default-dark".to_string(),
            line_numbers: true,
            relative_line_numbers: false,
            tab_width: 4,
            insert_spaces: true,
            mouse: true,
            syntax_highlighting: true,
            soft_wrap: true,
            show_whitespace: false,
            trim_trailing_whitespace_on_save: false,
            render_callouts: true,
            touch_scroll: false,
            show_tab_ruler: false,
        }
    }
}

/// Keybindings per mode: mode name -> (chord -> command name).
#[derive(Debug, Clone, Default)]
pub struct Keybindings {
    pub modes: HashMap<String, HashMap<String, String>>,
}

impl Keybindings {
    pub fn get(&self, mode: &str, chord: &str) -> Option<&str> {
        self.modes.get(mode)?.get(chord).map(|s| s.as_str())
    }

    fn merge(&mut self, other: HashMap<String, HashMap<String, String>>) {
        for (mode, binds) in other {
            let entry = self.modes.entry(mode).or_default();
            for (chord, cmd) in binds {
                entry.insert(chord, cmd);
            }
        }
    }
}

/// Read settings and keybinding overrides key by key, so one bad value only
/// drops that key instead of resetting the whole file to defaults. Returns the
/// names of the keys that were ignored.
fn parse_lenient(mut table: toml::Table) -> (Settings, HashMap<String, HashMap<String, String>>, Vec<String>) {
    let mut bad = Vec::new();
    let mut keybindings: HashMap<String, HashMap<String, String>> = HashMap::new();
    match table.remove("keybindings") {
        Some(toml::Value::Table(modes)) => {
            for (mode, binds) in modes {
                let toml::Value::Table(binds) = binds else {
                    bad.push(format!("keybindings.{mode}"));
                    continue;
                };
                let entry = keybindings.entry(mode.clone()).or_default();
                for (chord, cmd) in binds {
                    match cmd {
                        toml::Value::String(cmd) => {
                            entry.insert(chord, cmd);
                        }
                        _ => bad.push(format!("keybindings.{mode}.{chord}")),
                    }
                }
            }
        }
        Some(_) => bad.push("keybindings".to_string()),
        None => {}
    }

    // Settings: try the whole table first (the common case), then fall back
    // to accepting keys one at a time.
    if let Ok(s) = Settings::deserialize(toml::Value::Table(table.clone())) {
        return (s, keybindings, bad);
    }
    let mut accepted = toml::Table::new();
    for (key, value) in table {
        let mut candidate = accepted.clone();
        candidate.insert(key.clone(), value);
        if Settings::deserialize(toml::Value::Table(candidate.clone())).is_ok() {
            accepted = candidate;
        } else {
            bad.push(key);
        }
    }
    let settings = Settings::deserialize(toml::Value::Table(accepted)).unwrap_or_default();
    (settings, keybindings, bad)
}

impl Config {
    /// Load configuration, applying defaults and writing a starter config on
    /// first run. Never fails: on any error it falls back to defaults.
    pub fn load() -> Config {
        let config_dir = config_base_dir();
        let config_path = config_dir.join("config.toml");

        let mut settings = Settings::default();
        let mut keybindings = default_keybindings();
        let mut user_keybindings: HashMap<String, HashMap<String, String>> = HashMap::new();

        let mut load_warning = None;
        let mut unparseable = false;
        if let Ok(text) = std::fs::read_to_string(&config_path) {
            match text.parse::<toml::Table>() {
                Ok(table) => {
                    let (s, kb, bad) = parse_lenient(table);
                    settings = s;
                    user_keybindings = kb.clone();
                    keybindings.merge(kb);
                    if !bad.is_empty() {
                        load_warning = Some(format!("config.toml: ignored invalid {}", bad.join(", ")));
                    }
                }
                Err(e) => {
                    unparseable = true;
                    let msg = e.message().to_string();
                    load_warning = Some(format!("config.toml has a syntax error ({msg}); using defaults, not saving"));
                }
            }
        } else {
            // First run: scaffold config + default theme (best effort).
            scaffold(&config_dir);
        }

        let theme = Theme::load(&settings.theme, &config_dir.join("themes"));
        let callouts = Callouts::load(&config_dir);

        Config { settings, keybindings, theme, callouts, config_dir, user_keybindings, load_warning, unparseable }
    }

    /// Persist callout styles to `callouts.toml`.
    pub fn save_callouts(&self) {
        self.callouts.save(&self.config_dir);
    }

    /// Reload the active theme after `settings.theme` changes.
    pub fn reload_theme(&mut self) {
        self.theme = Theme::load(&self.settings.theme, &self.config_dir.join("themes"));
    }

    /// Theme names available in the themes directory (always includes the
    /// built-in `default-dark`).
    pub fn available_themes(&self) -> Vec<String> {
        let mut themes = vec!["default-dark".to_string()];
        if let Ok(entries) = std::fs::read_dir(self.config_dir.join("themes")) {
            for e in entries.flatten() {
                if let Some(name) = e.path().file_stem().and_then(|s| s.to_str()) {
                    if name != "default-dark" {
                        themes.push(name.to_string());
                    }
                }
            }
        }
        themes
    }

    /// Persist settings (and the user's keybinding overrides) to `config.toml`.
    pub fn save(&self) {
        if self.unparseable {
            return; // don't clobber a file we couldn't read
        }
        let _ = std::fs::create_dir_all(&self.config_dir);
        let mut out = String::from(
            "# DOE configuration — editable here or via the in-editor settings panel (Ctrl+,)\n\n",
        );
        if let Ok(s) = toml::to_string(&self.settings) {
            out.push_str(&s);
        }
        for (mode, binds) in &self.user_keybindings {
            if binds.is_empty() {
                continue;
            }
            out.push_str(&format!("\n[keybindings.{mode}]\n"));
            let mut keys: Vec<_> = binds.iter().collect();
            keys.sort();
            for (chord, cmd) in keys {
                out.push_str(&format!("{chord:?} = {cmd:?}\n"));
            }
        }
        let _ = std::fs::write(self.config_dir.join("config.toml"), out);
    }
}

/// The DOE config directory. Per spec this is `~/.config/doe` on Linux/macOS
/// and `%APPDATA%/doe` on Windows (note: macOS `dirs::config_dir()` would give
/// `~/Library/Application Support`, which we deliberately don't use here).
/// `DOE_CONFIG_DIR` overrides it (useful for sandboxing or custom locations).
fn config_base_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("DOE_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    #[cfg(windows)]
    {
        dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("doe")
    }
    #[cfg(not(windows))]
    {
        dirs::home_dir()
            .map(|h| h.join(".config"))
            .unwrap_or_else(|| PathBuf::from(".config"))
            .join("doe")
    }
}

/// Built-in keybindings so DOE is usable out of the box. DOE is modeless, so
/// there is a single `global` binding context (no Vim-style modes).
pub fn default_keybindings() -> Keybindings {
    let binds: &[(&str, &str)] = &[
        ("ctrl-p", "command_palette"),
        ("ctrl-t", "open_buffers"),
        ("ctrl-,", "settings"),
        ("ctrl-1", "goto_buffer 1"),
        ("ctrl-2", "goto_buffer 2"),
        ("ctrl-3", "goto_buffer 3"),
        ("ctrl-4", "goto_buffer 4"),
        ("ctrl-5", "goto_buffer 5"),
        ("ctrl-6", "goto_buffer 6"),
        ("ctrl-7", "goto_buffer 7"),
        ("ctrl-8", "goto_buffer 8"),
        ("ctrl-9", "goto_buffer 9"),
        ("ctrl-s", "save"),
        ("ctrl-w", "close_buffer"),
        ("alt-enter", "run_code_block"),
        ("ctrl-q", "quit"),
        ("ctrl-z", "undo"),
        ("ctrl-y", "redo"),
        ("ctrl-f", "find"),
        ("ctrl-o", "open"),
        ("ctrl-h", "replace"),
        ("ctrl-a", "select_all"),
        ("ctrl-b", "toggle_bold"),
        ("ctrl-i", "toggle_italic"),
        ("ctrl-d", "add_cursor_next_match"),
        ("ctrl-l", "select_line"),
        ("alt-f3", "select_all_matches"),
        ("ctrl-slash", "toggle_comment"),
        ("alt-z", "toggle_soft_wrap"),
        ("ctrl-pageup", "prev_buffer"),
        ("ctrl-pagedown", "next_buffer"),
        ("alt-up", "add_cursor_above"),
        ("alt-down", "add_cursor_below"),
        ("alt-shift-up", "expand_selection"),
        ("alt-shift-down", "shrink_selection"),
        ("ctrl-shift-o", "go_to_symbol"),
        ("left", "move_left"),
        ("right", "move_right"),
        ("up", "move_up"),
        ("down", "move_down"),
        ("ctrl-left", "move_word_left"),
        ("ctrl-right", "move_word_right"),
        ("home", "move_line_start"),
        ("end", "move_line_end"),
        ("ctrl-home", "move_buffer_start"),
        ("ctrl-end", "move_buffer_end"),
        ("pageup", "page_up"),
        ("pagedown", "page_down"),
        ("shift-left", "extend_left"),
        ("shift-right", "extend_right"),
        ("shift-up", "extend_up"),
        ("shift-down", "extend_down"),
        ("f1", "help"),
        ("f3", "find_next"),
        ("shift-f3", "find_prev"),
        ("esc", "clear_extra_cursors"),
        ("enter", "newline"),
        ("backspace", "backspace"),
        ("delete", "delete"),
        // Delete previous/next word: Alt on mac, Ctrl on PC — both bound.
        ("alt-backspace", "delete_word_left"),
        ("ctrl-backspace", "delete_word_left"),
        ("alt-delete", "delete_word_right"),
        ("ctrl-delete", "delete_word_right"),
        ("tab", "tab"),
    ];

    let mut global: HashMap<String, String> = HashMap::new();
    for (k, v) in binds {
        global.insert(k.to_string(), v.to_string());
    }

    let mut modes: HashMap<String, HashMap<String, String>> = HashMap::new();
    modes.insert(crate::commands::BINDING_CONTEXT.to_string(), global);
    Keybindings { modes }
}

fn scaffold(config_dir: &std::path::Path) {
    let _ = std::fs::create_dir_all(config_dir.join("themes"));
    let cfg = config_dir.join("config.toml");
    if !cfg.exists() {
        let _ = std::fs::write(&cfg, DEFAULT_CONFIG_TOML);
    }
    let theme = config_dir.join("themes").join("default-dark.toml");
    if !theme.exists() {
        let _ = std::fs::write(&theme, DEFAULT_THEME_TOML);
    }
}

const DEFAULT_CONFIG_TOML: &str = r##"# DOE - David's Own Editor configuration
# Edit and restart DOE. All keys here are optional; defaults are used otherwise.

theme = "default-dark"
line_numbers = true
relative_line_numbers = false
tab_width = 4
insert_spaces = true
mouse = true
syntax_highlighting = true
soft_wrap = true
show_whitespace = false
trim_trailing_whitespace_on_save = false

# DOE is modeless: there is one keybinding context, [keybindings.global].
# Bindings here are merged over the built-in defaults, so only list overrides.
# Chord syntax examples:
#   "ctrl-s", "alt-up", "shift-f3", "ctrl-shift-k", "enter", "esc"
[keybindings.global]
"ctrl-p" = "command_palette"
"ctrl-s" = "save"
"ctrl-f" = "find"
"ctrl-d" = "add_cursor_next_match"
"ctrl-b" = "toggle_bold"
"ctrl-i" = "toggle_italic"
"##;

const DEFAULT_THEME_TOML: &str = r##"# DOE theme: default-dark
[colors]
background = "#101218"
foreground = "#d0d0d0"
statusbar = "#30303a"
selection = "#334466"
cursor = "#ffffff"
keyword = "#ffcc66"
string = "#99cc99"
comment = "#777777"
number = "#f99157"
heading = "#6699cc"
bold = "#ffffff"
italic = "#c8c8ff"
code = "#cca97a"
link = "#6cb6ff"
list_marker = "#ffcc66"
quote = "#99a0aa"
callout = "#9d7cd8"
tag = "#e06c75"
attribute = "#ffcc66"
markup_punct = "#606672"
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_bad_value_keeps_the_other_settings() {
        let table: toml::Table = "tab_width = 8\nsoft_wrap = \"yes\"\n".parse().unwrap();
        let (s, _, bad) = parse_lenient(table);
        assert_eq!(s.tab_width, 8);
        assert_eq!(s.soft_wrap, Settings::default().soft_wrap);
        assert_eq!(bad, vec!["soft_wrap".to_string()]);
    }

    #[test]
    fn bad_keybinding_value_keeps_the_rest() {
        let table: toml::Table = "[keybindings.normal]\n\"ctrl-k\" = \"save\"\n\"ctrl-j\" = 3\n".parse().unwrap();
        let (_, kb, bad) = parse_lenient(table);
        assert_eq!(kb["normal"].get("ctrl-k").map(String::as_str), Some("save"));
        assert_eq!(bad, vec!["keybindings.normal.ctrl-j".to_string()]);
    }

    #[test]
    fn unparseable_config_is_not_overwritten() {
        let dir = std::env::temp_dir().join(format!("doe-test-cfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "tab_width = = 4\n").unwrap();
        let cfg = Config {
            settings: Settings::default(),
            keybindings: default_keybindings(),
            theme: Theme::load("default-dark", &dir.join("themes")),
            callouts: Callouts::load(&dir),
            config_dir: dir.clone(),
            user_keybindings: HashMap::new(),
            load_warning: None,
            unparseable: true,
        };
        cfg.save();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "tab_width = = 4\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
