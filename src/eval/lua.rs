//! A sandboxed Lua evaluator (mlua, vendored Lua 5.4). The interpreter starts
//! with the dangerous standard libraries removed (no `os`, `io`, `package`,
//! `require`, `load*`, `debug`), `print` redirected into a capture buffer, a
//! wall-clock timeout enforced via an instruction hook, a memory cap, and a
//! cap on captured output. It has no filesystem, network, or process access.
//!
//! Each run happens on a worker thread with a hard deadline: the instruction
//! hook can't interrupt a single long C call (e.g. a pathological
//! `string.find` pattern), so the editor stops waiting rather than freezing.

use super::{EvalRequest, EvalResult, Evaluator};
use mlua::{HookTriggers, Lua, MultiValue, Value, VmState};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

pub struct LuaEvaluator {
    timeout: Duration,
    output_cap: usize,
    memory_limit: usize,
}

impl Default for LuaEvaluator {
    fn default() -> Self {
        LuaEvaluator { timeout: Duration::from_millis(2000), output_cap: 64 * 1024, memory_limit: 256 * 1024 * 1024 }
    }
}

/// Extra time past `timeout` the editor waits for the worker before giving
/// up on it (the in-VM hook normally reports the timeout first).
const GRACE: Duration = Duration::from_millis(500);

impl Evaluator for LuaEvaluator {
    fn handles(&self, lang: &str) -> bool {
        lang == "lua"
    }

    fn eval(&mut self, req: &EvalRequest) -> EvalResult {
        let (timeout, cap, mem) = (self.timeout, self.output_cap, self.memory_limit);
        let source = req.source.to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("doe-lua".into())
            .spawn(move || {
                let _ = tx.send(run(&source, timeout, cap, mem));
            });
        if let Err(e) = spawned {
            return EvalResult { output: String::new(), error: Some(format!("could not start Lua: {e}")) };
        }
        // On timeout the worker is abandoned: it finishes (or is torn down at
        // exit) on its own, without blocking the UI.
        match rx.recv_timeout(timeout + GRACE) {
            Ok(Ok(output)) => EvalResult { output, error: None },
            Ok(Err((output, error))) => EvalResult { output, error: Some(error) },
            Err(_) => EvalResult { output: String::new(), error: Some("timed out".to_string()) },
        }
    }
}

/// Run `source`, returning the combined output, or `(partial_output, error)`.
fn run(source: &str, timeout: Duration, cap: usize, memory_limit: usize) -> Result<String, (String, String)> {
    let lua = Lua::new();
    let captured = Rc::new(RefCell::new(String::new()));
    if let Err(e) = lua.set_memory_limit(memory_limit) {
        return Err((String::new(), format!("sandbox setup failed: {e}")));
    }

    if let Err(e) = sandbox(&lua, &captured, cap) {
        return Err((String::new(), format!("sandbox setup failed: {e}")));
    }

    // Wall-clock timeout: a hook fires every N instructions and aborts past the
    // deadline.
    let deadline = Instant::now() + timeout;
    let _ = lua.set_hook(HookTriggers::new().every_nth_instruction(100_000), move |_, _| {
        if Instant::now() > deadline {
            Err(mlua::Error::RuntimeError("timed out".to_string()))
        } else {
            Ok(VmState::Continue)
        }
    });

    let result = lua.load(source).eval::<MultiValue>();
    let mut out = captured.borrow().clone();
    match result {
        Ok(values) => {
            let rets: Vec<String> = values.iter().map(value_to_string).collect();
            if !rets.is_empty() {
                if !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str(&rets.join("\t"));
            }
            Ok(truncate(trim_trailing_newlines(out), cap))
        }
        Err(e) => Err((truncate(trim_trailing_newlines(out), cap), clean_error(&e.to_string()))),
    }
}

fn trim_trailing_newlines(s: String) -> String {
    let trimmed = s.trim_end_matches('\n');
    if trimmed.len() == s.len() {
        s
    } else {
        trimmed.to_string()
    }
}

