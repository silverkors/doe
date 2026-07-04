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

    /// The first stop strictly greater than `col`: its column and alignment.
    /// Beyond the last explicit stop it rounds up to the next multiple of
    /// `default_every` (always strictly greater than `col`, always Left).
    fn stop_after(&self, col: usize) -> (usize, TabAlign) {
        if let Some(s) = self.stops.iter().find(|s| s.col > col) {
            return (s.col, s.align);
        }
        let w = self.default_every;
        ((col / w + 1) * w, TabAlign::Left)
    }

    /// The display column of the first stop strictly greater than `col`.
    pub fn next_col(&self, col: usize) -> usize {
        self.stop_after(col).0
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

    /// Per-char display layout of a line: for each char, the display column it
    /// starts at and the number of cells it occupies. Returns the spans plus the
    /// line's total display width. This is the single place tab expansion —
    /// including right/centre/decimal alignment lookahead — happens; everything
    /// else (rendering, cursor, mouse, wrap) derives from it.
    ///
    /// Alignment: a tab reaching a non-Left stop measures the *segment* that
    /// follows it (up to the next tab or end of line) and shrinks itself so the
    /// segment ends at (Right), centres on (Center), or puts its decimal
    /// separator at (Decimal) the stop column. A tab always occupies at least
    /// one cell, so overflowing segments degrade gracefully by shifting right.
    pub fn spans(&self, chars: &[char]) -> (Vec<CellSpan>, usize) {
        let mut out = Vec::with_capacity(chars.len());
        let mut col = 0;
        for (i, &c) in chars.iter().enumerate() {
            if c != '\t' {
                out.push(CellSpan { col, width: 1 });
                col += 1;
                continue;
            }
            let (stop_col, align) = self.stop_after(col);
            let seg_end = chars[i + 1..]
                .iter()
                .position(|&c| c == '\t')
                .map_or(chars.len(), |p| i + 1 + p);
            let seg = &chars[i + 1..seg_end];
            let target = match align {
                TabAlign::Left => stop_col,
                TabAlign::Right => stop_col.saturating_sub(seg.len()),
                TabAlign::Center => stop_col.saturating_sub(seg.len() / 2),
                TabAlign::Decimal => {
                    // Segment start so the separator lands on the stop; a
                    // number without a separator right-aligns (Word behaviour).
                    let int_len = seg.iter().position(|&c| c == '.' || c == ',').unwrap_or(seg.len());
                    stop_col.saturating_sub(int_len)
                }
            };
            let width = target.saturating_sub(col).max(1);
            out.push(CellSpan { col, width });
            col += width;
        }
        (out, col)
    }

    /// Display column of the char at offset `upto` within `chars` (i.e. the
    /// column the cursor sits at when placed before `chars[upto]`).
    pub fn char_to_col(&self, chars: &[char], upto: usize) -> usize {
        let (spans, total) = self.spans(chars);
        if upto >= chars.len() {
            total
        } else {
            spans[upto].col
        }
    }

    /// Total display width of a whole line.
    pub fn line_width(&self, chars: &[char]) -> usize {
        self.spans(chars).1
    }

    /// Char offset nearest the target display `col`. A click landing inside a
    /// tab's whitespace snaps to whichever edge is closer.
    pub fn col_to_char(&self, chars: &[char], target: usize) -> usize {
        let (spans, _) = self.spans(chars);
        for (i, s) in spans.iter().enumerate() {
            if target < s.col + s.width {
                return if target.saturating_sub(s.col) < s.width.div_ceil(2) { i } else { i + 1 };
            }
        }
        chars.len()
    }
}

/// One char's place in a line's display layout: starting column and cell width
/// (1 for everything except tabs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellSpan {
    pub col: usize,
    pub width: usize,
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

/// Render one stop back to its front-matter form: a bare column when it's a
/// plain left stop, otherwise a `{col, align, leader}` map omitting defaults.
fn serialize_stop(s: &TabStop) -> String {
    let mut fields = Vec::new();
    if s.align != TabAlign::Left {
        let a = match s.align {
            TabAlign::Right => "right",
            TabAlign::Center => "center",
            TabAlign::Decimal => "decimal",
            TabAlign::Left => "left",
        };
        fields.push(format!("align: {a}"));
    }
    if let Some(c) = s.leader {
        fields.push(format!("leader: \"{c}\""));
    }
    if fields.is_empty() {
        s.col.to_string()
    } else {
        format!("{{col: {}, {}}}", s.col, fields.join(", "))
    }
}

