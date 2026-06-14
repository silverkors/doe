//! Status bar text construction. Pure string formatting; the renderer paints
//! the result with theme colours.

use crate::editor::Buffer;

/// Left segment: file name and modified marker (DOE is modeless — no mode word).
pub fn left_text(buffer: &Buffer) -> String {
    let modified = if buffer.modified { " ●" } else { "" };
    format!(" {}{}", buffer.name(), modified)
}

/// Right segment: language, cursor position, selection/cursor counts, plus any
/// plugin-contributed segments.
pub fn right_text(buffer: &Buffer, plugin_segments: &[String], buffer_idx: usize, buffer_total: usize) -> String {
    let c = buffer.primary_cursor();
    let (line, col) = buffer.pos_to_line_col(c.head);
    let mut parts: Vec<String> = Vec::new();

    if buffer.cursors.len() > 1 {
        parts.push(format!("{} cursors", buffer.cursors.len()));
    }
    for seg in plugin_segments {
        parts.push(seg.clone());
    }
    parts.push(buffer.language.display_name().to_string());
    parts.push(format!("Ln {}, Col {}", line + 1, col + 1));
    parts.push(format!("[{}/{}]", buffer_idx + 1, buffer_total));

    format!("{} ", parts.join("  │  "))
}
