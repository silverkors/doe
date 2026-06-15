//! The application: editor state plus the central command-execution loop. Key
//! events and mouse events are translated into [`Command`]s (via the keymap and
//! command registry) and everything flows through [`App::execute`] — the single
//! command layer that keybindings, the command line, mouse and plugins share.

use crate::commands::palette::Palette;
use crate::commands::{registry, Command, BINDING_CONTEXT};
use crate::config::Config;
use crate::editor::Buffer;
use crate::files;
use crate::files::picker::FilePicker;
use crate::files::recovery::{Recovery, SessEntry, Session};
use crate::input::keymap;
use crate::input::mouse::gutter_width;
use crate::plugins::{Event, PluginRegistry};
use crate::search::{find, SearchState};
use crate::syntax::Language;
use crate::ui::commandline::{CommandLine, PromptKind};
use crate::ui::settings::{self, SettingsPanel};
use crate::ui::wrap;
use crossterm::event::{KeyEvent, MouseButton, MouseEvent, MouseEventKind, KeyModifiers};
use std::path::{Path, PathBuf};

pub struct App {
    pub config: Config,
    pub buffers: Vec<Buffer>,
    pub active: usize,
    pub top_line: usize,
    pub left_col: usize,
    /// First visible visual sub-row of `top_line` (soft-wrap scroll anchor).
    pub top_subrow: usize,
    pub command: CommandLine,
    pub palette: Palette,
    pub file_picker: FilePicker,
    pub settings_panel: SettingsPanel,
    pub search: SearchState,
    pub status_message: String,
    pub plugins: PluginRegistry,
    pub width: u16,
    pub height: u16,
    pub should_quit: bool,
    /// Whether the first visible line sits inside a fenced code block. Computed
    /// from the lines above the viewport so highlighting stays correct when the
    /// opening fence has scrolled off the top.
    pub top_in_code_block: bool,
    recovery: Recovery,
    next_recovery_id: u64,
    disk_warned: bool,
}

/// Buffers larger than this are not backed up (autosaving a huge file on every
/// edit would stall); their on-disk content is the recovery point.
const MAX_BACKUP_CHARS: usize = 5_000_000;

impl App {
    pub fn new(config: Config, files: Vec<PathBuf>) -> Self {
        let recovery = Recovery::new(&config.config_dir);
        let session = recovery.read_session();
        let mut next_recovery_id = 1u64;
        let mut active = 0usize;
        let mut buffers: Vec<Buffer> = Vec::new();
        let mut recovered = false;

        // Canonical paths opened explicitly via args (so we don't duplicate them
        // when also restoring the session).
        let mut arg_paths: Vec<PathBuf> = Vec::new();
        for f in &files {
            match Buffer::from_file(f) {
                Ok(mut b) => {
                    b.recovery_id = next_recovery_id;
                    next_recovery_id += 1;
                    if let Some(content) = backup_for_path(&recovery, &session, f) {
                        b.set_text(&content);
                        recovered = true;
                    }
                    arg_paths.push(f.canonicalize().unwrap_or_else(|_| f.clone()));
                    buffers.push(b);
                }
                Err(e) => eprintln!("doe: {e:#}"),
            }
        }

        // Restore the previous session. With no args we resume it fully; with
        // args we still bring back any buffers that have *unsaved* changes so
        // quitting without saving never loses work, but skip already-open files.
        if let Some(sess) = &session {
            for e in &sess.buffers {
                let entry_canon = e
                    .path
                    .as_ref()
                    .map(|p| Path::new(p).canonicalize().unwrap_or_else(|_| PathBuf::from(p)));
                if let Some(c) = &entry_canon {
                    if arg_paths.contains(c) {
                        continue; // already opened via args
                    }
                }
                if !files.is_empty() && !e.has_backup {
                    continue; // with args, only rescue unsaved buffers
                }
                let mut b = match &e.path {
                    Some(p) => Buffer::from_file(Path::new(p)).unwrap_or_else(|_| Buffer::empty()),
                    None => Buffer::empty(),
                };
                b.recovery_id = e.id;
                next_recovery_id = next_recovery_id.max(e.id + 1);
                if e.has_backup {
                    if let Some(content) = recovery.read_backup(e.id) {
                        b.set_text(&content);
                        b.backup_rev = b.revision; // already mirrored on disk
                        recovered = true;
                    }
                }
                buffers.push(b);
            }
            if files.is_empty() {
                active = sess.active.min(buffers.len().saturating_sub(1));
            }
        }

        if buffers.is_empty() {
            let mut b = Buffer::empty();
            b.recovery_id = next_recovery_id;
            next_recovery_id += 1;
            buffers.push(b);
        }

        let palette = Palette::new(&config.config_dir);
        let file_picker = FilePicker::new(&config.config_dir);
        let mut app = App {
            config,
            buffers,
            active,
            top_line: 0,
            left_col: 0,
            top_subrow: 0,
            command: CommandLine::default(),
            palette,
            file_picker,
            settings_panel: SettingsPanel::default(),
            search: SearchState::default(),
            status_message: String::new(),
            plugins: PluginRegistry::with_builtins(),
            width: 80,
            height: 24,
            should_quit: false,
            top_in_code_block: false,
            recovery,
            next_recovery_id,
            disk_warned: false,
        };
        let opened: Vec<PathBuf> = app.buffers.iter().filter_map(|b| b.path.clone()).collect();
        for p in &opened {
            app.plugins.dispatch(&Event::OpenFile(p.clone()));
        }
        for p in opened {
            app.file_picker.record_open(&p);
        }
        if recovered {
            app.set_status("restored unsaved changes");
        }
        app
    }

