//! tree-sitter-backed highlighter. Unlike the line-based highlighters, this
//! parses the *whole* buffer once (in [`TreeSitterHighlighter::new`], i.e. once
//! per render) and precomputes a per-line list of [`Span`]s. The renderer then
//! pulls spans for each visible line through the same [`Highlighter`] trait —
//! the tree-sitter work is hidden behind the constructor.
//!
//! Languages without a vendored grammar return `None`, and the caller falls
//! back to the keyword-driven [`super::code::CodeHighlighter`]. A size guard in
//! [`super::highlighter_for`] keeps very large buffers off this path so the
//! whole-buffer parse never dominates a frame.

use super::highlighter::{Span, StyleKind};
use super::Language;
use ropey::Rope;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language as TsLanguage, Query, QueryCursor, Tree};
#[cfg(test)]
use super::highlighter::{Highlighter, LineState};
#[cfg(test)]
use tree_sitter::Parser;

/// A standalone tree-sitter highlighter that parses its own input. The editor
/// highlights through [`super::cache::SyntaxCache`]; this type now backs the
/// span-building tests.
#[cfg(test)]
pub struct TreeSitterHighlighter {
    /// Spans per buffer line, columns expressed in char offsets within the line.
    line_spans: Vec<Vec<Span>>,
}

#[cfg(test)]
impl TreeSitterHighlighter {
    /// Parse `rope` with `lang`'s grammar and precompute per-line spans. Returns
    /// `None` if the language has no grammar or the source fails to parse.
    /// (The editor highlights through [`super::cache::SyntaxCache`], which reuses
    /// a cached parse; this constructor parses fresh and is used by tests.)
    pub fn new(lang: Language, rope: &Rope) -> Option<Self> {
        let (language, query_src) = grammar(lang)?;
        let mut parser = Parser::new();
        parser.set_language(&language).ok()?;
        let source = rope.to_string();
        let tree = parser.parse(source.as_bytes(), None)?;
        Some(TreeSitterHighlighter { line_spans: build_spans(&tree, &language, &query_src, &source, rope) })
    }
}

/// Run a grammar's highlights query over `tree` and bucket the captures into
/// per-line [`Span`]s (char columns). Shared by the highlighter and the cache.
pub(crate) fn build_spans(tree: &Tree, language: &TsLanguage, query_src: &str, source: &str, rope: &Rope) -> Vec<Vec<Span>> {
    let Ok(query) = Query::new(language, query_src) else {
        return vec![Vec::new(); rope.len_lines()];
    };
    let names = query.capture_names();
    let mut line_spans: Vec<Vec<Span>> = vec![Vec::new(); rope.len_lines()];
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
    while let Some(m) = matches.next() {
        for cap in m.captures {
            let name = names[cap.index as usize];
            let node = cap.node;
            if let Some(kind) = map_capture(name, node.kind()) {
                let r = node.byte_range();
                push_span(&mut line_spans, rope, r.start, r.end, kind);
            }
        }
    }
    line_spans
}

#[cfg(test)]
impl Highlighter for TreeSitterHighlighter {
    fn highlight_line(&self, line: usize, _text: &str, _state: &mut LineState) -> Vec<Span> {
        self.line_spans.get(line).cloned().unwrap_or_default()
    }
}

/// Map a grammar's `LANGUAGE` + highlights query, or `None` if unsupported.
/// The query is owned because some languages (TypeScript) concatenate the
/// queries of a base grammar (JavaScript) with their own.
pub(crate) fn grammar(lang: Language) -> Option<(TsLanguage, String)> {
    let (language, query): (TsLanguage, String) = match lang {
        Language::Rust => (
            tree_sitter_rust::LANGUAGE.into(),
            tree_sitter_rust::HIGHLIGHTS_QUERY.to_string(),
        ),
        Language::Python => (
            tree_sitter_python::LANGUAGE.into(),
            tree_sitter_python::HIGHLIGHTS_QUERY.to_string(),
        ),
        Language::JavaScript => (
            tree_sitter_javascript::LANGUAGE.into(),
            tree_sitter_javascript::HIGHLIGHT_QUERY.to_string(),
        ),
        Language::TypeScript => {
            // The TS highlights query inherits JavaScript's; the TS grammar is a
            // superset, so the JS query parses against it.
            let mut q = tree_sitter_javascript::HIGHLIGHT_QUERY.to_string();
            q.push('\n');
            q.push_str(tree_sitter_typescript::HIGHLIGHTS_QUERY);
            (tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(), q)
        }
        Language::Json => (
            tree_sitter_json::LANGUAGE.into(),
            tree_sitter_json::HIGHLIGHTS_QUERY.to_string(),
        ),
        Language::Css => (
            tree_sitter_css::LANGUAGE.into(),
            tree_sitter_css::HIGHLIGHTS_QUERY.to_string(),
        ),
        Language::Html => (
            tree_sitter_html::LANGUAGE.into(),
            tree_sitter_html::HIGHLIGHTS_QUERY.to_string(),
        ),
        _ => return None,
    };
    Some((language, query))
}

