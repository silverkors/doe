//! Renders the command palette using the shared overlay: a fuzzy-highlighted
//! list of actions with keybinding hints. Returns the desired cursor position.

use super::overlay::{self, Row};
use super::screen::Screen;
use crate::app::App;
use crate::commands::palette::catalog;

pub fn render(screen: &mut Screen, app: &App) -> Option<(u16, u16)> {
    let cat = catalog();
    let palette = &app.palette;
    let rows: Vec<Row> = palette
        .results
        .iter()
        .map(|r| Row {
            text: cat[r.idx].title,
            positions: &r.positions,
            hint: cat[r.idx].hint,
        })
        .collect();
    overlay::render(
        screen,
        app,
        "Command Palette",
        &palette.query,
        &rows,
        palette.selected,
        "no matching commands",
    )
}
