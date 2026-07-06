//! The text buffer: a [`ropey::Rope`] plus the set of cursors editing it, an
//! undo history and file metadata. All editing operations are multi-cursor
//! aware and apply through a single `apply_edits` path so behaviour stays
//! consistent whether there is one caret or fifty.

use super::cursor::Cursor;
use super::tabstops::{self, TabStop, TabStops};
use super::undo::{History, Snapshot};
use crate::syntax::Language;
use anyhow::{Context, Result};
use ropey::Rope;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// A single text edit: replace the char range `start..end` with `text`.
struct Edit {
    start: usize,
    end: usize,
    text: String,
}

pub struct Buffer {
    pub rope: Rope,
    pub path: Option<PathBuf>,
    pub modified: bool,
    pub cursors: Vec<Cursor>,
    pub primary: usize,
    pub language: Language,
    /// Monotonic edit counter, used to detect when a recovery backup is stale.
    pub revision: u64,
    /// Revision last written to the recovery backup (not persisted).
    pub backup_rev: u64,
    /// Stable id used for this buffer's recovery backup filename.
    pub recovery_id: u64,
    history: History,
    disk_mtime: Option<SystemTime>,
    /// Uniform fallback width (synced from `Settings::tab_width`) used to resolve
    /// tab stops beyond any explicit ones declared in front matter.
    tab_width: usize,
    /// Resolved tab stops for this document, cached and refreshed on every edit
    /// (front matter is small and lives at the top, so this stays cheap).
    tabstops: TabStops,
}

impl Buffer {
    pub fn empty() -> Self {
        Buffer {
            rope: Rope::new(),
            path: None,
            modified: false,
            cursors: vec![Cursor::new(0)],
            primary: 0,
            language: Language::PlainText,
            revision: 0,
            backup_rev: 0,
            recovery_id: 0,
            history: History::new(),
            disk_mtime: None,
            tab_width: 4,
            tabstops: TabStops::uniform(4),
        }
    }

