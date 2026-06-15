//! The Open picker — one searchable overlay that covers every way to open a
//! file:
//!
//! * **Recent files** — the most-recently-opened files (persisted), shown first
//!   when the query is empty; capped at 10 with a "show more" row that expands
//!   to the full history.
//! * **Fuzzy search** — type plain text to fuzzy-match across recent + project
//!   files (project files are scanned from the working directory).
//! * **Filesystem navigation** — type a path (anything containing `/`, or
//!   starting with `~`/`.`/`/`) to browse directories; pick a directory to
//!   descend into it, a file to open it.
//! * **Arbitrary paths** — in path mode the first row always opens exactly what
//!   you typed, existing or new (so you can create files or open outside the
//!   tree); in search mode an unmatched query offers to create it.

use crate::commands::palette::fuzzy;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const MAX_FILES: usize = 20_000;
const MAX_RECENT: usize = 50;
const RECENT_PREVIEW: usize = 10;
const MAX_LISTING: usize = 200;
const SKIP_DIRS: &[&str] = &["target", "node_modules", "dist", "build", "__pycache__", "vendor"];

/// What activating a result does.
#[derive(Clone)]
pub enum Activate {
    /// Open this (existing or new) file.
    Open(PathBuf),
    /// Replace the query with this directory path and keep browsing.
    EnterDir(String),
    /// Expand the recent list to show the full history.
    ExpandRecent,
}

/// The outcome of pressing Enter, for the app to act on.
pub enum Accept {
    Open(PathBuf),
    Stay,
}

pub struct PickResult {
    pub display: String,
    pub positions: Vec<usize>,
    pub hint: &'static str,
    pub action: Activate,
}

#[derive(Default)]
pub struct FilePicker {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    pub results: Vec<PickResult>,
    files: Vec<String>,
    recent: Vec<PathBuf>,
    recent_expanded: bool,
    root: PathBuf,
    recent_path: PathBuf,
    dirty: bool,
}

impl FilePicker {
    pub fn new(config_dir: &Path) -> Self {
        let recent_path = config_dir.join("recent.toml");
        let recent = load_recent(&recent_path);
        FilePicker { recent, recent_path, ..Default::default() }
    }

    pub fn open(&mut self, root: PathBuf) {
        self.files = scan(&root);
        self.root = root;
        self.query.clear();
        self.selected = 0;
        self.recent_expanded = false;
        self.open = true;
        self.update();
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.results.is_empty() {
            return;
        }
        let n = self.results.len() as isize;
        self.selected = (((self.selected as isize + delta) % n + n) % n) as usize;
    }

    /// Act on the current selection.
    pub fn accept(&mut self) -> Accept {
        let action = match self.results.get(self.selected) {
            Some(r) => r.action.clone(),
            None => return Accept::Stay,
        };
        match action {
            Activate::Open(p) => Accept::Open(p),
            Activate::EnterDir(d) => {
                self.query = d;
                self.selected = 0;
                self.update();
                Accept::Stay
            }
            Activate::ExpandRecent => {
                self.recent_expanded = true;
                self.update();
                Accept::Stay
            }
        }
    }

