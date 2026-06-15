//! Terminal user interface: a diffing cell-grid screen plus the renderer,
//! status bar and command line.

pub mod commandline;
pub mod layout;
pub mod modal;
pub mod overlay;
pub mod renderer;
pub mod screen;
pub mod settings;
pub mod statusbar;
pub mod wrap;

pub use screen::Screen;
