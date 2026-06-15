//! A reusable centred overlay used by the command palette and the file picker:
//! a titled box with a query line and a ranked, fuzzy-highlighted result list.
//! Returns the desired terminal cursor position (end of the query).

use super::screen::{Cell, Screen};
use crate::app::App;
use crossterm::style::Color;

const MAX_ROWS: usize = 12;

/// One result row. `positions` are matched char indices in `text` (highlighted);
/// `hint` is right-aligned (e.g. a keybinding).
pub struct Row<'a> {
    pub text: &'a str,
    pub positions: &'a [usize],
    pub hint: &'a str,
}

#[allow(clippy::too_many_arguments)]
pub fn render(
    screen: &mut Screen,
    app: &App,
    tabs: &[&str],
    active_tab: usize,
    query: &str,
    rows: &[Row],
    selected: usize,
    empty_msg: &str,
) -> Option<(u16, u16)> {
    if app.width < 24 || app.height < 8 {
        return None;
    }
    let theme = &app.config.theme;
    let panel_bg = theme.statusbar_bg;
    let border = theme.line_number_current;

    let width = app.width.saturating_sub(4).min(72).max(20);
    let x0 = (app.width - width) / 2;
    let y0 = 2u16;
    let inner = (width - 2) as usize;

    let count = rows.len();
    let visible = count.min(MAX_ROWS).max(1);
    let offset = if selected >= MAX_ROWS { selected - MAX_ROWS + 1 } else { 0 };

    // Top border with a tab bar; the active tab is highlighted.
    put_row(screen, x0, y0, width, panel_bg);
    screen.put_str(x0, y0, "┌─", border, panel_bg, false, false);
    let mut tx = x0 + 2;
    for (i, label) in tabs.iter().enumerate() {
        let active = i == active_tab;
        let (fg, txt, bold) = if active {
            (theme.keyword, format!(" {label} "), true)
        } else {
            (theme.comment, format!(" {label} "), false)
        };
        let bg = if active { theme.selection } else { panel_bg };
        tx = screen.put_str(tx, y0, &txt, fg, bg, bold, false);
    }
    // Fill the rest of the top border.
    let mut x = tx;
    while x < x0 + width - 1 {
        screen.set(x, y0, Cell { ch: '─', fg: border, bg: panel_bg, bold: false, italic: false });
        x += 1;
    }
    screen.set(x0 + width - 1, y0, Cell { ch: '┐', fg: border, bg: panel_bg, bold: false, italic: false });

    // Query line.
    let qy = y0 + 1;
    put_row(screen, x0, qy, width, panel_bg);
    screen.put_str(x0, qy, "│", border, panel_bg, false, false);
    screen.put_str(x0 + width - 1, qy, "│", border, panel_bg, false, false);
    let px = screen.put_str(x0 + 1, qy, "  › ", theme.keyword, panel_bg, true, false);
    let qx = screen.put_str(px, qy, query, theme.foreground, panel_bg, false, false);

    // Separator.
    let sy = y0 + 2;
    let mut sep = String::from("├");
    for _ in 0..inner {
        sep.push('─');
    }
    sep.push('┤');
    put_row(screen, x0, sy, width, panel_bg);
    screen.put_str(x0, sy, &sep, border, panel_bg, false, false);

    if count == 0 {
        let ry = sy + 1;
        put_row(screen, x0, ry, width, panel_bg);
        screen.put_str(x0, ry, "│", border, panel_bg, false, false);
        screen.put_str(x0 + width - 1, ry, "│", border, panel_bg, false, false);
        screen.put_str(x0 + 2, ry, empty_msg, theme.comment, panel_bg, false, false);
    } else {
        for row in 0..visible {
            let ri = offset + row;
            if ri >= count {
                break;
            }
            let ry = sy + 1 + row as u16;
            let item = &rows[ri];
            let is_sel = ri == selected;
            let row_bg = if is_sel { theme.selection } else { panel_bg };

            put_row(screen, x0, ry, width, row_bg);
            screen.put_str(x0, ry, "│", border, panel_bg, false, false);
            screen.put_str(x0 + width - 1, ry, "│", border, panel_bg, false, false);

            let caret = if is_sel { "▶ " } else { "  " };
            screen.put_str(x0 + 1, ry, caret, theme.keyword, row_bg, false, false);

            let text_x0 = x0 + 3;
            let hint_w = item.hint.chars().count();
            let room = inner.saturating_sub(2 + hint_w + 2);
            for (i, ch) in item.text.chars().enumerate() {
                if i >= room {
                    break;
                }
                let matched = item.positions.contains(&i);
                let (fg, bold) = if matched { (theme.keyword, true) } else { (theme.foreground, false) };
                screen.set(text_x0 + i as u16, ry, Cell { ch, fg, bg: row_bg, bold, italic: false });
            }

            if hint_w > 0 {
                let hint_x = x0 + width - 1 - hint_w as u16 - 1;
                screen.put_str(hint_x, ry, item.hint, theme.comment, row_bg, false, false);
            }
        }
    }

    // Bottom border.
    let by = sy + 1 + visible as u16;
    let mut bot = String::from("└");
    for _ in 0..inner {
        bot.push('─');
    }
    bot.push('┘');
    put_row(screen, x0, by, width, panel_bg);
    screen.put_str(x0, by, &bot, border, panel_bg, false, false);

    Some((qx.min(x0 + width - 2), qy))
}

fn put_row(screen: &mut Screen, x0: u16, y: u16, width: u16, bg: Color) {
    for x in x0..x0 + width {
        screen.set(x, y, Cell { ch: ' ', fg: bg, bg, bold: false, italic: false });
    }
}
