//! A sandboxed Lua evaluator (mlua, vendored Lua 5.4). The interpreter starts
//! with the dangerous standard libraries removed (no `os`, `io`, `package`,
//! `require`, `load*`, `debug`), `print` redirected into a capture buffer, a
//! wall-clock timeout enforced via an instruction hook, and a cap on captured
//! output. It has no filesystem, network, or process access.

use super::{EvalRequest, EvalResult, Evaluator};
use mlua::{HookTriggers, Lua, MultiValue, Value, VmState};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

pub struct LuaEvaluator {
    timeout: Duration,
    output_cap: usize,
}

impl Default for LuaEvaluator {
    fn default() -> Self {
        LuaEvaluator { timeout: Duration::from_millis(2000), output_cap: 64 * 1024 }
    }
}

impl Evaluator for LuaEvaluator {
    fn handles(&self, lang: &str) -> bool {
        lang == "lua"
    }

    fn eval(&mut self, req: &EvalRequest) -> EvalResult {
        match run(req.source, self.timeout, self.output_cap) {
            Ok(output) => EvalResult { output, error: None },
            Err((output, error)) => EvalResult { output, error: Some(error) },
        }
    }
}

/// Run `source`, returning the combined output, or `(partial_output, error)`.
fn run(source: &str, timeout: Duration, cap: usize) -> Result<String, (String, String)> {
    let lua = Lua::new();
    let captured = Rc::new(RefCell::new(String::new()));

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
        let mut e = LuaEvaluator { timeout: Duration::from_millis(50), output_cap: 1024 };
        let r = e.eval(&EvalRequest { lang: "lua", source: "while true do end", doc_path: None });
        assert!(r.error.as_deref().unwrap_or("").contains("timed out"));
    }

    #[test]
    fn output_is_capped() {
        let mut e = LuaEvaluator { timeout: Duration::from_secs(2), output_cap: 64 };
        let r = e.eval(&EvalRequest {
            lang: "lua",
            source: "for i=1,1000 do print('xxxxxxxx') end",
            doc_path: None,
        });
        assert!(r.output.len() < 200);
        assert!(r.output.contains("truncated"));
    }
}
