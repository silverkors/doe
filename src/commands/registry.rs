//! Maps textual command names (used in config files, the command line, and by
//! plugins) to [`Command`] values. Keeping the parsing in one place means the
//! keymap, command line and plugin API all speak the same language.

use super::Command;
use std::path::PathBuf;

/// Parse a command name (optionally with arguments) into a [`Command`].
///
/// Examples: `"save"`, `"add_cursor_below"`, `"open foo.md"`,
/// `"replace from to"`.
pub fn parse(input: &str) -> Option<Command> {
    let input = input.trim();
    let (name, rest) = match input.split_once(char::is_whitespace) {
        Some((n, r)) => (n, r.trim()),
        None => (input, ""),
    };

    let cmd = match name {
        "save" | "w" => Command::Save,
        "save_as" | "saveas" => Command::SaveAs(PathBuf::from(rest)),
        "open" | "e" | "edit" => Command::OpenFile(PathBuf::from(rest)),
        "quit" | "q" => Command::Quit,
        "force_quit" | "q!" => Command::ForceQuit,
        "save_quit" | "wq" => Command::SaveAndQuit,

        "undo" => Command::Undo,
        "redo" => Command::Redo,
        "tab" => Command::Tab,
        "toggle_comment" => Command::ToggleComment,
        "newline" => Command::InsertNewline,
        "backspace" => Command::Backspace,
        "delete" => Command::Delete,

        "toggle_bold" => Command::ToggleBold,
        "toggle_italic" => Command::ToggleItalic,

        "move_left" => Command::MoveLeft,
        "move_right" => Command::MoveRight,
        "move_up" => Command::MoveUp,
        "move_down" => Command::MoveDown,
        "move_word_left" => Command::MoveWordLeft,
        "move_word_right" => Command::MoveWordRight,
        "move_line_start" => Command::MoveLineStart,
        "move_line_end" => Command::MoveLineEnd,
        "move_buffer_start" => Command::MoveBufferStart,
        "move_buffer_end" => Command::MoveBufferEnd,
        "page_up" => Command::PageUp,
        "page_down" => Command::PageDown,

        "extend_left" => Command::ExtendLeft,
        "extend_right" => Command::ExtendRight,
        "extend_up" => Command::ExtendUp,
        "extend_down" => Command::ExtendDown,
        "select_all" => Command::SelectAll,
        "select_line" => Command::SelectLine,
        "collapse_selection" => Command::CollapseSelection,

        "add_cursor_above" => Command::AddCursorAbove,
        "add_cursor_below" => Command::AddCursorBelow,
        "add_cursor_next_match" => Command::AddCursorNextMatch,
        "select_all_matches" => Command::SelectAllMatches,
        "clear_extra_cursors" => Command::ClearExtraCursors,

        "find" => Command::Find,
        "find_next" => Command::FindNext,
        "find_prev" => Command::FindPrev,
        // Bare `replace`/`replace_all` (no args) open the interactive prompt;
        // with args they run directly. Empty `from` signals "open prompt".
        "replace" => match rest.split_once(' ') {
            Some((from, to)) => Command::Replace { from: from.to_string(), to: to.to_string() },
            None => Command::Replace { from: String::new(), to: String::new() },
        },
        "replace_all" => match rest.split_once(' ') {
            Some((from, to)) => Command::ReplaceAll { from: from.to_string(), to: to.to_string() },
            None => Command::ReplaceAll { from: String::new(), to: String::new() },
        },

        "next_buffer" => Command::NextBuffer,
        "prev_buffer" => Command::PrevBuffer,
        "close_buffer" => Command::CloseBuffer,
        "goto_buffer" => Command::GotoBuffer(rest.parse::<usize>().ok()?.saturating_sub(1)),

        "command_palette" => Command::CommandPalette,
        "open_buffers" | "buffers" => Command::OpenBuffers,
        "toggle_soft_wrap" => Command::ToggleSoftWrap,
        "settings" | "preferences" => Command::Settings,
        "callout_settings" | "callouts" => Command::CalloutSettings,
        "import_callouts" | "import_obsidian_callouts" => Command::ImportObsidianCallouts,

        _ => return None,
    };
    Some(cmd)
}
