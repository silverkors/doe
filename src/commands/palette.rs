//! The command palette — a Spotlight-style action launcher. Opening it (Ctrl+P)
//! shows a fuzzy-searchable list of every editor action. When the query is
//! empty it surfaces the actions you use most (usage counts are persisted
//! across sessions), so the common things you do are already at the top.
//!
//! Context-aware *guessing* (ranking by what you're currently doing) is a
//! planned follow-up; the ranking function is the single place that would grow.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One launchable action. `command` is the registry command name that gets
/// executed; `title`/`hint` are for display.
pub struct PaletteAction {
    pub command: &'static str,
    pub title: &'static str,
    pub hint: &'static str,
}

/// The catalog of palette actions, ordered by a sensible default priority so a
/// brand-new install already suggests common actions before any usage data
/// exists.
pub fn catalog() -> &'static [PaletteAction] {
    &[
        PaletteAction { command: "save", title: "Save File", hint: "Ctrl+S" },
        PaletteAction { command: "find", title: "Find…", hint: "Ctrl+F" },
        PaletteAction { command: "find_next", title: "Find Next", hint: "F3" },
        PaletteAction { command: "find_prev", title: "Find Previous", hint: "Shift+F3" },
        PaletteAction { command: "replace_all", title: "Replace All…", hint: "Ctrl+H" },
        PaletteAction { command: "add_cursor_next_match", title: "Select Word / Add Next Occurrence", hint: "Ctrl+D" },
        PaletteAction { command: "select_all_matches", title: "Select All Occurrences", hint: "Alt+F3" },
        PaletteAction { command: "add_cursor_above", title: "Add Cursor: Line Above", hint: "Alt+Up" },
        PaletteAction { command: "add_cursor_below", title: "Add Cursor: Line Below", hint: "Alt+Down" },
        PaletteAction { command: "clear_extra_cursors", title: "Clear Extra Cursors", hint: "Esc" },
        PaletteAction { command: "undo", title: "Undo", hint: "Ctrl+Z" },
        PaletteAction { command: "redo", title: "Redo", hint: "Ctrl+Y" },
        PaletteAction { command: "toggle_comment", title: "Toggle Line Comment", hint: "Ctrl+/" },
        PaletteAction { command: "toggle_soft_wrap", title: "Toggle Soft Wrap", hint: "Alt+Z" },
        PaletteAction { command: "settings", title: "Settings…", hint: "Ctrl+," },
        PaletteAction { command: "callout_settings", title: "Callout Styles…", hint: "" },
        PaletteAction { command: "import_callouts", title: "Import Callouts from Obsidian…", hint: "" },
        PaletteAction { command: "toggle_bold", title: "Markdown: Bold", hint: "Ctrl+B" },
        PaletteAction { command: "toggle_italic", title: "Markdown: Italic", hint: "Ctrl+I" },
        PaletteAction { command: "select_all", title: "Select All", hint: "Ctrl+A" },
        PaletteAction { command: "select_line", title: "Select Line", hint: "Ctrl+L" },
        PaletteAction { command: "open", title: "Open File…", hint: "Ctrl+O" },
        PaletteAction { command: "save_as", title: "Save As…", hint: "" },
        PaletteAction { command: "next_buffer", title: "Next Buffer", hint: "Ctrl+PageDown" },
        PaletteAction { command: "prev_buffer", title: "Previous Buffer", hint: "Ctrl+PageUp" },
        PaletteAction { command: "close_buffer", title: "Close Buffer", hint: "Ctrl+W" },
        PaletteAction { command: "move_buffer_start", title: "Go to Start of File", hint: "Ctrl+Home" },
        PaletteAction { command: "move_buffer_end", title: "Go to End of File", hint: "Ctrl+End" },
        PaletteAction { command: "save_quit", title: "Save and Quit", hint: "" },
        PaletteAction { command: "quit", title: "Quit", hint: "Ctrl+Q" },
        PaletteAction { command: "force_quit", title: "Discard Changes and Quit", hint: "" },
    ]
}

