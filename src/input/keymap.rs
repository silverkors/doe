//! Translates crossterm key events into canonical chord strings that match the
//! config syntax (e.g. `"ctrl-s"`, `"alt-up"`, `"shift-f3"`, `"colon"`). The
//! keymap itself lives in [`crate::config::Keybindings`]; this module is just
//! the normalizer so config files and code agree on names.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Build the canonical chord string for a key event, or `None` for keys we
/// don't bind (e.g. raw modifier presses).
pub fn chord_string(ev: &KeyEvent) -> Option<String> {
    let m = ev.modifiers;
    let ctrl = m.contains(KeyModifiers::CONTROL);
    let alt = m.contains(KeyModifiers::ALT);
    let shift = m.contains(KeyModifiers::SHIFT);

    // `allow_shift` is false for character keys because case is already encoded
    // in the char itself; it is true for named keys (arrows, F-keys, …).
    let (name, allow_shift) = match ev.code {
        KeyCode::Char(c) => {
            let s = match c {
                ' ' => "space".to_string(),
                ':' => "colon".to_string(),
                '/' => "slash".to_string(),
                c => c.to_ascii_lowercase().to_string(),
            };
            (s, false)
        }
        KeyCode::Enter => ("enter".to_string(), true),
        KeyCode::Esc => ("esc".to_string(), true),
        KeyCode::Backspace => ("backspace".to_string(), true),
        KeyCode::Delete => ("delete".to_string(), true),
        KeyCode::Tab => ("tab".to_string(), true),
        KeyCode::BackTab => ("backtab".to_string(), true),
        KeyCode::Left => ("left".to_string(), true),
        KeyCode::Right => ("right".to_string(), true),
        KeyCode::Up => ("up".to_string(), true),
        KeyCode::Down => ("down".to_string(), true),
        KeyCode::Home => ("home".to_string(), true),
        KeyCode::End => ("end".to_string(), true),
        KeyCode::PageUp => ("pageup".to_string(), true),
        KeyCode::PageDown => ("pagedown".to_string(), true),
        KeyCode::Insert => ("insert".to_string(), true),
        KeyCode::F(n) => (format!("f{n}"), true),
        _ => return None,
    };

    let mut out = String::new();
    if ctrl {
        out.push_str("ctrl-");
    }
    if alt {
        out.push_str("alt-");
    }
    if shift && allow_shift {
        out.push_str("shift-");
    }
    out.push_str(&name);
    Some(out)
}

/// If this event is a plain printable character (no ctrl/alt), return it for
/// direct insertion in insert mode.
pub fn printable_char(ev: &KeyEvent) -> Option<char> {
    if ev.modifiers.contains(KeyModifiers::CONTROL) || ev.modifiers.contains(KeyModifiers::ALT) {
        return None;
    }
    match ev.code {
        KeyCode::Char(c) => Some(c),
        _ => None,
    }
}
