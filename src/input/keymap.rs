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
    // The Command key on macOS (Windows/Super key elsewhere). Most macOS
    // terminals intercept ⌘ for their own shortcuts and never deliver it to the
    // app; it only reaches us on terminals with the keyboard-enhancement
    // protocol *and* ⌘ passthrough (Kitty, Ghostty, WezTerm). We still parse it
    // so a `cmd-…` binding works where the terminal cooperates.
    let sup = m.contains(KeyModifiers::SUPER);

    // For a bare character, case is already encoded in the char, so `Shift+a`
    // ("A") needs no `shift-` prefix. But once another modifier is held, the
    // enhancement protocol reports the *base* key (lowercase) plus a distinct
    // SHIFT flag — so `Ctrl+Shift+K` arrives as `Char('k') + CTRL|SHIFT` and we
    // must keep `shift-` to tell it apart from `Ctrl+K`. Named keys always allow
    // shift.
    let (name, allow_shift) = match ev.code {
        KeyCode::Char(c) => {
            let s = match c {
                ' ' => "space".to_string(),
                ':' => "colon".to_string(),
                '/' => "slash".to_string(),
                c => c.to_ascii_lowercase().to_string(),
            };
            (s, ctrl || alt || sup)
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

    // Canonical prefix order: cmd, ctrl, alt, shift. Config strings must match.
    let mut out = String::new();
    if sup {
        out.push_str("cmd-");
    }
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
    if ev.modifiers.contains(KeyModifiers::CONTROL)
        || ev.modifiers.contains(KeyModifiers::ALT)
        || ev.modifiers.contains(KeyModifiers::SUPER)
    {
        return None;
    }
    match ev.code {
        KeyCode::Char(c) => Some(c),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(code: KeyCode, mods: KeyModifiers) -> Option<String> {
        chord_string(&KeyEvent::new(code, mods))
    }

    #[test]
    fn bare_and_shifted_letters_stay_chars() {
        // A plain letter and a shift-uppercased letter both bind as the letter;
        // case/shift is not part of the chord for an unmodified char.
        assert_eq!(chord(KeyCode::Char('a'), KeyModifiers::NONE).as_deref(), Some("a"));
        assert_eq!(chord(KeyCode::Char('A'), KeyModifiers::SHIFT).as_deref(), Some("a"));
    }

    #[test]
    fn ctrl_shift_letter_keeps_shift() {
        // The regression this guards: Ctrl+Shift+K must be distinct from Ctrl+K,
        // regardless of whether the terminal sends 'k' or 'K'.
        assert_eq!(
            chord(KeyCode::Char('k'), KeyModifiers::CONTROL | KeyModifiers::SHIFT).as_deref(),
            Some("ctrl-shift-k")
        );
        assert_eq!(
            chord(KeyCode::Char('K'), KeyModifiers::CONTROL | KeyModifiers::SHIFT).as_deref(),
            Some("ctrl-shift-k")
        );
        assert_eq!(chord(KeyCode::Char('k'), KeyModifiers::CONTROL).as_deref(), Some("ctrl-k"));
    }

    #[test]
    fn cmd_prefix_and_order() {
        // ⌘⇧D → cmd-shift-d; full order is cmd, ctrl, alt, shift.
        assert_eq!(
            chord(KeyCode::Char('d'), KeyModifiers::SUPER | KeyModifiers::SHIFT).as_deref(),
            Some("cmd-shift-d")
        );
        let all = KeyModifiers::SUPER | KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT;
        assert_eq!(chord(KeyCode::Char('x'), all).as_deref(), Some("cmd-ctrl-alt-shift-x"));
    }

    #[test]
    fn named_keys_keep_shift() {
        assert_eq!(chord(KeyCode::Left, KeyModifiers::SHIFT).as_deref(), Some("shift-left"));
        assert_eq!(chord(KeyCode::F(3), KeyModifiers::SHIFT).as_deref(), Some("shift-f3"));
    }
}