    /// Record that `path` was opened (front of the recent list, deduped).
    pub fn record_open(&mut self, path: &Path) {
        let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.recent.retain(|p| p != &canon);
        self.recent.insert(0, canon);
        self.recent.truncate(MAX_RECENT);
        self.dirty = true;
    }

    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }
        let files: Vec<String> = self.recent.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        if let Ok(text) = toml::to_string(&RecentFile { files }) {
            let _ = std::fs::write(&self.recent_path, text);
        }
        self.dirty = false;
    }

    pub fn update(&mut self) {
        let q = self.query.trim().to_string();
        self.results.clear();
        if is_path_like(&q) {
            self.build_path_mode(&q);
        } else if q.is_empty() {
            self.build_default();
        } else {
            self.build_search(&q);
        }
        if self.selected >= self.results.len() {
            self.selected = self.results.len().saturating_sub(1);
        }
    }

    // --- result builders ---------------------------------------------------

    fn build_default(&mut self) {
        let limit = if self.recent_expanded { MAX_RECENT } else { RECENT_PREVIEW };
        let shown = self.recent.len().min(limit);
        for p in self.recent.iter().take(shown).cloned().collect::<Vec<_>>() {
            self.results.push(PickResult {
                display: nice(&p, &self.root),
                positions: Vec::new(),
                hint: "recent",
                action: Activate::Open(p),
            });
        }
        if !self.recent_expanded && self.recent.len() > RECENT_PREVIEW {
            let more = self.recent.len() - RECENT_PREVIEW;
            self.results.push(PickResult {
                display: format!("⋯ {more} more recent files"),
                positions: Vec::new(),
                hint: "expand",
                action: Activate::ExpandRecent,
            });
        }
        // Project files below recents.
        let recent_set: Vec<PathBuf> = self.recent.clone();
        for f in &self.files {
            let abs = self.root.join(f);
            if recent_set.iter().any(|r| r == &abs) {
                continue;
            }
            self.results.push(PickResult {
                display: f.clone(),
                positions: Vec::new(),
                hint: "",
                action: Activate::Open(abs),
            });
        }
    }

    fn build_search(&mut self, q: &str) {
        // Candidates: recent (deduped) + project files.
        let mut scored: Vec<(i32, PickResult)> = Vec::new();
        let recent_set: Vec<PathBuf> = self.recent.clone();
        for p in &self.recent {
            let disp = nice(p, &self.root);
            if let Some((score, positions)) = fuzzy(q, &disp) {
                scored.push((score + 30, PickResult { display: disp, positions, hint: "recent", action: Activate::Open(p.clone()) }));
            }
        }
        for f in &self.files {
            let abs = self.root.join(f);
            if recent_set.iter().any(|r| r == &abs) {
                continue;
            }
            if let Some((score, positions)) = fuzzy(q, f) {
                scored.push((score, PickResult { display: f.clone(), positions, hint: "", action: Activate::Open(abs) }));
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.display.len().cmp(&b.1.display.len())));
        self.results = scored.into_iter().map(|(_, r)| r).collect();

        // Offer to create a new file from the typed name when nothing matches.
        if self.results.is_empty() {
            let abs = self.root.join(q);
            self.results.push(PickResult {
                display: format!("Create \"{q}\""),
                positions: Vec::new(),
                hint: "new file",
                action: Activate::Open(abs),
            });
        }
    }

    fn build_path_mode(&mut self, q: &str) {
        let resolved = resolve(q, &self.root);

        // First row: open exactly what was typed (unless it's a bare directory).
        if !q.ends_with('/') {
            let exists = resolved.is_file();
            self.results.push(PickResult {
                display: format!("Open {q}"),
                positions: Vec::new(),
                hint: if exists { "file" } else { "new file" },
                action: Activate::Open(resolved.clone()),
            });
        }

        // Directory + partial name to filter the listing.
        let (dir, partial) = if q.ends_with('/') {
            (resolved.clone(), String::new())
        } else {
            (resolved.parent().map(|p| p.to_path_buf()).unwrap_or(resolved.clone()),
             resolved.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
        };
        let prefix = dir_prefix(q);

        let mut dirs: Vec<String> = Vec::new();
        let mut filesv: Vec<String> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !partial.is_empty() && !name.to_lowercase().contains(&partial.to_lowercase()) {
                    continue;
                }
                match entry.file_type() {
                    Ok(t) if t.is_dir() => dirs.push(name),
                    Ok(t) if t.is_file() => filesv.push(name),
                    _ => {}
                }
                if dirs.len() + filesv.len() >= MAX_LISTING {
                    break;
                }
            }
        }
        dirs.sort();
        filesv.sort();
        for name in dirs {
            self.results.push(PickResult {
                display: format!("{prefix}{name}/"),
                positions: Vec::new(),
                hint: "dir",
                action: Activate::EnterDir(format!("{prefix}{name}/")),
            });
        }
        for name in filesv {
            let disp = format!("{prefix}{name}");
            self.results.push(PickResult {
                display: disp,
                positions: Vec::new(),
                hint: "",
                action: Activate::Open(dir.join(name)),
            });
        }
    }
}

