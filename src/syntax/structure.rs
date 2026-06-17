//! Structural operations over the parse tree: growing a selection to the
//! enclosing syntax node, and collecting definition symbols for the outline.
//! Char offsets in, char offsets out; the byte↔char mapping uses the cached
//! rope.

use super::cache::ParsedDoc;
use ropey::Rope;
use tree_sitter::Node;

/// A definition symbol for the "Go to Symbol" outline.
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    /// Short kind label (`fn`, `struct`, `class`, …).
    pub kind: &'static str,
    /// 0-based line of the definition.
    pub line: usize,
}

/// Map a definition node kind to a short label, across the supported grammars.
fn symbol_kind(node_kind: &str) -> Option<&'static str> {
    Some(match node_kind {
        "function_item" | "function_definition" | "function_declaration" => "fn",
        "method_definition" => "method",
        "struct_item" => "struct",
        "enum_item" => "enum",
        "union_item" => "union",
        "trait_item" => "trait",
        "impl_item" => "impl",
        "mod_item" => "mod",
        "type_item" | "type_alias_declaration" => "type",
        "const_item" => "const",
        "static_item" => "static",
        "macro_definition" => "macro",
        "class_definition" | "class_declaration" => "class",
        "interface_declaration" => "interface",
        _ => return None,
    })
}

/// Collect definition symbols from the parse tree, in source order.
pub fn symbols(doc: &ParsedDoc) -> Vec<Symbol> {
    let mut out = Vec::new();
    visit(doc.tree.root_node(), doc.rope, &mut out);
    out
}

fn visit(node: Node, rope: &Rope, out: &mut Vec<Symbol>) {
    if let Some(kind) = symbol_kind(node.kind()) {
        // Most definitions expose a `name` field; `impl` blocks use `type`.
        let name_node = node.child_by_field_name("name").or_else(|| node.child_by_field_name("type"));
        if let Some(nn) = name_node {
            let s = rope.byte_to_char(nn.start_byte());
            let e = rope.byte_to_char(nn.end_byte());
            out.push(Symbol {
                name: rope.slice(s..e).to_string(),
                kind,
                line: rope.byte_to_line(node.start_byte()),
            });
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit(child, rope, out);
    }
}

/// The char range of the smallest tree node that strictly contains the current
/// selection `[s, e)` — i.e. the next "expand selection" step. `None` if there
/// is no larger node (already at the root) or the offsets are out of range.
pub fn expand(doc: &ParsedDoc, s: usize, e: usize) -> Option<(usize, usize)> {
    let rope = doc.rope;
    if s > rope.len_chars() || e > rope.len_chars() {
        return None;
    }
    let sb = rope.char_to_byte(s);
    let eb = rope.char_to_byte(e);
    let mut node = doc.tree.root_node().descendant_for_byte_range(sb, eb)?;
    // The smallest descendant may already equal the selection; climb until a
    // strictly larger node.
    while node.start_byte() == sb && node.end_byte() == eb {
        node = node.parent()?;
    }
    Some((rope.byte_to_char(node.start_byte()), rope.byte_to_char(node.end_byte())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::Buffer;
    use crate::syntax::cache::SyntaxCache;

    fn expand_in(src: &str, s: usize, e: usize) -> Option<(usize, usize)> {
        let mut b = Buffer::empty();
        b.set_text(src);
        b.language = crate::syntax::Language::Rust;
        let cache = SyntaxCache::new();
        cache.with_tree(&b, |doc| expand(&doc, s, e)).flatten()
    }

    #[test]
    fn grows_from_cursor_to_token_to_expr() {
        // `let x = a + b;` — cursor inside `a` grows to `a`, then `a + b`, ...
        let src = "fn f() { let x = aa + bb; }\n";
        let a = src.find("aa").unwrap();
        // Step 1: empty selection inside `aa` → the `aa` identifier.
        let (s1, e1) = expand_in(src, a + 1, a + 1).unwrap();
        assert_eq!(&src[s1..e1], "aa");
        // Step 2: from `aa` → the binary expression `aa + bb`.
        let (s2, e2) = expand_in(src, s1, e1).unwrap();
        assert_eq!(&src[s2..e2], "aa + bb");
    }

    #[test]
    fn collects_symbols_in_order() {
        let mut b = Buffer::empty();
        b.set_text("fn foo() {}\nstruct Bar;\nfn baz() {}\n");
        b.language = crate::syntax::Language::Rust;
        let cache = SyntaxCache::new();
        let syms = cache.with_tree(&b, |doc| symbols(&doc)).unwrap();
        let got: Vec<_> = syms.iter().map(|s| (s.kind, s.name.as_str(), s.line)).collect();
        assert_eq!(got, vec![("fn", "foo", 0), ("struct", "Bar", 1), ("fn", "baz", 2)]);
    }

    #[test]
    fn none_when_no_grammar() {
        let mut b = Buffer::empty();
        b.set_text("plain text\n");
        // default language is plain text → no tree.
        let cache = SyntaxCache::new();
        assert!(cache.with_tree(&b, |doc| expand(&doc, 0, 0)).is_none());
    }
}
