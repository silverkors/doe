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

        // Block quote.
        if trimmed_start < n && chars[trimmed_start] == '>' {
            let mut spans = vec![Span::new(0, trimmed_start + 1, StyleKind::MarkupPunct)];
            inline(&chars, trimmed_start + 1, n, &mut spans, StyleKind::Quote);
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
        i += 1;
    }
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
