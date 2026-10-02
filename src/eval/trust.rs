//! Per-folder trust for running document code. Executing embedded code is
//! arbitrary code execution, so nothing runs until the user trusts the
//! document's folder. Trusted directories persist in `<config>/trust.toml`;
//! "run once" grants are session-only and never written.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Default, Serialize, Deserialize)]
struct TrustFile {
    #[serde(default)]
    dirs: Vec<String>,
}

#[derive(Default)]
pub struct TrustStore {
    dirs: HashSet<PathBuf>,
    /// Folders trusted only for this session (the "once" grants live elsewhere;
    /// this set lets an untitled buffer's run be remembered within the session).
    session: HashSet<PathBuf>,
    path: PathBuf,
}

impl TrustStore {
    pub fn load(config_dir: &Path) -> TrustStore {
        let path = config_dir.join("trust.toml");
        let dirs = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| toml::from_str::<TrustFile>(&t).ok())
            // Relative entries (from older versions) would match a folder of
            // that name anywhere, or every folder for `""`; drop them.
            .map(|f| f.dirs.into_iter().map(PathBuf::from).filter(|p| p.is_absolute()).collect())
            .unwrap_or_default();
        TrustStore { dirs, session: HashSet::new(), path }
    }

    pub fn is_trusted(&self, dir: &Path) -> bool {
        self.dirs.contains(dir) || self.session.contains(dir)
    }

    /// Persistently trust a folder (writes `trust.toml`).
    pub fn trust(&mut self, dir: PathBuf) {
        if !dir.is_absolute() {
            return;
        }
        self.dirs.insert(dir);
        self.save();
    }

    /// Trust a folder for this session only (not written to disk).
    pub fn trust_session(&mut self, dir: PathBuf) {
        self.session.insert(dir);
    }

    fn save(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut dirs: Vec<String> = self.dirs.iter().map(|p| p.display().to_string()).collect();
        dirs.sort();
        if let Ok(text) = toml::to_string(&TrustFile { dirs }) {
            let _ = std::fs::write(&self.path, text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_entries_are_never_trusted() {
        let dir = std::env::temp_dir().join(format!("doe-test-trust-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("trust.toml"), "dirs = [\"\", \"sub\"]\n").unwrap();
        let mut t = TrustStore::load(&dir);
        assert!(!t.is_trusted(Path::new("")));
        assert!(!t.is_trusted(Path::new("sub")));
        t.trust(PathBuf::from(""));
        assert!(!t.is_trusted(Path::new("")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
