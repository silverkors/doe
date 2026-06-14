//! Styling primitives shared by all highlighters. A highlighter turns one line
//! of text into a list of [`Span`]s; the renderer maps each [`StyleKind`] to a
//! colour from the active theme and fills any gaps with the default style.

/// Semantic style categories. The theme resolves these to concrete colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleKind {
    Default,
    Keyword,
    Type,
    Function,
    String,
    Number,
    Comment,
    // Markdown
    Heading,
    Bold,
    Italic,
    Code,
    Link,
    ListMarker,
    Quote,
    /// Dimmed markup punctuation (`**`, backticks, `#`, …).
    MarkupPunct,
}

/// A styled run of characters, expressed in char columns within a single line.
#[derive(Debug, Clone, Copy)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: StyleKind,
    pub bold: bool,
    pub italic: bool,
}

impl Span {
    pub fn new(start: usize, end: usize, kind: StyleKind) -> Self {
        Span { start, end, kind, bold: false, italic: false }
    }
}

/// Per-line carry state so multi-line constructs (e.g. fenced code blocks) work
/// when the lines are visible together.
#[derive(Debug, Clone, Copy, Default)]
pub struct LineState {
    pub in_code_block: bool,
}

/// Anything that can highlight a line of text.
pub trait Highlighter {
    fn highlight_line(&self, text: &str, state: &mut LineState) -> Vec<Span>;
}
