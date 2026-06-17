//! Draws the full editor frame into the [`Screen`] back buffer. Only the
//! viewport's lines are processed (and over-long lines skip highlighting), so
//! rendering cost is bound to what's on screen rather than file size.

use super::screen::{Cell, Screen};
use super::{layout::Layout, statusbar, wrap};
use crate::app::App;
use crate::plugins::PluginView;
use crate::editor::Buffer;
use crate::syntax::{highlighter_for, Language, LineState, StyleKind};
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

    // The callout block the cursor is in (rendered raw for editing); other
    // callouts render in a decorated preview form.
    let preview_callouts = settings.render_callouts && buf.language == Language::Markdown;
    let cursor_callout = if preview_callouts { cursor_callout_block(buf, cur_line) } else { None };

    // Dynamic-document `doe:output` regions render as a "computed" card when the
    // cursor is elsewhere (markers concealed); raw when the cursor is inside.
    let preview_outputs = preview_callouts;
    let cursor_output = if preview_outputs { output_region_at(buf, cur_line) } else { None };

    let highlighter = highlighter_for(buf);
    // Seed fence state from the lines above the viewport so a code block whose
    // opening fence has scrolled off the top still highlights correctly.
    let mut state = LineState { in_code_block: app.top_in_code_block, ..Default::default() };
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
    let mut loaded_preview: Option<PreviewRole> = None;
    let mut loaded_output: Option<OutputRole> = None;
    let mut prev_glyphs: Vec<crate::syntax::markdown::InlineGlyph> = Vec::new();
    let mut prev_prefix: usize = 0;
    let mut sub = 0usize;

    for vr in &visrows {
        let new_line = loaded != Some(vr.line);
        if new_line {
            sub = 0;
            line_start = buf.rope.line_to_char(vr.line);
            let line_len = buf.line_len_chars(vr.line);
            // Decide whether this line renders as part of a callout preview.
            loaded_preview = if preview_callouts {
                preview_role(buf, vr.line, cur_line, cursor_callout, text_width)
            } else {
                None
            };
            loaded_output = if preview_outputs {
                output_role(buf, vr.line, cur_line, cursor_output)
            } else {
                None
            };
            // Render a callout header/body's inline content. Each glyph keeps its
            // source index, so the renderer can pick the glyphs that fall in each
            // raw wrap segment — giving exactly one card row per raw row (soft
            // wrap works, markup is concealed, and there is no row mismatch).
            prev_glyphs = Vec::new();
            prev_prefix = 0;
            if let Some(PreviewRole::Header(_) | PreviewRole::Body(_)) = &loaded_preview {
                let ltext = line_text(buf, vr.line);
                let after = ltext.trim_start().strip_prefix('>').unwrap_or("").trim_start();
                let content = if matches!(loaded_preview, Some(PreviewRole::Header(_))) {
                    after.splitn(2, ']').nth(1).unwrap_or("").trim_start()
                } else {
                    after
                };
                prev_glyphs = crate::syntax::markdown::rendered_inline(content);
                // Raw chars stripped before the content (the `> [!type] ` prefix).
                prev_prefix = line_len - content.chars().count();
            }
            let text: String = buf.rope.slice(line_start..line_start + line_len).to_string();
            chars = text.chars().collect();
            kinds = vec![StyleKind::Default; line_len];
            bolds = vec![false; line_len];
            itals = vec![false; line_len];
            if settings.syntax_highlighting && line_len <= MAX_HIGHLIGHT_LINE {
                for sp in highlighter.highlight_line(vr.line, &text, &mut state) {
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
        } else {
            sub += 1;
        }

        if vr.gutter {
            draw_gutter(screen, &layout, app, vr.line, cur_line, vr.y);
        } else {
            // Blank continuation gutter for wrapped sub-rows.
            for x in 0..layout.gutter {
                screen.set(x, vr.y, Cell { ch: ' ', fg: theme.line_number, bg: theme.background, bold: false, italic: false });
            }
        }

        // Callout preview: draw a decorated card row and skip the raw cells.
        if let Some(role) = &loaded_preview {
            draw_callout_card_row(screen, &layout, app, vr.y, role, sub == 0, &prev_glyphs, prev_prefix, vr.start, vr.end);
            continue;
        }

        // Dynamic-document output preview: draw the "computed" card row.
        if let Some(role) = loaded_output {
            draw_output_row(screen, &layout, app, vr.y, role, &chars, vr.start, vr.end);
            continue;
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
    let overlay_cursor = if app.modal_open {
        super::modal::render(screen, app)
    } else if app.settings_panel.open {
        super::settings::render(screen, app);
        None
    } else if app.callout_panel.open {
        super::callouts::render(screen, app);
        None
    } else {
        None
    };

    // --- final cursor position --------------------------------------------
    screen.cursor = if app.settings_panel.open || app.callout_panel.open {
        None // navigated with arrows; no text caret
    } else if app.modal_open {
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

// --- callout preview ------------------------------------------------------

fn in_block(block: Option<(usize, usize)>, line: usize) -> bool {
    block.is_some_and(|(s, e)| line >= s && line <= e)
}

fn line_text(buf: &Buffer, line: usize) -> String {
    let start = buf.rope.line_to_char(line);
    let len = buf.line_len_chars(line);
    buf.rope.slice(start..start + len).to_string()
}

fn line_is_blockquote(buf: &Buffer, line: usize) -> bool {
    line_text(buf, line).trim_start().starts_with('>')
}

/// The callout type of a `> [!type] …` header line, lowercased, if any.
fn callout_type(buf: &Buffer, line: usize) -> Option<String> {
    let text = line_text(buf, line);
    let rest = text.trim_start().strip_prefix('>')?.trim_start();
    let inner = rest.strip_prefix("[!")?;
    let close = inner.find(']')?;
    let ty = &inner[..close];
    if ty.is_empty() {
        None
    } else {
        Some(ty.to_lowercase())
    }
}

fn line_is_blank(buf: &Buffer, line: usize) -> bool {
    line_text(buf, line).trim().is_empty()
}

/// `(is_header, type)` if `line` is part of a callout block, else `None`.
fn callout_role(buf: &Buffer, line: usize) -> Option<(bool, String)> {
    if !line_is_blockquote(buf, line) {
        return None;
    }
    let mut start = line;
    while start > 0 && line_is_blockquote(buf, start - 1) {
        start -= 1;
    }
    let ty = callout_type(buf, start)?;
    Some((line == start, ty))
}

/// What a line renders as in callout-preview mode.
enum PreviewRole {
    /// Top border of a callout box (a blank line above the callout).
    Top(String),
    /// Bottom border (a blank line below the callout).
    Bottom(String),
    /// Header line — shows the title.
    Header(String),
    /// Body line.
    Body(String),
}

/// Decide how `line` renders in preview mode (or `None` for raw).
fn preview_role(
    buf: &Buffer,
    line: usize,
    cur_line: usize,
    cursor_callout: Option<(usize, usize)>,
    text_width: usize,
) -> Option<PreviewRole> {
    // The line under the cursor is always raw; so is the cursor's own callout.
    if line == cur_line || text_width < 8 {
        return None;
    }
    if let Some((is_header, ty)) = callout_role(buf, line) {
        if in_block(cursor_callout, line) {
            return None;
        }
        return Some(if is_header { PreviewRole::Header(ty) } else { PreviewRole::Body(ty) });
    }
    // A blank line adjacent to a callout becomes a box border (keeps geometry).
    if line_is_blank(buf, line) {
        if line + 1 < buf.len_lines() && !in_block(cursor_callout, line + 1) {
            if let Some((true, ty)) = callout_role(buf, line + 1) {
                return Some(PreviewRole::Top(ty));
            }
        }
        if line > 0 && !in_block(cursor_callout, line - 1) {
            if let Some((_, ty)) = callout_role(buf, line - 1) {
                return Some(PreviewRole::Bottom(ty));
            }
        }
    }
    None
}

/// The contiguous callout block containing `cur_line`, if the cursor is in one.
fn cursor_callout_block(buf: &Buffer, cur_line: usize) -> Option<(usize, usize)> {
    if !line_is_blockquote(buf, cur_line) {
        return None;
    }
    let mut start = cur_line;
    while start > 0 && line_is_blockquote(buf, start - 1) {
        start -= 1;
    }
    callout_type(buf, start)?;
    let mut end = cur_line;
    let last = buf.len_lines();
    while end + 1 < last && line_is_blockquote(buf, end + 1) {
        end += 1;
    }
    Some((start, end))
}

// --- dynamic-document output preview --------------------------------------

/// How an output-region line renders in preview mode.
#[derive(Debug, Clone, Copy)]
enum OutputRole {
    /// The `<!-- doe:output -->` marker line — top border.
    Open,
    /// A generated body line — card row.
    Body,
    /// The `<!-- /doe:output -->` marker line — bottom border.
    Close,
}

fn line_is_output_open(buf: &Buffer, line: usize) -> bool {
    let t = line_text(buf, line);
    let t = t.trim();
    t.starts_with("<!--")
        && t.ends_with("-->")
        && t.trim_start_matches("<!--").trim_start().starts_with("doe:output")
}

fn line_is_output_close(buf: &Buffer, line: usize) -> bool {
    line_text(buf, line).trim() == "<!-- /doe:output -->"
}

/// The output region (inclusive marker-line span) containing `line`, if any.
fn output_region_at(buf: &Buffer, line: usize) -> Option<(usize, usize)> {
    // Walk up to the opening marker, bailing if a closing marker comes first.
    let mut start = line;
    loop {
        if line_is_output_open(buf, start) {
            break;
        }
        if start != line && line_is_output_close(buf, start) {
            return None;
        }
        if start == 0 {
            return None;
        }
        start -= 1;
    }
    // Walk down to the closing marker.
    let mut end = start + 1;
    let n = buf.len_lines();
    while end < n && !line_is_output_close(buf, end) {
        if line_is_output_open(buf, end) {
            return None; // malformed: nested open
        }
        end += 1;
    }
    if end < n && line <= end {
        Some((start, end))
    } else {
        None
    }
}

/// What `line` renders as in output-preview mode (or `None` for raw — the line
/// under the cursor and the cursor's own region stay raw).
fn output_role(buf: &Buffer, line: usize, cur_line: usize, cursor_output: Option<(usize, usize)>) -> Option<OutputRole> {
    if line == cur_line {
        return None;
    }
    let (s, e) = output_region_at(buf, line)?;
    if let Some((cs, ce)) = cursor_output {
        if line >= cs && line <= ce {
            return None;
        }
    }
    Some(if line == s {
        OutputRole::Open
    } else if line == e {
        OutputRole::Close
    } else {
        OutputRole::Body
    })
}

/// Draw one row of a "computed" output card: tinted background, dim borders, the
/// generated text in italic, with the HTML-comment markers concealed.
fn draw_output_row(screen: &mut Screen, layout: &Layout, app: &App, y: u16, role: OutputRole, chars: &[char], seg_start: usize, seg_end: usize) {
    let theme = &app.config.theme;
    let accent = theme.comment;
    let card_bg = tint(theme.background, accent, 0.10);
    let x0 = layout.text_x();
    let right = app.width.saturating_sub(1);

    for x in x0..app.width {
        screen.set(x, y, Cell { ch: ' ', fg: theme.foreground, bg: card_bg, bold: false, italic: false });
    }
    let dim = |ch: char| Cell { ch, fg: accent, bg: card_bg, bold: false, italic: false };

    match role {
        OutputRole::Open | OutputRole::Close => {
            let (l, r) = if matches!(role, OutputRole::Open) { ('╭', '╮') } else { ('╰', '╯') };
            screen.set(x0, y, dim(l));
            for x in (x0 + 1)..right {
                screen.set(x, y, dim('─'));
            }
            if right > x0 {
                screen.set(right, y, dim(r));
            }
            if matches!(role, OutputRole::Open) {
                screen.put_str(x0 + 2, y, " computed ", accent, card_bg, false, true);
            }
        }
        OutputRole::Body => {
            screen.set(x0, y, dim('│'));
            for (k, col) in (seg_start..seg_end).enumerate() {
                let x = x0 + 2 + k as u16;
                if x >= right {
                    break;
                }
                let ch = chars.get(col).copied().unwrap_or(' ');
                screen.set(x, y, Cell { ch, fg: theme.foreground, bg: card_bg, bold: false, italic: true });
            }
            if right > x0 {
                screen.set(right, y, dim('│'));
            }
        }
    }
}

fn lerp(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8
}

/// Blend `accent` over `bg` by `t` for a subtle card tint.
fn tint(bg: Color, accent: Color, t: f32) -> Color {
    match (bg, accent) {
        (Color::Rgb { r: br, g: bgr, b: bb }, Color::Rgb { r: ar, g: ag, b: ab }) => Color::Rgb {
            r: lerp(br, ar, t),
            g: lerp(bgr, ag, t),
            b: lerp(bb, ab, t),
        },
        _ => bg,
    }
}

/// Draw one card row: tinted background, accent side borders, and — for
/// header/body — the rendered glyphs whose source falls in this raw wrap segment
/// `[seg_start, seg_end)` (so there is exactly one card row per raw row, soft
/// wrap works, and markup is concealed). The header label appears on its first
/// row. Top/bottom borders reuse the blank lines around the callout.
#[allow(clippy::too_many_arguments)]
fn draw_callout_card_row(
    screen: &mut Screen,
    layout: &Layout,
    app: &App,
    y: u16,
    role: &PreviewRole,
    first_row: bool,
    glyphs: &[crate::syntax::markdown::InlineGlyph],
    prefix: usize,
    seg_start: usize,
    seg_end: usize,
) {
    let theme = &app.config.theme;
    let ty = match role {
        PreviewRole::Top(t) | PreviewRole::Bottom(t) | PreviewRole::Header(t) | PreviewRole::Body(t) => t,
    };
    let (accent, icon) = app.config.callouts.style(ty);
    let card_bg = tint(theme.background, accent, 0.14);
    let x0 = layout.text_x();
    let right = app.width.saturating_sub(1);
    let w = (app.width - x0) as usize;

    for x in x0..app.width {
        screen.set(x, y, Cell { ch: ' ', fg: theme.foreground, bg: card_bg, bold: false, italic: false });
    }

    match role {
        PreviewRole::Top(_) | PreviewRole::Bottom(_) => {
            let (l, r) = if matches!(role, PreviewRole::Top(_)) { ('╭', '╮') } else { ('╰', '╯') };
            let mut s = l.to_string();
            while s.chars().count() + 1 < w {
                s.push('─');
            }
            s.push(r);
            screen.put_str(x0, y, &s, accent, card_bg, false, false);
        }
        PreviewRole::Header(_) | PreviewRole::Body(_) => {
            screen.set(x0, y, Cell { ch: '│', fg: accent, bg: card_bg, bold: false, italic: false });
            screen.set(right, y, Cell { ch: '│', fg: accent, bg: card_bg, bold: false, italic: false });
            let header = matches!(role, PreviewRole::Header(_));
            let mut cx = x0 + 2;
            if header && first_row {
                cx = screen.put_str(cx, y, &icon.to_string(), accent, card_bg, true, false);
                cx = screen.put_str(cx, y, " ", accent, card_bg, false, false);
                cx = screen.put_str(cx, y, &ty.to_uppercase(), accent, card_bg, true, false);
                cx = screen.put_str(cx, y, "  ", accent, card_bg, false, false);
            }
            // Glyphs whose raw position (index + prefix) lies in this segment.
            let seg: Vec<&crate::syntax::markdown::InlineGlyph> = glyphs
                .iter()
                .filter(|g| {
                    let raw = g.index + prefix;
                    raw >= seg_start && raw < seg_end
                })
                .collect();
            let mut x = cx;
            for g in seg {
                if x >= right {
                    break;
                }
                let (fg, bold) = if g.kind == StyleKind::Default {
                    (theme.foreground, header)
                } else {
                    (theme.color_for(g.kind), g.bold)
                };
                screen.set(x, y, Cell { ch: g.ch, fg, bg: card_bg, bold, italic: g.italic });
                x += 1;
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::Buffer;

    fn buf(s: &str) -> Buffer {
        let mut b = Buffer::empty();
        b.set_text(s);
        b
    }

    #[test]
    fn detects_output_region() {
        let b = buf("```lua run\nreturn 1\n```\n<!-- doe:output -->\n42\n<!-- /doe:output -->\nafter\n");
        // Region spans the marker lines 3..5 inclusive.
        assert_eq!(output_region_at(&b, 3), Some((3, 5)));
        assert_eq!(output_region_at(&b, 4), Some((3, 5)));
        assert_eq!(output_region_at(&b, 5), Some((3, 5)));
        // Lines outside the region resolve to None.
        assert_eq!(output_region_at(&b, 6), None);
        assert_eq!(output_region_at(&b, 1), None);
    }

    #[test]
    fn roles_and_cursor_makes_region_raw() {
        let b = buf("a\n<!-- doe:output -->\n42\n<!-- /doe:output -->\n");
        // Cursor outside (line 0): the region renders decorated.
        assert!(matches!(output_role(&b, 1, 0, None), Some(OutputRole::Open)));
        assert!(matches!(output_role(&b, 2, 0, None), Some(OutputRole::Body)));
        assert!(matches!(output_role(&b, 3, 0, None), Some(OutputRole::Close)));
        // Cursor inside the region: every line stays raw.
        let cur = output_region_at(&b, 2);
        assert!(output_role(&b, 1, 2, cur).is_none());
        assert!(output_role(&b, 2, 2, cur).is_none());
        assert!(output_role(&b, 3, 2, cur).is_none());
    }
}
