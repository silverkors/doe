//! Markdown highlighter. Handles headings, lists, block quotes, fenced code
//! blocks and inline constructs (bold, italic, inline code, links). Markup
//! punctuation is emitted as [`StyleKind::MarkupPunct`] so the renderer can dim
//! it, giving a clean "live preview" feel while keeping the raw source.

use super::highlighter::{Highlighter, LineState, Span, StyleKind};

pub struct MarkdownHighlighter;

impl Highlighter for MarkdownHighlighter {
    fn highlight_line(&self, text: &str, state: &mut LineState) -> Vec<Span> {
        let chars: Vec<char> = text.chars().collect();
        let n = chars.len();
        let trimmed_start = chars.iter().take_while(|c| c.is_whitespace()).count();
        let is_fence = {
            let rest: String = chars[trimmed_start..].iter().collect();
            rest.starts_with("```") || rest.starts_with("~~~")
        };

        // Fenced code block handling.
        if state.in_code_block {
            if is_fence {
                state.in_code_block = false;
                return vec![Span::new(0, n, StyleKind::MarkupPunct)];
            }
            return vec![Span::new(0, n, StyleKind::Code)];
        }
        if is_fence {
            state.in_code_block = true;
            return vec![Span::new(0, n, StyleKind::MarkupPunct)];
        }

        // A callout block continues only across consecutive `>` lines.
        let is_blockquote = trimmed_start < n && chars[trimmed_start] == '>';
        if !is_blockquote {
            state.in_callout = false;
        }

        // Heading: #{1,6} followed by space.
        if let Some(level) = heading_level(&chars, trimmed_start) {
            let mut spans = Vec::new();
            let marker_end = trimmed_start + level;
            spans.push(Span::new(trimmed_start, marker_end, StyleKind::MarkupPunct));
            let mut text_span = Span::new(marker_end, n, StyleKind::Heading);
            text_span.bold = true;
            spans.push(text_span);
            return spans;
        }

        // Block quote / callout (`> [!type] Title`).
        if is_blockquote {
            let mut content = trimmed_start + 1;
            if content < n && chars[content] == ' ' {
                content += 1;
            }
            let mut spans = Vec::new();

            if let Some(type_end) = callout_marker_end(&chars, content, n) {
                // Callout header: accent bar, dimmed [!type], styled title.
                state.in_callout = true;
                spans.push(Span::new(0, content, StyleKind::Callout));
                spans.push(Span::new(content, type_end, StyleKind::MarkupPunct));
                let mut title = Span::new(type_end, n, StyleKind::Callout);
                title.bold = true;
                spans.push(title);
                inline(&chars, type_end, n, &mut spans, StyleKind::Default);
                return spans;
            }

            // Plain blockquote line, or the body of a callout.
            let bar = if state.in_callout { StyleKind::Callout } else { StyleKind::MarkupPunct };
            spans.push(Span::new(0, content, bar));
            inline(&chars, content, n, &mut spans, StyleKind::Quote);
            return spans;
        }

        // List item: -, *, + or "N." marker.
        if let Some(marker_end) = list_marker(&chars, trimmed_start, n) {
            let mut spans = vec![Span::new(trimmed_start, marker_end, StyleKind::ListMarker)];
            inline(&chars, marker_end, n, &mut spans, StyleKind::Default);
            return spans;
        }

        // Plain paragraph line with inline styling.
        let mut spans = Vec::new();
        inline(&chars, 0, n, &mut spans, StyleKind::Default);
        spans
    }
}

fn heading_level(chars: &[char], start: usize) -> Option<usize> {
    let mut level = 0;
    let mut i = start;
    while i < chars.len() && chars[i] == '#' {
        level += 1;
        i += 1;
    }
    if (1..=6).contains(&level) && i < chars.len() && chars[i] == ' ' {
        Some(level)
    } else {
        None
    }
}

fn list_marker(chars: &[char], start: usize, n: usize) -> Option<usize> {
    if start >= n {
        return None;
    }
    let c = chars[start];
    if (c == '-' || c == '*' || c == '+') && start + 1 < n && chars[start + 1] == ' ' {
        return Some(start + 2);
    }
    // Ordered list: digits then '.' or ')' then space.
    let mut i = start;
    while i < n && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i > start && i + 1 < n && (chars[i] == '.' || chars[i] == ')') && chars[i + 1] == ' ' {
        return Some(i + 2);
    }
    None
}

