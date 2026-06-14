//! Search and replace.

pub mod find;

/// Live search state, shared between the command line and the renderer (which
/// highlights all matches).
#[derive(Debug, Clone, Default)]
pub struct SearchState {
    pub query: String,
    pub case_sensitive: bool,
    /// Cached match ranges (char indices) for the current buffer + query.
    pub matches: Vec<(usize, usize)>,
}

impl SearchState {
    pub fn recompute(&mut self, text: &str) {
        if self.query.is_empty() {
            self.matches.clear();
        } else {
            self.matches = find::find_all(text, &self.query, self.case_sensitive);
        }
    }
}
