//! The "Go to Symbol" outline — a modal listing a file's definitions (or
//! Markdown headings), fuzzy-filterable by typing, that jumps the cursor to the
//! selected one. Built on the same panel pattern as the settings/callout panels.

use super::screen::{Cell, Screen};
use crate::app::App;
use crate::commands::palette::fuzzy;
use crate::syntax::structure::Symbol;
use crossterm::style::Color;

#[derive(Default)]
pub struct SymbolPanel {
    pub open: bool,
    pub selected: usize,
    pub query: String,
    items: Vec<Symbol>,
    /// Indices into `items` matching the query, best match first.
    filtered: Vec<usize>,
}

impl SymbolPanel {
    pub fn open(&mut self, items: Vec<Symbol>) {
        self.open = true;
        self.selected = 0;
        self.query.clear();
        self.items = items;
        self.refilter();
    }

    pub fn close(&mut self) {
        self.open = false;
        self.items.clear();
        self.filtered.clear();
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.filtered.is_empty() {
            return;
        }
        let n = self.filtered.len() as isize;
        self.selected = (((self.selected as isize + delta) % n + n) % n) as usize;
    }

    pub fn push_char(&mut self, c: char) {
        self.query.push(c);
        self.refilter();
    }

    pub fn pop_char(&mut self) {
        self.query.pop();
        self.refilter();
    }

    /// The buffer line of the selected symbol, if any.
    pub fn selected_line(&self) -> Option<usize> {
        let i = *self.filtered.get(self.selected)?;
        Some(self.items[i].line)
    }

    fn refilter(&mut self) {
        let mut scored: Vec<(i32, usize)> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, s)| fuzzy(&self.query, &s.name).map(|(score, _)| (score, i)))
            .collect();
        // Best score first; ties keep source order.
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        self.filtered = scored.into_iter().map(|(_, i)| i).collect();
        if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len().saturating_sub(1);
        }
    }
}

pub fn render(screen: &mut Screen, app: &App) {
    if app.width < 30 || app.height < 10 {
        return;
    }
    let panel = &app.symbol_panel;
    let theme = &app.config.theme;
    let panel_bg = theme.statusbar_bg;
    let border = theme.line_number_current;

    let width = app.width.saturating_sub(6).min(64).max(30);
    let x0 = (app.width - width) / 2;
    let y0 = 2u16;
    let inner = (width - 2) as usize;
    let list_rows = (app.height as usize).saturating_sub(8).min(panel.filtered.len().max(1)) as u16;

    // Top border + title.
    let mut top = String::from("┌ Go to Symbol ");
    while top.chars().count() < (width - 1) as usize {
        top.push('─');
    }
    top.push('┐');
    fill(screen, x0, y0, width, panel_bg);
    screen.put_str(x0, y0, &top, border, panel_bg, false, false);

    // Query line.
    let qy = y0 + 1;
    fill(screen, x0, qy, width, panel_bg);
    screen.put_str(x0, qy, "│", border, panel_bg, false, false);
    screen.put_str(x0 + width - 1, qy, "│", border, panel_bg, false, false);
    let q = format!("› {}", panel.query);
    screen.put_str(x0 + 2, qy, &q, theme.foreground, panel_bg, false, false);

    // Rows.
    let top_idx = scroll_top(panel.selected, list_rows as usize, panel.filtered.len());
    for r in 0..list_rows {
        let y = qy + 1 + r;
        fill(screen, x0, y, width, panel_bg);
        screen.put_str(x0, y, "│", border, panel_bg, false, false);
        screen.put_str(x0 + width - 1, y, "│", border, panel_bg, false, false);
        let fi = top_idx + r as usize;
        if fi >= panel.filtered.len() {
            continue;
        }
        let sym = &panel.items[panel.filtered[fi]];
        let selected = fi == panel.selected;
        let row_bg = if selected { theme.selection } else { panel_bg };
        for x in (x0 + 1)..(x0 + width - 1) {
            screen.set(x, y, Cell { ch: ' ', fg: theme.foreground, bg: row_bg, bold: false, italic: false });
        }
        let caret = if selected { "▶ " } else { "  " };
        screen.put_str(x0 + 1, y, caret, theme.keyword, row_bg, false, false);
        let kind = format!("{:<7}", sym.kind);
        screen.put_str(x0 + 3, y, &kind, theme.type_, row_bg, false, true);
        screen.put_str(x0 + 11, y, &sym.name, theme.foreground, row_bg, false, false);
        // Right-aligned line number.
        let ln = format!("{}", sym.line + 1);
        let lx = x0 + width - 1 - ln.chars().count() as u16 - 1;
        screen.put_str(lx, y, &ln, theme.line_number, row_bg, false, false);
    }

    // Footer.
    let fy = qy + 1 + list_rows;
    let mut sep = String::from("├");
    for _ in 0..inner {
        sep.push('─');
    }
    sep.push('┤');
    fill(screen, x0, fy, width, panel_bg);
    screen.put_str(x0, fy, &sep, border, panel_bg, false, false);
    let hy = fy + 1;
    fill(screen, x0, hy, width, panel_bg);
    screen.put_str(x0, hy, "│", border, panel_bg, false, false);
    screen.put_str(x0 + width - 1, hy, "│", border, panel_bg, false, false);
    let hint = if panel.filtered.is_empty() { "no symbols · esc" } else { "type to filter · ↑↓ · enter jump · esc" };
    screen.put_str(x0 + 2, hy, hint, theme.comment, panel_bg, false, false);
    let mut bot = String::from("└");
    for _ in 0..inner {
        bot.push('─');
    }
    bot.push('┘');
    let by = hy + 1;
    fill(screen, x0, by, width, panel_bg);
    screen.put_str(x0, by, &bot, border, panel_bg, false, false);
}

fn scroll_top(selected: usize, rows: usize, len: usize) -> usize {
    if len <= rows || selected < rows {
        0
    } else {
        (selected + 1 - rows).min(len - rows)
    }
}

fn fill(screen: &mut Screen, x0: u16, y: u16, width: u16, bg: Color) {
    for x in x0..x0 + width {
        screen.set(x, y, Cell { ch: ' ', fg: bg, bg, bold: false, italic: false });
    }
}
