//! The settings panel — a modal opened from the command palette (or Ctrl+,)
//! that lets you change preferences by navigating a list and toggling/cycling
//! values, with no need to edit the config file by hand. Changes apply live and
//! are written to `config.toml` when the panel closes.

use super::screen::{Cell, Screen};
use crate::app::App;

/// How a setting is edited.
pub enum Kind {
    Bool,
    /// Integer within an inclusive range.
    Int(usize, usize),
    /// One of a set of strings (e.g. theme names).
    Choice,
}

/// A settings row: the `key` matches a `Settings` field; `label` is shown.
pub struct Item {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: Kind,
}

pub fn items() -> Vec<Item> {
    vec![
        Item { key: "theme", label: "Theme", kind: Kind::Choice },
        Item { key: "soft_wrap", label: "Soft wrap", kind: Kind::Bool },
        Item { key: "line_numbers", label: "Line numbers", kind: Kind::Bool },
        Item { key: "relative_line_numbers", label: "Relative line numbers", kind: Kind::Bool },
        Item { key: "syntax_highlighting", label: "Syntax highlighting", kind: Kind::Bool },
        Item { key: "render_callouts", label: "Render Markdown callouts", kind: Kind::Bool },
        Item { key: "tab_width", label: "Tab width", kind: Kind::Int(1, 8) },
        Item { key: "insert_spaces", label: "Insert spaces (instead of tabs)", kind: Kind::Bool },
        Item { key: "show_whitespace", label: "Show whitespace", kind: Kind::Bool },
        Item { key: "trim_trailing_whitespace_on_save", label: "Trim trailing whitespace on save", kind: Kind::Bool },
        Item { key: "mouse", label: "Mouse", kind: Kind::Bool },
        Item { key: "touch_scroll", label: "Drag to scroll (touch/SSH)", kind: Kind::Bool },
    ]
}

#[derive(Default)]
pub struct SettingsPanel {
    pub open: bool,
    pub selected: usize,
}

impl SettingsPanel {
    pub fn open(&mut self) {
        self.open = true;
        self.selected = 0;
    }
    pub fn close(&mut self) {
        self.open = false;
    }
    pub fn move_selection(&mut self, delta: isize) {
        let n = items().len() as isize;
        self.selected = (((self.selected as isize + delta) % n + n) % n) as usize;
    }
}

pub fn render(screen: &mut Screen, app: &App) {
    if app.width < 30 || app.height < 10 {
        return;
    }
    let theme = &app.config.theme;
    let panel_bg = theme.statusbar_bg;
    let border = theme.line_number_current;
    let items = items();

    let width = app.width.saturating_sub(6).min(60).max(30);
    let x0 = (app.width - width) / 2;
    let y0 = 2u16;
    let inner = (width - 2) as usize;
    let rows = items.len() as u16;

    // Top border + title.
    let mut top = String::from("┌ Settings ");
    while top.chars().count() < (width - 1) as usize {
        top.push('─');
    }
    top.push('┐');
    fill(screen, x0, y0, width, panel_bg);
    screen.put_str(x0, y0, &top, border, panel_bg, false, false);

    // Rows.
    for (i, item) in items.iter().enumerate() {
        let y = y0 + 1 + i as u16;
        let selected = i == app.selected_setting();
        let row_bg = if selected { theme.selection } else { panel_bg };
        fill(screen, x0, y, width, row_bg);
        screen.put_str(x0, y, "│", border, panel_bg, false, false);
        screen.put_str(x0 + width - 1, y, "│", border, panel_bg, false, false);

        let caret = if selected { "▶ " } else { "  " };
        screen.put_str(x0 + 1, y, caret, theme.keyword, row_bg, false, false);
        screen.put_str(x0 + 3, y, item.label, theme.foreground, row_bg, false, false);

        // Right-aligned value.
        let value = app.setting_value(item.key);
        let vlen = value.chars().count() as u16;
        if vlen + 2 < width {
            let vx = x0 + width - 1 - vlen - 1;
            let (fg, bold) = (theme.string, true);
            screen.put_str(vx, y, &value, fg, row_bg, bold, false);
        }
    }

    // Footer hint.
    let fy = y0 + 1 + rows;
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
    screen.put_str(x0 + 2, hy, "↑↓ move · ←→/space change · esc save & close", theme.comment, panel_bg, false, false);

    let mut bot = String::from("└");
    for _ in 0..inner {
        bot.push('─');
    }
    bot.push('┘');
    let by = hy + 1;
    fill(screen, x0, by, width, panel_bg);
    screen.put_str(x0, by, &bot, border, panel_bg, false, false);
}

fn fill(screen: &mut Screen, x0: u16, y: u16, width: u16, bg: crossterm::style::Color) {
    for x in x0..x0 + width {
        screen.set(x, y, Cell { ch: ' ', fg: bg, bg, bold: false, italic: false });
    }
}
