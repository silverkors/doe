//! Per-file cursor memory. Remembers the cursor position of every file you've
//! had open (`positions.toml` in the config dir, most-recent first, capped) and
//! restores it when the file is opened again — you continue where you left off.
//! Positions are stored as `(line, char-column)` so they survive edits made by
//! other tools better than a raw char offset would; both are clamped on
//! restore.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Keep this many entries (LRU by last open/close).
const MAX_ENTRIES: usize = 500;

#[derive(Serialize, Deserialize, Default)]
struct PositionsFile {
    #[serde(default)]
    entries: Vec<Entry>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
struct Entry {
    path: String,
    line: usize,
    col: usize,
}

pub struct PositionStore {
    entries: Vec<Entry>,
    store_path: PathBuf,
    dirty: bool,
}

impl PositionStore {
    pub fn new(config_dir: &Path) -> Self {
        let store_path = config_dir.join("positions.toml");
        let entries = std::fs::read_to_string(&store_path)
            .ok()
            .and_then(|t| toml::from_str::<PositionsFile>(&t).ok())
            .map(|f| f.entries)
            .unwrap_or_default();
        PositionStore { entries, store_path, dirty: false }
    }

    fn canon(path: &Path) -> String {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf()).to_string_lossy().into_owned()
    }

    /// Last known cursor position for `path`, as `(line, char-column)`.
    pub fn get(&self, path: &Path) -> Option<(usize, usize)> {
        let key = Self::canon(path);
        self.entries.iter().find(|e| e.path == key).map(|e| (e.line, e.col))
    }

    /// Remember the cursor position for `path` (front of the list, deduped).
    /// No-op when the stored position is already current, so frequent callers
    /// (autosave) don't dirty the store needlessly.
    pub fn record(&mut self, path: &Path, line: usize, col: usize) {
        let key = Self::canon(path);
        if self.entries.first().is_some_and(|e| e.path == key && e.line == line && e.col == col) {
            return;
        }
        self.entries.retain(|e| e.path != key);
        self.entries.insert(0, Entry { path: key, line, col });
        self.entries.truncate(MAX_ENTRIES);
        self.dirty = true;
    }

    /// Persist to disk if anything changed (best effort).
    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }
        if let Ok(text) = toml::to_string(&PositionsFile { entries: self.entries.clone() }) {
            let _ = std::fs::write(&self.store_path, text);
        }
        self.dirty = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("doe-pos-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn roundtrip_and_lru_order() {
        let dir = tmpdir();
        let _ = std::fs::remove_file(dir.join("positions.toml"));
        let a = dir.join("a.md");
        let b = dir.join("b.md");
        std::fs::write(&a, "x").unwrap();
        std::fs::write(&b, "y").unwrap();

        let mut s = PositionStore::new(&dir);
        s.record(&a, 10, 4);
        s.record(&b, 2, 0);
        s.record(&a, 12, 1); // update moves to front
        s.save();

        let s2 = PositionStore::new(&dir);
        assert_eq!(s2.get(&a), Some((12, 1)));
        assert_eq!(s2.get(&b), Some((2, 0)));
        assert_eq!(s2.entries.len(), 2);
        assert_eq!(s2.entries[0].path, PositionStore::canon(&a));
    }

    #[test]
    fn unchanged_record_stays_clean() {
        let dir = tmpdir();
        let a = dir.join("a.md");
        std::fs::write(&a, "x").unwrap();
        let mut s = PositionStore::new(&dir);
        s.record(&a, 3, 3);
        s.save();
        s.record(&a, 3, 3); // same position again
        assert!(!s.dirty);
    }
}
