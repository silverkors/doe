//! DOE — David's Own Editor.
//!
//! A fast, memory-safe terminal text editor with first-class Markdown support,
//! multi-cursor editing, configurable keybindings, mouse support and a plugin
//! architecture. This file wires up the terminal, runs the event loop and
//! guarantees the terminal is restored even on panic.

mod app;
mod commands;
mod config;
mod editor;
mod files;
mod input;
mod plugins;
mod search;
mod syntax;
mod ui;

use anyhow::Result;
use app::App;
use config::Config;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::{cursor, execute, terminal};
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How often, at most, to flush modified buffers to the recovery store.
const AUTOSAVE_INTERVAL: Duration = Duration::from_millis(750);
use ui::renderer;
use ui::Screen;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> Result<()> {
    let mut paths = Vec::new();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            "-v" | "--version" => {
                println!("doe {VERSION}");
                return Ok(());
            }
            _ if arg.starts_with('-') => {
                eprintln!("doe: unknown option {arg}");
                return Ok(());
            }
            _ => paths.push(PathBuf::from(arg)),
        }
    }

    let config = Config::load();
    let mut app = App::new(config, paths);

    // Install a panic hook that restores the terminal before printing the panic.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        default_hook(info);
    }));

    let result = run(&mut app);

    restore_terminal()?;
    result
}

fn print_help() {
    println!(
        "DOE {VERSION} — David's Own Editor\n\n\
         Usage: doe [FILE]...\n\n\
         Options:\n  \
         -h, --help     Show this help\n  \
         -v, --version  Show version\n\n\
         In-editor: Ctrl+S save · Ctrl+Q quit · Ctrl+F find · Ctrl+D add cursor\n  \
         at next match · Alt+Up/Down add cursor · Ctrl+B bold · Ctrl+I italic\n  \
         · `:` command line. Config: ~/.config/doe/config.toml"
    );
}

fn setup_terminal() -> Result<()> {
    terminal::enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, terminal::EnterAlternateScreen, EnableMouseCapture, cursor::Hide)?;
    // Ask for the keyboard-enhancement protocol so chords like Ctrl+Tab and
    // Ctrl+1..9 are reported distinctly (no-op on terminals that don't support
    // it). Best effort — ignored if unsupported.
    if matches!(terminal::supports_keyboard_enhancement(), Ok(true)) {
        let _ = execute!(out, PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
    }
    Ok(())
}

fn restore_terminal() -> Result<()> {
    let mut out = io::stdout();
    if matches!(terminal::supports_keyboard_enhancement(), Ok(true)) {
        let _ = execute!(out, PopKeyboardEnhancementFlags);
    }
    execute!(out, cursor::Show, DisableMouseCapture, terminal::LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;
    Ok(())
}

fn run(app: &mut App) -> Result<()> {
    setup_terminal()?;
    let (w, h) = terminal::size()?;
    app.resize(w, h);

    let mut screen = Screen::new();
    let mut out = io::stdout();
    let mut last_autosave = Instant::now();

    loop {
        app.recompute_fence_state();
        draw(&mut screen, app, &mut out)?;
        if app.should_quit {
            break;
        }

        // Poll so we can periodically check for external file changes while
        // still blocking (no busy-loop) when nothing is happening.
        if event::poll(Duration::from_millis(500))? {
            match event::read()? {
                Event::Key(key) => {
                    // Ignore key-release events (reported by some terminals).
                    if key.kind != KeyEventKind::Release {
                        app.handle_key(key);
                    }
                }
                Event::Mouse(m) => app.handle_mouse(m),
                Event::Resize(w, h) => {
                    app.resize(w, h);
                    screen.mark_all_dirty();
                }
                _ => {}
            }
        } else {
            app.check_external_changes();
        }

        // Throttled invisible autosave to the recovery store. Skipped when
        // quitting so a clean exit's recovery cleanup is not re-created.
        if !app.should_quit && last_autosave.elapsed() >= AUTOSAVE_INTERVAL {
            app.autosave();
            last_autosave = Instant::now();
        }
    }
    Ok(())
}

fn draw(screen: &mut Screen, app: &App, out: &mut impl Write) -> Result<()> {
    renderer::render(screen, app, out)?;
    match screen.cursor {
        Some((x, y)) => execute!(out, cursor::MoveTo(x, y), cursor::Show)?,
        None => execute!(out, cursor::Hide)?,
    }
    out.flush()?;
    Ok(())
}
