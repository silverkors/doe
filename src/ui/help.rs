//! Keyboard-shortcut overview — a scrollable modal listing every binding,
//! grouped by area. The chords are read from the *active* keybindings (built-in
//! defaults merged with the user's `config.toml` overrides), so what you see is
//! what your keyboard actually does. Open from the palette ("Keyboard
//! Shortcuts…"), with F1, or via `:help`.

use super::screen::{Cell, Screen};
use crate::app::App;

#[derive(Default)]
pub struct HelpPanel {
    pub open: bool,
    pub scroll: usize,
}

impl HelpPanel {
    pub fn open(&mut self) {
        self.open = true;
        self.scroll = 0;
    }
    pub fn close(&mut self) {
        self.open = false;
    }
    pub fn scroll_by(&mut self, delta: isize, page: usize, total: usize) {
        let max = total.saturating_sub(page);
        let s = self.scroll as isize + delta;
        self.scroll = (s.max(0) as usize).min(max);
    }
}

/// The catalog: sections of `(command name, description)`. Chords are resolved
/// against the live keybindings at render time; commands the user has unbound
/// are skipped automatically.
const SECTIONS: &[(&str, &[(&str, &str)])] = &[
    ("Files & buffers", &[
        ("save", "Save file"),
        ("open", "Open file…"),
        ("close_buffer", "Close buffer"),
        ("next_buffer", "Next buffer"),
        ("prev_buffer", "Previous buffer"),
        ("quit", "Quit"),
    ]),
    ("Editing", &[
        ("undo", "Undo"),
        ("redo", "Redo"),
        ("backspace", "Delete previous char"),
        ("delete", "Delete next char"),
        ("delete_word_left", "Delete previous word"),
        ("delete_word_right", "Delete next word"),
        ("delete_line", "Delete line"),
        ("duplicate_line", "Duplicate line"),
        ("toggle_comment", "Toggle line comment"),
        ("toggle_bold", "Markdown: bold"),
        ("toggle_italic", "Markdown: italic"),
        ("tab", "Insert tab (to next tab stop)"),
    ]),
    ("Movement", &[
        ("move_word_left", "Word left"),
        ("move_word_right", "Word right"),
        ("move_line_start", "Start of line (smart)"),
        ("move_line_end", "End of line"),
        ("move_buffer_start", "Start of file"),
        ("move_buffer_end", "End of file"),
        ("page_up", "Page up"),
        ("page_down", "Page down"),
    ]),
    ("Selection & multi-cursor", &[
        ("select_all", "Select all"),
        ("select_line", "Select line"),
        ("add_cursor_next_match", "Select word / add next occurrence"),
        ("select_all_matches", "Select all occurrences"),
        ("add_cursor_above", "Add cursor above"),
        ("add_cursor_below", "Add cursor below"),
        ("expand_selection", "Expand selection (syntax)"),
        ("shrink_selection", "Shrink selection"),
        ("clear_extra_cursors", "Clear extra cursors / dismiss"),
    ]),
    ("Search", &[
        ("find", "Find…"),
        ("find_next", "Find next"),
        ("find_prev", "Find previous"),
        ("replace_all", "Replace all…"),
    ]),
    ("Panels & view", &[
        ("command_palette", "Command palette"),
        ("open_buffers", "Buffer list"),
        ("go_to_symbol", "Go to symbol…"),
        ("settings", "Settings"),
        ("toggle_soft_wrap", "Toggle soft wrap"),
        ("toggle_tab_ruler", "Toggle tab-stop ruler"),
        ("help", "This overview"),
    ]),
    ("Dynamic documents", &[
        ("run_code_block", "Run code block"),
    ]),
    ("AI", &[
        ("ai", "Prompt AI (reply streams into the buffer)"),
        ("ai_providers", "Configure AI providers"),
    ]),
];

