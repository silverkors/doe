//! Themes. A theme maps semantic [`StyleKind`]s plus UI chrome (background,
//! status bar, selection, …) to terminal colours. Themes are plain TOML files;
//! `default-dark` is embedded so DOE always has something to render with.

use crate::syntax::StyleKind;
use crossterm::style::Color;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Theme {
    pub background: Color,
    pub foreground: Color,
    pub statusbar_bg: Color,
    pub statusbar_fg: Color,
    pub selection: Color,
    pub cursor: Color,
    pub line_number: Color,
    pub line_number_current: Color,
    pub whitespace: Color,

    pub keyword: Color,
    pub type_: Color,
    pub function: Color,
    pub string: Color,
    pub number: Color,
    pub comment: Color,
    pub heading: Color,
    pub bold: Color,
    pub italic: Color,
    pub code: Color,
    pub link: Color,
    pub list_marker: Color,
    pub quote: Color,
    pub markup_punct: Color,
}

impl Theme {
    pub fn default_dark() -> Self {
        Theme {
            background: Color::Rgb { r: 0x10, g: 0x12, b: 0x18 },
            foreground: Color::Rgb { r: 0xd0, g: 0xd0, b: 0xd0 },
            statusbar_bg: Color::Rgb { r: 0x30, g: 0x30, b: 0x3a },
            statusbar_fg: Color::Rgb { r: 0xe8, g: 0xe8, b: 0xe8 },
            selection: Color::Rgb { r: 0x33, g: 0x44, b: 0x66 },
            cursor: Color::Rgb { r: 0xff, g: 0xff, b: 0xff },
            line_number: Color::Rgb { r: 0x55, g: 0x5a, b: 0x66 },
            line_number_current: Color::Rgb { r: 0xc0, g: 0xc6, b: 0xd4 },
            whitespace: Color::Rgb { r: 0x3a, g: 0x3f, b: 0x4a },

            keyword: Color::Rgb { r: 0xff, g: 0xcc, b: 0x66 },
            type_: Color::Rgb { r: 0x66, g: 0xcc, b: 0xcc },
            function: Color::Rgb { r: 0x6c, g: 0xb6, b: 0xff },
            string: Color::Rgb { r: 0x99, g: 0xcc, b: 0x99 },
            number: Color::Rgb { r: 0xf9, g: 0x91, b: 0x57 },
            comment: Color::Rgb { r: 0x77, g: 0x77, b: 0x77 },
            heading: Color::Rgb { r: 0x66, g: 0x99, b: 0xcc },
            bold: Color::Rgb { r: 0xff, g: 0xff, b: 0xff },
            italic: Color::Rgb { r: 0xc8, g: 0xc8, b: 0xff },
            code: Color::Rgb { r: 0xcc, g: 0xa9, b: 0x7a },
            link: Color::Rgb { r: 0x6c, g: 0xb6, b: 0xff },
            list_marker: Color::Rgb { r: 0xff, g: 0xcc, b: 0x66 },
            quote: Color::Rgb { r: 0x99, g: 0xa0, b: 0xaa },
            markup_punct: Color::Rgb { r: 0x60, g: 0x66, b: 0x72 },
        }
    }

    /// Load a theme by name from `<config>/themes/<name>.toml`, falling back to
    /// the embedded default if the file is missing or invalid.
    pub fn load(name: &str, themes_dir: &Path) -> Theme {
        let path = themes_dir.join(format!("{name}.toml"));
        let mut theme = Theme::default_dark();
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(file) = toml::from_str::<ThemeFile>(&text) {
                file.colors.apply(&mut theme);
            }
        }
        theme
    }

    pub fn color_for(&self, kind: StyleKind) -> Color {
        match kind {
            StyleKind::Default => self.foreground,
            StyleKind::Keyword => self.keyword,
            StyleKind::Type => self.type_,
            StyleKind::Function => self.function,
            StyleKind::String => self.string,
            StyleKind::Number => self.number,
            StyleKind::Comment => self.comment,
            StyleKind::Heading => self.heading,
            StyleKind::Bold => self.bold,
            StyleKind::Italic => self.italic,
            StyleKind::Code => self.code,
            StyleKind::Link => self.link,
            StyleKind::ListMarker => self.list_marker,
            StyleKind::Quote => self.quote,
            StyleKind::MarkupPunct => self.markup_punct,
        }
    }
}

#[derive(Deserialize)]
struct ThemeFile {
    #[serde(default)]
    colors: ThemeColors,
}

#[derive(Deserialize, Default)]
struct ThemeColors {
    background: Option<String>,
    foreground: Option<String>,
    statusbar: Option<String>,
    statusbar_fg: Option<String>,
    selection: Option<String>,
    cursor: Option<String>,
    line_number: Option<String>,
    line_number_current: Option<String>,
    whitespace: Option<String>,
    keyword: Option<String>,
    type_: Option<String>,
    function: Option<String>,
    string: Option<String>,
    number: Option<String>,
    comment: Option<String>,
    heading: Option<String>,
    bold: Option<String>,
    italic: Option<String>,
    code: Option<String>,
    link: Option<String>,
    list_marker: Option<String>,
    quote: Option<String>,
    markup_punct: Option<String>,
}

impl ThemeColors {
    fn apply(&self, t: &mut Theme) {
        set(&self.background, &mut t.background);
        set(&self.foreground, &mut t.foreground);
        set(&self.statusbar, &mut t.statusbar_bg);
        set(&self.statusbar_fg, &mut t.statusbar_fg);
        set(&self.selection, &mut t.selection);
        set(&self.cursor, &mut t.cursor);
        set(&self.line_number, &mut t.line_number);
        set(&self.line_number_current, &mut t.line_number_current);
        set(&self.whitespace, &mut t.whitespace);
        set(&self.keyword, &mut t.keyword);
        set(&self.type_, &mut t.type_);
        set(&self.function, &mut t.function);
        set(&self.string, &mut t.string);
        set(&self.number, &mut t.number);
        set(&self.comment, &mut t.comment);
        set(&self.heading, &mut t.heading);
        set(&self.bold, &mut t.bold);
        set(&self.italic, &mut t.italic);
        set(&self.code, &mut t.code);
        set(&self.link, &mut t.link);
        set(&self.list_marker, &mut t.list_marker);
        set(&self.quote, &mut t.quote);
        set(&self.markup_punct, &mut t.markup_punct);
    }
}

fn set(src: &Option<String>, dst: &mut Color) {
    if let Some(hex) = src {
        if let Some(c) = parse_hex(hex) {
            *dst = c;
        }
    }
}

/// Parse `#rrggbb` (or `rrggbb`) into a terminal RGB colour.
pub fn parse_hex(s: &str) -> Option<Color> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some(Color::Rgb { r, g, b })
}