/// The `tabstops:` front-matter line for a set of stops (no trailing newline).
pub fn serialize_line(stops: &[TabStop]) -> String {
    let items: Vec<String> = stops.iter().map(serialize_stop).collect();
    format!("tabstops: [{}]", items.join(", "))
}

/// Compute the edit that writes `stops` into `text`'s front matter, as a
/// `(start, end, replacement)` triple in **char** offsets. An empty range with
/// empty replacement means "no change needed". Handles three cases: replacing an
/// existing `tabstops:` entry, inserting one into existing front matter, and
/// creating front matter when the document has none. Passing an empty `stops`
/// removes the entry (and never creates front matter).
pub fn splice_tabstops(text: &str, stops: &[TabStop]) -> (usize, usize, String) {
    // (char_start, char_len_including_newline, trimmed_content) per line.
    let mut lines: Vec<(usize, usize, &str)> = Vec::new();
    let mut off = 0;
    for l in text.split_inclusive('\n') {
        let trimmed = l.trim_end_matches(['\r', '\n']);
        lines.push((off, l.chars().count(), trimmed));
        off += l.chars().count();
    }

    let has_fm = lines.first().map(|(_, _, t)| *t == "---").unwrap_or(false);
    let serialized = serialize_line(stops);

    if !has_fm {
        if stops.is_empty() {
            return (0, 0, String::new());
        }
        return (0, 0, format!("---\n{serialized}\n---\n\n"));
    }

    // Closing fence line index.
    let close = lines[1..]
        .iter()
        .position(|(_, _, t)| *t == "---" || *t == "...")
        .map(|i| i + 1);
    let close = match close {
        Some(c) => c,
        None => return (0, 0, String::new()), // malformed; leave it alone
    };

    // Find an existing `tabstops:` key within the front matter.
    let ti = (1..close).find(|&i| lines[i].2.trim_start().starts_with("tabstops:"));
    if let Some(ti) = ti {
        let key_indent = lines[ti].2.len() - lines[ti].2.trim_start().len();
        // Extend over any following indented block-list lines.
        let mut end_line = ti + 1;
        while end_line < close {
            let t = lines[end_line].2;
            let indent = t.len() - t.trim_start().len();
            if t.trim().is_empty() || indent <= key_indent {
                break;
            }
            end_line += 1;
        }
        let start = lines[ti].0;
        let end = lines[end_line].0; // start of the first line after the value
        if stops.is_empty() {
            return (start, end, String::new()); // drop the entry entirely
        }
        return (start, end, format!("{serialized}\n"));
    }

    if stops.is_empty() {
        return (0, 0, String::new());
    }
    // No key yet: insert right after the opening fence.
    let pos = lines[1].0;
    (pos, pos, format!("{serialized}\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spliced(text: &str, stops: &[TabStop]) -> String {
        let (s, e, rep) = splice_tabstops(text, stops);
        let chars: Vec<char> = text.chars().collect();
        let mut out: String = chars[..s].iter().collect();
        out.push_str(&rep);
        out.extend(&chars[e..]);
        out
    }

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

    fn stops(v: Vec<TabStop>) -> TabStops {
        TabStops::new(v, 4)
    }

    #[test]
    fn right_align_ends_segment_at_stop() {
        // "a\tbb" with a right stop at 10: "bb" occupies cols 8..10.
        let t = stops(vec![TabStop { col: 10, align: TabAlign::Right, leader: None }]);
        let chars: Vec<char> = "a\tbb".chars().collect();
        let (s, total) = t.spans(&chars);
        assert_eq!(s[1], CellSpan { col: 1, width: 7 }); // the tab
        assert_eq!(s[2].col, 8); // first 'b'
        assert_eq!(s[3].col, 9); // second 'b' ends flush at 10
        assert_eq!(total, 10);
    }

    #[test]
    fn center_align_centres_segment_on_stop() {
        // "a\tbbbb" centred on 10: segment starts at 8, middle at the stop.
        let t = stops(vec![TabStop { col: 10, align: TabAlign::Center, leader: None }]);
        let chars: Vec<char> = "a\tbbbb".chars().collect();
        let (s, _) = t.spans(&chars);
        assert_eq!(s[2].col, 8);
        // Odd-length segment: middle char lands exactly on the stop.
        let chars: Vec<char> = "a\tbbb".chars().collect();
        let (s, _) = t.spans(&chars);
        assert_eq!(s[3].col, 10); // middle 'b'
    }

    #[test]
    fn decimal_align_puts_separator_at_stop() {
        let t = stops(vec![TabStop { col: 10, align: TabAlign::Decimal, leader: None }]);
        // Dot separator.
        let chars: Vec<char> = "x\t12.5".chars().collect();
        let (s, _) = t.spans(&chars);
        assert_eq!(s[4].col, 10); // the '.'
        // Comma separator (Swedish locale).
        let chars: Vec<char> = "x\t3,14".chars().collect();
        let (s, _) = t.spans(&chars);
        assert_eq!(s[3].col, 10); // the ','
        // No separator: right-aligns against the stop.
        let chars: Vec<char> = "x\t1234".chars().collect();
        let (s, _) = t.spans(&chars);
        assert_eq!(s[5].col, 9); // last digit ends flush at 10
    }

    #[test]
    fn aligned_overflow_keeps_minimum_one_cell() {
        // Segment wider than the space before the stop: the tab degrades to one
        // cell and the text just shifts right.
        let t = stops(vec![TabStop { col: 4, align: TabAlign::Right, leader: None }]);
        let chars: Vec<char> = "abc\tlongtext".chars().collect();
        let (s, _) = t.spans(&chars);
        assert_eq!(s[3].width, 1); // the tab
        assert_eq!(s[4].col, 4); // text follows immediately
    }

    #[test]
    fn alignment_only_reaches_to_next_tab() {
        // Two columns: the right-aligned segment is only up to the second tab.
        let t = stops(vec![
            TabStop { col: 10, align: TabAlign::Right, leader: None },
            TabStop::left(14),
        ]);
        let chars: Vec<char> = "a\tbb\tcc".chars().collect();
        let (s, _) = t.spans(&chars);
        assert_eq!(s[2].col, 8); // "bb" right-aligned to 10
        assert_eq!(s[5].col, 14); // "cc" left at the next stop
    }

    #[test]
    fn serialize_plain_and_rich_stops() {
        assert_eq!(serialize_line(&[TabStop::left(16), TabStop::left(32)]), "tabstops: [16, 32]");
        let rich = TabStop { col: 56, align: TabAlign::Right, leader: Some('.') };
        assert_eq!(serialize_line(&[rich]), "tabstops: [{col: 56, align: right, leader: \".\"}]");
    }

    #[test]
    fn splice_creates_frontmatter_when_absent() {
        let out = spliced("# Title\n\nbody\n", &[TabStop::left(16)]);
        assert_eq!(out, "---\ntabstops: [16]\n---\n\n# Title\n\nbody\n");
    }

    #[test]
    fn splice_inserts_into_existing_frontmatter() {
        let out = spliced("---\ntitle: x\n---\nbody\n", &[TabStop::left(16)]);
        assert_eq!(out, "---\ntabstops: [16]\ntitle: x\n---\nbody\n");
    }

    #[test]
    fn splice_replaces_inline_entry() {
        let out = spliced("---\ntabstops: [8]\ntitle: x\n---\nbody\n", &[TabStop::left(16), TabStop::left(40)]);
        assert_eq!(out, "---\ntabstops: [16, 40]\ntitle: x\n---\nbody\n");
    }

    #[test]
    fn splice_replaces_block_entry() {
        let doc = "---\ntabstops:\n  - 8\n  - 24\ntitle: x\n---\nbody\n";
        let out = spliced(doc, &[TabStop::left(16)]);
        assert_eq!(out, "---\ntabstops: [16]\ntitle: x\n---\nbody\n");
    }

    #[test]
    fn splice_empty_removes_entry_but_keeps_frontmatter() {
        let out = spliced("---\ntabstops: [8]\ntitle: x\n---\nbody\n", &[]);
        assert_eq!(out, "---\ntitle: x\n---\nbody\n");
    }

    #[test]
    fn splice_roundtrips_through_parser() {
        let stops = vec![TabStop::left(12), TabStop { col: 40, align: TabAlign::Right, leader: Some('.') }];
        let out = spliced("body only\n", &stops);
        let parsed = from_document(&out, 4);
        assert_eq!(parsed.explicit(), stops.as_slice());
    }
}
