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
            .map(|f| f.dirs.into_iter().map(PathBuf::from).collect())
            .unwrap_or_default();
        TrustStore { dirs, session: HashSet::new(), path }
    }

    pub fn is_trusted(&self, dir: &Path) -> bool {
        self.dirs.contains(dir) || self.session.contains(dir)
    }

    /// Persistently trust a folder (writes `trust.toml`).
    pub fn trust(&mut self, dir: PathBuf) {
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