    fn fresh_recovery_id(&mut self) -> u64 {
        let id = self.next_recovery_id;
        self.next_recovery_id += 1;
        id
    }

    /// Mirror modified buffers into the recovery store (invisible autosave).
    pub fn autosave(&mut self) {
        self.recovery.ensure_dir();
        let active = self.active;
        let mut entries: Vec<SessEntry> = Vec::new();
        for buf in &mut self.buffers {
            // Skip blank, never-saved scratch buffers.
            if buf.path.is_none() && buf.len_chars() == 0 {
                continue;
            }
            let mut has_backup = false;
            if buf.modified && buf.len_chars() <= MAX_BACKUP_CHARS {
                if buf.revision != buf.backup_rev
                    && self.recovery.write_backup(buf.recovery_id, &buf.rope).is_ok()
                {
                    buf.backup_rev = buf.revision;
                }
                has_backup = true;
            } else if !buf.modified {
                self.recovery.remove_backup(buf.recovery_id);
            }
            entries.push(SessEntry {
                id: buf.recovery_id,
                path: buf.path.as_ref().map(|p| p.to_string_lossy().into_owned()),
                has_backup,
            });
        }
        self.recovery.write_session(&Session { active, buffers: entries });
    }

    pub fn active_buffer(&self) -> &Buffer {
        &self.buffers[self.active]
    }

    pub fn active_buffer_mut(&mut self) -> &mut Buffer {
        &mut self.buffers[self.active]
    }

    pub fn resize(&mut self, w: u16, h: u16) {
        self.width = w;
        self.height = h;
        self.ensure_cursor_visible();
    }

    /// Recompute whether the top visible line is inside a fenced code block, by
    /// counting fence toggles in the lines above the viewport. Only Markdown
    /// buffers need this; for everything else the state is trivially `false`,
    /// which also keeps it O(1) on large code/text files. Called once per frame.
    pub fn recompute_fence_state(&mut self) {
        let buf = self.active_buffer();
        if buf.language != Language::Markdown {
            self.top_in_code_block = false;
            return;
        }
        let rope = &buf.rope;
        let limit = self.top_line.min(rope.len_lines());
        let mut in_block = false;
        for i in 0..limit {
            if line_is_fence(rope.line(i)) {
                in_block = !in_block;
            }
        }
        self.top_in_code_block = in_block;
    }

