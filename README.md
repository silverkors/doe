# DOE — David's Own Editor

A fast, memory-safe terminal text editor written in Rust, with first-class
Markdown support, multi-cursor editing (Sublime-style), configurable
keybindings, mouse support and a plugin architecture.

```sh
doe README.md          # open a file
doe                    # start with an empty buffer
doe a.md b.rs c.toml   # open several files as buffers
```

## Features (v0.1 MVP)

- **Rope-based buffer** (`ropey`) — opens 100 MB files in ~0.1 s, edits in the
  middle of huge files without copying, full UTF-8 support.
- **Markdown highlighting** — headings, lists, block quotes, fenced code,
  links, inline code, **bold**/*italic*, with dimmed markup punctuation.
- **Code highlighting** — Rust, Python, JS/TS, HTML/XML, CSS, JSON, TOML, YAML,
  Swift (keyword/string/comment/number, keyword-driven for now).
- **Multi-cursor** — add cursor on next match (`Ctrl+D`), above/below
  (`Alt+↑/↓`), select all matches (`Ctrl+L`), edit at every cursor at once.
- **Editing** — undo/redo, auto-indent, toggle line comment (`Ctrl+/`),
  bold/italic wrap (`Ctrl+B` / `Ctrl+I`), matching-bracket highlight, smart
  home, word motions.
- **Search & replace** — incremental find (`Ctrl+F`), next/prev (`F3`/`Shift+F3`),
  smart-case, `:replace_all from to`.
- **Mouse** — click to place cursor, drag to select, scroll wheel,
  `Ctrl/Alt+click` to add a cursor.
- **Soft wrap** — on by default (great for Markdown/prose); long lines wrap at
  word boundaries and `↑`/`↓` move by visual row. Toggle with `Alt+Z` or set
  `soft_wrap = false`.
- **Multiple buffers**, status bar, line numbers (absolute or relative).
- **Configurable** keybindings, settings and themes — no recompile needed.
- **Incremental rendering** — a diffing cell grid redraws only changed cells.
- **Plugin system** — internal API (events, buffer view, status segments,
  command aliases) designed to back sandboxed WASM plugins later.

## Command palette (Spotlight-style)

DOE is **modeless** — you're always editing, no Vim modes. The command entry
point is a fast, fuzzy command palette:

- `Ctrl+P` opens it; type to fuzzy-filter actions, `↑/↓` to move, `Enter` to
  run, `Esc` to close.
- With an empty query it surfaces the actions you use most. Usage counts are
  persisted (`~/.config/doe/usage.toml`), so your common actions stay on top.
- *(Planned: context-aware ranking — guessing the next action from what you're
  doing.)*

## Key bindings (defaults)

| Key | Action | Key | Action |
|-----|--------|-----|--------|
| `Ctrl+P` | command palette | `Ctrl+F` | find |
| `Ctrl+S` | save | `F3` / `Shift+F3` | find next / prev |
| `Ctrl+Q` | quit | `Ctrl+H` | replace (`from\|to`) |
| `Ctrl+Z` / `Ctrl+Y` | undo / redo | `Ctrl+D` | add cursor at next match |
| `Ctrl+B` / `Ctrl+I` | bold / italic | `Alt+↑` / `Alt+↓` | add cursor above / below |
| `Ctrl+A` | select all | `Ctrl+L` | select line |
| `Ctrl+D` | select word / add next occurrence | `Alt+F3` | select all occurrences |
| `Ctrl+/` | toggle comment | `Esc` | clear extra cursors |
| `Ctrl+O` | open file | `Ctrl+End` / `Ctrl+Home` | end / start of file |
| `Alt+Z` | toggle soft wrap | | |

Everything else lives in the palette.

## Configuration

`~/.config/doe/config.toml` (created on first run), themes in
`~/.config/doe/themes/<name>.toml`. DOE is modeless, so there is a single
`[keybindings.global]` context; bindings are merged over the built-in
defaults, so you only specify overrides:

```toml
theme = "default-dark"
line_numbers = true
tab_width = 4
insert_spaces = true
trim_trailing_whitespace_on_save = false

[keybindings.global]
"ctrl-p" = "command_palette"
"ctrl-d" = "add_cursor_next_match"
"alt-up" = "add_cursor_above"
```

## Architecture

```
src/
  main.rs        terminal setup + event loop
  app.rs         editor state + central command execution
  commands/      Command enum, name registry, command palette (catalog+fuzzy)
  config/        settings, keybindings, themes
  editor/        rope buffer, cursors, selections, undo
  syntax/        language detection, markdown + code highlighters
  ui/            diffing screen, renderer, status bar, command line, palette
  input/         key-chord normalization, mouse layout
  search/        find / replace
  plugins/       plugin API, registry, built-ins
  files/         path helpers
```

Everything — keybindings, command line, mouse, plugins — flows through the
single `Command` layer in `app.rs`, so new functionality means one new enum
variant and one handler.

## Build & test

```sh
cargo build --release     # binary at target/release/doe
cargo test                # buffer, multi-cursor, undo, search tests
```

Written without `unsafe` Rust.

## Roadmap

- **0.2 (in progress):** modeless editing + command palette ✓, bracket
  matching ✓; remaining: file picker via the palette, context-aware palette
  ranking.
- **0.3:** tree-sitter highlighting, WASM sandboxed plugins, project view,
  Git status, autosave/recovery.
