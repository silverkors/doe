//! Mouse model helpers. Translating a click into a buffer position needs the
//! current viewport and gutter width, which live on the app, so the actual
//! hit-testing happens in `App`. This module documents the shared layout
//! contract and provides the gutter-width calculation used by both rendering
//! and mouse handling so they never disagree.

/// Width of the line-number gutter (including trailing separator space) for a
/// buffer with `total_lines` lines, or 0 when line numbers are disabled.
pub fn gutter_width(total_lines: usize, line_numbers: bool) -> u16 {
    if !line_numbers {
        return 0;
    }
    let digits = total_lines.max(1).to_string().len() as u16;
    // digits + one space of left padding + one space separator.
    digits + 2
}
