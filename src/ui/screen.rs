//! A double-buffered terminal cell grid. The renderer draws a full frame into
//! the back buffer each tick; `flush` diffs it against the previously displayed
//! frame and emits escape sequences only for cells that actually changed. This
//! gives flicker-free, minimal-write rendering ("only redraw what's needed")
//! without the renderer having to reason about deltas itself.

use crossterm::style::{Attribute, Color, SetAttribute, SetBackgroundColor, SetForegroundColor};
use crossterm::{cursor::MoveTo, queue, style::Print};
use std::io::Write;

#[derive(Clone, PartialEq)]
pub struct Cell {
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub italic: bool,
}

impl Cell {
    fn blank(fg: Color, bg: Color) -> Self {
        Cell { ch: ' ', fg, bg, bold: false, italic: false }
    }
}

pub struct Screen {
    width: u16,
    height: u16,
    cells: Vec<Cell>,
    prev: Vec<Cell>,
    /// Where the visible terminal cursor should sit (None = hidden).
    pub cursor: Option<(u16, u16)>,
    default_fg: Color,
    default_bg: Color,
    /// Forces a full repaint on the next flush (after resize/theme change).
    dirty_all: bool,
}

impl Screen {
    pub fn new() -> Self {
        Screen {
            width: 0,
            height: 0,
            cells: Vec::new(),
            prev: Vec::new(),
            cursor: None,
            default_fg: Color::Reset,
            default_bg: Color::Reset,
            dirty_all: true,
        }
    }

    #[allow(dead_code)]
    pub fn width(&self) -> u16 {
        self.width
    }

    #[allow(dead_code)]
    pub fn height(&self) -> u16 {
        self.height
    }

    /// Prepare a fresh frame: resize if needed and clear to the default colours.
    pub fn begin(&mut self, width: u16, height: u16, fg: Color, bg: Color) {
        if width != self.width || height != self.height {
            self.width = width;
            self.height = height;
            self.cells = vec![Cell::blank(fg, bg); width as usize * height as usize];
            self.prev = vec![Cell::blank(Color::Reset, Color::Reset); self.cells.len()];
            self.dirty_all = true;
        }
        self.default_fg = fg;
        self.default_bg = bg;
        let blank = Cell::blank(fg, bg);
        for c in &mut self.cells {
            c.clone_from(&blank);
        }
        self.cursor = None;
    }

    pub fn set(&mut self, x: u16, y: u16, cell: Cell) {
        if x >= self.width || y >= self.height {
            return;
        }
        let idx = y as usize * self.width as usize + x as usize;
        self.cells[idx] = cell;
    }

    /// Write `text` starting at (x, y) with one style, returning the next free
    /// column. Characters past the right edge are clipped.
    pub fn put_str(
        &mut self,
        mut x: u16,
        y: u16,
        text: &str,
        fg: Color,
        bg: Color,
        bold: bool,
        italic: bool,
    ) -> u16 {
        for ch in text.chars() {
            if x >= self.width {
                break;
            }
            self.set(x, y, Cell { ch, fg, bg, bold, italic });
            x += 1;
        }
        x
    }

    /// Force a complete repaint on the next flush.
    pub fn mark_all_dirty(&mut self) {
        self.dirty_all = true;
    }

    /// Diff against the previous frame and write only the changes.
    pub fn flush(&mut self, out: &mut impl Write) -> std::io::Result<()> {
        let mut cur_fg: Option<Color> = None;
        let mut cur_bg: Option<Color> = None;
        let mut cur_bold = false;
        let mut cur_italic = false;
        let mut pen: Option<(u16, u16)> = None;

        for y in 0..self.height {
            for x in 0..self.width {
                let idx = y as usize * self.width as usize + x as usize;
                if !self.dirty_all && self.cells[idx] == self.prev[idx] {
                    continue;
                }
                let cell = self.cells[idx].clone();

                if pen != Some((x, y)) {
                    queue!(out, MoveTo(x, y))?;
                }
                if cur_bold != cell.bold {
                    let a = if cell.bold { Attribute::Bold } else { Attribute::NormalIntensity };
                    queue!(out, SetAttribute(a))?;
                    cur_bold = cell.bold;
                }
                if cur_italic != cell.italic {
                    let a = if cell.italic { Attribute::Italic } else { Attribute::NoItalic };
                    queue!(out, SetAttribute(a))?;
                    cur_italic = cell.italic;
                }
                if cur_fg != Some(cell.fg) {
                    queue!(out, SetForegroundColor(cell.fg))?;
                    cur_fg = Some(cell.fg);
                }
                if cur_bg != Some(cell.bg) {
                    queue!(out, SetBackgroundColor(cell.bg))?;
                    cur_bg = Some(cell.bg);
                }
                queue!(out, Print(cell.ch))?;
                pen = Some((x + 1, y));
            }
        }

        // Swap front/back buffers.
        std::mem::swap(&mut self.cells, &mut self.prev);
        self.dirty_all = false;
        Ok(())
    }
}

impl Default for Screen {
    fn default() -> Self {
        Self::new()
    }
}
