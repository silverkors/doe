//! The callout style panel — a modal (opened from the command palette) to
//! change each callout type's accent colour and glyph. Navigate with the
//! arrows, cycle the colour with ←/→ and the glyph with `[`/`]`; changes apply
//! live and are written to `callouts.toml` when the panel closes.

use super::screen::{Cell, Screen};
use crate::app::App;
use crossterm::style::Color;

/// Colour choices offered when cycling a callout's accent with ←/→.
pub const COLOR_PALETTE: &[(u8, u8, u8)] = &[
    (0x44, 0x8a, 0xff), // blue
    (0x00, 0xb8, 0xd4), // cyan
    (0x00, 0xbf, 0xa6), // teal
    (0x00, 0xc8, 0x53), // green
    (0xf0, 0xb4, 0x00), // amber
    (0xff, 0x91, 0x00), // orange
    (0xf5, 0x51, 0x2e), // deep orange
    (0xff, 0x52, 0x52), // red
    (0xff, 0x17, 0x44), // crimson
    (0xe0, 0x6c, 0x75), // rose
    (0x7c, 0x4d, 0xff), // violet
    (0x9d, 0x7c, 0xd8), // purple
    (0x6c, 0xb6, 0xff), // sky
    (0x9e, 0x9e, 0x9e), // grey
];

/// Glyph choices offered when cycling a callout's icon with `[`/`]`. Kept to
/// single-width symbols so the card layout stays aligned.
pub const ICON_PALETTE: &[char] = &[
    '●', '◆', '▲', '■', '✓', '✗', '?', '!', '»', '"', '★', '☆', '⚑', '✎', '✦', '◉', '➤', '❖', '♦',
    '♪', '✝', '†',
];

#[derive(Default)]
pub struct CalloutPanel {
    pub open: bool,
    pub selected: usize,
}

impl CalloutPanel {
    pub fn open(&mut self) {
        self.open = true;
        self.selected = 0;
    }
    pub fn close(&mut self) {
        self.open = false;
    }
    pub fn move_selection(&mut self, delta: isize, len: usize) {
        if len == 0 {
            return;
        }
        let n = len as isize;
        self.selected = (((self.selected as isize + delta) % n + n) % n) as usize;
    }
}

/// Step a callout's accent colour to the next/previous palette entry. An
/// off-palette colour (e.g. freshly imported) snaps to the first entry.
pub fn cycle_color(current: Color, dir: isize) -> Color {
    let idx = COLOR_PALETTE.iter().position(|&(r, g, b)| current == Color::Rgb { r, g, b });
    let n = COLOR_PALETTE.len() as isize;
    let next = match idx {
        Some(i) => (((i as isize + dir) % n + n) % n) as usize,
        None => 0,
    };
    let (r, g, b) = COLOR_PALETTE[next];
    Color::Rgb { r, g, b }
}

/// Step a callout's glyph to the next/previous palette entry.
pub fn cycle_icon(current: char, dir: isize) -> char {
    let idx = ICON_PALETTE.iter().position(|&c| c == current);
    let n = ICON_PALETTE.len() as isize;
    let next = match idx {
        Some(i) => (((i as isize + dir) % n + n) % n) as usize,
        None => 0,
    };
    ICON_PALETTE[next]
}

fn hex(color: Color) -> String {
    match color {
        Color::Rgb { r, g, b } => format!("#{r:02x}{g:02x}{b:02x}"),
        _ => "------".to_string(),
    }
}

pub fn render(screen: &mut Screen, app: &App) {
    if app.width < 36 || app.height < 12 {
        return;
    }
    let theme = &app.config.theme;
    let panel_bg = theme.statusbar_bg;
    let border = theme.line_number_current;
    let list = &app.config.callouts.list;

    let width = app.width.saturating_sub(6).min(56).max(36);
    let x0 = (app.width - width) / 2;
    let y0 = 2u16;
    let inner = (width - 2) as usize;
    let rows = list.len().min((app.height as usize).saturating_sub(7)) as u16;

    // Top border + title.
    let mut top = String::from("┌ Callouts ");
    while top.chars().count() < (width - 1) as usize {
        top.push('─');
    }
    top.push('┐');
    fill(screen, x0, y0, width, panel_bg);
    screen.put_str(x0, y0, &top, border, panel_bg, false, false);

    // Rows: caret · glyph(in accent) · name · hex.
    let top_idx = scroll_top(app.callout_panel.selected, rows as usize, list.len());
    for r in 0..rows {
        let i = top_idx + r as usize;
        if i >= list.len() {
            break;
        }
        let c = &list[i];
        let y = y0 + 1 + r;
        let selected = i == app.callout_panel.selected;
        let row_bg = if selected { theme.selection } else { panel_bg };
        fill(screen, x0, y, width, row_bg);
        screen.put_str(x0, y, "│", border, panel_bg, false, false);
        screen.put_str(x0 + width - 1, y, "│", border, panel_bg, false, false);

        let caret = if selected { "▶ " } else { "  " };
        screen.put_str(x0 + 1, y, caret, theme.keyword, row_bg, false, false);
        // Accent glyph in the callout's own colour.
        screen.set(x0 + 3, y, Cell { ch: c.icon, fg: c.color, bg: row_bg, bold: true, italic: false });
        screen.put_str(x0 + 5, y, &c.name, theme.foreground, row_bg, false, false);

        // Right-aligned hex value.
        let value = hex(c.color);
        let vlen = value.chars().count() as u16;
        if vlen + 6 < width {
            let vx = x0 + width - 1 - vlen - 1;
            screen.put_str(vx, y, &value, c.color, row_bg, true, false);
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
    screen.put_str(x0 + 2, hy, "↑↓ move · ←→ colour · [ ] icon · i import · esc save", theme.comment, panel_bg, false, false);

    let mut bot = String::from("└");
    for _ in 0..inner {
        bot.push('─');
    }
    bot.push('┘');
    let by = hy + 1;
    fill(screen, x0, by, width, panel_bg);
    screen.put_str(x0, by, &bot, border, panel_bg, false, false);
}

/// First visible row index so the selection stays in view.
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
