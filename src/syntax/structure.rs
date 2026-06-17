//! Structural operations over the parse tree: growing a selection to the
//! enclosing syntax node. Char offsets in, char offsets out; the byte↔char
//! mapping uses the cached rope.

use super::cache::ParsedDoc;

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
    fn none_when_no_grammar() {
        let mut b = Buffer::empty();
        b.set_text("plain text\n");
        // default language is plain text → no tree.
        let cache = SyntaxCache::new();
        assert!(cache.with_tree(&b, |doc| expand(&doc, 0, 0)).is_none());
    }
}