// --- helpers ---------------------------------------------------------------

fn is_path_like(q: &str) -> bool {
    q.starts_with('/') || q.starts_with('~') || q.starts_with("./") || q.starts_with("../") || q.contains('/')
}

/// Resolve a typed path against `~` and the working directory.
fn resolve(q: &str, root: &Path) -> PathBuf {
    if let Some(rest) = q.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    if q == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    let p = PathBuf::from(q);
    if p.is_absolute() {
        p
    } else {
        root.join(p)
    }
}

/// The portion of `q` up to and including the last `/` (the typed directory).
fn dir_prefix(q: &str) -> String {
    match q.rfind('/') {
        Some(i) => q[..=i].to_string(),
        None => String::new(),
    }
}

/// A friendly display path: relative to `root` when possible, else `~`-relative.
fn nice(p: &Path, root: &Path) -> String {
    if let Ok(rel) = p.strip_prefix(root) {
        return rel.to_string_lossy().into_owned();
    }
    if let Some(home) = dirs::home_dir() {
        if let Ok(rel) = p.strip_prefix(&home) {
            return format!("~/{}", rel.to_string_lossy());
        }
    }
    p.to_string_lossy().into_owned()
}

fn scan(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if out.len() >= MAX_FILES {
            break;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') {
                continue;
            }
            let ftype = match entry.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            let path = entry.path();
            if ftype.is_dir() {
                if !SKIP_DIRS.contains(&name.as_ref()) {
                    stack.push(path);
                }
            } else if ftype.is_file() {
                if let Ok(rel) = path.strip_prefix(root) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
                if out.len() >= MAX_FILES {
                    break;
                }
            }
        }
    }
    out.sort();
    out
}

#[derive(Serialize, Deserialize, Default)]
struct RecentFile {
    files: Vec<String>,
}

fn load_recent(path: &Path) -> Vec<PathBuf> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| toml::from_str::<RecentFile>(&t).ok())
        .map(|r| r.files.into_iter().map(PathBuf::from).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_path_like_queries() {
        assert!(is_path_like("/etc/hosts"));
        assert!(is_path_like("~/notes"));
        assert!(is_path_like("./x"));
        assert!(is_path_like("src/main.rs"));
        assert!(!is_path_like("main"));
        assert!(!is_path_like("mainrs"));
    }

    #[test]
    fn dir_prefix_splits_on_last_slash() {
        assert_eq!(dir_prefix("src/ma"), "src/");
        assert_eq!(dir_prefix("/etc/ho"), "/etc/");
        assert_eq!(dir_prefix("nope"), "");
    }

    #[test]
    fn recent_preview_then_expand() {
        let mut p = FilePicker::default();
        p.recent = (0..12).map(|i| PathBuf::from(format!("/x/f{i}.txt"))).collect();
        p.root = PathBuf::from("/x");
        p.update(); // empty query -> default view

        assert_eq!(p.results.iter().filter(|r| r.hint == "recent").count(), 10);
        let exp = p.results.iter().position(|r| matches!(r.action, Activate::ExpandRecent));
        assert!(exp.is_some());

        p.selected = exp.unwrap();
        p.accept(); // expand
        assert_eq!(p.results.iter().filter(|r| r.hint == "recent").count(), 12);
        assert!(!p.results.iter().any(|r| matches!(r.action, Activate::ExpandRecent)));
    }

    #[test]
    fn search_offers_create_when_no_match() {
        let mut p = FilePicker::default();
        p.root = PathBuf::from("/proj");
        p.files = vec!["a.txt".into(), "b.txt".into()];
        p.query = "zzz".into();
        p.update();
        assert_eq!(p.results.len(), 1);
        assert_eq!(p.results[0].hint, "new file");
        match &p.results[0].action {
            Activate::Open(path) => assert_eq!(path, &PathBuf::from("/proj/zzz")),
            _ => panic!("expected Open"),
        }
    }
}
