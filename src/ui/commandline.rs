//! The command/message line (bottom row). When a prompt is active it shows the
//! prompt label and the user's input; otherwise it shows the latest transient
//! status message.

/// What kind of input the command line is currently collecting. These are the
/// free-text prompts; discrete actions go through the command palette instead.
#[derive(Debug, Clone, PartialEq)]
pub enum PromptKind {
    /// Incremental find.
    Find,
    /// Replace, entered as `from|to`.
    Replace,
    /// Save under a new path.
    SaveAs,
    /// Path to an Obsidian vault / Callout Manager `data.json` to import.
    ImportCallouts,
    /// A prompt to send to the default chat AI provider; the reply streams into
    /// the buffer at the cursor.
    AiChat,
    /// Closing a modified buffer: Save / Discard / Cancel. Not a text prompt —
    /// the command-line key handler intercepts s/d/c instead of inserting.
    ConfirmClose,
    /// Running document code in an untrusted folder: Once / Always / Never.
    ConfirmTrust,
}

impl PromptKind {
    pub fn label(&self) -> &'static str {
        match self {
            PromptKind::Find => "find: ",
            PromptKind::Replace => "replace (from|to): ",
            PromptKind::SaveAs => "save as: ",
            PromptKind::ImportCallouts => "import callouts from (vault or data.json): ",
            PromptKind::AiChat => "ai: ",
            PromptKind::ConfirmClose => "unsaved changes — [s]ave  [d]iscard  [c]ancel",
            PromptKind::ConfirmTrust => "run code in this folder? [o]nce  [a]lways  [n]ever",
        }
    }

    /// A confirm prompt captures single keys instead of collecting text.
    pub fn is_confirm(&self) -> bool {
        matches!(self, PromptKind::ConfirmClose | PromptKind::ConfirmTrust)
    }
}

/// State of the command line.
#[derive(Debug, Clone, Default)]
pub struct CommandLine {
    pub active: bool,
    pub kind: Option<PromptKind>,
    pub input: String,
}

impl CommandLine {
    pub fn open(&mut self, kind: PromptKind, prefill: &str) {
        self.active = true;
        self.kind = Some(kind);
        self.input = prefill.to_string();
    }

    pub fn close(&mut self) {
        self.active = false;
        self.kind = None;
        self.input.clear();
    }

    pub fn label(&self) -> &'static str {
        self.kind.as_ref().map(|k| k.label()).unwrap_or("")
    }
}
