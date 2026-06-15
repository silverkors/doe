//! Central command system. All editor functionality is expressed as a
//! [`Command`]. Keybindings, the command line, mouse actions and (future)
//! plugins all funnel through this single layer.

pub mod palette;
pub mod registry;

use std::path::PathBuf;

/// The single keybinding context. DOE is modeless (always editing); this name
/// is the section used in `[keybindings.global]`.
pub const BINDING_CONTEXT: &str = "global";

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
    SaveAndQuit,

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

    // Command palette (Spotlight-style action launcher)
    CommandPalette,

    // View
    ToggleSoftWrap,
    Settings,

    // Misc
    #[allow(dead_code)]
    NoOp,
}
