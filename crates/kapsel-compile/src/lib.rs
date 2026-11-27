//! Plan to WASM component compiler (REQ-KS-020, DD-KS-005).
//!
//! Owner: compiler workstream. Emits a component of world `kapsel:shell/plan`
//! (`wit/kapsel-shell.wit`). The same plan always gives byte-identical output.

use kapsel_plan::Plan;

/// The WIT the compiled component targets, embedded so host and compiler use
/// the same text.
pub const SHELL_WIT: &str = include_str!("../../../wit/kapsel-shell.wit");

#[derive(Debug)]
pub struct CompileError(pub String);

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CompileError {}

/// Compiles a checked plan into component bytes.
pub fn compile(plan: &Plan) -> Result<Vec<u8>, CompileError> {
    let _ = plan;
    Err(CompileError("compiler not implemented yet".into()))
}