/// True if `name` is a palette action (so we only record usage for meaningful
/// actions, not navigation/typing).
pub fn is_action(name: &str) -> bool {
    catalog().iter().any(|a| a.command == name)
}

/// A ranked search result: an index into [`catalog`] plus the matched character
/// positions in the title (for highlighting).
pub struct Result {
    pub idx: usize,
    pub positions: Vec<usize>,
}

/// A snapshot of the editing context used to nudge palette ranking so the
/// actions most relevant to what you're doing surface higher (especially with
/// an empty query). Purely additive: fuzzy match quality always dominates.
#[derive(Debug, Clone, Copy, Default)]
pub struct PaletteContext {
    pub markdown: bool,
    /// The language has a line-comment (i.e. it's a programming language).
    pub code: bool,
    pub has_selection: bool,
    pub multiple_cursors: bool,
    pub modified: bool,
}

/// Context relevance bonus for an action. Higher = more relevant right now.
fn context_boost(command: &str, ctx: &PaletteContext) -> i32 {
    match command {
        "toggle_bold" | "toggle_italic" if ctx.markdown => 30,
        "callout_settings" | "import_callouts" if ctx.markdown => 20,
        "toggle_comment" if ctx.code => 30,
        "save" | "save_as" if ctx.modified => 25,
        "clear_extra_cursors" if ctx.multiple_cursors => 30,
        "add_cursor_next_match" | "select_all_matches" if ctx.has_selection => 15,
        _ => 0,
    }
}

pub struct Palette {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    pub results: Vec<Result>,
    context: PaletteContext,
    usage: HashMap<String, u32>,
    usage_path: PathBuf,
    dirty: bool,
}

impl Palette {
    pub fn new(config_dir: &Path) -> Self {
        let usage_path = config_dir.join("usage.toml");
        let usage = load_usage(&usage_path);
        let mut p = Palette {
            open: false,
            query: String::new(),
            selected: 0,
            results: Vec::new(),
            context: PaletteContext::default(),
            usage,
            usage_path,
            dirty: false,
        };
        p.update();
        p
    }

    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
        self.update();
    }

    /// Refresh the editing-context snapshot used for ranking. Call before
    /// `open`/`update` when the buffer state may have changed.
    pub fn set_context(&mut self, ctx: PaletteContext) {
        self.context = ctx;
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.results.is_empty() {
            return;
        }
        let n = self.results.len() as isize;
        self.selected = (((self.selected as isize + delta) % n + n) % n) as usize;
    }

    pub fn selected_command(&self) -> Option<&'static str> {
        let r = self.results.get(self.selected)?;
        Some(catalog()[r.idx].command)
    }

    /// Record one use of an action and remember to persist it.
    pub fn record_use(&mut self, command: &str) {
        if !is_action(command) {
            return;
        }
        *self.usage.entry(command.to_string()).or_insert(0) += 1;
        self.dirty = true;
    }

    /// Recompute the ranked result list for the current query.
    pub fn update(&mut self) {
        let cat = catalog();
        let mut scored: Vec<(i32, i32, u32, usize, Vec<usize>)> = Vec::new();
        for (idx, action) in cat.iter().enumerate() {
            let usage = self.usage.get(action.command).copied().unwrap_or(0);
            let boost = context_boost(action.command, &self.context);
            if let Some((score, positions)) = fuzzy(&self.query, action.title) {
                scored.push((score, boost, usage, idx, positions));
            }
        }
        // Sort by match score, then context relevance, then usage, then catalog
        // order. With an empty query all scores tie at 0, so context + usage
        // decide which actions surface first.
        scored.sort_by(|a, b| {
            b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(b.2.cmp(&a.2)).then(a.3.cmp(&b.3))
        });
        self.results = scored.into_iter().map(|(_, _, _, idx, positions)| Result { idx, positions }).collect();
        if self.selected >= self.results.len() {
            self.selected = self.results.len().saturating_sub(1);
        }
    }

    /// Persist usage counts if they changed (best effort).
    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }
        if let Ok(text) = toml::to_string(&self.usage) {
            let _ = std::fs::write(&self.usage_path, text);
        }
        self.dirty = false;
    }
}

