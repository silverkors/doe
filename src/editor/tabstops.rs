//! Word-style tab stops. A tab character (`\t`) is stored verbatim in the
//! buffer and *expands at render time* to reach the next tab stop, so moving a
//! stop reflows existing text (unlike inserting spaces). Custom stops are
//! declared per document in YAML front matter:
//!
//! ```text
//! ---
//! tabstops: [8, 24, 40]
//! # or, richer:
//! tabstops:
//!   - {col: 24, align: left}
//!   - {col: 60, align: right, leader: "."}
//! ---
//! ```
//!
//! With no declaration the document falls back to uniform stops every
//! `tab_width` columns — i.e. classic editor behaviour.
//!
//! This module owns the single source of truth for converting between a
//! character offset within a line and its **display column**. Everything that
//! used to assume "1 char = 1 cell" (rendering, the cursor, the mouse, soft
//! wrap, horizontal scroll) routes through here.

/// How text following a tab is aligned against its stop. Only [`TabAlign::Left`]
/// affects the column math; the others are honoured by the renderer's lookahead
/// (right/centre/decimal alignment) and are treated as `Left` for plain column
/// mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabAlign {
    #[default]
    Left,
    Right,
    Center,
    Decimal,
}

/// A single custom tab stop at display column `col`.
#[derive(Debug, Clone, PartialEq)]
pub struct TabStop {
    pub col: usize,
    pub align: TabAlign,
    /// Fill character drawn across the tab's whitespace (e.g. `.` for a dotted
    /// leader). `None` renders blanks.
    pub leader: Option<char>,
}

impl TabStop {
    pub fn left(col: usize) -> Self {
        TabStop { col, align: TabAlign::Left, leader: None }
    }
}

/// The ordered set of tab stops for one document, with a uniform fallback used
/// past the last explicit stop (and everywhere when there are none).
#[derive(Debug, Clone, PartialEq)]
pub struct TabStops {
    /// Explicit stops, sorted ascending by `col`, deduplicated.
    stops: Vec<TabStop>,
    /// Uniform stop spacing (>= 1) used beyond the last explicit stop.
    default_every: usize,
}

impl TabStops {
    /// Uniform stops every `width` columns and nothing explicit — equivalent to
    /// a classic fixed tab width.
    pub fn uniform(width: usize) -> Self {
        TabStops { stops: Vec::new(), default_every: width.max(1) }
    }

    /// Build from a list of explicit stops plus the uniform fallback. Stops are
    /// sorted and de-duplicated by column.
    pub fn new(mut stops: Vec<TabStop>, default_every: usize) -> Self {
        stops.sort_by_key(|s| s.col);
        stops.dedup_by_key(|s| s.col);
        TabStops { stops, default_every: default_every.max(1) }
    }

    #[allow(dead_code)] // used by the tab-stop ruler UI (later step)
    pub fn is_empty(&self) -> bool {
        self.stops.is_empty()
    }

    #[allow(dead_code)] // used by the tab-stop ruler UI (later step)
    pub fn explicit(&self) -> &[TabStop] {
        &self.stops
    }

    /// The display column of the first stop strictly greater than `col`. Beyond
    /// the last explicit stop it rounds up to the next multiple of
    /// `default_every` (always strictly greater than `col`).
    pub fn next_col(&self, col: usize) -> usize {
        if let Some(s) = self.stops.iter().find(|s| s.col > col) {
            return s.col;
        }
        // Next multiple of `default_every` strictly greater than `col`.
        let w = self.default_every;
        (col / w + 1) * w
    }

    /// Cell width a tab occupies when it starts at display column `col`.
    pub fn tab_width_at(&self, col: usize) -> usize {
        self.next_col(col) - col
    }

    /// Leader (fill) character for a tab starting at `col`, i.e. the leader of
    /// the explicit stop it reaches. `None` when the stop is the uniform
    /// fallback or declares no leader.
    pub fn leader_at(&self, col: usize) -> Option<char> {
        self.stops.iter().find(|s| s.col > col).and_then(|s| s.leader)
    }

    /// Display column of the char at offset `upto` within `chars` (i.e. the
    /// column the cursor sits at when placed before `chars[upto]`).
    pub fn char_to_col(&self, chars: &[char], upto: usize) -> usize {
        let mut col = 0;
        for &c in &chars[..upto.min(chars.len())] {
            col = if c == '\t' { self.next_col(col) } else { col + 1 };
        }
        col
    }

