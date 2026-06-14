//! Screen layout arithmetic. Keeps the geometry (text area height, gutter
//! width, status/command rows) in one place so rendering and mouse hit-testing
//! always agree.

use crate::input::mouse::gutter_width;

pub struct Layout {
    pub width: u16,
    #[allow(dead_code)]
    pub height: u16,
    pub gutter: u16,
    /// Number of text rows (excludes status bar + command line).
    pub text_rows: u16,
    /// Row index of the status bar.
    pub status_row: u16,
    /// Row index of the command/message line.
    pub command_row: u16,
}

impl Layout {
    pub fn compute(width: u16, height: u16, total_lines: usize, line_numbers: bool) -> Layout {
        let gutter = gutter_width(total_lines, line_numbers);
        // Reserve the last two rows for status bar and command line.
        let reserved = 2u16;
        let text_rows = height.saturating_sub(reserved);
        Layout {
            width,
            height,
            gutter,
            text_rows,
            status_row: height.saturating_sub(2),
            command_row: height.saturating_sub(1),
        }
    }

    /// First text column (after the gutter).
    pub fn text_x(&self) -> u16 {
        self.gutter
    }

    /// Width available for text content.
    pub fn text_width(&self) -> u16 {
        self.width.saturating_sub(self.gutter)
    }
}