fn load_usage(path: &Path) -> HashMap<String, u32> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or_default()
}

/// Fuzzy subsequence match with scoring. Returns `(score, matched_positions)`
/// or `None` if `query` is not a subsequence of `text`. Higher score = better.
/// An empty query matches everything with a neutral score.
pub fn fuzzy(query: &str, text: &str) -> Option<(i32, Vec<usize>)> {
    let q: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).map(|c| c.to_ascii_lowercase()).collect();
    if q.is_empty() {
        return Some((0, Vec::new()));
    }
    let t: Vec<char> = text.chars().collect();
    let mut qi = 0;
    let mut positions = Vec::with_capacity(q.len());
    let mut score = 0i32;
    let mut prev: Option<usize> = None;
    for (i, &ch) in t.iter().enumerate() {
        if qi >= q.len() {
            break;
        }
        if ch.to_ascii_lowercase() == q[qi] {
            score += 10;
            if let Some(p) = prev {
                if p + 1 == i {
                    score += 15; // consecutive match
                }
            }
            if i == 0 || !t[i - 1].is_alphanumeric() {
                score += 20; // start of a word
            }
            positions.push(i);
            prev = Some(i);
            qi += 1;
        }
    }
    if qi == q.len() {
        // Prefer shorter titles and earlier first matches.
        score -= t.len() as i32;
        score -= positions.first().copied().unwrap_or(0) as i32;
        Some((score, positions))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_subsequence_and_scoring() {
        // "sf" matches "Save File" at the two word starts and ranks it above a
        // mid-word match.
        let save = fuzzy("sf", "Save File");
        assert!(save.is_some());
        let other = fuzzy("sf", "Select All Matches"); // no 'f'
        assert!(other.is_none());
    }

    #[test]
    fn empty_query_matches_all() {
        assert!(fuzzy("", "anything").is_some());
    }

    #[test]
    fn word_start_beats_midword() {
        let a = fuzzy("rb", "Replace Both").unwrap().0; // R..B at word starts
        let b = fuzzy("rb", "Carbon").unwrap().0; // mid-word
        assert!(a > b);
    }

    #[test]
    fn catalog_commands_are_known() {
        // Every palette action must be a real, parseable command.
        for a in catalog() {
            assert!(super::super::registry::parse(a.command).is_some(), "{}", a.command);
        }
    }

    fn rank_of(p: &Palette, cmd: &str) -> Option<usize> {
        p.results.iter().position(|r| catalog()[r.idx].command == cmd)
    }

    #[test]
    fn context_boosts_relevant_actions() {
        let mut p = Palette::new(&std::env::temp_dir());
        // Empty query in a markdown buffer: Bold should outrank an unrelated
        // action like Quit purely from context.
        p.set_context(PaletteContext { markdown: true, ..Default::default() });
        p.update();
        assert!(rank_of(&p, "toggle_bold") < rank_of(&p, "quit"));

        // In a code buffer, Toggle Comment is the boosted one instead.
        p.set_context(PaletteContext { code: true, ..Default::default() });
        p.update();
        assert!(rank_of(&p, "toggle_comment") < rank_of(&p, "quit"));
    }

    #[test]
    fn fuzzy_match_dominates_context() {
        let mut p = Palette::new(&std::env::temp_dir());
        p.set_context(PaletteContext { markdown: true, ..Default::default() });
        p.query = "quit".to_string();
        p.update();
        // A specific query still finds its target regardless of context boosts.
        assert!(rank_of(&p, "quit").is_some());
        assert!(rank_of(&p, "toggle_bold").is_none());
    }
}
