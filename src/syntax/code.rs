//! A lightweight, keyword-driven highlighter for programming and config
//! languages. It is line-based (no cross-line block comments yet) but covers
//! strings, line comments, numbers and per-language keyword sets — enough to
//! make code readable until tree-sitter lands in 0.3.

use super::highlighter::{Highlighter, LineState, Span, StyleKind};
use super::Language;

pub struct CodeHighlighter {
    keywords: &'static [&'static str],
    line_comment: Option<&'static str>,
}

impl CodeHighlighter {
    pub fn new(lang: Language) -> Self {
        CodeHighlighter { keywords: lang.keywords(), line_comment: lang.line_comment() }
    }
}

impl Highlighter for CodeHighlighter {
    fn highlight_line(&self, _line: usize, text: &str, _state: &mut LineState) -> Vec<Span> {
        let chars: Vec<char> = text.chars().collect();
        let n = chars.len();
        let mut spans = Vec::new();
        let mut i = 0;

        while i < n {
            let c = chars[i];

            // Line comment to end of line.
            if let Some(lc) = self.line_comment {
                let lc_chars: Vec<char> = lc.chars().collect();
                if matches_at(&chars, i, &lc_chars) {
                    spans.push(Span::new(i, n, StyleKind::Comment));
                    break;
                }
            }

            // Strings: "..." or '...'
            if c == '"' || c == '\'' {
                let mut j = i + 1;
                while j < n {
                    if chars[j] == '\\' {
                        j += 2;
                        continue;
                    }
                    if chars[j] == c {
                        j += 1;
                        break;
                    }
                    j += 1;
                }
                spans.push(Span::new(i, j.min(n), StyleKind::String));
                i = j.min(n);
                continue;
            }

            // Numbers.
            if c.is_ascii_digit() {
                let mut j = i;
                while j < n && (chars[j].is_ascii_alphanumeric() || chars[j] == '.' || chars[j] == '_') {
                    j += 1;
                }
                spans.push(Span::new(i, j, StyleKind::Number));
                i = j;
                continue;
            }

            // Identifiers / keywords.
            if c.is_alphabetic() || c == '_' {
                let mut j = i;
                while j < n && (chars[j].is_alphanumeric() || chars[j] == '_') {
                    j += 1;
                }
                let word: String = chars[i..j].iter().collect();
                if self.keywords.contains(&word.as_str()) {
                    spans.push(Span::new(i, j, StyleKind::Keyword));
                } else if j < n && chars[j] == '(' {
                    spans.push(Span::new(i, j, StyleKind::Function));
                } else if word.chars().next().is_some_and(|c| c.is_uppercase()) {
                    spans.push(Span::new(i, j, StyleKind::Type));
                }
                i = j;
                continue;
            }

            i += 1;
        }

        spans
    }
}

fn matches_at(chars: &[char], at: usize, needle: &[char]) -> bool {
    if at + needle.len() > chars.len() {
        return false;
    }
    chars[at..at + needle.len()] == *needle
}
