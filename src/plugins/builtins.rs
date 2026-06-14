//! Built-in example plugins. These exist to exercise (and document) the plugin
//! API end to end. `WordCountPlugin` adds a status-bar segment with the word
//! count of the current selection, or the whole document when nothing is
//! selected — kept cheap by only counting on demand.

use super::api::{Event, Plugin, PluginView};

#[derive(Default)]
pub struct WordCountPlugin {
    last_command: Option<String>,
}

impl Plugin for WordCountPlugin {
    fn name(&self) -> &str {
        "word-count"
    }

    fn on_event(&mut self, event: &Event) {
        if let Event::Command(c) = event {
            self.last_command = Some(c.clone());
        }
    }

    fn status_segment(&self, view: &PluginView) -> Option<String> {
        if let Some((s, e)) = view.selection {
            let text = view.rope.slice(s..e).to_string();
            let words = text.split_whitespace().count();
            Some(format!("{words} words sel"))
        } else if view.language == "markdown" {
            // Whole-document word count is handy when writing prose.
            let words = view.rope.to_string().split_whitespace().count();
            Some(format!("{words} words"))
        } else {
            None
        }
    }
}
