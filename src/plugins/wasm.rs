//! A wasmi-backed plugin host. A `.wasm` module that follows the DOE plugin
//! ABI is loaded into a sandbox and bridged to the same [`Plugin`] trait the
//! built-in Rust plugins use, so the registry treats WASM and native plugins
//! identically.
//!
//! ## ABI
//!
//! The guest module must export:
//! - `memory`
//! - `alloc(i32) -> i32` — reserve `n` bytes, return the offset
//! - `dealloc(i32, i32)` — free a previously-alloc'd `(ptr, len)`
//!
//! and may export any of these entry points. Strings cross the boundary as a
//! packed `i64` return value: `(ptr as u64) << 32 | (len as u64)`; the host
//! reads the bytes out of `memory` and then calls `dealloc`. Inputs are written
//! into guest memory via `alloc` and passed as `(ptr, len)` pairs.
//! - `doe_name() -> i64` — UTF-8 plugin name
//! - `doe_status(ptr, len) -> i64` — given a JSON [`ViewJson`], a status string
//!   (empty = no segment)
//! - `doe_on_event(ptr, len)` — given a JSON [`EventJson`]
//! - `doe_commands() -> i64` — a JSON array of `[alias, command]` pairs
//!
//! The sandbox has no WASI and no host imports beyond an optional `env.doe_log`,
//! so a plugin can compute and react but cannot touch the filesystem, network
//! or editor internals it was not handed.

use super::api::{Event, Plugin, PluginView};
use serde::Serialize;
use std::cell::RefCell;
use wasmi::{Caller, Engine, Instance, Linker, Memory, Module, Store, TypedFunc};

/// Host-side state available to host functions (currently just captured logs).
#[derive(Default)]
pub struct HostState {
    logs: Vec<String>,
}

/// Scalar editor view handed to a plugin (deliberately *not* the buffer text —
/// that would be costly to serialise every frame; content access can come later
/// via host functions).
#[derive(Serialize)]
struct ViewJson<'a> {
    line: usize,
    col: usize,
    language: &'a str,
    path: Option<&'a str>,
    selection: Option<[usize; 2]>,
}

#[derive(Serialize)]
struct EventJson {
    kind: &'static str,
    path: Option<String>,
    command: Option<String>,
}

type Func0I64 = TypedFunc<(), i64>;
type FuncStrI64 = TypedFunc<(i32, i32), i64>;
type FuncStrUnit = TypedFunc<(i32, i32), ()>;

pub struct WasmPlugin {
    name: String,
    store: RefCell<Store<HostState>>,
    memory: Memory,
    alloc: TypedFunc<i32, i32>,
    dealloc: FuncStrUnit,
    f_status: Option<FuncStrI64>,
    f_on_event: Option<FuncStrUnit>,
    f_commands: Option<Func0I64>,
    /// Kept alive for the lifetime of the plugin (exports borrow from it).
    _instance: Instance,
}

impl WasmPlugin {
    /// Load and instantiate a plugin from `.wasm` bytes. Returns an error string
    /// if the module is invalid or is missing the required ABI exports.
    pub fn load(file_name: &str, wasm: &[u8]) -> Result<WasmPlugin, String> {
        let engine = Engine::default();
        let module = Module::new(&engine, wasm).map_err(|e| format!("parse: {e}"))?;
        let mut store = Store::new(&engine, HostState::default());

        let mut linker = Linker::<HostState>::new(&engine);
        // Optional logging host function; ignored if the module doesn't import it.
        linker
            .func_wrap("env", "doe_log", |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                if let Some(s) = read_caller_str(&mut caller, ptr, len) {
                    caller.data_mut().logs.push(s);
                }
            })
            .map_err(|e| format!("linker: {e}"))?;

        let instance = linker
            .instantiate_and_start(&mut store, &module)
            .map_err(|e| format!("instantiate: {e}"))?;

        let memory = instance
            .get_memory(&store, "memory")
            .ok_or("missing `memory` export")?;
        let alloc = instance
            .get_typed_func::<i32, i32>(&store, "alloc")
            .map_err(|_| "missing `alloc` export".to_string())?;
        let dealloc = instance
            .get_typed_func::<(i32, i32), ()>(&store, "dealloc")
            .map_err(|_| "missing `dealloc` export".to_string())?;

        let f_name = instance.get_typed_func::<(), i64>(&store, "doe_name").ok();
        let f_status = instance.get_typed_func::<(i32, i32), i64>(&store, "doe_status").ok();
        let f_on_event = instance.get_typed_func::<(i32, i32), ()>(&store, "doe_on_event").ok();
        let f_commands = instance.get_typed_func::<(), i64>(&store, "doe_commands").ok();

        // Resolve the name up front (fall back to the file stem).
        let name = f_name
            .and_then(|f| f.call(&mut store, ()).ok())
            .map(|packed| read_packed(&memory, &mut store, &dealloc, packed))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| file_name.trim_end_matches(".wasm").to_string());

        Ok(WasmPlugin {
            name,
            store: RefCell::new(store),
            memory,
            alloc,
            dealloc,
            f_status,
            f_on_event,
            f_commands,
            _instance: instance,
        })
    }

    /// Write `s` into guest memory, returning `(ptr, len)`. Caller must dealloc.
    fn write_str(&self, store: &mut Store<HostState>, s: &str) -> Option<(i32, i32)> {
        let len = s.len() as i32;
        let ptr = self.alloc.call(&mut *store, len).ok()?;
        self.memory.write(&mut *store, ptr as usize, s.as_bytes()).ok()?;
        Some((ptr, len))
    }
}

