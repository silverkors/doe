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
    if width == 0 || len <= width || len > WRAP_LINE_LIMIT {
        return vec![(0, len)];
    }
    let start = buf.rope.line_to_char(line);
    let chars: Vec<char> = buf.rope.slice(start..start + len).chars().collect();

    let mut segs = Vec::new();
    let mut s = 0;
    while s < len {
        let mut e = (s + width).min(len);
        if e < len {
            // Prefer breaking at the last whitespace inside this window.
            if let Some(b) = (s..e).rev().find(|&i| chars[i] == ' ' || chars[i] == '\t') {
                if b + 1 > s {
                    e = b + 1; // keep the breaking space at the end of this row
                }
            }
        }
        if e <= s {
            e = (s + width).min(len).max(s + 1);
        }
        segs.push((s, e));
        s = e;
    }
    if segs.is_empty() {
        segs.push((0, 0));
    }
    segs
}

/// Number of visual rows a buffer line occupies.
pub fn subrows(buf: &Buffer, line: usize, width: usize) -> usize {
    segments(buf, line, width).len()
}

/// Visual coordinates of a buffer char position: `(line, subrow, col)`.
pub fn vpos_of(buf: &Buffer, pos: usize, width: usize) -> (usize, usize, usize) {
    let pos = pos.min(buf.len_chars());
    let line = buf.rope.char_to_line(pos);
    let lstart = buf.rope.line_to_char(line);
    let off = (pos - lstart).min(buf.line_len_chars(line));
    let segs = segments(buf, line, width);
    for (i, (s, e)) in segs.iter().enumerate() {
        if off < *e || i == segs.len() - 1 {
            return (line, i, off - s);
        }
    }
    (line, 0, 0)
}

/// Buffer char position for a visual cell `(line, subrow, col)`.
pub fn pos_at(buf: &Buffer, line: usize, subrow: usize, col: usize, width: usize) -> usize {
    let line = line.min(buf.len_lines().saturating_sub(1));
    let segs = segments(buf, line, width);
    let sub = subrow.min(segs.len() - 1);
    let (s, e) = segs[sub];
    let off = (s + col.min(e - s)).min(buf.line_len_chars(line));
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

    #[test]
    fn short_line_is_single_segment() {
        let b = buf("hi");
        assert_eq!(segments(&b, 0, 80), vec![(0, 2)]);
        assert_eq!(subrows(&b, 0, 80), 1);
    }
}
