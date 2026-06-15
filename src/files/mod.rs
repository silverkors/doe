//! File-related helpers. The heavy lifting (rope load/save, modified tracking,
//! external-change detection) lives on [`crate::editor::buffer::Buffer`]; this
//! module holds path utilities shared across the editor.

pub mod picker;
pub mod recovery;

use std::path::{Path, PathBuf};

/// Expand a leading `~` to the user's home directory.
pub fn expand_path(input: &str) -> PathBuf {
    let trimmed = input.trim();
    if let Some(rest) = trimmed.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    if trimmed == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(trimmed)
}

/// A short, display-friendly form of a path (relative to cwd when possible).
pub fn display_path(path: &Path) -> String {
    if let Ok(cwd) = std::env::current_dir() {
        if let Ok(rel) = path.strip_prefix(&cwd) {
            return rel.display().to_string();
        }
    }
    path.display().to_string()
}
