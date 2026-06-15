//! Syntax highlighting. [`Language`] is detected from the file extension and
//! selects a [`Highlighter`]. Markdown gets a dedicated highlighter; other
//! languages share a keyword-driven code highlighter.

pub mod code;
pub mod highlighter;
pub mod markdown;
pub mod treesitter;

pub use highlighter::{Highlighter, LineState, StyleKind};

use crate::editor::Buffer;
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

/// Above this buffer size we skip the whole-buffer tree-sitter parse and use
/// the cheap line-based highlighter, preserving the large-file cost guarantee.
const TREE_SITTER_MAX_BYTES: usize = 1_000_000;

/// Build the appropriate highlighter for a buffer. Markdown keeps its dedicated
/// highlighter; other languages prefer a tree-sitter grammar when one is
/// vendored and the buffer is small enough, else fall back to the keyword-driven
/// [`code::CodeHighlighter`].
pub fn highlighter_for(buf: &Buffer) -> Box<dyn Highlighter> {
    match buf.language {
        Language::Markdown => Box::new(markdown::MarkdownHighlighter),
        other => {
            if buf.rope.len_bytes() <= TREE_SITTER_MAX_BYTES {
                if let Some(h) = treesitter::TreeSitterHighlighter::new(other, &buf.rope) {
                    return Box::new(h);
                }
            }
            Box::new(code::CodeHighlighter::new(other))
        }
    }
}
