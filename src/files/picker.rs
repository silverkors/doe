//! Fuzzy file picker. Scans the working directory (skipping VCS/build/hidden
//! dirs) into a flat list of project-relative paths, then fuzzy-filters them
//! with the same matcher the command palette uses. Opening a result switches to
//! that file.

use crate::commands::palette::fuzzy;
use std::path::{Path, PathBuf};

/// Cap the scan so opening the picker stays fast in large trees.
const MAX_FILES: usize = 20_000;
const SKIP_DIRS: &[&str] = &["target", "node_modules", "dist", "build", "__pycache__", "vendor"];

/// A ranked result: index into `files` plus matched char positions.
pub struct Match {
    pub idx: usize,
    pub positions: Vec<usize>,
}

#[derive(Default)]
pub struct FilePicker {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    pub results: Vec<Match>,
    files: Vec<String>,
    root: PathBuf,
}

impl FilePicker {
    pub fn new() -> Self {
        FilePicker::default()
    }

    /// Open the picker rooted at `root`, scanning its files.
    pub fn open(&mut self, root: PathBuf) {
        self.files = scan(&root);
        self.root = root;
        self.query.clear();
        self.selected = 0;
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

    /// Display path for a result row.
    pub fn path_str(&self, m: &Match) -> &str {
        &self.files[m.idx]
    }

    /// Absolute path of the current selection.
    pub fn selected_path(&self) -> Option<PathBuf> {
        let m = self.results.get(self.selected)?;
        Some(self.root.join(&self.files[m.idx]))
    }

    /// Recompute the ranked results for the current query.
    pub fn update(&mut self) {
        if self.query.is_empty() {
            self.results = (0..self.files.len()).map(|idx| Match { idx, positions: Vec::new() }).collect();
        } else {
            let mut scored: Vec<(i32, usize, Vec<usize>)> = Vec::new();
            for (i, f) in self.files.iter().enumerate() {
                if let Some((score, positions)) = fuzzy(&self.query, f) {
                    scored.push((score, i, positions));
                }
            }
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            self.results = scored.into_iter().map(|(_, idx, positions)| Match { idx, positions }).collect();
        }
        if self.selected >= self.results.len() {
            self.selected = self.results.len().saturating_sub(1);
        }
    }
}

/// Recursively collect project-relative file paths under `root`.
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
                continue; // hidden files/dirs (.git, .config, …)
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