/// Read a packed `(ptr<<32|len)` string out of guest memory and free it.
fn read_packed(memory: &Memory, store: &mut Store<HostState>, dealloc: &FuncStrUnit, packed: i64) -> String {
    let p = packed as u64;
    let ptr = (p >> 32) as usize;
    let len = (p & 0xffff_ffff) as usize;
    if len == 0 {
        return String::new();
    }
    let mut buf = vec![0u8; len];
    let out = match memory.read(&*store, ptr, &mut buf) {
        Ok(()) => String::from_utf8_lossy(&buf).into_owned(),
        Err(_) => String::new(),
    };
    let _ = dealloc.call(&mut *store, (ptr as i32, len as i32));
    out
}

/// Read a string argument out of a host-function caller's memory.
fn read_caller_str(caller: &mut Caller<'_, HostState>, ptr: i32, len: i32) -> Option<String> {
    let memory = caller.get_export("memory")?.into_memory()?;
    let mut buf = vec![0u8; len.max(0) as usize];
    memory.read(&*caller, ptr as usize, &mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

impl Plugin for WasmPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn on_event(&mut self, event: &Event) {
        let Some(func) = self.f_on_event else { return };
        let json = serde_json::to_string(&event_json(event)).unwrap_or_default();
        let mut store = self.store.borrow_mut();
        if let Some((ptr, len)) = self.write_str(&mut store, &json) {
            let _ = func.call(&mut *store, (ptr, len));
            let _ = self.dealloc.call(&mut *store, (ptr, len));
        }
    }

    fn status_segment(&self, view: &PluginView) -> Option<String> {
        let func = self.f_status?;
        let vj = ViewJson {
            line: view.cursor_line,
            col: view.cursor_col,
            language: view.language,
            path: view.path.and_then(|p| p.to_str()),
            selection: view.selection.map(|(s, e)| [s, e]),
        };
        let json = serde_json::to_string(&vj).ok()?;
        let mut store = self.store.borrow_mut();
        let (ptr, len) = self.write_str(&mut store, &json)?;
        let packed = func.call(&mut *store, (ptr, len)).ok();
        let _ = self.dealloc.call(&mut *store, (ptr, len));
        let out = read_packed(&self.memory, &mut store, &self.dealloc, packed?);
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }

    fn commands(&self) -> Vec<(String, String)> {
        let Some(func) = self.f_commands else { return Vec::new() };
        let mut store = self.store.borrow_mut();
        let Ok(packed) = func.call(&mut *store, ()) else { return Vec::new() };
        let json = read_packed(&self.memory, &mut store, &self.dealloc, packed);
        serde_json::from_str::<Vec<(String, String)>>(&json).unwrap_or_default()
    }
}

fn event_json(event: &Event) -> EventJson {
    match event {
        Event::OpenFile(p) => EventJson { kind: "open_file", path: Some(p.display().to_string()), command: None },
        Event::SaveFile(p) => EventJson { kind: "save_file", path: Some(p.display().to_string()), command: None },
        Event::BufferChange => EventJson { kind: "buffer_change", path: None, command: None },
        Event::CursorMove => EventJson { kind: "cursor_move", path: None, command: None },
        Event::Command(c) => EventJson { kind: "command", path: None, command: Some(c.clone()) },
        Event::Exit => EventJson { kind: "exit", path: None, command: None },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A minimal ABI-conformant module: a bump allocator plus static name/status/
    // commands strings returned as packed pointers.
    const WAT: &str = r#"
        (module
          (memory (export "memory") 1)
          (global $heap (mut i32) (i32.const 1024))
          (data (i32.const 16) "wat-plugin")
          (data (i32.const 32) "OK")
          (data (i32.const 48) "[[\22hi\22,\22save\22]]")
          (func (export "alloc") (param i32) (result i32)
            (local $p i32)
            (local.set $p (global.get $heap))
            (global.set $heap (i32.add (global.get $heap) (local.get 0)))
            (local.get $p))
          (func (export "dealloc") (param i32 i32))
          (func (export "doe_name") (result i64)
            (i64.or (i64.shl (i64.const 16) (i64.const 32)) (i64.const 10)))
          (func (export "doe_status") (param i32 i32) (result i64)
            (i64.or (i64.shl (i64.const 32) (i64.const 32)) (i64.const 2)))
          (func (export "doe_commands") (result i64)
            (i64.or (i64.shl (i64.const 48) (i64.const 32)) (i64.const 15))))
    "#;

    fn load() -> WasmPlugin {
        let wasm = wat::parse_str(WAT).expect("wat compiles");
        WasmPlugin::load("test.wasm", &wasm).expect("loads")
    }

    #[test]
    fn name_and_commands_bridge() {
        let p = load();
        assert_eq!(p.name(), "wat-plugin");
        assert_eq!(p.commands(), vec![("hi".to_string(), "save".to_string())]);
    }

    #[test]
    fn status_segment_bridges() {
        use ropey::Rope;
        let p = load();
        let rope = Rope::from_str("hi");
        let view = PluginView {
            rope: &rope,
            cursor_line: 0,
            cursor_col: 0,
            selection: None,
            language: "markdown",
            path: None,
        };
        assert_eq!(p.status_segment(&view), Some("OK".to_string()));
    }

    #[test]
    fn missing_required_export_is_an_error() {
        let wasm = wat::parse_str("(module (memory (export \"memory\") 1))").unwrap();
        assert!(WasmPlugin::load("bad.wasm", &wasm).is_err());
    }
}