/// Scan `chars[from..to]` for inline constructs, appending spans. Regions not
/// covered keep `base` styling (filled by the renderer as a gap otherwise).
fn inline(chars: &[char], from: usize, to: usize, out: &mut Vec<Span>, base: StyleKind) {
    if base != StyleKind::Default && to > from {
        out.push(Span::new(from, to, base));
    }
    let mut i = from;
    while i < to {
        let c = chars[i];
        // Inline code: `code`
        if c == '`' {
            if let Some(j) = find_char(chars, i + 1, to, '`') {
                out.push(Span::new(i, i + 1, StyleKind::MarkupPunct));
                out.push(Span::new(i + 1, j, StyleKind::Code));
                out.push(Span::new(j, j + 1, StyleKind::MarkupPunct));
                i = j + 1;
                continue;
            }
        }
        // Bold: **text**
        if c == '*' && i + 1 < to && chars[i + 1] == '*' {
            if let Some(j) = find_seq(chars, i + 2, to, '*', '*') {
                out.push(Span::new(i, i + 2, StyleKind::MarkupPunct));
                let mut s = Span::new(i + 2, j, StyleKind::Bold);
                s.bold = true;
                out.push(s);
                out.push(Span::new(j, j + 2, StyleKind::MarkupPunct));
                i = j + 2;
                continue;
            }
        }
        // Italic: *text* or _text_
        if (c == '*' || c == '_') && i + 1 < to && chars[i + 1] != c {
            if let Some(j) = find_char(chars, i + 1, to, c) {
                out.push(Span::new(i, i + 1, StyleKind::MarkupPunct));
                let mut s = Span::new(i + 1, j, StyleKind::Italic);
                s.italic = true;
                out.push(s);
                out.push(Span::new(j, j + 1, StyleKind::MarkupPunct));
                i = j + 1;
                continue;
            }
        }
        // Link: [text](url)
        if c == '[' {
            if let Some(close) = find_char(chars, i + 1, to, ']') {
                if close + 1 < to && chars[close + 1] == '(' {
                    if let Some(paren) = find_char(chars, close + 2, to, ')') {
                        out.push(Span::new(i, i + 1, StyleKind::MarkupPunct));
                        out.push(Span::new(i + 1, close, StyleKind::Link));
                        out.push(Span::new(close, paren + 1, StyleKind::MarkupPunct));
                        i = paren + 1;
                        continue;
                    }
                }
            }
        }
        // Inline HTML/XML tag: <font color="…"> … </font>
        if c == '<' {
            if let Some(end) = html_tag(chars, i, to, out) {
                i = end;
                continue;
            }
        }
        i += 1;
    }
}

/// One visible character of rendered inline Markdown (markup concealed).
pub struct InlineGlyph {
    pub ch: char,
    pub kind: StyleKind,
    pub bold: bool,
    pub italic: bool,
    /// Source char index in the text passed to `rendered_inline`.
    pub index: usize,
}

/// Render inline Markdown for preview: parse `text`, then emit only the visible
/// characters with their styles, concealing markup punctuation and HTML tag
/// structure. E.g. `*Svensk psalm*` → "Svensk psalm" (italic);
/// `<font ...>_(x)_</font>` → "(x)" (italic).
pub fn rendered_inline(text: &str) -> Vec<InlineGlyph> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut spans = Vec::new();
    inline(&chars, 0, n, &mut spans, StyleKind::Default);

    let mut kind = vec![StyleKind::Default; n];
    let mut bold = vec![false; n];
    let mut ital = vec![false; n];
    for s in &spans {
        for c in s.start..s.end.min(n) {
            kind[c] = s.kind;
            bold[c] = s.bold;
            ital[c] = s.italic;
        }
    }

    (0..n)
        .filter(|&c| !is_concealed(kind[c]))
        .map(|c| InlineGlyph { ch: chars[c], kind: kind[c], bold: bold[c], italic: ital[c], index: c })
        .collect()
}

fn is_concealed(k: StyleKind) -> bool {
    matches!(
        k,
        StyleKind::MarkupPunct | StyleKind::Tag | StyleKind::Attribute | StyleKind::String
    )
}

/// If `[!type]` starts at `start`, return the index just past the `]`.
fn callout_marker_end(chars: &[char], start: usize, n: usize) -> Option<usize> {
    if start + 2 < n && chars[start] == '[' && chars[start + 1] == '!' {
        let j = find_char(chars, start + 2, n, ']')?;
        if j > start + 2 {
            return Some(j + 1);
        }
    }
    None
}