    /// Total display width of a whole line.
    #[allow(dead_code)] // used by the tab-stop ruler UI (later step)
    pub fn line_width(&self, chars: &[char]) -> usize {
        self.char_to_col(chars, chars.len())
    }

    /// Char offset nearest the target display `col`. A click landing inside a
    /// tab's whitespace snaps to whichever edge is closer.
    pub fn col_to_char(&self, chars: &[char], target: usize) -> usize {
        let mut col = 0;
        for (i, &c) in chars.iter().enumerate() {
            let w = if c == '\t' { self.tab_width_at(col) } else { 1 };
            if target < col + w {
                // Inside this cell span: snap to the nearer boundary.
                return if target - col < w.div_ceil(2) { i } else { i + 1 };
            }
            col += w;
        }
        chars.len()
    }
}

/// Resolve the tab stops for a document from its leading YAML front matter,
/// falling back to uniform `default_width` stops. Only the `tabstops:` key is
/// read; everything else in the front matter is ignored. Tolerant by design —
/// any malformed entry is skipped rather than failing the document.
pub fn from_document(text: &str, default_width: usize) -> TabStops {
    match frontmatter_block(text).and_then(tabstops_value) {
        Some(raw) => TabStops::new(parse_stops(&raw), default_width),
        None => TabStops::uniform(default_width),
    }
}

/// The lines between a leading `---` fence and its closing `---`/`...`, or
/// `None` when the document has no front matter.
fn frontmatter_block(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n"))?;
    let mut idx = 0;
    for line in rest.split_inclusive('\n') {
        let t = line.trim_end_matches(['\r', '\n']);
        if t == "---" || t == "..." {
            return Some(&rest[..idx]);
        }
        idx += line.len();
    }
    None
}

/// Extract the raw text of the `tabstops:` value — either the inline remainder
/// (flow `[...]`) or the indented block list that follows it.
fn tabstops_value(block: &str) -> Option<String> {
    let lines: Vec<&str> = block.lines().collect();
    let i = lines.iter().position(|l| l.trim_start().starts_with("tabstops:"))?;
    let inline = lines[i].trim_start().strip_prefix("tabstops:").unwrap().trim();
    if !inline.is_empty() {
        return Some(inline.to_string());
    }
    // Block form: collect the following more-indented `- ...` lines.
    let key_indent = lines[i].len() - lines[i].trim_start().len();
    let mut items = Vec::new();
    for l in &lines[i + 1..] {
        if l.trim().is_empty() {
            continue;
        }
        let indent = l.len() - l.trim_start().len();
        if indent <= key_indent {
            break;
        }
        if let Some(item) = l.trim_start().strip_prefix('-') {
            items.push(item.trim().to_string());
        }
    }
    if items.is_empty() {
        None
    } else {
        Some(format!("[{}]", items.join(",")))
    }
}

/// Parse a flow list like `[8, {col: 24, align: right}, 40]` into stops.
fn parse_stops(raw: &str) -> Vec<TabStop> {
    let inner = raw.trim().trim_start_matches('[').trim_end_matches(']');
    split_items(inner).iter().filter_map(|s| parse_stop_item(s)).collect()
}

