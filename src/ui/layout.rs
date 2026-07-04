//! Screen layout arithmetic. Keeps the geometry (text area height, gutter
//! width, status/command rows) in one place so rendering and mouse hit-testing
//! always agree.

use crate::input::mouse::gutter_width;

pub struct Layout {
    pub width: u16,
    #[allow(dead_code)]
    pub height: u16,
    pub gutter: u16,
    /// Whether the tab-stop ruler row is shown above the text area.
    pub ruler: bool,
    /// Number of text rows (excludes the ruler and the status/command line).
    pub text_rows: u16,
    /// Row index of the combined status / command / message line.
    pub status_row: u16,
}

impl Layout {
    pub fn compute(width: u16, height: u16, total_lines: usize, line_numbers: bool, ruler: bool) -> Layout {
        let gutter = gutter_width(total_lines, line_numbers);
        // Reserve the last row for the combined status/command line, and the
        // first row for the ruler when it's enabled.
        let text_rows = height.saturating_sub(1 + ruler as u16);
        Layout {
            width,
            height,
            gutter,
            ruler,
            text_rows,
            status_row: height.saturating_sub(1),
        }
    }

    /// First text column (after the gutter).
    pub fn text_x(&self) -> u16 {
        self.gutter
    }

    /// First text row (below the ruler when shown).
    pub fn text_y(&self) -> u16 {
        self.ruler as u16
    }

    /// Width available for text content.
    pub fn text_width(&self) -> u16 {
        self.width.saturating_sub(self.gutter)
    }
}
