//! Editor core: text buffer, cursors, selections and undo history.

pub mod buffer;
pub mod cursor;
pub mod selection;
pub mod undo;

pub use buffer::Buffer;
