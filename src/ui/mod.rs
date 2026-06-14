//! Terminal user interface: a diffing cell-grid screen plus the renderer,
//! status bar and command line.

pub mod commandline;
pub mod layout;
pub mod palette;
pub mod renderer;
pub mod screen;
pub mod statusbar;

pub use screen::Screen;
