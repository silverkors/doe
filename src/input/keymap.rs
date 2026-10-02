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

    // For a plain character the case already encodes Shift, so `shift-` is
    // only emitted alongside Ctrl/Alt (`ctrl-shift-o`). Terminals report that
    // combo as either `O`+CONTROL or `o`+CONTROL|SHIFT; both map the same.
    // Named keys (arrows, F-keys, …) always carry `shift-`.
    let (name, allow_shift) = match ev.code {
        KeyCode::Char(c) => {
            let s = match c {
                ' ' => "space".to_string(),
                ':' => "colon".to_string(),
                '/' => "slash".to_string(),
                c => c.to_ascii_lowercase().to_string(),
            };
            let shifted = (ctrl || alt) && (shift || c.is_ascii_uppercase());
            return Some(build(ctrl, alt, shifted, &s));
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

    Some(build(ctrl, alt, shift && allow_shift, &name))
}

fn build(ctrl: bool, alt: bool, shift: bool, name: &str) -> String {
    let mut out = String::new();
    if ctrl {
        out.push_str("ctrl-");
    }
    if alt {
        out.push_str("alt-");
    }
    if shift {
        out.push_str("shift-");
    }
    out.push_str(name);
    out
}

/// The chord to try when `chord_string` has no binding: a Ctrl/Alt+Shift+letter
/// falls back to its unshifted form, so an unbound `ctrl-shift-z` still acts
/// like `ctrl-z` (as it did before Shift was distinguished on letters).
pub fn fallback_chord(ev: &KeyEvent) -> Option<String> {
    let chord = chord_string(ev)?;
    match ev.code {
        KeyCode::Char(_) if chord.contains("shift-") => Some(chord.replacen("shift-", "", 1)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(code: KeyCode, m: KeyModifiers) -> Option<String> {
        chord_string(&KeyEvent::new(code, m))
    }

    #[test]
    fn ctrl_shift_letter_keeps_shift() {
        let cs = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        assert_eq!(chord(KeyCode::Char('O'), cs).as_deref(), Some("ctrl-shift-o"));
        assert_eq!(chord(KeyCode::Char('o'), cs).as_deref(), Some("ctrl-shift-o"));
        assert_eq!(chord(KeyCode::Char('O'), KeyModifiers::CONTROL).as_deref(), Some("ctrl-shift-o"));
        assert_eq!(chord(KeyCode::Char('o'), KeyModifiers::CONTROL).as_deref(), Some("ctrl-o"));
    }

    #[test]
    fn plain_shifted_letter_has_no_shift_prefix() {
        assert_eq!(chord(KeyCode::Char('A'), KeyModifiers::SHIFT).as_deref(), Some("a"));
        assert_eq!(chord(KeyCode::Up, KeyModifiers::SHIFT).as_deref(), Some("shift-up"));
    }

    #[test]
    fn fallback_drops_shift_only_for_letters() {
        let ev = KeyEvent::new(KeyCode::Char('Z'), KeyModifiers::CONTROL | KeyModifiers::SHIFT);
        assert_eq!(fallback_chord(&ev).as_deref(), Some("ctrl-z"));
        let ev = KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT);
        assert_eq!(fallback_chord(&ev), None);
    }
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
