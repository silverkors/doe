//! Terminal user interface: a diffing cell-grid screen plus the renderer,
//! status bar and command line.

pub mod commandline;
pub mod file_picker;
pub mod layout;
pub mod overlay;
pub mod palette;
pub mod renderer;
pub mod screen;
pub mod statusbar;
pub mod wrap;

pub use screen::Screen;
