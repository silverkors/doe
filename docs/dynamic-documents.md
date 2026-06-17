# Dynamic documents — design

> **Status:** the "smallest safe slice" (§6) is **implemented** in 0.3 — runnable
> fenced blocks, `RunCodeBlock`/`RunDocument`, output spliced into a
> `doe:output` region as one undo step, a sandboxed **Lua** evaluator (`mlua`,
> stripped stdlib, timeout, output cap), and per-folder trust (Once / Always /
> Never). Live-preview of the output region (§6.5) is not done yet; Python/JS
> remain additive via the evaluator table or a WASM plugin. The rest below is the
> original design.

Goal: let a document *do things with code* so it can be dynamic — e.g. a
Markdown file that embeds Lua/Python/other code which runs and whose output
becomes part of the document (computed tables, generated text, live values).
This must fit DOE's existing plugin architecture and stay safe.

This is a design, not yet implemented. It builds directly on what's already in
`src/plugins/` (events, a read view of the buffer, command registration) and the
roadmap's WASM plugin direction.

## 1. Authoring model — executable code blocks

Reuse fenced code blocks with an *info string* that marks them runnable:

````markdown
```lua run
return 2 + 40
```
````

- Info string = `<lang> <directives…>`. `run` (or `eval`) marks it executable.
- Optional directives: `id=foo` (name a block), `auto` (run on save/open),
  `out=below|replace|hidden` (where output goes), `lang=python` etc.
- Output is written into a sibling, generated block the editor owns:

````markdown
```lua run id=sum
return 2 + 40
```
<!-- doe:output id=sum -->
42
<!-- /doe:output -->
````

The output region is delimited by HTML comments so the source stays valid
Markdown everywhere else, and DOE can find/replace just the generated part
without touching the user's code. (In the live-preview renderer, the output
region can be shown rendered and the markers concealed — same machinery as the
callout preview.)

## 2. Execution model

A new command layer entry drives it (consistent with everything going through
`Command`):

- `Command::RunCodeBlock` — run the block under the cursor.
- `Command::RunDocument` — run all `run`/`auto` blocks top-to-bottom.
- Triggered by keybinding, the palette ("Run Code Block"), or `on_save` for
  `auto` blocks.

Flow: locate the block (cursor ∈ fenced block) → extract `(lang, source,
directives)` → dispatch to a registered **evaluator** for `lang` → capture
`stdout` + return value + diagnostics → splice into the `doe:output` region as
one undoable edit → mark the buffer modified (autosave/recovery already cover
the result).

Evaluation is async/off-thread so a slow script never blocks the UI (the event
loop already polls; results arrive as an internal event and re-render).

## 3. Evaluator interface

Extend the plugin API with an evaluator capability (sibling to today's
`Plugin`):

```rust
pub struct EvalRequest<'a> { pub lang: &'a str, pub source: &'a str, pub directives: &'a Directives, pub doc_path: Option<&'a Path> }
pub struct EvalResult { pub stdout: String, pub value: Option<String>, pub diagnostics: Vec<Diagnostic> }

pub trait Evaluator {
    fn languages(&self) -> &[&str];
    fn eval(&mut self, req: &EvalRequest) -> EvalResult;
}
```

The `PluginRegistry` gains an evaluator table keyed by language, so a plugin can
register support for `lua`, `python`, `js`, … exactly like it registers commands
today.

## 4. Backends (pick per language, by trust)

| Backend | Safety | Notes |
|---|---|---|
| **WASM (Wasmtime/extism)** | Sandboxed by default | The recommended path and the roadmap's plugin format. Capabilities (fs, net, time) are explicitly granted; deterministic; cross-platform. Lua/Python/JS all compile to or run inside WASM. |
| **Embedded Lua (`mlua`)** | Medium | Small, fast, easy to sandbox (custom std, no `io`/`os` unless granted). Great default for in-document scripting. |
| **External interpreter (subprocess)** | Unsafe without a sandbox | `python`/`node` via `std::process` with stdin=source. Easiest to ship, but it's arbitrary code execution — must be gated behind explicit per-project trust. |

Start with **one** safe backend (mlua or a WASM host), add others as plugins.

## 5. Security — the hard requirement

Executing document code is arbitrary code execution, so it is **off by default**
and gated:

- **Trust per workspace/file.** First run prompts "Run code in this document?"
  with Once / Always-for-this-folder / Never. Trust is stored per directory.
- **Capabilities, default-deny.** WASM/Lua sandboxes start with no filesystem,
  network, environment, or process access; a block opts in via directives that
  the trust prompt surfaces (`caps=fs:read,net`).
- **Never auto-run untrusted.** `auto` blocks only run in trusted docs; opening
  a file never executes anything until the user trusts it.
- **Resource limits.** Wall-clock timeout, memory cap, output-size cap; runs are
  cancellable.
- **Visible provenance.** Generated regions are clearly marked and shown as
  "computed", never silently editable as if hand-written.

This mirrors the existing rule that plugins must be sandboxed and never given
unbounded access.

## 6. Suggested first milestone (smallest safe slice)

1. Parse executable fenced blocks + the `doe:output` region (pure, testable).
2. `Command::RunCodeBlock` + palette action, output spliced as one undo step.
3. One sandboxed evaluator: **Lua via `mlua`** with a stripped stdlib (no
   `io`/`os`/`package`), a timeout, and an output cap. No fs/net.
4. Per-folder trust prompt before the first run.
5. Live-preview the output region (conceal markers), reusing the callout
   preview machinery.

That yields genuinely dynamic Markdown (compute values, generate text/tables)
with no unsafe surface, and the evaluator table makes Python/JS/WASM additive
plugins rather than a rewrite.
