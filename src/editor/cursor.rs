//! Cursor and selection model. A cursor is a `head` position with an `anchor`;
//! when they differ the cursor has a selection. Multi-cursor editing is just a
//! `Vec<Cursor>` on the buffer.

/// A single caret, expressed as char indices into the rope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// The moving end of the cursor (where text is inserted).
    pub head: usize,
    /// The fixed end of a selection. Equal to `head` when nothing is selected.
    pub anchor: usize,
    /// Preferred visual column, remembered across vertical movement.
    pub goal_col: Option<usize>,
}

impl Cursor {
    pub fn new(pos: usize) -> Self {
        Cursor { head: pos, anchor: pos, goal_col: None }
    }

    /// True when the cursor spans a selection.
    pub fn has_selection(&self) -> bool {
        self.head != self.anchor
    }

    /// Selection bounds as `(start, end)` with `start <= end`.
    pub fn range(&self) -> (usize, usize) {
        if self.head <= self.anchor {
            (self.head, self.anchor)
        } else {
            (self.anchor, self.head)
        }
    }

    /// Collapse any selection onto the head.
    pub fn collapse(&mut self) {
        self.anchor = self.head;
    }
}