    pub fn from_file(path: &Path) -> Result<Self> {
        let rope = if path.exists() {
            let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
            Rope::from_reader(BufReader::new(file))
                .with_context(|| format!("reading {}", path.display()))?
        } else {
            Rope::new()
        };
        let mtime = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());
        let mut buf = Buffer {
            rope,
            language: Language::from_path(path),
            path: Some(path.to_path_buf()),
            modified: false,
            cursors: vec![Cursor::new(0)],
            primary: 0,
            revision: 0,
            backup_rev: 0,
            recovery_id: 0,
            history: History::new(),
            disk_mtime: mtime,
            tab_width: 4,
            tabstops: TabStops::uniform(4),
        };
        buf.refresh_tabstops();
        Ok(buf)
    }

    fn mark_modified(&mut self) {
        self.modified = true;
        self.revision = self.revision.wrapping_add(1);
        self.refresh_tabstops();
    }

    /// Largest slice of the document head that front matter could occupy. Tab
    /// stops are resolved from this bounded prefix so re-parsing per edit stays
    /// O(1) regardless of file size.
    const FRONTMATTER_SCAN: usize = 8192;

    /// Recompute the cached tab stops from the document's front matter, using
    /// the current `tab_width` as the uniform fallback.
    pub fn refresh_tabstops(&mut self) {
        let n = self.rope.len_chars().min(Self::FRONTMATTER_SCAN);
        let head = self.rope.slice(0..n).to_string();
        self.tabstops = tabstops::from_document(&head, self.tab_width.max(1));
    }

    /// Sync the uniform fallback width from settings, recomputing stops if it
    /// changed. Called when configuration loads or `tab_width` is edited.
    pub fn set_tab_width(&mut self, width: usize) {
        let width = width.max(1);
        if width != self.tab_width {
            self.tab_width = width;
            self.refresh_tabstops();
        }
    }

    pub fn tabstops(&self) -> &TabStops {
        &self.tabstops
    }

    #[cfg(test)]
    pub fn set_tabstops_for_test(&mut self, stops: TabStops) {
        self.tabstops = stops;
    }

    /// Rewrite the document's explicit tab stops in its YAML front matter as one
    /// undoable edit, preserving cursor positions (the edit is at the top of the
    /// document, so body cursors just shift). The cached [`TabStops`] refreshes
    /// via `mark_modified`.
    pub fn set_tab_stops(&mut self, stops: Vec<TabStop>) {
        let n = self.rope.len_chars().min(Self::FRONTMATTER_SCAN);
        let head = self.rope.slice(0..n).to_string();
        let (s, e, rep) = tabstops::splice_tabstops(&head, &stops);
        if s == e && rep.is_empty() {
            return; // nothing to do
        }
        self.record(false);
        if e > s {
            self.rope.remove(s..e);
        }
        if !rep.is_empty() {
            self.rope.insert(s, &rep);
        }
        let rep_len = rep.chars().count();
        let delta = rep_len as isize - (e - s) as isize;
        let shift = |p: usize| -> usize {
            if p <= s {
                p
            } else if p >= e {
                (p as isize + delta) as usize
            } else {
                s + rep_len // landed inside the rewritten region
            }
        };
        for c in &mut self.cursors {
            c.head = shift(c.head);
            c.anchor = shift(c.anchor);
            c.goal_col = None;
        }
        self.mark_modified();
        self.normalize();
    }

    /// Add a left tab stop at display column `col` (no-op if one already exists
    /// there). Returns whether anything changed.
    pub fn add_tab_stop(&mut self, col: usize) -> bool {
        let mut stops = self.tabstops.explicit().to_vec();
        if stops.iter().any(|s| s.col == col) {
            return false;
        }
        stops.push(TabStop::left(col));
        self.set_tab_stops(stops);
        true
    }

    /// Remove the explicit stop nearest `col`. Returns the removed column, if any.
    pub fn remove_tab_stop_near(&mut self, col: usize) -> Option<usize> {
        let stops = self.tabstops.explicit();
        let idx = (0..stops.len()).min_by_key(|&i| stops[i].col.abs_diff(col))?;
        let removed = stops[idx].col;
        let mut kept = stops.to_vec();
        kept.remove(idx);
        self.set_tab_stops(kept);
        Some(removed)
    }

    /// Remove every explicit tab stop. Returns whether any existed.
    pub fn clear_tab_stops(&mut self) -> bool {
        if self.tabstops.explicit().is_empty() {
            return false;
        }
        self.set_tab_stops(Vec::new());
        true
    }

    /// Characters of `line` as a `Vec`, excluding the trailing newline.
    fn line_chars(&self, line: usize) -> Vec<char> {
        let start = self.rope.line_to_char(line);
        let len = self.line_len_chars(line);
        self.rope.slice(start..start + len).chars().collect()
    }

    /// The display layout of `line`: per-char cell spans plus total width.
    /// This is the single source for on-screen geometry — rendering, cursor,
    /// mouse and soft wrap all agree because they all come here.
    ///
    /// Markdown callout lines lay out on the *visible-column grid*: the `> `
    /// marker and concealed markup (`**`, `[!type]`, `\`) occupy screen cells
    /// but don't count toward tab-stop columns, so the raw view gets exactly
    /// the tab widths its preview card shows.
    pub fn line_spans(&self, line: usize) -> (Vec<tabstops::CellSpan>, usize) {
        let chars = self.line_chars(line);
        if self.language == Language::Markdown {
            if let Some(cstart) = self.callout_content_start(line, &chars) {
                let content: String = chars[cstart.min(chars.len())..].iter().collect();
                let vis = crate::syntax::markdown::visible_mask(&content);
                return self.tabstops.spans_concealed(&chars, cstart, &vis);
            }
        }
        self.tabstops.spans(&chars)
    }

    /// Display column of a char offset within `line` (tab-aware).
    pub fn display_col(&self, line: usize, off: usize) -> usize {
        let (spans, total) = self.line_spans(line);
        if off >= spans.len() {
            total
        } else {
            spans[off].col
        }
    }

    /// Char offset within `line` nearest the given display column (tab-aware).
    /// A position inside a tab's whitespace snaps to the nearer edge.
    pub fn char_off_for_col(&self, line: usize, col: usize) -> usize {
        let (spans, _) = self.line_spans(line);
        for (i, s) in spans.iter().enumerate() {
            if col < s.col + s.width {
                return if col.saturating_sub(s.col) < s.width.div_ceil(2) { i } else { i + 1 };
            }
        }
        spans.len()
    }

    /// Total display width of `line` (tab-aware).
    #[allow(dead_code)] // used by the tab-stop ruler UI (later step)
    pub fn line_display_width(&self, line: usize) -> usize {
        self.line_spans(line).1
    }

    /// If `line` belongs to a callout block (a run of `>` lines whose first
    /// line is a `> [!type]` header), the char offset where its *preview
    /// content* starts: past `>` and one conventional space, and for the
    /// header line also past the `[!type]` marker. Mirrors the renderer's
    /// block detection so geometry and painting agree.
    fn callout_content_start(&self, line: usize, chars: &[char]) -> Option<usize> {
        /// Bound the upward scan so pathological quote runs stay cheap.
        const SCAN_UP: usize = 512;
        let quote_marker = |chars: &[char]| -> Option<usize> {
            let i = chars.iter().position(|c| !c.is_whitespace())?;
            (chars[i] == '>').then_some(i)
        };
        let marker = quote_marker(chars)?;
        let mut top = line;
        while top > 0 && line - top < SCAN_UP {
            let above = self.line_chars(top - 1);
            if quote_marker(&above).is_none() {
                break;
            }
            top -= 1;
        }
        // The block's first line must be a callout header: `> [!type…]`.
        let is_header_line = |chars: &[char]| -> Option<usize> {
            let m = quote_marker(chars)?;
            let mut i = m + 1;
            if chars.get(i) == Some(&' ') {
                i += 1;
            }
            if chars.get(i) != Some(&'[') || chars.get(i + 1) != Some(&'!') {
                return None;
            }
            let close = (i + 2..chars.len()).find(|&j| chars[j] == ']')?;
            (close > i + 2).then_some(close + 1)
        };
        if line == top {
            // Header: content starts after `[!type]` and one space.
            let mut i = is_header_line(chars)?;
            if chars.get(i) == Some(&' ') {
                i += 1;
            }
            Some(i)
        } else {
            is_header_line(&self.line_chars(top))?;
            // Body: content starts after `>` and one conventional space.
            let mut i = marker + 1;
            if chars.get(i) == Some(&' ') {
                i += 1;
            }
            Some(i)
        }
    }

    /// Replace the entire contents (used to restore recovered/unsaved content).
    /// Marks the buffer modified so it can be saved to its file or a new one.
    pub fn set_text(&mut self, text: &str) {
        self.rope = Rope::from_str(text);
        self.cursors = vec![Cursor::new(0)];
        self.primary = 0;
        self.history = History::new();
        self.mark_modified();
    }

    // --- queries -----------------------------------------------------------

    pub fn name(&self) -> String {
        match &self.path {
            Some(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.to_string_lossy().into_owned()),
            None => "[No Name]".to_string(),
        }
    }

    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    pub fn len_lines(&self) -> usize {
        self.rope.len_lines()
    }

    /// Length of `line` in chars, excluding the trailing line break.
    pub fn line_len_chars(&self, line: usize) -> usize {
        if line >= self.rope.len_lines() {
            return 0;
        }
        let slice = self.rope.line(line);
        let mut len = slice.len_chars();
        if len > 0 && slice.char(len - 1) == '\n' {
            len -= 1;
            if len > 0 && slice.char(len - 1) == '\r' {
                len -= 1;
            }
        }
        len
    }

    pub fn pos_to_line_col(&self, pos: usize) -> (usize, usize) {
        let pos = pos.min(self.rope.len_chars());
        let line = self.rope.char_to_line(pos);
        let col = pos - self.rope.line_to_char(line);
        (line, col)
    }

    /// Char position for a `(line, char-column)` pair. Note this is a *char*
    /// column, not a display column — use [`Buffer::char_off_for_col`] for
    /// tab-aware (display-column) mapping.
    #[allow(dead_code)]
    pub fn line_col_to_pos(&self, line: usize, col: usize) -> usize {
        let last = self.rope.len_lines().saturating_sub(1);
        let line = line.min(last);
        let start = self.rope.line_to_char(line);
        start + col.min(self.line_len_chars(line))
    }

    pub fn primary_cursor(&self) -> Cursor {
        self.cursors[self.primary.min(self.cursors.len() - 1)]
    }

    /// Text of the primary selection, if any.
    pub fn primary_selection_text(&self) -> Option<String> {
        let c = self.primary_cursor();
        if !c.has_selection() {
            return None;
        }
        let (s, e) = c.range();
        Some(self.rope.slice(s..e).to_string())
    }

    // --- bracket matching --------------------------------------------------

    /// If a bracket sits at `pos` (or just before it, so the cursor can be on
    /// either side), return `(bracket_pos, match_pos)` for the matching bracket.
    /// The search is bounded by `max_scan` chars to stay fast on huge files.
    pub fn matching_bracket(&self, pos: usize, max_scan: usize) -> Option<(usize, usize)> {
        let len = self.rope.len_chars();
        for &p in &[pos, pos.wrapping_sub(1)] {
            if p >= len {
                continue;
            }
            let ch = self.rope.char(p);
            if let Some(m) = self.find_bracket_match(p, ch, max_scan) {
                return Some((p, m));
            }
        }
        None
    }

    fn find_bracket_match(&self, p: usize, ch: char, max_scan: usize) -> Option<usize> {
        const OPEN: [char; 3] = ['(', '[', '{'];
        const CLOSE: [char; 3] = [')', ']', '}'];
        let len = self.rope.len_chars();
        if let Some(i) = OPEN.iter().position(|&o| o == ch) {
            let close = CLOSE[i];
            let mut depth = 1usize;
            let end = (p + 1 + max_scan).min(len);
            for q in (p + 1)..end {
                let c = self.rope.char(q);
                if c == ch {
                    depth += 1;
                } else if c == close {
                    depth -= 1;
                    if depth == 0 {
                        return Some(q);
                    }
                }
            }
            None
        } else if let Some(i) = CLOSE.iter().position(|&c| c == ch) {
            let open = OPEN[i];
            let mut depth = 1usize;
            let start = p.saturating_sub(max_scan);
            for q in (start..p).rev() {
                let c = self.rope.char(q);
                if c == ch {
                    depth += 1;
                } else if c == open {
                    depth -= 1;
                    if depth == 0 {
                        return Some(q);
                    }
                }
            }
            None
        } else {
            None
        }
    }

    // --- undo --------------------------------------------------------------

    fn snapshot(&self) -> Snapshot {
        Snapshot { rope: self.rope.clone(), cursors: self.cursors.clone() }
    }

    fn record(&mut self, coalesce: bool) {
        let snap = self.snapshot();
        self.history.record(snap, coalesce);
    }

    pub fn undo(&mut self) -> bool {
        let cur = self.snapshot();
        if let Some(prev) = self.history.undo(cur) {
            self.rope = prev.rope;
            self.cursors = prev.cursors;
            self.primary = self.primary.min(self.cursors.len() - 1);
            self.mark_modified();
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        let cur = self.snapshot();
        if let Some(next) = self.history.redo(cur) {
            self.rope = next.rope;
            self.cursors = next.cursors;
            self.primary = self.primary.min(self.cursors.len() - 1);
            self.mark_modified();
            true
        } else {
            false
        }
    }

    // --- core mutation -----------------------------------------------------

    /// Apply one edit per cursor. `edits[i]` corresponds to `cursors[i]`.
    /// Edits must be non-overlapping (callers normalize cursors first).
    fn apply_edits(&mut self, edits: Vec<Edit>) {
        let mut order: Vec<usize> = (0..edits.len()).collect();
        order.sort_by_key(|&i| edits[i].start);

        let mut delta: isize = 0;
        let mut new_heads = vec![0usize; edits.len()];
        for &i in &order {
            let e = &edits[i];
            let s = (e.start as isize + delta) as usize;
            let en = (e.end as isize + delta) as usize;
            if en > s {
                self.rope.remove(s..en);
            }
            if !e.text.is_empty() {
                self.rope.insert(s, &e.text);
            }
            let inserted = e.text.chars().count();
            new_heads[i] = s + inserted;
            delta += inserted as isize - (en - s) as isize;
        }
        for (i, c) in self.cursors.iter_mut().enumerate() {
            c.head = new_heads[i];
            c.anchor = new_heads[i];
            c.goal_col = None;
        }
        self.mark_modified();
        self.normalize();
    }

    /// One edit per cursor: delete its selection (if any) then insert `text`.
    fn insert_at_cursors(&mut self, text: &str, coalesce: bool) {
        self.record(coalesce);
        let edits = self
            .cursors
            .iter()
            .map(|c| {
                let (s, e) = c.range();
                Edit { start: s, end: e, text: text.to_string() }
            })
            .collect();
        self.apply_edits(edits);
    }

    pub fn insert_char(&mut self, ch: char) {
        let coalesce = !ch.is_whitespace();
        self.insert_at_cursors(&ch.to_string(), coalesce);
    }

    #[allow(dead_code)]
    pub fn insert_str(&mut self, s: &str) {
        self.insert_at_cursors(s, false);
    }

    pub fn insert_newline(&mut self, auto_indent: bool) {
        self.record(false);
        let edits = self
            .cursors
            .iter()
            .map(|c| {
                let (s, e) = c.range();
                let mut text = String::from("\n");
                if auto_indent {
                    let (line, _) = self.pos_to_line_col(s);
                    text.push_str(&self.leading_whitespace(line));
                }
                Edit { start: s, end: e, text }
            })
            .collect();
        self.apply_edits(edits);
    }

    fn leading_whitespace(&self, line: usize) -> String {
        let mut out = String::new();
        let start = self.rope.line_to_char(line);
        let len = self.line_len_chars(line);
        for i in 0..len {
            let ch = self.rope.char(start + i);
            if ch == ' ' || ch == '\t' {
                out.push(ch);
            } else {
                break;
            }
        }
        out
    }

    pub fn backspace(&mut self) {
        // If any cursor has a selection, treat backspace as delete-selection.
        let any_sel = self.cursors.iter().any(|c| c.has_selection());
        self.record(false);
        let edits = self
            .cursors
            .iter()
            .map(|c| {
                if c.has_selection() {
                    let (s, e) = c.range();
                    Edit { start: s, end: e, text: String::new() }
                } else if !any_sel && c.head > 0 {
                    Edit { start: c.head - 1, end: c.head, text: String::new() }
                } else {
                    Edit { start: c.head, end: c.head, text: String::new() }
                }
            })
            .collect();
        self.apply_edits(edits);
    }

    pub fn delete(&mut self) {
        let len = self.rope.len_chars();
        let any_sel = self.cursors.iter().any(|c| c.has_selection());
        self.record(false);
        let edits = self
            .cursors
            .iter()
            .map(|c| {
                if c.has_selection() {
                    let (s, e) = c.range();
                    Edit { start: s, end: e, text: String::new() }
                } else if !any_sel && c.head < len {
                    Edit { start: c.head, end: c.head + 1, text: String::new() }
                } else {
                    Edit { start: c.head, end: c.head, text: String::new() }
                }
            })
            .collect();
        self.apply_edits(edits);
    }

    /// Insert a tab at every cursor. With `insert_spaces` it expands to the
    /// number of spaces needed to reach the next tab stop *for that cursor's
    /// column*; otherwise it stores a literal `\t`, which reflows if the stops
    /// change. Documents that declare explicit stops always store `\t` — spaces
    /// would burn the layout in and (in previews that conceal markup) be
    /// computed against the wrong column grid. The `_tab_width` argument is
    /// kept for call-site compatibility — the uniform fallback now lives on the
    /// buffer's resolved [`TabStops`].
    pub fn insert_tab(&mut self, insert_spaces: bool, _tab_width: usize) {
        let insert_spaces = insert_spaces && self.tabstops.explicit().is_empty();
        self.record(false);
        let edits = self
            .cursors
            .iter()
            .map(|c| {
                let (s, e) = c.range();
                let text = if insert_spaces {
                    let (line, off) = self.pos_to_line_col(s);
                    let col = self.display_col(line, off);
                    " ".repeat(self.tabstops.tab_width_at(col))
                } else {
                    "\t".to_string()
                };
                Edit { start: s, end: e, text }
            })
            .collect();
        self.apply_edits(edits);
    }

    // --- movement ----------------------------------------------------------

    fn set_head(c: &mut Cursor, pos: usize, extend: bool) {
        c.head = pos;
        if !extend {
            c.anchor = pos;
        }
        c.goal_col = None;
    }

    pub fn move_left(&mut self, extend: bool) {
        self.history.break_coalescing();
        for c in &mut self.cursors {
            if !extend && c.has_selection() {
                let (s, _) = c.range();
                Self::set_head(c, s, false);
            } else {
                Self::set_head(c, c.head.saturating_sub(1), extend);
            }
        }
        self.normalize();
    }

    pub fn move_right(&mut self, extend: bool) {
        self.history.break_coalescing();
        let len = self.rope.len_chars();
        for c in &mut self.cursors {
            if !extend && c.has_selection() {
                let (_, e) = c.range();
                Self::set_head(c, e, false);
            } else {
                Self::set_head(c, (c.head + 1).min(len), extend);
            }
        }
        self.normalize();
    }

    pub fn move_vertical(&mut self, delta: isize, extend: bool) {
        self.history.break_coalescing();
        let last_line = self.rope.len_lines().saturating_sub(1);
        let cursors = std::mem::take(&mut self.cursors);
        let mut moved = Vec::with_capacity(cursors.len());
        for mut c in cursors {
            let (line, off) = self.pos_to_line_col(c.head);
            // Goal column is a display column so tabs preserve visual alignment.
            let goal = c.goal_col.unwrap_or_else(|| self.display_col(line, off));
            let target = (line as isize + delta).clamp(0, last_line as isize) as usize;
            let new_off = self.char_off_for_col(target, goal);
            let pos = self.rope.line_to_char(target) + new_off;
            c.head = pos;
            if !extend {
                c.anchor = pos;
            }
            c.goal_col = Some(goal);
            moved.push(c);
        }
        self.cursors = moved;
        self.normalize();
    }

    pub fn move_word_left(&mut self, extend: bool) {
        self.history.break_coalescing();
        let cursors = std::mem::take(&mut self.cursors);
        let mut out = Vec::with_capacity(cursors.len());
        for mut c in cursors {
            let pos = self.word_boundary_left(c.head);
            Self::set_head(&mut c, pos, extend);
            out.push(c);
        }
        self.cursors = out;
        self.normalize();
    }

    pub fn move_word_right(&mut self, extend: bool) {
        self.history.break_coalescing();
        let cursors = std::mem::take(&mut self.cursors);
        let mut out = Vec::with_capacity(cursors.len());
        for mut c in cursors {
            let pos = self.word_boundary_right(c.head);
            Self::set_head(&mut c, pos, extend);
            out.push(c);
        }
        self.cursors = out;
        self.normalize();
    }

    fn is_word(ch: char) -> bool {
        ch.is_alphanumeric() || ch == '_'
    }

    fn word_boundary_left(&self, mut pos: usize) -> usize {
        while pos > 0 && self.rope.char(pos - 1).is_whitespace() {
            pos -= 1;
        }
        while pos > 0 && Self::is_word(self.rope.char(pos - 1)) {
            pos -= 1;
        }
        pos
    }

    fn word_boundary_right(&self, mut pos: usize) -> usize {
        let len = self.rope.len_chars();
        while pos < len && self.rope.char(pos).is_whitespace() {
            pos += 1;
        }
        while pos < len && Self::is_word(self.rope.char(pos)) {
            pos += 1;
        }
        pos
    }

    pub fn move_line_start(&mut self, extend: bool) {
        self.history.break_coalescing();
        let cursors = std::mem::take(&mut self.cursors);
        let mut out = Vec::with_capacity(cursors.len());
        for mut c in cursors {
            let (line, _) = self.pos_to_line_col(c.head);
            let start = self.rope.line_to_char(line);
            // Smart home: first non-whitespace, then column 0.
            let indent = self.leading_whitespace(line).chars().count();
            let target = if c.head > start + indent || c.head == start {
                start + indent
            } else {
                start
            };
            Self::set_head(&mut c, target, extend);
            out.push(c);
        }
        self.cursors = out;
        self.normalize();
    }

    pub fn move_line_end(&mut self, extend: bool) {
        self.history.break_coalescing();
        let cursors = std::mem::take(&mut self.cursors);
        let mut out = Vec::with_capacity(cursors.len());
        for mut c in cursors {
            let (line, _) = self.pos_to_line_col(c.head);
            let pos = self.rope.line_to_char(line) + self.line_len_chars(line);
            Self::set_head(&mut c, pos, extend);
            out.push(c);
        }
        self.cursors = out;
        self.normalize();
    }

    pub fn move_buffer_start(&mut self, extend: bool) {
        self.history.break_coalescing();
        self.cursors.truncate(1);
        self.primary = 0;
        Self::set_head(&mut self.cursors[0], 0, extend);
    }

    pub fn move_buffer_end(&mut self, extend: bool) {
        self.history.break_coalescing();
        self.cursors.truncate(1);
        self.primary = 0;
        let len = self.rope.len_chars();
        Self::set_head(&mut self.cursors[0], len, extend);
    }

    // --- selection ---------------------------------------------------------

    pub fn select_all(&mut self) {
        self.cursors.truncate(1);
        self.primary = 0;
        self.cursors[0].anchor = 0;
        self.cursors[0].head = self.rope.len_chars();
        self.cursors[0].goal_col = None;
    }

    pub fn select_line(&mut self) {
        let cursors = std::mem::take(&mut self.cursors);
        let mut out = Vec::with_capacity(cursors.len());
        for mut c in cursors {
            let (line, _) = self.pos_to_line_col(c.head);
            let start = self.rope.line_to_char(line);
            // Select the line content only — exclude the trailing newline.
            let end = start + self.line_len_chars(line);
            c.anchor = start;
            c.head = end;
            c.goal_col = None;
            out.push(c);
        }
        self.cursors = out;
        self.normalize();
    }

    pub fn collapse_selections(&mut self) {
        for c in &mut self.cursors {
            c.collapse();
        }
        self.normalize();
    }

    // --- multi-cursor ------------------------------------------------------

    pub fn add_cursor_vertical(&mut self, delta: isize) {
        let last_line = self.rope.len_lines().saturating_sub(1);
        let base = self.cursors[self.primary.min(self.cursors.len() - 1)];
        let (line, col) = self.pos_to_line_col(base.head);
        let target = line as isize + delta;
        if target < 0 || target > last_line as isize {
            return;
        }
        let target = target as usize;
        let new_col = col.min(self.line_len_chars(target));
        let pos = self.rope.line_to_char(target) + new_col;
        self.cursors.push(Cursor::new(pos));
        self.primary = self.cursors.len() - 1;
        self.normalize();
    }

    /// Sublime-style "add next occurrence": the first press (no selection) just
    /// selects the word under the cursor; each subsequent press adds a cursor
    /// selecting the next occurrence of that text.
    pub fn add_cursor_next_match(&mut self, case_sensitive: bool) {
        let needle = match self.primary_selection_text() {
            Some(s) if !s.is_empty() => s,
            _ => {
                // First press: select the word under the cursor and stop.
                self.select_word_under_primary();
                return;
            }
        };
        if needle.is_empty() {
            return;
        }
        let text = self.rope.to_string();
        let matches = crate::search::find::find_all(&text, &needle, case_sensitive);
        if matches.is_empty() {
            return;
        }
        let max_head = self.cursors.iter().map(|c| c.range().1).max().unwrap_or(0);
        let existing: Vec<(usize, usize)> = self.cursors.iter().map(|c| c.range()).collect();
        // First match starting at/after the current furthest cursor, wrapping.
        let next = matches
            .iter()
            .find(|(s, _)| *s >= max_head && !existing.contains(&(*s, *s + needle.chars().count())))
            .or_else(|| matches.iter().find(|m| !existing.contains(m)))
            .copied();
        if let Some((s, e)) = next {
            let mut c = Cursor::new(e);
            c.anchor = s;
            self.cursors.push(c);
            self.primary = self.cursors.len() - 1;
            self.normalize();
        }
    }

    pub fn select_all_matches(&mut self, case_sensitive: bool) {
        let needle = match self.primary_selection_text() {
            Some(s) if !s.is_empty() => s,
            _ => {
                if self.select_word_under_primary() {
                    self.primary_selection_text().unwrap_or_default()
                } else {
                    return;
                }
            }
        };
        if needle.is_empty() {
            return;
        }
        let text = self.rope.to_string();
        let matches = crate::search::find::find_all(&text, &needle, case_sensitive);
        if matches.is_empty() {
            return;
        }
        self.cursors = matches
            .into_iter()
            .map(|(s, e)| {
                let mut c = Cursor::new(e);
                c.anchor = s;
                c
            })
            .collect();
        self.primary = self.cursors.len() - 1;
        self.normalize();
    }

    fn select_word_under_primary(&mut self) -> bool {
        let idx = self.primary.min(self.cursors.len() - 1);
        let pos = self.cursors[idx].head;
        let len = self.rope.len_chars();
        let mut start = pos;
        let mut end = pos;
        while start > 0 && Self::is_word(self.rope.char(start - 1)) {
            start -= 1;
        }
        while end < len && Self::is_word(self.rope.char(end)) {
            end += 1;
        }
        if start == end {
            return false;
        }
        self.cursors[idx].anchor = start;
        self.cursors[idx].head = end;
        true
    }

    pub fn clear_extra_cursors(&mut self) {
        let keep = self.cursors[self.primary.min(self.cursors.len() - 1)];
        self.cursors = vec![keep];
        self.primary = 0;
    }

    pub fn set_single_cursor(&mut self, pos: usize, extend: bool) {
        if extend {
            let mut c = self.cursors[self.primary.min(self.cursors.len() - 1)];
            c.head = pos;
            c.goal_col = None;
            self.cursors = vec![c];
        } else {
            self.cursors = vec![Cursor::new(pos)];
        }
        self.primary = 0;
        self.history.break_coalescing();
    }

    /// Replace all cursors with a single selection from `anchor` to `head`.
    pub fn set_selection(&mut self, anchor: usize, head: usize) {
        let n = self.rope.len_chars();
        let mut c = Cursor::new(head.min(n));
        c.anchor = anchor.min(n);
        self.cursors = vec![c];
        self.primary = 0;
        self.history.break_coalescing();
    }

    pub fn add_cursor_at(&mut self, pos: usize) {
        self.cursors.push(Cursor::new(pos));
        self.primary = self.cursors.len() - 1;
        self.normalize();
    }

    /// Replace the cursor set wholesale, then normalize (clamp + dedup). Used by
    /// the app for soft-wrap-aware vertical movement, which needs viewport width.
    pub fn replace_cursors(&mut self, cursors: Vec<Cursor>) {
        if cursors.is_empty() {
            return;
        }
        self.cursors = cursors;
        if self.primary >= self.cursors.len() {
            self.primary = self.cursors.len() - 1;
        }
        self.normalize();
    }

    /// Break undo coalescing (e.g. after a cursor move).
    pub fn break_coalescing(&mut self) {
        self.history.break_coalescing();
    }

    // --- markdown helpers --------------------------------------------------

    pub fn toggle_wrap(&mut self, marker: &str) {
        let idx = self.primary.min(self.cursors.len() - 1);
        let c = self.cursors[idx];
        let mlen = marker.chars().count();
        if c.has_selection() {
            let (s, e) = c.range();
            let len = self.rope.len_chars();
            let sel = self.rope.slice(s..e).to_string();
            let sel_chars = sel.chars().count();

            // Markers sitting immediately outside the selection (the state right
            // after a previous wrap, where only the inner text is selected).
            let outside = s >= mlen
                && e + mlen <= len
                && self.rope.slice(s - mlen..s).to_string() == marker
                && self.rope.slice(e..e + mlen).to_string() == marker;
            // Markers included within the selection itself.
            let inside =
                sel_chars >= 2 * mlen && sel.starts_with(marker) && sel.ends_with(marker);

            self.record(false);
            if outside {
                // Remove the surrounding markers (after first, to keep indices).
                self.rope.remove(e..e + mlen);
                self.rope.remove(s - mlen..s);
                let mut nc = Cursor::new(e - mlen);
                nc.anchor = s - mlen;
                self.cursors = vec![nc];
            } else if inside {
                let inner: String = sel.chars().skip(mlen).take(sel_chars - 2 * mlen).collect();
                self.rope.remove(s..e);
                self.rope.insert(s, &inner);
                let mut nc = Cursor::new(s + inner.chars().count());
                nc.anchor = s;
                self.cursors = vec![nc];
            } else {
                let wrapped = format!("{marker}{sel}{marker}");
                self.rope.remove(s..e);
                self.rope.insert(s, &wrapped);
                // Keep the original text (without markers) selected.
                let mut nc = Cursor::new(s + mlen + sel_chars);
                nc.anchor = s + mlen;
                self.cursors = vec![nc];
            }
            self.primary = 0;
            self.mark_modified();
        } else {
            // Insert empty markers and place the cursor between them.
            let pos = c.head;
            self.record(false);
            let pair = format!("{marker}{marker}");
            self.rope.insert(pos, &pair);
            self.cursors = vec![Cursor::new(pos + mlen)];
            self.primary = 0;
            self.mark_modified();
        }
    }

    pub fn toggle_line_comment(&mut self) {
        let prefix = match self.language.line_comment() {
            Some(p) => p,
            None => return,
        };
        // Collect the set of lines touched by any cursor.
        let mut lines: Vec<usize> = Vec::new();
        for c in &self.cursors {
            let (s, e) = c.range();
            let (ls, _) = self.pos_to_line_col(s);
            let (le, _) = self.pos_to_line_col(e);
            for l in ls..=le {
                if !lines.contains(&l) {
                    lines.push(l);
                }
            }
        }
        lines.sort_unstable();
        // Comment unless every non-empty line is already commented (then uncomment).
        let all_commented = lines.iter().all(|&l| {
            let ws = self.leading_whitespace(l).chars().count();
            let start = self.rope.line_to_char(l) + ws;
            let llen = self.line_len_chars(l);
            llen == ws // empty line counts as commented
                || self
                    .rope
                    .slice(start..self.rope.line_to_char(l) + llen)
                    .to_string()
                    .starts_with(prefix)
        });
        self.record(false);
        // Apply from last line to first so earlier indices stay valid.
        for &l in lines.iter().rev() {
            let ws = self.leading_whitespace(l).chars().count();
            let llen = self.line_len_chars(l);
            if llen == ws {
                continue; // skip blank lines
            }
            let insert_at = self.rope.line_to_char(l) + ws;
            if all_commented {
                let with_prefix = format!("{prefix} ");
                let line_text = self
                    .rope
                    .slice(insert_at..self.rope.line_to_char(l) + llen)
                    .to_string();
                if line_text.starts_with(&with_prefix) {
                    self.rope.remove(insert_at..insert_at + with_prefix.chars().count());
                } else if line_text.starts_with(prefix) {
                    self.rope.remove(insert_at..insert_at + prefix.chars().count());
                }
            } else {
                self.rope.insert(insert_at, &format!("{prefix} "));
            }
        }
        self.mark_modified();
        self.clamp_cursors();
    }

    // --- normalization -----------------------------------------------------

    fn clamp_cursors(&mut self) {
        let len = self.rope.len_chars();
        for c in &mut self.cursors {
            c.head = c.head.min(len);
            c.anchor = c.anchor.min(len);
        }
    }

    /// Clamp, then drop duplicate cursors (same head and anchor), preserving
    /// order and tracking the primary index.
    fn normalize(&mut self) {
        self.clamp_cursors();
        let primary_val = self.cursors[self.primary.min(self.cursors.len() - 1)];
        let mut out: Vec<Cursor> = Vec::with_capacity(self.cursors.len());
        for c in &self.cursors {
            if !out.iter().any(|o| o.head == c.head && o.anchor == c.anchor) {
                out.push(*c);
            }
        }
        self.primary = out
            .iter()
            .position(|c| c.head == primary_val.head && c.anchor == primary_val.anchor)
            .unwrap_or(out.len() - 1);
        self.cursors = out;
    }

    // --- file I/O ----------------------------------------------------------

    pub fn save(&mut self) -> Result<()> {
        let path = self
            .path
            .clone()
            .ok_or_else(|| anyhow::anyhow!("no file name; use :save_as <path>"))?;
        self.save_to(&path)
    }

    pub fn save_to(&mut self, path: &Path) -> Result<()> {
        let file = File::create(path).with_context(|| format!("creating {}", path.display()))?;
        self.rope
            .write_to(BufWriter::new(file))
            .with_context(|| format!("writing {}", path.display()))?;
        self.path = Some(path.to_path_buf());
        self.language = Language::from_path(path);
        self.modified = false;
        self.disk_mtime = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());
        Ok(())
    }

    /// Replace each of `ranges` with `to`, as one undoable step. Ranges must be
    /// non-overlapping. Used by search-and-replace.
    pub fn replace_ranges(&mut self, ranges: &[(usize, usize)], to: &str) {
        if ranges.is_empty() {
            return;
        }
        self.record(false);
        let mut sorted = ranges.to_vec();
        sorted.sort_by_key(|r| r.0);
        for (s, e) in sorted.iter().rev() {
            self.rope.remove(*s..*e);
            self.rope.insert(*s, to);
        }
        let pos = sorted[0].0 + to.chars().count();
        self.cursors = vec![Cursor::new(pos.min(self.rope.len_chars()))];
        self.primary = 0;
        self.mark_modified();
    }

    /// Apply several `(start, end, text)` replacements as one undoable step.
    /// Edits must be non-overlapping; applied right-to-left so earlier offsets
    /// stay valid. Used to splice dynamic-document output regions.
    pub fn splice_segments(&mut self, edits: &[(usize, usize, String)]) {
        if edits.is_empty() {
            return;
        }
        self.record(false);
        let mut sorted = edits.to_vec();
        sorted.sort_by_key(|e| e.0);
        for (s, e, text) in sorted.iter().rev() {
            self.rope.remove(*s..*e);
            self.rope.insert(*s, text);
        }
        let first = &sorted[0];
        let pos = first.0 + first.2.chars().count();
        self.cursors = vec![Cursor::new(pos.min(self.rope.len_chars()))];
        self.primary = 0;
        self.mark_modified();
    }

    pub fn trim_trailing_whitespace(&mut self) {
        let mut changed = false;
        for line in (0..self.rope.len_lines()).rev() {
            let start = self.rope.line_to_char(line);
            let len = self.line_len_chars(line);
            let mut trimmed = len;
            while trimmed > 0 {
                let ch = self.rope.char(start + trimmed - 1);
                if ch == ' ' || ch == '\t' {
                    trimmed -= 1;
                } else {
                    break;
                }
            }
            if trimmed < len {
                self.rope.remove(start + trimmed..start + len);
                changed = true;
            }
        }
        if changed {
            self.clamp_cursors();
            self.mark_modified();
        }
    }

    /// Detect whether the file changed on disk since we last loaded/saved it.
    pub fn disk_changed(&self) -> bool {
        if let (Some(path), Some(known)) = (&self.path, self.disk_mtime) {
            if let Ok(meta) = std::fs::metadata(path) {
                if let Ok(modified) = meta.modified() {
                    return modified != known;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::cursor::Cursor;

    fn buf(text: &str) -> Buffer {
        let mut b = Buffer::empty();
        b.rope = Rope::from_str(text);
        b
    }

    #[test]
    fn insert_char_single_cursor() {
        let mut b = buf("ac");
        b.cursors = vec![Cursor::new(1)];
        b.insert_char('b');
        assert_eq!(b.rope.to_string(), "abc");
        assert_eq!(b.cursors[0].head, 2);
        assert!(b.modified);
    }

    #[test]
    fn multi_cursor_insert_keeps_indices_valid() {
        // Insert 'X' at the start of every line simultaneously.
        let mut b = buf("aa\nbb\ncc");
        b.cursors = vec![Cursor::new(0), Cursor::new(3), Cursor::new(6)];
        b.insert_char('X');
        assert_eq!(b.rope.to_string(), "Xaa\nXbb\nXcc");
        assert_eq!(b.cursors.len(), 3);
        assert_eq!(b.cursors[0].head, 1);
        assert_eq!(b.cursors[2].head, 9);
    }

    #[test]
    fn backspace_at_multiple_cursors() {
        let mut b = buf("a1\nb2\nc3");
        // Cursors after each digit.
        b.cursors = vec![Cursor::new(2), Cursor::new(5), Cursor::new(8)];
        b.backspace();
        assert_eq!(b.rope.to_string(), "a\nb\nc");
    }

    #[test]
    fn undo_redo_roundtrip() {
        let mut b = buf("");
        b.cursors = vec![Cursor::new(0)];
        b.insert_char('h');
        b.insert_char('i');
        assert_eq!(b.rope.to_string(), "hi");
        assert!(b.undo());
        assert_eq!(b.rope.to_string(), "");
        assert!(b.redo());
        assert_eq!(b.rope.to_string(), "hi");
    }

    #[test]
    fn vertical_movement_keeps_goal_column() {
        let mut b = buf("hello\nx\nworld");
        b.cursors = vec![Cursor::new(4)]; // col 4 on line 0
        b.move_vertical(1, false); // line 1 only has 1 char -> col clamps to 1
        let (l, c) = b.pos_to_line_col(b.cursors[0].head);
        assert_eq!((l, c), (1, 1));
        b.move_vertical(1, false); // line 2 "world" -> goal col 4 restored
        let (l, c) = b.pos_to_line_col(b.cursors[0].head);
        assert_eq!((l, c), (2, 4));
    }

    #[test]
    fn add_tab_stop_writes_frontmatter_and_keeps_cursor() {
        let mut b = buf("body\n");
        b.cursors = vec![Cursor::new(2)]; // between 'o' and 'd'
        assert!(b.add_tab_stop(16));
        assert!(b.rope.to_string().starts_with("---\ntabstops: [16]\n---\n\n"));
        assert_eq!(b.tabstops().explicit().iter().map(|s| s.col).collect::<Vec<_>>(), vec![16]);
        // The cursor still sits between 'o' and 'd' in the body.
        let head = b.cursors[0].head;
        assert_eq!(b.rope.slice(head - 2..head).to_string(), "bo");
        // Idempotent: adding the same column again does nothing.
        assert!(!b.add_tab_stop(16));
    }

    #[test]
    fn callout_raw_tabs_match_preview_grid() {
        // stops [3]; a callout with markup before one tab and none before the other.
        let mut b = buf("---\ntabstops: [3]\n---\n> [!note] T\n> **II**\tTy\n> \tsom\n");
        b.language = Language::Markdown;
        b.refresh_tabstops();
        // `> **II**\tTy`: tab (index 8) sits at *visible* col 2 ("II") -> stop 3
        // -> width 1, exactly like the preview card.
        let (spans, _) = b.line_spans(4);
        assert_eq!(spans[8].width, 1);
        // `> \tsom`: tab (index 2) at visible col 0 -> stop 3 -> width 3.
        let (spans, _) = b.line_spans(5);
        assert_eq!(spans[2].width, 3);
        // Concealed markup still occupies one screen cell each (raw shows it).
        let (spans, _) = b.line_spans(4);
        assert_eq!(spans[2].col, 2); // first '*' right after "> "
        assert_eq!(spans[8].col, 8); // tab's screen cell after "> **II**"
    }

    #[test]
    fn plain_quote_keeps_absolute_grid() {
        // A blockquote without a callout header is not previewed, so it keeps
        // the plain absolute-column layout.
        let mut b = buf("> quote\there");
        b.language = Language::Markdown;
        b.refresh_tabstops();
        let (spans, _) = b.line_spans(0);
        // tab at raw col 7 -> uniform stop 8 -> width 1.
        assert_eq!(spans[7].width, 1);
    }

    #[test]
    fn explicit_stops_force_real_tabs_even_in_spaces_mode() {
        let mut b = buf("---\ntabstops: [3]\n---\nx");
        b.refresh_tabstops();
        b.cursors = vec![Cursor::new(b.rope.len_chars())];
        b.insert_tab(true, 4); // insert_spaces on, but the doc declares stops
        assert!(b.rope.to_string().ends_with("x\t"));
        // Without explicit stops, spaces mode still applies.
        let mut b = buf("x");
        b.cursors = vec![Cursor::new(1)];
        b.insert_tab(true, 4);
        assert_eq!(b.rope.to_string(), "x   "); // to the next uniform stop (4)
    }

    #[test]
    fn remove_and_clear_tab_stops() {
        let mut b = buf("---\ntabstops: [8, 24, 40]\n---\nbody\n");
        b.refresh_tabstops();
        assert_eq!(b.remove_tab_stop_near(26), Some(24)); // nearest to 26
        assert_eq!(b.tabstops().explicit().iter().map(|s| s.col).collect::<Vec<_>>(), vec![8, 40]);
        assert!(b.clear_tab_stops());
        assert!(b.tabstops().explicit().is_empty());
        // Front matter is preserved (just the entry removed) and body intact.
        assert!(b.rope.to_string().contains("body"));
    }

    #[test]
    fn bracket_matching_pairs_and_nesting() {
        // a ( b c [ d ] e ) f  -> indices 0..10
        let b = buf("a(bc[d]e)f");
        assert_eq!(b.matching_bracket(1, 1000), Some((1, 8))); // on '('
        assert_eq!(b.matching_bracket(8, 1000), Some((8, 1))); // on ')'
        assert_eq!(b.matching_bracket(9, 1000), Some((8, 1))); // just after ')'
        assert_eq!(b.matching_bracket(4, 1000), Some((4, 6))); // inner '['
        assert_eq!(b.matching_bracket(0, 1000), None); // not a bracket
        // Unbalanced: no match.
        let u = buf("(()");
        assert_eq!(u.matching_bracket(0, 1000), None);
    }

    #[test]
    fn select_line_excludes_newline() {
        let mut b = buf("aa\nbb\ncc");
        b.cursors = vec![Cursor::new(4)]; // on line "bb"
        b.select_line();
        assert_eq!(b.cursors.len(), 1);
        assert_eq!(b.cursors[0].range(), (3, 5)); // "bb", not the trailing \n
    }

    #[test]
    fn add_cursor_next_match_selects_word_then_occurrences() {
        let mut b = buf("foo bar foo baz foo");
        b.cursors = vec![Cursor::new(1)]; // inside the first "foo"
        // First press: just select the word under the cursor.
        b.add_cursor_next_match(false);
        assert_eq!(b.cursors.len(), 1);
        assert_eq!(b.cursors[0].range(), (0, 3));
        // Each subsequent press adds the next occurrence.
        b.add_cursor_next_match(false);
        assert_eq!(b.cursors.len(), 2);
        b.add_cursor_next_match(false);
        assert_eq!(b.cursors.len(), 3);
        assert!(b.cursors.iter().all(|c| c.range().1 - c.range().0 == 3));
    }

    #[test]
    fn select_all_matches_creates_cursor_per_match() {
        let mut b = buf("foo foo foo");
        b.cursors = vec![Cursor::new(0)];
        b.select_all_matches(false);
        assert_eq!(b.cursors.len(), 3);
        assert!(b.cursors.iter().all(|c| c.has_selection()));
    }

    #[test]
    fn toggle_bold_wraps_and_unwraps() {
        let mut b = buf("word");
        let mut c = Cursor::new(4);
        c.anchor = 0;
        b.cursors = vec![c];
        b.toggle_wrap("**");
        assert_eq!(b.rope.to_string(), "**word**");
        b.toggle_wrap("**");
        assert_eq!(b.rope.to_string(), "word");
    }

    #[test]
    fn toggle_line_comment_rust() {
        let mut b = buf("let x = 1;");
        b.language = Language::Rust;
        b.cursors = vec![Cursor::new(0)];
        b.toggle_line_comment();
        assert_eq!(b.rope.to_string(), "// let x = 1;");
        b.toggle_line_comment();
        assert_eq!(b.rope.to_string(), "let x = 1;");
    }

    #[test]
    fn trim_trailing_whitespace_works() {
        let mut b = buf("a   \nb\t\nc");
        b.cursors = vec![Cursor::new(0)];
        b.trim_trailing_whitespace();
        assert_eq!(b.rope.to_string(), "a\nb\nc");
    }
}
