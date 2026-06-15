//! Draws the full editor frame into the [`Screen`] back buffer. Only the
//! viewport's lines are processed (and over-long lines skip highlighting), so
//! rendering cost is bound to what's on screen rather than file size.

use super::screen::{Cell, Screen};
use super::{layout::Layout, statusbar, wrap};
use crate::app::App;
use crate::plugins::PluginView;
use crate::syntax::{highlighter_for, LineState, StyleKind};
use crossterm::style::Color;
use std::io::Write;

const SEARCH_BG: Color = Color::Rgb { r: 0x5a, g: 0x4a, b: 0x10 };
const BRACKET_BG: Color = Color::Rgb { r: 0x3a, g: 0x44, b: 0x4e };
const MAX_HIGHLIGHT_LINE: usize = 2000;
/// Bound the matching-bracket scan so it stays cheap on very large files.
const BRACKET_SCAN_LIMIT: usize = 200_000;

pub fn render(screen: &mut Screen, app: &App, out: &mut impl Write) -> std::io::Result<()> {
    let theme = &app.config.theme;
    let settings = &app.config.settings;
    screen.begin(app.width, app.height, theme.foreground, theme.background);

    let buf = app.active_buffer();
    let layout = Layout::compute(app.width, app.height, buf.len_lines(), settings.line_numbers);
    let (cur_line, _) = buf.pos_to_line_col(buf.primary_cursor().head);

    // Matching bracket pair for the primary cursor (display-only, bounded scan).
    let bracket_pair = buf.matching_bracket(buf.primary_cursor().head, BRACKET_SCAN_LIMIT);

    let highlighter = highlighter_for(buf.language);
    // Seed fence state from the lines above the viewport so a code block whose
    // opening fence has scrolled off the top still highlights correctly.
    let mut state = LineState { in_code_block: app.top_in_code_block };
    let text_width = layout.text_width() as usize;

    // --- build the list of visual rows to paint ----------------------------
    // A visual row is a slice `[start, end)` of one buffer line on one screen
    // row. Without soft wrap there is exactly one per line (with horizontal
    // scroll); with soft wrap a long line spans several.
    struct VisRow {
        line: usize,
        start: usize,
        end: usize,
        y: u16,
        gutter: bool,
    }
    let mut visrows: Vec<VisRow> = Vec::new();
    if settings.soft_wrap {
        let width = text_width.max(1);
        let (mut vl, mut vs) = (app.top_line, app.top_subrow);
        for row in 0..layout.text_rows {
            if vl >= buf.len_lines() {
                break;
            }
            let segs = wrap::segments(buf, vl, width);
            let (s, e) = segs[vs.min(segs.len() - 1)];
            visrows.push(VisRow { line: vl, start: s, end: e, y: row, gutter: vs == 0 });
            match wrap::next_visual(buf, vl, vs, width) {
                Some((l, ss)) => {
                    vl = l;
                    vs = ss;
                }
                None => break,
            }
        }
    } else {
        for row in 0..layout.text_rows {
            let ln = app.top_line + row as usize;
            if ln >= buf.len_lines() {
                break;
            }
            let line_len = buf.line_len_chars(ln);
            let s = app.left_col.min(line_len);
            let e = (app.left_col + text_width).min(line_len);
            visrows.push(VisRow { line: ln, start: s, end: e, y: row, gutter: true });
        }
    }

    // --- paint visual rows -------------------------------------------------
    // Styles are computed once per buffer line and reused across its wrapped
    // sub-rows; the highlighter state advances once per line.
    let mut loaded: Option<usize> = None;
    let mut kinds: Vec<StyleKind> = Vec::new();
    let mut bolds: Vec<bool> = Vec::new();
    let mut itals: Vec<bool> = Vec::new();
    let mut chars: Vec<char> = Vec::new();
    let mut line_start = 0usize;
    let mut line_matches: Vec<(usize, usize)> = Vec::new();

    for vr in &visrows {
        if loaded != Some(vr.line) {
            line_start = buf.rope.line_to_char(vr.line);
            let line_len = buf.line_len_chars(vr.line);
            let text: String = buf.rope.slice(line_start..line_start + line_len).to_string();
            chars = text.chars().collect();
            kinds = vec![StyleKind::Default; line_len];
            bolds = vec![false; line_len];
            itals = vec![false; line_len];
            if settings.syntax_highlighting && line_len <= MAX_HIGHLIGHT_LINE {
                for sp in highlighter.highlight_line(&text, &mut state) {
                    for c in sp.start..sp.end.min(line_len) {
                        kinds[c] = sp.kind;
                        bolds[c] = sp.bold;
                        itals[c] = sp.italic;
                    }
                }
            }
            let line_end = line_start + line_len;
            line_matches = app
                .search
                .matches
                .iter()
                .filter(|(s, e)| *s < line_end && *e > line_start)
                .copied()
                .collect();
            loaded = Some(vr.line);
        }

        if vr.gutter {
            draw_gutter(screen, &layout, app, vr.line, cur_line, vr.y);
        } else {
            // Blank continuation gutter for wrapped sub-rows.
            for x in 0..layout.gutter {
                screen.set(x, vr.y, Cell { ch: ' ', fg: theme.line_number, bg: theme.background, bold: false, italic: false });
            }
        }

        for (k, col) in (vr.start..vr.end).enumerate() {
            let x = layout.text_x() + k as u16;
            if x >= app.width {
                break;
            }
            let idx = line_start + col;
            let ch = chars[col];

            let mut fg = theme.color_for(kinds[col]);
            let mut bg = theme.background;
            let mut bold = bolds[col];

            let is_ws = ch == ' ' || ch == '\t';
            let disp = match ch {
                '\t' if settings.show_whitespace => '→',
                ' ' if settings.show_whitespace => '·',
                c if c.is_control() => ' ',
                c => c,
            };
            if settings.show_whitespace && is_ws {
                fg = theme.whitespace;
            }
            if let Some((a, b)) = bracket_pair {
                if idx == a || idx == b {
                    bg = BRACKET_BG;
                    bold = true;
                }
            }
            if is_in_match(idx, &line_matches) {
                bg = SEARCH_BG;
            }
            if buf.cursors.iter().any(|c| {
                let (s, e) = c.range();
                s != e && idx >= s && idx < e
            }) {
                bg = theme.selection;
            }

            screen.set(x, vr.y, Cell { ch: disp, fg, bg, bold, italic: itals[col] });
        }
    }

    // Tilde markers for screen rows past end of buffer.
    for row in (visrows.len() as u16)..layout.text_rows {
        if layout.gutter > 0 {
            screen.put_str(0, row, "~", theme.line_number, theme.background, false, false);
        }
    }

    // --- cursors (drawn over text) ----------------------------------------
    let mut primary_screen_pos = None;
    for (i, c) in buf.cursors.iter().enumerate() {
        let head = c.head.min(buf.len_chars());
        let cl = buf.rope.char_to_line(head);
        let ls = buf.rope.line_to_char(cl);
        let off = head - ls;
        // Locate the visual row containing this cursor.
        let ri = visrows
            .iter()
            .position(|vr| vr.line == cl && off >= vr.start && off < vr.end)
            .or_else(|| visrows.iter().position(|vr| vr.line == cl && off == vr.end));
        let vr = match ri {
            Some(r) => &visrows[r],
            None => continue,
        };
        let k = off - vr.start;
        if k >= text_width {
            continue;
        }
        let x = layout.text_x() + k as u16;
        let y = vr.y;
        if i == buf.primary {
            primary_screen_pos = Some((x, y));
        } else {
            let llen = buf.line_len_chars(cl);
            let ch = if off < llen { buf.rope.char(ls + off) } else { ' ' };
            screen.set(x, y, Cell { ch, fg: theme.background, bg: theme.cursor, bold: false, italic: false });
        }
    }

    // --- combined status / command / message line -------------------------
    let command_cursor = if app.command.active {
        draw_prompt(screen, app, &layout)
    } else {
        draw_status_bar(screen, app, &layout);
        None
    };

    // --- overlays (drawn on top of everything) ----------------------------
    let overlay_cursor = if app.palette.open {
        super::palette::render(screen, app)
    } else if app.file_picker.open {
        super::file_picker::render(screen, app)
    } else {
        None
    };

    // --- final cursor position --------------------------------------------
    screen.cursor = if app.palette.open || app.file_picker.open {
        overlay_cursor
    } else if app.command.active {
        command_cursor
    } else {
        primary_screen_pos
    };

    screen.flush(out)
}

