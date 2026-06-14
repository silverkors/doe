//! Central command system. All editor functionality is expressed as a
//! [`Command`]. Keybindings, the command line, mouse actions and (future)
//! plugins all funnel through this single layer.

pub mod registry;

use std::path::PathBuf;

/// Editor modes. The structure is intentionally generic so Vim-like behaviour
/// can be layered on later without reworking the input pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    Normal,
    Insert,
    Select,
    Command,
}

impl Mode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Insert => "INSERT",
            Mode::Select => "SELECT",
            Mode::Command => "COMMAND",
        }
    }

    /// The TOML keybinding section name for this mode.
    pub fn config_key(&self) -> &'static str {
        match self {
            Mode::Normal => "normal",
            Mode::Insert => "insert",
            Mode::Select => "select",
            Mode::Command => "command",
        }
    }
}

/// Every action the editor can perform. Adding a feature means adding a
/// variant here and handling it in `App::execute`.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    // Files
    Save,
    SaveAs(PathBuf),
    OpenFile(PathBuf),
    Quit,
    ForceQuit,

    // Editing
    InsertChar(char),
    InsertNewline,
    Backspace,
    Delete,
    Undo,
    Redo,
    Tab,
    ToggleComment,

    // Markdown / markup helpers
    ToggleBold,
    ToggleItalic,

    // Movement
    MoveLeft,
    MoveRight,
    MoveUp,
    MoveDown,
    MoveWordLeft,
    MoveWordRight,
    MoveLineStart,
    MoveLineEnd,
    MoveBufferStart,
    MoveBufferEnd,
    PageUp,
    PageDown,

    // Selection
    ExtendLeft,
    ExtendRight,
    ExtendUp,
    ExtendDown,
    SelectAll,
    SelectLine,
    CollapseSelection,

    // Multi-cursor (Sublime-style)
    AddCursorAbove,
    AddCursorBelow,
    AddCursorNextMatch,
    SelectAllMatches,
    ClearExtraCursors,

    // Search / replace
    Find,
    FindNext,
    FindPrev,
    Replace { from: String, to: String },
    ReplaceAll { from: String, to: String },

    // Buffers
    NextBuffer,
    PrevBuffer,
    CloseBuffer,

    // Modes
    EnterMode(Mode),
    EnterCommandLine,

    // Misc
    #[allow(dead_code)]
    NoOp,
}
