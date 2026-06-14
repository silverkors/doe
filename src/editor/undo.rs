//! Snapshot-based undo/redo. Ropey clones share their underlying tree nodes via
//! reference counting, so a snapshot is cheap even for very large files — only
//! the edited path is copied on the next mutation.

use super::cursor::Cursor;
use ropey::Rope;

/// One point-in-time editor state.
#[derive(Clone)]
pub struct Snapshot {
    pub rope: Rope,
    pub cursors: Vec<Cursor>,
}

/// Undo/redo stacks plus simple time/`coalesce` based grouping so that a run of
/// single-character inserts collapses into one undo step.
pub struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// Whether the next `record` should be merged with the previous one.
    coalescing: bool,
    max_depth: usize,
}

impl History {
    pub fn new() -> Self {
        History { undo: Vec::new(), redo: Vec::new(), coalescing: false, max_depth: 2000 }
    }

    /// Record the state *before* an edit. `coalesce` requests that this edit be
    /// grouped with the previous recorded edit (used for continuous typing).
    pub fn record(&mut self, snapshot: Snapshot, coalesce: bool) {
        self.redo.clear();
        if coalesce && self.coalescing && !self.undo.is_empty() {
            // Keep the earlier snapshot as the undo target; drop this one.
            self.coalescing = coalesce;
            return;
        }
        self.undo.push(snapshot);
        if self.undo.len() > self.max_depth {
            self.undo.remove(0);
        }
        self.coalescing = coalesce;
    }

    /// Break any active coalescing run (e.g. after movement or a space).
    pub fn break_coalescing(&mut self) {
        self.coalescing = false;
    }

    pub fn undo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let prev = self.undo.pop()?;
        self.redo.push(current);
        self.coalescing = false;
        Some(prev)
    }

    pub fn redo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        self.coalescing = false;
        Some(next)
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}
