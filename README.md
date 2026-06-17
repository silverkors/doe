# DOE — David's Own Editor

A fast, memory-safe terminal text editor written in Rust, with first-class
Markdown support, multi-cursor editing (Sublime-style), configurable
keybindings, mouse support and a plugin architecture.

> [!note]
> Open this file in DOE (`doe README.md`) and the callouts below render as
> live preview cards — move the cursor onto one to edit its raw source.

```sh
doe README.md          # open a file
doe                    # start with an empty buffer
doe a.md b.rs c.toml   # open several files as buffers
```

> [!tip]
> Press `Ctrl+P` for the command palette and `Ctrl+,` for the settings panel —
> you rarely need to remember any other shortcut.

## Features (v0.1 MVP)

- **Rope-based buffer** (`ropey`) — opens 100 MB files in ~0.1 s, edits in the
  middle of huge files without copying, full UTF-8 support.
- **Markdown highlighting** — headings, lists, block quotes, fenced code,
  links, inline code, **bold**/*italic*, with dimmed markup punctuation.
  Also **callouts** (`> [!note] Title` — accent bar, dimmed `[!type]`, styled
  title and body) and inline **HTML/XML tags** (`<font color="…">`). Callouts
  get a **live preview**: decorated when the cursor is elsewhere, raw source
  when you're editing them. Per-type **accent colour and glyph are
  configurable** in a panel (palette: "Callout Styles…") and can be
  **imported from Obsidian's Callout Manager** ("Import Callouts from Obsidian…").
- **Settings panel** — `Ctrl+,` (or the palette) opens a modal to change
  preferences (theme, wrap, tab width, …) by navigating and toggling; changes
  apply live and save to `config.toml`. No need to hand-edit anything.
- **Code highlighting** — **tree-sitter** grammars for Rust, Python, JS/TS,
  HTML/XML, CSS and JSON (real parse trees, not regex); TOML, YAML and Swift
  use the keyword-driven fallback. Large files (>1 MB) skip the parse and use
  the fallback to stay cheap.
- **Multi-cursor** — add cursor on next match (`Ctrl+D`), above/below
  (`Alt+↑/↓`), select all matches (`Ctrl+L`), edit at every cursor at once.
- **Structural editing** (tree-sitter) — **expand/shrink selection** to the
  enclosing syntax node (`Alt+Shift+↑/↓`) and **Go to Symbol** (`Ctrl+Shift+O`,
  fuzzy outline of definitions, or headings in Markdown). The buffer is parsed
  once per edit and cached, so this and highlighting share one parse.
- **Editing** — undo/redo, auto-indent, toggle line comment (`Ctrl+/`),
  bold/italic wrap (`Ctrl+B` / `Ctrl+I`), matching-bracket highlight, smart
  home, word motions.
- **Search & replace** — incremental find (`Ctrl+F`), next/prev (`F3`/`Shift+F3`),
  smart-case, `:replace_all from to`.
- **Mouse** — click to place cursor, drag to select, scroll wheel,
  `Ctrl/Alt+click` to add a cursor. Enable **"Drag to scroll"** in settings to
  pan the document with a one-finger drag (handy over SSH/tmux on touch clients
  like Termius — needs `set -g mouse on` in tmux).
- **Soft wrap** — on by default (great for Markdown/prose); long lines wrap at
  word boundaries and `↑`/`↓` move by visual row. Toggle with `Alt+Z` or set
  `soft_wrap = false`.
- **Never lose work** — an invisible autosave continuously mirrors open buffers
  to a recovery store, so you can quit without saving (no prompt): relaunching
  reopens your files *with* their unsaved changes (including never-saved
  buffers, which you can then Save As). Survives crashes too. The status bar
  shows `*` while a buffer has unsaved changes; "Discard Changes and Quit"
  throws them away. Closing a single modified buffer (`Ctrl+W`) prompts to
  **Save / Discard / Cancel**.
- **Multiple buffers**, status bar, line numbers (absolute or relative).
- **Configurable** keybindings, settings and themes — no recompile needed.
- **Incremental rendering** — a diffing cell grid redraws only changed cells.
- **Plugin system** — internal API (events, buffer view, status segments,
  command aliases) plus a **sandboxed WASM host** (wasmi, no WASI/fs/network):
  drop `*.wasm` in `~/.config/doe/plugins/` and they load on startup. Plugins
  can read the document and set status via host functions. See the
  [plugin ABI](docs/wasm-plugins.md).
- **Dynamic documents** — fenced code blocks marked `run` execute and their
  output is spliced into a `<!-- doe:output -->` region (`Alt+Enter` / palette
  "Run Code Block"), shown as a **live "computed" card** (markers concealed,
  raw when the cursor is inside). A **sandboxed Lua** evaluator is built in (no
  fs/net, with a timeout and output cap); running is gated by **per-folder
  trust** (Once / Always / Never). `auto` blocks run on save in trusted
  folders, and a block that errors shows a **diagnostic** in the gutter.
  Other languages plug in as WASM evaluator modules. See the
  [design](docs/dynamic-documents.md).

## The modal (Spotlight-style, tabbed)

DOE is **modeless** — you're always editing, no Vim modes. One fuzzy modal with
three tabs is the entry point for commands, opening files and switching buffers:

- **Commands** (`Ctrl+P`) — fuzzy-filter actions; empty query surfaces the ones
  you use most (usage persisted to `~/.config/doe/usage.toml`).
- **Open** (`Ctrl+O`) — the Open picker (below).
- **Buffers** (`Ctrl+T`) — switch between open files; `Enter` jumps to one.

**`Tab` / `Shift+Tab`** cycle the tabs (each keeps its own query); `←`/`→` also
cycle in the Commands and Buffers tabs (in the Open tab they navigate the file
tree). `Ctrl+Tab` works too. `↑/↓` move, `Enter` runs/opens, `Esc` closes.
`Ctrl+1`…`Ctrl+9` switch directly to a buffer without opening the modal.
(`Ctrl+Tab` and `Ctrl+digit` need a terminal with the keyboard-enhancement
protocol, which DOE requests on startup; `Tab`/`Shift+Tab` and the arrows work
everywhere.)

**The Open tab** is one searchable picker that does everything:

- **Recent files** first when the query is empty (the 10 most recent, persisted
  to `~/.config/doe/recent.toml`); a "⋯ N more recent files" row expands to the
  full history.
- **Fuzzy search** — type plain text to match across recent + project files
  (the working dir is scanned, skipping `.git`/`target`/`node_modules`/`dist`/
  `build`/hidden).
- **Filesystem navigation** — type a path (anything with `/`, or starting `~`
  `.` `/`) to browse: `Tab` (or `Enter`) descends into the selected directory,
  `←` goes back out, and `Tab` autocompletes a half-typed name.
- **Arbitrary paths & new files** — the first row in path mode opens exactly
  what you typed (existing or new, inside or outside the tree); in search mode
  an unmatched name offers to create it.

## Key bindings (defaults)

| Key | Action | Key | Action |
|-----|--------|-----|--------|
| `Ctrl+P` | modal: commands | `Ctrl+F` | find |
| `Ctrl+O` | modal: open file | `F3` / `Shift+F3` | find next / prev |
| `Ctrl+T` | modal: buffers | `Ctrl+H` | replace (`from\|to`) |
| `Tab` / `Shift+Tab` | cycle modal tabs | `Ctrl+D` | select word / add next |
| `Ctrl+1`…`Ctrl+9` | switch to buffer N | `Alt+F3` | select all occurrences |
| `Ctrl+S` / `Ctrl+Q` | save / quit | `Alt+↑` / `Alt+↓` | add cursor above / below |
| `Ctrl+W` | close buffer (prompts if unsaved) | | |
| `Ctrl+Z` / `Ctrl+Y` | undo / redo | `Ctrl+L` | select line |
| `Ctrl+B` / `Ctrl+I` | bold / italic | `Ctrl+A` | select all |
| `Ctrl+/` | toggle comment | `Esc` | clear extra cursors |
| `Alt+Z` | toggle soft wrap | `Ctrl+,` | settings panel |
| `Ctrl+Home` / `Ctrl+End` | start / end of file | | |

Everything else lives in the command palette.

## Configuration

`~/.config/doe/config.toml` (created on first run), themes in
`~/.config/doe/themes/<name>.toml`, and callout styles in
`~/.config/doe/callouts.toml` (set `DOE_CONFIG_DIR` to use another
location). DOE is modeless, so there is a single
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

## Examples

### A dynamic document

Open this in DOE, put the cursor in the block and press `Alt+Enter` (the first
run in a folder asks you to trust it). DOE runs the Lua and owns the
`doe:output` region — rerunning replaces it, and with the cursor elsewhere it
shows as a dim "computed" card:

````markdown
> [!tip] Live values
> Edit the loop, rerun, and the table below updates.

```lua run id=primes
local function is_prime(n)
  for i = 2, math.floor(n ^ 0.5) do
    if n % i == 0 then return false end
  end
  return n > 1
end
local out = {}
for n = 2, 20 do
  if is_prime(n) then out[#out + 1] = n end
end
return table.concat(out, ", ")
```
<!-- doe:output id=primes -->
2, 3, 5, 7, 11, 13, 17, 19
<!-- /doe:output -->
````

The Lua runs sandboxed — no `os`/`io`/filesystem/network, with a timeout and an
output cap. `print(...)` is captured too, so `print("hi")` and `return value`
both end up in the output region.

### Styling callouts

`~/.config/doe/callouts.toml` — override a built-in type or add your own (or
edit it live in the "Callout Styles…" panel, or import from Obsidian):

```toml
[note]            # override a built-in
color = "#448aff"
icon  = "●"

[psalm]           # a custom type — [!psalm] in your Markdown
color = "#a882ff"
icon  = "♪"

[blessing]
color = "#fb464c"
icon  = "☞"
```

### A status-bar plugin

Drop a `.wasm` in `~/.config/doe/plugins/` to add a status segment or commands;
the full ABI (and a minimal example) is in
[docs/wasm-plugins.md](docs/wasm-plugins.md).

## Architecture

```
src/
  main.rs        terminal setup + event loop
  app.rs         editor state + central command execution
  commands/      Command enum, name registry, command palette (catalog+fuzzy)
  config/        settings, keybindings, themes
  editor/        rope buffer, cursors, selections, undo
  syntax/        language detection, markdown + code highlighters
  ui/            diffing screen, renderer, soft-wrap, overlay (palette/picker)
  input/         key-chord normalization, mouse layout
  search/        find / replace
  plugins/       plugin API, registry, built-ins
  files/         path helpers, fuzzy file picker
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

- **0.2 (released):** modeless editing + command palette ✓, bracket matching ✓,
  soft wrap ✓, fuzzy file picker (with fs navigation) ✓, crash
  recovery/autosave ✓, unified tabbed modal ✓, Markdown callout live preview ✓.
- **0.3 (released):** tree-sitter highlighting (Rust/Python/JS/TS/HTML/CSS/
  JSON ✓), configurable callouts + Obsidian import ✓, context-aware palette
  ranking ✓, WASM sandboxed plugins ✓, and **dynamic documents** ✓ — runnable
  embedded code with a sandboxed Lua evaluator and live-previewed output
  ([design](docs/dynamic-documents.md)).
- **Later:** more tree-sitter grammars, Python/JS document evaluators (via the
  evaluator table or WASM), richer plugin host capabilities.

> [!warning]
> Dynamic documents run **arbitrary code**. They are off until you trust a
> folder, the Lua sandbox is default-deny (no fs/net/process, with a timeout and
> output cap), and `auto` blocks never run in untrusted documents. Read the
> [design](docs/dynamic-documents.md) before enabling untrusted content.
