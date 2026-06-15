//! Renders the fuzzy file picker using the shared overlay.

use super::overlay::{self, Row};
use super::screen::Screen;
use crate::app::App;

pub fn render(screen: &mut Screen, app: &App) -> Option<(u16, u16)> {
    let picker = &app.file_picker;
    let rows: Vec<Row> = picker
        .results
        .iter()
        .map(|m| Row {
            text: picker.path_str(m),
            positions: &m.positions,
            hint: "",
        })
        .collect();
    overlay::render(
        screen,
        app,
        "Open File",
        &picker.query,
        &rows,
        picker.selected,
        "no files found",
    )
}