    /// Detect external modification of the active file (called between input
    /// events). Warns once until the next save/open/switch.
    pub fn check_external_changes(&mut self) {
        if !self.disk_warned && self.active_buffer().disk_changed() {
            self.disk_warned = true;
            self.set_status("file changed on disk — :e <path> to reload, Ctrl+S to overwrite");
        }
    }

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = msg.into();
    }

    // --- geometry helpers --------------------------------------------------

    fn gutter(&self) -> u16 {
        gutter_width(self.active_buffer().len_lines(), self.config.settings.line_numbers)
    }

    fn text_rows(&self) -> usize {
        // One reserved row at the bottom: the combined status/command line.
        self.height.saturating_sub(1) as usize
    }

    fn text_cols(&self) -> usize {
        self.width.saturating_sub(self.gutter()) as usize
    }

    fn soft_wrap(&self) -> bool {
        self.config.settings.soft_wrap
    }

    fn ensure_cursor_visible(&mut self) {
        if self.soft_wrap() {
            self.ensure_visible_wrapped();
        } else {
            self.ensure_visible_plain();
        }
    }

    fn ensure_visible_plain(&mut self) {
        let rows = self.text_rows().max(1);
        let cols = self.text_cols().max(1);
        let (line, col) = {
            let b = self.active_buffer();
            b.pos_to_line_col(b.primary_cursor().head)
        };
        if line < self.top_line {
            self.top_line = line;
        } else if line >= self.top_line + rows {
            self.top_line = line + 1 - rows;
        }
        if col < self.left_col {
            self.left_col = col;
        } else if col >= self.left_col + cols {
            self.left_col = col + 1 - cols;
        }
    }

    fn ensure_visible_wrapped(&mut self) {
        self.left_col = 0;
        let width = self.text_cols().max(1);
        let rows = self.text_rows().max(1);
        let buf = self.active_buffer();
        let head = buf.primary_cursor().head;
        let (cl, cs, _) = wrap::vpos_of(buf, head, width);
        let top = (self.top_line, self.top_subrow);
        if (cl, cs) < top {
            self.top_line = cl;
            self.top_subrow = cs;
            return;
        }
        // Walk back from the cursor by (rows-1) visual rows: the topmost anchor
        // that still keeps the cursor on screen. If it's below the current top,
        // the cursor was past the bottom, so scroll down to it.
        let (mut bl, mut bs) = (cl, cs);
        for _ in 0..rows.saturating_sub(1) {
            match wrap::prev_visual(buf, bl, bs, width) {
                Some((l, s)) => {
                    bl = l;
                    bs = s;
                }
                None => break,
            }
        }
        if (bl, bs) > top {
            self.top_line = bl;
            self.top_subrow = bs;
        }
    }

    // --- vertical movement (soft-wrap aware) -------------------------------

    fn move_up(&mut self, extend: bool) {
        if self.soft_wrap() {
            self.move_visual(-1, extend);
        } else {
            self.active_buffer_mut().move_vertical(-1, extend);
        }
    }

    fn move_down(&mut self, extend: bool) {
        if self.soft_wrap() {
            self.move_visual(1, extend);
        } else {
            self.active_buffer_mut().move_vertical(1, extend);
        }
    }

    fn page(&mut self, dir: isize) {
        let rows = self.text_rows().max(1) as isize;
        if self.soft_wrap() {
            self.move_visual(dir * rows, false);
        } else {
            self.active_buffer_mut().move_vertical(dir * rows, false);
        }
    }

    /// Move every cursor by `delta` visual rows, preserving the goal column.
    fn move_visual(&mut self, delta: isize, extend: bool) {
        let width = self.text_cols().max(1);
        let buf = &mut self.buffers[self.active];
        buf.break_coalescing();
        let cursors = std::mem::take(&mut buf.cursors);
        let mut out = Vec::with_capacity(cursors.len());
        for mut c in cursors {
            let (line, sub, col) = wrap::vpos_of(buf, c.head, width);
            let goal = c.goal_col.unwrap_or(col);
            let (mut tl, mut ts) = (line, sub);
            for _ in 0..delta.unsigned_abs() {
                let next = if delta < 0 {
                    wrap::prev_visual(buf, tl, ts, width)
                } else {
                    wrap::next_visual(buf, tl, ts, width)
                };
                match next {
                    Some((l, s)) => {
                        tl = l;
                        ts = s;
                    }
                    None => break,
                }
            }
            let pos = wrap::pos_at(buf, tl, ts, goal, width);
            c.head = pos;
            if !extend {
                c.anchor = pos;
            }
            c.goal_col = Some(goal);
            out.push(c);
        }
        buf.replace_cursors(out);
    }

    // --- input -------------------------------------------------------------

    pub fn handle_key(&mut self, ev: KeyEvent) {
        if self.settings_panel.open {
            self.handle_settings_key(ev);
            return;
        }
        if self.palette.open {
            self.handle_palette_key(ev);
            return;
        }
        if self.file_picker.open {
            self.handle_file_picker_key(ev);
            return;
        }
        if self.command.active {
            self.handle_command_key(ev);
            return;
        }

        self.status_message.clear();

        let chord = keymap::chord_string(&ev);
        let name = chord
            .as_deref()
            .and_then(|c| self.config.keybindings.get(BINDING_CONTEXT, c))
            .map(|s| s.to_string());

        match name {
            Some(name) => self.run_named(&name),
            None => {
                // DOE is modeless: any unbound printable char inserts.
                if let Some(ch) = keymap::printable_char(&ev) {
                    self.execute(Command::InsertChar(ch));
                }
            }
        }
    }

    /// Run a command by name (from a keybinding or the palette): record usage,
    /// resolve via the registry/plugin aliases, and execute.
    fn run_named(&mut self, name: &str) {
        self.palette.record_use(name);
        if let Some(cmd) = self.resolve_command_name(name) {
            self.execute(cmd);
        }
    }

    fn resolve_command_name(&self, name: &str) -> Option<Command> {
        if let Some(cmd) = registry::parse(name) {
            return Some(cmd);
        }
        // Try plugin aliases.
        let resolved = self.plugins.resolve_alias(name)?;
        registry::parse(resolved)
    }

    /// Key handling while the command palette is open.
    fn handle_palette_key(&mut self, ev: KeyEvent) {
        use crossterm::event::KeyCode::*;
        match ev.code {
            Esc => self.palette.close(),
            Enter => {
                if let Some(name) = self.palette.selected_command() {
                    let name = name.to_string();
                    self.palette.close();
                    self.status_message.clear();
                    self.run_named(&name);
                } else {
                    self.palette.close();
                }
            }
            Up | BackTab => self.palette.move_selection(-1),
            Down | Tab => self.palette.move_selection(1),
            Backspace => {
                self.palette.query.pop();
                self.palette.update();
            }
            Char(c) if !ev.modifiers.contains(KeyModifiers::CONTROL) => {
                self.palette.query.push(c);
                self.palette.update();
            }
            _ => {}
        }
    }

    /// Key handling while the fuzzy file picker is open.
    fn handle_file_picker_key(&mut self, ev: KeyEvent) {
        use crossterm::event::KeyCode::*;
        match ev.code {
            Esc => self.file_picker.close(),
            Enter => match self.file_picker.accept() {
                crate::files::picker::Accept::Open(p) => {
                    self.file_picker.close();
                    self.status_message.clear();
                    self.do_open(p);
                }
                crate::files::picker::Accept::Stay => {}
            },
            Up | BackTab => self.file_picker.move_selection(-1),
            Down => self.file_picker.move_selection(1),
            Tab => self.file_picker.tab(),
            Left => self.file_picker.go_up(),
            Backspace => {
                self.file_picker.query.pop();
                self.file_picker.update();
            }
            Char(c) if !ev.modifiers.contains(KeyModifiers::CONTROL) => {
                self.file_picker.query.push(c);
                self.file_picker.update();
            }
            _ => {}
        }
    }

    pub fn selected_setting(&self) -> usize {
        self.settings_panel.selected
    }

    /// Key handling while the settings panel is open.
    fn handle_settings_key(&mut self, ev: KeyEvent) {
        use crossterm::event::KeyCode::*;
        match ev.code {
            Esc | Enter => {
                self.settings_panel.close();
                self.config.save();
            }
            Up | BackTab => self.settings_panel.move_selection(-1),
            Down | Tab => self.settings_panel.move_selection(1),
            Left => self.adjust_selected_setting(-1),
            Right | Char(' ') => self.adjust_selected_setting(1),
            _ => {}
        }
    }

    fn adjust_selected_setting(&mut self, delta: isize) {
        let items = settings::items();
        let item = &items[self.settings_panel.selected.min(items.len() - 1)];
        let s = &mut self.config.settings;
        match item.kind {
            settings::Kind::Bool => match item.key {
                "soft_wrap" => s.soft_wrap = !s.soft_wrap,
                "line_numbers" => s.line_numbers = !s.line_numbers,
                "relative_line_numbers" => s.relative_line_numbers = !s.relative_line_numbers,
                "syntax_highlighting" => s.syntax_highlighting = !s.syntax_highlighting,
                "insert_spaces" => s.insert_spaces = !s.insert_spaces,
                "show_whitespace" => s.show_whitespace = !s.show_whitespace,
                "trim_trailing_whitespace_on_save" => {
                    s.trim_trailing_whitespace_on_save = !s.trim_trailing_whitespace_on_save
                }
                "render_callouts" => s.render_callouts = !s.render_callouts,
                "mouse" => s.mouse = !s.mouse,
                _ => {}
            },
            settings::Kind::Int(lo, hi) => {
                if item.key == "tab_width" {
                    let v = s.tab_width as isize + delta;
                    s.tab_width = v.clamp(lo as isize, hi as isize) as usize;
                }
            }
            settings::Kind::Choice => {
                if item.key == "theme" {
                    let themes = self.config.available_themes();
                    if !themes.is_empty() {
                        let cur = themes.iter().position(|t| t == &self.config.settings.theme).unwrap_or(0);
                        let n = themes.len() as isize;
                        let next = (((cur as isize + delta) % n + n) % n) as usize;
                        self.config.settings.theme = themes[next].clone();
                        self.config.reload_theme();
                    }
                }
            }
        }
        // Geometry-affecting settings need the viewport re-clamped.
        self.ensure_cursor_visible();
    }

    /// Human-readable current value for a setting key (for the panel display).
    pub fn setting_value(&self, key: &str) -> String {
        let s = &self.config.settings;
        let on = |b: bool| if b { "on".to_string() } else { "off".to_string() };
        match key {
            "theme" => s.theme.clone(),
            "soft_wrap" => on(s.soft_wrap),
            "line_numbers" => on(s.line_numbers),
            "relative_line_numbers" => on(s.relative_line_numbers),
            "syntax_highlighting" => on(s.syntax_highlighting),
            "tab_width" => s.tab_width.to_string(),
            "insert_spaces" => on(s.insert_spaces),
            "show_whitespace" => on(s.show_whitespace),
            "trim_trailing_whitespace_on_save" => on(s.trim_trailing_whitespace_on_save),
            "render_callouts" => on(s.render_callouts),
            "mouse" => on(s.mouse),
            _ => String::new(),
        }
    }

    fn handle_command_key(&mut self, ev: KeyEvent) {
        use crossterm::event::KeyCode::*;
        match ev.code {
            Esc => {
                self.command.close();
                self.search.matches.clear();
            }
            Enter => {
                self.execute_command_line();
            }
            Backspace => {
                self.command.input.pop();
                self.live_find();
            }
            Char(c) if !ev.modifiers.contains(KeyModifiers::CONTROL) => {
                self.command.input.push(c);
                self.live_find();
            }
            _ => {}
        }
    }

    /// For the find prompt, update highlighted matches as the user types.
    fn live_find(&mut self) {
        if self.command.kind == Some(PromptKind::Find) {
            let q = self.command.input.clone();
            self.search.query = q.clone();
            self.search.case_sensitive = q.chars().any(|c| c.is_uppercase());
            let text = self.active_buffer().rope.to_string();
            self.search.recompute(&text);
        }
    }

    fn open_prompt(&mut self, kind: PromptKind, prefill: &str) {
        self.command.open(kind, prefill);
    }

    fn execute_command_line(&mut self) {
        let kind = self.command.kind.clone();
        let input = self.command.input.trim().to_string();
        self.command.close();

        match kind {
            Some(PromptKind::Find) => {
                self.search.query = input.clone();
                self.search.case_sensitive = input.chars().any(|c| c.is_uppercase());
                let text = self.active_buffer().rope.to_string();
                self.search.recompute(&text);
                let n = self.search.matches.len();
                if n == 0 {
                    self.set_status(format!("no matches for \"{input}\""));
                } else {
                    self.execute(Command::FindNext);
                    self.set_status(format!("{n} matches"));
                }
            }
            Some(PromptKind::Replace) => {
                let (from, to) = match input.split_once('|') {
                    Some((f, t)) => (f.to_string(), t.to_string()),
                    None => (input.clone(), String::new()),
                };
                if from.is_empty() {
                    self.set_status("replace: nothing to find");
                } else {
                    self.do_replace(&from, &to, true);
                }
            }
            Some(PromptKind::SaveAs) => {
                self.execute(Command::SaveAs(files::expand_path(&input)));
            }
            None => {}
        }
    }

    pub fn handle_mouse(&mut self, ev: MouseEvent) {
        if !self.config.settings.mouse {
            return;
        }
        match ev.kind {
            MouseEventKind::ScrollDown => self.scroll(3),
            MouseEventKind::ScrollUp => self.scroll(-3),
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(pos) = self.mouse_to_pos(ev.column, ev.row) {
                    let extra = ev.modifiers.contains(KeyModifiers::CONTROL)
                        || ev.modifiers.contains(KeyModifiers::ALT);
                    if extra {
                        self.active_buffer_mut().add_cursor_at(pos);
                    } else {
                        self.active_buffer_mut().set_single_cursor(pos, false);
                    }
                    self.plugins.dispatch(&Event::CursorMove);
                    self.ensure_cursor_visible();
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(pos) = self.mouse_to_pos(ev.column, ev.row) {
                    self.active_buffer_mut().set_single_cursor(pos, true);
                    self.ensure_cursor_visible();
                }
            }
            _ => {}
        }
    }

    /// Scroll the viewport by `delta` rows (visual rows when wrapping).
    fn scroll(&mut self, delta: isize) {
        if self.soft_wrap() {
            let width = self.text_cols().max(1);
            let buf = self.active_buffer();
            let (mut l, mut s) = (self.top_line, self.top_subrow);
            for _ in 0..delta.unsigned_abs() {
                let next = if delta < 0 {
                    wrap::prev_visual(buf, l, s, width)
                } else {
                    wrap::next_visual(buf, l, s, width)
                };
                match next {
                    Some((nl, ns)) => {
                        l = nl;
                        s = ns;
                    }
                    None => break,
                }
            }
            self.top_line = l;
            self.top_subrow = s;
        } else if delta < 0 {
            self.top_line = self.top_line.saturating_sub(delta.unsigned_abs());
        } else {
            let max = self.active_buffer().len_lines().saturating_sub(1);
            self.top_line = (self.top_line + delta as usize).min(max);
        }
    }

    /// Map a terminal cell to a buffer char position, or `None` if outside the
    /// text area.
    fn mouse_to_pos(&self, col: u16, row: u16) -> Option<usize> {
        let gutter = self.gutter();
        if row as usize >= self.text_rows() {
            return None;
        }
        if self.soft_wrap() {
            let width = self.text_cols().max(1);
            let buf = self.active_buffer();
            let char_col = col.saturating_sub(gutter) as usize;
            let (mut l, mut s) = (self.top_line, self.top_subrow);
            for _ in 0..row {
                match wrap::next_visual(buf, l, s, width) {
                    Some((nl, ns)) => {
                        l = nl;
                        s = ns;
                    }
                    None => return Some(buf.len_chars()),
                }
            }
            Some(wrap::pos_at(buf, l, s, char_col, width))
        } else {
            if col < gutter {
                return None;
            }
            let line = self.top_line + row as usize;
            let b = self.active_buffer();
            if line >= b.len_lines() {
                return Some(b.len_chars());
            }
            let char_col = self.left_col + (col - gutter) as usize;
            Some(b.line_col_to_pos(line, char_col))
        }
    }

    // --- command execution -------------------------------------------------

    pub fn execute(&mut self, command: Command) {
        let mut edited = false;
        let mut moved = false;

        match command {
            // Files
            Command::Save => self.do_save(),
            Command::SaveAs(p) => self.do_save_as(p),
            Command::OpenFile(p) => {
                if p.as_os_str().is_empty() {
                    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                    self.file_picker.open(cwd);
                } else {
                    self.do_open(p);
                }
            }
            // Quitting never loses work: unsaved changes stay in the recovery
            // store and come back on the next launch.
            Command::Quit => self.shutdown(false),
            // Explicitly discard unsaved changes (clears the recovery store).
            Command::ForceQuit => self.shutdown(true),
            Command::SaveAndQuit => {
                if self.active_buffer().path.is_some() {
                    self.do_save();
                    self.shutdown(false);
                } else {
                    self.open_prompt(PromptKind::SaveAs, "");
                }
            }

            // Editing
            Command::InsertChar(c) => {
                self.active_buffer_mut().insert_char(c);
                edited = true;
            }
            Command::InsertNewline => {
                self.active_buffer_mut().insert_newline(true);
                edited = true;
            }
            Command::Backspace => {
                self.active_buffer_mut().backspace();
                edited = true;
            }
            Command::Delete => {
                self.active_buffer_mut().delete();
                edited = true;
            }
            Command::Tab => {
                let (sp, tw) = (self.config.settings.insert_spaces, self.config.settings.tab_width);
                self.active_buffer_mut().insert_tab(sp, tw);
                edited = true;
            }
            Command::Undo => {
                if !self.active_buffer_mut().undo() {
                    self.set_status("nothing to undo");
                }
                moved = true;
            }
            Command::Redo => {
                if !self.active_buffer_mut().redo() {
                    self.set_status("nothing to redo");
                }
                moved = true;
            }
            Command::ToggleComment => {
                self.active_buffer_mut().toggle_line_comment();
                edited = true;
            }
            Command::ToggleBold => {
                self.active_buffer_mut().toggle_wrap("**");
                edited = true;
            }
            Command::ToggleItalic => {
                self.active_buffer_mut().toggle_wrap("_");
                edited = true;
            }

            // Movement
            Command::MoveLeft => { self.active_buffer_mut().move_left(false); moved = true; }
            Command::MoveRight => { self.active_buffer_mut().move_right(false); moved = true; }
            Command::MoveUp => { self.move_up(false); moved = true; }
            Command::MoveDown => { self.move_down(false); moved = true; }
            Command::MoveWordLeft => { self.active_buffer_mut().move_word_left(false); moved = true; }
            Command::MoveWordRight => { self.active_buffer_mut().move_word_right(false); moved = true; }
            Command::MoveLineStart => { self.active_buffer_mut().move_line_start(false); moved = true; }
            Command::MoveLineEnd => { self.active_buffer_mut().move_line_end(false); moved = true; }
            Command::MoveBufferStart => { self.active_buffer_mut().move_buffer_start(false); moved = true; }
            Command::MoveBufferEnd => { self.active_buffer_mut().move_buffer_end(false); moved = true; }
            Command::PageUp => { self.page(-1); moved = true; }
            Command::PageDown => { self.page(1); moved = true; }

            // Selection
            Command::ExtendLeft => { self.active_buffer_mut().move_left(true); moved = true; }
            Command::ExtendRight => { self.active_buffer_mut().move_right(true); moved = true; }
            Command::ExtendUp => { self.move_up(true); moved = true; }
            Command::ExtendDown => { self.move_down(true); moved = true; }
            Command::SelectAll => { self.active_buffer_mut().select_all(); moved = true; }
            Command::SelectLine => { self.active_buffer_mut().select_line(); moved = true; }
            Command::CollapseSelection => { self.active_buffer_mut().collapse_selections(); moved = true; }

            // Multi-cursor
            Command::AddCursorAbove => { self.active_buffer_mut().add_cursor_vertical(-1); moved = true; }
            Command::AddCursorBelow => { self.active_buffer_mut().add_cursor_vertical(1); moved = true; }
            Command::AddCursorNextMatch => {
                let cs = self.smart_case();
                self.active_buffer_mut().add_cursor_next_match(cs);
                moved = true;
            }
            Command::SelectAllMatches => {
                let cs = self.smart_case();
                self.active_buffer_mut().select_all_matches(cs);
                let n = self.active_buffer().cursors.len();
                self.set_status(format!("{n} cursors"));
                moved = true;
            }
            Command::ClearExtraCursors => {
                self.active_buffer_mut().clear_extra_cursors();
                moved = true;
            }

            // Search / replace
            Command::Find => self.open_prompt(PromptKind::Find, ""),
            Command::FindNext => self.find_step(true),
            Command::FindPrev => self.find_step(false),
            Command::Replace { from, to } => {
                if from.is_empty() {
                    self.open_prompt(PromptKind::Replace, "");
                } else {
                    self.do_replace(&from, &to, false);
                    edited = true;
                }
            }
            Command::ReplaceAll { from, to } => {
                if from.is_empty() {
                    self.open_prompt(PromptKind::Replace, "");
                } else {
                    self.do_replace(&from, &to, true);
                    edited = true;
                }
            }

            // Buffers
            Command::NextBuffer => self.switch_buffer(1),
            Command::PrevBuffer => self.switch_buffer(-1),
            Command::CloseBuffer => self.close_buffer(),

            // Command palette
            Command::CommandPalette => self.palette.open(),

            Command::Settings => self.settings_panel.open(),

            // View
            Command::ToggleSoftWrap => {
                self.config.settings.soft_wrap = !self.config.settings.soft_wrap;
                self.top_subrow = 0;
                self.left_col = 0;
                self.ensure_cursor_visible();
                let state = if self.config.settings.soft_wrap { "on" } else { "off" };
                self.set_status(format!("soft wrap {state}"));
            }

            Command::NoOp => {}
        }

        if edited {
            // Match highlights are invalidated by edits; clear until next find.
            self.search.matches.clear();
            self.plugins.dispatch(&Event::BufferChange);
            self.ensure_cursor_visible();
        }
        if moved {
            self.plugins.dispatch(&Event::CursorMove);
            self.ensure_cursor_visible();
        }
    }

    fn smart_case(&self) -> bool {
        self.active_buffer()
            .primary_selection_text()
            .map(|s| s.chars().any(|c| c.is_uppercase()))
            .unwrap_or(false)
    }


    /// Persist palette usage + recent files, notify plugins, and request exit.
    /// When `discard` is false (normal quit) the recovery store is flushed and
    /// kept, so unsaved changes survive and reopen next launch. When true
    /// (discard/force-quit) the store is cleared, throwing away unsaved work.
    fn shutdown(&mut self, discard: bool) {
        self.palette.save();
        self.file_picker.save();
        if discard {
            self.recovery.clear();
        } else {
            self.autosave(); // final flush so the very latest edits are kept
        }
        self.plugins.dispatch(&Event::Exit);
        self.should_quit = true;
    }

    fn do_save(&mut self) {
        if self.active_buffer().path.is_none() {
            self.open_prompt(PromptKind::SaveAs, "");
            return;
        }
        if self.config.settings.trim_trailing_whitespace_on_save {
            self.active_buffer_mut().trim_trailing_whitespace();
        }
        self.disk_warned = false;
        match self.active_buffer_mut().save() {
            Ok(()) => {
                let name = self.active_buffer().name();
                self.set_status(format!("saved {name}"));
                if let Some(p) = self.active_buffer().path.clone() {
                    self.plugins.dispatch(&Event::SaveFile(p));
                }
            }
            Err(e) => self.set_status(format!("save failed: {e:#}")),
        }
    }

    fn do_save_as(&mut self, path: PathBuf) {
        self.disk_warned = false;
        if self.config.settings.trim_trailing_whitespace_on_save {
            self.active_buffer_mut().trim_trailing_whitespace();
        }
        match self.active_buffer_mut().save_to(&path) {
            Ok(()) => {
                let name = self.active_buffer().name();
                self.set_status(format!("saved {name}"));
                self.plugins.dispatch(&Event::SaveFile(path));
            }
            Err(e) => self.set_status(format!("save failed: {e:#}")),
        }
    }

    fn do_open(&mut self, path: PathBuf) {
        self.disk_warned = false;
        self.file_picker.record_open(&path);
        // If already open, just switch to it.
        if let Some(i) = self.buffers.iter().position(|b| b.path.as_deref() == Some(path.as_path())) {
            self.active = i;
            self.top_line = 0;
            self.top_subrow = 0;
            self.left_col = 0;
            self.ensure_cursor_visible();
            return;
        }
        match Buffer::from_file(&path) {
            Ok(mut b) => {
                b.recovery_id = self.fresh_recovery_id();
                self.buffers.push(b);
                self.active = self.buffers.len() - 1;
                self.top_line = 0;
                self.top_subrow = 0;
                self.left_col = 0;
                self.set_status(format!("opened {}", files::display_path(&path)));
                self.plugins.dispatch(&Event::OpenFile(path));
                self.ensure_cursor_visible();
            }
            Err(e) => self.set_status(format!("open failed: {e:#}")),
        }
    }

    fn switch_buffer(&mut self, delta: isize) {
        let n = self.buffers.len() as isize;
        self.active = (((self.active as isize + delta) % n + n) % n) as usize;
        self.top_line = 0;
        self.top_subrow = 0;
        self.left_col = 0;
        self.disk_warned = false;
        self.ensure_cursor_visible();
    }

    fn close_buffer(&mut self) {
        if self.active_buffer().modified {
            self.set_status("buffer modified; save or use :q! semantics first");
            return;
        }
        if self.buffers.len() == 1 {
            let mut b = Buffer::empty();
            b.recovery_id = self.fresh_recovery_id();
            self.buffers[0] = b;
        } else {
            let closed = self.buffers.remove(self.active);
            self.recovery.remove_backup(closed.recovery_id);
            if self.active >= self.buffers.len() {
                self.active = self.buffers.len() - 1;
            }
        }
        self.top_line = 0;
        self.top_subrow = 0;
        self.left_col = 0;
    }

    fn find_step(&mut self, forward: bool) {
        if self.search.query.is_empty() {
            self.open_prompt(PromptKind::Find, "");
            return;
        }
        let text = self.active_buffer().rope.to_string();
        let head = self.active_buffer().primary_cursor().head;
        let cs = self.search.case_sensitive;
        let hit = if forward {
            find::find_next(&text, &self.search.query, head + 1, cs)
        } else {
            find::find_prev(&text, &self.search.query, head, cs)
        };
        self.search.recompute(&text);
        if let Some((s, e)) = hit {
            let b = self.active_buffer_mut();
            b.set_single_cursor(e, false);
            // Select the match.
            b.cursors[0].anchor = s;
            b.primary = 0;
            self.plugins.dispatch(&Event::CursorMove);
            self.ensure_cursor_visible();
        } else {
            self.set_status("no matches");
        }
    }

    fn do_replace(&mut self, from: &str, to: &str, all: bool) {
        let text = self.active_buffer().rope.to_string();
        let cs = from.chars().any(|c| c.is_uppercase());
        let matches = find::find_all(&text, from, cs);
        if matches.is_empty() {
            self.set_status(format!("no matches for \"{from}\""));
            return;
        }
        let count = if all { matches.len() } else { 1 };
        let to_apply: Vec<(usize, usize)> = if all { matches } else { vec![matches[0]] };
        self.active_buffer_mut().replace_ranges(&to_apply, to);
        self.search.matches.clear();
        self.plugins.dispatch(&Event::BufferChange);
        self.ensure_cursor_visible();
        self.set_status(format!("replaced {count} occurrence(s)"));
    }
}

/// True if a line's first non-whitespace run opens/closes a Markdown code
/// fence (```` ``` ```` or `~~~`). Only inspects the leading characters.
fn line_is_fence(line: ropey::RopeSlice) -> bool {
    let mut chars = line.chars().skip_while(|c| *c == ' ' || *c == '\t');
    let first = match chars.next() {
        Some(c @ ('`' | '~')) => c,
        _ => return false,
    };
    chars.next() == Some(first) && chars.next() == Some(first)
}

/// Find the recovery backup for a specific file path in a saved session.
fn backup_for_path(recovery: &Recovery, session: &Option<Session>, path: &Path) -> Option<String> {
    let sess = session.as_ref()?;
    let target = path.canonicalize().ok();
    for e in &sess.buffers {
        if !e.has_backup {
            continue;
        }
        let p = e.path.as_ref()?;
        let same = match &target {
            Some(t) => std::path::Path::new(p).canonicalize().ok().as_ref() == Some(t),
            None => std::path::Path::new(p) == path,
        };
        if same {
            return recovery.read_backup(e.id);
        }
    }
    None
}