/// Highlight an HTML/XML tag starting at `<` (index `i`): delimiters and `=` as
/// punctuation, the tag name, attribute names and quoted values. Returns the
/// index just past `>`, or `None` if it isn't a tag.
fn html_tag(chars: &[char], i: usize, to: usize, out: &mut Vec<Span>) -> Option<usize> {
    let mut p = i + 1;
    let slash = p < to && chars[p] == '/';
    if slash {
        p += 1;
    }
    if p >= to || !chars[p].is_ascii_alphabetic() {
        return None;
    }
    let gt = find_char(chars, p, to, '>')?;

    out.push(Span::new(i, p, StyleKind::MarkupPunct)); // `<` or `</`
    let mut q = p;
    while q < gt && (chars[q].is_ascii_alphanumeric() || chars[q] == '-' || chars[q] == ':') {
        q += 1;
    }
    out.push(Span::new(p, q, StyleKind::Tag));

    while q < gt {
        let c = chars[q];
        if c == '"' || c == '\'' {
            let end = find_char(chars, q + 1, gt, c).map(|e| e + 1).unwrap_or(gt);
            out.push(Span::new(q, end, StyleKind::String));
            q = end;
        } else if c == '=' || c == '/' {
            out.push(Span::new(q, q + 1, StyleKind::MarkupPunct));
            q += 1;
        } else if c.is_ascii_alphabetic() || c == '-' || c == ':' {
            let mut r = q;
            while r < gt && (chars[r].is_ascii_alphanumeric() || chars[r] == '-' || chars[r] == ':') {
                r += 1;
            }
            out.push(Span::new(q, r, StyleKind::Attribute));
            q = r;
        } else {
            q += 1;
        }
    }
    out.push(Span::new(gt, gt + 1, StyleKind::MarkupPunct)); // `>`
    Some(gt + 1)
}

fn find_char(chars: &[char], from: usize, to: usize, target: char) -> Option<usize> {
    (from..to).find(|&i| chars[i] == target)
}

fn find_seq(chars: &[char], from: usize, to: usize, a: char, b: char) -> Option<usize> {
    let mut i = from;
    while i + 1 < to {
        if chars[i] == a && chars[i + 1] == b {
            return Some(i);
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::MarkdownHighlighter;
    use crate::syntax::{Highlighter, LineState, StyleKind};

    fn hl(text: &str, state: &mut LineState) -> Vec<super::Span> {
        MarkdownHighlighter.highlight_line(text, state)
    }

    #[test]
    fn callout_header_sets_state_and_styles() {
        let mut st = LineState::default();
        let spans = hl("> [!key] Åkallan *Svensk psalm*", &mut st);
        assert!(st.in_callout);
        assert!(spans.iter().any(|s| s.kind == StyleKind::Callout)); // accent bar/title
        assert!(spans.iter().any(|s| s.kind == StyleKind::MarkupPunct)); // [!key]
        assert!(spans.iter().any(|s| s.kind == StyleKind::Italic)); // *Svensk psalm*
    }

    #[test]
    fn callout_body_uses_accent_bar() {
        let mut st = LineState { in_callout: true, ..Default::default() };
        let spans = hl("> Gud, kom till min räddning,", &mut st);
        let bar = spans.iter().find(|s| s.start == 0).unwrap();
        assert_eq!(bar.kind, StyleKind::Callout);
        assert!(st.in_callout);
    }

    #[test]
    fn plain_blockquote_is_not_a_callout() {
        let mut st = LineState::default();
        let spans = hl("> just a quote", &mut st);
        assert!(!st.in_callout);
        assert!(spans.iter().any(|s| s.kind == StyleKind::Quote));
    }

    #[test]
    fn non_blockquote_line_clears_callout() {
        let mut st = LineState { in_callout: true, ..Default::default() };
        hl("back to normal text", &mut st);
        assert!(!st.in_callout);
    }

    #[test]
    fn inline_html_tag_highlighted() {
        let mut st = LineState::default();
        let spans = hl(r##"<font color="#c00000">_(Tystnad)_</font>"##, &mut st);
        assert!(spans.iter().any(|s| s.kind == StyleKind::Tag)); // font
        assert!(spans.iter().any(|s| s.kind == StyleKind::Attribute)); // color
        assert!(spans.iter().any(|s| s.kind == StyleKind::String)); // "#c00000"
    }
}
