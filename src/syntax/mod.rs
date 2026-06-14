//! Syntax highlighting. [`Language`] is detected from the file extension and
//! selects a [`Highlighter`]. Markdown gets a dedicated highlighter; other
//! languages share a keyword-driven code highlighter.

pub mod code;
pub mod highlighter;
pub mod markdown;

pub use highlighter::{Highlighter, LineState, StyleKind};

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    PlainText,
    Markdown,
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Html,
    Css,
    Json,
    Toml,
    Yaml,
    Swift,
}

impl Language {
    pub fn from_path(path: &Path) -> Language {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "md" | "markdown" | "mdown" | "mkd" => Language::Markdown,
            "rs" => Language::Rust,
            "py" | "pyw" => Language::Python,
            "js" | "jsx" | "mjs" | "cjs" => Language::JavaScript,
            "ts" | "tsx" => Language::TypeScript,
            "html" | "htm" | "xml" => Language::Html,
            "css" | "scss" => Language::Css,
            "json" => Language::Json,
            "toml" => Language::Toml,
            "yaml" | "yml" => Language::Yaml,
            "swift" => Language::Swift,
            _ => Language::PlainText,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Language::PlainText => "text",
            Language::Markdown => "markdown",
            Language::Rust => "rust",
            Language::Python => "python",
            Language::JavaScript => "javascript",
            Language::TypeScript => "typescript",
            Language::Html => "html",
            Language::Css => "css",
            Language::Json => "json",
            Language::Toml => "toml",
            Language::Yaml => "yaml",
            Language::Swift => "swift",
        }
    }

    pub fn line_comment(&self) -> Option<&'static str> {
        match self {
            Language::Rust
            | Language::JavaScript
            | Language::TypeScript
            | Language::Css
            | Language::Swift => Some("//"),
            Language::Python | Language::Yaml | Language::Toml => Some("#"),
            _ => None,
        }
    }

    pub fn keywords(&self) -> &'static [&'static str] {
        match self {
            Language::Rust => &[
                "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else",
                "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop",
                "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static",
                "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while",
            ],
            Language::Python => &[
                "and", "as", "assert", "async", "await", "break", "class", "continue", "def",
                "del", "elif", "else", "except", "False", "finally", "for", "from", "global",
                "if", "import", "in", "is", "lambda", "None", "nonlocal", "not", "or", "pass",
                "raise", "return", "True", "try", "while", "with", "yield",
            ],
            Language::JavaScript | Language::TypeScript => &[
                "async", "await", "break", "case", "catch", "class", "const", "continue",
                "default", "delete", "do", "else", "export", "extends", "false", "finally",
                "for", "from", "function", "if", "import", "in", "instanceof", "interface",
                "let", "new", "null", "of", "return", "super", "switch", "this", "throw",
                "true", "try", "type", "typeof", "undefined", "var", "void", "while", "yield",
            ],
            Language::Css => &[
                "important", "inherit", "initial", "none", "auto", "flex", "grid", "block",
                "inline", "absolute", "relative", "fixed", "hidden",
            ],
            Language::Swift => &[
                "as", "associatedtype", "break", "case", "catch", "class", "continue", "default",
                "defer", "do", "else", "enum", "extension", "false", "for", "func", "guard",
                "if", "import", "in", "init", "internal", "let", "nil", "private", "protocol",
                "public", "return", "self", "static", "struct", "switch", "throw", "throws",
                "true", "try", "var", "where", "while",
            ],
            Language::Json | Language::Yaml | Language::Toml => {
                &["true", "false", "null"]
            }
            _ => &[],
        }
    }
}

/// Build the appropriate highlighter for a language.
pub fn highlighter_for(lang: Language) -> Box<dyn Highlighter> {
    match lang {
        Language::Markdown => Box::new(markdown::MarkdownHighlighter),
        Language::PlainText => Box::new(code::CodeHighlighter::new(Language::PlainText)),
        other => Box::new(code::CodeHighlighter::new(other)),
    }
}
