//! Renders the Open picker (recent / search / filesystem) via the shared overlay.

use super::overlay::{self, Row};
use super::screen::Screen;
use crate::app::App;

pub fn render(screen: &mut Screen, app: &App) -> Option<(u16, u16)> {
    let picker = &app.file_picker;
    let rows: Vec<Row> = picker
        .results
        .iter()
        .map(|r| Row {
            text: &r.display,
            positions: &r.positions,
            hint: r.hint,
        })
        .collect();
    overlay::render(
        screen,
        app,
        "Open",
        &picker.query,
        &rows,
        picker.selected,
        "type a path, or a name to create",
    )
}
