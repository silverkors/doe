//! Callout styling, data-driven. Each callout type has an accent colour (hex)
//! and a single display glyph. Built-in defaults reproduce the common Obsidian
//! set (with the usual aliases grouped); users override colours/icons — or add
//! wholly new types — in `<config>/doe/callouts.toml`, edited by hand, through
//! the in-editor callout panel, or imported from Obsidian's Callout Manager.

use super::theme::parse_hex;
use crossterm::style::Color;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

/// One callout style. `name` is the primary type shown in the editor; `aliases`
/// are extra type names that share the style (e.g. `tip` also covers `hint`).
#[derive(Debug, Clone)]
pub struct Callout {
    pub name: String,
    pub aliases: Vec<String>,
    pub color: Color,
    pub icon: char,
}

impl Callout {
    fn matches(&self, ty: &str) -> bool {
        self.name == ty || self.aliases.iter().any(|a| a == ty)
    }
}

#[derive(Debug, Clone)]
pub struct Callouts {
    pub list: Vec<Callout>,
}

/// Fallback accent for an unknown callout type (matches the old hardcoded
/// default: the theme-independent callout purple).
const FALLBACK_COLOR: Color = Color::Rgb { r: 0x9d, g: 0x7c, b: 0xd8 };
const FALLBACK_ICON: char = '◆';

impl Default for Callouts {
    fn default() -> Self {
        // (primary, aliases, hex, icon) — Obsidian-like defaults.
        let defs: &[(&str, &[&str], &str, char)] = &[
            ("note", &["info"], "#448aff", '●'),
            ("abstract", &["summary", "tldr"], "#00b8d4", '●'),
            ("todo", &[], "#448aff", '●'),
            ("tip", &["hint", "important"], "#00bfa6", '◆'),
            ("success", &["check", "done"], "#00c853", '✓'),
            ("question", &["help", "faq"], "#f0b400", '?'),
            ("warning", &["caution", "attention"], "#ff9100", '▲'),
            ("failure", &["fail", "missing"], "#ff5252", '■'),
            ("danger", &["error"], "#ff1744", '⚑'),
            ("bug", &[], "#f5512e", '■'),
            ("example", &[], "#7c4dff", '»'),
            ("quote", &["cite"], "#9e9e9e", '"'),
        ];
        let list = defs
            .iter()
            .map(|(name, aliases, hex, icon)| Callout {
                name: name.to_string(),
                aliases: aliases.iter().map(|a| a.to_string()).collect(),
                color: parse_hex(hex).unwrap_or(FALLBACK_COLOR),
                icon: *icon,
            })
            .collect();
        Callouts { list }
    }
}

/// Raw `callouts.toml` row: `[name]` with optional `color`/`icon`.
#[derive(Deserialize)]
struct RawCallout {
    color: Option<String>,
    icon: Option<String>,
}

impl Callouts {
    pub fn load(config_dir: &Path) -> Callouts {
        let mut callouts = Callouts::default();
        let path = config_dir.join("callouts.toml");
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(raw) = toml::from_str::<BTreeMap<String, RawCallout>>(&text) {
                for (name, def) in raw {
                    let color = def.color.as_deref().and_then(parse_hex);
                    let icon = def.icon.as_deref().and_then(|s| s.chars().next());
                    callouts.apply_override(&name, color, icon);
                }
            }
        }
        callouts
    }

    /// Look up the style for a callout type (case-insensitive). A standalone
    /// entry whose name matches wins over a group that only lists it as an alias.
    pub fn get(&self, ty: &str) -> Option<&Callout> {
        let ty = ty.to_lowercase();
        self.list
            .iter()
            .find(|c| c.name == ty)
            .or_else(|| self.list.iter().find(|c| c.matches(&ty)))
    }

    /// Accent colour + glyph for a type, falling back to the default callout look.
    pub fn style(&self, ty: &str) -> (Color, char) {
        self.get(ty).map(|c| (c.color, c.icon)).unwrap_or((FALLBACK_COLOR, FALLBACK_ICON))
    }

    /// Override an existing entry's colour/icon, or append a new standalone one.
    /// `None` fields leave the current value untouched.
    pub fn apply_override(&mut self, name: &str, color: Option<Color>, icon: Option<char>) {
        let name = name.to_lowercase();
        // Prefer an exact-name entry; otherwise the group that aliases it.
        let idx = self
            .list
            .iter()
            .position(|c| c.name == name)
            .or_else(|| self.list.iter().position(|c| c.matches(&name)));
        match idx {
            // Editing an entry by its exact primary name updates that entry.
            Some(i) if self.list[i].name == name => {
                if let Some(c) = color {
                    self.list[i].color = c;
                }
                if let Some(ic) = icon {
                    self.list[i].icon = ic;
                }
            }
            // The name is only an alias of a group: split it into a standalone
            // entry so the override is specific to this type.
            Some(i) => {
                let base = &self.list[i];
                let new = Callout {
                    name: name.clone(),
                    aliases: Vec::new(),
                    color: color.unwrap_or(base.color),
                    icon: icon.unwrap_or(base.icon),
                };
                self.list.push(new);
            }
            None => self.list.push(Callout {
                name,
                aliases: Vec::new(),
                color: color.unwrap_or(FALLBACK_COLOR),
                icon: icon.unwrap_or(FALLBACK_ICON),
            }),
        }
    }

    /// Serialise every entry to `callouts.toml` (round-trips colour + icon).
    pub fn save(&self, config_dir: &Path) {
        let _ = std::fs::create_dir_all(config_dir);
        let mut out = String::from(
            "# DOE callouts — accent colour (#rrggbb) and glyph per type.\n\
             # Edit here, via the callout panel, or import from Obsidian.\n\n",
        );
        for c in &self.list {
            let Color::Rgb { r, g, b } = c.color else { continue };
            out.push_str(&format!("[{}]\ncolor = \"#{r:02x}{g:02x}{b:02x}\"\nicon = \"{}\"\n\n", c.name, c.icon));
        }
        let _ = std::fs::write(config_dir.join("callouts.toml"), out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alias_resolves_to_group_style() {
        let c = Callouts::default();
        // `hint` is an alias of the `tip` group.
        assert_eq!(c.style("hint").1, c.style("tip").1);
        assert_eq!(c.style("HINT").1, c.style("tip").1); // case-insensitive
    }

    #[test]
    fn unknown_type_uses_fallback() {
        let c = Callouts::default();
        assert_eq!(c.style("psalm"), (FALLBACK_COLOR, FALLBACK_ICON));
    }

    #[test]
    fn override_alias_splits_into_standalone() {
        let mut c = Callouts::default();
        let tip_icon = c.style("tip").1;
        c.apply_override("important", parse_hex("#123456"), Some('★'));
        // `important` now has its own style, but `tip`/`hint` are unchanged.
        assert_eq!(c.style("important"), (parse_hex("#123456").unwrap(), '★'));
        assert_eq!(c.style("tip").1, tip_icon);
        assert_eq!(c.style("hint").1, tip_icon);
    }

    #[test]
    fn override_new_type_appends() {
        let mut c = Callouts::default();
        c.apply_override("psalm", parse_hex("#9d7cd8"), Some('♪'));
        assert_eq!(c.style("psalm"), (parse_hex("#9d7cd8").unwrap(), '♪'));
    }
}