/// One display row: either a section header or a `keys · description` line.
pub enum Row {
    Section(&'static str),
    Bind { keys: String, desc: &'static str },
}

/// Resolve the rows against the active keybindings (user overrides included).
pub fn rows(app: &App) -> Vec<Row> {
    let binds = app
        .config
        .keybindings
        .modes
        .get(crate::commands::BINDING_CONTEXT)
        .cloned()
        .unwrap_or_default();
    // Invert chord -> command into command -> [chords].
    let mut by_cmd: std::collections::HashMap<&str, Vec<&str>> = std::collections::HashMap::new();
    for (chord, cmd) in &binds {
        by_cmd.entry(cmd.as_str()).or_default().push(chord.as_str());
    }
    for chords in by_cmd.values_mut() {
        chords.sort();
    }

    let mut out = Vec::new();
    for (title, entries) in SECTIONS {
        let mut section: Vec<Row> = Vec::new();
        for (cmd, desc) in *entries {
            if let Some(chords) = by_cmd.get(cmd) {
                let keys = chords.iter().map(|c| pretty_chord(c)).collect::<Vec<_>>().join("  /  ");
                section.push(Row::Bind { keys, desc });
            }
        }
        if !section.is_empty() {
            out.push(Row::Section(title));
            out.append(&mut section);
        }
    }
    out
}

/// `"ctrl-shift-o"` → `"Ctrl+Shift+O"`, with arrows and paging compacted.
fn pretty_chord(chord: &str) -> String {
    chord
        .split('-')
        .map(|part| match part {
            "ctrl" => "Ctrl".to_string(),
            "alt" => "Alt".to_string(),
            "shift" => "Shift".to_string(),
            "up" => "↑".to_string(),
            "down" => "↓".to_string(),
            "left" => "←".to_string(),
            "right" => "→".to_string(),
            "pageup" => "PgUp".to_string(),
            "pagedown" => "PgDn".to_string(),
            "backspace" => "Backspace".to_string(),
            "delete" => "Delete".to_string(),
            "space" => "Space".to_string(),
            "colon" => ":".to_string(),
            "slash" => "/".to_string(),
            p if p.len() == 1 => p.to_uppercase(),
            p => {
                // Capitalise names like "esc", "enter", "home", "f3".
                let mut c = p.chars();
                match c.next() {
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                    None => String::new(),
                }
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// Visible list rows for the current terminal height (shared by render + keys).
pub fn page_rows(app: &App) -> usize {
    (app.height as usize).saturating_sub(8).max(3)
}

pub fn render(screen: &mut Screen, app: &App) {
    if app.width < 40 || app.height < 10 {
        return;
    }
    let theme = &app.config.theme;
    let panel_bg = theme.statusbar_bg;
    let border = theme.line_number_current;

    let rows = rows(app);
    let page = page_rows(app);
    let width = app.width.saturating_sub(6).min(64).max(40);
    let x0 = (app.width - width) / 2;
    let y0 = 2u16;
    let inner = (width - 2) as usize;
    let scroll = app.help_panel.scroll.min(rows.len().saturating_sub(page));

    let mut top = String::from("┌ Keyboard Shortcuts ");
    while top.chars().count() < (width - 1) as usize {
        top.push('─');
    }
    top.push('┐');
    fill(screen, x0, y0, width, panel_bg);
    screen.put_str(x0, y0, &top, border, panel_bg, false, false);

    for (i, row) in rows.iter().skip(scroll).take(page).enumerate() {
        let y = y0 + 1 + i as u16;
        fill(screen, x0, y, width, panel_bg);
        screen.put_str(x0, y, "│", border, panel_bg, false, false);
        screen.put_str(x0 + width - 1, y, "│", border, panel_bg, false, false);
        match row {
            Row::Section(title) => {
                screen.put_str(x0 + 2, y, title, theme.heading, panel_bg, true, false);
            }
            Row::Bind { keys, desc } => {
                // Keys right-padded in a fixed column, description after.
                const KEYCOL: usize = 26;
                let keys_trunc: String = keys.chars().take(KEYCOL).collect();
                screen.put_str(x0 + 3, y, &keys_trunc, theme.keyword, panel_bg, false, false);
                let dx = x0 + 3 + KEYCOL as u16 + 1;
                let avail = (x0 + width - 1).saturating_sub(dx) as usize;
                let d: String = desc.chars().take(avail).collect();
                screen.put_str(dx, y, &d, theme.foreground, panel_bg, false, false);
            }
        }
    }

    // Footer with scroll position.
    let fy = y0 + 1 + page as u16;
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
    let more = rows.len().saturating_sub(scroll + page);
    let hint = if more > 0 {
        format!("↑↓/PgUp/PgDn scroll ({more} more) · esc close")
    } else {
        "↑↓/PgUp/PgDn scroll · esc close".to_string()
    };
    screen.put_str(x0 + 2, hy, &hint, theme.comment, panel_bg, false, false);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_commands_are_known() {
        // A typo here would silently drop the row — every entry must parse.
        for (_, entries) in SECTIONS {
            for (cmd, _) in *entries {
                assert!(crate::commands::registry::parse(cmd).is_some(), "{cmd}");
            }
        }
    }

    #[test]
    fn catalog_commands_have_default_bindings_or_palette() {
        // Every listed command should be reachable: bound by default or in the
        // palette (the overview skips unbound rows at render time).
        let kb = crate::config::default_keybindings();
        let global = kb.modes.get(crate::commands::BINDING_CONTEXT).unwrap();
        for (_, entries) in SECTIONS {
            for (cmd, _) in *entries {
                let bound = global.values().any(|c| c == cmd);
                let in_palette = crate::commands::palette::is_action(cmd);
                assert!(bound || in_palette, "{cmd} is unreachable");
            }
        }
    }

    #[test]
    fn chords_prettify() {
        assert_eq!(pretty_chord("ctrl-shift-o"), "Ctrl+Shift+O");
        assert_eq!(pretty_chord("alt-up"), "Alt+↑");
        assert_eq!(pretty_chord("f1"), "F1");
        assert_eq!(pretty_chord("ctrl-backspace"), "Ctrl+Backspace");
    }
}
