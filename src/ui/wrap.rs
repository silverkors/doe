//! Soft-wrap layout. A single buffer line can occupy several *visual rows*;
//! these helpers convert between buffer positions and visual `(line, subrow,
//! col)` coordinates so rendering, vertical movement, scrolling and the mouse
//! all agree. Everything works per buffer line on demand (no global layout),
//! so cost stays bound to the viewport.

use crate::editor::Buffer;

/// Lines longer than this are not wrapped (kept as a single segment) to avoid
/// materializing a huge line on every keystroke. Soft wrap targets prose, not
/// pathological single-line megafiles.
const WRAP_LINE_LIMIT: usize = 100_000;

/// Wrap segments of `line` for the given content `width`, as char offsets
/// `(start, end)` *within the line* (excluding the trailing newline). Always
/// returns at least one segment. Breaks at the last whitespace before the
/// width when possible, otherwise hard-breaks.
pub fn segments(buf: &Buffer, line: usize, width: usize) -> Vec<(usize, usize)> {
    let len = buf.line_len_chars(line);
    // Skip wrapping only when it's disabled (width 0) or the line is so long
    // that materializing it would be pathological. A line that is <= width
    // *chars* may still exceed width *cells* once tabs expand, so it can't take
    // a fast path — but it's short, so measuring it is cheap anyway.
    if width == 0 || len > WRAP_LINE_LIMIT {
        return vec![(0, len)];
    }
    let start = buf.rope.line_to_char(line);
    let chars: Vec<char> = buf.rope.slice(start..start + len).chars().collect();
    let stops = buf.tabstops();

    let mut segs = Vec::new();
    let mut seg_start = 0; // char index where the current visual row begins
    let mut col = 0; // absolute display column from the start of the line
    let mut seg_col = 0; // display column relative to `seg_start` (the row's x)
    let mut last_ws: Option<usize> = None; // char after the last whitespace in row
    let mut i = 0;
    while i < len {
        let c = chars[i];
        // Tab cell-width depends on the *absolute* line column, so stops stay
        // anchored to the document's layout rather than to each wrapped row.
        let w = if c == '\t' { stops.tab_width_at(col) } else { 1 };
        if seg_col + w > width && i > seg_start {
            // Break: prefer the last whitespace boundary in this row.
            let brk = last_ws.filter(|&b| b > seg_start).unwrap_or(i);
            segs.push((seg_start, brk));
            // Re-establish the running columns at the new row's start and rewind
            // to it: chars between a whitespace break and `i` belong to this row.
            seg_start = brk;
            seg_col = 0;
            col = stops.char_to_col(&chars, brk);
            last_ws = None;
            i = brk;
            continue;
        }
        col += w;
        seg_col += w;
        if c == ' ' || c == '\t' {
            last_ws = Some(i + 1);
        }
        i += 1;
    }
    segs.push((seg_start, len));
    segs
}

/// Number of visual rows a buffer line occupies.
pub fn subrows(buf: &Buffer, line: usize, width: usize) -> usize {
    segments(buf, line, width).len()
}

/// Visual coordinates of a buffer char position: `(line, subrow, col)`, where
/// `col` is the tab-expanded **display column** within the visual row.
pub fn vpos_of(buf: &Buffer, pos: usize, width: usize) -> (usize, usize, usize) {
    let pos = pos.min(buf.len_chars());
    let line = buf.rope.char_to_line(pos);
    let lstart = buf.rope.line_to_char(line);
    let off = (pos - lstart).min(buf.line_len_chars(line));
    let segs = segments(buf, line, width);
    for (i, (s, e)) in segs.iter().enumerate() {
        if off < *e || i == segs.len() - 1 {
            let col = buf.display_col(line, off) - buf.display_col(line, *s);
            return (line, i, col);
        }
    }
    (line, 0, 0)
}

/// Buffer char position for a visual cell `(line, subrow, col)`, treating `col`
/// as a display column within the row (snaps onto the nearest char boundary).
pub fn pos_at(buf: &Buffer, line: usize, subrow: usize, col: usize, width: usize) -> usize {
    let line = line.min(buf.len_lines().saturating_sub(1));
    let segs = segments(buf, line, width);
    let sub = subrow.min(segs.len() - 1);
    let (s, e) = segs[sub];
    let target = buf.display_col(line, s) + col;
    let off = buf.char_off_for_col(line, target).clamp(s, e);
    buf.rope.line_to_char(line) + off
}

/// The visual row after `(line, subrow)`, or `None` at end of buffer.
pub fn next_visual(buf: &Buffer, line: usize, subrow: usize, width: usize) -> Option<(usize, usize)> {
    if subrow + 1 < subrows(buf, line, width) {
        Some((line, subrow + 1))
    } else if line + 1 < buf.len_lines() {
        Some((line + 1, 0))
    } else {
        None
    }
}

/// The visual row before `(line, subrow)`, or `None` at start of buffer.
pub fn prev_visual(buf: &Buffer, line: usize, subrow: usize, width: usize) -> Option<(usize, usize)> {
    if subrow > 0 {
        Some((line, subrow - 1))
    } else if line > 0 {
        Some((line - 1, subrows(buf, line - 1, width) - 1))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::Buffer;
    use ropey::Rope;

    fn buf(s: &str) -> Buffer {
        let mut b = Buffer::empty();
        b.rope = Rope::from_str(s);
        b
    }

    #[test]
    fn wraps_at_word_boundaries() {
        let b = buf("hello world foo");
        assert_eq!(segments(&b, 0, 8), vec![(0, 6), (6, 12), (12, 15)]);
    }

    #[test]
    fn hard_breaks_long_word() {
        let b = buf("abcdefghij");
        assert_eq!(segments(&b, 0, 4), vec![(0, 4), (4, 8), (8, 10)]);
    }

    #[test]
    fn vpos_and_pos_at_roundtrip() {
        let b = buf("hello world foo");
        assert_eq!(vpos_of(&b, 8, 8), (0, 1, 2)); // 'r' in "world"
        assert_eq!(pos_at(&b, 0, 1, 2, 8), 8);
    }

    fn buf_tabs(s: &str, stops: Vec<usize>, default_every: usize) -> Buffer {
        use crate::editor::tabstops::{TabStop, TabStops};
        let mut b = buf(s);
        let stops = stops.into_iter().map(TabStop::left).collect();
        b.set_tabstops_for_test(TabStops::new(stops, default_every));
        b
    }

    #[test]
    fn vpos_expands_tab_to_display_column() {
        // "ab\tcd" with a stop at column 10: 'c' sits at display column 10.
        let b = buf_tabs("ab\tcd", vec![10], 4);
        assert_eq!(vpos_of(&b, 3, 80), (0, 0, 10)); // before 'c'
        assert_eq!(vpos_of(&b, 4, 80), (0, 0, 11)); // before 'd'
        // Click on column 10 lands on 'c' (char offset 3).
        assert_eq!(pos_at(&b, 0, 0, 10, 80), 3);
    }

    #[test]
    fn wraps_using_display_width_of_tabs() {
        // The tab alone expands to 8 cells (stop at 8), so "x\ty" needs 9 cells
        // and wraps at width 8.
        let b = buf_tabs("x\ty", vec![], 8);
        assert_eq!(segments(&b, 0, 8), vec![(0, 2), (2, 3)]);
    }

    #[test]
    fn short_line_is_single_segment() {
        let b = buf("hi");
        assert_eq!(segments(&b, 0, 80), vec![(0, 2)]);
        assert_eq!(subrows(&b, 0, 80), 1);
    }
}
