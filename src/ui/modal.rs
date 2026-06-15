//! The unified modal: one overlay with tabs for **Commands** (the command
//! palette), **Open** (the fuzzy file picker) and **Buffers** (switch between
//! open files). Ctrl+Tab / Ctrl+Shift+Tab cycle the tabs; Ctrl+P / Ctrl+O /
//! Ctrl+T open it on a specific tab. Each tab keeps its own query.

use super::overlay::{self, Row};
use super::screen::Screen;
use crate::app::App;
use crate::commands::palette::{catalog, fuzzy};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalTab {
    Commands,
    Open,
    Buffers,
}

impl ModalTab {
    pub fn index(self) -> usize {
        match self {
            ModalTab::Commands => 0,
            ModalTab::Open => 1,
            ModalTab::Buffers => 2,
        }
    }
    pub fn next(self) -> ModalTab {
        match self {
            ModalTab::Commands => ModalTab::Open,
            ModalTab::Open => ModalTab::Buffers,
            ModalTab::Buffers => ModalTab::Commands,
        }
    }
    pub fn prev(self) -> ModalTab {
        self.next().next()
    }
}

/// State for the Buffers tab: filter open buffers by name and pick one.
#[derive(Default)]
pub struct BufferTab {
    pub query: String,
    pub selected: usize,
    pub results: Vec<usize>,
}

impl BufferTab {
    pub fn reset(&mut self) {
        self.query.clear();
        self.selected = 0;
    }

    /// Recompute the filtered/ordered buffer indices from their display names.
    pub fn update(&mut self, names: &[String]) {
        if self.query.is_empty() {
            self.results = (0..names.len()).collect();
        } else {
            let mut scored: Vec<(i32, usize)> = names
                .iter()
                .enumerate()
                .filter_map(|(i, n)| fuzzy(&self.query, n).map(|(s, _)| (s, i)))
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            self.results = scored.into_iter().map(|(_, i)| i).collect();
        }
        if self.selected >= self.results.len() {
            self.selected = self.results.len().saturating_sub(1);
        }
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.results.is_empty() {
            return;
        }
        let n = self.results.len() as isize;
        self.selected = (((self.selected as isize + delta) % n + n) % n) as usize;
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.results.get(self.selected).copied()
    }
}

const TABS: [&str; 3] = ["Commands", "Open", "Buffers"];

/// Display label for an open buffer (relative path or "[No Name]").
pub fn buffer_label(app: &App, i: usize) -> String {
    match &app.buffers[i].path {
        Some(p) => crate::files::display_path(p),
        None => "[No Name]".to_string(),
    }
}

fn buffer_hint(app: &App, i: usize) -> &'static str {
    if i == app.active {
        "active"
    } else if app.buffers[i].modified {
        "unsaved"
    } else {
        ""
    }
}

pub fn render(screen: &mut Screen, app: &App) -> Option<(u16, u16)> {
    let active = app.modal_tab.index();
    match app.modal_tab {
        ModalTab::Commands => {
            let cat = catalog();
            let rows: Vec<Row> = app
                .palette
                .results
                .iter()
                .map(|r| Row { text: cat[r.idx].title, positions: &r.positions, hint: cat[r.idx].hint })
                .collect();
            overlay::render(screen, app, &TABS, active, &app.palette.query, &rows, app.palette.selected, "no matching commands")
        }
        ModalTab::Open => {
            let rows: Vec<Row> = app
                .file_picker
                .results
                .iter()
                .map(|m| Row { text: &m.display, positions: &m.positions, hint: m.hint })
                .collect();
            overlay::render(screen, app, &TABS, active, &app.file_picker.query, &rows, app.file_picker.selected, "type a path, or a name to create")
        }
        ModalTab::Buffers => {
            let labels: Vec<String> = app.buffer_tab.results.iter().map(|&i| buffer_label(app, i)).collect();
            let rows: Vec<Row> = app
                .buffer_tab
                .results
                .iter()
                .zip(labels.iter())
                .map(|(&i, label)| Row { text: label, positions: &[], hint: buffer_hint(app, i) })
                .collect();
            overlay::render(screen, app, &TABS, active, &app.buffer_tab.query, &rows, app.buffer_tab.selected, "no open files")
        }
    }
}