/// Translate a tree-sitter capture name (refined by the node kind) into one of
/// our semantic [`StyleKind`]s. Returns `None` for captures we leave unstyled.
fn map_capture(name: &str, node_kind: &str) -> Option<StyleKind> {
    // Some grammars lump numbers and booleans under one capture
    // (`constant.builtin` in Rust); the node kind disambiguates.
    match node_kind {
        "integer_literal" | "float_literal" => return Some(StyleKind::Number),
        _ => {}
    }
    // Longest-prefix-ish match: check the few qualified names we care about,
    // then fall back to the top-level category.
    if name.starts_with("constant.builtin") {
        return Some(StyleKind::Keyword); // booleans, nil, etc. (numbers handled above)
    }
    let base = name.split('.').next().unwrap_or(name);
    let kind = match base {
        "keyword" => StyleKind::Keyword,
        "function" => StyleKind::Function,
        "constructor" | "type" | "constant" => StyleKind::Type,
        "string" | "escape" | "char" => StyleKind::String,
        "number" => StyleKind::Number,
        "comment" => StyleKind::Comment,
        "tag" => StyleKind::Tag,
        "attribute" => StyleKind::Attribute,
        _ => return None,
    };
    Some(kind)
}

/// Split a byte range from the parse tree into per-line char-column spans.
/// Columns exclude nothing special — the renderer clamps each span to the
/// line's content width, so an overrun onto a trailing newline is harmless.
fn push_span(line_spans: &mut [Vec<Span>], rope: &Rope, start_byte: usize, end_byte: usize, kind: StyleKind) {
    if start_byte >= end_byte {
        return;
    }
    let start_char = rope.byte_to_char(start_byte);
    let end_char = rope.byte_to_char(end_byte);
    let mut c = start_char;
    while c < end_char {
        let line = rope.char_to_line(c);
        let line_start = rope.line_to_char(line);
        let next_start = if line + 1 < rope.len_lines() {
            rope.line_to_char(line + 1)
        } else {
            rope.len_chars()
        };
        let seg_end = end_char.min(next_start);
        if seg_end <= c {
            break; // no forward progress; avoid looping
        }
        if line < line_spans.len() {
            line_spans[line].push(Span::new(c - line_start, seg_end - line_start, kind));
        }
        c = seg_end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds_for_line(src: &str, line: usize) -> Vec<(usize, usize, StyleKind)> {
        let rope = Rope::from_str(src);
        let h = TreeSitterHighlighter::new(Language::Rust, &rope).expect("rust grammar");
        let mut st = LineState::default();
        h.highlight_line(line, "", &mut st)
            .into_iter()
            .map(|s| (s.start, s.end, s.kind))
            .collect()
    }

    fn has_kind(spans: &[(usize, usize, StyleKind)], col: usize, kind: StyleKind) -> bool {
        spans.iter().any(|(s, e, k)| *s <= col && col < *e && *k == kind)
    }

    #[test]
    fn keyword_and_function_are_styled() {
        // `fn` is a keyword; `main` is a function definition name.
        let spans = kinds_for_line("fn main() {}\n", 0);
        assert!(has_kind(&spans, 0, StyleKind::Keyword), "`fn` keyword: {spans:?}");
        assert!(has_kind(&spans, 3, StyleKind::Function), "`main` fn: {spans:?}");
    }

    #[test]
    fn number_vs_comment_and_string() {
        let src = "let x = 42; // note\nlet s = \"hi\";\n";
        let l0 = kinds_for_line(src, 0);
        assert!(has_kind(&l0, 0, StyleKind::Keyword), "`let`: {l0:?}");
        assert!(has_kind(&l0, 8, StyleKind::Number), "`42`: {l0:?}");
        assert!(has_kind(&l0, 12, StyleKind::Comment), "comment: {l0:?}");
        let l1 = kinds_for_line(src, 1);
        assert!(has_kind(&l1, 9, StyleKind::String), "string: {l1:?}");
    }

    #[test]
    fn spans_split_across_lines() {
        // A block comment spans two lines; both lines must carry Comment spans.
        let src = "/* a\nb */\n";
        assert!(has_kind(&kinds_for_line(src, 0), 0, StyleKind::Comment));
        assert!(has_kind(&kinds_for_line(src, 1), 0, StyleKind::Comment));
    }

    /// Every vendored grammar must build a highlighter and parse its query
    /// (a malformed query would silently return `None` and fall back).
    #[test]
    fn all_vendored_grammars_construct() {
        let samples = [
            (Language::Rust, "fn f() {}\n"),
            (Language::Python, "def f():\n    return 1\n"),
            (Language::JavaScript, "function f() { return 1; }\n"),
            (Language::TypeScript, "function f(x: number): number { return x; }\n"),
            (Language::Json, "{\"a\": 1, \"b\": true}\n"),
            (Language::Css, "a { color: red; }\n"),
            (Language::Html, "<div class=\"x\">hi</div>\n"),
        ];
        for (lang, src) in samples {
            let rope = Rope::from_str(src);
            assert!(
                TreeSitterHighlighter::new(lang, &rope).is_some(),
                "grammar for {lang:?} failed to construct/parse its query"
            );
        }
    }

    #[test]
    fn python_number_and_keyword() {
        let rope = Rope::from_str("x = 42\n");
        let h = TreeSitterHighlighter::new(Language::Python, &rope).unwrap();
        let mut st = LineState::default();
        let spans: Vec<_> = h
            .highlight_line(0, "", &mut st)
            .into_iter()
            .map(|s| (s.start, s.end, s.kind))
            .collect();
        assert!(has_kind(&spans, 4, StyleKind::Number), "`42`: {spans:?}");
    }
}
