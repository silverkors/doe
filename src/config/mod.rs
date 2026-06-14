//! Configuration loading. Settings, keybindings and the active theme are read
//! from `<config>/doe/config.toml` (Linux/macOS: `~/.config/doe`, Windows:
//! `%APPDATA%/doe`). Everything has sensible built-in defaults, so DOE runs
//! fine with no config file at all; on first run a documented default config
//! and theme are written for the user to edit.

pub mod theme;

use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use theme::Theme;

#[derive(Debug, Clone)]
pub struct Config {
    pub settings: Settings,
    pub keybindings: Keybindings,
    pub theme: Theme,
    /// Base config directory; used for theme loading and future plugin discovery.
    #[allow(dead_code)]
    pub config_dir: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
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
    /// Start in insert mode (modern, modeless feel) vs normal mode.
    pub start_in_insert: bool,
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
            soft_wrap: false,
            show_whitespace: false,
            trim_trailing_whitespace_on_save: false,
            start_in_insert: true,
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

#[derive(Deserialize, Default)]
struct ConfigFile {
    #[serde(default)]
    settings: Option<Settings>,
    #[serde(default)]
    keybindings: HashMap<String, HashMap<String, String>>,
}

impl Config {
    /// Load configuration, applying defaults and writing a starter config on
    /// first run. Never fails: on any error it falls back to defaults.
    pub fn load() -> Config {
        let config_dir = dirs::config_dir()
            .map(|d| d.join("doe"))
            .unwrap_or_else(|| PathBuf::from(".doe"));
        let config_path = config_dir.join("config.toml");

        let mut settings = Settings::default();
        let mut keybindings = default_keybindings();

        if let Ok(text) = std::fs::read_to_string(&config_path) {
            if let Ok(file) = toml::from_str::<ConfigFile>(&text) {
                if let Some(s) = file.settings {
                    settings = s;
                }
                keybindings.merge(file.keybindings);
            }
        } else {
            // First run: scaffold config + default theme (best effort).
            scaffold(&config_dir);
        }

        let theme = Theme::load(&settings.theme, &config_dir.join("themes"));

        Config { settings, keybindings, theme, config_dir }
    }
}

/// Built-in keybindings so DOE is usable out of the box.
fn default_keybindings() -> Keybindings {
    let mut modes: HashMap<String, HashMap<String, String>> = HashMap::new();

    let common: &[(&str, &str)] = &[
        ("ctrl-s", "save"),
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
        ("ctrl-l", "select_all_matches"),
        ("ctrl-slash", "toggle_comment"),
        ("ctrl-pageup", "prev_buffer"),
        ("ctrl-pagedown", "next_buffer"),
        ("alt-up", "add_cursor_above"),
        ("alt-down", "add_cursor_below"),
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
        ("f3", "find_next"),
        ("shift-f3", "find_prev"),
        ("esc", "clear_extra_cursors"),
    ];

    let insert_only: &[(&str, &str)] = &[
        ("enter", "newline"),
        ("backspace", "backspace"),
        ("delete", "delete"),
        ("tab", "tab"),
    ];

    let mut insert: HashMap<String, String> = HashMap::new();
    for (k, v) in common.iter().chain(insert_only.iter()) {
        insert.insert(k.to_string(), v.to_string());
    }

    let mut normal: HashMap<String, String> = HashMap::new();
    for (k, v) in common {
        normal.insert(k.to_string(), v.to_string());
    }
    // Vim-ish niceties in normal mode.
    normal.insert("i".to_string(), "insert_mode".to_string());
    normal.insert("colon".to_string(), "command_mode".to_string());
    normal.insert("h".to_string(), "move_left".to_string());
    normal.insert("j".to_string(), "move_down".to_string());
    normal.insert("k".to_string(), "move_up".to_string());
    normal.insert("l".to_string(), "move_right".to_string());

    let select = normal.clone();

    modes.insert("insert".to_string(), insert);
    modes.insert("normal".to_string(), normal);
    modes.insert("select".to_string(), select);
    modes.insert("command".to_string(), HashMap::new());

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
soft_wrap = false
show_whitespace = false
trim_trailing_whitespace_on_save = false
start_in_insert = true

# Keybindings are merged over the built-in defaults. Chord syntax examples:
#   "ctrl-s", "alt-up", "shift-f3", "ctrl-shift-k", "enter", "esc", "colon"
[keybindings.normal]
"ctrl-s" = "save"
"ctrl-q" = "quit"
"ctrl-f" = "find"
"ctrl-b" = "toggle_bold"
"ctrl-i" = "toggle_italic"
"alt-up" = "add_cursor_above"
"alt-down" = "add_cursor_below"
"ctrl-d" = "add_cursor_next_match"

[keybindings.insert]
"ctrl-s" = "save"
"ctrl-z" = "undo"
"ctrl-y" = "redo"
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
markup_punct = "#606672"
"##;