fn draw_gutter(screen: &mut Screen, layout: &Layout, app: &App, ln: usize, cur_line: usize, y: u16) {
    if layout.gutter == 0 {
        return;
    }
    let settings = &app.config.settings;
    let theme = &app.config.theme;
    let is_current = ln == cur_line;
    let num = if settings.relative_line_numbers && !is_current {
        ln.abs_diff(cur_line)
    } else {
        ln + 1
    };
    let width = (layout.gutter - 1) as usize;
    let s = format!("{num:>width$} ");
    let fg = if is_current { theme.line_number_current } else { theme.line_number };
    screen.put_str(0, y, &s, fg, theme.background, is_current, false);
}

fn draw_status_bar(screen: &mut Screen, app: &App, layout: &Layout) {
    let theme = &app.config.theme;
    let buf = app.active_buffer();
    let y = layout.status_row;
    // Background fill.
    for x in 0..layout.width {
        screen.set(x, y, Cell { ch: ' ', fg: theme.statusbar_fg, bg: theme.statusbar_bg, bold: false, italic: false });
    }

    // Left shows a transient message when present, otherwise the file name.
    let left = if app.status_message.is_empty() {
        statusbar::left_text(buf)
    } else {
        format!(" {}", app.status_message)
    };
    let bold = app.status_message.is_empty();
    screen.put_str(0, y, &left, theme.statusbar_fg, theme.statusbar_bg, bold, false);

    // Plugin status segments.
    let c = buf.primary_cursor();
    let (cl, cc) = buf.pos_to_line_col(c.head);
    let view = PluginView {
        rope: &buf.rope,
        cursor_line: cl,
        cursor_col: cc,
        selection: if c.has_selection() { Some(c.range()) } else { None },
        language: buf.language.display_name(),
        path: buf.path.as_deref(),
    };
    let segments = app.plugins.status_segments(&view);
    let right = statusbar::right_text(buf, &segments, app.active, app.buffers.len());

    let right_len = right.chars().count() as u16;
    if right_len < layout.width {
        let start_x = layout.width - right_len;
        screen.put_str(start_x, y, &right, theme.statusbar_fg, theme.statusbar_bg, false, false);
    }
}

/// Draw an active prompt (find/replace/save-as) on the bottom row, returning the
/// terminal cursor position.
fn draw_prompt(screen: &mut Screen, app: &App, layout: &Layout) -> Option<(u16, u16)> {
    let theme = &app.config.theme;
    let y = layout.status_row;
    for x in 0..layout.width {
        screen.set(x, y, Cell { ch: ' ', fg: theme.foreground, bg: theme.background, bold: false, italic: false });
    }
    let label = app.command.label();
    let x = screen.put_str(0, y, label, theme.line_number_current, theme.background, false, false);
    let end = screen.put_str(x, y, &app.command.input, theme.foreground, theme.background, false, false);
    Some((end.min(layout.width.saturating_sub(1)), y))
}

fn is_in_match(idx: usize, matches: &[(usize, usize)]) -> bool {
    matches.iter().any(|(s, e)| idx >= *s && idx < *e)
}
