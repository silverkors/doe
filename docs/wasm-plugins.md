# WASM plugins

DOE loads sandboxed WebAssembly plugins from `~/.config/doe/plugins/*.wasm` on
startup (set `DOE_CONFIG_DIR` to relocate). Each module runs in a
[wasmi](https://github.com/wasmi-labs/wasmi) interpreter with **no WASI, no
filesystem, no network** — a plugin can only compute over the data the host
hands it and return strings back. A module that fails to load is skipped (the
error is shown in the status bar); it never aborts the others.

Plugins implement the same contract as the built-in Rust plugins: react to
events, contribute a status-bar segment, and register command aliases.

## ABI

### Required exports

| Export | Signature | Purpose |
|--------|-----------|---------|
| `memory` | — | linear memory |
| `alloc` | `(i32) -> i32` | reserve `n` bytes, return the offset |
| `dealloc` | `(i32, i32)` | free a `(ptr, len)` pair |

A module missing any of these fails to load.

### Passing strings

Strings cross the boundary as a **packed `i64`**:

```
packed = (ptr as u64) << 32 | (len as u64)
```

- **Host → guest** (inputs): the host calls `alloc(len)`, writes the bytes at
  the returned `ptr`, calls the entry point with `(ptr, len)`, then `dealloc`s.
- **Guest → host** (outputs): the guest returns a packed `i64`; the host reads
  `len` bytes at `ptr`, then calls `dealloc(ptr, len)`. A `len` of `0` means
  "nothing".

### Entry points (all optional)

| Export | Signature | Returns |
|--------|-----------|---------|
| `doe_name` | `() -> i64` | UTF-8 plugin name (else the file stem is used) |
| `doe_status` | `(i32, i32) -> i64` | given a JSON view, a status segment (empty = none) |
| `doe_on_event` | `(i32, i32)` | given a JSON event |
| `doe_commands` | `() -> i64` | JSON array of `[alias, command]` pairs |

### JSON shapes

`doe_status` receives a scalar view (the buffer text is intentionally **not**
included, so the call stays cheap every frame):

```json
{ "line": 0, "col": 4, "language": "markdown", "path": "/x.md", "selection": [10, 14] }
```

`doe_on_event` receives:

```json
{ "kind": "save_file", "path": "/x.md", "command": null }
```

`kind` is one of `open_file`, `save_file`, `buffer_change`, `cursor_move`,
`command`, `exit`.

`doe_commands` returns e.g. `[["wc", "word_count"], ["up", "uppercase"]]` — each
alias becomes available on the command line and bindable like any command.

### Host functions

The host provides one optional import, ignored if unused:

```
(import "env" "doe_log" (func (param i32 i32)))   ;; (ptr, len) UTF-8
```

## Minimal example (WAT)

```wat
(module
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 1024))
  (data (i32.const 16) "hello")
  (func (export "alloc") (param i32) (result i32)
    (local $p i32)
    (local.set $p (global.get $heap))
    (global.set $heap (i32.add (global.get $heap) (local.get 0)))
    (local.get $p))
  (func (export "dealloc") (param i32 i32))
  (func (export "doe_name") (result i64)
    (i64.or (i64.shl (i64.const 16) (i64.const 32)) (i64.const 5))))
```

In Rust, compile a plugin to `wasm32-unknown-unknown`, export `alloc`/`dealloc`
over a bump or global allocator, and return packed pointers from the `doe_*`
entry points.
