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

    /// Tab: descend into the selected directory if one is highlighted; otherwise
    /// (path mode) complete the path, or (search mode) advance the selection.
    pub fn tab(&mut self) {
        if self.enter_selected_dir() {
            return;
        }
        self.complete();
    }

    fn enter_selected_dir(&mut self) -> bool {
        if let Some(r) = self.results.get(self.selected) {
            if let Activate::EnterDir(d) = &r.action {
                self.query = d.clone();
                self.selected = 0;
                self.update();
                return true;
            }
        }
        false
    }

    /// Left arrow: go back out of a directory. From the default view it starts
    /// browsing the working directory; while browsing it drops a half-typed name
    /// first, then climbs to the parent directory.
    pub fn go_up(&mut self) {
        let q = self.query.clone();
        if !q.is_empty() && !is_path_like(&q) {
            return; // Left has no meaning during a plain search.
        }
        if q.is_empty() {
            self.query = "./".to_string();
            self.selected = 0;
            self.update();
            return;
        }
        if !q.ends_with('/') {
            let dp = dir_prefix(&q);
            if q.len() > dp.len() {
                // Drop the partially-typed name, staying in the same directory.
                self.query = if dp.is_empty() { "./".to_string() } else { dp };
                self.selected = 0;
                self.update();
                return;
            }
        }
        let base = if q.ends_with('/') { q.clone() } else { dir_prefix(&q) };
        self.query = parent_dir_str(&base);
        self.selected = 0;
        self.update();
    }

    /// Tab completion. In path mode, complete the typed path toward the matching
    /// directory entries: a unique match completes fully (directories gain a
    /// trailing `/` so you descend straight in), multiple matches complete to
    /// their longest common prefix. Outside path mode, Tab just advances the
    /// selection.
    pub fn complete(&mut self) {
        if !is_path_like(&self.query) {
            self.move_selection(1);
            return;
        }
        let q = self.query.clone();
        let resolved = resolve(&q, &self.root);
        let (dir, partial) = if q.ends_with('/') {
            (resolved.clone(), String::new())
        } else {
            (
                resolved.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| resolved.clone()),
                resolved.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            )
        };
        let prefix = dir_prefix(&q);
        let pl = partial.to_lowercase();

        let mut names: Vec<(String, bool)> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                // Skip hidden entries unless the user explicitly typed a dot.
                if name.starts_with('.') && !pl.starts_with('.') {
                    continue;
                }
                if !name.to_lowercase().starts_with(&pl) {
                    continue;
                }
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                names.push((name, is_dir));
            }
        }
        if names.is_empty() {
            return;
        }
        names.sort();
        if names.len() == 1 {
            let (name, is_dir) = &names[0];
            self.query = format!("{prefix}{name}{}", if *is_dir { "/" } else { "" });
        } else {
            let lcp = longest_common_prefix(names.iter().map(|(n, _)| n.as_str()));
            if lcp.chars().count() > partial.chars().count() {
                self.query = format!("{prefix}{lcp}");
            }
        }
        self.selected = 0;
        self.update();
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

/// Longest common (char-wise) prefix of a set of strings.
fn longest_common_prefix<'a>(mut iter: impl Iterator<Item = &'a str>) -> String {
    let mut prefix: String = match iter.next() {
        Some(s) => s.to_string(),
        None => return String::new(),
    };
    for s in iter {
        let common: String = prefix.chars().zip(s.chars()).take_while(|(a, b)| a == b).map(|(a, _)| a).collect();
        prefix = common;
        if prefix.is_empty() {
            break;
        }
    }
    prefix
}

/// The portion of `q` up to and including the last `/` (the typed directory).
fn dir_prefix(q: &str) -> String {
    match q.rfind('/') {
        Some(i) => q[..=i].to_string(),
        None => String::new(),
    }
}

/// The parent directory of a directory string (which ends in `/`), preserving
/// the relative/absolute style the user typed.
fn parent_dir_str(base: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/".to_string(); // already at the filesystem root
    }
    match trimmed.rfind('/') {
        Some(i) => trimmed[..=i].to_string(),
        None => match trimmed {
            "." => "../".to_string(),
            ".." => "../../".to_string(),
            _ => "./".to_string(), // a single relative component → working dir
        },
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
    fn go_up_navigates_out_of_directories() {
        let mut p = FilePicker { root: PathBuf::from("/proj"), ..Default::default() };
        p.query = "src/foo".into();
        p.go_up();
        assert_eq!(p.query, "src/"); // drop the half-typed name first
        p.go_up();
        assert_eq!(p.query, "./"); // src/ -> working dir
        p.go_up();
        assert_eq!(p.query, "../"); // ./ -> parent
        p.query = "/Users/david/".into();
        p.go_up();
        assert_eq!(p.query, "/Users/");
        p.query = String::new();
        p.go_up();
        assert_eq!(p.query, "./"); // from default view, start browsing cwd
    }

    #[test]
    fn go_up_ignored_during_plain_search() {
        let mut p = FilePicker { root: PathBuf::from("/proj"), ..Default::default() };
        p.query = "main".into();
        p.go_up();
        assert_eq!(p.query, "main"); // unchanged: not a path
    }

    #[test]
    fn tab_completes_unique_directory() {
        let tmp = std::env::temp_dir().join("doe_complete_unique");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("uniquedir")).unwrap();
        std::fs::write(tmp.join("afile.txt"), "x").unwrap();

        let mut p = FilePicker { root: tmp.clone(), ..Default::default() };
        p.query = format!("{}/uniq", tmp.display());
        p.complete();
        assert_eq!(p.query, format!("{}/uniquedir/", tmp.display()));

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn tab_completes_to_common_prefix() {
        let tmp = std::env::temp_dir().join("doe_complete_lcp");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("common_a")).unwrap();
        std::fs::create_dir_all(tmp.join("common_b")).unwrap();

        let mut p = FilePicker { root: tmp.clone(), ..Default::default() };
        p.query = format!("{}/comm", tmp.display());
        p.complete();
        assert_eq!(p.query, format!("{}/common_", tmp.display()));

        std::fs::remove_dir_all(&tmp).ok();
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
