//! Dynamic documents: running embedded code blocks and splicing their output
//! back into the document. This module is the language-agnostic core — block
//! parsing ([`block`]) and the [`Evaluator`] capability — plus the built-in
//! sandboxed Lua backend ([`lua`]). Executing document code is gated by
//! per-folder trust in the editor layer; an evaluator itself is pure compute.

pub mod block;
pub mod lua;

use std::path::Path;

/// A request to evaluate one code block.
pub struct EvalRequest<'a> {
    pub lang: &'a str,
    pub source: &'a str,
    pub doc_path: Option<&'a Path>,
}

/// The result of an evaluation: combined output (captured prints plus the
/// block's return value) and an optional error message.
pub struct EvalResult {
    pub output: String,
    pub error: Option<String>,
}

/// A backend that can run code for one or more languages. Registered in the
/// editor's evaluator table keyed by language, mirroring command registration.
pub trait Evaluator {
    fn languages(&self) -> &[&str];
    fn eval(&mut self, req: &EvalRequest) -> EvalResult;
}
