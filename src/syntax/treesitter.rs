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

use super::highlighter::{Highlighter, LineState, Span, StyleKind};
use super::Language;
use ropey::Rope;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language as TsLanguage, Parser, Query, QueryCursor};

pub struct TreeSitterHighlighter {
    /// Spans per buffer line, columns expressed in char offsets within the line.
    line_spans: Vec<Vec<Span>>,
}

impl TreeSitterHighlighter {
    /// Parse `rope` with `lang`'s grammar and precompute per-line spans. Returns
    /// `None` if the language has no grammar or the source fails to parse.
    pub fn new(lang: Language, rope: &Rope) -> Option<Self> {
        let (language, query_src) = grammar(lang)?;
        let mut parser = Parser::new();
        parser.set_language(&language).ok()?;
        let source = rope.to_string();
        let tree = parser.parse(source.as_bytes(), None)?;
        let query = Query::new(&language, query_src).ok()?;
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
        Some(TreeSitterHighlighter { line_spans })
    }
}

impl Highlighter for TreeSitterHighlighter {
    fn highlight_line(&self, line: usize, _text: &str, _state: &mut LineState) -> Vec<Span> {
        self.line_spans.get(line).cloned().unwrap_or_default()
    }
}

/// Map a grammar's `LANGUAGE` + highlights query, or `None` if unsupported.
fn grammar(lang: Language) -> Option<(TsLanguage, &'static str)> {
    match lang {
        Language::Rust => {
            Some((tree_sitter_rust::LANGUAGE.into(), tree_sitter_rust::HIGHLIGHTS_QUERY))
        }
        _ => None,
    }
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
        "comment" => StyleKind::Comment,
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
}
