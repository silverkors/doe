//! The command/message line (bottom row). When a prompt is active it shows the
//! prompt label and the user's input; otherwise it shows the latest transient
//! status message.

/// What kind of input the command line is currently collecting.
#[derive(Debug, Clone, PartialEq)]
pub enum PromptKind {
    /// `:` ex-style command.
    Command,
    /// Incremental find.
    Find,
    /// Save under a new path.
    SaveAs,
    /// Open a file by path.
    Open,
}

impl PromptKind {
    pub fn label(&self) -> &'static str {
        match self {
            PromptKind::Command => ":",
            PromptKind::Find => "find: ",
            PromptKind::SaveAs => "save as: ",
            PromptKind::Open => "open: ",
        }
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
