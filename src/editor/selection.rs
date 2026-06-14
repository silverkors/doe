//! Small helpers around selection ranges. Selections themselves live on the
//! [`crate::editor::cursor::Cursor`]; this module holds reusable range logic
//! used by selection-merging features.
#![allow(dead_code)]

/// Returns true if two half-open ranges overlap or touch.
pub fn overlaps(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 <= b.1 && b.0 <= a.1
}

/// Merge two touching/overlapping ranges into one.
pub fn merge(a: (usize, usize), b: (usize, usize)) -> (usize, usize) {
    (a.0.min(b.0), a.1.max(b.1))
}