/// Split on commas that are not inside `{...}`.
fn split_items(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '{' => {
                depth += 1;
                cur.push(c);
            }
            '}' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// Parse one item: a bare integer column, or a `{col: N, align: X, leader: "c"}`
/// map. Returns `None` for anything unrecognised.
fn parse_stop_item(item: &str) -> Option<TabStop> {
    let item = item.trim();
    if let Ok(col) = item.parse::<usize>() {
        return Some(TabStop::left(col));
    }
    let map = item.strip_prefix('{')?.strip_suffix('}')?;
    let (mut col, mut align, mut leader) = (None, TabAlign::Left, None);
    for field in map.split(',') {
        let (k, v) = field.split_once(':')?;
        let (k, v) = (k.trim(), v.trim().trim_matches(['"', '\'']));
        match k {
            "col" => col = v.parse::<usize>().ok(),
            "align" => {
                align = match v.to_ascii_lowercase().as_str() {
                    "right" => TabAlign::Right,
                    "center" | "centre" => TabAlign::Center,
                    "decimal" => TabAlign::Decimal,
                    _ => TabAlign::Left,
                }
            }
            "leader" => leader = v.chars().next(),
            _ => {}
        }
    }
    Some(TabStop { col: col?, align, leader })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_next_col_matches_fixed_width() {
        let t = TabStops::uniform(4);
        assert_eq!(t.next_col(0), 4);
        assert_eq!(t.next_col(1), 4);
        assert_eq!(t.next_col(4), 8);
        assert_eq!(t.next_col(5), 8);
    }

    #[test]
    fn explicit_then_uniform_fallback() {
        let t = TabStops::new(vec![TabStop::left(10), TabStop::left(20)], 8);
        assert_eq!(t.next_col(0), 10);
        assert_eq!(t.next_col(10), 20);
        assert_eq!(t.next_col(20), 24); // past last stop -> multiples of 8
        assert_eq!(t.next_col(25), 32);
    }

    #[test]
    fn stops_are_sorted_and_deduped() {
        let t = TabStops::new(vec![TabStop::left(20), TabStop::left(10), TabStop::left(10)], 8);
        assert_eq!(t.explicit().iter().map(|s| s.col).collect::<Vec<_>>(), vec![10, 20]);
    }

    #[test]
    fn char_to_col_expands_tabs() {
        // "ab\tcd" with a stop at column 10.
        let chars: Vec<char> = "ab\tcd".chars().collect();
        let t = TabStops::new(vec![TabStop::left(10)], 4);
        assert_eq!(t.char_to_col(&chars, 0), 0); // before 'a'
        assert_eq!(t.char_to_col(&chars, 2), 2); // before '\t'
        assert_eq!(t.char_to_col(&chars, 3), 10); // before 'c' -> at the stop
        assert_eq!(t.char_to_col(&chars, 4), 11); // before 'd'
        assert_eq!(t.line_width(&chars), 12);
    }

    #[test]
    fn col_to_char_snaps_within_tab() {
        let chars: Vec<char> = "ab\tcd".chars().collect();
        let t = TabStops::new(vec![TabStop::left(10)], 4);
        // tab spans cells [2,10): width 8, midpoint at +4.
        assert_eq!(t.col_to_char(&chars, 2), 2); // left edge -> before tab
        assert_eq!(t.col_to_char(&chars, 5), 2); // closer to left -> before tab
        assert_eq!(t.col_to_char(&chars, 6), 3); // closer to right -> after tab
        assert_eq!(t.col_to_char(&chars, 10), 3); // at 'c'
        assert_eq!(t.col_to_char(&chars, 99), 5); // past end -> line end
    }

    #[test]
    fn roundtrip_char_col_on_plain_text() {
        let chars: Vec<char> = "hello".chars().collect();
        let t = TabStops::uniform(4);
        for i in 0..=chars.len() {
            assert_eq!(t.col_to_char(&chars, t.char_to_col(&chars, i)), i);
        }
    }

    #[test]
    fn no_frontmatter_falls_back_to_uniform() {
        let t = from_document("# Heading\n\nbody\n", 4);
        assert!(t.is_empty());
        assert_eq!(t.next_col(0), 4);
    }

    #[test]
    fn flow_list_of_columns() {
        let doc = "---\ntabstops: [8, 24, 40]\n---\nbody\n";
        let t = from_document(doc, 4);
        assert_eq!(t.explicit().iter().map(|s| s.col).collect::<Vec<_>>(), vec![8, 24, 40]);
    }

    #[test]
    fn block_list_with_maps() {
        let doc = "---\ntitle: x\ntabstops:\n  - 24\n  - {col: 60, align: right, leader: \".\"}\nother: 1\n---\nbody\n";
        let t = from_document(doc, 4);
        let s = t.explicit();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0], TabStop::left(24));
        assert_eq!(s[1], TabStop { col: 60, align: TabAlign::Right, leader: Some('.') });
    }

    #[test]
    fn inline_map_and_bad_entries_skipped() {
        let doc = "---\ntabstops: [{col: 12, align: decimal}, oops, 30]\n---\n";
        let t = from_document(doc, 4);
        let s = t.explicit();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0], TabStop { col: 12, align: TabAlign::Decimal, leader: None });
        assert_eq!(s[1], TabStop::left(30));
    }

    #[test]
    fn unterminated_frontmatter_is_ignored() {
        let t = from_document("---\ntabstops: [8]\nno closing fence\n", 4);
        assert!(t.is_empty());
    }
}
