//! Renders the command palette as a centred overlay near the top of the
//! screen (Spotlight-style): a query line plus a ranked, fuzzy-highlighted list
//! of actions with their keybinding hints. Returns the desired terminal cursor
//! position (end of the query) so the caller can place the caret there.

use super::screen::{Cell, Screen};
use crate::app::App;
use crate::commands::palette::catalog;
use crossterm::style::Color;

const MAX_ROWS: usize = 12;

pub fn render(screen: &mut Screen, app: &App) -> Option<(u16, u16)> {
    if app.width < 24 || app.height < 8 {
        return None;
    }
    let theme = &app.config.theme;
    let panel_bg = theme.statusbar_bg;
    let border = theme.line_number_current;
    let cat = catalog();
    let palette = &app.palette;

    let width = app.width.saturating_sub(4).min(64).max(20);
    let x0 = (app.width - width) / 2;
    let y0 = 2u16;
    let inner = (width - 2) as usize;

    let result_count = palette.results.len();
    let visible = result_count.min(MAX_ROWS).max(1);
    // Scroll the list so the selection stays visible.
    let offset = if palette.selected >= MAX_ROWS {
        palette.selected - MAX_ROWS + 1
    } else {
        0
    };

    // Top border with title.
    let title = " Command Palette ";
    let mut top = String::from("┌");
    top.push_str(title);
    while top.chars().count() < (width - 1) as usize {
        top.push('─');
    }
    top.push('┐');
    put_row(screen, x0, y0, width, panel_bg);
    screen.put_str(x0, y0, &top, border, panel_bg, false, false);

    // Query line.
    let qy = y0 + 1;
    put_row(screen, x0, qy, width, panel_bg);
    screen.put_str(x0, qy, "│", border, panel_bg, false, false);
    screen.put_str(x0 + width - 1, qy, "│", border, panel_bg, false, false);
    let prompt = "  › ";
    let px = screen.put_str(x0 + 1, qy, prompt, theme.keyword, panel_bg, true, false);
    let qx = screen.put_str(px, qy, &palette.query, theme.foreground, panel_bg, false, false);

    // Separator.
    let sy = y0 + 2;
    let mut sep = String::from("├");
    for _ in 0..inner {
        sep.push('─');
    }
    sep.push('┤');
    put_row(screen, x0, sy, width, panel_bg);
    screen.put_str(x0, sy, &sep, border, panel_bg, false, false);

    // Result rows.
    if result_count == 0 {
        let ry = sy + 1;
        put_row(screen, x0, ry, width, panel_bg);
        screen.put_str(x0, ry, "│", border, panel_bg, false, false);
        screen.put_str(x0 + width - 1, ry, "│", border, panel_bg, false, false);
        screen.put_str(x0 + 2, ry, "no matching commands", theme.comment, panel_bg, false, false);
    } else {
        for row in 0..visible {
            let ri = offset + row;
            if ri >= result_count {
                break;
            }
            let ry = sy + 1 + row as u16;
            let result = &palette.results[ri];
            let action = &cat[result.idx];
            let selected = ri == palette.selected;
            let row_bg = if selected { theme.selection } else { panel_bg };

            put_row(screen, x0, ry, width, row_bg);
            screen.put_str(x0, ry, "│", border, panel_bg, false, false);
            screen.put_str(x0 + width - 1, ry, "│", border, panel_bg, false, false);

            // Selection caret + title with matched characters highlighted.
            let caret = if selected { "▶ " } else { "  " };
            screen.put_str(x0 + 1, ry, caret, theme.keyword, row_bg, false, false);

            let title_x0 = x0 + 3;
            let hint_w = action.hint.chars().count();
            let title_room = inner.saturating_sub(2 + hint_w + 2);
            for (i, ch) in action.title.chars().enumerate() {
                if i >= title_room {
                    break;
                }
                let matched = result.positions.contains(&i);
                let (fg, bold) = if matched {
                    (theme.keyword, true)
                } else {
                    (theme.foreground, false)
                };
                screen.set(title_x0 + i as u16, ry, Cell { ch, fg, bg: row_bg, bold, italic: false });
            }

            // Right-aligned keybinding hint.
            if hint_w > 0 {
                let hint_x = x0 + width - 1 - hint_w as u16 - 1;
                screen.put_str(hint_x, ry, action.hint, theme.comment, row_bg, false, false);
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

/// Fill a box row with the panel background.
fn put_row(screen: &mut Screen, x0: u16, y: u16, width: u16, bg: Color) {
    for x in x0..x0 + width {
        screen.set(x, y, Cell { ch: ' ', fg: bg, bg, bold: false, italic: false });
    }
}
