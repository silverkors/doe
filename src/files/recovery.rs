//! Crash recovery / invisible autosave.
//!
//! While DOE runs it continuously mirrors the open buffers into a recovery
//! store (`~/.config/doe/recovery/`): a `session.toml` listing the open buffers
//! (in order, with the active one) plus a `<id>.bak` file holding the live
//! content of every buffer that has unsaved changes (including never-saved
//! "untitled" buffers).
//!
//! On a clean exit the store is cleared. So if the store still exists at
//! startup, the previous run ended unexpectedly: DOE reopens those buffers and
//! restores their unsaved content, ready to be saved to the original file or,
//! for an untitled buffer, to a new one.

use ropey::Rope;
use serde::{Deserialize, Serialize};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Default)]
pub struct Session {
    pub active: usize,
    #[serde(default)]
    pub buffers: Vec<SessEntry>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SessEntry {
    pub id: u64,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub has_backup: bool,
}

pub struct Recovery {
    dir: PathBuf,
}

impl Recovery {
    pub fn new(config_dir: &Path) -> Self {
        Recovery { dir: config_dir.join("recovery") }
    }

    fn session_path(&self) -> PathBuf {
        self.dir.join("session.toml")
    }

    pub fn bak_path(&self, id: u64) -> PathBuf {
        self.dir.join(format!("{id}.bak"))
    }

    pub fn ensure_dir(&self) {
        let _ = std::fs::create_dir_all(&self.dir);
    }

    pub fn read_session(&self) -> Option<Session> {
        let text = std::fs::read_to_string(self.session_path()).ok()?;
        toml::from_str(&text).ok()
    }

    pub fn read_backup(&self, id: u64) -> Option<String> {
        std::fs::read_to_string(self.bak_path(id)).ok()
    }

    /// Write a buffer's content to its backup file.
    pub fn write_backup(&self, id: u64, rope: &Rope) -> std::io::Result<()> {
        let file = std::fs::File::create(self.bak_path(id))?;
        rope.write_to(BufWriter::new(file))
    }

    pub fn remove_backup(&self, id: u64) {
        let _ = std::fs::remove_file(self.bak_path(id));
    }

    /// Write the session index and prune `.bak` files for ids no longer present.
    pub fn write_session(&self, session: &Session) {
        if let Ok(text) = toml::to_string(session) {
            let _ = std::fs::write(self.session_path(), text);
        }
        // Prune stale backups.
        let keep: Vec<u64> = session.buffers.iter().filter(|b| b.has_backup).map(|b| b.id).collect();
        if let Ok(entries) = std::fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if let Some(stem) = name.strip_suffix(".bak") {
                    if let Ok(id) = stem.parse::<u64>() {
                        if !keep.contains(&id) {
                            let _ = std::fs::remove_file(entry.path());
                        }
                    }
                }
            }
        }
    }

    /// Remove the whole recovery store (called on a clean exit).
    pub fn clear(&self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
