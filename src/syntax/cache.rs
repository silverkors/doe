//! A revision-keyed tree-sitter parse cache. The buffer is parsed once per
//! edit (not once per frame): highlight spans are precomputed on (re)parse and
//! the parse tree is kept so structural features — expand selection, folding,
//! the symbol outline — can reuse it. Interior mutability lets the immutable
//! render path warm and read the cache.

use super::highlighter::Span;
use super::treesitter::{build_spans, grammar};
use super::Language;
use crate::editor::Buffer;
use ropey::Rope;
use std::cell::RefCell;
use tree_sitter::{Parser, Tree};

/// Above this size we skip tree-sitter entirely (the keyword highlighter and a
/// "no structural ops" fallback), keeping huge files cheap.
const MAX_BYTES: usize = 1_000_000;

struct Cached {
    lang: Language,
    revision: u64,
    tree: Tree,
    rope: Rope,
    line_spans: Vec<Vec<Span>>,
}

/// A parsed document handed to structural operations.
pub struct ParsedDoc<'a> {
    pub tree: &'a Tree,
    pub rope: &'a Rope,
}

#[derive(Default)]
pub struct SyntaxCache {
    inner: RefCell<Option<Cached>>,
}

impl SyntaxCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reparse if the cached entry doesn't match `buf`'s language/revision.
    fn refresh(&self, buf: &Buffer) {
        if let Some(c) = self.inner.borrow().as_ref() {
            if c.lang == buf.language && c.revision == buf.revision {
                return;
            }
        }
        *self.inner.borrow_mut() = parse(buf);
    }

    /// Whether tree-sitter handles this buffer (also warms the cache).
    pub fn handles(&self, buf: &Buffer) -> bool {
        self.refresh(buf);
        self.inner.borrow().is_some()
    }

    /// Highlight spans for one line (empty if the buffer isn't tree-sitter
    /// handled or the line is out of range).
    pub fn highlight(&self, buf: &Buffer, line: usize) -> Vec<Span> {
        self.refresh(buf);
        self.inner
            .borrow()
            .as_ref()
            .and_then(|c| c.line_spans.get(line).cloned())
            .unwrap_or_default()
    }

    /// Run `f` with the parse tree, e.g. for structural selection. Returns
    /// `None` if the buffer isn't tree-sitter handled.
    pub fn with_tree<R>(&self, buf: &Buffer, f: impl FnOnce(ParsedDoc) -> R) -> Option<R> {
        self.refresh(buf);
        let cell = self.inner.borrow();
        let c = cell.as_ref()?;
        Some(f(ParsedDoc { tree: &c.tree, rope: &c.rope }))
    }
}

fn parse(buf: &Buffer) -> Option<Cached> {
    if buf.rope.len_bytes() > MAX_BYTES {
        return None;
    }
    let (tslang, query) = grammar(buf.language)?;
    let mut parser = Parser::new();
    parser.set_language(&tslang).ok()?;
    let source = buf.rope.to_string();
    let tree = parser.parse(source.as_bytes(), None)?;
    let rope = Rope::from_str(&source);
    let line_spans = build_spans(&tree, &tslang, &query, &source, &rope);
    Some(Cached { lang: buf.language, revision: buf.revision, tree, rope, line_spans })
}