/// Remove dangerous globals and redirect `print` into the capture buffer.
fn sandbox(lua: &Lua, captured: &Rc<RefCell<String>>, cap: usize) -> mlua::Result<()> {
    let globals = lua.globals();
    for name in ["os", "io", "package", "require", "dofile", "loadfile", "load", "loadstring", "debug"] {
        globals.set(name, Value::Nil)?;
    }

    let buf = captured.clone();
    let print = lua.create_function(move |_, args: MultiValue| {
        let mut s = buf.borrow_mut();
        // Stop appending once over the cap; the final truncate adds the marker.
        if s.len() < cap {
            let line: Vec<String> = args.iter().map(value_to_string).collect();
            s.push_str(&line.join("\t"));
            s.push('\n');
        }
        Ok(())
    })?;
    globals.set("print", print)?;
    Ok(())
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::Nil => "nil".to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Integer(i) => i.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.to_string_lossy().to_string(),
        other => other.type_name().to_string(),
    }
}

fn truncate(mut s: String, cap: usize) -> String {
    if s.len() > cap {
        // Truncate on a char boundary at or below the cap.
        let mut end = cap;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
        s.push_str("\n…(output truncated)");
    }
    s
}

/// Strip the `[string "..."]:N:` prefix Lua puts on runtime errors.
fn clean_error(msg: &str) -> String {
    if let Some(pos) = msg.find("]:") {
        if msg.starts_with("[string") {
            return msg[pos + 2..].trim_start().to_string();
        }
    }
    msg.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(src: &str) -> EvalResult {
        LuaEvaluator::default().eval(&EvalRequest { lang: "lua", source: src, doc_path: None })
    }

    #[test]
    fn returns_value() {
        let r = eval("return 2 + 40");
        assert_eq!(r.output, "42");
        assert!(r.error.is_none());
    }

    #[test]
    fn captures_print() {
        let r = eval("print('hello'); print(1, 2)");
        assert_eq!(r.output, "hello\n1\t2");
    }

    #[test]
    fn print_then_return() {
        let r = eval("print('log')\nreturn 7");
        assert_eq!(r.output, "log\n7");
    }

    #[test]
    fn sandbox_blocks_os_and_io() {
        // `os` is nil, so indexing it is a runtime error.
        assert!(eval("return os.time()").error.is_some());
        assert!(eval("io.write('x')").error.is_some());
        assert!(eval("require('socket')").error.is_some());
    }

    #[test]
    fn timeout_aborts_infinite_loop() {
        let mut e = LuaEvaluator { timeout: Duration::from_millis(50), output_cap: 1024, ..LuaEvaluator::default() };
        let r = e.eval(&EvalRequest { lang: "lua", source: "while true do end", doc_path: None });
        assert!(r.error.as_deref().unwrap_or("").contains("timed out"));
    }

    #[test]
    fn output_is_capped() {
        let mut e = LuaEvaluator { timeout: Duration::from_secs(2), output_cap: 64, ..LuaEvaluator::default() };
        let r = e.eval(&EvalRequest {
            lang: "lua",
            source: "for i=1,1000 do print('xxxxxxxx') end",
            doc_path: None,
        });
        assert!(r.output.len() < 200);
        assert!(r.output.contains("truncated"));
    }

    #[test]
    fn long_c_call_cannot_freeze_the_editor() {
        let mut ev = LuaEvaluator { timeout: Duration::from_millis(100), ..LuaEvaluator::default() };
        let start = Instant::now();
        let r = ev.eval(&EvalRequest {
            lang: "lua",
            source: "return string.find(string.rep('a', 3000), string.rep('a*', 6) .. 'b')",
            doc_path: None,
        });
        assert!(start.elapsed() < Duration::from_secs(2), "eval blocked for {:?}", start.elapsed());
        assert_eq!(r.error.as_deref(), Some("timed out"));
    }

    #[test]
    fn memory_is_capped() {
        let r = eval("local t = {} for i = 1, 64 do t[i] = string.rep('x', 2^24 + i) end return #t");
        assert!(r.error.is_some(), "expected an out-of-memory error, got {:?}", r.output);
    }
}
