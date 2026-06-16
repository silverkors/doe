//! Import callout styles from Obsidian's Callout Manager plugin
//! (`<vault>/.obsidian/plugins/callout-manager/data.json`).
//!
//! Colours import exactly (`"r, g, b"` → RGB); Obsidian icons are Lucide icon
//! *names*, which we map to the nearest single-width glyph (falling back to the
//! default glyph). Rules can be conditioned on the colour scheme — DOE is
//! a dark terminal, so unconditional and `dark` rules apply and `light` is
//! skipped.

use crossterm::style::Color;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// One imported callout: a type name plus whichever of colour/icon were set.
#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    pub name: String,
    pub color: Option<Color>,
    pub icon: Option<char>,
}

/// Parse a Callout Manager `data.json` into per-type overrides.
pub fn parse(json: &str) -> Vec<Import> {
    let root: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let settings = match root.get("callouts").and_then(|c| c.get("settings")).and_then(|s| s.as_object()) {
        Some(s) => s,
        None => return Vec::new(),
    };

    let mut out = Vec::new();
    for (name, rules) in settings {
        let Some(rules) = rules.as_array() else { continue };
        let mut color: Option<Color> = None;
        let mut icon: Option<char> = None;
        for rule in rules {
            // Skip light-scheme-only rules; apply unconditional and dark ones.
            if let Some(scheme) = rule.get("condition").and_then(|c| c.get("colorScheme")).and_then(|s| s.as_str()) {
                if scheme == "light" {
                    continue;
                }
            }
            let Some(changes) = rule.get("changes") else { continue };
            if let Some(c) = changes.get("color").and_then(|c| c.as_str()).and_then(parse_rgb) {
                color = Some(c);
            }
            if let Some(name) = changes.get("icon").and_then(|i| i.as_str()) {
                if let Some(g) = lucide_to_glyph(name) {
                    icon = Some(g);
                }
            }
        }
        if color.is_some() || icon.is_some() {
            out.push(Import { name: name.to_lowercase(), color, icon });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Parse Obsidian's `"r, g, b"` decimal-triplet colour string.
fn parse_rgb(s: &str) -> Option<Color> {
    let parts: Vec<_> = s.split(',').map(|p| p.trim()).collect();
    if parts.len() != 3 {
        return None;
    }
    let r = parts[0].parse().ok()?;
    let g = parts[1].parse().ok()?;
    let b = parts[2].parse().ok()?;
    Some(Color::Rgb { r, g, b })
}

/// Map a Lucide icon name (with or without the `lucide-` prefix) to the nearest
/// single-width glyph. Tries the full name, then the leading token, so
/// `flower-2`→`flower` and `heart-handshake`→`heart` resolve.
pub fn lucide_to_glyph(name: &str) -> Option<char> {
    let name = name.strip_prefix("lucide-").unwrap_or(name);
    glyph_for(name).or_else(|| glyph_for(name.split('-').next().unwrap_or(name)))
}

fn glyph_for(name: &str) -> Option<char> {
    let g = match name {
        "pencil" | "pen" | "edit" | "file-pen" | "square-pen" => '✎',
        "check" | "check-circle" | "circle-check" | "checks" => '✓',
        "x" | "x-circle" | "circle-x" => '✗',
        "info" | "circle-info" => '●',
        "flame" | "fire" => '▲',
        "alert-triangle" | "triangle-alert" => '▲',
        "alert-circle" | "circle-alert" => '!',
        "help-circle" | "circle-help" | "help" => '?',
        "quote" => '"',
        "list" | "book" | "book-open" => '»',
        "music" => '♪',
        "heart" | "heart-handshake" => '♥',
        "star" => '★',
        "flower" => '❀',
        "cross" => '✝',
        "atom" => '⚛',
        "smile" => '☺',
        "pause" => '‖',
        "hand" => '☞',
        "lightbulb" | "idea" => '✦',
        "party-popper" => '✸',
        "bug" => '■',
        "zap" | "lightning" => '⚡',
        "bell" => '⚑',
        "pin" => '✚',
        "sun" => '☼',
        "moon" => '☽',
        "key" => '⚷',
        "droplet" => '◇',
        _ => return None,
    };
    Some(g)
}

/// Resolve a user-supplied path to a Callout Manager `data.json`. Accepts the
/// file itself, a vault root, or a `.obsidian` directory.
pub fn resolve_data_path(input: &Path) -> Option<PathBuf> {
    if input.is_file() {
        return Some(input.to_path_buf());
    }
    let candidates = [
        input.join(".obsidian/plugins/callout-manager/data.json"), // vault root
        input.join("plugins/callout-manager/data.json"),           // .obsidian dir
        input.join("data.json"),                                   // plugin dir
    ];
    candidates.into_iter().find(|p| p.is_file())
}

/// Best-effort auto-detect: read Obsidian's vault registry and return the first
/// vault that has Callout Manager data. Returns `None` if nothing is found.
pub fn autodetect() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    // Obsidian records known vaults here (macOS path).
    let registry = home.join("Library/Application Support/obsidian/obsidian.json");
    let text = std::fs::read_to_string(&registry).ok()?;
    let root: Value = serde_json::from_str(&text).ok()?;
    let vaults = root.get("vaults")?.as_object()?;
    for v in vaults.values() {
        if let Some(path) = v.get("path").and_then(|p| p.as_str()) {
            let data = Path::new(path).join(".obsidian/plugins/callout-manager/data.json");
            if data.is_file() {
                return Some(data);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "callouts": {
            "custom": ["psalm", "blessing"],
            "settings": {
                "psalm": [{ "changes": { "icon": "lucide-music", "color": "168, 130, 255" } }],
                "blessing": [{ "changes": { "icon": "lucide-hand", "color": "251, 70, 76" } }],
                "litred": [
                    { "changes": { "icon": "lucide-droplet" } },
                    { "condition": { "colorScheme": "light" }, "changes": { "color": "1, 2, 3" } },
                    { "condition": { "colorScheme": "dark" }, "changes": { "color": "251, 70, 76" } }
                ]
            }
        }
    }"#;

    #[test]
    fn imports_color_and_mapped_icon() {
        let imps = parse(SAMPLE);
        let psalm = imps.iter().find(|i| i.name == "psalm").unwrap();
        assert_eq!(psalm.color, Some(Color::Rgb { r: 168, g: 130, b: 255 }));
        assert_eq!(psalm.icon, Some('♪'));
    }

    #[test]
    fn dark_scheme_wins_over_light() {
        let imps = parse(SAMPLE);
        let lr = imps.iter().find(|i| i.name == "litred").unwrap();
        // The light rule (1,2,3) is skipped; the dark rule applies.
        assert_eq!(lr.color, Some(Color::Rgb { r: 251, g: 70, b: 76 }));
        assert_eq!(lr.icon, Some('◇'));
    }

    #[test]
    fn lucide_prefix_and_compound_names() {
        assert_eq!(lucide_to_glyph("lucide-music"), Some('♪'));
        assert_eq!(lucide_to_glyph("heart-handshake"), Some('♥')); // leading token
        assert_eq!(lucide_to_glyph("flower-2"), Some('❀'));
        assert_eq!(lucide_to_glyph("totally-unknown"), None);
    }
}
